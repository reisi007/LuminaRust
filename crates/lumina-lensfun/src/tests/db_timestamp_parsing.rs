//! LENSFUN-DB-33: the **measured** value of `timestamp.txt` (findings
//! F1-BRUTK / MITTEL-1 / M1).
//!
//! Split out of `db_timestamp.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//! That module owns the algorithm; this one owns the *evidence* for it: the
//! value the real upstream symbol produced for every fixture, quoted verbatim,
//! so a regression is a test failure and not a re-litigation of the C++
//! semantics.
//!
//! # How the numbers were obtained
//!
//! A scratch C shim (outside the repository — it must not become a dependency)
//! declared the mangled name of lensfun 0.3.4's real exported symbol with an
//! explicit `asm` label, so the linker bound exactly
//! `__Z27_lf_read_database_timestampPKc` out of
//! `/opt/homebrew/lib/liblensfun.dylib`; `dladdr` and `dlsym` were checked to
//! resolve that binding to the same address. For every fixture directory the
//! shim printed the returned `long int`, the *same* tree was read by
//! [`crate::db_timestamp::read`] through the real `FsProbe`, and the two columns
//! were diffed.
//!
//! Method: 46 hand-built content rows (all below) plus the 8 filesystem-kind
//! rows in `tests::fs_probe::MEASURED_FILE_KINDS` (which need a real file
//! rather than a byte string), and a seeded random byte corpus, re-runnable.
//! **The fuzz did not agree on every row**: it hit the `LONG_MIN` band `-2^63`
//! — the negative inputs whose digits are exactly `2^63`, where the pre-fix
//! code returned `LONG_MIN + 1` and upstream returns `LONG_MIN`. That is the
//! `M1` off-by-one this file now pins. The exact hit count is
//! generator-specific (the reviewing verifier's independent generator: 267 of
//! 15 000 pre-fix, 0 post-fix); the numbers below are therefore *measured and
//! re-measured after the fix*, not claimed.

use crate::db_timestamp::{is_c_locale_space, parse, DatabaseTimestamp};

/// Every row below is a **measurement**, not a reading of the C++ source; see
/// the module docs for the method. The two columns are the file content and the
/// value the real symbol returned for it.
const MEASURED: &[(&str, &str, DatabaseTimestamp)] = &[
    // --- the sentry-failure cases: `-1`, NOT `0` -----------------------
    ("empty file", "", DatabaseTimestamp::BlankTimestampFile),
    (
        "spaces, tabs and newlines only",
        "  \t\n\n  ",
        DatabaseTimestamp::BlankTimestampFile,
    ),
    (
        "vertical tab only",
        "\u{b}\n",
        DatabaseTimestamp::BlankTimestampFile,
    ),
    (
        "form feed only",
        "\u{c}",
        DatabaseTimestamp::BlankTimestampFile,
    ),
    ("CRLF only", "\r\n", DatabaseTimestamp::BlankTimestampFile),
    (
        "a long run of spaces then 7",
        "        7",
        DatabaseTimestamp::At(7),
    ),
    // --- the sentry succeeds, `num_get` finds no digits: `0` -----------
    ("the literal 0", "0", DatabaseTimestamp::At(0)),
    ("garbage", "garbage", DatabaseTimestamp::At(0)),
    ("not-a-number", "not-a-number", DatabaseTimestamp::At(0)),
    (
        "binary garbage",
        "\u{1}\u{2}\u{3}",
        DatabaseTimestamp::At(0),
    ),
    (
        "UTF-8 text",
        "\u{e4}\u{f6}\u{fc} 42",
        DatabaseTimestamp::At(0),
    ),
    (
        "NBSP is NOT C-locale whitespace",
        "\u{a0} 42",
        DatabaseTimestamp::At(0),
    ),
    ("NBSP only", "\u{a0}", DatabaseTimestamp::At(0)),
    (
        "UTF-8 BOM then spaces",
        "\u{feff}   ",
        DatabaseTimestamp::At(0),
    ),
    ("a single NUL byte", "\0", DatabaseTimestamp::At(0)),
    (
        "an embedded NUL then digits",
        "\0 42",
        DatabaseTimestamp::At(0),
    ),
    ("a bare minus sign", "-", DatabaseTimestamp::At(0)),
    (
        "a decimal point, no digit before it",
        "  .5",
        DatabaseTimestamp::At(0),
    ),
    ("only zeros", "0000000000", DatabaseTimestamp::At(0)),
    (
        "hexadecimal-looking digits",
        "0x10",
        DatabaseTimestamp::At(0),
    ),
    ("a double sign", "--5", DatabaseTimestamp::At(0)),
    (
        "a plus followed by a minus",
        "+-5",
        DatabaseTimestamp::At(0),
    ),
    // --- the sentry succeeds and digits are parsed ----------------------
    (
        "the real Homebrew timestamp.txt",
        "1645386247\n",
        DatabaseTimestamp::At(1_645_386_247),
    ),
    ("a leading space then 42", "  42", DatabaseTimestamp::At(42)),
    (
        "trailing text is ignored",
        "42 rest of the line",
        DatabaseTimestamp::At(42),
    ),
    (
        "a trailing newline ends the number",
        "12\n34",
        DatabaseTimestamp::At(12),
    ),
    (
        "a trailing minus ends the number",
        "5-",
        DatabaseTimestamp::At(5),
    ),
    (
        "an exponent is not part of the number",
        "1e5",
        DatabaseTimestamp::At(1),
    ),
    ("an explicit plus sign", "+9", DatabaseTimestamp::At(9)),
    ("a negative value", "-5", DatabaseTimestamp::At(-5)),
    (
        "a negative value after whitespace",
        "   -5",
        DatabaseTimestamp::At(-5),
    ),
    ("negative zero is zero", "-0", DatabaseTimestamp::At(0)),
    // --- the `int` range, which upstream narrows to but we do not -------
    (
        "INT_MAX",
        "2147483647",
        DatabaseTimestamp::At(2_147_483_647),
    ),
    (
        "INT_MAX plus one (upstream would wrap to INT_MIN)",
        "2147483648",
        DatabaseTimestamp::At(2_147_483_648),
    ),
    (
        "INT_MIN",
        "-2147483648",
        DatabaseTimestamp::At(-2_147_483_648),
    ),
    // --- out of `long int` range: C++11 saturates by sign ---------------
    (
        "exactly LONG_MAX",
        "9223372036854775807",
        DatabaseTimestamp::At(i64::MAX),
    ),
    (
        "LONG_MAX plus one saturates to LONG_MAX",
        "9223372036854775808",
        DatabaseTimestamp::At(i64::MAX),
    ),
    (
        "LONG_MAX plus one behind a plus sign",
        "+9223372036854775808",
        DatabaseTimestamp::At(i64::MAX),
    ),
    (
        "u64::MAX saturates to LONG_MAX",
        "18446744073709551615",
        DatabaseTimestamp::At(i64::MAX),
    ),
    (
        "positive overflow saturates to LONG_MAX",
        "99999999999999999999999",
        DatabaseTimestamp::At(i64::MAX),
    ),
    // --- the band the fuzz found: every negative magnitude above LONG_MAX
    // is LONG_MIN, never LONG_MIN + 1 (finding M1) ----------------------
    (
        "exactly LONG_MIN",
        "-9223372036854775808",
        DatabaseTimestamp::At(i64::MIN),
    ),
    (
        "LONG_MIN behind whitespace",
        "   -9223372036854775808",
        DatabaseTimestamp::At(i64::MIN),
    ),
    (
        "LONG_MIN minus one saturates to LONG_MIN",
        "-9223372036854775809",
        DatabaseTimestamp::At(i64::MIN),
    ),
    (
        "nineteen nines with a minus sign saturate to LONG_MIN",
        "-9999999999999999999",
        DatabaseTimestamp::At(i64::MIN),
    ),
    (
        "u64::MAX with a minus sign saturates to LONG_MIN",
        "-18446744073709551615",
        DatabaseTimestamp::At(i64::MIN),
    ),
    (
        "negative overflow saturates to LONG_MIN",
        "-99999999999999999999999",
        DatabaseTimestamp::At(i64::MIN),
    ),
];

/// The measured matrix, row by row. A mutation that changes any single
/// branch of `parse` fails here.
#[test]
fn every_measured_file_content_parses_to_the_measured_value() {
    for (label, content, expected) in MEASURED {
        assert_eq!(
            &parse(content.as_bytes()),
            expected,
            "measured value for `{label}` (content {content:?}) changed"
        );
    }
}

/// **M1: `LONG_MIN` must be representable.** `i64::MIN` is its own negation
/// problem: `-i64::MAX` is `LONG_MIN + 1`, so a magnitude of `2^63` — which
/// *fits* in `u64`, hence is not the "does not fit at all" case — comes out one
/// off. The fuzz hit this band, and an earlier revision of this file carried a
/// claim of "0 mismatches" that the generator had simply never stepped into.
///
/// The magnitude `2^63` is therefore pinned in both spellings (with and without
/// leading whitespace) and the *representable* neighbours are pinned with it, so
/// a fix that hard-wires `i64::MIN` without looking at the sign is caught too.
#[test]
fn long_min_is_a_value_the_parser_can_produce() {
    for content in [
        "-9223372036854775808",
        "   -9223372036854775808",
        "-9223372036854775809",
        "-9999999999999999999",
        "-18446744073709551615",
    ] {
        assert_eq!(
            parse(content.as_bytes()),
            DatabaseTimestamp::At(i64::MIN),
            "{content:?} must be LONG_MIN, not LONG_MIN + 1"
        );
        assert_eq!(
            parse(content.as_bytes()).as_secs(),
            i64::MIN,
            "and it must compare as LONG_MIN, the way upstream's Load() compares"
        );
    }
    // The bug is specifically the negation of a saturated `i64::MAX`, so the
    // neighbouring magnitudes must keep their exact values.
    assert_eq!(
        parse(b"9223372036854775807"),
        DatabaseTimestamp::At(i64::MAX)
    );
    assert_eq!(
        parse(b"9223372036854775808"),
        DatabaseTimestamp::At(i64::MAX),
        "one above LONG_MAX still saturates upwards, not downwards"
    );
    assert_eq!(parse(b"-5"), DatabaseTimestamp::At(-5));
    // A negative value whose magnitude *is* representable must not be clamped to
    // LONG_MIN either — the saturation is a fallback, not a rule.
    assert_eq!(
        parse(b"-9223372036854775807"),
        DatabaseTimestamp::At(-9_223_372_036_854_775_807)
    );
    assert_eq!(parse(b"-0"), DatabaseTimestamp::At(0));
}

/// The two rows that earlier revisions got wrong, isolated so the reason for
/// the dedicated `BlankTimestampFile` variant is legible on its own.
///
/// The failure mode this guards: the C++11 sentry for `operator>>(long&)`
/// skips whitespace with `peek()`; on an all-whitespace file `peek()` hits
/// EOF, sets `eofbit`, the sentry then sets `failbit`, and `operator>>`
/// returns **without ever calling `num_get`** — so the variable keeps the
/// `-1` it was initialised with. Reading the source as "failed conversion
/// stores 0" is what produced `0` here, and measurement says `-1`.
#[test]
fn a_blank_timestamp_file_is_minus_one_not_zero() {
    assert_eq!(parse(b""), DatabaseTimestamp::BlankTimestampFile);
    assert_eq!(parse(b"   \t\n"), DatabaseTimestamp::BlankTimestampFile);
    assert_eq!(
        parse(b"\x0b"),
        DatabaseTimestamp::BlankTimestampFile,
        "a lone vertical tab is C-locale whitespace, so the sentry runs into EOF"
    );
    // …and it compares as -1, which is the point.
    assert_eq!(
        DatabaseTimestamp::BlankTimestampFile.as_secs(),
        -1,
        "upstream compares this value, and it is -1"
    );
    // …but it is NOT the same *reason* as a missing file, which is 0.
    assert_ne!(
        DatabaseTimestamp::BlankTimestampFile,
        DatabaseTimestamp::NoTimestampFile
    );
    assert_eq!(DatabaseTimestamp::NoTimestampFile.as_secs(), 0);
}

/// `u8::is_ascii_whitespace()` is **not** C's `isspace`: it omits `\v`
/// (U+000B), which `isspace` includes. Measured: the real symbol returned
/// `-1` for a `timestamp.txt` holding one `\v`, i.e. it skipped the byte and
/// then hit EOF. Substituting the obvious standard-library helper is exactly
/// the mistake this test exists to catch.
#[test]
fn whitespace_is_the_c_locale_set_including_vertical_tab() {
    for byte in [b' ', b'\t', b'\n', 0x0B, 0x0C, b'\r'] {
        assert!(
            is_c_locale_space(byte),
            "byte {byte:#04x} is C-locale whitespace"
        );
        assert_eq!(
            parse(&[byte]),
            DatabaseTimestamp::BlankTimestampFile,
            "a file holding only {byte:#04x} must score -1"
        );
    }
    // U+00A0 and U+FEFF are whitespace to Rust but not to `isspace`, which
    // is why the scan is over bytes with an explicit set.
    for byte in [0xC2u8, 0xEF, 0xA0, 0xBB] {
        assert!(
            !is_c_locale_space(byte),
            "byte {byte:#04x} must not be treated as C-locale whitespace"
        );
    }
    // The regression in one line: `is_ascii_whitespace` is false for 0x0B.
    assert!(
        !0x0Bu8.is_ascii_whitespace() && is_c_locale_space(0x0B),
        "this test is only meaningful while the two disagree on \\v"
    );
}

/// The comparison helpers agree with the measured values they summarise, and
/// every `-1` reason keeps its own identity: four situations score `-1`, and
/// collapsing them is what made the "opened but unreadable" case (M2)
/// indistinguishable from a missing file.
#[test]
fn the_variants_map_to_the_upstream_values() {
    assert_eq!(DatabaseTimestamp::DirectoryAbsent.as_secs(), -1);
    assert_eq!(DatabaseTimestamp::BlankTimestampFile.as_secs(), -1);
    assert_eq!(DatabaseTimestamp::TimestampUnreadable.as_secs(), -1);
    assert_eq!(DatabaseTimestamp::NoTimestampFile.as_secs(), 0);
    assert_eq!(DatabaseTimestamp::At(0).as_secs(), 0);
    assert!(DatabaseTimestamp::At(1).is_explicit_date());
    assert!(!DatabaseTimestamp::At(0).is_explicit_date());
    assert!(!DatabaseTimestamp::At(i64::MIN).is_explicit_date());
    assert!(!DatabaseTimestamp::NoTimestampFile.is_explicit_date());
    assert!(!DatabaseTimestamp::BlankTimestampFile.is_explicit_date());
    assert!(!DatabaseTimestamp::TimestampUnreadable.is_explicit_date());
    assert!(!DatabaseTimestamp::DirectoryAbsent.is_explicit_date());
}

/// Each `-1` reason has its own message, so a blank `timestamp.txt` is never
/// diagnosed as a missing one — and a `timestamp.txt` that opened but cannot be
/// read is never diagnosed as a missing one either.
#[test]
fn the_minus_one_reasons_stay_distinguishable() {
    let texts: Vec<String> = [
        DatabaseTimestamp::DirectoryAbsent,
        DatabaseTimestamp::BlankTimestampFile,
        DatabaseTimestamp::TimestampUnreadable,
    ]
    .iter()
    .map(std::string::ToString::to_string)
    .collect();
    let mut sorted = texts.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), texts.len(), "duplicate wording in {texts:?}");
    assert!(DatabaseTimestamp::BlankTimestampFile
        .to_string()
        .contains("leer"));
    assert!(DatabaseTimestamp::NoTimestampFile
        .to_string()
        .contains("fehlt"));
    assert!(
        DatabaseTimestamp::TimestampUnreadable
            .to_string()
            .contains("Verzeichnis"),
        "the unopenable-vs-unreadable distinction must be visible in the text: {}",
        DatabaseTimestamp::TimestampUnreadable
    );
}
