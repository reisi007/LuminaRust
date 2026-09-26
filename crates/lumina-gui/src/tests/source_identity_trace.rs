//! THUMB-HASH-PERF-35: the memo's two *observability and write-guard* claims.
//!
//! Split out of `source_identity_cache.rs` (which keeps the memo-semantics
//! tests and owns the shared `Fixture`) purely to stay under the 500-line
//! file-size rule; the cohesion is real — these two tests are about what the
//! memo **reports** and what it **refuses to write**, not about identity
//! values.
//!
//! 1. A cache miss emits exactly one greppable `trace!` line, a hit emits none.
//!    That is the seam the SOLL names for a manual `RUST_LOG=trace`
//!    acceptance run (Agents.md R5-LOG-1), so it is pinned rather than
//!    promised.
//! 2. The TOCTOU guard: a file mutated *while* it is being hashed yields a
//!    torn digest that belongs to no key, and the memo's single write path
//!    must refuse it.

use super::{identity_of_bytes, payload, Fixture};
use crate::source_actions::FileContentIdentity;
use crate::source_identity;

/// A cold lookup must emit **exactly one** line in the format the SOLL
/// specifies, and a warm lookup must emit **nothing** — otherwise the count in
/// a trace run is meaningless (one line per frame instead of per file state).
///
/// The emission is `trace!` (via `timing::emit`), so at the default level the
/// format string is never even built.
#[test]
fn a_miss_emits_one_greppable_trace_line_and_a_hit_emits_none() {
    let bytes = payload(120 * 1024, 0x6B);
    let fixture = Fixture::new("traced.arw", &bytes);

    let _ = crate::timing::take_timing_log();
    assert_eq!(
        fixture.identity(),
        FileContentIdentity::Hashed(identity_of_bytes(&bytes))
    );
    let log = crate::timing::take_timing_log();
    assert_eq!(log.len(), 1, "a cold miss emits exactly one line: {log:?}");
    let line = &log[0];
    assert!(
        line.starts_with("GUI source identity hashed (cache miss) path="),
        "{line}"
    );
    assert!(
        line.contains(&format!("path={}", fixture.path.display())),
        "{line}"
    );
    assert!(
        line.contains(&format!("bytes={}", bytes.len())),
        "the line must carry the byte length: {line}"
    );
    let raw = line
        .rsplit("hash_ms=")
        .next()
        .expect("the line must carry hash_ms");
    let ms: f64 = raw.parse().expect("one-decimal parseable milliseconds");
    assert!(ms >= 0.0 && ms.is_finite(), "{line}");

    // A cache hit must be completely silent: this is what makes the count in a
    // trace run meaningful (one line per file state, not one per frame).
    for _ in 0..5 {
        let _ = fixture.identity();
    }
    assert!(
        crate::timing::take_timing_log().is_empty(),
        "warm lookups must not emit a miss line"
    );
}

/// A file mutated *while* it is being hashed yields a digest that belongs to no
/// key at all, so the memo's single write path must refuse a stamp that no
/// longer describes the file. The ~115 ms race window is not reproducible on
/// demand, so the test drives [`source_identity::store`] directly with a stamp
/// it deliberately invalidated — exactly the state the race leaves behind, which
/// makes the guard falsifiable instead of decorative.
#[test]
fn a_stamp_that_moved_during_the_read_is_not_memoized() {
    let bytes = payload(8 * 1024, 0x3C);
    let fixture = Fixture::new("torn.arw", &bytes);
    let stable = source_identity::current_key(&fixture.path).unwrap();
    assert!(
        stable.is_some(),
        "an ordinary file must have a memoizable stamp"
    );

    // The pure decision behind the guard.
    assert!(
        source_identity::stamp_survived(stable.as_ref(), stable.as_ref()),
        "an unchanged stamp survives the read and is memoizable"
    );
    assert!(
        !source_identity::stamp_survived(None, stable.as_ref()),
        "no pre-read stamp means nothing may be stored"
    );
    assert!(
        !source_identity::stamp_survived(stable.as_ref(), None),
        "a vanished file must not be stored under the pre-read stamp"
    );

    // Advance the stamp: a same-length rewrite with the mtime pinned back is
    // the adversarial case, and it advances `ctime`.
    let mut changed = bytes.clone();
    changed[0] ^= 0x01;
    std::fs::write(&fixture.path, &changed).unwrap();
    fixture.restore_mtime();
    let moved = source_identity::current_key(&fixture.path).unwrap();
    assert_ne!(moved, stable, "the rewrite must have advanced the stamp");
    assert!(
        !source_identity::stamp_survived(stable.as_ref(), moved.as_ref()),
        "a stamp that moved during the read must not count as surviving"
    );

    // The write path refuses the stale stamp — this is the torn read. Assert
    // against the *stale* key, not the current one: an entry filed under the
    // old stamp must be directly observable as absent, otherwise
    // `hash_count(path)` (which only ever reads the current key) could not
    // tell the guarded and unguarded implementations apart.
    source_identity::store(
        &fixture.path,
        stable.as_ref().unwrap(),
        &identity_of_bytes(&bytes),
    );
    assert_eq!(
        source_identity::hash_count_for(stable.as_ref().unwrap()),
        None,
        "a digest read from a file that moved must not be memoized under the old stamp"
    );
    assert_eq!(
        source_identity::hash_count(&fixture.path),
        None,
        "and nothing may appear under the current key either"
    );

    // And it accepts a stamp that is still current, so the guard filters rather
    // than blanket-refuses.
    source_identity::store(
        &fixture.path,
        moved.as_ref().unwrap(),
        &identity_of_bytes(&changed),
    );
    assert_eq!(
        source_identity::hash_count_for(moved.as_ref().unwrap()),
        Some(1),
        "a current stamp must be memoized"
    );
}
