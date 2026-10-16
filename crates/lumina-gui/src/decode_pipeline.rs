//! GUI-LIB-SPLIT-65 Slice 15: Werttypen der Hintergrund-Decode-Pipeline.
//!
//! Die Vertragstypen des asynchronen RAW-/Raster-Dekodierens: das
//! Worker-Ergebnis `DecodedFrame`, die EXIF-Objektiv-Identitaet
//! `LensIdentity` inklusive ihres Snapshot-Helpers, die
//! Verzeichnis-Oeffnungspolitik `DirectoryOpenPolicy` mit ihrem
//! `PendingDirectoryOpen`-Zustand sowie die Kanal-Tupel `DecodeResult`
//! und `DecodeRequestResult`. Extrahiert aus `lib.rs` (File-Size-Ratchet),
//! eine Verantwortlichkeit (Datenkontrakte zwischen Produzent, Kanal und
//! Verbraucher des Decoder-Threads, ohne Panel-, Rezept- oder
//! Sidecar-Logik), reine Struktur ohne Test- oder Verhaltensaenderung;
//! Sichtbarkeiten minimal auf `pub(crate)` erweitert (einige Strukturen
//! werden von Geschwister-Modulen konstruiert) und auf Modul-Ebene
//! re-exportiert.

use super::*;

/// PERF-GUI-7: result of a background RAW/raster decode. Carries the decoded
/// frame plus the metadata needed to apply it on the main thread (and to load
/// the matching sidecar). `Err` carries the (path, message) so the GUI can show
/// the decode failure without blocking the worker thread.
pub(crate) struct DecodedFrame {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) frame: ImageFrame,
    pub(crate) orientation: u8,
    pub(crate) camera_white_balance: Option<[f32; 4]>,
    pub(crate) source_is_raw: bool,
    /// EXIF lens identity for the Lensfun auto-profile resolution (G-06):
    /// populated for RAW sources from the decoder metadata, `None` for
    /// raster sources (no EXIF) and when the metadata is unusable. Best
    /// effort — never a decode failure.
    pub(crate) lens_identity: Option<LensIdentity>,
}

/// EXIF lens identity snapshot driving the Lensfun auto-profile resolution
/// (G-06, LRPAR-G06-GEO). Plain data (no lensfun types) so the decode path
/// stays feature-independent; the corrector is built from it at render
/// time behind the `lensfun` feature.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LensIdentity {
    pub(crate) camera_make: Option<String>,
    pub(crate) camera_model: Option<String>,
    pub(crate) lens: Option<String>,
    pub(crate) focal_length: Option<f32>,
    pub(crate) aperture: Option<f32>,
}

/// Snapshot helper: `None` when the metadata carries no lens-relevant EXIF
/// at all (raster-equivalent); otherwise the identity, even partial (the
/// corrector build enforces the required completeness strictly).
pub(crate) fn lens_identity_from_metadata(
    metadata: &lumina_raw::RawMetadata,
) -> Option<LensIdentity> {
    let identity = LensIdentity {
        camera_make: metadata.camera_make.clone(),
        camera_model: metadata.camera_model.clone(),
        lens: metadata.lens.clone(),
        focal_length: metadata.focal_length,
        aperture: metadata.aperture,
    };
    if identity.camera_make.is_none()
        && identity.camera_model.is_none()
        && identity.lens.is_none()
        && identity.focal_length.is_none()
        && identity.aperture.is_none()
    {
        None
    } else {
        Some(identity)
    }
}

/// Whether a background decode is allowed to adopt its target directory.
///
/// `None` is the ordinary lineage: `open_file` has already prepared the
/// directory, while direct `begin_load_path` callers (the Develop path field,
/// warmup, and tests) intentionally do not navigate. `Deferred` is reserved
/// for native path drops, which must not mutate library navigation until their
/// decode succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectoryOpenPolicy {
    None,
    Deferred,
}

/// A decoded cross-directory target whose neighbor window must wait for the
/// directory scan that will provide its authoritative listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingDirectoryOpen {
    pub(crate) directory: String,
    pub(crate) active_path: String,
    pub(crate) scan_generation: Option<u64>,
}

pub(crate) type DecodeResult = Result<DecodedFrame, (String, String)>;
pub(crate) type DecodeRequestResult = (u64, DirectoryOpenPolicy, DecodeResult);
