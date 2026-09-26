//! glib's `g_get_user_data_dir()` / upstream's `HomeDataDir` (LENSFUN-DB-33).
//!
//! Split out of `db_layers.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//! It has its own source of truth — glib 2.88.3's `glib/gutils.c` — and is
//! therefore a module of its own rather than a helper buried in the load
//! algorithm. The deviation it carries (no passwd fallback) is stated in the
//! doc comment below and in `feature/platform/capability-matrix.md`.

#![cfg(feature = "native")]

use crate::db_path::USER_DB_SUBDIR;
use std::ffi::OsStr;
use std::path::PathBuf;

/// Reproduced from glib 2.88.3 `glib/gutils.c` (`g_build_user_data_dir` and
/// `g_build_home_dir`), which is the glib lensfun links against:
///
/// - `XDG_DATA_HOME`, when **set and non-empty**, wins — used **as-is**,
///   including a *relative* value. glib performs no absoluteness check here
///   (that check only exists on Windows, for `HOME`).
/// - otherwise `$HOME/.local/share`. glib uses a *set* `HOME` as-is too: an
///   empty `HOME` yields `/.local/share`, and a relative one yields
///   `<rel>/.local/share`. Only an **unset** `HOME` makes glib continue.
/// - `/lensfun` is appended in both cases.
///
/// # Documented deviation: no passwd fallback
///
/// With `HOME` unset, glib reads the passwd database and finally falls back to
/// `/` with a `g_warning`. This crate has no dependencies and no portable way
/// to read the passwd database (macOS resolves home directories through Open
/// Directory, not `/etc/passwd`), so [`user_db_dir_from`] returns `None`
/// instead of guessing. That is **not** silent: both the user database and the
/// user-update package are then recorded in [`Plan::skipped`] with
/// [`SkipReason::NoUserDataDir`], which is actionable and is pushed into the
/// diagnostics stream. The consequence — an operator with no `HOME` loses the
/// user layer — is stated in `feature/platform/capability-matrix.md`.
pub fn user_db_dir_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(value) = xdg.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value).join(USER_DB_SUBDIR));
    }
    let home = home?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(USER_DB_SUBDIR),
    )
}

/// The process-environment entry point for [`user_db_dir_from`].
pub fn user_db_dir() -> Option<PathBuf> {
    user_db_dir_from(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}
