//! LENSFUN-CALLER-37 / F2: the CLI's Lensfun corrector construction.
//!
//! Extracted from `main.rs` so the `lensfun` concern — the strict metadata
//! contract, the documented subject-distance default, and the process-lifetime
//! diagnostics sink — lives in one cohesive module instead of growing the
//! ratchet-baselined binary root.
//!
//! The *behaviour* is unchanged from the original `main.rs` function: `None`
//! unless every required EXIF field is present and finite, no loose profile
//! matching, and the database handle is returned alongside the corrector because
//! the modifier references lens data owned by the database.

use lumina_lensfun::db_sinks::ReportOnce;
use lumina_lensfun::{Corrector, LensfunDb};
use lumina_raw::RawMetadata;

/// Process-lifetime de-duplicating Lensfun diagnostics sink.
///
/// `build_lensfun_corrector` runs **per image**, so a batch on a machine without
/// a resolvable Lensfun database printed one unlevelled block per image. Holding
/// the sink for the whole process means the operator sees the failure **once**,
/// while the *lookup* still runs per image — a database installed while the CLI is
/// running is picked up, because a miss is never cached.
///
/// The sink is handed to `f` rather than returned: a `MutexGuard` cannot outlive
/// its guard, so a `&'static mut` return type would be a lie.
pub(crate) fn with_diagnostics<R>(f: impl FnOnce(&mut ReportOnce) -> R) -> R {
    use std::sync::{Mutex, OnceLock};
    static SINK: OnceLock<Mutex<ReportOnce>> = OnceLock::new();
    let mut guard = SINK
        .get_or_init(|| Mutex::new(ReportOnce::new()))
        .lock()
        // `ReportOnce` keeps its own `Mutex<BTreeSet<_>>` and swallows poisoning,
        // so a poisoned lock here can only mean a previous holder panicked
        // mid-load. Recovering keeps the diagnostic path alive rather than
        // turning it into a second, silent failure.
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

/// Build the lens corrector for one image, or `None` if no strict match exists.
///
/// # Strict, documented fallback (no silent correction)
/// `None` is returned unless the `lensfun` feature is enabled **and** all of
/// `camera_make`, `camera_model`, `focal_length` and `aperture` are present and
/// finite. When the system Lensfun database cannot be loaded, or no matching,
/// non-identity profile is found, `None` is returned and the manual LuminaRust
/// model (or identity) applies instead — never a guessed correction.
///
/// # Lens identification (G-06 EXIF-Erkennung, GUI-ROUTING-N6)
/// `RawMetadata.lens` (REVIEW-RAW-N2) is passed as the Lensfun lens name. A
/// **named** lens must exist in the database (strict, no `LF_SEARCH_LOOSE`);
/// otherwise `None` and the manual model apply. Loose matching fabricated
/// profiles (`EOS R1`→`EOS R`, `RF200-800mm`→`RF 24-240mm`) and applied wrong
/// corrections — forbidden ("nie ein geratenes Profil"). Without a lens name the
/// documented body/mount fallback applies; `--lensfun-status` and the render
/// `info!` log name the match explicitly.
///
/// # Subject (focus) distance
/// `RawMetadata` carries no subject-distance field, so a documented default of
/// `10.0` (metres) is used. Lensfun vignetting/distortion calibration is in
/// practice focus-distance-independent for the MVP profiles, and `lumina-lensfun`'s
/// own reference tests use exactly this value, so it yields a matching,
/// non-identity corrector for the `Nikon D40` example profile.
///
/// # Known limits
/// * The system Lensfun database is **looked up** once per call (no cross-render
///   cache of the database itself); only the *diagnostics* are de-duplicated for
///   the process lifetime. Acceptable for the MVP, but repeated
///   `process`/`render` invocations each re-load the DB.
/// * Manual `ca_red`/`ca_blue` are skipped when the built corrector carries
///   TCA calibration (G-06: TCA is corrected geometrically in the lens
///   stage; see `lumina-core` `apply_lens`).
///
/// The returned `(LensfunDb, Corrector)` keeps the database handle alive as long
/// as the corrector is used: the modifier internally references lens data owned
/// by the database, so the database must not be dropped before the corrector.
#[cfg(feature = "lensfun")]
pub(crate) fn build_lensfun_corrector(
    metadata: Option<&RawMetadata>,
) -> Option<(LensfunDb, Corrector)> {
    let metadata = metadata?;
    let make = metadata.camera_make.as_deref()?;
    let model = metadata.camera_model.as_deref()?;
    // Finite focal length and aperture are required; a missing/NaN value means
    // we cannot build a meaningful corrector → fall back to `None` strictly.
    let focal_length = metadata.focal_length.filter(|value| value.is_finite())?;
    let aperture = metadata.aperture.filter(|value| value.is_finite())?;
    let db = with_diagnostics(LensfunDb::load_system_with)?;
    // `RawMetadata` has no subject distance, so use the documented 10.0 m default.
    let distance = 10.0_f32;
    let corrector = db.for_camera(
        make,
        model,
        metadata.lens.as_deref(),
        metadata.width,
        metadata.height,
        focal_length,
        aperture,
        distance,
    )?;
    Some((db, corrector))
}
