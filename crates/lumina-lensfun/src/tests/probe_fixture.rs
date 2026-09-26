//! Shared in-memory [`Probe`](crate::db_path::Probe) for the hermetic
//! LENSFUN-DB-33 tests (file-size ratchet, User-Vorgabe 2026-09-17).
//!
//! Holds no global state, so every test that uses it stays parallel-safe.
//!
//! # Time is modelled as `timestamp.txt`, never as an mtime
//!
//! The old version of this fixture modelled a directory's age as `newest_mtime`
//! and the tests built on it therefore *encoded the bug* they were meant to
//! catch: an `updates/version_1` directory without `timestamp.txt` scored a
//! "very new" mtime and displaced the whole system database, which is exactly
//! the opposite of upstream (`timestamp.txt` missing ⇒ `0` ⇒ never newer than a
//! dated system database). The fixture now stores the value of `timestamp.txt`
//! exactly as upstream computes it — see [`Timestamp`].

use crate::db_path::{MissReason, Probe};
use crate::db_timestamp::DatabaseTimestamp;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a fake directory's `timestamp.txt` says, expressed the way upstream
/// derives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timestamp {
    /// Directory absent or empty ⇒ upstream returns `-1`.
    Absent,
    /// `timestamp.txt` present but blank/whitespace-only ⇒ upstream returns
    /// `-1` too, for a completely different reason (the C++11 sentry fails on
    /// EOF while skipping whitespace, so `num_get` never runs). Measured.
    Blank,
    /// Directory present and non-empty, `timestamp.txt` missing/unreadable ⇒ `0`.
    NoFile,
    /// `timestamp.txt` holds this many seconds.
    At(i64),
}

impl From<Timestamp> for DatabaseTimestamp {
    fn from(value: Timestamp) -> Self {
        match value {
            Timestamp::Absent => DatabaseTimestamp::DirectoryAbsent,
            Timestamp::Blank => DatabaseTimestamp::BlankTimestampFile,
            Timestamp::NoFile => DatabaseTimestamp::NoTimestampFile,
            Timestamp::At(seconds) => DatabaseTimestamp::At(seconds),
        }
    }
}

/// In-memory probe: which directories exist, how many XML files they hold, and
/// what their `timestamp.txt` says.
pub struct FakeProbe {
    /// `dir` → XML count directly in `dir` (lensfun's `LoadDirectory`).
    pub dirs: BTreeMap<PathBuf, usize>,
    /// `dir` → what its `timestamp.txt` says. No entry = `Absent`.
    times: BTreeMap<PathBuf, Timestamp>,
}

impl FakeProbe {
    pub fn new(present: &[(&str, usize)]) -> Self {
        Self {
            dirs: present
                .iter()
                .map(|(dir, n)| (PathBuf::from(dir), *n))
                .collect(),
            times: BTreeMap::new(),
        }
    }

    /// `dir` holds a `timestamp.txt` with this UNIX-seconds value.
    pub fn dated(mut self, dir: &str, seconds: i64) -> Self {
        self.times
            .insert(PathBuf::from(dir), Timestamp::At(seconds));
        self
    }

    /// `dir` exists and is non-empty but has **no** `timestamp.txt` — upstream
    /// scores that `0`. This is the CASE-B shape that an mtime reading got
    /// backwards.
    pub fn undated(mut self, dir: &str) -> Self {
        self.times.insert(PathBuf::from(dir), Timestamp::NoFile);
        self
    }

    /// `dir` exists, is non-empty, and its `timestamp.txt` is blank or
    /// whitespace-only — upstream scores **that** `-1`, not `0` (measured).
    pub fn blank(mut self, dir: &str) -> Self {
        self.times.insert(PathBuf::from(dir), Timestamp::Blank);
        self
    }
}

impl Probe for FakeProbe {
    fn dir_files(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, MissReason> {
        Ok(self.dirs.get(dir).map(|n| {
            (0..*n)
                .map(|i| dir.join(format!("db{i}.xml")))
                .collect::<Vec<_>>()
        }))
    }

    /// Upstream reads the *directory* it timestamps (`main_dirname` is
    /// `…/version_1`, an update candidate is the `version_1` dir itself), so
    /// the lookup is exact — no normalisation, which keeps a test that stamps
    /// the wrong directory honest instead of accidentally passing.
    fn database_timestamp(&self, dir: &Path) -> DatabaseTimestamp {
        self.times
            .get(dir)
            .copied()
            .unwrap_or(Timestamp::Absent)
            .into()
    }
}
