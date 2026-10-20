#!/bin/sh
# TEST-ORPHAN-97, Abnahme-Punkt (4): a test file that NO compilation target
# reads. Exit 0 = none, 1 = orphan found, 2 = cannot conclude (build first).
#
# The defect class, measured at commit 4a8bad5: 16 test files were re-added
# under `src/tests/` with +4133 lines and **not a single `mod` declaration**.
# cargo never opened them, so no run went red, the files read like live
# coverage, and the only thing that told the truth was rustc's own dep-info.
# That is exactly what this gate re-checks on every build.
#
# THE CRITERION (and nothing else):
#
#     every `*.rs` under crates/<crate>/src/tests/ must appear in the
#     dep-info of at least one compilation unit of package <crate>.
#
# It deliberately does NOT compare file bodies. The withdrawn TEST-DEDUP-100
# acceptance asked for "two identical bodies"; measured, that pattern fires 97
# times on files cargo never reads and still misses the real defect. This gate
# reports "exists, but nothing compiles it", which is the finding.
#
# Why dep-info and not a source walk: a filesystem walk has no concept of a
# compilation target — that method error produced the withdrawn finding in the
# first place. dep-info is rustc's own list of files it actually opened, so it
# is correct by construction about files reached through `#[path]`
# (`src/tests/navigator.rs` -> `preview_zoom_mode.rs`) and about nested
# submodules (`src/tests/geometry/crop_session/r5_tools.rs` is reached through
# `geometry.rs` + `crop_session.rs`, not through a flat walk). Both classes are
# present in this tree today and both are green.
#
# Scope and named limits (DoD §9 — a claim without its limit is not a claim):
#   * Default scope is `crates/lumina-gui`, the acceptance's subject. Five more
#     crates carry `src/tests/` (`lumina-cli`, `lumina-core`, `lumina-gpu`,
#     `lumina-lensfun`, `lumina-sidecar`) and are covered ONLY MANUALLY: pass
#     the crate name as `$1`. Only `lumina-gui` has a default-feature sentinel
#     (see below); the other crates need current default-feature artifacts, and
#     the gate cannot tell a lean build from a full one for them.
#   * The gate can only read what was built. It needs artifacts from a
#     **default-feature build that includes the test target**; without them it
#     exits 2 instead of guessing.
#   * `crates/lumina-gui/src/tests/lensfun_diagnostics.rs` is reached through
#     `#[cfg(feature = "lensfun")] mod lensfun_diagnostics;`, so a lean
#     `--no-default-features` build makes it absent from dep-info. The sentinel
#     below turns that case into exit 2 rather than a false orphan finding.
#   * Feature-gated test files are DERIVED from the code, never hand-listed:
#     every `*.rs` under `src/tests/` (a nested submodule file declares its own
#     children, e.g. `geometry.rs` -> `crop_session`) plus the crate-root
#     `mod tests { ... }` block. `lumina-gui` declares its whole test tree
#     INLINE in `src/lib.rs` and has no `src/tests/mod.rs` at all, so a
#     tests-tree-only scan is inert for it. Only a file whose gated `mod` name
#     matches it exactly is ignored, so a typo'd declaration still leaves an
#     ORPHAN. The OK line names how many files that is.
#   * A stale artifact from a DIFFERENT feature set can still list an orphan
#     and mask it. The freshness filter drops every dep-info older than the
#     sources it was built from, which covers the common case; it cannot
#     distinguish two same-age artifacts with different features.
#
# Usage:
#   sh scripts/check_test_orphans.sh                # crates/lumina-gui
#   sh scripts/check_test_orphans.sh lumina-core    # another crate
#   LUMINA_DEP_INFO_DIR=target/release/deps sh scripts/check_test_orphans.sh
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8 — Defektklasse und Anlass
# stehen oben, hier nur die noch fehlenden Teile):
#   Ratchet-Richtung: Boolean-Gate (keine Baseline-Datei) — die Zahl der
#                     Waisen muss 0 bleiben; es gibt keinen Wert, der steigen
#                     darf, und keinen Paydown-Pfad (die Waise wird behoben,
#                     nicht weggerechnet).
#   Nicht gedeckt:     die NAMED LIMITS oben (Scope je Crate, benoetigte
#                     Artefakte, Frische, Feature-Satz-Zuordnung).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="${1:-lumina-gui}"
CRATE_DIR="$ROOT/crates/$CRATE"
TESTS_DIR="$CRATE_DIR/src/tests"
DEP_INFO_DIR="${LUMINA_DEP_INFO_DIR:-$ROOT/target/debug/deps}"
# rustc turns `lumina-gui` into the unit prefix `lumina_gui-<hash>.d`.
PREFIX=$(printf '%s' "$CRATE" | tr '-' '_')
# Sentinels and feature sensitivity: only lumina-gui carries a test module that
# is only reached with default features; for all other supported crates the
# dependent units get checked without a per-crate sentinel.
SENTINEL=""
case "$CRATE" in
    lumina-gui)
        SENTINEL="crates/lumina-gui/src/tests/lensfun_diagnostics.rs"
        ;;
esac

WORK=$(mktemp -d "${TMPDIR:-/tmp}/check_test_orphans.XXXXXX") || exit 1
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

if [ ! -d "$TESTS_DIR" ]; then
    echo "FAIL  $TESTS_DIR does not exist - did the test tree move?" >&2
    exit 1
fi

# ---------------------------------------------------------------- 1. what is on disk
find "$TESTS_DIR" -name '*.rs' | LC_ALL=C sort > "$WORK/on_disk"
seen=$(wc -l < "$WORK/on_disk" | tr -d ' ')
if [ "$seen" -eq 0 ]; then
    # A gate that finds nothing to check is a gate that cannot fail (DoD §10).
    echo "FAIL  no *.rs under ${TESTS_DIR#$ROOT/} - nothing to check, so a green result would be vacuous" >&2
    exit 2
fi

# ------------------------------------------- 1b. feature-gated test files
# Files that are only reachable under a `#[cfg(feature = ...)]` mod declaration
# are legitimately absent from a lean build and must not look like orphans. The
# list is derived from the code, so a newly gated test module lands here
# without a script edit - but only if the derivation reads EVERY declaration
# site of the test tree. Two of them carry file-module declarations, and both
# are read:
#
#   (1) every `*.rs` under $TESTS_DIR. A nested submodule file declares its
#       own children (`src/tests/geometry.rs` -> `mod crop_session;`), and a
#       gated `mod.rs` hides files that a root-only scan never sees.
#   (2) the crate-root `mod tests { ... }` block, brace-matched so production
#       `mod`s in the same file cannot leak in. `lumina-gui` declares its
#       whole test tree INLINE in `src/lib.rs` (there is no `src/tests/mod.rs`
#       at all), so a tests-tree-only scan is inert for it - MEASURED at
#       ff4e071: one extra gated module made the gate report a false ORPHAN
#       (TEST-ORPHAN-SCOPE-60 F1). The root file is found by grep, not
#       hardcoded, so the derivation follows the code if the tree moves.
#
# The parser accepts the writing styles a gated declaration comes in
# (one-liner, `pub mod`, indentation, multi-line attribute, all/any/not
# compositions); a missed style would itself be a false ORPHAN (F2). Only a
# file whose gated `mod` name matches it EXACTLY is ignored, so a typo'd
# declaration keeps its file under comparison and stays red.
: > "$WORK/ignore"
cat > "$WORK/gated.awk" <<'AWK'
# Prints the NAME of every file-module declaration (`mod NAME;`) that is
# gated behind a `#[cfg(... feature ...)]` attribute, in any writing style.
function handle_code(rest,    name) {
    if (pending && rest ~ /^[[:space:]]*(pub[[:space:]]*(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/) {
        name = rest
        sub(/^[[:space:]]+/, "", name)
        sub(/^pub[[:space:]]*(\([^)]*\))?[[:space:]]+/, "", name)
        sub(/^mod[[:space:]]+/, "", name)
        sub(/[^A-Za-z0-9_].*$/, "", name)
        print name
        pending = 0
        return
    }
    pending = 0
}
BEGIN { pending = 0; in_attr = 0; in_block = 0; attr = "" }
{
    line = $0
    if (in_block) {                       # inside a /* ... */ comment
        if (line ~ /\*\//) { sub(/^.*\*\//, "", line); in_block = 0 } else next
    }
    while (length(line) > 0) {
        rest = line
        sub(/^[[:space:]]+/, "", rest)
        if (rest == "") break
        if (in_attr) {                    # continuation of a multi-line #[..]
            attr = attr " " rest
            if (rest ~ /\]/) {
                sub(/^[^]]*\][[:space:]]*/, "", line)
                in_attr = 0
                if (attr ~ /cfg\(/ && attr ~ /feature[[:space:]]*=/) pending = 1
                continue
            }
            break
        }
        if (rest ~ /^#\[/) {
            if (rest ~ /\]/) {            # whole attribute on one line
                if (rest ~ /cfg\(/ && rest ~ /feature[[:space:]]*=/) pending = 1
                sub(/^[^]]*\][[:space:]]*/, "", line)
                continue
            }
            attr = rest                   # attribute continues on the next line
            in_attr = 1
            break
        }
        if (rest ~ /^\/\//) break         # comment between attribute and mod
        if (rest ~ /^\/\*/) {
            if (line ~ /\*\//) { sub(/^[[:space:]]*\/\*.*\*\//, "", line); continue }
            in_block = 1
            break
        }
        handle_code(rest)
        break
    }
}
AWK
cat > "$WORK/tests_block.awk" <<'AWK'
# Prints the `mod tests { ... }` block of a crate-root file and nothing else,
# so the derivation above cannot pick up production `mod`s from the same file.
BEGIN { inblk = 0; depth = 0 }
{
    line = $0
    sub(/[[:space:]]*\/\/.*$/, "", line)
    if (inblk == 0 && line ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{/) {
        inblk = 1
    } else if (inblk == 0) {
        next
    }
    print line
    n = gsub(/\{/, "", line); depth += n
    m = gsub(/\}/, "", line); depth -= m
    if (depth <= 0) inblk = 0
}
AWK

# (1) the tests tree: a gated child resolves next to the declaring file.
find "$TESTS_DIR" -name '*.rs' | LC_ALL=C sort | while IFS= read -r manifest; do
    base=$(dirname "$manifest")
    awk -f "$WORK/gated.awk" "$manifest" | while IFS= read -r modname; do
        for cand in "$base/$modname.rs" "$base/$modname/mod.rs"; do
            if [ -f "$cand" ]; then
                printf '%s\n' "$cand" >> "$WORK/ignore"
            fi
        done
    done
done

# (2) the crate-root `mod tests { ... }` block: its children live in the tests
# tree, whichever file carries the block.
for root in "$CRATE_DIR"/src/*.rs; do
    [ -f "$root" ] || continue
    grep -q 'mod tests' "$root" || continue
    awk -f "$WORK/tests_block.awk" "$root" > "$WORK/tests_block"
    [ -s "$WORK/tests_block" ] || continue
    awk -f "$WORK/gated.awk" "$WORK/tests_block" | while IFS= read -r modname; do
        for cand in "$TESTS_DIR/$modname.rs" "$TESTS_DIR/$modname/mod.rs"; do
            if [ -f "$cand" ]; then
                printf '%s\n' "$cand" >> "$WORK/ignore"
            fi
        done
    done
done

# Rebuild on_disk without the ignored feature-gated files. The list is
# deduplicated (a file reachable through two declaration sites is ignored once)
# and counted for the OK line - "all files read by rustc" silently skipping
# ignored files was itself part of the defect (F3).
ignored=0
if [ -s "$WORK/ignore" ]; then
    LC_ALL=C sort -u "$WORK/ignore" -o "$WORK/ignore"
    ignored=$(wc -l < "$WORK/ignore" | tr -d ' ')
    grep -Fxv -f "$WORK/ignore" "$WORK/on_disk" > "$WORK/on_disk_filtered" || true
    mv "$WORK/on_disk_filtered" "$WORK/on_disk"
fi

# ------------------------------------------------- 2. which artifacts describe the tree
if [ ! -d "$DEP_INFO_DIR" ]; then
    echo "cannot conclude: $DEP_INFO_DIR does not exist - run \`cargo test -p $CRATE\` first" >&2
    exit 2
fi

# A dep-info older than the sources it was built from does not describe the
# current tree; dropping it keeps a stale artifact from answering for the new
# code (and, in the other direction, from masking an orphan). The list is kept
# quietly and only printed when the gate actually goes red - a green run has
# nothing to say about artifacts it did not need.
fresh_count=0
stale_count=0
: > "$WORK/deps"
: > "$WORK/stale"
for dep in "$DEP_INFO_DIR/$PREFIX"-*.d; do
    [ -e "$dep" ] || continue
    stale=$(find "$CRATE_DIR" \( -name '*.rs' -o -name 'Cargo.toml' \) \
                  -not -path '*/target/*' -newer "$dep" -print | head -n 1)
    if [ -n "$stale" ]; then
        printf '  skip (stale): %s is older than %s\n' \
            "${dep#$ROOT/}" "${stale#$ROOT/}" >> "$WORK/stale"
        stale_count=$((stale_count + 1))
        continue
    fi
    # dep-info is one line of `<unit>.d: dep1 dep2 ...` followed by a
    # `dep1:` rule per file. Splitting on spaces keeps both halves usable; the
    # `# env-dep:` and `# env` trailer lines are dropped by the path filter.
    tr ' ' '\n' < "$dep" >> "$WORK/deps"
    fresh_count=$((fresh_count + 1))
done

if [ "$fresh_count" -eq 0 ]; then
    cat "$WORK/stale" >&2
    echo "cannot conclude: no current rustc dep-info under ${DEP_INFO_DIR#$ROOT/} for package $CRATE" >&2
    echo "  run \`cargo test -p $CRATE\` and re-run this gate" >&2
    exit 2
fi

# rustc writes the paths cargo handed it: workspace-relative here, absolute in
# some other setups. Normalising to workspace-relative keeps both comparable.
sed -e 's/:$//' -e "s|^$ROOT/||" "$WORK/deps" | grep '^crates/' | LC_ALL=C sort -u > "$WORK/deps_norm"

# Guard against the vacuous case from the other side: dep-info that contains
# no `src/tests/` file at all comes from a unit compiled WITHOUT `cfg(test)`
# (`cargo check -p $CRATE --lib`), which would make every file look orphaned.
in_deps=$(grep -c "^crates/$CRATE/src/tests/" "$WORK/deps_norm" || true)
if [ "$in_deps" -eq 0 ]; then
    echo "cannot conclude: no ${TESTS_DIR#$ROOT/} file appears in the current dep-info" >&2
    echo "  the artifacts come from a unit built without cfg(test) - run \`cargo test -p $CRATE\`" >&2
    exit 2
fi

if [ -n "$SENTINEL" ]; then
    # Only crates with a feature-gated test module (lumina-gui today) carry a
    # per-crate sentinel.  Others are checked on whatever default-feature build
    # is present; a lean build of those is the caller's responsibility.
    if ! grep -Fxq "$SENTINEL" "$WORK/deps_norm"; then
        echo "cannot conclude: $SENTINEL is not in any current dep-info" >&2
        echo "  that module is only reached under the feature set it was gated for," >&2
        echo "  so the artifacts above were built with a different feature set." >&2
        echo "  Re-run \`cargo test -p $CRATE\` (default features) instead of" >&2
        echo "  concluding falsely." >&2
        exit 2
    fi
fi

# ------------------------------------------------------------- 3. the comparison itself
orphans=0
compared=$((seen - ignored))
while IFS= read -r f; do
    rel="${f#$ROOT/}"
    stem=$(basename "$rel" .rs)
    if grep -Fxq -- "$rel" "$WORK/deps_norm"; then
        continue
    fi
    echo "ORPHAN  $rel - exists on disk but appears in no dep-info of package $CRATE:"
    echo "        no \`mod $stem;\` (nor a \`#[path]\` submodule) reaches it, so rustc never opens it."
    orphans=$((orphans + 1))
done < "$WORK/on_disk"

if [ "$orphans" -ne 0 ]; then
    cat "$WORK/stale" >&2
    echo "test_orphans FAILED: $orphans orphan test file(s) under ${TESTS_DIR#$ROOT/}" >&2
    exit 1
fi

echo "test_orphans OK ($seen files under ${TESTS_DIR#$ROOT/}: $compared compared, $ignored feature-gated ignored - all $compared read by rustc through $fresh_count current dep-info unit(s), $stale_count stale unit(s) ignored)"
