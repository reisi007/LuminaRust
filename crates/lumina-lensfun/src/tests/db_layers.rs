//! LENSFUN-DB-33: the load plan — upstream `lfDatabase::Load()` parity.
//!
//! Extracted from `tests/db_path.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//!
//! The regression these tests exist for (review finding **F1-BRUTK**) is a
//! wrong *quantity*: the plan used the newest **file mtime** where upstream
//! compares the **content of `timestamp.txt`**
//! (`libs/lensfun/auxfun.cpp::_lf_read_database_timestamp`). On a real install
//! the system `timestamp.txt` is `1645386247` while the file mtimes are
//! `1689186244`, and an `updates/version_1` directory *without* a
//! `timestamp.txt` scored `0` upstream but "brand new" under mtime — so it
//! won and displaced all 55 system XML files. Every fixture here therefore
//! models `timestamp.txt`, never an mtime (see [`probe_fixture::Timestamp`]),
//! and `a_stale_update_package_without_a_timestamp_never_displaces_the_system_database`
//! pins that case against the true semantics.
//!
//! Also covered: the user-database merge (F1), upstream's tie-break order, the
//! system-update guard, the operator-override pin, and the skip bookkeeping that
//! replaces the old silent `continue`.

use super::probe_fixture::FakeProbe;
use crate::db_layers::{user_db_dir_from, SkipReason};
use crate::db_path::*;
use std::ffi::OsStr;
use std::path::PathBuf;
/// The real value shipped in the Homebrew lensfun 0.3.4 `version_1`.
const SYSTEM_STAMP: i64 = 1_645_386_247;

fn linux_probe() -> FakeProbe {
    FakeProbe::new(&[("/usr/share/lensfun/version_1", 56)])
        .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP)
}

const UPDATES: &str = "/var/lib/lensfun-updates/version_1";

/// The one system layer of a resolved plan.
fn system_layer(resolved: &Resolved) -> &Layer {
    assert_eq!(
        resolved.layers[0].origin, resolved.primary,
        "layers[0] must be the primary layer (one source of truth)"
    );
    &resolved.layers[0]
}

// ---------------------------------------------------------------------------
// The user database (F1)
// ---------------------------------------------------------------------------

#[test]
fn home_data_dir_follows_glib() {
    // XDG_DATA_HOME wins when set and non-empty.
    assert_eq!(
        user_db_dir_from(Some(OsStr::new("/xdg")), Some(OsStr::new("/home/u"))),
        Some(PathBuf::from("/xdg/lensfun"))
    );
    // …and it is used **as-is**, even when relative: glib 2.88.3's
    // `g_build_user_data_dir` does no absoluteness check (that exists only on
    // Windows, for HOME). The previous code ignored a relative value, which
    // silently dropped the user database.
    assert_eq!(
        user_db_dir_from(Some(OsStr::new("rel/xdg")), Some(OsStr::new("/home/u"))),
        Some(PathBuf::from("rel/xdg/lensfun"))
    );
    // An empty value counts as unset (`g_getenv` + `[0]` check), so HOME wins.
    assert_eq!(
        user_db_dir_from(Some(OsStr::new("")), Some(OsStr::new("/home/u"))),
        Some(PathBuf::from("/home/u/.local/share/lensfun"))
    );
    // A *set* HOME is used as-is too — including an empty one, which glib turns
    // into a *relative* `.local/share`: `g_build_filename` ignores empty
    // elements, so `g_build_filename ("", ".local", "share", NULL)` is
    // `.local/share`.
    assert_eq!(
        user_db_dir_from(None, Some(OsStr::new("/home/u"))),
        Some(PathBuf::from("/home/u/.local/share/lensfun"))
    );
    assert_eq!(
        user_db_dir_from(None, Some(OsStr::new(""))),
        Some(PathBuf::from(".local/share/lensfun"))
    );
    assert_eq!(
        user_db_dir_from(None, Some(OsStr::new("rel"))),
        Some(PathBuf::from("rel/.local/share/lensfun"))
    );
    // Only an **unset** HOME yields None — the documented deviation from
    // glib's passwd fallback. The consequence must be reported, not silent.
    assert_eq!(user_db_dir_from(None, None), None);
    let resolved = resolve_with(
        None,
        None,
        None,
        None,
        &FakeProbe::new(&[("/usr/share/lensfun/version_1", 56)]),
    )
    .expect("resolve");
    let user_skips: Vec<_> = resolved
        .skipped
        .iter()
        .filter(|s| s.reason == SkipReason::NoUserDataDir)
        .collect();
    assert_eq!(
        user_skips.len(),
        2,
        "both the user database and the user update package must be recorded: {:?}",
        resolved.skipped
    );
    assert!(user_skips.iter().all(|s| s.dir.is_none()));
    assert!(SkipReason::NoUserDataDir.is_actionable());
}

/// F1 core: the user database is merged **unconditionally**, exactly like
/// `LoadDirectory(HomeDataDir)`, and it is a *flat* directory (no `version_1`).
#[test]
fn the_user_database_is_merged_on_top_of_the_system_database() {
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        ("/home/u/.local/share/lensfun", 2),
    ])
    .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP);
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    assert_eq!(resolved.layers.len(), 2, "got {:?}", resolved.layers);
    assert_eq!(resolved.primary, LayerOrigin::SystemSchema);
    let user = resolved.user_layer().expect("user layer must be present");
    assert_eq!(user.origin, LayerOrigin::UserData);
    assert_eq!(user.dir, PathBuf::from("/home/u/.local/share/lensfun"));
    assert_eq!(user.files.len(), 2);
    // The system profiles are still there — the user layer is additive.
    assert_eq!(resolved.file_count(), 58);
}

/// A user database that exists but holds no XML is recorded, not skipped
/// silently, and does not become an empty layer.
#[test]
fn an_empty_user_database_is_recorded_instead_of_being_dropped() {
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        ("/home/u/.local/share/lensfun", 0),
    ])
    .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP);
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    assert_eq!(
        resolved.layers.len(),
        1,
        "no empty layer: {:?}",
        resolved.layers
    );
    let skip = resolved
        .skipped
        .iter()
        .find(|s| s.origin == LayerOrigin::UserData)
        .expect("the empty user database must be recorded");
    assert_eq!(skip.reason, SkipReason::NoXmlFiles);
    assert!(skip.reason.is_actionable());
}

/// A user database that does not exist is the normal case: recorded, but not
/// reported as a defect.
#[test]
fn an_absent_user_database_is_recorded_but_not_actionable() {
    let resolved = resolve_with(
        None,
        None,
        None,
        Some(OsStr::new("/home/u")),
        &linux_probe(),
    )
    .expect("resolve");
    let skip = resolved
        .skipped
        .iter()
        .find(|s| s.origin == LayerOrigin::UserData)
        .expect("recorded");
    assert_eq!(skip.reason, SkipReason::Absent);
    assert!(!skip.reason.is_actionable());
}

// ---------------------------------------------------------------------------
// The newest-wins competition (F1-BRUTK)
// ---------------------------------------------------------------------------

/// **F1-BRUTK core.** An `updates/version_1` directory that holds XML but has
/// no `timestamp.txt` scores `0` upstream, so it can never displace a system
/// database carrying a real timestamp. The mtime-based implementation had the
/// sign backwards and lost the entire system database here.
#[test]
fn a_stale_update_package_without_a_timestamp_never_displaces_the_system_database() {
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        ("/home/u/.local/share/lensfun/updates/version_1", 7),
        ("/home/u/.local/share/lensfun", 2),
    ])
    .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP)
    .undated("/home/u/.local/share/lensfun/updates/version_1");
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    let system = system_layer(&resolved);
    assert_eq!(
        system.origin,
        LayerOrigin::SystemSchema,
        "a dateless update package must not win: {system:?}"
    );
    assert_eq!(system.files.len(), 56, "all 56 system files must survive");
    assert!(resolved.user_layer().is_some());
    // A dateless update package *did* take part in the contest (it holds XML) —
    // it simply lost, which is exactly what `primary`/`pin_honored` report. The
    // point of the assertion above is that it did not win.
    assert!(resolved
        .skipped
        .iter()
        .all(|s| s.origin != LayerOrigin::UserUpdates));
}

/// The all-`-1` case: the system database has no `timestamp.txt` and no update
/// package exists. Upstream's tie-break would pick the non-existent
/// `system_updates_dirname` and load **nothing**; the resolved system database is
/// kept instead, so the profiles survive.
#[test]
fn an_undated_system_database_with_no_update_package_still_loads() {
    let probe = FakeProbe::new(&[("/usr/share/lensfun/version_1", 56)])
        .undated("/usr/share/lensfun/version_1");
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    let system = system_layer(&resolved);
    assert_eq!(system.origin, LayerOrigin::SystemSchema, "{system:?}");
    assert_eq!(
        system.files.len(),
        56,
        "an undated system database must still be loaded in full"
    );
}

/// A user update package with a genuinely newer `timestamp.txt` does win.
#[test]
fn a_newer_user_update_package_supersedes_the_system_database() {
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        // A user *update package* AND a user database, both under HomeDataDir.
        ("/home/u/.local/share/lensfun/updates/version_1", 7),
        ("/home/u/.local/share/lensfun", 2),
    ])
    .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP)
    .dated(
        "/home/u/.local/share/lensfun/updates/version_1",
        1_700_000_000,
    );
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    let system = system_layer(&resolved);
    assert_eq!(system.origin, LayerOrigin::UserUpdates, "{system:?}");
    assert_eq!(system.files.len(), 7);
    // …and the user database is still merged on top (upstream parity).
    assert!(resolved.user_layer().is_some());
    // The system directory is no longer loaded: the single source of truth
    // says so, and `pin_honored` reports the divergence.
    assert!(!resolved.pin_honored());
    assert_eq!(resolved.primary, LayerOrigin::UserUpdates);
}

/// The distro update directory wins when it is genuinely newer.
#[test]
fn a_newer_system_update_package_supersedes_the_system_database() {
    let probe = FakeProbe::new(&[("/usr/share/lensfun/version_1", 56), (UPDATES, 3)])
        .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP)
        .dated(UPDATES, 1_700_000_000);
    let resolved = resolve_with(None, None, None, None, &probe).expect("resolve");
    let system = system_layer(&resolved);
    assert_eq!(system.origin, LayerOrigin::SystemUpdates, "{system:?}");
    assert_eq!(system.files.len(), 3);
    assert!(!resolved.pin_honored());
}

/// The resolved system database wins when nothing is newer — the normal case.
#[test]
fn the_resolved_system_database_wins_when_nothing_is_newer() {
    let probe = FakeProbe::new(&[("/usr/share/lensfun/version_1", 56), (UPDATES, 3)])
        .dated("/usr/share/lensfun/version_1", 1_700_000_000)
        .dated(UPDATES, SYSTEM_STAMP);
    let resolved = resolve_with(None, None, None, None, &probe).expect("resolve");
    let system = system_layer(&resolved);
    assert_eq!(system.origin, LayerOrigin::SystemSchema, "{system:?}");
    assert_eq!(system.files.len(), 56);
    assert!(resolved.pin_honored());
}

/// **MITTEL-1: the tie-break, pinned against upstream's own code.**
///
/// ```text
/// if (M > S) { if (U > M) UserUpdates else main }
/// else       { if (U > S) UserUpdates else system_updates }
/// ```
///
/// The old implementation iterated user-updates → system-updates → system-schema
/// with a strict `>`, so on a tie the *user* package won — the exact opposite
/// of upstream's `system_updates > main > user_updates`.
#[test]
fn the_tie_break_follows_upstream_not_the_iteration_order() {
    // Full tie M == S == U: upstream takes `system_updates_dirname`.
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        (UPDATES, 3),
        ("/home/u/.local/share/lensfun/updates/version_1", 7),
    ])
    .dated("/usr/share/lensfun/version_1", 1_700_000_000)
    .dated(UPDATES, 1_700_000_000)
    .dated(
        "/home/u/.local/share/lensfun/updates/version_1",
        1_700_000_000,
    );
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    assert_eq!(
        system_layer(&resolved).origin,
        LayerOrigin::SystemUpdates,
        "M == S == U must resolve to system_updates, like upstream"
    );

    // M == U > S: `M > S` holds, `U > M` does not ⇒ `main_dirname`.
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        (UPDATES, 3),
        ("/home/u/.local/share/lensfun/updates/version_1", 7),
    ])
    .dated("/usr/share/lensfun/version_1", 1_700_000_000)
    .dated(UPDATES, 1_600_000_000)
    .dated(
        "/home/u/.local/share/lensfun/updates/version_1",
        1_700_000_000,
    );
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    assert_eq!(
        system_layer(&resolved).origin,
        LayerOrigin::SystemSchema,
        "U == M must not beat main: upstream needs U > M"
    );

    // S == U > M: `M > S` fails, `U > S` fails ⇒ `system_updates_dirname`.
    let probe = FakeProbe::new(&[
        ("/usr/share/lensfun/version_1", 56),
        (UPDATES, 3),
        ("/home/u/.local/share/lensfun/updates/version_1", 7),
    ])
    .dated("/usr/share/lensfun/version_1", 1_500_000_000)
    .dated(UPDATES, 1_600_000_000)
    .dated(
        "/home/u/.local/share/lensfun/updates/version_1",
        1_600_000_000,
    );
    let resolved =
        resolve_with(None, None, None, Some(OsStr::new("/home/u")), &probe).expect("resolve");
    assert_eq!(
        system_layer(&resolved).origin,
        LayerOrigin::SystemUpdates,
        "S == U must resolve to system_updates, like upstream"
    );
}

/// **MITTEL-2: the hard-coded `SYSTEM_UPDATES_DIR` needs a real date.** It is a
/// *build-time* path in lensfun (`SYSTEM_DB_UPDATE_PATH` in
/// `include/lensfun/config.h.in.cmake`), so `/var/lib/lensfun-updates` may not
/// even belong to this build. Upstream's tie-break (`0 == 0` ⇒ system updates
/// win) would let a dateless directory there displace a real system database.
#[test]
fn the_system_update_layer_needs_a_real_timestamp_txt() {
    for stamp in [None, Some(0), Some(-1)] {
        let mut probe = FakeProbe::new(&[("/usr/share/lensfun/version_1", 56), (UPDATES, 3)]);
        probe = match stamp {
            Some(value) => probe.dated("/usr/share/lensfun/version_1", value),
            None => probe.undated("/usr/share/lensfun/version_1"),
        };
        let resolved = resolve_with(None, None, None, None, &probe).expect("resolve");
        assert_eq!(
            system_layer(&resolved).origin,
            LayerOrigin::SystemSchema,
            "a system update package with timestamp.txt = {stamp:?} must not win"
        );
        let skip = resolved
            .skipped
            .iter()
            .find(|s| s.origin == LayerOrigin::SystemUpdates)
            .expect("the rejected system update package must be recorded");
        assert_eq!(skip.reason, SkipReason::NoTimestampFile);
        assert!(skip.reason.is_actionable());
    }
}

/// An update package that exists but holds no XML can never win: upstream would
/// select it by timestamp and then load *nothing* from any of the three
/// directories, silently losing the whole system database.
#[test]
fn an_update_package_without_xml_cannot_win() {
    let probe = FakeProbe::new(&[("/usr/share/lensfun/version_1", 56), (UPDATES, 0)])
        .dated("/usr/share/lensfun/version_1", SYSTEM_STAMP)
        .dated(UPDATES, 1_700_000_000);
    let resolved = resolve_with(None, None, None, None, &probe).expect("resolve");
    assert_eq!(system_layer(&resolved).origin, LayerOrigin::SystemSchema);
    assert_eq!(system_layer(&resolved).files.len(), 56);
    let skip = resolved
        .skipped
        .iter()
        .find(|s| s.origin == LayerOrigin::SystemUpdates)
        .expect("recorded");
    assert_eq!(skip.reason, SkipReason::NoXmlFiles);
}

// ---------------------------------------------------------------------------
// The operator pin (F1-2)
// ---------------------------------------------------------------------------

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
