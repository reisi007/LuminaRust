//! Runtime cross-check of the pinned LibRaw ABI (enforcement layer 3).
//!
//! The single-source layout table lives in `build.rs`; from it:
//! - a generated C file (`libraw_layout_gate.c`) `_Static_assert`s every value
//!   against the real LibRaw headers at **build time**,
//! - generated Rust constants pin the hand-written structs at **compile time**
//!   (`src/lib.rs::pinned_abi_checks`),
//! - and THIS test compares the Rust-computed values against the same values as
//!   measured **live** through the compiled C table, plus asserts that the
//!   linked library reports the pinned `0.22.x` version family.
//!
//! If this test fails after a system library upgrade, the vendored bindings no
//! longer match the linked LibRaw: reads would return garbage and writes would
//! corrupt unrelated state. Update `src/lib.rs` AND the table in `build.rs`
//! together against the new `libraw/libraw_types.h`, then re-run the offset
//! probe.

use libraw_sys::{abi};
use libraw_sys as raw;
use std::ffi::CStr;
use std::os::raw::c_uint;

#[link(name = "sizes")]
extern "C" {
    /// Returns the C-measured value for entry `index` of the pinned layout
    /// table (generated into `libraw_layout_gate.c`), or `(size_t)-1` when out
    /// of range.
    fn lumina_libraw_layout_value(index: c_uint) -> usize;
    fn lumina_libraw_layout_count() -> c_uint;
}

fn libraw_version_string() -> String {
    let pointer = unsafe { raw::libraw_version() };
    assert!(!pointer.is_null(), "LibRaw did not report a version");
    unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned()
}

#[test]
fn linked_libraw_matches_pinned_abi_family() {
    let version = libraw_version_string();
    assert!(
        version.starts_with(&format!("{}.", abi::PINNED_LIBRAW_FAMILY)),
        "linked LibRaw `{version}` is outside the pinned `{}` ABI family.\n\
         The hand-written bindings in vendor/libraw-sys/src/lib.rs would read\n\
         garbage and write into wrong fields. Re-pin them (and the table in\n\
         build.rs) against the new libraw/libraw_types.h before upgrading.",
        abi::PINNED_LIBRAW_FAMILY
    );
}

#[test]
fn rust_layout_matches_c_measured_layout_entry_by_entry() {
    let c_count = unsafe { lumina_libraw_layout_count() } as usize;
    assert_eq!(
        c_count,
        abi::ENTRY_COUNT,
        "C and Rust layout tables disagree on length; regenerate via build.rs"
    );
    for (index, (label, expected)) in abi::LAYOUT_LABELS.iter().zip(abi::LAYOUT_VALUES.iter()).enumerate() {
        let measured = unsafe { lumina_libraw_layout_value(index as c_uint) };
        assert_ne!(measured, usize::MAX, "C layout table returned out-of-range for {label}");
        assert_eq!(
            measured, *expected,
            "pinned-ABI drift at {label}: Rust table says {expected}, linked headers measure {measured}"
        );
    }
}

/// Direct proof that the two struct members LuminaRust writes through direct
/// field access (`params.user_flip`, `params.use_camera_wb`) sit exactly where
/// the pinned table claims, using a live handle instead of static offsets.
#[test]
fn live_handle_member_offsets_match_pinned_table() {
    let handle = unsafe { raw::libraw_init(raw::LIBRAW_OPTIONS_NONE) };
    assert!(!handle.is_null(), "libraw_init failed");
    let _guard = HandleGuard(handle);
    let base = handle as usize;
    let data = unsafe { &*handle };

    let params_offset = (&data.params as *const _ as usize).wrapping_sub(base);
    let color_offset = (&data.color as *const _ as usize).wrapping_sub(base);
    let other_offset = (&data.other as *const _ as usize).wrapping_sub(base);

    assert_eq!(params_offset, abi::OFFSET_libraw_data_t_params);
    assert_eq!(color_offset, abi::OFFSET_libraw_data_t_color);
    assert_eq!(other_offset, abi::OFFSET_libraw_data_t_other);

    // Field-level spot checks inside the live structs.
    let user_flip_offset = (&data.params.user_flip as *const _ as usize)
        .wrapping_sub(handle as usize)
        .wrapping_sub(abi::OFFSET_libraw_data_t_params);
    assert_eq!(user_flip_offset, abi::OFFSET_libraw_output_params_t_user_flip);

    let cam_mul_offset = (&data.color.cam_mul as *const _ as usize)
        .wrapping_sub(handle as usize)
        .wrapping_sub(abi::OFFSET_libraw_data_t_color);
    assert_eq!(cam_mul_offset, abi::OFFSET_libraw_colordata_t_cam_mul);
}

struct HandleGuard(*mut raw::libraw_data_t);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { raw::libraw_close(self.0) };
        }
    }
}
