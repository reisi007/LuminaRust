//! Regression suite for REVIEW-RAW-ABI-1 / WTREE-REVIEW-1.
//!
//! Pins the contract between LuminaRust and the vendored `libraw-sys`
//! bindings against the linked LibRaw 0.22 ABI:
//!
//! 1. the Rust-side layout constants exported by `libraw_sys::abi` match the
//!    hand-written structs (mirrors the compile-time gate as a runtime check);
//! 2. member offsets measured through a LIVE handle equal the pinned table
//!    (this is what the former diagnostic-only `probe_flip.rs` explored);
//! 3. writing `params.user_flip = 0` really lands in `params` — it forces
//!    `sizes.flip` to 0 via `libraw_adjust_sizes_info_only` and leaves
//!    unrelated state (e.g. `idata.make`) untouched. Before the ABI fix this
//!    write corrupted the makernotes region and was silently ignored;
//! 4. EXIF/matrix metadata decoded from the committed CR3 fixtures is finite,
//!    positive and non-empty (garbage reads used to flow into Sidecars).
#![cfg(not(target_arch = "wasm32"))]

use libraw_sys as raw;
use libraw_sys::abi;
use std::os::raw::c_void;

const LANDSCAPE: &[u8] = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
const PORTRAIT: &[u8] = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");

struct Handle(*mut raw::libraw_data_t);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { raw::libraw_close(self.0) };
        }
    }
}

fn open(bytes: &[u8]) -> Handle {
    let handle = Handle(unsafe { raw::libraw_init(raw::LIBRAW_OPTIONS_NONE) });
    assert!(!handle.0.is_null(), "libraw_init failed");
    // SAFETY: `bytes` outlives the call; LibRaw copies into its own buffers.
    let code =
        unsafe { raw::libraw_open_buffer(handle.0, bytes.as_ptr() as *mut c_void, bytes.len()) };
    assert_eq!(code, raw::LIBRAW_SUCCESS, "open_buffer failed");
    handle
}

fn c_text(value: &[std::os::raw::c_char]) -> String {
    let bytes: Vec<u8> = value.iter().map(|byte| *byte as u8).collect();
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

#[test]
fn abi_constants_match_rust_struct_layout() {
    use std::mem::{offset_of, size_of};

    assert_eq!(size_of::<raw::libraw_data_t>(), abi::SIZEOF_libraw_data_t);
    assert_eq!(
        offset_of!(raw::libraw_data_t, params),
        abi::OFFSET_libraw_data_t_params
    );
    assert_eq!(
        offset_of!(raw::libraw_data_t, color),
        abi::OFFSET_libraw_data_t_color
    );
    assert_eq!(
        offset_of!(raw::libraw_data_t, other),
        abi::OFFSET_libraw_data_t_other
    );
    assert_eq!(
        offset_of!(raw::libraw_output_params_t, user_flip),
        abi::OFFSET_libraw_output_params_t_user_flip
    );
}

#[test]
fn live_handle_member_offsets_match_pinned_abi() {
    let handle = open(PORTRAIT);
    let base = handle.0 as usize;
    let data = unsafe { &*handle.0 };

    let offset_in_data = |member: usize| member - base;

    assert_eq!(
        offset_in_data(&data.sizes as *const _ as usize),
        abi::OFFSET_libraw_data_t_sizes
    );
    assert_eq!(
        offset_in_data(&data.idata as *const _ as usize),
        abi::OFFSET_libraw_data_t_idata
    );
    assert_eq!(
        offset_in_data(&data.params as *const _ as usize),
        abi::OFFSET_libraw_data_t_params
    );
    assert_eq!(
        offset_in_data(&data.rawparams as *const _ as usize),
        abi::OFFSET_libraw_data_t_rawparams
    );
    assert_eq!(
        offset_in_data(&data.color as *const _ as usize),
        abi::OFFSET_libraw_data_t_color
    );
    assert_eq!(
        offset_in_data(&data.other as *const _ as usize),
        abi::OFFSET_libraw_data_t_other
    );
    assert_eq!(
        offset_in_data(&data.thumbnail as *const _ as usize),
        abi::OFFSET_libraw_data_t_thumbnail
    );
    assert_eq!(
        offset_in_data(&data.rawdata as *const _ as usize),
        abi::OFFSET_libraw_data_t_rawdata
    );
    assert_eq!(
        offset_in_data(&data.parent_class as *const _ as usize),
        abi::OFFSET_libraw_data_t_parent_class
    );

    // Field-level spot checks relative to their parent struct.
    let params_base = &data.params as *const _ as usize;
    assert_eq!(
        (&data.params.user_flip as *const _ as usize) - params_base,
        abi::OFFSET_libraw_output_params_t_user_flip
    );
    assert_eq!(
        (&data.params.use_camera_wb as *const _ as usize) - params_base,
        abi::OFFSET_libraw_output_params_t_use_camera_wb
    );

    let color_base = &data.color as *const _ as usize;
    assert_eq!(
        (&data.color.cam_mul as *const _ as usize) - color_base,
        abi::OFFSET_libraw_colordata_t_cam_mul
    );
    assert_eq!(
        (&data.color.profile_length as *const _ as usize) - color_base,
        abi::OFFSET_libraw_colordata_t_profile_length
    );
}

/// Acceptance proof for REVIEW-RAW-ABI-1: after `params.user_flip = 0`,
/// `libraw_adjust_sizes_info_only` must set `sizes.flip` to 0 — i.e. the write
/// landed in the real output-params block instead of corrupting makernotes.
#[test]
fn user_flip_write_lands_and_forces_no_rotation() {
    let handle = open(PORTRAIT);
    let data = unsafe { &mut *handle.0 };

    // The portrait fixture carries dcraw flip 5 (LibRaw's coding for
    // EXIF orientation 8, "Rotate 270 CW").
    let flip_before = data.sizes.flip;
    assert_eq!(
        flip_before, 5,
        "portrait fixture must carry its raw dcraw flip"
    );

    let make_before = c_text(&data.idata.make);
    let cam_mul_before = data.color.cam_mul;

    data.params.user_flip = 0;
    data.params.use_camera_wb = 1;
    data.params.use_camera_matrix = 1;

    let code = unsafe { raw::libraw_adjust_sizes_info_only(handle.0) };
    assert_eq!(code, raw::LIBRAW_SUCCESS, "adjust_sizes_info_only failed");

    assert_eq!(
        data.sizes.flip, 0,
        "user_flip=0 did not take effect — ABI drift?"
    );
    assert_eq!(data.params.user_flip, 0, "user_flip readback failed");
    assert_eq!(
        data.params.use_camera_wb, 1,
        "use_camera_wb readback failed"
    );
    assert_eq!(
        data.params.use_camera_matrix, 1,
        "use_camera_matrix readback failed"
    );

    // The parameter writes must not have smashed neighbouring state.
    assert_eq!(
        c_text(&data.idata.make),
        make_before,
        "idata.make corrupted by param write"
    );
    assert_eq!(
        data.color.cam_mul, cam_mul_before,
        "color.cam_mul corrupted by param write"
    );
}

/// EXIF and colour-matrix fields must be real values for the committed CR3
/// fixture; before the ABI pin they were read from wrong offsets and reached
/// the Sidecar as garbage.
#[test]
fn exif_and_matrix_metadata_are_sane_for_cr3_fixture() {
    let image =
        lumina_raw::decode_bytes(LANDSCAPE, "aircraft-landscape.cr3").expect("decode failed");

    let make = image
        .metadata
        .camera_make
        .clone()
        .expect("camera_make missing");
    assert!(!make.is_empty(), "camera_make empty");

    let model = image
        .metadata
        .camera_model
        .clone()
        .expect("camera_model missing");
    assert!(!model.is_empty(), "camera_model empty");
    assert!(
        model.chars().all(|c| c.is_ascii_graphic() || c == ' '),
        "camera_model contains garbage bytes: {model:?}"
    );

    let iso = image.metadata.iso.expect("iso missing");
    let shutter = image.metadata.shutter.expect("shutter missing");
    let aperture = image.metadata.aperture.expect("aperture missing");
    let focal = image.metadata.focal_length.expect("focal_length missing");
    for (name, value) in [
        ("iso", iso),
        ("shutter", shutter),
        ("aperture", aperture),
        ("focal", focal),
    ] {
        assert!(
            value.is_finite() && value > 0.0,
            "{name} is not positive/finite: {value}"
        );
    }

    for (row, matrix_row) in image.metadata.camera_matrix.iter().enumerate() {
        for (col, value) in matrix_row.iter().enumerate() {
            assert!(
                value.is_finite(),
                "rgb_cam[{row}][{col}] not finite: {value}"
            );
        }
    }
    for (index, value) in image.metadata.camera_white_balance.iter().enumerate() {
        assert!(
            value.is_finite() && *value > 0.0,
            "cam_mul[{index}] not positive/finite: {value}"
        );
    }
    for (index, value) in image.metadata.pre_multipliers.iter().enumerate() {
        assert!(
            value.is_finite() && *value >= 0.0,
            "pre_mul[{index}] not finite/non-negative: {value}"
        );
    }
    // The first three multipliers (R, G, B) must be real positive values.
    for (index, value) in image.metadata.pre_multipliers[..3].iter().enumerate() {
        assert!(*value > 0.0, "pre_mul[{index}] not positive: {value}");
    }

    if let Some(profile) = &image.metadata.icc_profile {
        assert!(!profile.is_empty(), "icc_profile present but empty");
    }
}

/// End-to-end: LibRaw now honours `user_flip = 0` (unrotated output) and the
/// promotion step applies the captured orientation itself. The fixture's raw
/// dcraw flip 5 translates to EXIF orientation **8** (REVIEW-RAW-FLIP-1), so
/// the decoded portrait frame comes out rotated to 4024×6032 with
/// orientation 8.
#[test]
fn portrait_decode_applies_orientation_after_user_flip_zero() {
    let image = lumina_raw::decode_bytes(PORTRAIT, "aircraft-portrait.cr3").expect("decode failed");
    assert_eq!(image.metadata.orientation, 8);
    assert_eq!((image.frame.width, image.frame.height), (4024, 6032));
    assert_eq!((image.metadata.width, image.metadata.height), (4024, 6032));
    assert_eq!(image.frame.pixels.len(), 4024 * 6032 * 4);

    let landscape =
        lumina_raw::decode_bytes(LANDSCAPE, "aircraft-landscape.cr3").expect("decode failed");
    assert_eq!(landscape.metadata.orientation, 1);
    assert_eq!(
        (landscape.frame.width, landscape.frame.height),
        (6032, 4024)
    );
}

// ---------------------------------------------------------------------------
// REVIEW-RAW-FLIP-1: empirical verification of the dcraw-flip → EXIF-orientation
// translation against the LINKED LibRaw 0.22.2 binary (not just its sources).
//
// Ground truth: when `params.user_flip = k` (k in 0..=7) is set before
// processing, `raw2image()` assigns `S.flip = k` and `dcraw_make_mem_image`
// applies LibRaw's own `flip_index` permutation (with swapped output
// dimensions for flip & 4). Comparing that output byte-for-byte against our
// translation proves the whole chain — translation table AND promotion
// mapping — against the reference decoder itself.
// ---------------------------------------------------------------------------

/// Final LibRaw mem image (rotation already applied by LibRaw when
/// `user_flip != 0`).
struct MemImage {
    width: usize,
    height: usize,
    colors: usize,
    data: Vec<u8>,
}

/// Decodes `bytes` through the linked LibRaw with an explicit `user_flip`
/// override, mirroring exactly the parameters `lumina_raw::decode_bytes` uses
/// (`use_camera_wb`, `use_camera_matrix`, 8-bit sRGB output, no auto bright),
/// so both runs process identical pixel data.
fn decode_with_user_flip(bytes: &[u8], user_flip: i32) -> MemImage {
    let handle = open(bytes);
    unsafe {
        (*handle.0).params.user_flip = user_flip;
        (*handle.0).params.use_camera_wb = 1;
        (*handle.0).params.use_camera_matrix = 1;
        raw::libraw_set_output_bps(handle.0, 8);
        raw::libraw_set_output_color(handle.0, 1);
        raw::libraw_set_no_auto_bright(handle.0, 0);
    }
    let code = unsafe { raw::libraw_unpack(handle.0) };
    assert_eq!(code, raw::LIBRAW_SUCCESS, "unpack failed");
    let code = unsafe { raw::libraw_dcraw_process(handle.0) };
    assert_eq!(code, raw::LIBRAW_SUCCESS, "dcraw_process failed");
    let mut error = raw::LIBRAW_SUCCESS;
    let processed = unsafe { raw::libraw_dcraw_make_mem_image(handle.0, &mut error) };
    assert!(
        !processed.is_null(),
        "make_mem_image failed with code {error}"
    );
    // SAFETY: `processed` is a valid LibRaw-owned `libraw_processed_image_t`
    // until `libraw_dcraw_clear_mem` below; `bits == 8` is asserted so the
    // element size is one byte.
    let image = unsafe { &*processed };
    assert_eq!(image.bits, 8, "expected 8-bit mem image");
    let length = image.width as usize * image.height as usize * image.colors as usize;
    assert_eq!(
        image.data_size as usize, length,
        "mem image payload size mismatch"
    );
    let data = unsafe { std::slice::from_raw_parts(image.data.as_ptr(), length) }.to_vec();
    let result = MemImage {
        width: image.width as usize,
        height: image.height as usize,
        colors: image.colors as usize,
        data,
    };
    unsafe { raw::libraw_dcraw_clear_mem(processed) };
    result
}

/// Output-pixel → unrotated-source-coordinate mapping derived directly from
/// the CIPA EXIF Orientation value semantics ("0th row / 0th column" table):
///
/// | value | meaning                                   |
/// | ----- | ----------------------------------------- |
/// | 1     | identity                                  |
/// | 2     | mirror horizontal                          |
/// | 3     | rotate 180°                                |
/// | 4     | mirror vertical                            |
/// | 5     | transpose (main diagonal)                  |
/// | 6     | rotate 90° CW                              |
/// | 7     | transverse (anti-diagonal mirror)          |
/// | 8     | rotate 270° CW (= 90° CCW)                 |
fn exif_orientation_source_xy(x: u32, y: u32, w: u32, h: u32, orientation: u8) -> (u32, u32) {
    match orientation {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => (x, y),
    }
}

/// For EVERY dcraw flip 0..=7: LibRaw's own rotation of the portrait fixture
/// must be pixel-for-pixel identical to the unrotated buffer permuted with our
/// translation `dcraw_flip_to_exif_orientation(k)` and the EXIF-spec mapping.
#[test]
fn promotion_matches_libraw_own_rotation_for_every_flip() {
    let unrotated = decode_with_user_flip(PORTRAIT, 0);
    for flip in 0..=7i32 {
        let reference = decode_with_user_flip(PORTRAIT, flip);
        let orientation = lumina_raw::dcraw_flip_to_exif_orientation(flip);
        let swaps_dims = (5..=8).contains(&orientation);
        assert_eq!(
            (reference.width, reference.height),
            if swaps_dims {
                (unrotated.height, unrotated.width)
            } else {
                (unrotated.width, unrotated.height)
            },
            "flip {flip}: LibRaw output dims must follow the transpose bit"
        );
        let colors = reference.colors;
        assert_eq!(colors, unrotated.colors, "flip {flip}: channel count drift");
        for y in 0..reference.height {
            for x in 0..reference.width {
                let (sx, sy) = exif_orientation_source_xy(
                    x as u32,
                    y as u32,
                    unrotated.width as u32,
                    unrotated.height as u32,
                    orientation,
                );
                let src = ((sy as usize * unrotated.width) + sx as usize) * colors;
                let dst = (y * reference.width + x) * colors;
                assert_eq!(
                    reference.data[dst..dst + colors],
                    unrotated.data[src..src + colors],
                    "flip {flip} → EXIF {orientation}: pixel mismatch at ({x}, {y})"
                );
            }
        }
    }
}

/// Content-driven end-to-end proof for the portrait fixture: the production
/// decode (`decode_bytes`, i.e. LibRaw unrotated + our promotion from
/// translated EXIF orientation 8) must be pixel-for-pixel what LibRaw ITSELF
/// produces when told to rotate by the fixture's own dcraw flip
/// (`user_flip = 5`). Every pixel of the real scene is compared, which pins
/// the rotation content-driven without relying on scene-brightness
/// assumptions (the former luminance-band heuristic measured FALSE on this
/// fixture: top ≈ 111.0 vs bottom ≈ 118.9 mean luminance).
#[test]
fn portrait_decode_is_byte_identical_to_libraw_own_rotation() {
    let reference = decode_with_user_flip(PORTRAIT, 5);
    assert_eq!(reference.width, 4024);
    assert_eq!(reference.height, 6032);

    let image = lumina_raw::decode_bytes(PORTRAIT, "aircraft-portrait.cr3").expect("decode failed");
    assert_eq!(image.metadata.orientation, 8);
    assert_eq!(
        (image.frame.width as usize, image.frame.height as usize),
        (reference.width, reference.height)
    );

    let colors = reference.colors;
    for y in 0..reference.height {
        for x in 0..reference.width {
            let src = (y * reference.width + x) * colors;
            let dst = (y * image.frame.width as usize + x) * 4;
            assert_eq!(
                &image.frame.pixels[dst..dst + 3],
                &reference.data[src..src + colors],
                "production decode diverges from LibRaw's own rotation at ({x}, {y})"
            );
            assert_eq!(image.frame.pixels[dst + 3], 255, "alpha must stay opaque");
        }
    }
}
