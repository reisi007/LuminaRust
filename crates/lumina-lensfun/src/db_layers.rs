//! The Lensfun load plan: which directories are read, in which order
//! (LENSFUN-DB-33).
//!
//! Split out of `db_path.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//! `db_path` owns the *pure resolution order*; this module owns upstream's
//! *load algorithm*, which is a separate concern with a separate source of
//! truth (`lfDatabase::Load()` in lensfun 0.3.4 `libs/lensfun/database.cpp`).
//!
//! # Upstream contract being reproduced
//!
//! ```text
//! main_dirname         = CONF_DATADIR / "version_1"
//! system_updates_dirname = SYSTEM_DB_UPDATE_PATH / "version_1"
//! UserUpdatesDir       = HomeDataDir / "updates" / "version_1"
//! HomeDataDir          = g_get_user_data_dir() / "lensfun"
//!
//! M = _lf_read_database_timestamp (main_dirname)
//! S = _lf_read_database_timestamp (system_updates_dirname)
//! U = _lf_read_database_timestamp (UserUpdatesDir)
//!
//! if (M > S) { if (U > M) LoadDirectory (UserUpdatesDir); else LoadDirectory (main_dirname); }
//! else       { if (U > S) LoadDirectory (UserUpdatesDir); else LoadDirectory (system_updates_dirname); }
//! LoadDirectory (HomeDataDir)   // unconditional, flat, no version_1
//! ```
//!
//! Three properties of that code are easy to get wrong and are pinned by
//! tests in `src/tests/db_layers.rs`:
//!
//! 1. The compared quantity is the **content of `timestamp.txt`**, not a file
//!    mtime (see [`crate::db_timestamp`]).
//! 2. On a tie the preference is **system-updates > main > user-updates**.
//!    With `M == S == U` upstream takes `system_updates_dirname`; with
//!    `M > S` and `U == M` it takes `main_dirname`. `select_primary` below is a
//!    line-for-line transcription, not a re-derivation.
//! 3. `LoadDirectory (HomeDataDir)` runs **unconditionally**, so dropping the
//!    user layer would be a silent loss of profiles (F1).
//!
//! # Documented deviations from upstream (each one an anti-silent decision)
//!
//! - **A layer must hold XML to win.** Upstream selects by timestamp alone; a
//!   `version_1` directory that holds no `*.xml` can therefore win and then
//!   contribute nothing, silently dropping *all three* directories. Such a
//!   candidate is removed from the competition *before* the comparison (and
//!   compared as `-1`, the value it would have had if it did not exist) and the
//!   removal is reported through [`SkipReason::NoXmlFiles`].
//! - **All three timestamps `-1` keeps the system database.** In that state
//!   upstream's tie-break picks `system_updates_dirname`, which does not exist,
//!   and loads nothing at all. The resolved system database is kept instead.
//! - **The system-update layer needs a real `timestamp.txt`.** See
//!   [`SYSTEM_UPDATES_DIR`].
//! - **An operator override is never displaced.** See
//!   [`Source::EnvOverride`].
//! - **The user-data location follows glib's real rules.** See
//!   [`user_db_dir_from`].

use crate::db_path::{Probe, Source, SYSTEM_UPDATES_DIR, USER_DB_SUBDIR};
use crate::db_timestamp::DatabaseTimestamp;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

/// Which database a loaded layer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerOrigin {
    /// `CONF_DATADIR/version_1` — the resolved (and possibly pinned) system
    /// database.
    SystemSchema,
    /// `HomeDataDir/updates/version_1` — a user-supplied *update* package.
    UserUpdates,
    /// `SYSTEM_DB_UPDATE_PATH/version_1` — a distro-supplied update package.
    SystemUpdates,
    /// `HomeDataDir` — the user database, merged unconditionally.
    UserData,
}

impl LayerOrigin {
    /// Human-readable, stable label for diagnostics and test failure output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemSchema => "System-Datenbank",
            Self::UserUpdates => "Benutzer-Update-Paket",
            Self::SystemUpdates => "System-Update-Paket",
            Self::UserData => "Benutzer-Datenbank",
        }
    }
}

/// One directory of XML profiles to load, in load order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// The directory actually read (no implicit `version_1` suffix).
    pub dir: PathBuf,
    /// Sorted `*.xml` files — the exact, deterministic load list.
    pub files: Vec<PathBuf>,
    /// Which database this is.
    pub origin: LayerOrigin,
}

/// Why a layer that was considered is **not** part of the plan.
///
/// Nothing is dropped without one of these: an update directory that exists but
/// is empty or unreadable is a real defect and is reported, never skipped in
/// silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The directory does not exist.
    ///
    /// The normal case — macOS has no system update package, and most users
    /// never install a user one. Inspectable via [`crate::db_path::Resolved`]
    /// but deliberately **not** pushed into the diagnostics stream, because it
    /// would be pure noise on every lookup.
    Absent,
    /// The directory exists and holds no `*.xml` file, so it may not win.
    NoXmlFiles,
    /// The directory exists but could not be read (permissions, I/O error).
    Unreadable,
    /// A system update package without a usable `timestamp.txt`. See
    /// [`SYSTEM_UPDATES_DIR`].
    NoTimestampFile,
    /// Neither `XDG_DATA_HOME` nor `HOME` yielded a user-data directory, so
    /// there is no `HomeDataDir` to merge. Upstream would fall back to the
    /// passwd database here; see [`user_db_dir_from`].
    NoUserDataDir,
}

impl SkipReason {
    /// Whether this is worth pushing into the [`Diagnostics`](crate::system_load::Diagnostics)
    /// stream, as opposed to only being recorded in the plan.
    pub const fn is_actionable(self) -> bool {
        !matches!(self, Self::Absent)
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str("Verzeichnis nicht vorhanden"),
            Self::NoXmlFiles => f.write_str("Update-Paket ohne XML-Datei, nicht wertungsfähig"),
            Self::Unreadable => f.write_str("Verzeichnis nicht lesbar"),
            Self::NoTimestampFile => {
                f.write_str("System-Update-Paket ohne verwertbares timestamp.txt")
            }
            Self::NoUserDataDir => {
                f.write_str("weder XDG_DATA_HOME noch HOME gesetzt, keine Benutzer-Ebene")
            }
        }
    }
}

/// A layer that was considered and left out, with the reason why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLayer {
    /// The directory that was probed. `None` only for
    /// [`SkipReason::NoUserDataDir`], where there is no path at all.
    pub dir: Option<PathBuf>,
    /// Which database it would have been.
    pub origin: LayerOrigin,
    /// Why it is not in the plan.
    pub reason: SkipReason,
}

impl fmt::Display for SkippedLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) – {}",
            self.origin.as_str(),
            self.dir
                .as_deref()
                .map_or_else(|| "<kein Pfad>".to_owned(), |d| d.display().to_string()),
            self.reason
        )
    }
}

/// The load plan plus everything that was considered and left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The layers to load, in load order. `layers[0]` is always the chosen
    /// system layer; `layers[1]`, if present, is the user database.
    pub layers: Vec<Layer>,
    /// Every considered-but-excluded layer. Complete by construction, so the
    /// absence of a diagnostic is provably a decision and not an oversight.
    pub skipped: Vec<SkippedLayer>,
}

/// One `version_1` directory competing to be *the* system database.
struct Candidate {
    origin: LayerOrigin,
    dir: PathBuf,
    files: Vec<PathBuf>,
}

/// Build the ordered load plan, reproducing `lfDatabase::Load()`.
///
/// `dir`/`system_files` are the *resolved* system database and the XML files
/// resolution already proved it holds. `source` is the resolution step that
/// produced it: an [`Source::EnvOverride`] pins the system layer (no update
/// package may displace it), every other source runs upstream's competition.
pub fn build(
    source: Source,
    dir: &Path,
    system_files: &[PathBuf],
    xdg: Option<&OsStr>,
    home: Option<&OsStr>,
    probe: &impl Probe,
) -> Plan {
    let mut candidates = vec![Candidate {
        origin: LayerOrigin::SystemSchema,
        dir: dir.to_path_buf(),
        files: system_files.to_vec(),
    }];
    let mut skipped: Vec<SkippedLayer> = Vec::new();

    // The two update directories, probed independently of one another — in
    // particular the system-update probe must not depend on a user data
    // directory existing.
    //
    // `path` already ends in the schema subdir, so read it flat — going through
    // `schema_files` would append `version_1` a second time.
    consider_update(
        PathBuf::from(SYSTEM_UPDATES_DIR).join(crate::db_path::SCHEMA_SUBDIR),
        LayerOrigin::SystemUpdates,
        &mut candidates,
        &mut skipped,
        probe,
    );

    let user_dir = user_db_dir_from(xdg, home);
    match &user_dir {
        Some(user_dir) => consider_update(
            user_dir.join("updates").join(crate::db_path::SCHEMA_SUBDIR),
            LayerOrigin::UserUpdates,
            &mut candidates,
            &mut skipped,
            probe,
        ),
        None => skipped.push(SkippedLayer {
            dir: None,
            origin: LayerOrigin::UserUpdates,
            reason: SkipReason::NoUserDataDir,
        }),
    }

    finish(candidates, skipped, source, dir, user_dir.as_deref(), probe)
}

/// Probe one update directory and either admit it to the competition or record
/// why it cannot take part.
fn consider_update(
    path: PathBuf,
    origin: LayerOrigin,
    candidates: &mut Vec<Candidate>,
    skipped: &mut Vec<SkippedLayer>,
    probe: &impl Probe,
) {
    let files = match probe.dir_files(&path) {
        Ok(None) => {
            skipped.push(SkippedLayer {
                dir: Some(path),
                origin,
                reason: SkipReason::Absent,
            });
            return;
        }
        // An unreadable update package is skipped, never fatal — the system
        // database is still perfectly loadable without it.
        Err(_) => {
            skipped.push(SkippedLayer {
                dir: Some(path),
                origin,
                reason: SkipReason::Unreadable,
            });
            return;
        }
        Ok(Some(files)) if files.is_empty() => {
            skipped.push(SkippedLayer {
                dir: Some(path),
                origin,
                reason: SkipReason::NoXmlFiles,
            });
            return;
        }
        Ok(Some(files)) => files,
    };
    // MITTEL-2: the hard-coded system-update path is only correct for a
    // default-prefix Linux build, and upstream's tie-break (`0 == 0` ⇒ system
    // updates win) would let a dateless directory there displace a real system
    // database. Require a real, positive `timestamp.txt` before it may compete.
    if origin == LayerOrigin::SystemUpdates && !probe.database_timestamp(&path).is_explicit_date() {
        skipped.push(SkippedLayer {
            dir: Some(path),
            origin,
            reason: SkipReason::NoTimestampFile,
        });
        return;
    }
    candidates.push(Candidate {
        origin,
        dir: path,
        files,
    });
}

/// Assemble the plan: pick the primary layer and, if the user database holds
/// XML, append it.
fn finish(
    mut candidates: Vec<Candidate>,
    mut skipped: Vec<SkippedLayer>,
    source: Source,
    dir: &Path,
    user_dir: Option<&Path>,
    probe: &impl Probe,
) -> Plan {
    let primary = if source == Source::EnvOverride {
        // An operator override is explicit intent and is never displaced — not
        // even by upstream's own newest-wins competition.
        LayerOrigin::SystemSchema
    } else {
        select_primary(&candidates, dir, probe)
    };
    // `select_primary` can name a candidate that is not present. That only
    // happens when *every* timestamp is `-1` — the system database has no
    // `timestamp.txt` and no update package exists at all — and upstream would
    // then call `LoadDirectory` on a directory that does not exist and load
    // nothing from any of the three `version_1` directories, i.e. silently lose
    // the whole system database. Keeping the resolved system database, which
    // resolution already proved holds XML, is the only outcome that preserves
    // the profiles; the divergence is documented in the module docs and pinned by
    // `an_undated_system_database_with_no_update_package_still_loads` in
    // `src/tests/db_layers.rs`.
    let chosen_index = candidates
        .iter()
        .position(|c| c.origin == primary)
        .unwrap_or_else(|| {
            debug_assert_eq!(
                primary,
                LayerOrigin::SystemUpdates,
                "only the all-`-1` case can name a missing candidate"
            );
            candidates
                .iter()
                .position(|c| c.origin == LayerOrigin::SystemSchema)
                .expect("the resolved system database always competes")
        });
    let chosen = candidates.swap_remove(chosen_index);
    let mut layers = vec![Layer {
        dir: chosen.dir,
        files: chosen.files,
        origin: chosen.origin,
    }];

    // The user database, merged unconditionally, exactly as
    // `lfDatabase::Load()` does. Profiles installed by `lf_db_save`/other tools
    // live here and must keep working (F1).
    match user_dir {
        Some(user_dir) => {
            let user_dir = user_dir.to_path_buf();
            match probe.dir_files(&user_dir) {
                Ok(Some(files)) if !files.is_empty() => layers.push(Layer {
                    dir: user_dir,
                    files,
                    origin: LayerOrigin::UserData,
                }),
                Ok(Some(_)) => skipped.push(SkippedLayer {
                    dir: Some(user_dir),
                    origin: LayerOrigin::UserData,
                    reason: SkipReason::NoXmlFiles,
                }),
                Ok(None) => skipped.push(SkippedLayer {
                    dir: Some(user_dir),
                    origin: LayerOrigin::UserData,
                    reason: SkipReason::Absent,
                }),
                Err(_) => skipped.push(SkippedLayer {
                    dir: Some(user_dir),
                    origin: LayerOrigin::UserData,
                    reason: SkipReason::Unreadable,
                }),
            }
        }
        // Both the user database *and* the user-update package are lost without
        // a user-data directory; `build` records the latter, this records the
        // former, so the plan accounts for both.
        None => skipped.push(SkippedLayer {
            dir: None,
            origin: LayerOrigin::UserData,
            reason: SkipReason::NoUserDataDir,
        }),
    }
    Plan { layers, skipped }
}

/// Line-for-line transcription of upstream's competition in
/// `lfDatabase::Load()` (lensfun 0.3.4).
///
/// ```text
/// if (M > S) { if (U > M) UserUpdates else main }
/// else       { if (U > S) UserUpdates else system_updates }
/// ```
///
/// A candidate that is not present scores `-1`, matching
/// `_lf_read_database_timestamp` for a missing directory and matching the
/// documented treatment of a directory that lost the XML pre-filter.
fn select_primary(candidates: &[Candidate], system_dir: &Path, probe: &impl Probe) -> LayerOrigin {
    let timestamp = |origin: LayerOrigin| -> DatabaseTimestamp {
        match candidates.iter().find(|c| c.origin == origin) {
            Some(c) => {
                let schema = if origin == LayerOrigin::SystemSchema {
                    system_dir.join(crate::db_path::SCHEMA_SUBDIR)
                } else {
                    c.dir.clone()
                };
                probe.database_timestamp(&schema)
            }
            None => DatabaseTimestamp::DirectoryAbsent,
        }
    };
    let (main, system_updates, user_updates) = (
        timestamp(LayerOrigin::SystemSchema).as_secs(),
        timestamp(LayerOrigin::SystemUpdates).as_secs(),
        timestamp(LayerOrigin::UserUpdates).as_secs(),
    );
    if main > system_updates {
        if user_updates > main {
            LayerOrigin::UserUpdates
        } else {
            LayerOrigin::SystemSchema
        }
    } else if user_updates > system_updates {
        LayerOrigin::UserUpdates
    } else {
        LayerOrigin::SystemUpdates
    }
}

/// glib's `g_get_user_data_dir()` / upstream's `HomeDataDir`.
///
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
