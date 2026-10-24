//! LibRaw-backed RAW decoding (native-only).

use lumina_core::ImageFrame;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// RAW file extensions supported by the native LibRaw adapter (R2-CLI-01).
///
/// Single source of truth for "which file names are RAW inputs", pinned to the
/// SOLL format list in `feature/platform/cli-gui-wasm.md` ("Der native
/// LibRaw-Adapter unterstützt CR2, CR3, NEF, ARW, DNG, ORF, RAF, RW2, CRW, PEF,
/// SRW, 3FR, IIQ, RWL, MOS, ERF, KDC und X3F"). Consumers (CLI batch
/// collection, decode routing, MCP) MUST reference this list instead of keeping
/// private copies — the previous triple duplication drifted and silently
/// skipped 9 of the 18 formats in `lumina batch`.
///
/// Values are lowercase, without the leading dot. Matching is ASCII-case-
/// insensitive via [`is_raw_extension`].
pub const RAW_EXTENSIONS: &[&str] = &[
    "arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2", "crw", "pef", "srw", "3fr", "iiq",
    "rwl", "mos", "erf", "kdc", "x3f",
];

/// Whether `extension` names a RAW format from [`RAW_EXTENSIONS`]
/// (ASCII-case-insensitive, leading dot must already be stripped).
pub fn is_raw_extension(extension: &str) -> bool {
    let lowered = extension.to_ascii_lowercase();
    RAW_EXTENSIONS.contains(&lowered.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DemosaicMethod {
    #[default]
    LibRawDefault,
    Linear,
    Vng,
    Ppg,
    Ahd,
    Dcb,
    Dht,
    Aahd,
}

impl DemosaicMethod {
    /// Translates a method into LibRaw's `output_params.user_qual` value that
    /// `libraw_set_demosaic` stores and `libraw_dcraw_process` dispatches on
    /// (R2-RAW-01).
    ///
    /// Reference semantics of the linked LibRaw 0.22.x (`dcraw_process`
    /// interpolation dispatch, `src/postprocessing/dcraw_process.cpp`; also
    /// documented in the official API reference under
    /// `libraw_output_params_t.user_qual`):
    ///
    /// | `user_qual` | algorithm |
    /// | ----------- | --------- |
    /// | 0           | linear    |
    /// | 1           | VNG       |
    /// | 2           | PPG       |
    /// | 3           | AHD (default) |
    /// | 4           | DCB       |
    /// | 11          | DHT       |
    /// | 12          | AAHD      |
    ///
    /// Any other value falls through the dispatch chain without selecting an
    /// algorithm (a silent fallback — forbidden by Agents.md). Before this fix
    /// the mapping was shifted by +1 (`Linear→1 … Aahd→13`), so every explicit
    /// non-default choice ran the wrong algorithm and `Aahd` silently degraded
    /// to AHD. The table is pinned byte-exactly by
    /// `demosaic_libraw_values_match_dcraw_process_user_qual_table`.
    fn libraw_value(self) -> Option<i32> {
        match self {
            Self::LibRawDefault => None,
            Self::Linear => Some(0),
            Self::Vng => Some(1),
            Self::Ppg => Some(2),
            Self::Ahd => Some(3),
            Self::Dcb => Some(4),
            Self::Dht => Some(11),
            Self::Aahd => Some(12),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawDecodeOptions {
    pub demosaicing: DemosaicMethod,
    pub output_bits: u8,
}

impl Default for RawDecodeOptions {
    fn default() -> Self {
        Self {
            demosaicing: DemosaicMethod::LibRawDefault,
            output_bits: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawMetadata {
    /// Visible output geometry; this is not necessarily the pre-orientation buffer geometry.
    pub width: u32,
    /// Visible output geometry; this is not necessarily the pre-orientation buffer geometry.
    pub height: u32,
    /// True EXIF orientation code (`1..=8`) of the source: the permutation that
    /// the decoder promotion step applied to turn the unrotated sensor buffer
    /// into the visible frame. This is NOT the raw dcraw flip value; see
    /// [`dcraw_flip_to_exif_orientation`] for the verified translation.
    pub orientation: u8,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub iso: Option<f32>,
    pub shutter: Option<f32>,
    pub aperture: Option<f32>,
    /// Human-readable lens name as reported by the decoder (REVIEW-RAW-N2):
    /// the standardised EXIF `LensModel` tag (`0xA434`, parsed by LibRaw into
    /// `imgdata.lens.Lens`) when present, otherwise LibRaw's vendor-makernote
    /// lens name (`imgdata.lens.makernotes.Lens`); `None` when the source
    /// carries no usable lens identification. This is raw source metadata —
    /// a Lensfun profile match may still resolve a different, database-
    /// normalised lens name.
    pub lens: Option<String>,
    pub focal_length: Option<f32>,
    pub timestamp: Option<i64>,
    pub artist: Option<String>,
    pub description: Option<String>,
    pub camera_matrix: [[f32; 4]; 3],
    pub camera_white_balance: [f32; 4],
    pub pre_multipliers: [f32; 4],
    pub icc_profile: Option<Vec<u8>>,
}

/// Translates the dcraw/LibRaw `flip` bit-field (`libraw_image_sizes_t.flip`,
/// values `0..=7`) into the EXIF orientation code (`1..=8`) that
/// [`RawMetadata::orientation`] promises. The two codings are **not**
/// identical: dcraw encodes the rotation as a bit field, EXIF uses an
/// enumeration (e.g. dcraw flip 5 is EXIF orientation 8, not 5).
///
/// REVIEW-RAW-FLIP-1: the table was verified empirically against the linked
/// LibRaw 0.22.2 sources —
///
/// - `src/write/file_write.cpp`, `LibRaw::flip_index()`: flip bit semantics.
///   For an output pixel `(x, y)` the source coordinate in the unrotated
///   buffer is derived as: swap row/column when bit `4` is set (transpose),
///   mirror rows when bit `2` is set, mirror columns when bit `1` is set.
/// - `src/metadata/tiff.cpp:631`: LibRaw's own inverse lookup
///   `"50132467"[orientation & 7]` maps EXIF orientation → flip and confirms
///   this table as its exact inverse (see the round-trip unit test).
/// - `src/metadata/identify.cpp`: degree-based flips are normalised at open
///   time (90° → 6, 180° → 3, 270° → 5), so `sizes.flip` is always in
///   `0..=7` after `libraw_open_*`.
///
/// | flip | bits (4/2/1) | geometry                        | EXIF orientation |
/// | ---- | ------------ | ------------------------------- | ---------------- |
/// | 0    | 000          | identity                        | 1                |
/// | 1    | 001          | mirror horizontal               | 2                |
/// | 2    | 010          | mirror vertical                 | 4                |
/// | 3    | 011          | rotate 180°                     | 3                |
/// | 4    | 100          | transpose (main diagonal)       | 5                |
/// | 5    | 101          | rotate 90° CCW (= 270° CW)      | 8                |
/// | 6    | 110          | rotate 90° CW                   | 6                |
/// | 7    | 111          | anti-transpose (anti-diagonal)  | 7                |
///
/// Values outside `0..=7` do not occur after `identify()` (an unknown flip is
/// stored as `UINT_MAX` and reset to `0`); they are defensively mapped to
/// orientation `1` ("no rotation"), mirroring LibRaw's own unknown→0 handling.
pub fn dcraw_flip_to_exif_orientation(flip: i32) -> u8 {
    match flip {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 3,
        4 => 5,
        5 => 8,
        6 => 6,
        7 => 7,
        _ => 1,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawImage {
    pub frame: ImageFrame,
    pub metadata: RawMetadata,
}

#[derive(Debug, Error)]
pub enum RawError {
    #[error("RAW decoding is not available on this platform (WASM/browser)")]
    UnsupportedPlatform,
    #[error("could not read RAW file `{path}`: {message}")]
    Io { path: String, message: String },
    #[error("LibRaw {operation} failed ({code}): {message}")]
    LibRaw {
        operation: &'static str,
        code: i32,
        message: String,
    },
    #[error("LibRaw returned an invalid {0}")]
    InvalidData(&'static str),
    #[error("memory budget exceeded: {source}")]
    MemoryBudgetExceeded {
        source: lumina_core::memory::MemoryBudgetError,
    },
}

pub fn decode_file(path: impl AsRef<std::path::Path>) -> Result<RawImage, RawError> {
    // R2-RAW-02: hand the path to LibRaw directly (`libraw_open_file`
    // streams through its own datastream) instead of reading the whole
    // RAW — up to 50–150 MB for a CR3 — into a heap buffer first. Peak
    // memory drops by roughly the file size; `open_buffer` remains in
    // use only for the byte-oriented `decode_bytes*` entry points.
    native::decode_with_options(
        native::DecodeInput::File(path.as_ref().to_path_buf()),
        &RawDecodeOptions::default(),
    )
}

pub fn decode_bytes(bytes: &[u8], name: impl AsRef<str>) -> Result<RawImage, RawError> {
    // R2-RAW-03 (API-Hygiene): the `name` parameter was previously dead
    // (`let _ = name`). It is now **used** for input validation and diagnostic
    // context — an empty/missing name is rejected loudly instead of being
    // silently ignored. The signature is intentionally preserved (no breaking
    // removal) so the cross-crate `decode_bytes(bytes, filename)` contract stays
    // stable until a deliberate version bump; a `#[deprecated]` would push
    // `warn(deprecated)` onto every caller (cli/mcp/gui/bench) and break the
    // workspace 0-warning gate. If the parameter is ever dropped, all call sites
    // listed above must change in the same release.
    let name_ref = name.as_ref();
    if name_ref.trim().is_empty() {
        return Err(RawError::InvalidData("missing file name"));
    }
    native::decode_with_options(
        native::DecodeInput::Buffer(bytes),
        &RawDecodeOptions::default(),
    )
}

pub fn decode_bytes_with_options(
    bytes: &[u8],
    name: impl AsRef<str>,
    options: &RawDecodeOptions,
) -> Result<RawImage, RawError> {
    let name_ref = name.as_ref();
    if name_ref.trim().is_empty() {
        return Err(RawError::InvalidData("missing file name"));
    }
    native::decode_with_options(native::DecodeInput::Buffer(bytes), options)
}

/// Reads only the metadata of a RAW file — **without** decoding pixels
/// (R2-CLI-04).
///
/// This is the metadata-only counterpart of [`decode_file`]: EXIF identity
/// (camera, lens, exposure fields), orientation and the final visible output
/// geometry are returned exactly as a full decode would report them in
/// [`RawImage::metadata`], but demosaicing, colour conversion, the processed
/// memory image and the RGBA promotion step never run. Consumers that need
/// four lines of EXIF (`inspect`) no longer pay for a 24-megapixel frame.
///
/// # Honest LibRaw limit (documented, not hidden)
/// The returned geometry is finalised by `libraw_adjust_sizes_info_only`,
/// which LibRaw guards with `CHECK_ORDER_LOW(LIBRAW_PROGRESS_LOAD_RAW)`:
/// it refuses to run before [`libraw_unpack`](raw's unpack). Compressed RAW
/// entropy decoding therefore still happens here — it is required by the
/// library's progress-order contract, not a LuminaRust choice. What is
/// skipped is everything AFTER unpack: `dcraw_process` (demosaicing +
/// colour pipeline), `dcraw_make_mem_image` (full-frame allocation) and the
/// orientation promotion copy. No pixel-sized allocation occurs, so the
/// [`MemoryBudget`](lumina_core::memory) decode gate does not apply.
///
/// The geometry/orientation values match a full [`decode_file`] of the same
/// source bit-for-bit; this is pinned by tests against committed fixtures.
pub fn read_metadata(path: impl AsRef<std::path::Path>) -> Result<RawMetadata, RawError> {
    native::read_metadata_with_input(native::DecodeInput::File(path.as_ref().to_path_buf()))
}

/// Metadata-only variant of [`read_metadata`] for in-memory bytes.
///
/// `name` exists for signature parity with [`decode_bytes`] (R2-RAW-03) and is
/// now used for the same empty-name validation as the decode entry points
/// (no silent fallback when the caller forgets the file name).
pub fn read_metadata_bytes(bytes: &[u8], name: impl AsRef<str>) -> Result<RawMetadata, RawError> {
    let name_ref = name.as_ref();
    if name_ref.trim().is_empty() {
        return Err(RawError::InvalidData("missing file name"));
    }
    native::read_metadata_with_input(native::DecodeInput::Buffer(bytes))
}

/// Returns the linked LibRaw version string (e.g. `"0.22.2"`).
///
/// Including the linked decoder version in the decode identity lets LuminaRust
/// detect when an upgraded LibRaw produces different output geometry or pixel
/// values for the same source — CR3 dimensions, for example, changed between
/// LibRaw 0.21.x (6160×4144) and 0.22.x (6032×4024) — so cached renders and
/// masks are invalidated instead of silently reused.
pub fn libraw_version() -> Option<String> {
    native::libraw_version()
}

/// Decode-identity version string for the LibRaw decoder.
///
/// The value carries a `+luminaabiN` generation suffix. It changes whenever a
/// LuminaRust-side fix alters observable decode output **without** changing the
/// linked library version, so caches and persisted artefacts computed with the
/// previous behaviour are invalidated instead of silently reused:
///
/// - `+luminaabi2`: the vendored bindings were re-pinned to the true LibRaw
///   0.22 layout (REVIEW-RAW-ABI-1); `use_camera_wb`/`use_camera_matrix` are
///   now actually applied and EXIF orientation is applied by the promotion
///   step instead of LibRaw.
/// - `+luminaabi3`: `RawMetadata.orientation` now carries the true EXIF
///   orientation code instead of the raw dcraw flip bit-field
///   (REVIEW-RAW-FLIP-1). For flips 1, 2, 4 and 5 this changes both the
///   persisted orientation value and the pixel permutation applied by the
///   promotion step (e.g. dcraw flip 5: previously persisted as orientation 5
///   with a transpose, now correctly orientation 8 with a 90° CCW rotation).
///
/// Deliberate non-change #2 (R2-RAW-01): correcting the
/// `DemosaicMethod::libraw_value` mapping to the real `user_qual` table
/// changes observable pixels **only** for decodes that explicitly select a
/// non-default demosaic algorithm. The default (`LibRawDefault`) has never
/// called `libraw_set_demosaic` and stays byte-identical, and the old mapping
/// was never persisted: neither `DemosaicMethod` nor `RawDecodeOptions`
/// appear in the sidecar schema or any other persisted artefact (both types
/// are referenced exclusively inside this crate), so no persisted state can
/// encode a choice made under the wrong mapping and no cache key built from
/// persisted data depends on it. A generation bump would invalidate caches
/// for nothing; none is issued.
///
/// Deliberate non-change (REVIEW-RAW-N2): populating `RawMetadata.lens`
/// enriches display metadata only. It alters no pixel value, geometry, colour
/// handling or budget input, and `RawMetadata` is neither persisted in
/// sidecars nor hashed into decode fingerprints/RenderKeys, so caches and
/// persisted artefacts computed before the change stay valid and no new
/// generation suffix is required.
pub fn libraw_decode_version() -> String {
    const DECODE_GENERATION: &str = "+luminaabi3";
    match libraw_version() {
        Some(version) => format!("{version}{DECODE_GENERATION}"),
        None => format!("unknown{DECODE_GENERATION}"),
    }
}

mod native;

#[cfg(test)]
mod tests;
