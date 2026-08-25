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

    // The portrait fixture carries EXIF flip 5 (transpose); this is the same
    // value that ends up in `RawMetadata.orientation`.
    let flip_before = data.sizes.flip;
    assert_eq!(flip_before, 5, "portrait fixture must carry its EXIF flip");

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
/// promotion step applies the captured EXIF orientation itself, so the decoded
/// portrait frame still comes out rotated to 4024×6032 with orientation 5.
#[test]
fn portrait_decode_applies_orientation_after_user_flip_zero() {
    let image = lumina_raw::decode_bytes(PORTRAIT, "aircraft-portrait.cr3").expect("decode failed");
    assert_eq!(image.metadata.orientation, 5);
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
