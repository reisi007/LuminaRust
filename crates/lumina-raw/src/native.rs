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

fn strerror(code: i32) -> String {
    unsafe { CStr::from_ptr(raw::libraw_strerror(code)) }
        .to_string_lossy()
        .into_owned()
}

fn error(operation: &'static str, code: i32) -> RawError {
    let message = strerror(code);
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

/// Where the RAW input comes from. `Buffer` keeps the byte-oriented API
/// on `libraw_open_buffer`; `File` lets LibRaw stream from its own
/// datastream via `libraw_open_file` (R2-RAW-02) instead of materialising
/// a full-file heap copy first.
pub(super) enum DecodeInput<'a> {
    Buffer(&'a [u8]),
    File(std::path::PathBuf),
}

/// Initialises a fresh LibRaw handle and opens `input` on it.
///
/// Shared by the full decode ([`decode_with_options`]) and the
/// metadata-only path ([`read_metadata_with_input`]) so both entry points
/// keep identical open semantics — including the `RawError::Io` error
/// shape for plain filesystem failures of `decode_file`/`read_metadata`.
fn open_handle(input: &DecodeInput<'_>) -> Result<Handle, RawError> {
    let file_display_path = match input {
        DecodeInput::File(path) => Some(path.display().to_string()),
        DecodeInput::Buffer(_) => None,
    };
    let handle = Handle(unsafe { raw::libraw_init(raw::LIBRAW_OPTIONS_NONE) });
    if handle.0.is_null() {
        return Err(RawError::InvalidData("LibRaw handle"));
    }
    let code = match input {
        // SAFETY: LibRaw's `open_buffer` reads the input bytes and never
        // mutates them; `bytes` outlives this call (it is a function
        // parameter dropped only after `handle`, whose `Drop` closes the
        // decoder). Handing the const slice pointer to the `*mut c_void`
        // FFI argument therefore lets us skip an otherwise redundant
        // full-file copy of the RAW (F-074-A2) without giving LibRaw
        // writable access to caller memory.
        DecodeInput::Buffer(bytes) => unsafe {
            raw::libraw_open_buffer(handle.0, bytes.as_ptr() as *mut c_void, bytes.len())
        },
        // SAFETY: `open_file` copies the path string for the duration of
        // the call and afterwards owns its internal datastream; the
        // `CString` outlives the call and no caller memory stays aliased.
        // Unlike `open_buffer`, LibRaw keeps reading lazily from its own
        // file handle until `libraw_close` (see `Handle::drop`). The
        // `char*` variant expects a native narrow path (UTF-8 on Unix;
        // on Windows this goes through the ANSI code page — acceptable
        // for the current macOS/Linux-first capability matrix).
        DecodeInput::File(path) => {
            match std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
                Ok(c_path) => unsafe { raw::libraw_open_file(handle.0, c_path.as_ptr()) },
                // Interior NUL bytes cannot name a real filesystem entry;
                // reported in the same shape `std::fs::read` failures used
                // to produce before the streaming refactor (R2-RAW-02).
                Err(_) => {
                    return Err(RawError::Io {
                        path: file_display_path.unwrap_or_default(),
                        message: "path contains an interior NUL byte".to_string(),
                    })
                }
            }
        }
    };
    if code != raw::LIBRAW_SUCCESS {
        // Preserve the historical error shape for plain filesystem
        // failures of `decode_file` (missing/unreadable file) so callers
        // keep seeing `RawError::Io` with the path; everything else keeps
        // the richer LibRaw diagnostics.
        if code == raw::LIBRAW_IO_ERROR && file_display_path.is_some() {
            return Err(RawError::Io {
                path: file_display_path.unwrap_or_default(),
                message: strerror(code),
            });
        }
        return Err(error("opening input", code));
    }
    Ok(handle)
}

/// Captures the source-level metadata (EXIF identity, colour context,
/// ICC profile) and the EXIF orientation from an OPENED handle, before
/// any unpack/processing happens.
///
/// Returns the [`RawMetadata`] with width/height still zero (they are
/// finalised later: after `adjust_sizes_info_only` in the decode path,
/// respectively from the adjusted sizes in the metadata-only path) plus
/// the translated EXIF orientation that the pixel promotion step applies.
///
/// Shared verbatim by both paths so full-decode and metadata-only reports
/// can never drift (R2-CLI-04).
fn capture_base_metadata(handle: *mut raw::libraw_data_t) -> Result<(RawMetadata, u8), RawError> {
    let data = unsafe { &*handle };
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
            unsafe { std::slice::from_raw_parts(data.color.profile.cast::<u8>(), length) }.to_vec(),
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
        timestamp: (data.other.timestamp != 0).then_some(data.other.timestamp),
        artist: text(&data.other.artist),
        description: text(&data.other.desc),
        camera_matrix,
        camera_white_balance,
        pre_multipliers,
        icc_profile,
    };
    Ok((metadata, orientation))
}

pub fn decode_with_options(
    input: DecodeInput<'_>,
    options: &RawDecodeOptions,
) -> Result<RawImage, RawError> {
    if let DecodeInput::Buffer(bytes) = &input {
        if bytes.is_empty() {
            return Err(RawError::InvalidData("empty input"));
        }
    }
    if !matches!(options.output_bits, 8 | 16) {
        return Err(RawError::InvalidData("output bit depth"));
    }
    let handle = open_handle(&input)?;
    let (metadata, orientation) = capture_base_metadata(handle.0)?;
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
    metadata.width = frame.width();
    metadata.height = frame.height();
    Ok(RawImage { frame, metadata })
}

/// Metadata-only read (R2-CLI-04): opens the input, captures the source
/// metadata through the SAME helper as the full decode (so both reports
/// cannot drift), then finalises only the geometry.
///
/// LibRaw's progress-order guard requires `libraw_unpack` before
/// `adjust_sizes_info_only` (`CHECK_ORDER_LOW(LIBRAW_PROGRESS_LOAD_RAW)`),
/// so the compressed entropy decode still runs here — see the honest-limit
/// note on the public [`read_metadata`]. Everything after unpack in the
/// decode path (processing parameters, `dcraw_process`, memory image,
/// promotion) is deliberately absent: no pixel-sized allocation happens,
/// so no MemoryBudget gate is required.
///
/// `user_flip` is intentionally NOT touched: it only affects LibRaw's own
/// pixel rotation during processing, which never runs on this path. The
/// captured flip/orientation and the adjusted sizes fully determine the
/// visible output geometry via the same swap rule the promotion step uses
/// for orientations 5..=8 (90°/270° transpose family).
pub(super) fn read_metadata_with_input(input: DecodeInput<'_>) -> Result<RawMetadata, RawError> {
    if let DecodeInput::Buffer(bytes) = &input {
        if bytes.is_empty() {
            return Err(RawError::InvalidData("empty input"));
        }
    }
    let handle = open_handle(&input)?;
    let (mut metadata, orientation) = capture_base_metadata(handle.0)?;
    let code = unsafe { raw::libraw_unpack(handle.0) };
    if code != raw::LIBRAW_SUCCESS {
        return Err(error("unpacking input", code));
    }
    adjust_sizes_checked(handle.0)?;
    let data = unsafe { &*handle.0 };
    let width = u32::from(data.sizes.width);
    let height = u32::from(data.sizes.height);
    if width == 0 || height == 0 {
        return Err(RawError::InvalidData("image dimensions"));
    }
    // Mirror the promotion step's orientation rule exactly: EXIF
    // orientations 5..=8 (the 90°/270° family) swap width and height, so
    // `metadata.width/height` report the same visible geometry a full
    // [`decode_with_options`] would produce (pinned by fixture tests).
    let (out_width, out_height) = if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    };
    metadata.width = out_width;
    metadata.height = out_height;
    Ok(metadata)
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

    ImageFrame::new(out_width, out_height, pixels).map_err(|_| RawError::InvalidData("RGBA frame"))
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
                let (sx, sy) = oriented_source_xy(x as u32, y as u32, width, height, orientation);
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

    ImageFrame::new(out_width, out_height, pixels).map_err(|_| RawError::InvalidData("RGBA frame"))
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
                    frame.pixels(),
                    expected,
                    "8-bit orientation {orientation}, channels {channels}"
                );
                let (ew, eh) = if (5..=8).contains(&orientation) {
                    (h, w)
                } else {
                    (w, h)
                };
                assert_eq!((frame.width(), frame.height()), (ew, eh));
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
            assert_eq!(frame.pixels(), expected, "16-bit orientation {orientation}");
        }
    }

    #[test]
    fn opaque_alpha_for_three_channel_source() {
        // The committed fixtures decode as 3-channel RGB; the promoted RGBA
        // frame must be fully opaque.
        let src = synthetic(3, 4, 3);
        let frame = rgba_from_bytes(&src, 4, 3, 3, 1).unwrap();
        assert!(frame.pixels().iter().skip(3).step_by(4).all(|&a| a == 255));
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
                    let expected = libraw_flip_index_source_xy(flip as u32, x, y, width, height);
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
