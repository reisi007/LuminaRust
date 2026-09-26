//! Platform-dependent resolution of the Lensfun profile database (LENSFUN-DB-33).
//!
//! SOLL: `feature/platform/capability-matrix.md`, section
//! „Lensfun-Profil-Datenbank (plattformabhängige Auflösung, LENSFUN-DB-33)“.
//!
//! The *resolution order* (which `CONF_DATADIR` to use) is pure and lives here;
//! the *load plan* (which of upstream's four directories to read, and in which
//! order) is in [`crate::db_layers`], next to the algorithm it reproduces; the
//! "newer database" quantity upstream compares is in [`crate::db_timestamp`];
//! the named failures are in [`crate::db_error`].
//!
//! Everything here is pure apart from [`EnvValues::from_process`] and
//! [`resolve`], the two places that touch the real process environment.
//! [`resolve_with`] takes the environment values and the directory probe as
//! parameters, so the precedence rules are testable hermetically without
//! mutating global state.
//!
//! # The environment is read in exactly one place
//!
//! [`EnvValues::read`] is the *only* place that names an environment variable,
//! and production reaches it through [`EnvValues::from_process`] (which passes
//! `std::env::var_os`). Tests call the same function with a reader of their own,
//! so the mapping "which variable feeds which resolution input" is covered by a
//! test instead of being asserted by reading the source — a renamed or swapped
//! variable is a production change that turns
//! `tests::production_seam::environment::
//! the_resolution_reads_exactly_three_named_environment_values` red.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

pub use crate::db_error::{MissReason, ProbeMiss, SystemDbError};
pub use crate::db_layers::{Layer, LayerOrigin};

use crate::db_timestamp::DatabaseTimestamp;

/// Operator override: path of a directory that contains a `version_1/`
/// subdirectory with Lensfun XML files.
///
/// **Must be absolute.** A relative value is resolved by the OS against the
/// process working directory, which for a Finder/Dock-launched macOS app is
/// `/` — an operator would silently get a different database than intended.
/// [`resolve_with`] rejects a relative value as [`MissReason::NotADirectory`].
///
/// An override is also **never displaced**: it pins the system layer of the
/// load plan, so no update package can push it out (see [`crate::db_layers`]).
pub const DB_DIR_ENV: &str = "LUMINA_LENSFUN_DB";

/// `cargo:rustc-env` value emitted by `build.rs` from
/// `pkg-config --variable=datadir lensfun`.
pub const COMPILED_DATADIR_ENV: &str = "LUMINA_LENSFUN_COMPILED_DATADIR";

/// The build-time datadir `build.rs` baked in, or `None` when the build could
/// not determine one.
///
/// A `const`, not an `option_env!` at the use site: the *name* of the variable
/// is then a single, checkable constant instead of a string literal repeated at
/// every read, and a test can assert that the baked-in value is the one
/// `build.rs` promises.
pub const COMPILED_DATADIR: Option<&str> = option_env!("LUMINA_LENSFUN_COMPILED_DATADIR");

/// glib's user-data base (`g_get_user_data_dir()`), which wins unchecked when it
/// is set and non-empty — including a relative value. See
/// [`crate::db_layers::user_db_dir_from`].
pub const XDG_DATA_HOME_ENV: &str = "XDG_DATA_HOME";

/// glib's fallback for the user-data base, used only when
/// [`XDG_DATA_HOME_ENV`] is **not** set (`g_build_home_dir`).
pub const HOME_ENV: &str = "HOME";

/// The schema-version subdirectory the *system* database uses.
pub const SCHEMA_SUBDIR: &str = "version_1";

/// Distro default: Debian/Ubuntu `liblensfun1` ships here.
pub const LINUX_DEFAULT: &str = "/usr/share/lensfun";
/// Homebrew default on Apple Silicon macOS.
pub const MACOS_APPLE_SILICON: &str = "/opt/homebrew/share/lensfun";
/// Homebrew default on Intel macOS.
pub const MACOS_INTEL: &str = "/usr/local/share/lensfun";

/// lensfun's `SYSTEM_DB_UPDATE_PATH` for a default-prefix Linux build:
/// `SYSTEM_DB_UPDATE_PATH "/${CMAKE_INSTALL_LOCALSTATEDIR}/lib/lensfun-updates"`
/// (`include/lensfun/config.h.in.cmake`, lensfun 0.3.4).
///
/// This is a **build-time** value in the C library, so a NixOS, MacPorts or
/// custom-prefix build uses a different path that we cannot discover at
/// runtime (lensfun 0.3.4 exposes no environment variable for it). Hard-coding
/// the Debian/Ubuntu value is therefore only correct for that layout — which is
/// why the layer is treated defensively:
///
/// - it is part of upstream's load algorithm, so it is probed, not dropped;
/// - it may only win with a **real, positive `timestamp.txt`** *and* a
///   non-empty `*.xml` set (see [`crate::db_layers`]). Upstream's tie-break
///   (`0 == 0` ⇒ system updates win) would otherwise let a dateless directory
///   at a path that may not even belong to this build displace a real system
///   database;
/// - it can never displace an operator override.
///
/// Absent (the normal case on macOS and on most desktops) it is recorded in
/// [`Resolved::skipped`] as [`crate::db_layers::SkipReason::Absent`] and is not
/// reported as a defect.
pub const SYSTEM_UPDATES_DIR: &str = "/var/lib/lensfun-updates";

/// The `lensfun` subdirectory glib appends to the user data dir.
pub const USER_DB_SUBDIR: &str = "lensfun";

/// Which resolution step produced a directory. Also the precedence rank:
/// `EnvOverride` (0) beats `CompiledDataDir` (1) beats `PlatformDefault` (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// `LUMINA_LENSFUN_DB` was set.
    EnvOverride,
    /// The build baked in a datadir via `build.rs`.
    CompiledDataDir,
    /// A hard-coded, target-dependent default path.
    PlatformDefault,
}

impl Source {
    /// Human-readable, stable label used in the error `Display`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnvOverride => "Umgebungs-Override LUMINA_LENSFUN_DB",
            Self::CompiledDataDir => "kompiliertes LUMINA_LENSFUN_COMPILED_DATADIR",
            Self::PlatformDefault => "plattformabhängiger Default",
        }
    }
}

/// A successfully resolved database location and its full load plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The system database directory the **resolution** chose,
    /// e.g. `/opt/homebrew/share/lensfun`. This is the operator's pin when
    /// [`Self::source`] is [`Source::EnvOverride`].
    ///
    /// It is *not* necessarily the directory that gets loaded: upstream's
    /// newest-wins competition may pick an update package instead. The
    /// directory that is actually loaded is named by [`Self::primary`]; when
    /// the two disagree the caller is told through
    /// `Diagnostics::pin_displaced` ([`crate::system_load`]).
    pub dir: PathBuf,
    /// `dir/version_1`, the schema directory of the system database.
    pub schema_dir: PathBuf,
    /// The resolution step that produced `dir`.
    pub source: Source,
    /// Which layer supplies the system database, i.e. what `layers[0]` is.
    /// The single source of truth for "what was loaded".
    pub primary: LayerOrigin,
    /// Every directory to load, in order. `layers[0]` is always the layer named
    /// by [`Self::primary`]; a [`LayerOrigin::UserData`] entry, when present,
    /// is the user database that upstream `lfDatabase::Load()` merges
    /// unconditionally.
    pub layers: Vec<Layer>,
    /// Every considered-but-excluded layer, with a reason. Complete by
    /// construction — see [`crate::db_layers::Plan::skipped`].
    pub skipped: Vec<crate::db_layers::SkippedLayer>,
}

impl Resolved {
    /// Total number of XML files in the whole load plan.
    pub fn file_count(&self) -> usize {
        self.layers.iter().map(|l| l.files.len()).sum()
    }

    /// The user-database layer, if one was found.
    pub fn user_layer(&self) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|l| l.origin == LayerOrigin::UserData)
    }

    /// Whether the directory the resolution pinned is the one that is loaded.
    ///
    /// `false` means an update package won the competition. That is upstream
    /// parity for [`Source::CompiledDataDir`]/[`Source::PlatformDefault`] and
    /// impossible for [`Source::EnvOverride`] (which pins the layer), but it is
    /// still **reported** — `Resolved::dir` is what a caller would otherwise
    /// quote, and quoting it while something else loads is the silent
    /// contradiction LENSFUN-DB-33 exists to remove.
    pub fn pin_honored(&self) -> bool {
        self.primary == LayerOrigin::SystemSchema
    }
}

/// Reads databases from the filesystem. Injectable so [`resolve_with`] is
/// testable without touching the real disk.
pub trait Probe {
    /// The `*.xml` files **directly** in `dir` — what lensfun's
    /// `LoadDirectory(dir)` would load.
    ///
    /// `Ok(None)` = `dir` does not exist; `Ok(Some(vec![]))` = it exists but
    /// holds no XML; `Err(reason)` = it exists and could not be used. The
    /// distinction matters: reporting a permission error or a regular file as
    /// "directory not present" sends the operator to the wrong fix.
    fn dir_files(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, MissReason>;

    /// The value upstream's `_lf_read_database_timestamp(dir)` returns — the
    /// **content of `dir/timestamp.txt`**, not a file modification time.
    ///
    /// See [`crate::db_timestamp`] for the exact semantics and why an mtime is
    /// the wrong quantity. A `dir` that does not exist (or is empty) is
    /// [`DatabaseTimestamp::DirectoryAbsent`], *not* "no answer": the
    /// difference decides which database wins.
    fn database_timestamp(&self, dir: &Path) -> DatabaseTimestamp;

    /// The `*.xml` files in `dir/version_1`.
    fn schema_files(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, MissReason> {
        self.dir_files(&dir.join(SCHEMA_SUBDIR))
    }
}

/// Real filesystem probe.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsProbe;

/// Sorted `*.xml` file names directly in `dir`.
///
/// `Ok(None)` = the directory does not exist; `Ok(Some(vec![]))` = it exists but
/// holds no XML; `Err` = it exists and could not be used, with a reason that
/// says so (never a bare "absent" for a directory that is really there).
///
/// Only **regular** files are listed: a directory named `*.xml` cannot hold
/// profiles, so admitting it would let a candidate without a real database win
/// and then hard-fail (`AllFilesRejected`) instead of the documented
/// `NoXmlFiles` skip — at the cost of no `file_rejected` event for it.
pub fn xml_files_in(dir: &Path) -> Result<Option<Vec<PathBuf>>, MissReason> {
    if !dir.exists() {
        return Ok(None);
    }
    if !dir.is_dir() {
        return Err(MissReason::NotADirectory);
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Do not claim "absent" for a directory that exists but cannot be read.
        Err(_) => return Err(MissReason::Unreadable),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect();
    // Deterministic load order, independent of readdir order.
    files.sort();
    Ok(Some(files))
}

impl Probe for FsProbe {
    fn dir_files(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, MissReason> {
        xml_files_in(dir)
    }

    fn database_timestamp(&self, dir: &Path) -> DatabaseTimestamp {
        crate::db_timestamp::read(dir)
    }
}

/// The four inputs the resolution consumes — three process variables and the one
/// build-time constant — captured at one point in time. A struct rather than
/// four loose parameters because the *production* path must be the one the tests
/// drive: [`crate::system_load::report_with`] takes this, and
/// [`LensfunDb::resolve_system_with`](crate::system_load::LensfunDb::resolve_system_with)
/// builds it from the real process environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvValues {
    /// `LUMINA_LENSFUN_DB` — the operator override ([`DB_DIR_ENV`]).
    pub override_dir: Option<OsString>,
    /// The build-time datadir from [`COMPILED_DATADIR`], i.e. source 2 of the
    /// resolution order. **Not** readable at runtime and therefore **not** part
    /// of [`Self::read`]: a compile-time constant, which no environment can
    /// change and no test may pretend otherwise. Its own field — rather than a
    /// constant read inside [`Self::resolve`] — only so the hermetic suites can
    /// point the *system database* at a temp tree.
    pub compiled: Option<OsString>,
    /// `XDG_DATA_HOME` — glib's user-data base.
    pub xdg_data_home: Option<OsString>,
    /// `HOME` — glib's fallback for the user-data base.
    pub home: Option<OsString>,
}

impl EnvValues {
    /// Read the three process variables with `read`; [`Self::compiled`] stays
    /// `None` (only production fills it). This is the one and only place that
    /// names an environment variable. `read` is a parameter so the mapping is
    /// testable without `std::env::set_var`, which is undefined behaviour next
    /// to a concurrent `getenv` in any other thread — a test-local lock cannot
    /// make that safe.
    pub fn read(read: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            override_dir: read(DB_DIR_ENV),
            compiled: None,
            xdg_data_home: read(XDG_DATA_HOME_ENV),
            home: read(HOME_ENV),
        }
    }

    /// The real process environment plus the build-time datadir. This is what
    /// production calls, so there is no second reader to fall out of sync.
    pub fn from_process() -> Self {
        Self {
            compiled: COMPILED_DATADIR.map(OsString::from),
            ..Self::read(|name| std::env::var_os(name))
        }
    }

    /// The resolution these values produce, with the directory probe injected.
    pub fn resolve(&self, probe: &impl Probe) -> Result<Resolved, SystemDbError> {
        resolve_with(
            self.override_dir.as_deref(),
            self.compiled.as_deref(),
            self.xdg_data_home.as_deref(),
            self.home.as_deref(),
            probe,
        )
    }
}

/// The process-environment entry point: the real, loud resolution.
pub fn resolve() -> Result<Resolved, SystemDbError> {
    EnvValues::from_process().resolve(&FsProbe)
}

/// The resolution algorithm, with the environment and the probe injected.
///
/// `env` is the `LUMINA_LENSFUN_DB` value, `compiled` the build-time datadir,
/// `xdg`/`home` the user-data location. All may be `None`. Precedence is fixed:
/// env → compiled → platform default. The user database is then merged on top
/// the way `lfDatabase::Load()` does (unconditionally, flat, no `version_1`) —
/// see [`crate::db_layers`] for the deviations from that algorithm, and
/// `tests::db_layers::the_user_database_is_merged_on_top_of_the_system_database`
/// for the anchor.
pub fn resolve_with(
    env: Option<&OsStr>,
    compiled: Option<&OsStr>,
    xdg: Option<&OsStr>,
    home: Option<&OsStr>,
    probe: &impl Probe,
) -> Result<Resolved, SystemDbError> {
    let (dir, source, files) = resolve_primary(env, compiled, probe)?;
    let schema_dir = dir.join(SCHEMA_SUBDIR);
    let plan = crate::db_layers::build(source, &dir, &files, xdg, home, probe);
    // `build` always emits the chosen system layer first, so `primary` is read
    // back out of the plan instead of being decided a second time — one source
    // of truth for "what is loaded".
    let primary = plan.layers[0].origin;
    debug_assert!(
        !plan.layers[0].files.is_empty(),
        "the primary layer must hold XML, or the whole system database would be lost"
    );
    Ok(Resolved {
        dir,
        schema_dir,
        source,
        primary,
        layers: plan.layers,
        skipped: plan.skipped,
    })
}

/// The first step of [`resolve_with`]: which *system* database directory wins.
fn resolve_primary(
    env: Option<&OsStr>,
    compiled: Option<&OsStr>,
    probe: &impl Probe,
) -> Result<(PathBuf, Source, Vec<PathBuf>), SystemDbError> {
    let mut misses: Vec<ProbeMiss> = Vec::new();
    for (source, dir) in candidates(env, compiled) {
        // A relative path is resolved against the process working directory,
        // which for a Finder/Dock-launched app is `/` — the operator would get
        // a different database than they named. Reject it explicitly instead.
        if !dir.is_absolute() {
            misses.push(ProbeMiss {
                dir: dir.clone(),
                source,
                reason: MissReason::NotADirectory,
            });
            if source == Source::EnvOverride {
                return Err(override_error(dir, MissReason::NotADirectory));
            }
            continue;
        }
        match probe.schema_files(&dir) {
            Ok(None) => {
                misses.push(ProbeMiss {
                    dir: dir.clone(),
                    source,
                    reason: MissReason::Absent,
                });
                if source == Source::EnvOverride {
                    return Err(override_error(dir, MissReason::Absent));
                }
            }
            Err(reason) => {
                misses.push(ProbeMiss {
                    dir: dir.clone(),
                    source,
                    reason,
                });
                if source == Source::EnvOverride {
                    return Err(override_error(dir, reason));
                }
            }
            Ok(Some(files)) if files.is_empty() => {
                misses.push(ProbeMiss {
                    dir: dir.clone(),
                    source,
                    reason: MissReason::NoXmlFiles,
                });
                if source == Source::EnvOverride {
                    return Err(override_error(dir, MissReason::NoXmlFiles));
                }
            }
            Ok(Some(files)) => return Ok((dir, source, files)),
        }
    }
    Err(SystemDbError::NotFound { misses })
}

fn override_error(dir: PathBuf, reason: MissReason) -> SystemDbError {
    SystemDbError::OverrideUnusable { dir, reason }
}

/// The ordered, de-duplicated candidate list (highest precedence first).
pub fn candidates(env: Option<&OsStr>, compiled: Option<&OsStr>) -> Vec<(Source, PathBuf)> {
    let mut out: Vec<(Source, PathBuf)> = Vec::new();
    let mut push = |source: Source, dir: PathBuf| {
        if !out.iter().any(|(_, seen)| *seen == dir) {
            out.push((source, dir));
        }
    };
    if let Some(value) = env {
        push(Source::EnvOverride, PathBuf::from(value));
    }
    if let Some(dir) = compiled.and_then(lensfun_dir_in_datadir) {
        push(Source::CompiledDataDir, dir);
    }
    for dir in platform_default_dirs() {
        push(Source::PlatformDefault, dir);
    }
    out
}

/// Map a pkg-config `datadir` to the Lensfun directory inside it.
///
/// `…/share` → `…/share/lensfun`; a value that already ends in `lensfun` is
/// used as-is. An empty value yields `None` (no candidate).
pub(crate) fn lensfun_dir_in_datadir(datadir: &OsStr) -> Option<PathBuf> {
    let path = PathBuf::from(datadir);
    if path.as_os_str().is_empty() {
        return None;
    }
    if path.file_name() == Some(OsStr::new("lensfun")) {
        Some(path)
    } else {
        Some(path.join("lensfun"))
    }
}

/// The distro (Linux/BSD) default candidates.
///
/// Deliberately target-independent (a plain function, not a `cfg!` block) so
/// that the **Linux** layout — the CI container's `/usr/share/lensfun` — is
/// pinned by a test even when that test runs on macOS, and vice versa. An
/// untestable `cfg!` branch is how a "works on my machine" path survives.
pub fn linux_default_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from(LINUX_DEFAULT)]
}

/// The macOS default candidates, most specific first.
///
/// Apple Silicon Homebrew first on arm64, Intel Homebrew first on x86_64; the
/// other prefix stays in the list because a machine can have both installed,
/// and `/usr/share/lensfun` covers MacPorts and self-built installs.
pub fn macos_default_dirs(arch_is_arm64: bool) -> Vec<PathBuf> {
    let (primary, secondary) = if arch_is_arm64 {
        (MACOS_APPLE_SILICON, MACOS_INTEL)
    } else {
        (MACOS_INTEL, MACOS_APPLE_SILICON)
    };
    [primary, secondary, LINUX_DEFAULT]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Hard-coded, target-dependent defaults, most specific first.
pub fn platform_default_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        macos_default_dirs(cfg!(target_arch = "aarch64"))
    } else {
        linux_default_dirs()
    }
}
