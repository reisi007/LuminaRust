//! LENSFUN-DB-33: the **operator pin** — what `LUMINA_LENSFUN_DB` guarantees
//! (review finding F1-2).
//!
//! Split out of `tests/db_layers.rs` (file-size ratchet, User-Vorgabe
//! 2026-09-17). The pin has its own normative rules — a broken override is a hard
//! named error instead of a fall-through, and a valid one is *never displaced* by
//! an update package — and they belong in one place, separate from the
//! newest-wins algorithm they constrain.

use super::db_layers::system_layer;
use super::probe_fixture::FakeProbe;
use crate::db_path::*;
use std::ffi::OsStr;
use std::path::PathBuf;

/// The real `timestamp.txt` value the distro package ships.
const SYSTEM_STAMP: i64 = 1_645_386_247;
const UPDATES: &str = "/var/lib/lensfun-updates/version_1";

/// **F1-2: an operator override is never displaced.** The measured defect was
/// `LUMINA_LENSFUN_DB=/custom` (3 files) with a newer `updates/version_1`
/// winning, so the loaded set silently was *not* the pinned directory. Upstream
/// has no override concept at all, so this is a documented divergence in favour
/// of the SOLL rule that explicit operator intent is never replaced.
#[test]
fn an_operator_override_is_never_displaced_by_an_update_package() {
    let probe = FakeProbe::new(&[
        ("/custom/lensfun/version_1", 3),
        ("/var/lib/lensfun-updates/version_1", 3),
        ("/home/u/.local/share/lensfun/updates/version_1", 7),
    ])
    // Every candidate claims a far newer date than the pinned directory.
    .dated("/custom/lensfun/version_1", 1)
    .dated("/var/lib/lensfun-updates/version_1", 1_700_000_000)
    .dated(
        "/home/u/.local/share/lensfun/updates/version_1",
        1_800_000_000,
    );
    let resolved = resolve_with(
        Some(OsStr::new("/custom/lensfun")),
        None,
        None,
        Some(OsStr::new("/home/u")),
        &probe,
    )
    .expect("resolve");
    assert_eq!(resolved.source, Source::EnvOverride);
    assert!(
        resolved.pin_honored(),
        "the pinned directory must be the one that is loaded"
    );
    let system = system_layer(&resolved);
    assert_eq!(system.origin, LayerOrigin::SystemSchema, "{system:?}");
    assert_eq!(system.dir, PathBuf::from("/custom/lensfun"));
    assert_eq!(
        system.files.len(),
        3,
        "exactly the pinned files, nothing else"
    );
}

/// Without an override, a compiled/platform source *is* displaced by upstream's
/// competition — but the divergence is visible through `primary` /
/// `pin_honored` instead of being silent.
#[test]
fn a_non_override_source_may_be_displaced_but_is_never_silent() {
    let probe = FakeProbe::new(&[("/usr/share/lensfun/version_1", 56), (UPDATES, 3)])
        .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP)
        .dated(UPDATES, 1_700_000_000);
    let resolved = resolve_with(None, None, None, None, &probe).expect("resolve");
    assert_eq!(resolved.source, Source::PlatformDefault);
    assert!(!resolved.pin_honored());
    assert_ne!(
        system_layer(&resolved).dir,
        resolved.dir,
        "layers[0] must name what is loaded, not what was resolved"
    );
}

/// The override does not suppress the user database: upstream always merges it,
/// and dropping it would be a silent capability loss for an operator who only
/// wanted to pin the *system* database.
#[test]
fn the_override_does_not_suppress_the_user_database() {
    let probe = FakeProbe::new(&[
        ("/custom/lensfun/version_1", 3),
        ("/home/u/.local/share/lensfun", 1),
    ])
    .dated("/custom/lensfun/version_1", SYSTEM_STAMP);
    let resolved = resolve_with(
        Some(OsStr::new("/custom/lensfun")),
        None,
        None,
        Some(OsStr::new("/home/u")),
        &probe,
    )
    .expect("resolve");
    assert_eq!(resolved.dir, PathBuf::from("/custom/lensfun"));
    assert!(resolved.user_layer().is_some(), "user layer must be merged");
    assert_eq!(resolved.file_count(), 4);
}
