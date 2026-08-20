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

use std::env;
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
    for token in flags.split_whitespace() {
        if let Some(path) = token.strip_prefix("-L") {
            // Strip an optional leading `:` (some pkg-config prints `-L/path`).
            let path = path.strip_prefix(':').unwrap_or(path);
            if !path.is_empty() {
                println!("cargo:rustc-link-search=native={path}");
            }
        } else if token == "-llensfun" {
            println!("cargo:rustc-link-lib=dylib=lensfun");
            linked = true;
        }
        // `-I` include paths are ignored: we hand-write the bindings and do
        // not include the C/C++ header.
    }

    if !linked {
        // Fall back to a plain dynamic link so the eventual linker error is
        // still clear if pkg-config omitted `-llensfun` for some reason.
        println!("cargo:rustc-link-lib=dylib=lensfun");
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=LIBLENSFUN_SYS_DIR");
}
