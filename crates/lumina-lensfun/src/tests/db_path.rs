//! Hermetic tests for the platform-dependent Lensfun database resolution
//! (LENSFUN-DB-33).
//!
//! Extracted from `db_path.rs` (file-size ratchet, User-Vorgabe 2026-09-17),
//! same pattern as `src/tests/strict_match.rs`.
//!
//! Every test injects the environment values and the in-memory directory probe
//! from [`probe_fixture`], so the precedence rules are pinned without touching
//! the real filesystem or the process environment — and without needing an
//! installed database. Nothing here reads or writes a process-global, so these
//! tests need no lock and stay parallel-safe. The load *plan* itself is pinned in
//! `db_layers.rs`.

use super::probe_fixture::FakeProbe;
use crate::db_path::*;
use std::ffi::OsStr;
use std::path::PathBuf;

/// `/usr/share/lensfun` with a populated `version_1/` carrying the real
/// `timestamp.txt` value the distro package ships.
fn linux_probe() -> FakeProbe {
    FakeProbe::new(&[("/usr/share/lensfun/version_1", 56)])
        .dated("/usr/share/lensfun/version_1", 1_645_386_247)
}

#[test]
fn env_override_beats_compiled_and_platform_default() {
    let probe = FakeProbe::new(&[
        ("/custom/lensfun/version_1", 3),
        ("/usr/share/lensfun/version_1", 9),
    ])
    .dated("/custom/lensfun/version_1", 1_645_386_247);
    let resolved = resolve_with(
        Some(OsStr::new("/custom/lensfun")),
        Some(OsStr::new("/compiled/share/lensfun")),
        None,
        None,
        &probe,
    )
    .expect("override must resolve");
    assert_eq!(resolved.dir, PathBuf::from("/custom/lensfun"));
    assert_eq!(resolved.source, Source::EnvOverride);
    assert_eq!(
        resolved.schema_dir,
        PathBuf::from("/custom/lensfun/version_1")
    );
    assert!(resolved.pin_honored());
    assert_eq!(resolved.file_count(), 3);
}

#[test]
fn compiled_datadir_beats_platform_default() {
    let probe = FakeProbe::new(&[
        ("/compiled/share/lensfun/version_1", 4),
        ("/usr/share/lensfun/version_1", 9),
    ]);
    let resolved = resolve_with(
        None,
        Some(OsStr::new("/compiled/share/lensfun")),
        None,
        None,
        &probe,
    )
    .expect("resolve");
    assert_eq!(resolved.dir, PathBuf::from("/compiled/share/lensfun"));
    assert_eq!(resolved.source, Source::CompiledDataDir);
}

#[test]
fn platform_default_is_the_last_resort() {
    let resolved =
        resolve_with(None, None, None, None, &linux_probe()).expect("linux default must resolve");
    assert_eq!(resolved.dir, PathBuf::from(LINUX_DEFAULT));
    assert_eq!(resolved.source, Source::PlatformDefault);
    assert_eq!(resolved.file_count(), 56);
}

#[test]
fn a_broken_override_is_a_loud_error_and_never_falls_through() {
    // The real system default IS present, yet the override decides.
    let probe = linux_probe();
    let err = resolve_with(
        Some(OsStr::new("/nope/lensfun")),
        Some(OsStr::new("/compiled/share/lensfun")),
        None,
        None,
        &probe,
    )
    .expect_err("a broken override must not silently fall through");
    assert_eq!(
        err,
        SystemDbError::OverrideUnusable {
            dir: PathBuf::from("/nope/lensfun"),
            reason: MissReason::Absent,
        }
    );
    let text = err.to_string();
    assert!(text.contains("OverrideUnusable"), "{text}");
    assert!(text.contains("/nope/lensfun"), "{text}");
}

/// A relative override is a config error, not something to resolve against the
/// process working directory (a Finder-launched app runs with cwd `/`).
#[test]
fn a_relative_override_is_rejected() {
    let probe = linux_probe();
    let err = resolve_with(
        Some(OsStr::new("relative/lensfun")),
        None,
        None,
        None,
        &probe,
    )
    .expect_err("a relative override must be rejected");
    assert_eq!(
        err,
        SystemDbError::OverrideUnusable {
            dir: PathBuf::from("relative/lensfun"),
            reason: MissReason::NotADirectory,
        }
    );
}

#[test]
fn an_empty_override_directory_is_a_loud_error() {
    let probe = FakeProbe::new(&[
        ("/empty/lensfun/version_1", 0),
        ("/usr/share/lensfun/version_1", 56),
    ]);
    let err = resolve_with(Some(OsStr::new("/empty/lensfun")), None, None, None, &probe)
        .expect_err("empty is a miss");
    assert_eq!(
        err,
        SystemDbError::OverrideUnusable {
            dir: PathBuf::from("/empty/lensfun"),
            reason: MissReason::NoXmlFiles,
        }
    );
}

#[test]
fn nothing_found_names_every_probed_location() {
    let err = resolve_with(
        None,
        Some(OsStr::new("/compiled/share")),
        None,
        None,
        &FakeProbe::new(&[]),
    )
    .expect_err("an empty machine must fail loudly");
    let SystemDbError::NotFound { misses } = &err else {
        panic!("expected NotFound, got {err:?}");
    };
    // Compiled first, then every platform default.
    assert_eq!(misses[0].dir, PathBuf::from("/compiled/share/lensfun"));
    assert_eq!(misses[0].source, Source::CompiledDataDir);
    let defaults = platform_default_dirs();
    assert_eq!(misses.len(), 1 + defaults.len());
    let text = err.to_string();
    assert!(text.contains("SystemDbError/NotFound"), "{text}");
    assert!(
        text.contains("KEIN stiller Ersatz"),
        "anti-silent wording: {text}"
    );
}

#[test]
fn datadir_is_mapped_to_the_lensfun_subdirectory() {
    assert_eq!(
        lensfun_dir_in_datadir(OsStr::new("/opt/homebrew/Cellar/lensfun/0.3.4/share")),
        Some(PathBuf::from(
            "/opt/homebrew/Cellar/lensfun/0.3.4/share/lensfun"
        ))
    );
    assert_eq!(
        lensfun_dir_in_datadir(OsStr::new("/usr/local/share/lensfun")),
        Some(PathBuf::from("/usr/local/share/lensfun"))
    );
    assert_eq!(lensfun_dir_in_datadir(OsStr::new("")), None);
}

#[test]
fn candidates_are_deduplicated_and_ordered() {
    let list = candidates(
        Some(OsStr::new("/usr/share/lensfun")),
        Some(OsStr::new("/usr/share/lensfun")),
    );
    let dirs: Vec<&PathBuf> = list.iter().map(|(_, d)| d).collect();
    let mut deduped = dirs.clone();
    deduped.dedup();
    assert_eq!(dirs.len(), deduped.len(), "duplicates in {dirs:?}");
    // The override keeps its higher-precedence source even when the path is
    // identical to the compiled value.
    assert_eq!(
        list[0],
        (Source::EnvOverride, PathBuf::from("/usr/share/lensfun"))
    );
}

/// Both platform layouts are pinned **unconditionally**, so the Linux branch is
/// verified even when the suite runs on macOS and vice versa.
#[test]
fn both_platform_default_layouts_are_pinned() {
    assert_eq!(linux_default_dirs(), vec![PathBuf::from(LINUX_DEFAULT)]);
    assert_eq!(
        macos_default_dirs(true),
        vec![
            PathBuf::from(MACOS_APPLE_SILICON),
            PathBuf::from(MACOS_INTEL),
            PathBuf::from(LINUX_DEFAULT),
        ],
        "on Apple Silicon the Homebrew arm64 prefix must come first"
    );
    assert_eq!(
        macos_default_dirs(false),
        vec![
            PathBuf::from(MACOS_INTEL),
            PathBuf::from(MACOS_APPLE_SILICON),
            PathBuf::from(LINUX_DEFAULT),
        ],
        "on Intel macOS the Homebrew x86_64 prefix must come first"
    );
}

#[test]
fn platform_default_dirs_matches_the_target() {
    let expected = if cfg!(target_os = "macos") {
        macos_default_dirs(cfg!(target_arch = "aarch64"))
    } else {
        linux_default_dirs()
    };
    assert_eq!(platform_default_dirs(), expected);
}

/// The Linux/CI layout must actually *resolve* — `/usr/share/lensfun` with
/// `version_1/*.xml` is the container case this change must not break.
#[test]
fn the_ci_container_layout_resolves() {
    let resolved =
        resolve_with(None, None, None, None, &linux_probe()).expect("the CI layout must resolve");
    assert_eq!(resolved.dir, PathBuf::from(LINUX_DEFAULT));
    assert_eq!(resolved.source, Source::PlatformDefault);
    assert!(
        resolved.pin_honored(),
        "without an override the resolved system database is the one that is loaded"
    );
    assert_eq!(resolved.primary, LayerOrigin::SystemSchema);
    assert_eq!(
        resolved.schema_dir,
        PathBuf::from("/usr/share/lensfun/version_1")
    );
    assert_eq!(resolved.file_count(), 56);
    assert_eq!(resolved.layers.len(), 1, "no user layer without a user DB");
    assert_eq!(resolved.layers[0].origin, LayerOrigin::SystemSchema);
}
