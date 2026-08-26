//! LibRaw-backed RAW decoding. The native backend is deliberately absent from WASM.

use lumina_core::ImageFrame;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    #[cfg(not(target_arch = "wasm32"))]
    fn libraw_value(self) -> Option<i32> {
        match self {
            Self::LibRawDefault => None,
            Self::Linear => Some(1),
            Self::Vng => Some(2),
            Self::Ppg => Some(3),
            Self::Ahd => Some(4),
            Self::Dcb => Some(11),
            Self::Dht => Some(12),
            Self::Aahd => Some(13),
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
    #[error("RAW input name is required for byte decoding")]
    MissingName,
    #[error("memory budget exceeded: {source}")]
    MemoryBudgetExceeded {
        source: lumina_core::memory::MemoryBudgetError,
    },
}

pub fn decode_file(path: impl AsRef<std::path::Path>) -> Result<RawImage, RawError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        return Err(RawError::UnsupportedPlatform);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|error| RawError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(RawError::MissingName)?;
        decode_bytes(&bytes, name)
    }
}

pub fn decode_bytes(bytes: &[u8], name: impl AsRef<str>) -> Result<RawImage, RawError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (bytes, name);
        return Err(RawError::UnsupportedPlatform);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::decode_bytes_with_options(bytes, name.as_ref(), &RawDecodeOptions::default())
    }
}

pub fn decode_bytes_with_options(
    bytes: &[u8],
    name: impl AsRef<str>,
    options: &RawDecodeOptions,
) -> Result<RawImage, RawError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (bytes, name, options);
        return Err(RawError::UnsupportedPlatform);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::decode_bytes_with_options(bytes, name.as_ref(), options)
    }
}

/// Returns the linked LibRaw version string (e.g. `"0.22.2"`) when native RAW
/// decoding is available. On platforms without native LibRaw (WASM/browser)
/// this returns `None`.
///
/// Including the linked decoder version in the decode identity lets LuminaRust
/// detect when an upgraded LibRaw produces different output geometry or pixel
/// values for the same source — CR3 dimensions, for example, changed between
/// LibRaw 0.21.x (6160×4144) and 0.22.x (6032×4024) — so cached renders and
/// masks are invalidated instead of silently reused.
pub fn libraw_version() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::libraw_version()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// Decode-identity version string for the LibRaw decoder.
///
/// Falls back to `"unknown"` when no native LibRaw is linked (WASM), keeping
/// the decoder identity stable for non-native targets.
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

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use libraw_sys as raw;
    use std::ffi::CStr;
    use std::os::raw::c_void;

    struct Handle(*mut raw::libraw_data_t);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { raw::libraw_close(self.0) };
            }
        }
    }

    struct Processed(*mut raw::libraw_processed_image_t);

    impl Drop for Processed {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { raw::libraw_dcraw_clear_mem(self.0) };
            }
        }
    }

    fn error(operation: &'static str, code: i32) -> RawError {
        let message = unsafe {
            CStr::from_ptr(raw::libraw_strerror(code))
                .to_string_lossy()
                .into_owned()
        };
        RawError::LibRaw {
            operation,
            code,
            message,
        }
    }

    /// Runs `libraw_adjust_sizes_info_only` and propagates LibRaw's error code
    /// instead of swallowing it (REVIEW-RAW-N1).
    ///
    /// LibRaw semantics (verified against the LibRaw 0.22 C API):
    /// `libraw_adjust_sizes_info_only` returns an `int` error code
    /// (`LIBRAW_SUCCESS = 0`, negative values on failure). Internally it is
    /// `LibRaw::adjust_sizes_info_only()`, which is guarded by
    /// `CHECK_ORDER_LOW(LIBRAW_PROGRESS_LOAD_RAW)` and finalises the visible
    /// output geometry (`sizes.width`, `sizes.height`, margins, flip bookkeeping)
    /// without allocating the processed buffer. A non-zero result therefore
    /// means the geometry that both the memory-budget gate and the subsequent
    /// `dcraw_process` rely on has NOT been reliably adjusted — proceeding
    /// silently would let the budget gate validate outdated measures, so the
    /// decode fails loudly instead.
    pub(super) fn adjust_sizes_checked(handle: *mut raw::libraw_data_t) -> Result<(), RawError> {
        let code = unsafe { raw::libraw_adjust_sizes_info_only(handle) };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("adjusting sizes", code));
        }
        Ok(())
    }

    pub fn libraw_version() -> Option<String> {
        let ptr = unsafe { raw::libraw_version() };
        if ptr.is_null() {
            return None;
        }
        let version = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        let version = version.trim();
        if version.is_empty() {
            None
        } else {
            Some(version.to_string())
        }
    }

    fn text(value: &[std::os::raw::c_char]) -> Option<String> {
        let bytes: Vec<u8> = value.iter().map(|byte| *byte as u8).collect();
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        let value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    fn positive(value: f32) -> Option<f32> {
        value
            .is_finite()
            .then_some(value)
            .filter(|value| *value > 0.0)
    }

    /// Upper sanity bound for an embedded ICC profile. Real profiles stay far
    /// below this; anything larger indicates a corrupt `profile_length`.
    const MAX_ICC_PROFILE_BYTES: usize = 64 * 1024 * 1024;

    /// Resolves the human-readable lens name from the available LibRaw sources
    /// with a deterministic source priority (REVIEW-RAW-N2): the standardised
    /// EXIF `LensModel` tag wins over the vendor-specific makernote name,
    /// because EXIF is format-independent and verbatim from the file, while
    /// `imgdata.lens.makernotes.Lens` is only filled for cameras whose
    /// makernotes LibRaw knows how to parse. Both inputs are already
    /// normalised by [`text`] (`None` for empty/whitespace-only buffers), so a
    /// present-but-empty EXIF field falls through to the makernote name.
    fn resolve_lens_name(
        exif_lens_model: Option<String>,
        makernotes_lens: Option<String>,
    ) -> Option<String> {
        exif_lens_model.or(makernotes_lens)
    }

    pub fn decode_bytes_with_options(
        bytes: &[u8],
        name: &str,
        options: &RawDecodeOptions,
    ) -> Result<RawImage, RawError> {
        if bytes.is_empty() {
            return Err(RawError::InvalidData("empty input"));
        }
        if !matches!(options.output_bits, 8 | 16) {
            return Err(RawError::InvalidData("output bit depth"));
        }
        let handle = Handle(unsafe { raw::libraw_init(raw::LIBRAW_OPTIONS_NONE) });
        if handle.0.is_null() {
            return Err(RawError::InvalidData("LibRaw handle"));
        }
        // SAFETY: LibRaw's `open_buffer` reads the input bytes and never mutates
        // them; `bytes` outlives this call (it is a function parameter dropped
        // only after `handle`, whose `Drop` closes the decoder). Handing the
        // const slice pointer to the `*mut c_void` FFI argument therefore lets us
        // skip an otherwise redundant full-file copy of the RAW (F-074-A2) without
        // giving LibRaw writable access to caller memory.
        let code = unsafe {
            raw::libraw_open_buffer(handle.0, bytes.as_ptr() as *mut c_void, bytes.len())
        };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("opening input", code));
        }
        let data = unsafe { &*handle.0 };
        // The dcraw flip is captured BEFORE `user_flip` is forced to 0 below:
        // it records the source orientation that the promotion step must apply
        // itself, because LibRaw is deliberately told NOT to rotate. The raw
        // flip bit-field is translated into the EXIF orientation code that
        // `RawMetadata.orientation` and the promotion step operate on
        // (REVIEW-RAW-FLIP-1; see `dcraw_flip_to_exif_orientation`).
        let orientation = dcraw_flip_to_exif_orientation(data.sizes.flip);
        let camera_matrix = data.color.rgb_cam;
        let camera_white_balance = data.color.cam_mul;
        let pre_multipliers = data.color.pre_mul;
        let icc_profile = if data.color.profile.is_null() || data.color.profile_length == 0 {
            None
        } else {
            // LibRaw owns exactly `profile_length` bytes behind `profile`; the
            // pinned-ABI asserts guarantee the field offsets are real. The
            // length is still sanity-capped before materialising a slice from
            // the foreign pointer, so a corrupt length can never turn into an
            // out-of-bounds read or an absurd allocation.
            let length = usize::try_from(data.color.profile_length)
                .map_err(|_| RawError::InvalidData("ICC profile length"))?;
            if length > MAX_ICC_PROFILE_BYTES {
                return Err(RawError::InvalidData("ICC profile length"));
            }
            // SAFETY: `profile` points to LibRaw-owned memory of exactly
            // `profile_length` bytes while the handle is open; `length` was
            // validated against that bound above.
            Some(
                unsafe { std::slice::from_raw_parts(data.color.profile.cast::<u8>(), length) }
                    .to_vec(),
            )
        };
        let metadata = RawMetadata {
            width: 0,
            height: 0,
            orientation,
            camera_make: text(&data.idata.make),
            camera_model: text(&data.idata.model),
            iso: positive(data.other.iso_speed),
            shutter: positive(data.other.shutter),
            aperture: positive(data.other.aperture),
            lens: resolve_lens_name(text(&data.lens.Lens), text(&data.lens.makernotes.Lens)),
            focal_length: positive(data.other.focal_len),
            timestamp: (data.other.timestamp != 0).then_some(data.other.timestamp as i64),
            artist: text(&data.other.artist),
            description: text(&data.other.desc),
            camera_matrix,
            camera_white_balance,
            pre_multipliers,
            icc_profile,
        };
        unsafe {
            // Direct field writes are safe only because the vendored bindings
            // are pinned to the linked LibRaw 0.22 ABI (see
            // vendor/libraw-sys/src/lib.rs and the layout gates in its
            // build.rs). Before the pin, these writes landed inside
            // `makernotes` and neither flag was ever applied.
            (*handle.0).params.user_flip = 0;
            if let Some(value) = options.demosaicing.libraw_value() {
                raw::libraw_set_demosaic(handle.0, value);
            }
            (*handle.0).params.use_camera_wb = 1;
            (*handle.0).params.use_camera_matrix = 1;
            raw::libraw_set_output_bps(handle.0, options.output_bits as i32);
            raw::libraw_set_output_color(handle.0, 1);
            raw::libraw_set_no_auto_bright(handle.0, 0);
        }
        let code = unsafe { raw::libraw_unpack(handle.0) };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("unpacking input", code));
        }
        // Compute the output geometry without allocating the processed image
        // buffer, then enforce the memory budget before the allocation-prone
        // processing step (F-075). The return code is checked, not swallowed
        // (REVIEW-RAW-N1): this is the call that finalises
        // `sizes.width`/`sizes.height`, so a failure means the budget gate
        // below would validate against stale/unadjusted measures and the
        // subsequent `dcraw_process` output geometry would diverge from what
        // was budgeted. No silent fallback.
        adjust_sizes_checked(handle.0)?;
        let out_width = unsafe { &*handle.0 }.sizes.width as u64;
        let out_height = unsafe { &*handle.0 }.sizes.height as u64;
        let channels = 4u32; // final RGBA frame (3-channel LibRaw output is
                             // promoted to RGBA); using 4 keeps the guard conservative for 8-bit.
        let bytes_per_channel = options.output_bits as u32 / 8;
        lumina_core::memory::MemoryBudget::from_env()
            .check_decode(out_width, out_height, channels, bytes_per_channel)
            .map_err(|source| RawError::MemoryBudgetExceeded { source })?;
        let code = unsafe { raw::libraw_dcraw_process(handle.0) };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("processing input", code));
        }
        let mut image_error = raw::LIBRAW_SUCCESS;
        let processed =
            Processed(unsafe { raw::libraw_dcraw_make_mem_image(handle.0, &mut image_error) });
        if processed.0.is_null() {
            return Err(error("creating processed image", image_error));
        }
        let image = unsafe { &*processed.0 };
        if !(image.bits == 8 || image.bits == 16) || !(image.colors == 3 || image.colors == 4) {
            return Err(RawError::InvalidData("RGB processed image"));
        }
        if image.width == 0 || image.height == 0 {
            return Err(RawError::InvalidData("image dimensions"));
        }
        let frame = if image.bits == 8 {
            let length = (image.width as usize)
                .checked_mul(image.height as usize)
                .and_then(|value| value.checked_mul(image.colors as usize))
                .ok_or(RawError::InvalidData("image data length"))?;
            if (image.data_size as usize) < length {
                return Err(RawError::InvalidData("image data size"));
            }
            let source = unsafe { std::slice::from_raw_parts(image.data.as_ptr(), length) };
            // `user_flip` is forced to 0, so LibRaw hands out the UNROTATED
            // buffer; the captured EXIF `orientation` is applied here in the
            // promotion step (see `rgba_from_bytes`).
            rgba_from_bytes(
                source,
                image.width as u32,
                image.height as u32,
                image.colors as usize,
                orientation,
            )?
        } else {
            let length = (image.width as usize)
                .checked_mul(image.height as usize)
                .and_then(|value| value.checked_mul(image.colors as usize))
                .ok_or(RawError::InvalidData("image data length"))?;
            if (image.data_size as usize) < length.saturating_mul(2) {
                return Err(RawError::InvalidData("image data size"));
            }
            let source =
                unsafe { std::slice::from_raw_parts(image.data.as_ptr().cast::<u16>(), length) };
            rgba_from_words(
                source,
                image.width as u32,
                image.height as u32,
                image.colors as usize,
                orientation,
            )?
        };
        let mut metadata = metadata;
        metadata.width = frame.width;
        metadata.height = frame.height;
        let _ = name;
        Ok(RawImage { frame, metadata })
    }

    /// Promotes a LibRaw 8-bit RGB(A) processed image to an RGBA8 `ImageFrame`,
    /// applying the EXIF-orientation permutation recorded in
    /// `RawMetadata.orientation`. The input is the UNROTATED LibRaw buffer
    /// (`user_flip` is forced to 0), so this step owns the rotation.
    /// Orientation `1` (no rotation/flip) is the hot path used by the
    /// landscape fixture: it is a pure RGB -> RGBA promotion (opaque alpha)
    /// with no per-pixel branch and no `slice::copy_from_slice` bounds churn,
    /// which keeps the inner loop tight and vectorizable. The generic arm
    /// reproduces the original per-pixel mapping exactly for the other
    /// orientations.
    fn rgba_from_bytes(
        source: &[u8],
        width: u32,
        height: u32,
        channels: usize,
        orientation: u8,
    ) -> Result<ImageFrame, RawError> {
        let (out_width, out_height) = if (5..=8).contains(&orientation) {
            (height, width)
        } else {
            (width, height)
        };
        let mut pixels = vec![0u8; out_width as usize * out_height as usize * 4];

        if orientation == 1 {
            // Source and target share the same row-major layout; only the channel
            // count changes (3 -> 4). The opaque-alpha branch is hoisted out of the
            // pixel loop so the compiler can stream each row without per-pixel
            // branching or slice bounds checks.
            if channels == 4 {
                for y in 0..out_height as usize {
                    let row = y * out_width as usize * 4;
                    let end = row + out_width as usize * 4;
                    pixels[row..end].copy_from_slice(&source[row..end]);
                }
            } else {
                for y in 0..out_height as usize {
                    let s = y * out_width as usize * 3;
                    let d = y * out_width as usize * 4;
                    for x in 0..out_width as usize {
                        let sc = s + x * 3;
                        let dc = d + x * 4;
                        pixels[dc] = source[sc];
                        pixels[dc + 1] = source[sc + 1];
                        pixels[dc + 2] = source[sc + 2];
                        pixels[dc + 3] = 255;
                    }
                }
            }
        } else {
            transform_rows(
                &mut pixels,
                source,
                width,
                height,
                out_width,
                out_height,
                channels,
                orientation,
            );
        }

        ImageFrame::new(out_width, out_height, pixels)
            .map_err(|_| RawError::InvalidData("RGBA frame"))
    }

    /// Promotes a LibRaw 16-bit RGB(A) processed image to an RGBA8 `ImageFrame`.
    /// The high byte of each 16-bit sample becomes the 8-bit channel value,
    /// matching the previous `orient_16` shift (`value >> 8`). The 16-bit source
    /// is read directly, so no intermediate `u8` buffer is allocated (the old
    /// `orient_16` collected a full `Vec<u8>` only to feed `orient`).
    fn rgba_from_words(
        source: &[u16],
        width: u32,
        height: u32,
        channels: usize,
        orientation: u8,
    ) -> Result<ImageFrame, RawError> {
        let (out_width, out_height) = if (5..=8).contains(&orientation) {
            (height, width)
        } else {
            (width, height)
        };
        let mut pixels = vec![0u8; out_width as usize * out_height as usize * 4];

        if orientation == 1 {
            if channels == 4 {
                for y in 0..out_height as usize {
                    let s = y * out_width as usize * 4;
                    let d = y * out_width as usize * 4;
                    for x in 0..out_width as usize {
                        let sc = s + x * 4;
                        let dc = d + x * 4;
                        pixels[dc] = (source[sc] >> 8) as u8;
                        pixels[dc + 1] = (source[sc + 1] >> 8) as u8;
                        pixels[dc + 2] = (source[sc + 2] >> 8) as u8;
                        pixels[dc + 3] = (source[sc + 3] >> 8) as u8;
                    }
                }
            } else {
                for y in 0..out_height as usize {
                    let s = y * out_width as usize * 3;
                    let d = y * out_width as usize * 4;
                    for x in 0..out_width as usize {
                        let sc = s + x * 3;
                        let dc = d + x * 4;
                        pixels[dc] = (source[sc] >> 8) as u8;
                        pixels[dc + 1] = (source[sc + 1] >> 8) as u8;
                        pixels[dc + 2] = (source[sc + 2] >> 8) as u8;
                        pixels[dc + 3] = 255;
                    }
                }
            }
        } else {
            for y in 0..out_height as usize {
                for x in 0..out_width as usize {
                    let (sx, sy) =
                        oriented_source_xy(x as u32, y as u32, width, height, orientation);
                    let source_offset = (sy as usize * width as usize + sx as usize) * channels;
                    let target_offset = (y * out_width as usize + x) * 4;
                    pixels[target_offset] = (source[source_offset] >> 8) as u8;
                    pixels[target_offset + 1] = (source[source_offset + 1] >> 8) as u8;
                    pixels[target_offset + 2] = (source[source_offset + 2] >> 8) as u8;
                    pixels[target_offset + 3] = if channels == 4 {
                        (source[source_offset + 3] >> 8) as u8
                    } else {
                        255
                    };
                }
            }
        }

        ImageFrame::new(out_width, out_height, pixels)
            .map_err(|_| RawError::InvalidData("RGBA frame"))
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn transform_rows(
        pixels: &mut [u8],
        source: &[u8],
        width: u32,
        height: u32,
        out_width: u32,
        out_height: u32,
        channels: usize,
        orientation: u8,
    ) {
        for y in 0..out_height as usize {
            for x in 0..out_width as usize {
                let (sx, sy) = oriented_source_xy(x as u32, y as u32, width, height, orientation);
                let source_offset = (sy as usize * width as usize + sx as usize) * channels;
                let target_offset = (y * out_width as usize + x) * 4;
                pixels[target_offset] = source[source_offset];
                pixels[target_offset + 1] = source[source_offset + 1];
                pixels[target_offset + 2] = source[source_offset + 2];
                pixels[target_offset + 3] = if channels == 4 {
                    source[source_offset + 3]
                } else {
                    255
                };
            }
        }
    }

    #[inline]
    fn oriented_source_xy(x: u32, y: u32, width: u32, height: u32, orientation: u8) -> (u32, u32) {
        match orientation {
            2 => (width - 1 - x, y),
            3 => (width - 1 - x, height - 1 - y),
            4 => (x, height - 1 - y),
            5 => (y, x),
            6 => (y, height - 1 - x),
            7 => (width - 1 - y, height - 1 - x),
            8 => (width - 1 - y, x),
            _ => (x, y),
        }
    }

    #[cfg(test)]
    mod decode_gate_tests {
        use super::*;

        /// REVIEW-RAW-N1: a failure of `libraw_adjust_sizes_info_only` must be
        /// propagated as a [`RawError::LibRaw`], not swallowed. A freshly
        /// initialised handle has neither opened nor unpacked any input, so
        /// LibRaw's own `CHECK_ORDER_LOW(LIBRAW_PROGRESS_LOAD_RAW)` guard
        /// inside `adjust_sizes_info_only()` rejects the call with
        /// `LIBRAW_OUT_OF_ORDER_CALL`. This exercises the new error path
        /// end-to-end through real LibRaw semantics — no crafted file is
        /// needed to make the call fail.
        #[test]
        fn adjust_sizes_failure_is_propagated_not_swallowed() {
            let handle = Handle(unsafe { raw::libraw_init(raw::LIBRAW_OPTIONS_NONE) });
            assert!(!handle.0.is_null(), "LibRaw handle must initialise");
            let error = adjust_sizes_checked(handle.0).expect_err("out-of-order call must fail");
            match error {
                RawError::LibRaw {
                    operation,
                    code,
                    message,
                } => {
                    assert_eq!(operation, "adjusting sizes");
                    assert_eq!(code, raw::LIBRAW_OUT_OF_ORDER_CALL);
                    assert!(!message.is_empty(), "strerror text must be present");
                }
                other => panic!("expected RawError::LibRaw, got {other:?}"),
            }
        }

        /// REVIEW-RAW-N2 source priority: the standardised EXIF LensModel wins
        /// over the vendor-specific makernote name.
        #[test]
        fn lens_resolution_prefers_exif_over_makernotes() {
            let resolved = resolve_lens_name(Some("EXIF Lens".into()), Some("Maker Lens".into()));
            assert_eq!(resolved.as_deref(), Some("EXIF Lens"));
        }

        /// When LibRaw could not parse an EXIF LensModel (`text()` normalises
        /// empty/whitespace-only buffers to `None`), the makernote name is used;
        /// with neither source available the field stays `None`.
        #[test]
        fn lens_resolution_falls_back_and_reports_absence() {
            let fallback = resolve_lens_name(None, Some("Maker Lens".into()));
            assert_eq!(fallback.as_deref(), Some("Maker Lens"));

            let none = resolve_lens_name(None, None);
            assert!(none.is_none());
        }

        /// Regression guard for the wiring: the metadata construction feeds
        /// both LibRaw sources through `text()` into the resolver, so this pure
        /// function documents the exact combination semantics relied upon.
        #[test]
        fn lens_resolution_matches_text_normalisation_contract() {
            // `text()` never yields Some(empty); but if it ever did, the
            // resolver would still prefer it over makernotes — the contract is
            // "first Some wins", which keeps the priority deterministic.
            let resolved = resolve_lens_name(Some(String::new()), Some("Maker Lens".into()));
            assert_eq!(resolved.as_deref(), Some(""));
        }
    }

    #[cfg(test)]
    mod conversion_tests {
        use super::*;

        /// Independent re-implementation of the original per-pixel orientation
        /// mapping, used to prove the optimized converters emit byte-identical
        /// output (F-074-A2: no semantic / output change).
        fn reference_rgba(
            source: &[u8],
            width: u32,
            height: u32,
            channels: usize,
            orientation: u8,
        ) -> Vec<u8> {
            let (out_w, out_h) = if (5..=8).contains(&orientation) {
                (height, width)
            } else {
                (width, height)
            };
            let mut pixels = vec![0u8; out_w as usize * out_h as usize * 4];
            for y in 0..out_h {
                for x in 0..out_w {
                    let (sx, sy) = match orientation {
                        2 => (width - 1 - x, y),
                        3 => (width - 1 - x, height - 1 - y),
                        4 => (x, height - 1 - y),
                        5 => (y, x),
                        6 => (y, height - 1 - x),
                        7 => (width - 1 - y, height - 1 - x),
                        8 => (width - 1 - y, x),
                        _ => (x, y),
                    };
                    let so = (sy * width + sx) as usize * channels;
                    let to = (y * out_w + x) as usize * 4;
                    pixels[to] = source[so];
                    pixels[to + 1] = source[so + 1];
                    pixels[to + 2] = source[so + 2];
                    pixels[to + 3] = if channels == 4 { source[so + 3] } else { 255 };
                }
            }
            pixels
        }

        fn synthetic(channels: usize, width: u32, height: u32) -> Vec<u8> {
            let mut v = vec![0u8; width as usize * height as usize * channels];
            for (i, b) in v.iter_mut().enumerate() {
                *b = (i % 251) as u8;
            }
            v
        }

        #[test]
        fn eight_bit_conversion_matches_reference_for_all_orientations() {
            for &orientation in &[1u8, 2, 3, 4, 5, 6, 7, 8] {
                for &channels in &[3usize, 4usize] {
                    let (w, h) = (7u32, 5u32);
                    let src = synthetic(channels, w, h);
                    let frame = rgba_from_bytes(&src, w, h, channels, orientation).unwrap();
                    let expected = reference_rgba(&src, w, h, channels, orientation);
                    assert_eq!(
                        frame.pixels, expected,
                        "8-bit orientation {orientation}, channels {channels}"
                    );
                    let (ew, eh) = if (5..=8).contains(&orientation) {
                        (h, w)
                    } else {
                        (w, h)
                    };
                    assert_eq!((frame.width, frame.height), (ew, eh));
                }
            }
        }

        #[test]
        fn sixteen_bit_conversion_matches_reference() {
            for &orientation in &[1u8, 5, 8] {
                let (w, h) = (6u32, 4u32);
                let mut src = vec![0u16; w as usize * h as usize * 3];
                for (i, v) in src.iter_mut().enumerate() {
                    *v = ((i % 1000) as u16) << 4;
                }
                let frame = rgba_from_words(&src, w, h, 3, orientation).unwrap();
                let hi: Vec<u8> = src.iter().map(|v| (v >> 8) as u8).collect();
                let expected = reference_rgba(&hi, w, h, 3, orientation);
                assert_eq!(frame.pixels, expected, "16-bit orientation {orientation}");
            }
        }

        #[test]
        fn opaque_alpha_for_three_channel_source() {
            // The committed fixtures decode as 3-channel RGB; the promoted RGBA
            // frame must be fully opaque.
            let src = synthetic(3, 4, 3);
            let frame = rgba_from_bytes(&src, 4, 3, 3, 1).unwrap();
            assert!(frame.pixels.iter().skip(3).step_by(4).all(|&a| a == 255));
        }

        /// Verbatim port of LibRaw 0.22.2 `LibRaw::flip_index`
        /// (`src/write/file_write.cpp`) restricted to coordinate mapping: for
        /// an output pixel `(x, y)` of the flipped image it yields the source
        /// coordinates in the unrotated W×H buffer. Bit semantics: bit 4
        /// transposes, bit 2 mirrors rows, bit 1 mirrors columns. This is the
        /// exact transform LibRaw applies when `flip != 0`; our promotion step
        /// must reproduce it from the translated EXIF orientation.
        fn libraw_flip_index_source_xy(
            flip: u32,
            x: u32,
            y: u32,
            width: u32,
            height: u32,
        ) -> (u32, u32) {
            let mut row = y;
            let mut col = x;
            if flip & 4 != 0 {
                std::mem::swap(&mut row, &mut col);
            }
            if flip & 2 != 0 {
                row = height - 1 - row;
            }
            if flip & 1 != 0 {
                col = width - 1 - col;
            }
            (col, row)
        }

        /// REVIEW-RAW-FLIP-1 geometry proof: for every dcraw flip 0..=7, the
        /// promotion mapping (`oriented_source_xy` driven by the translated
        /// EXIF orientation) must be pixel-for-pixel identical to what LibRaw
        /// itself would produce via `flip_index` on a non-square buffer.
        #[test]
        fn exif_promotion_matches_libraw_flip_index_for_every_flip() {
            let (width, height) = (7u32, 5u32);
            for flip in 0u8..=7 {
                let orientation = crate::dcraw_flip_to_exif_orientation(flip as i32);
                let (out_width, out_height) = if (5..=8).contains(&orientation) {
                    (height, width)
                } else {
                    (width, height)
                };
                assert_eq!(
                    (out_width, out_height),
                    if flip & 4 != 0 {
                        (height, width)
                    } else {
                        (width, height)
                    },
                    "flip {flip}: output dimensions must match LibRaw's swap rule"
                );
                for y in 0..out_height {
                    for x in 0..out_width {
                        let expected =
                            libraw_flip_index_source_xy(flip as u32, x, y, width, height);
                        let actual = oriented_source_xy(x, y, width, height, orientation);
                        assert_eq!(
                            actual, expected,
                            "flip {flip} → orientation {orientation} diverges at ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_raw_bytes_are_reported_as_decode_errors() {
        let error = decode_bytes(b"not a raw", "bad.cr2").unwrap_err();
        assert!(!matches!(error, RawError::UnsupportedPlatform));
    }

    #[test]
    fn empty_bytes_are_rejected() {
        assert!(decode_bytes(&[], "empty.nef").is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "set LUMINA_RAW_FIXTURE to a licensed fixture"]
    fn optional_real_fixture_checks_decode_orientation_and_dimensions() {
        let path = std::env::var_os("LUMINA_RAW_FIXTURE")
            .expect("LUMINA_RAW_FIXTURE must point to a licensed RAW fixture");
        let image = decode_file(std::path::PathBuf::from(path)).unwrap();
        assert!(image.metadata.width > 0 && image.metadata.height > 0);
        assert!((1..=8).contains(&image.metadata.orientation));
        assert_eq!(
            (image.frame.width, image.frame.height),
            (image.metadata.width, image.metadata.height)
        );
        assert_eq!(
            image.frame.pixels.len(),
            image.frame.width as usize * image.frame.height as usize * 4
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn aircraft_landscape_fixture_has_expected_geometry_and_metadata() {
        let bytes = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
        let image = decode_bytes(bytes, "aircraft-landscape.cr3").unwrap();
        assert_eq!(image.metadata.orientation, 1);
        assert_eq!((image.frame.width, image.frame.height), (6032, 4024));
        assert_eq!((image.metadata.width, image.metadata.height), (6032, 4024));
        assert_eq!(image.frame.pixels.len(), 6032 * 4024 * 4);
    }

    /// Content-driven verification note (REVIEW-RAW-FLIP-1): a planned
    /// luminance-band heuristic ("bright sky above dark ground") was measured
    /// against this fixture and turned out FALSE (top-row mean luminance
    /// ≈ 111.0 vs bottom-row ≈ 118.9; the scene has no reliable vertical
    /// brightness gradient). Content-driven rotation verification is therefore
    /// done against ground truth instead of scene assumptions:
    /// `tests/abi_layout.rs` compares the promoted output byte-for-byte against
    /// LibRaw's OWN rotation (`user_flip=k` → `dcraw_make_mem_image`, which
    /// applies `flip_index`), for every dcraw flip 0..=7 and for this fixture
    /// end-to-end.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn aircraft_portrait_fixture_applies_exif_orientation() {
        // The fixture carries dcraw flip 5, which is EXIF orientation **8**
        // ("Rotate 270 CW", confirmed via exiftool), NOT orientation 5. Before
        // REVIEW-RAW-FLIP-1 the raw flip value was persisted 1:1 as
        // `orientation` and the promotion step applied a transpose instead of
        // the correct 90° CCW rotation (dimensions matched by coincidence,
        // pixel content was mirrored).
        let bytes = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");
        let image = decode_bytes(bytes, "aircraft-portrait.cr3").unwrap();
        assert_eq!(image.metadata.orientation, 8);
        assert_eq!((image.frame.width, image.frame.height), (4024, 6032));
        assert_eq!((image.metadata.width, image.metadata.height), (4024, 6032));
        assert_eq!(image.frame.pixels.len(), 4024 * 6032 * 4);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn aircraft_fixtures_expose_exif_lens_model() {
        // REVIEW-RAW-N2: both committed CR3 fixtures carry the EXIF LensModel
        // tag 0xA434 with value "RF200-800mm F6.3-9 IS USM" (verified with
        // exiftool). LibRaw parses that tag into `imgdata.lens.Lens`, which
        // `RawMetadata.lens` must now expose verbatim instead of a constant
        // `None`. Pins the EXIF-first source priority end-to-end.
        let fixtures: [(&str, &[u8]); 2] = [
            (
                "aircraft-landscape.cr3",
                include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3"),
            ),
            (
                "aircraft-portrait.cr3",
                include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3"),
            ),
        ];
        for (name, bytes) in fixtures {
            let image = decode_bytes(bytes, name).unwrap();
            assert_eq!(
                image.metadata.lens.as_deref(),
                Some("RF200-800mm F6.3-9 IS USM"),
                "fixture {name} must report its EXIF lens model"
            );
        }
    }

    #[test]
    fn dcraw_flip_maps_to_verified_exif_orientation_table() {
        // Verified against LibRaw 0.22.2: flip_index() bit semantics and the
        // inverse "50132467" lookup in src/metadata/tiff.cpp.
        let expected = [
            (0, 1), // identity
            (1, 2), // mirror horizontal
            (2, 4), // mirror vertical
            (3, 3), // rotate 180°
            (4, 5), // transpose (main diagonal)
            (5, 8), // rotate 90° CCW (= 270° CW)
            (6, 6), // rotate 90° CW
            (7, 7), // anti-transpose (anti-diagonal)
        ];
        for (flip, orientation) in expected {
            assert_eq!(
                dcraw_flip_to_exif_orientation(flip),
                orientation,
                "dcraw flip {flip} must map to EXIF orientation {orientation}"
            );
        }
    }

    #[test]
    fn dcraw_flip_out_of_range_maps_to_no_rotation() {
        // After identify() LibRaw only reports 0..=7; unknown flips are reset
        // to 0 (= no rotation). Anything else must defensively behave the same
        // way instead of wrapping into a bogus orientation.
        for flip in [-5i32, -1, 8, 9, 90, 180, 270, i32::MAX, i32::MIN] {
            assert_eq!(
                dcraw_flip_to_exif_orientation(flip),
                1,
                "out-of-range flip {flip} must map to EXIF orientation 1"
            );
        }
    }

    /// Round-trip through LibRaw's own inverse table:
    /// `src/metadata/tiff.cpp` maps EXIF orientation → flip via the string
    /// `"50132467"[orientation & 7]`. Our translation must be its exact
    /// inverse — this pins the table to LibRaw's semantics, not to a copy of
    /// itself.
    #[test]
    fn exif_orientation_round_trips_through_libraw_inverse_table() {
        const LIBRAW_ORIENTATION_TO_FLIP: [u8; 8] = [5, 0, 1, 3, 2, 4, 6, 7]; // "50132467"
        for orientation in 1u8..=8 {
            let flip = LIBRAW_ORIENTATION_TO_FLIP[(orientation & 7) as usize];
            assert_eq!(
                dcraw_flip_to_exif_orientation(i32::from(flip)),
                orientation,
                "round trip failed for EXIF orientation {orientation}"
            );
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn wasm_is_explicitly_unsupported() {
        assert!(matches!(
            decode_bytes(b"raw", "x.cr2"),
            Err(RawError::UnsupportedPlatform)
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn libraw_version_is_present_and_well_formed() {
        let version = libraw_version().expect("native LibRaw should report a version");
        assert!(!version.is_empty(), "version must not be empty");
        let parts = version.split('.').collect::<Vec<_>>();
        assert!(
            parts.len() >= 2,
            "version should contain at least major.minor, got `{version}`"
        );
        assert!(
            parts.iter().all(|part| !part.is_empty()),
            "version parts must not be empty, got `{version}`"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn libraw_decode_version_carries_decode_generation() {
        let decode_version = libraw_decode_version();
        assert!(
            decode_version.ends_with("+luminaabi3"),
            "decode identity must carry the current decode generation suffix, got `{decode_version}`"
        );
        assert!(
            decode_version.starts_with("0.22."),
            "native decode identity should start with the linked version, got `{decode_version}`"
        );
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn libraw_decode_version_falls_back_to_unknown_with_generation() {
        assert_eq!(libraw_decode_version(), "unknown+luminaabi3");
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn libraw_version_is_none_on_wasm() {
        assert!(libraw_version().is_none());
    }
}
