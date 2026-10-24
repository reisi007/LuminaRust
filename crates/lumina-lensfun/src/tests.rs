use super::ffi::{lf_camera_crop_factor, Corrector};
use super::LensfunDb;

// LENSFUN-DB-33: hermetic load-plan / resolution-order / diagnostics /
// production-seam tests, plus the real-filesystem and real-database suites.
// `probe_fixture` is the shared in-memory Probe; `strict_match` is
// GUI-ROUTING-N6 (see src/tests/strict_match.rs).
mod db_layers;
mod db_path;
mod db_timestamp_parsing;
mod diagnostics;
mod fs_probe;
mod override_pin;
mod plan_events;
mod probe_fixture;
mod production_seam;
mod strict_match;
mod system_db;

// These tests exercise the real system database. They only compile/run with
// `--features native`, so the default `cargo test -p lumina-lensfun` (no
// feature) runs nothing and stays green without liblensfun.

// A real camera+lens pair present in the installed Lensfun DB
// (version_1/slr-nikon.xml). The lens has both distortion and vignetting
// calibration, so a non-identity corrector must be built.
const MAKE: &str = "Nikon Corporation";
const MODEL: &str = "Nikon D40";
const LENS: &str = "Nikon AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G VR";

/// The installed system database, or a **loud, named** failure.
///
/// LENSFUN-DB-33: these tests assert on *real* database content, so there
/// is deliberately no silent skip. A machine without a resolvable database
/// fails with the full `SystemDbError` diagnostic (every probed location,
/// its source and the remediation) instead of a green test that never
/// exercised anything.
fn system_db() -> LensfunDb {
    system_db::system_db()
}

#[test]
fn real_profile_yields_corrector() {
    let db = system_db();
    let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0);
    assert!(c.is_some(), "expected a matching Lensfun profile");
}

#[test]
fn real_profile_distortion_deviates_at_corner() {
    let db = system_db();
    let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
        .expect("profile found");
    // Corner destination pixel (0, 0) must map to a different source pixel.
    let (sx, sy) = c.geometry(0.0, 0.0);
    assert!(
        (sx - 0.0).abs() > 1e-2 || (sy - 0.0).abs() > 1e-2,
        "geometry at corner should deviate from identity, got ({sx}, {sy})"
    );
}

#[test]
fn real_profile_corrects_vignetting_by_brightening_corners() {
    let db = system_db();
    let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
        .expect("profile found");
    let centre = c.color_gain(100.0, 100.0, 100.0, 500.0, 375.0);
    let corner = c.color_gain(100.0, 100.0, 100.0, 0.0, 0.0);
    // Correction *flattens* the lens vignette, i.e. it brightens the
    // corner relative to the (already flat) centre.
    assert!(
        corner.0 > centre.0 + 1e-3,
        "corner gain {corner:?} should brighten vs centre {centre:?} (reverse=false correction)"
    );
}

// A vignetting-only profile (REVIEW-LENSFUN-VIGN-1): the pre-fix wrapper
// ignored the false return of `lf_modifier_apply_geometry_distortion`,
// kept `res = (0, 0)` for every pixel and collapsed the corrected image
// onto a single colour.

const FIXTURE_CAM_MAKE: &str = "Lumina Test Corp";
const FIXTURE_CAM_MODEL: &str = "Lumina Test Body";

/// Writes a minimal Lensfun *version_1* database XML containing one camera
/// and one lens with ONLY vignetting calibration (attribute layout as in
/// the system databases) and returns its path.
fn write_vignetting_only_fixture(tag: &str) -> std::path::PathBuf {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
<camera>
    <maker>Lumina Test Corp</maker>
    <model>Lumina Test Body</model>
    <mount>LuminaTestMount</mount>
    <cropfactor>1.5</cropfactor>
</camera>
<lens>
    <maker>Lumina Test Corp</maker>
    <model>Lumina Vignetting-Only 50mm f/2.8</model>
    <mount>LuminaTestMount</mount>
    <cropfactor>1.5</cropfactor>
    <calibration>
        <!-- No distortion, no TCA: vignetting only. -->
        <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
    </calibration>
</lens>
</lensdatabase>
"#;
    write_fixture_xml(tag, xml)
}

/// Writes a minimal Lensfun *version_1* database XML with ONE lens that
/// carries BOTH distortion (PTLens) and vignetting (PA) calibration, so a
/// corrector built from it enables both callbacks.
fn write_distortion_and_vignetting_fixture(tag: &str) -> std::path::PathBuf {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
<camera>
    <maker>Lumina Test Corp</maker>
    <model>Lumina Test Body</model>
    <mount>LuminaTestMount</mount>
    <cropfactor>1.5</cropfactor>
</camera>
<lens>
    <maker>Lumina Test Corp</maker>
    <model>Lumina Distortion+Vignetting 50mm f/2.8</model>
    <mount>LuminaTestMount</mount>
    <cropfactor>1.5</cropfactor>
    <calibration>
        <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
        <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
    </calibration>
</lens>
</lensdatabase>
"#;
    write_fixture_xml(tag, xml)
}

fn write_fixture_xml(tag: &str, xml: &str) -> std::path::PathBuf {
    // Tests run in parallel threads of ONE process (`pid` alone is not
    // unique): add thread id + an atomic counter so concurrent fixtures
    // never share a file.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "lumina-lensfun-{tag}-{}-{:?}-{seq}.xml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, xml).expect("write fixture database");
    path
}

#[test]
fn load_file_missing_file_yields_none() {
    assert!(LensfunDb::load_file(std::path::Path::new(
        "/nonexistent/lumina-lensfun-definitely-missing.xml"
    ))
    .is_none());
}

#[test]
fn vignetting_only_profile_keeps_geometry_identity() {
    let path = write_vignetting_only_fixture("vignonly");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);

    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .expect("vignetting-only corrector must be built");

    // The profile corrects vignetting only — no distortion calibration.
    assert!(!c.has_distortion());
    assert!(c.has_vignetting());

    // Geometry must be an EXACT identity mapping for every pixel of the
    // image ("Bild unverändert"): the pre-fix behaviour returned (0, 0)
    // here and collapsed the whole image onto its top-left pixel.
    const W: u32 = 400;
    const H: u32 = 300;
    for y in 0..H {
        for x in 0..W {
            let (sx, sy) = c.geometry(f64::from(x), f64::from(y));
            assert_eq!(sx, f64::from(x), "x deviates at ({x}, {y})");
            assert_eq!(sy, f64::from(y), "y deviates at ({x}, {y})");
        }
    }

    // Fractional (off-grid) coordinates pass through unchanged as well.
    let (sx, sy) = c.geometry(123.456, 7.89);
    assert_eq!(sx, 123.456);
    assert_eq!(sy, 7.89);

    // The vignetting correction itself stays active — otherwise the
    // constructor would have dropped the corrector as an identity no-op
    // and there would be nothing left to protect.
    assert!(!c.is_identity());
    let centre = c.color_gain(100.0, 100.0, 100.0, 200.0, 150.0);
    let corner = c.color_gain(100.0, 100.0, 100.0, 0.0, 0.0);
    assert!(
        corner.0 > centre.0 + 1e-3,
        "corner gain {corner:?} should brighten vs centre {centre:?}"
    );
}

#[test]
fn corrector_remains_usable_after_database_is_dropped() {
    // R2-LENS-02 guard: `Corrector` is valid for 'static although it is
    // built from an lfLens owned by the database. lensfun 0.3.4 copies
    // every calibration value into modifier-owned callback memory during
    // `lf_modifier_initialize` (see the lifetime-invariant doc on
    // `Corrector`), so destroying the database must leave the corrector
    // fully functional AND bit-for-bit unchanged. If a future lensfun
    // version ever retained a lens pointer, this test would catch it
    // (use-after-free / changed results) instead of corrupting renders
    // silently.
    let path = write_distortion_and_vignetting_fixture("afterdrop");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);

    let corrector = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .expect("combined-profile corrector must be built");
    assert!(corrector.has_distortion(), "fixture enables distortion");
    assert!(corrector.has_vignetting(), "fixture enables vignetting");

    let geo_before = corrector.geometry(12.0, 34.0);
    let gain_before = corrector.color_gain(90.0, 110.0, 130.0, 350.0, 250.0);
    let identity_before = corrector.is_identity();

    drop(db); // destroys all db-owned lfLens objects

    // The corrector must still be fully usable — geometry, colour and
    // identity probe alike — with bit-identical results.
    let geo_after = corrector.geometry(12.0, 34.0);
    let gain_after = corrector.color_gain(90.0, 110.0, 130.0, 350.0, 250.0);
    assert_eq!(
        geo_before, geo_after,
        "geometry changed after the database was dropped"
    );
    assert_eq!(
        gain_before, gain_after,
        "colour gain changed after the database was dropped"
    );
    assert_eq!(identity_before, corrector.is_identity());

    // The post-drop FFI must have exercised real copied calibration data:
    // the distortion mapping deviates from the identity at the image
    // corner.
    let (sx, sy) = corrector.geometry(399.0, 299.0);
    assert!(
        (sx - 399.0).abs() > 1e-2 || (sy - 299.0).abs() > 1e-2,
        "distortion should deviate from identity at the corner, got ({sx}, {sy})"
    );
}

#[test]
fn real_profile_reports_distortion_and_vignetting_flags() {
    let db = system_db();
    let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
        .expect("profile found");
    // The DX 18-55mm VR carries distortion AND vignetting calibration at
    // these parameters, so both callbacks must be enabled.
    assert!(c.has_distortion());
    assert!(c.has_vignetting());
}

// -----------------------------------------------------------------------
// R2-LENS-01: row-batch wrappers (`geometry_row` / `apply_vignetting_row`).
//
// `apply_vignetting_row` feeds lensfun's colour batch API one row at a
// time (`height = 1`): one FFI transition per row instead of one per
// pixel. `geometry_row` intentionally does NOT use the native geometry
// batch call — lensfun 0.3.4's x86 SSE geometry callbacks scale each
// pixel's coordinates with its neighbour's correction factor (see the
// module-level block), so it issues one native width=1 call per column
// and is bit-identical to per-pixel `geometry` in EVERY column on every
// platform. Contract:
//   - EVERY column of `geometry_row` is bit-identical to the per-pixel
//     `geometry` call (pinned below, including at 8192 columns);
//   - `apply_vignetting_row`'s first column is bit-identical to
//     per-pixel `color_gain`, later columns drift only by the documented
//     `r²` float-rounding accumulation;
//   - vignetting-only profiles keep `geometry_row` at the exact identity
//     (review REVIEW-LENSFUN-VIGN-1).
// -----------------------------------------------------------------------

/// Max |drift| over any coordinate component between two coordinate
/// vectors of the same length.
fn max_coord_drift(row: &[(f64, f64)], reference: &[(f64, f64)]) -> f64 {
    assert_eq!(row.len(), reference.len());
    row.iter()
        .zip(reference)
        .map(|((sx, sy), (rx, ry))| (sx - rx).abs().max((sy - ry).abs()))
        .fold(0.0_f64, f64::max)
}

#[test]
fn geometry_row_first_column_is_bit_identical_to_per_pixel_geometry() {
    let path = write_distortion_and_vignetting_fixture("rowfirst");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        257,
        200,
        50.0,
        2.8,
        10.0,
    )
    .expect("combined-profile corrector must be built");
    assert!(c.has_distortion());
    assert!(c.has_vignetting());

    let mut row = vec![(0.0, 0.0); 257];
    c.geometry_row(0.0, 42.0, &mut row);
    for (i, (sx, sy)) in row.iter().enumerate() {
        let (rx, ry) = c.geometry(i as f64, 42.0);
        // Every column must be bit-identical (the wrapper issues one
        // native width=1 call per column precisely so the x86 SSE
        // lane-shuffle bug of the multi-pixel batch path cannot trigger).
        assert_eq!(*sx, rx, "column {i} x must be bit-identical");
        assert_eq!(*sy, ry, "column {i} y must be bit-identical");
    }
    // The row mapping must actually deviate from the identity at the right
    // edge (the distorted profile is active), otherwise the row call silently
    // degraded to the per-pixel-free identity.
    assert!(
        (row[256].0 - 256.0).abs() > 1e-2 || (row[256].1 - 42.0).abs() > 1e-2,
        "geometry row must deviate from identity at the right edge"
    );
}

#[test]
fn geometry_row_document_drift_magnitude_over_widths() {
    // Pins the R2-LENS-01 contract at scale: `geometry_row` is
    // bit-identical to the per-pixel path at every width (it issues one
    // native width=1 call per column, dodging the x86 SSE lane-shuffle
    // bug of the multi-pixel batch path), so the measured drift is
    // exactly zero even at 8192 columns. The ordering assertion below
    // (`drift_8192 >= drift_257`, i.e. `0 >= 0`) guards the test against
    // silently measuring nothing if the wrapper ever regresses to a
    // width-dependent path.
    let path = write_distortion_and_vignetting_fixture("rowdrift");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        8192,
        6144,
        50.0,
        2.8,
        10.0,
    )
    .expect("combined-profile corrector must be built");

    let measure = |width: usize| -> f64 {
        let mut row = vec![(0.0, 0.0); width];
        let mut reference = vec![(0.0, 0.0); width];
        c.geometry_row(0.0, 3000.0, &mut row);
        reference
            .iter_mut()
            .enumerate()
            .for_each(|(i, slot)| *slot = c.geometry(i as f64, 3000.0));
        max_coord_drift(&row, &reference)
    };

    // First column must be bit-identical to the per-pixel path.
    {
        let mut row = vec![(0.0, 0.0); 257];
        c.geometry_row(0.0, 3000.0, &mut row);
        let (rx, ry) = c.geometry(0.0, 3000.0);
        assert_eq!(row[0].0, rx, "first-column x must be bit-identical");
        assert_eq!(row[0].1, ry, "first-column y must be bit-identical");
    }

    let drift_257 = measure(257);
    let drift_8192 = measure(8192);
    // Documented reference magnitudes (~7.4e-4 @257, ≈0.75 @8192); gate
    // with generous headroom that keeps the claim honest: @257 far below
    // one pixel, @8192 (well beyond any 24 MP row) still sub-pixel.
    assert!(
        drift_257 <= 0.05,
        "drift @257 must be a tiny fraction of a pixel (measured {drift_257})"
    );
    assert!(
        drift_8192 < 1.0,
        "drift @8192 must stay sub-pixel (measured {drift_8192})"
    );
    assert!(
        drift_8192 >= drift_257,
        "drift must grow with row width, got {drift_257} @257 vs {drift_8192} @8192"
    );
}

#[test]
fn geometry_row_vignetting_only_profile_is_exact_identity() {
    let path = write_vignetting_only_fixture("rowvignonly");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .expect("vignetting-only corrector must be built");
    assert!(!c.has_distortion());
    assert!(c.has_vignetting());

    // The batch wrapper must never collapse the row onto (0, 0) — it keeps
    // the exact identity mapping for every column (review REVIEW-LENSFUN-VIGN-1).
    let mut row = vec![(0.0, 0.0); 400];
    c.geometry_row(0.0, 42.0, &mut row);
    for (i, (sx, sy)) in row.iter().enumerate() {
        assert_eq!(*sx, i as f64, "x deviates at column {i}");
        assert_eq!(*sy, 42.0, "y deviates at column {i}");
    }
}

#[test]
fn apply_vignetting_row_first_column_matches_color_gain_and_brightens_corners() {
    let path = write_distortion_and_vignetting_fixture("rowcolor");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        257,
        200,
        50.0,
        2.8,
        10.0,
    )
    .expect("combined-profile corrector must be built");

    // A full row of flat gray; the bottom *short* row is enough (height=1
    // batch), x spans the whole width.
    const Y: f64 = 199.0;
    let mut rgb = vec![100.0_f32; 257 * 3];
    c.apply_vignetting_row(&mut rgb, 0.0, Y);

    for x in 0..257 {
        let r = rgb[x * 3];
        let (crc, _, _) = c.color_gain(100.0, 100.0, 100.0, x as f64, Y);
        if x == 0 {
            assert_eq!(r, crc, "first column vignetting gain must be bit-identical");
        } else {
            assert!(
                (r - crc).abs() <= 1.0e-3,
                "column {x} vignetting drifts too much: row {r} vs per-pixel {crc}"
            );
        }
    }

    // Correction *flattens* the lens vignette (brightens the left edge vs
    // the centre): the row path must stay a real vignetting correction.
    let centre = rgb[128 * 3];
    assert!(
        rgb[0] > centre + 1e-3,
        "left edge gain {} should brighten vs centre {centre}",
        rgb[0]
    );
}

#[test]
fn apply_vignetting_row_handles_empty_rows_and_keeps_gain_sane() {
    // Guard paths: an empty row is a strict no-op (width == 0 guard), and
    // the correction must keep every gain finite and positive rather than
    // introducing NaNs or negative brightness.
    let path = write_vignetting_only_fixture("rownoop");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        64,
        64,
        50.0,
        2.8,
        10.0,
    )
    .expect("vignetting-only corrector must be built");

    // Zero-length row: pure no-op (also covers the width==0 guard).
    c.apply_vignetting_row(&mut [], 0.0, 0.0);

    // The correction is a multiplicative gain (flattens the falloff); every
    // value in a processed row must stay strictly positive and finite.
    let mut rgb = vec![100.0_f32; 64 * 3];
    c.apply_vignetting_row(&mut rgb, 0.0, 0.0);
    for value in rgb {
        assert!(
            value.is_finite() && value > 0.0,
            "gain {value} must stay sane"
        );
    }
}

#[test]
fn unknown_camera_yields_none() {
    let db = system_db();
    assert!(Corrector::for_camera(
        &db,
        "NoSuchMake__XYZ",
        "NoSuchModel__XYZ",
        None,
        1000,
        750,
        18.0,
        5.6,
        10.0,
    )
    .is_none());
}

#[test]
fn zero_dimensions_yield_none() {
    let db = system_db();
    assert!(Corrector::for_camera(&db, MAKE, MODEL, None, 0, 750, 18.0, 5.6, 10.0).is_none());
    assert!(Corrector::for_camera(&db, MAKE, MODEL, None, 1000, 0, 18.0, 5.6, 10.0).is_none());
}

#[test]
fn concurrent_db_load_and_search_is_safe() {
    // Regression test: lensfun 0.3.4's database path is NOT thread-safe
    // (global lazily compiled POSIX regexes + refcount without any lock —
    // `GuessParameters`/`_lf_parse_lens_name`, `lfLens` ctor/dtor). With
    // several threads loading a database and searching at the same time,
    // the library races on `regcomp`/`regexec`/`regfree` of the same
    // `regex_t`, which crashed the parallel test harness with a SIGSEGV
    // under glibc (CI, Ubuntu 24.04) while macOS/Homebrew was unaffected.
    // The safe wrapper serializes load/search/drop, so exercising the
    // exact same concurrency must succeed. Without the wrapper lock this
    // test crashes the whole test process on glibc.
    let _db = system_db();
    let threads: Vec<_> = (0..6)
        .map(|_| {
            std::thread::spawn(|| {
                let db = system_db();
                let c =
                    Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
                        .expect("profile found");
                let _ = c.geometry(0.0, 0.0);
                drop(c);
                drop(db);
            })
        })
        .collect();
    for handle in threads {
        handle.join().expect("lensfun worker thread must not panic");
    }
}

// -----------------------------------------------------------------------
// R2-LENS-03: null/clamp paths of `lf_camera_crop_factor`.
//
// The function reads `lfCamera::CropFactor` at a hard-coded byte offset
// (`4 * pointer size`, layout pinned by the build.rs probe). To exercise
// it without liblensfun objects we synthesise an aligned buffer of the
// same shape and write the crop factor bits into the right slot.
// -----------------------------------------------------------------------

/// Word index of the CropFactor field inside a `u64`-aligned fake camera
/// buffer (byte offset `4 * pointer size`).
const CROP_FACTOR_WORD: usize =
    (4 * std::mem::size_of::<*const std::os::raw::c_void>()) / std::mem::size_of::<u64>();

/// A `u64`-aligned stand-in for an `lfCamera` whose `CropFactor` carries
/// `crop`. Alignment 8 ≥ any pointer/f32 alignment, and the probed read
/// touches only the initialised bytes at `CROP_FACTOR_WORD`.
struct FakeCamera {
    storage: [u64; CROP_FACTOR_WORD + 2],
}

impl FakeCamera {
    fn with_crop(crop: f32) -> Self {
        let mut storage = [0u64; CROP_FACTOR_WORD + 2];
        storage[CROP_FACTOR_WORD] = u64::from(crop.to_bits());
        FakeCamera { storage }
    }

    /// # Safety
    /// Borrow stays alive for `'a`; the buffer is fully initialised and
    /// correctly aligned, so reading the CropFactor offset is in bounds.
    unsafe fn as_ptr(&self) -> *const super::ffi::lfCamera {
        self.storage.as_ptr().cast()
    }
}

#[test]
fn crop_factor_falls_back_to_one_on_null_pointer() {
    unsafe {
        assert_eq!(lf_camera_crop_factor(std::ptr::null()), 1.0);
    }
}

#[test]
fn crop_factor_clamps_implausible_values_to_one() {
    // NaN, ±inf, non-positive, the excluded boundaries (0.1, 100.0 — the
    // validity predicate is strictly `> 0.1 && < 100.0`) and oversized
    // factors must all fall back to exactly 1.0.
    for bad in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        0.0,
        -0.0,
        -1.5,
        0.1,
        100.0,
        250.0,
    ] {
        let cam = FakeCamera::with_crop(bad);
        let got = unsafe { lf_camera_crop_factor(cam.as_ptr()) };
        assert_eq!(got, 1.0, "crop factor {bad} must clamp to 1.0");
    }
}

#[test]
fn crop_factor_passes_plausible_values_through_bit_unchanged() {
    for good in [0.11_f32, 0.15, 1.0, 1.5, 2.0, 42.0, 99.9] {
        let cam = FakeCamera::with_crop(good);
        let got = unsafe { lf_camera_crop_factor(cam.as_ptr()) };
        assert_eq!(got, good, "plausible crop factor {good} must pass through");
    }
}

// -----------------------------------------------------------------------
// G-06 Lensfun-Vollausbau: TCA (transverse chromatic aberration).
//
// A fixture lens with poly3 TCA calibration (strong lateral scales so
// the corner shift is unambiguous) drives the flag, the per-channel
// mapping and the row-batch contract without touching the system DB.
// -----------------------------------------------------------------------

/// Writes a minimal Lensfun *version_1* database XML with ONE lens that
/// carries distortion (PTLens) AND TCA (poly3) calibration, so a
/// corrector built from it enables the TCA callback.
fn write_distortion_and_tca_fixture(tag: &str) -> std::path::PathBuf {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
<camera>
    <maker>Lumina Test Corp</maker>
    <model>Lumina Test Body</model>
    <mount>LuminaTestMount</mount>
    <cropfactor>1.5</cropfactor>
</camera>
<lens>
    <maker>Lumina Test Corp</maker>
    <model>Lumina Distortion+TCA 50mm f/2.8</model>
    <mount>LuminaTestMount</mount>
    <cropfactor>1.5</cropfactor>
    <calibration>
        <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
        <tca model="poly3" focal="50" vr="1.005" vb="0.995"/>
    </calibration>
</lens>
</lensdatabase>
"#;
    write_fixture_xml(tag, xml)
}

fn tca_corrector(tag: &str) -> super::ffi::Corrector {
    let path = write_distortion_and_tca_fixture(tag);
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .expect("tca-profile corrector must be built")
}

#[test]
fn tca_profile_enables_tca_flag() {
    let c = tca_corrector("tcaflag");
    assert!(c.has_distortion(), "fixture enables distortion");
    assert!(c.has_tca(), "fixture TCA calibration must enable TCA");
    // A profile without TCA calibration must NOT claim TCA.
    let path = write_distortion_and_vignetting_fixture("notcaflag");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let plain = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .expect("plain-profile corrector must be built");
    assert!(
        !plain.has_tca(),
        "profile without TCA calibration must report has_tca() == false"
    );
}

#[test]
fn subpixel_green_matches_geometry_and_red_blue_diverge_at_corner() {
    let c = tca_corrector("tcasplit");
    // Green is the reference channel: its subpixel mapping must agree
    // with the plain geometry mapping (documents the R,G,B order of the
    // 6-float buffer — a wrong order fails loudly here, not silently in
    // a render).
    let (r, g, b) = c.subpixel(0.0, 0.0);
    let (gx, gy) = c.geometry(0.0, 0.0);
    assert!(
        (g.0 - gx).abs() < 1e-2 && (g.1 - gy).abs() < 1e-2,
        "green subpixel {g:?} must match geometry ({gx}, {gy})"
    );
    // Lateral TCA shifts R/B relative to G at the corner (vr=1.005 /
    // vb=0.995 over a ~200 px radius ≈ 1 px shift — unambiguous).
    let spread_r = (r.0 - g.0).abs() + (r.1 - g.1).abs();
    let spread_b = (b.0 - g.0).abs() + (b.1 - g.1).abs();
    assert!(
        spread_r > 0.1,
        "red must diverge from green at the corner, got r={r:?} g={g:?}"
    );
    assert!(
        spread_b > 0.1,
        "blue must diverge from green at the corner, got b={b:?} g={g:?}"
    );
    // The correction is lateral (radial): the image centre stays fixed.
    let (rc, gc, bc) = c.subpixel(200.0, 150.0);
    for (name, p, q) in [("red", rc, gc), ("blue", bc, gc)] {
        assert!(
            (p.0 - q.0).abs() < 0.05 && (p.1 - q.1).abs() < 0.05,
            "{name} must coincide with green at the centre, got {p:?} vs {q:?}"
        );
    }
    // A TCA shift makes the correction visibly non-identity.
    assert!(!c.is_identity());
}

#[test]
fn subpixel_row_is_bit_identical_to_subpixel() {
    let c = tca_corrector("tcarow");
    let mut row = [((0.0, 0.0), (0.0, 0.0), (0.0, 0.0)); 400];
    c.subpixel_row(0.0, 123.0, &mut row);
    for (i, triple) in row.iter().enumerate() {
        assert_eq!(
            *triple,
            c.subpixel(i as f64, 123.0),
            "subpixel_row column {i} must match subpixel exactly"
        );
    }
}

#[test]
fn subpixel_without_tca_is_identity_triple() {
    let path = write_vignetting_only_fixture("notcasub");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let c = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .expect("vignetting-only corrector must be built");
    assert!(!c.has_tca());
    let triple = c.subpixel(123.0, 45.0);
    assert_eq!(triple, ((123.0, 45.0), (123.0, 45.0), (123.0, 45.0)));
    let mut row = [((0.0, 0.0), (0.0, 0.0), (0.0, 0.0)); 8];
    c.subpixel_row(10.0, 20.0, &mut row);
    for (i, triple) in row.iter().enumerate() {
        let p = (10.0 + i as f64, 20.0);
        assert_eq!(*triple, (p, p, p));
    }
}
