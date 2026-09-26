//! Upstream's "newer database" quantity: the **content of `timestamp.txt`**
//! (LENSFUN-DB-33, findings F1-BRUTK / MITTEL-1).
//!
//! lensfun 0.3.4 does **not** compare file modification times.
//! `docs/manual-main.txt` says it outright:
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
//! # Why this file is derived from measurement, not from reading the C++
//!
//! Four rounds of this task claimed the transcription was exact and were refuted
//! by measurement — always for the same reason: the *runtime* semantics of
//! `operator>>(long int&)`, of `std::ifstream`'s constructor, and of
//! `num_get`'s saturation were inferred from the source instead of observed.
//! They are not self-evident. C++11's `sentry` skips whitespace with `peek()`; on
//! an all-whitespace or empty file `peek()` hits EOF, sets `eofbit`, the sentry
//! then sets `failbit`, and `operator>>` returns **without ever calling
//! `num_get`** — so `timestamp` keeps its initialised `-1`. The
//! "store 0 on a failed conversion" rule only applies when the sentry
//! *succeeds*, i.e. when a non-whitespace byte is actually available. Three
//! further rows existed for the same reason: the `\v` in the C-locale whitespace
//! set, the saturation of a magnitude of exactly `2^63` to `LONG_MIN`, and the
//! difference between a `timestamp.txt` that cannot be **opened** (`0`) and one
//! that opens but cannot be **read** (`-1`).
//!
//! So the value table below is not a reading of the source. Every row was
//! **measured** by calling the real exported symbol
//! (`__Z27_lf_read_database_timestampPKc`, a `T` symbol in
//! `/opt/homebrew/lib/liblensfun.dylib`) over a fixture matrix of 54 hand-built
//! cases — the 46 content rows in `tests::db_timestamp_parsing::MEASURED` plus
//! the 8 file kinds in `tests::fs_probe::MEASURED_FILE_KINDS` — and a seeded
//! random byte corpus; this file reproduces the measurement. The harness lives
//! outside the repository (it must not become a dependency); the committed tests
//! reproduce its conclusions hermetically, with the measured values quoted
//! verbatim. The last re-measurement did **not** agree on every randomised case:
//! every deviation sat in the `LONG_MIN` band around `2^63` — and that is the
//! reason the numbers here are stated as measurements and not as properties of
//! the source: an earlier revision of this module claimed its random matrix
//! "agreed on every single row", which was a statement about a generator, not
//! about the code.
//!
//! # The measured value table
//!
//! The content rows are reproduced, with the measured values quoted verbatim, in
//! `tests::db_timestamp_parsing::MEASURED`; the rows about *what kind of file*
//! `timestamp.txt` is live in `tests::fs_probe::MEASURED_FILE_KINDS`.
//!
//! | situation | value |
//! | --- | --- |
//! | `g_dir_open` failed — absent, not a directory, unreadable | `-1` |
//! | `g_dir_read_name` NULL — directory has **no entries** | `-1` |
//! | `timestamp.txt` could not be **opened** (missing, permission, a UNIX socket) | `0` |
//! | `timestamp.txt` **opened** but its first byte cannot be read (a directory, a symlink to one) | `-1` |
//! | `timestamp.txt` is **empty or only ASCII whitespace** (sentry fails, `num_get` never runs) | `-1` |
//! | `timestamp.txt` opens, sentry succeeds, but holds no digits (`garbage`, `not-a-number`, `-`, `.5`, binary, a UTF-8 BOM) | `0` |
//! | `timestamp.txt` holds a decimal integer | that integer |
//! | …out of `long int` range | `LONG_MAX` / `LONG_MIN` |
//!
//! Consequences an mtime-based re-implementation gets wrong, all measured on a
//! real install (system `timestamp.txt` = `1645386247`, file mtimes =
//! `1689186244` — different orders of magnitude, and an mtime can be *newer* for
//! a directory upstream considers *older* or undated):
//!
//! 1. A `version_1` directory **without** `timestamp.txt` scores `0`, so it can
//!    only ever lose against a dated system database. An mtime reading makes
//!    such a directory win (its files were just written) and displaces the whole
//!    system database.
//! 2. A **non-existent** directory, an **empty** directory and a
//!    **blank `timestamp.txt`** all score `-1`. An mtime probe cannot tell them
//!    apart from "no answer at all".
//! 3. The value is a **UNIX-seconds integer parsed from the file**, so a
//!    directory copied around keeps its upstream age instead of acquiring the
//!    mtime of the copy.
//!
//! # Three details that a "reasonable" re-implementation gets wrong
//!
//! - **Whitespace is ASCII-only.** The sentry skips with `isspace` in the
//!   stream's locale, which is the classic `"C"` locale here, so it skips only
//!   `' ' \t \n \v \f \r`. A leading U+00A0 (NBSP) is *not* whitespace: upstream
//!   measured `0` for `"\u{a0} 42"` where a Unicode-aware `trim_start()` yields
//!   `42`. This file therefore skips bytes, never `char`s.
//! - **`LONG_MIN`, not `LONG_MAX`, on negative overflow.** C++11 `num_get`
//!   saturates to the `numeric_limits` bound matching the sign. Note that
//!   "negative overflow" is *not* the same as "more digits than `u64`": a
//!   magnitude of exactly `2^63` fits in `u64`, so the obvious `-value` on a
//!   saturated `i64::MAX` would produce `LONG_MIN + 1`. Measured for
//!   `i64::MIN`, `i64::MIN - 1` and `-9223372036854775808` behind whitespace:
//!   upstream is `LONG_MIN` in all three.
//! - **Open and read are different failures.** `std::ifstream` first *opens*,
//!   then *extracts*. Only the failed open takes the `else timestamp = 0`
//!   branch; a failed read fails the sentry and yields the initialised `-1`
//!   (see [`DatabaseTimestamp::TimestampUnreadable`]).
//!
//! # Deliberate deviation
//!
//! `lfDatabase::Load()` stores the three results in `const int` variables while
//! `_lf_read_database_timestamp` returns `long int`, so upstream silently
//! truncates anything above `INT_MAX` (2038-01-19). We compare in `i64` and do
//! **not** reproduce that narrowing — a wrapped timestamp is exactly the kind
//! of silent mis-ordering this module exists to prevent.
//!
//! That is the **only** known deviation. `read_dir`/`File::open` classify I/O
//! errors a little more coarsely than `g_dir_open`/`g_dir_read_name` do, but both
//! can only produce the `-1` this function already returns, so no value changes;
//! the *reason* attached to a `-1` can be coarser than upstream's, which is why
//! the reasons are named per variant. The one situation that is **not** covered
//! by a committed test is a **FIFO** as `timestamp.txt`: measured, upstream and
//! this implementation behave *identically* — both block in `open(2)` while no
//! writer is attached, and both block in the read while a writer is attached but
//! has not closed (libc++'s `filebuf` fills its whole buffer). A test could
//! therefore only assert a deadlock, so the row is documented in
//! `tests::fs_probe::MEASURED_FILE_KINDS` instead of being tested.
use std::fmt;
use std::path::{Path, PathBuf};

/// The file whose **content** is the database timestamp. Upstream builds the
/// name as `g_build_filename (dirname, "timestamp.txt", NULL)`.
pub const TIMESTAMP_FILE: &str = "timestamp.txt";

/// What upstream's `_lf_read_database_timestamp` returns for one `version_1`
/// directory.
///
/// [`Self::as_secs`] is the quantity `lfDatabase::Load()` compares. The variants
/// stay separate because **four different situations score `-1`**, and
/// collapsing them hides exactly the distinction that decides which database
/// wins (the module docs show how each value was measured).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseTimestamp {
    /// `-1` — `g_dir_open` failed (absent, not a directory, no read permission)
    /// **or** `g_dir_read_name` returned NULL because the directory has no
    /// entries.
    DirectoryAbsent,
    /// `0` — the directory exists and is non-empty, but `timestamp.txt` could
    /// not be **opened** (missing, a permission error, or a path the OS refuses
    /// to open — measured: a UNIX socket, `open(2)` ⇒ `ENXIO`): `fail()` was
    /// already true, so upstream assigned `0` explicitly.
    NoTimestampFile,
    /// `-1` — the directory exists, is non-empty, and `timestamp.txt` opened
    /// fine but is **empty or only ASCII whitespace**. Not the same as
    /// [`Self::NoTimestampFile`]: the C++11 sentry fails on EOF-while-skipping,
    /// so `operator>>` returns without ever calling `num_get` and the variable
    /// keeps the `-1` it was initialised with. Measured, not inferred.
    BlankTimestampFile,
    /// `-1` — `timestamp.txt` could be **opened**, but its first byte cannot be
    /// read. Measured: the file is a **directory**, or a symlink to one —
    /// `open(2)` accepts `O_RDONLY` on a directory and the `read` then fails
    /// with `EISDIR`, so the sentry hits the same wall it hits at EOF and
    /// `num_get` never runs.
    ///
    /// This is the row that separates "the file could not be opened" (`0`, see
    /// [`Self::NoTimestampFile`]) from "the file cannot be read" (`-1`). Getting
    /// it wrong is not cosmetic: `0` is *greater* than `-1`, so a
    /// `timestamp.txt` that is a directory would let its `version_1` beat a
    /// legitimately undated system database — the mirror image of the measured
    /// F1 defect, where an `updates/version_1` without any `timestamp.txt`
    /// displaced the whole system database.
    TimestampUnreadable,
    /// The value parsed out of `timestamp.txt` (UNIX seconds).
    ///
    /// `At(0)` means the sentry succeeded but `num_get` found no digits
    /// (`garbage`, `not-a-number`, `-`, `.5`, binary, a UTF-8 BOM) — C++11 stores
    /// `0` and sets `failbit`, which upstream ignores — so it compares equal to
    /// [`Self::NoTimestampFile`], which is what upstream compares.
    At(i64),
}

impl DatabaseTimestamp {
    /// The value upstream's `Load()` compares (`-1`, `0` or the parsed seconds).
    pub const fn as_secs(self) -> i64 {
        match self {
            Self::DirectoryAbsent | Self::BlankTimestampFile | Self::TimestampUnreadable => -1,
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
            Self::DirectoryAbsent => f.write_str("-1 (Verzeichnis fehlt, unlesbar oder ist leer)"),
            Self::NoTimestampFile => f.write_str("0 (timestamp.txt fehlt oder unlesbar)"),
            Self::BlankTimestampFile => {
                f.write_str("-1 (timestamp.txt ist leer/nur Leerraum: C++-Sentry scheitert)")
            }
            Self::TimestampUnreadable => f.write_str(
                "-1 (timestamp.txt ließ sich öffnen, ist aber nicht lesbar, z. B. ein Verzeichnis)",
            ),
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
    read_timestamp_file(dir.join(TIMESTAMP_FILE))
}

/// `std::ifstream timestamp_file (filename); …; if (!timestamp_file.fail ())
/// timestamp_file >> timestamp; else timestamp = 0;`
///
/// The two failure modes have to be told apart, because upstream scores them
/// differently, and `std::fs::read` fuses them into one `Err`:
///
/// - the **open** fails — the file is missing, unreadable, or a path the OS
///   refuses to open at all (measured: a **UNIX socket**, `open(2)` answers
///   `ENXIO`) ⇒ `fail()` was already true before the extraction ⇒ `0`;
/// - the open **succeeds** but the first byte cannot be read — measured: the
///   file is a **directory** or a symlink to one, `open(2)` takes `O_RDONLY` on
///   a directory and the `read` then fails with `EISDIR` ⇒ the sentry skips
///   whitespace with `peek()`, gets the error, sets `failbit`, and
///   `operator>>` returns **without calling `num_get`** ⇒ `-1`, exactly like an
///   empty file.
///
/// Collapsing both into `0` is a real mis-ordering, not cosmetics: `0` beats
/// `-1`, so such a `version_1` would displace a legitimately undated system
/// database.
fn read_timestamp_file(path: PathBuf) -> DatabaseTimestamp {
    use std::io::Read;
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        // `fail()` was true before the extraction ever started.
        Err(_) => return DatabaseTimestamp::NoTimestampFile,
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return DatabaseTimestamp::TimestampUnreadable;
    }
    parse(&bytes)
}

/// The measured value of `std::ifstream >> long int` for the given file
/// contents.
///
/// This is the part of upstream that **cannot** be read off the source, so each
/// branch below is annotated with the value the real exported symbol produced
/// (see the module docs). The two rules that a plausible-looking re-implementation
/// gets wrong:
///
/// 1. **Sentry failure is not a conversion failure.** The sentry skips
///    whitespace with `peek()`; if that hits EOF it sets `eofbit` *and then*
///    `failbit`, and `operator>>` returns **without calling `num_get`**. The
///    variable therefore keeps its initialised `-1` — upstream measured `-1` for
///    an empty file and for whitespace-only content, where the "store 0 on a
///    failed conversion" rule would say `0`.
/// 2. **Only ASCII whitespace is skipped.** The sentry uses `isspace` in the
///    stream's locale, which is the classic `"C"` locale, so a leading U+00A0 is
///    *not* whitespace: upstream measured `0` for `"\u{a0} 42"` where a
///    Unicode-aware trim yields `42`. This is why the scan is over bytes.
///
/// `pub(crate)` so the measured matrix can live in
/// `tests::db_timestamp_parsing` next to the other hermetic suites, instead of
/// growing this module past the 500-line ratchet. It is not part of the public
/// API: [`read`] is the entry point.
pub(crate) fn parse(bytes: &[u8]) -> DatabaseTimestamp {
    let body = skip_c_locale_space(bytes);
    if body.is_empty() {
        return DatabaseTimestamp::BlankTimestampFile;
    }
    let (negative, digits) = match body[0] {
        b'+' => (false, &body[1..]),
        b'-' => (true, &body[1..]),
        _ => (false, body),
    };
    let end = digits
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        // The sentry succeeded, `num_get` ran and found no digits: C++11 stores
        // `0` and sets `failbit`, which upstream ignores. Measured `0` for
        // `garbage`, `not-a-number`, `-`, `.5`, `\x01\x02\x03`, a UTF-8 BOM
        // and a single NUL byte.
        return DatabaseTimestamp::At(0);
    }
    DatabaseTimestamp::At(match decimal_magnitude(&digits[..end]) {
        // `magnitude` is the *absolute* value, so the sign is applied here — and
        // `i64::MIN` has no positive counterpart. Negating the saturated
        // `i64::MAX` instead (the obvious `-value`) yields `LONG_MIN + 1`, and
        // measurement says upstream saturates to `LONG_MIN` for every magnitude
        // above `i64::MAX` with a minus sign, not only for the ones that
        // overflow `u64`. Handled here rather than by saturating `u64` first,
        // so the `+`-sign case keeps the same code path.
        Some(magnitude) => match i64::try_from(magnitude) {
            // `magnitude <= i64::MAX` here, so `-value` cannot overflow.
            Ok(value) if negative => -value,
            Ok(value) => value,
            // `magnitude == 2^63`: the only `i64` it can be is `i64::MIN`.
            Err(_) if negative => i64::MIN,
            Err(_) => i64::MAX,
        },
        // C++11 `num_get` saturates to `numeric_limits<long>::max()` or
        // `::min()` according to the sign and sets `failbit`; upstream ignores
        // the flag, so the saturated value is what gets compared. Measured
        // `9223372036854775807` and `-9223372036854775808`.
        None if negative => i64::MIN,
        None => i64::MAX,
    })
}

/// `isspace` in the classic `"C"` locale: `' '`, `\t`, `\n`, `\v`, `\f`, `\r`.
///
/// **Not** `u8::is_ascii_whitespace()`: that helper omits `\v` (U+000B), because
/// Rust classifies it as a control character rather than whitespace. C's
/// `isspace` includes it, and the difference is observable — the real symbol
/// measured `-1` for a `timestamp.txt` holding a single `\v`, i.e. it treated
/// the byte as whitespace and the sentry ran into EOF. Substituting the "obvious"
/// standard-library helper here is exactly the class of bug this module documents.
pub(crate) fn is_c_locale_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

fn skip_c_locale_space(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < bytes.len() && is_c_locale_space(bytes[start]) {
        start += 1;
    }
    &bytes[start..]
}

/// The decimal value of `digits`, or `None` when it does not fit in `u64`
/// (i.e. the `i64`/`long` range is exceeded).
fn decimal_magnitude(digits: &[u8]) -> Option<u64> {
    digits.iter().try_fold(0u64, |acc, b| {
        acc.checked_mul(10)?.checked_add(u64::from(b - b'0'))
    })
}
