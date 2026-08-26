//! Build script for `lumina-lensfun` (F-098-N1).
//!
//! The `native` feature enables the real `liblensfun` FFI. When it is on we
//! locate the system library with `pkg-config` and emit the necessary
//! `-L` search paths and the dynamic link directive
//! (`-llensfun`). We link **dynamically** so the LGPL-3.0 of Lensfun does not
//! extend to the whole work.
//!
//! With `native` off (the default) this script does nothing and no C library
//! is required — the crate compiles to an empty, dependency-free library.
//!
//! # Layout verification (review REVIEW-LENSFUN-N1)
//!
//! The hand-written FFI reads `lfCamera::CropFactor` at a hard-coded byte
//! offset (`4 * pointer size`, see `ffi::lf_camera_crop_factor`). That is a
//! bet on the lensfun ABI, so whenever a C compiler and the lensfun headers
//! are available this script compiles and runs a tiny probe that prints the
//! real `offsetof(lfCamera, CropFactor)` / `sizeof(lfCamera)` reported by the
//! **installed** headers and panics on any mismatch (build-time offset
//! assert). No bindgen, no `cc` crate — just `std::process::Command`.
//!
//! If verification is impossible (no compiler or no headers) we are never
//! silent: a `cargo:warning` is emitted and builds against lensfun versions
//! outside the validated 0.3.x series are refused outright instead of
//! silently misreading struct memory.
//!
//! # Library tracking (review R2-LENS-04)
//!
//! Because the probe validates against whatever library is *installed*, the
//! script also emits `cargo:rerun-if-changed` for the resolved `liblensfun`
//! shared-library file, so a system upgrade re-runs the probe instead of
//! silently keeping a stale ABI verdict (see `track_resolved_library`).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // The `native` feature maps to the `CARGO_FEATURE_NATIVE` env var.
    if env::var("CARGO_FEATURE_NATIVE").is_err() {
        return;
    }

    let output = Command::new("pkg-config")
        .args(["--cflags", "--libs", "lensfun"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            panic!(
                "lumina-lensfun: `pkg-config lensfun` failed (exit {:?}). \
                 The `native` feature requires liblensfun to be installed \
                 (e.g. `brew install lensfun` / `apt-get install liblensfun-dev`).\n\
                 pkg-config stderr: {}",
                o.status.code(),
                stderr.trim()
            );
        }
        Err(e) => {
            panic!(
                "lumina-lensfun: could not invoke `pkg-config` ({e}). \
                 The `native` feature requires liblensfun and pkg-config."
            );
        }
    };

    let flags = String::from_utf8_lossy(&output.stdout);
    let mut linked = false;
    let mut include_dirs: Vec<String> = Vec::new();
    let mut link_dirs: Vec<String> = Vec::new();
    for token in flags.split_whitespace() {
        if let Some(path) = token.strip_prefix("-L") {
            // Strip an optional leading `:` (some pkg-config prints `-L/path`).
            let path = path.strip_prefix(':').unwrap_or(path);
            if !path.is_empty() {
                println!("cargo:rustc-link-search=native={path}");
                link_dirs.push(path.to_owned());
            }
        } else if let Some(path) = token.strip_prefix("-I") {
            let path = path.strip_prefix(':').unwrap_or(path);
            if !path.is_empty() {
                include_dirs.push(path.to_owned());
            }
        } else if token == "-llensfun" {
            println!("cargo:rustc-link-lib=dylib=lensfun");
            linked = true;
        }
        // Remaining `-I` include paths are consumed above for the layout
        // probe; we still do not include the C/C++ header from Rust itself.
    }

    if !linked {
        // Fall back to a plain dynamic link so the eventual linker error is
        // still clear if pkg-config omitted `-llensfun` for some reason.
        println!("cargo:rustc-link-lib=dylib=lensfun");
    }

    verify_lfcamera_layout(&include_dirs);

    // R2-LENS-04: track the *resolved* shared library so this script (and its
    // ABI layout probe) re-runs when the installed liblensfun changes, not
    // only when build.rs itself is edited.
    track_resolved_library(&link_dirs);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=LIBLENSFUN_SYS_DIR");
    println!("cargo:rerun-if-env-changed=CC");
}

/// Shared-library file names to look for, per target OS.
fn lensfun_dylib_names() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["liblensfun.dylib"]
    } else if cfg!(target_os = "windows") {
        &["liblensfun.dll", "lensfun.dll"]
    } else {
        &[
            "liblensfun.so",
            "liblensfun.so.3",
            "liblensfun.so.2",
            "liblensfun.so.1",
            "liblensfun.so.0",
        ]
    }
}

/// Emit `cargo:rerun-if-changed` for the resolved `liblensfun` shared library
/// (review R2-LENS-04).
///
/// Without this, cargo only re-runs the ABI layout probe when `build.rs`
/// changes; a system upgrade of liblensfun (same link path, new library) went
/// unnoticed. We resolve the actual file next to the pkg-config link dirs
/// (falling back to `pkg-config --variable=libdir`) and canonicalise it, so
/// both an upgraded file's mtime and a changed symlink target trigger a
/// rebuild. If the library cannot be located at all we are never silent: a
/// loud warning explains that the probe will not auto-rerun on upgrades.
fn track_resolved_library(link_dirs: &[String]) {
    let candidates = lensfun_dylib_names();

    let mut found: Option<PathBuf> = None;
    'dirs: for dir in link_dirs {
        for name in candidates {
            let candidate = PathBuf::from(dir).join(name);
            if candidate.is_file() {
                found = Some(candidate);
                break 'dirs;
            }
        }
    }

    // Some setups rely on the linker's default search paths instead of an
    // explicit `-L`; pkg-config still knows the libdir in that case.
    if found.is_none() {
        if let Ok(output) = Command::new("pkg-config")
            .args(["--variable=libdir", "lensfun"])
            .output()
        {
            if output.status.success() {
                let dir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if !dir.is_empty() {
                    for name in candidates {
                        let candidate = PathBuf::from(&dir).join(name);
                        if candidate.is_file() {
                            found = Some(candidate);
                            break;
                        }
                    }
                }
            }
        }
    }

    match found {
        Some(lib) => {
            // Canonicalise through Homebrew/Debian-style symlink chains
            // (`liblensfun.dylib -> .../0.3.4/lib/liblensfun.dylib`): stat
            // then tracks the real file, and an upgrade that repoints or
            // replaces it triggers a rebuild of this crate.
            let tracked = std::fs::canonicalize(&lib).unwrap_or(lib);
            println!("cargo:rerun-if-changed={}", tracked.display());
        }
        None => println!(
            "cargo:warning=lumina-lensfun: could not locate the resolved \
             liblensfun shared library next to the pkg-config link path; the \
             ABI layout probe will NOT re-run automatically after a system \
             lensfun upgrade."
        ),
    }
}

/// Build-time assert for the `lfCamera` layout assumption baked into
/// [`ffi::lf_camera_crop_factor`] (review REVIEW-LENSFUN-N1).
///
/// Assumed layout (lensfun 0.3.x, plain POD — the C++ special member
/// functions declared in the header are non-virtual, so there is no vtable):
///
/// ```text
/// Maker | Model | Variant | Mount   -> four pointers (lfMLstr/char*)
/// CropFactor                      -> float   at 4 * pointer_size
/// Score                           -> int     right after it
/// ```
fn verify_lfcamera_layout(include_dirs: &[String]) {
    // Pointer width of the *target* triple (matters when cross-compiling).
    let target_pointer_bytes: usize = env::var("CARGO_CFG_TARGET_POINTER_WIDTH")
        .ok()
        .and_then(|w| w.parse::<usize>().ok())
        .map(|bits| bits / 8)
        .unwrap_or_else(std::mem::size_of::<usize>);
    let expected_offset = 4 * target_pointer_bytes;
    // Four pointers + float + int (8 bytes together), padded up to pointer
    // alignment — identical arithmetic on 32- and 64-bit targets.
    let expected_size = 4 * target_pointer_bytes + 8;

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string()));
    let src = out_dir.join("lf_camera_layout_probe.c");
    let bin = out_dir.join("lf_camera_layout_probe");

    const PROBE_SRC: &str = r#"#include <stddef.h>
#include <stdio.h>
#include <lensfun/lensfun.h>

int main(void)
{
    printf("crop_factor_offset=%zu\n", offsetof(struct lfCamera, CropFactor));
    printf("pointer_size=%zu\n", sizeof(void *));
    printf("struct_size=%zu\n", sizeof(struct lfCamera));
    return 0;
}
"#;

    let run_probe = || -> Result<(usize, usize, usize), String> {
        let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
        std::fs::write(&src, PROBE_SRC).map_err(|e| format!("writing probe source failed: {e}"))?;

        let mut compile = Command::new(&cc);
        compile.arg(&src).arg("-o").arg(&bin);
        for dir in include_dirs {
            compile.arg(format!("-I{dir}"));
        }
        let compiled = compile
            .output()
            .map_err(|e| format!("could not invoke `{cc}`: {e}"))?;
        if !compiled.status.success() {
            return Err(format!(
                "`{cc}` failed: {}",
                String::from_utf8_lossy(&compiled.stderr).trim()
            ));
        }

        let ran = Command::new(&bin)
            .output()
            .map_err(|e| format!("could not run the probe binary: {e}"))?;
        if !ran.status.success() {
            return Err(format!("probe exited with {:?}", ran.status.code()));
        }

        let stdout = String::from_utf8_lossy(&ran.stdout);
        let parse = |prefix: &str| -> Option<usize> {
            stdout
                .lines()
                .find_map(|line| line.strip_prefix(prefix)?.parse().ok())
        };
        let offset = parse("crop_factor_offset=").ok_or("probe printed no crop_factor_offset")?;
        let pointer = parse("pointer_size=").ok_or("probe printed no pointer_size")?;
        let size = parse("struct_size=").ok_or("probe printed no struct_size")?;
        Ok((offset, pointer, size))
    };

    match run_probe() {
        Ok((offset, pointer_size, struct_size)) => {
            if pointer_size != target_pointer_bytes {
                // The probe measured the host ABI but we are building for a
                // different pointer width (cross-compilation): its numbers say
                // nothing about the target, so treat as unverified.
                unverified_layout(&format!(
                    "probe reports pointer size {pointer_size}, but the target \
                     expects {target_pointer_bytes} (cross-compilation?)"
                ));
            } else if offset != expected_offset || struct_size != expected_size {
                panic!(
                    "lumina-lensfun: lfCamera layout mismatch.\n\
                     The installed lensfun headers report:\n\
                       offsetof(struct lfCamera, CropFactor) = {offset}\n\
                       sizeof(struct lfCamera)               = {struct_size}\n\
                     but the hand-written FFI assumes offset {expected_offset} \
                     and size {expected_size}.\n\
                     Update `ffi::lf_camera_crop_factor` in \
                     crates/lumina-lensfun/src/lib.rs after re-verifying the \
                     new struct definition in the installed lensfun header."
                );
            }
            // Verified: the offset assumption holds against the real headers.
        }
        Err(reason) => unverified_layout(&reason),
    }
}

/// Layout could not be verified. Never silent: emit a loud warning and refuse
/// to build against a lensfun version outside the validated 0.3.x series
/// (whose header layout was checked by hand), rather than silently betting on
/// an unknown ABI.
fn unverified_layout(reason: &str) {
    println!(
        "cargo:warning=lumina-lensfun: could NOT verify the \
         lfCamera::CropFactor struct offset against the installed headers \
         ({reason}). Assuming the lensfun 0.3.x layout (four pointers before \
         CropFactor); install a C compiler and the lensfun headers to verify."
    );

    let version = Command::new("pkg-config")
        .args(["--modversion", "lensfun"])
        .output();
    match version {
        Ok(o) if o.status.success() => {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            if !v.starts_with("0.3.") {
                panic!(
                    "lumina-lensfun: refusing to build against lensfun {v}: the \
                     hard-coded lfCamera layout was only verified for the 0.3.x \
                     series and this environment cannot verify it (no usable C \
                     compiler or missing lensfun headers). Install both, or \
                     re-verify the layout for lensfun {v} and update \
                     `ffi::lf_camera_crop_factor` accordingly."
                );
            }
        }
        _ => panic!(
            "lumina-lensfun: could not verify the lfCamera layout and \
             `pkg-config --modversion lensfun` failed as well; refusing to \
             guess the ABI."
        ),
    }
}
