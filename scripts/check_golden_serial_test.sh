#!/bin/sh
# Regression tests for check_golden_serial.sh. All fixtures live in a temp
# directory outside the repository; the checker parses them and never executes
# the cargo commands they contain.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECKER="$ROOT/scripts/check_golden_serial.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/check_golden_serial.XXXXXX") || exit 1
case "$WORK" in
    "$ROOT" | "$ROOT"/*)
        echo "FEHLER: Fixture-Verzeichnis liegt im Repository: $WORK" >&2
        exit 1
        ;;
esac
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

cat > "$WORK/unthreaded.md" <<'EOF'
cargo test -p lumina-gui --test kittest_snapshots -- --ignored
EOF
if sh "$CHECKER" --files "$WORK/unthreaded.md" > "$WORK/unthreaded.out" 2>&1; then
    echo "FEHLER: unthreaded Golden invocation was accepted." >&2
    cat "$WORK/unthreaded.out" >&2
    exit 1
fi
if ! grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/unthreaded.out"; then
    echo "FEHLER: unthreaded invocation failed without the expected diagnostic." >&2
    cat "$WORK/unthreaded.out" >&2
    exit 1
fi
echo "ok: unthreaded Golden invocation rejected with the serial-thread diagnostic."

cat > "$WORK/serial.sh" <<'EOF'
cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1
EOF
if ! sh "$CHECKER" --files "$WORK/serial.sh" > "$WORK/serial.out" 2>&1; then
    echo "FEHLER: serial Golden invocation was rejected." >&2
    cat "$WORK/serial.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 1 passende Aufrufe' "$WORK/serial.out"
echo "ok: serial Golden invocation accepted."

cat > "$WORK/historical.sh" <<'EOF'
cargo test -p lumina-gui --test kittest_snapshots -- --ignored  # GOLDEN-SERIAL-HISTORICAL: 2026-10-27 preserved measurement
EOF
if ! sh "$CHECKER" --files "$WORK/historical.sh" > "$WORK/historical.out" 2>&1; then
    echo "FEHLER: explicitly tagged historical invocation was rejected." >&2
    cat "$WORK/historical.out" >&2
    exit 1
fi
grep -Fq 'historical exemption:' "$WORK/historical.out"
grep -Fq 'GOLDEN-SERIAL-HISTORICAL: 2026-10-27 preserved measurement' "$WORK/historical.out"
echo "ok: same-line historical marker accepted and reported."

cat > "$WORK/continued.md" <<'EOF'
```sh
cargo test -p lumina-gui \
  --test kittest_snapshots -- \
  --ignored
```
EOF
if sh "$CHECKER" --files "$WORK/continued.md" > "$WORK/continued.out" 2>&1; then
    echo "FEHLER: unthreaded backslash-continued invocation was accepted." >&2
    cat "$WORK/continued.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/continued.out"
echo "ok: backslash-continued unthreaded invocation rejected."

cat > "$WORK/continued-serial.sh" <<'EOF'
cargo test -p lumina-gui \
  --test kittest_snapshots -- \
  --ignored --test-threads=1
EOF
if ! sh "$CHECKER" --files "$WORK/continued-serial.sh" > "$WORK/continued-serial.out" 2>&1; then
    echo "FEHLER: backslash-continued serial invocation was rejected." >&2
    cat "$WORK/continued-serial.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 1 passende Aufrufe' "$WORK/continued-serial.out"
echo "ok: backslash-continued serial invocation accepted."

cat > "$WORK/comment-token.sh" <<'EOF'
cargo test -p lumina-gui --test kittest_snapshots -- --ignored # --test-threads=1
EOF
if sh "$CHECKER" --files "$WORK/comment-token.sh" > "$WORK/comment-token.out" 2>&1; then
    echo "FEHLER: serial option in a shell comment was accepted." >&2
    cat "$WORK/comment-token.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/comment-token.out"
echo "ok: serial option hidden in a shell comment rejected."

cat > "$WORK/wrapped-inline.md" <<'EOF'
`cargo test -p lumina-gui --features gpu --test
kittest_local_mask_present -- --ignored`
EOF
if sh "$CHECKER" --files "$WORK/wrapped-inline.md" > "$WORK/wrapped-inline.out" 2>&1; then
    echo "FEHLER: wrapped unthreaded inline Markdown invocation was accepted." >&2
    cat "$WORK/wrapped-inline.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/wrapped-inline.out"
echo "ok: unthreaded command wrapped in inline Markdown rejected."

cat > "$WORK/wrapped-inline-serial.md" <<'EOF'
`cargo test -p lumina-gui --test
kittest_snapshots -- --ignored --test-threads=1`
EOF
if ! sh "$CHECKER" --files "$WORK/wrapped-inline-serial.md" > "$WORK/wrapped-inline-serial.out" 2>&1; then
    echo "FEHLER: wrapped serial inline Markdown invocation was rejected." >&2
    cat "$WORK/wrapped-inline-serial.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 1 passende Aufrufe' "$WORK/wrapped-inline-serial.out"
echo "ok: serial command wrapped in inline Markdown accepted."

cat > "$WORK/later-command.sh" <<'EOF'
cargo test -p lumina-gui --test kittest_snapshots -- --ignored && printf '%s\n' --test-threads=1
EOF
if sh "$CHECKER" --files "$WORK/later-command.sh" > "$WORK/later-command.out" 2>&1; then
    echo "FEHLER: serial option in a later shell command satisfied cargo test." >&2
    cat "$WORK/later-command.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/later-command.out"
echo "ok: serial option in a later && command does not satisfy cargo test."

cat > "$WORK/split-spans.md" <<'EOF'
`cargo test -p lumina-gui` `--test kittest_snapshots -- --ignored`
EOF
if sh "$CHECKER" --files "$WORK/split-spans.md" > "$WORK/split-spans.out" 2>&1; then
    echo "FEHLER: unthreaded command split across inline spans was accepted." >&2
    cat "$WORK/split-spans.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/split-spans.out"
echo "ok: unthreaded command split across adjacent inline spans rejected."

cat > "$WORK/split-spans-serial.md" <<'EOF'
`cargo test -p lumina-gui`
`--test kittest_snapshots -- --ignored --test-threads=1`
EOF
if ! sh "$CHECKER" --files "$WORK/split-spans-serial.md" > "$WORK/split-spans-serial.out" 2>&1; then
    echo "FEHLER: serial command split across wrapped inline spans was rejected." >&2
    cat "$WORK/split-spans-serial.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 1 passende Aufrufe' "$WORK/split-spans-serial.out"
echo "ok: serial command split across wrapped inline spans accepted."

cat > "$WORK/rust-doc.rs" <<'EOF'
/// "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"
EOF
if sh "$CHECKER" --files "$WORK/rust-doc.rs" > "$WORK/rust-doc.out" 2>&1; then
    echo "FEHLER: unthreaded Rust doc-comment command was accepted." >&2
    cat "$WORK/rust-doc.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/rust-doc.out"
echo "ok: unthreaded Rust doc-comment command rejected."

cat > "$WORK/rust-doc.rs" <<'EOF'
/// "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"
EOF
if ! sh "$CHECKER" --files "$WORK/rust-doc.rs" > "$WORK/rust-doc-serial.out" 2>&1; then
    echo "FEHLER: serial Rust doc-comment command was rejected." >&2
    cat "$WORK/rust-doc-serial.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 1 passende Aufrufe' "$WORK/rust-doc-serial.out"
echo "ok: serial Rust doc-comment command accepted."

cat > "$WORK/rust-ignore.rs" <<'EOF'
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
EOF
if sh "$CHECKER" --files "$WORK/rust-ignore.rs" > "$WORK/rust-ignore.out" 2>&1; then
    echo "FEHLER: unthreaded Rust #[ignore] reason command was accepted." >&2
    cat "$WORK/rust-ignore.out" >&2
    exit 1
fi
grep -Fq 'muss literal --test-threads=1 enthalten' "$WORK/rust-ignore.out"
echo "ok: unthreaded Rust #[ignore] reason command rejected."

cat > "$WORK/rust-ignore.rs" <<'EOF'
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
EOF
if ! sh "$CHECKER" --files "$WORK/rust-ignore.rs" > "$WORK/rust-ignore-serial.out" 2>&1; then
    echo "FEHLER: serial Rust #[ignore] reason command was rejected." >&2
    cat "$WORK/rust-ignore-serial.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 1 passende Aufrufe' "$WORK/rust-ignore-serial.out"
echo "ok: serial Rust #[ignore] reason command accepted."

if ! sh "$CHECKER" --files "$ROOT/crates/lumina-gui/tests/kittest_snapshots.rs" > "$WORK/source-current.out" 2>&1; then
    echo "FEHLER: current GUI test-source documentation was rejected." >&2
    cat "$WORK/source-current.out" >&2
    exit 1
fi
grep -Fq 'golden_serial OK: 2 passende Aufrufe' "$WORK/source-current.out"
echo "ok: both current Rust doc-comment invocations are recognized from source."

echo "check_golden_serial_test OK: 16 Fälle; kein cargo-Test wurde gestartet."
