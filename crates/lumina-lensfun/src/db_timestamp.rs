//! Upstream's "newer database" quantity: the **content of `timestamp.txt`**
//! (LENSFUN-DB-33, finding F1-BRUTK).
//!
//! lensfun 0.3.4 does **not** compare file modification times. `docs/manual-
//! main.txt` says it outright:
//!
//! > 'Newer database' means that the file **timestamp.txt**, which is amongst
//! > the XML files, contains a larger value.
//!
//! and the implementation (`libs/lensfun/auxfun.cpp`, tag `v0.3.4`) is:
//!
//! ```c
//! long int _lf_read_database_timestamp (const gchar *dirname)
//! {
//!     long int timestamp = -1;
//!     GDir *dir = g_dir_open (dirname, 0, NULL);
//!     if (dir) {
//!         if (g_dir_read_name (dir)) {
//!             gchar *filename = g_build_filename (dirname, "timestamp.txt", NULL);
//!             std::ifstream timestamp_file (filename);
//!             g_free (filename);
//!             if (!timestamp_file.fail ()) timestamp_file >> timestamp;
//!             else timestamp = 0;
//!         }
//!         g_dir_close (dir);
//!     }
//!     return timestamp;
//! }
//! ```
//!
//! Three consequences that an mtime-based re-implementation gets wrong, all of
//! them measured on a real install (system `timestamp.txt` = `1645386247`,
//! file mtimes = `1689186244` — different orders of magnitude, and an mtime can
//! be *newer* for a directory that upstream considers *older* or undated):
//!
//! 1. A `version_1` directory **without** `timestamp.txt` scores `0`, i.e. it
//!    can only ever lose against a dated system database. An mtime reading
//!    makes such a directory win (its files were just written) and displace the
//!    whole system database.
//! 2. A **non-existent** directory and an **empty** directory both score `-1`.
//!    An mtime probe cannot tell them apart from "no answer at all".
//! 3. The value is a **UNIX-seconds integer parsed from the file**, so a
//!    directory copied around keeps its upstream age instead of acquiring the
//!    mtime of the copy.
//!
//! # Deliberate deviation
//!
//! `lfDatabase::Load()` stores the three results in `const int` variables while
//! `_lf_read_database_timestamp` returns `long int`, so upstream silently
//! truncates anything above `INT_MAX` (2038-01-19). We compare in `i64` and do
//! **not** reproduce that narrowing — a wrapped timestamp is exactly the kind
//! of silent mis-ordering this module exists to prevent.

use std::fmt;
use std::path::Path;

/// The file whose **content** is the database timestamp. Upstream builds the
/// name as `g_build_filename (dirname, "timestamp.txt", NULL)`.
pub const TIMESTAMP_FILE: &str = "timestamp.txt";

/// What upstream's `_lf_read_database_timestamp` returns for one `version_1`
/// directory.
///
/// [`Self::as_secs`] is the quantity `lfDatabase::Load()` compares; the three
/// variants exist so a test and an operator can see *why* a value came out the
/// way it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseTimestamp {
    /// `-1` — the directory does not exist, is not a directory, cannot be
    /// opened, or is **empty** (`g_dir_open` failed, or `g_dir_read_name`
    /// returned NULL).
    DirectoryAbsent,
    /// `0` — the directory exists and is non-empty, but `timestamp.txt` is
    /// missing or could not be opened (`timestamp_file.fail()`).
    NoTimestampFile,
    /// The value parsed out of `timestamp.txt` (UNIX seconds). Upstream also
    /// produces `At(0)` here when the file opens but holds no parsable number,
    /// so `At(0)` and [`Self::NoTimestampFile`] compare equal — which is
    /// exactly what upstream compares.
    At(i64),
}

impl DatabaseTimestamp {
    /// The value upstream's `Load()` compares (`-1`, `0` or the parsed seconds).
    pub const fn as_secs(self) -> i64 {
        match self {
            Self::DirectoryAbsent => -1,
            Self::NoTimestampFile => 0,
            Self::At(value) => value,
        }
    }

    /// Whether this is a real, explicitly dated `timestamp.txt`.
    ///
    /// A missing, empty or unparsable `timestamp.txt` is **not** a date, and
    /// the system-update layer is only allowed to win with one (see
    /// [`crate::db_layers`]).
    pub const fn is_explicit_date(self) -> bool {
        matches!(self, Self::At(value) if value > 0)
    }
}

impl fmt::Display for DatabaseTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryAbsent => f.write_str("-1 (Verzeichnis fehlt oder ist leer)"),
            Self::NoTimestampFile => f.write_str("0 (timestamp.txt fehlt oder unlesbar)"),
            Self::At(value) => write!(f, "{value} (timestamp.txt)"),
        }
    }
}

/// Read the upstream timestamp of the `version_1` directory `dir`.
///
/// The three steps mirror the C function one for one; see the module docs for
/// the source.
pub fn read(dir: &Path) -> DatabaseTimestamp {
    // `g_dir_open` failing (absent, not a directory, no read permission) leaves
    // the initial `-1` in place.
    let Ok(mut entries) = std::fs::read_dir(dir) else {
        return DatabaseTimestamp::DirectoryAbsent;
    };
    // `g_dir_read_name` returning NULL — end of directory *or* a read error —
    // also leaves `-1` in place. Note this is a *different* question from
    // `xml_files_in`'s `Unreadable`: here we report what upstream computes, not
    // a diagnosis.
    match entries.next() {
        None | Some(Err(_)) => return DatabaseTimestamp::DirectoryAbsent,
        Some(Ok(_)) => {}
    }
    // `std::ifstream` construction failed -> `fail()` -> `timestamp = 0`.
    match std::fs::read(dir.join(TIMESTAMP_FILE)) {
        Ok(bytes) => parse(&bytes),
        Err(_) => DatabaseTimestamp::NoTimestampFile,
    }
}

/// `std::ifstream >> long int`, including the C++11 value-on-failure rules that
/// upstream depends on: it checks `fail()` **only before** the extraction, so
/// whatever the extraction leaves in the variable is what gets compared.
///
/// - leading whitespace is skipped, one optional `+`/`-` then decimal digits;
/// - the extraction stops at the first non-digit (trailing text is ignored);
/// - no digits at all ⇒ C++11 stores `0` and sets `failbit` ⇒ `At(0)`;
/// - out of `long int` range ⇒ C++11 saturates to `LONG_MAX`/`LONG_MIN` and
///   sets `failbit` ⇒ the saturated value.
fn parse(bytes: &[u8]) -> DatabaseTimestamp {
    let text = String::from_utf8_lossy(bytes);
    let body = text.trim_start();
    let (negative, digits) = match body.as_bytes().first() {
        Some(b'+') => (false, &body[1..]),
        Some(b'-') => (true, &body[1..]),
        _ => (false, body),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return DatabaseTimestamp::At(0);
    }
    match digits[..end].parse::<i64>() {
        Ok(value) => DatabaseTimestamp::At(if negative { -value } else { value }),
        Err(_) => DatabaseTimestamp::At(if negative { i64::MIN } else { i64::MAX }),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, DatabaseTimestamp};

    /// The real `timestamp.txt` shipped with the Homebrew lensfun 0.3.4
    /// database, so the parser is pinned against a real upstream artefact.
    const REAL_HOME_BREW: &str = "1645386247\n";

    #[test]
    fn parses_the_real_homebrew_timestamp_file() {
        assert_eq!(
            parse(REAL_HOME_BREW.as_bytes()),
            DatabaseTimestamp::At(1645386247)
        );
    }

    #[test]
    fn skips_leading_whitespace_and_ignores_trailing_text() {
        for text in ["  42", "\n\t1645386247", "7 rest of the line", "+9"] {
            assert!(
                matches!(parse(text.as_bytes()), DatabaseTimestamp::At(v) if v > 0),
                "{text:?} must parse to a positive timestamp"
            );
        }
        assert_eq!(parse(b"  42"), DatabaseTimestamp::At(42));
        assert_eq!(parse(b"  -5"), DatabaseTimestamp::At(-5));
    }

    /// No digits ⇒ C++11 stores 0 and sets `failbit`; upstream ignores the
    /// flag, so 0 is what gets compared. Not the same *reason* as a missing
    /// file, the same *value*.
    #[test]
    fn unparsable_content_compares_as_zero() {
        for text in ["", "   ", "abc", "-", "+", "  .5"] {
            assert_eq!(
                parse(text.as_bytes()),
                DatabaseTimestamp::At(0),
                "{text:?} must compare as 0"
            );
        }
    }

    /// Overflow saturates the way C++11 `num_get` does.
    #[test]
    fn overflow_saturates_like_cpp11() {
        assert_eq!(
            parse(b"99999999999999999999999"),
            DatabaseTimestamp::At(i64::MAX)
        );
        assert_eq!(
            parse(b"-99999999999999999999999"),
            DatabaseTimestamp::At(i64::MIN)
        );
    }

    #[test]
    fn the_three_variants_map_to_the_upstream_values() {
        assert_eq!(DatabaseTimestamp::DirectoryAbsent.as_secs(), -1);
        assert_eq!(DatabaseTimestamp::NoTimestampFile.as_secs(), 0);
        assert_eq!(DatabaseTimestamp::At(0).as_secs(), 0);
        assert!(DatabaseTimestamp::At(1).is_explicit_date());
        assert!(!DatabaseTimestamp::At(0).is_explicit_date());
        assert!(!DatabaseTimestamp::NoTimestampFile.is_explicit_date());
        assert!(!DatabaseTimestamp::DirectoryAbsent.is_explicit_date());
    }
}
