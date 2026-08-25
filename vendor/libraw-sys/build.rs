extern crate pkg_config;
extern crate cc;

use std::fs;
use std::path::{Path, PathBuf};

/// The vendored bindings in `src/lib.rs` are hand-written and pinned to the
/// LibRaw 0.22 ABI family (locked against 0.22.2). Every size/offset below was
/// measured empirically (sizeof/offsetof probe compiled against the LibRaw
/// 0.22.2 headers). This table is the SINGLE SOURCE OF TRUTH for three
/// enforcement layers:
///
/// 1. It is emitted as a generated C file containing `_Static_assert`s that
///    are compiled here against the ACTUAL linked LibRaw headers — a layout
///    mismatch aborts the build loudly.
/// 2. The same values are emitted as Rust constants (`OUT_DIR/libraw_layout.rs`,
///    re-exported as `libraw_sys::abi`) that pin the Rust struct definitions
///    via compile-time `mem::size_of` / `mem::offset_of!` asserts.
/// 3. The generated C file also exposes the measured values as a table so
///    `tests/sizes.rs` can cross-check Rust vs. C at test time.
const PINNED_LIBRAW_FAMILY: &str = "0.22";

struct LayoutEntry {
    /// Rust type identifier (used for generated constant names).
    rust_type: &'static str,
    /// C type expression used in sizeof/offsetof.
    c_type: &'static str,
    /// `None` -> sizeof entry, `Some(field)` -> offsetof entry.
    field: Option<&'static str>,
    value: usize,
}

const fn sz(rust_type: &'static str, c_type: &'static str, value: usize) -> LayoutEntry {
    LayoutEntry { rust_type, c_type, field: None, value }
}

const fn off(rust_type: &'static str, c_type: &'static str, field: &'static str, value: usize) -> LayoutEntry {
    LayoutEntry { rust_type, c_type, field: Some(field), value }
}

#[rustfmt::skip]
static LAYOUT: &[LayoutEntry] = &[
    // ---- sizes ---------------------------------------------------------
    sz("libraw_data_t",                 "libraw_data_t",                 381_576),
    sz("libraw_image_sizes_t",          "libraw_image_sizes_t",          184),
    sz("libraw_iparams_t",              "libraw_iparams_t",              440),
    sz("libraw_lensinfo_t",             "libraw_lensinfo_t",             1_296),
    sz("libraw_nikonlens_t",            "libraw_nikonlens_t",            8),
    sz("libraw_dnglens_t",              "libraw_dnglens_t",              16),
    sz("libraw_makernotes_lens_t",      "libraw_makernotes_lens_t",      736),
    sz("libraw_makernotes_t",           "libraw_makernotes_t",           3_160),
    sz("libraw_shootinginfo_t",         "libraw_shootinginfo_t",         142),
    sz("libraw_output_params_t",        "libraw_output_params_t",        304),
    sz("libraw_raw_unpack_params_t",    "libraw_raw_unpack_params_t",    48),
    sz("ph1_t",                         "struct ph1_t",                  36),
    sz("libraw_dng_color_t",            "libraw_dng_color_t",            168),
    sz("libraw_dng_levels_t",           "libraw_dng_levels_t",           32_976),
    sz("libraw_P1_color_t",             "libraw_P1_color_t",             36),
    sz("libraw_colordata_t",            "libraw_colordata_t",            187_088),
    sz("libraw_gps_info_t",             "libraw_gps_info_t",             48),
    sz("libraw_imgother_t",             "libraw_imgother_t",             800),
    sz("libraw_thumbnail_t",            "libraw_thumbnail_t",            24),
    sz("libraw_thumbnail_item_t",       "libraw_thumbnail_item_t",       32),
    sz("libraw_thumbnail_list_t",       "libraw_thumbnail_list_t",       264),
    sz("libraw_internal_output_params_t","libraw_internal_output_params_t",16),
    sz("libraw_rawdata_t",              "libraw_rawdata_t",              187_800),
    sz("libraw_processed_image_t",      "libraw_processed_image_t",      20),
    sz("libraw_decoder_info_t",         "libraw_decoder_info_t",         16),
    // ---- member offsets inside libraw_data_t ---------------------------
    off("libraw_data_t", "libraw_data_t", "image",           0),
    off("libraw_data_t", "libraw_data_t", "sizes",           8),
    off("libraw_data_t", "libraw_data_t", "idata",           192),
    off("libraw_data_t", "libraw_data_t", "lens",            632),
    off("libraw_data_t", "libraw_data_t", "makernotes",      1_928),
    off("libraw_data_t", "libraw_data_t", "shootinginfo",    5_088),
    off("libraw_data_t", "libraw_data_t", "params",          5_232),
    off("libraw_data_t", "libraw_data_t", "rawparams",       5_536),
    off("libraw_data_t", "libraw_data_t", "progress_flags",  5_584),
    off("libraw_data_t", "libraw_data_t", "process_warnings",5_588),
    off("libraw_data_t", "libraw_data_t", "color",           5_592),
    off("libraw_data_t", "libraw_data_t", "other",           192_680),
    off("libraw_data_t", "libraw_data_t", "thumbnail",       193_480),
    off("libraw_data_t", "libraw_data_t", "thumbs_list",     193_504),
    off("libraw_data_t", "libraw_data_t", "rawdata",         193_768),
    off("libraw_data_t", "libraw_data_t", "parent_class",    381_568),
    // ---- fields LuminaRust reads/writes ---------------------------------
    off("libraw_image_sizes_t",  "libraw_image_sizes_t",   "flip",             32),
    off("libraw_iparams_t",      "libraw_iparams_t",       "make",             4),
    off("libraw_iparams_t",      "libraw_iparams_t",       "model",            68),
    off("libraw_output_params_t","libraw_output_params_t", "use_camera_wb",    152),
    off("libraw_output_params_t","libraw_output_params_t", "use_camera_matrix",156),
    off("libraw_output_params_t","libraw_output_params_t", "user_flip",        212),
    off("libraw_output_params_t","libraw_output_params_t", "output_bps",       200),
    off("libraw_output_params_t","libraw_output_params_t", "output_color",     160),
    off("libraw_output_params_t","libraw_output_params_t", "no_auto_bright",   256),
    off("libraw_colordata_t",    "libraw_colordata_t",     "curve",            0),
    off("libraw_colordata_t",    "libraw_colordata_t",     "cam_mul",          147_652),
    off("libraw_colordata_t",    "libraw_colordata_t",     "pre_mul",          147_668),
    off("libraw_colordata_t",    "libraw_colordata_t",     "rgb_cam",          147_780),
    off("libraw_colordata_t",    "libraw_colordata_t",     "profile",          148_264),
    off("libraw_colordata_t",    "libraw_colordata_t",     "profile_length",   148_272),
    off("libraw_imgother_t",     "libraw_imgother_t",      "iso_speed",        0),
    off("libraw_imgother_t",     "libraw_imgother_t",      "shutter",          4),
    off("libraw_imgother_t",     "libraw_imgother_t",      "aperture",         8),
    off("libraw_imgother_t",     "libraw_imgother_t",      "focal_len",        12),
    off("libraw_imgother_t",     "libraw_imgother_t",      "timestamp",        16),
    off("libraw_imgother_t",     "libraw_imgother_t",      "desc",             204),
    off("libraw_imgother_t",     "libraw_imgother_t",      "artist",           716),
    off("libraw_rawdata_t",      "libraw_rawdata_t",       "color",            712),
    off("libraw_processed_image_t","libraw_processed_image_t","height",         4),
    off("libraw_processed_image_t","libraw_processed_image_t","width",          6),
    off("libraw_processed_image_t","libraw_processed_image_t","colors",         8),
    off("libraw_processed_image_t","libraw_processed_image_t","bits",           10),
    off("libraw_processed_image_t","libraw_processed_image_t","data_size",      12),
    off("libraw_processed_image_t","libraw_processed_image_t","data",           16),
];

fn entry_label(entry: &LayoutEntry) -> String {
    match entry.field {
        Some(field) => format!("offsetof({}, {})", entry.c_type, field),
        None => format!("sizeof({})", entry.c_type),
    }
}

fn const_name(entry: &LayoutEntry) -> String {
    match entry.field {
        Some(field) => format!("OFFSET_{}_{}", entry.rust_type, field),
        None => format!("SIZEOF_{}", entry.rust_type),
    }
}

fn generate_rust_constants(out_dir: &Path) {
    let mut file = String::new();
    file.push_str("// @generated by vendor/libraw-sys/build.rs — do not edit.\n");
    file.push_str(&format!(
        "// Pinned LibRaw ABI family {} (values measured against 0.22.2).\n\n",
        PINNED_LIBRAW_FAMILY
    ));
    file.push_str(&format!("pub const ENTRY_COUNT: usize = {};\n", LAYOUT.len()));
    file.push_str(&format!(
        "pub const PINNED_LIBRAW_FAMILY: &str = \"{}\";\n",
        PINNED_LIBRAW_FAMILY
    ));
    file.push_str("pub const MEASURED_LIBRAW_VERSION: &str = \"0.22.2\";\n\n");
    for entry in LAYOUT {
        file.push_str(&format!(
            "pub const {}: usize = {};\n",
            const_name(entry),
            entry.value
        ));
    }
    file.push_str("\n/// Human-readable labels parallel to [`LAYOUT_VALUES`] (test support).\n");
    file.push_str("#[doc(hidden)]\npub const LAYOUT_LABELS: [&str; ENTRY_COUNT] = [\n");
    for entry in LAYOUT {
        file.push_str(&format!("    \"{}\",\n", entry_label(entry)));
    }
    file.push_str("];\n\n#[doc(hidden)]\npub const LAYOUT_VALUES: [usize; ENTRY_COUNT] = [\n");
    for entry in LAYOUT {
        file.push_str(&format!("    {},\n", entry.value));
    }
    file.push_str("];\n");

    let path = out_dir.join("libraw_layout.rs");
    fs::write(&path, file).unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
}

/// Emits a C translation unit that (a) `_Static_assert`s every pinned value
/// against the real LibRaw headers and (b) exposes the same values through a
/// runtime-accessible table for `tests/sizes.rs`.
fn generate_c_layout_gate(out_dir: &Path) -> PathBuf {
    let mut file = String::new();
    file.push_str("/* @generated by vendor/libraw-sys/build.rs — do not edit. */\n");
    file.push_str("#include <stddef.h>\n#include <libraw.h>\n\n");
    for entry in LAYOUT {
        let expr = match entry.field {
            Some(field) => format!("offsetof({}, {})", entry.c_type, field),
            None => format!("sizeof({})", entry.c_type),
        };
        file.push_str(&format!(
            "_Static_assert({} == {}, \"libraw-sys pinned-ABI drift: expected {} == {} (LibRaw family {}); linked LibRaw headers do not match the hand-written bindings\");\n",
            expr, entry.value, entry_label(entry), entry.value, PINNED_LIBRAW_FAMILY
        ));
    }
    file.push_str("\nsize_t lumina_libraw_layout_value(unsigned index) {\n");
    file.push_str("    static const size_t values[] = {\n");
    for entry in LAYOUT {
        let expr = match entry.field {
            Some(field) => format!("offsetof({}, {})", entry.c_type, field),
            None => format!("sizeof({})", entry.c_type),
        };
        file.push_str(&format!("        {},\n", expr));
    }
    file.push_str("    };\n");
    file.push_str(&format!(
        "    return index < (unsigned)(sizeof(values) / sizeof(values[0])) ? values[index] : (size_t)-1;\n}}\n"
    ));
    file.push_str(&format!(
        "unsigned lumina_libraw_layout_count(void) {{ return {}u; }}\n",
        LAYOUT.len()
    ));

    let path = out_dir.join("libraw_layout_gate.c");
    fs::write(&path, file).unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
    path
}

fn main() {
    let libraw = pkg_config::Config::new()
        .cargo_metadata(false)
        .atleast_version("0.22.0")
        .find("libraw_r")
        .unwrap_or_else(|error| {
            panic!(
                "libraw-sys requires libraw >= {} (pkg-config: {}). The hand-written \
                 bindings in src/lib.rs are pinned to the {} ABI family.",
                "0.22.0", error, PINNED_LIBRAW_FAMILY
            )
        });
    for path in &libraw.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    for library in &libraw.libs {
        println!("cargo:rustc-link-lib={}", link_name(library, &target_os));
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    generate_rust_constants(&out_dir);
    let gate = generate_c_layout_gate(&out_dir);

    // Compiling the gate aborts the build with the _Static_assert messages when
    // the linked LibRaw does not match the pinned 0.22 layout.
    let mut cc = cc::Build::new();
    for path in &libraw.include_paths {
        cc.include(path.to_str().unwrap());
    }
    cc.file(gate).compile("libsizes.a");
}

// Only Apple's macOS runtime needs this correction. Other targets retain the
// link name supplied by pkg-config.
fn link_name<'a>(library: &'a str, target_os: &str) -> &'a str {
    if target_os == "macos" && library == "stdc++" { "c++" } else { library }
}

#[cfg(test)]
mod tests {
    use super::link_name;

    #[test]
    fn only_macos_maps_stdcpp_to_libcxx() {
        assert_eq!(link_name("stdc++", "macos"), "c++");
        assert_eq!(link_name("stdc++", "linux"), "stdc++");
        assert_eq!(link_name("stdc++", "freebsd"), "stdc++");
    }
}
