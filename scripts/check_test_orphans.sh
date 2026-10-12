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
#     `lumina-lensfun`, `lumina-sidecar`) and are NOT covered. Pass the crate
#     name as `$1` to check one of them.
#   * The gate can only read what was built. It needs artifacts from a
#     **default-feature build that includes the test target**; without them it
#     exits 2 instead of guessing.
#   * `crates/lumina-gui/src/tests/lensfun_diagnostics.rs` is reached through
#     `#[cfg(feature = "lensfun")] mod lensfun_diagnostics;`, so a lean
#     `--no-default-features` build makes it absent from dep-info. The sentinel
#     below turns that case into exit 2 rather than a false orphan finding.
#   * A stale artifact from a DIFFERENT feature set can still list an orphan
#     and mask it. The freshness filter drops every dep-info older than the
#     sources it was built from, which covers the common case; it cannot
#     distinguish two same-age artifacts with different features.
#
# Usage:
#   sh scripts/check_test_orphans.sh                # crates/lumina-gui
#   sh scripts/check_test_orphans.sh lumina-core    # another crate
#   LUMINA_DEP_INFO_DIR=target/release/deps sh scripts/check_test_orphans.sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="${1:-lumina-gui}"
CRATE_DIR="$ROOT/crates/$CRATE"
TESTS_DIR="$CRATE_DIR/src/tests"
DEP_INFO_DIR="${LUMINA_DEP_INFO_DIR:-$ROOT/target/debug/deps}"
# rustc turns `lumina-gui` into the unit prefix `lumina_gui-<hash>.d`.
PREFIX=$(printf '%s' "$CRATE" | tr '-' '_')
# A module that only a default-feature build reaches. Its absence means the
# artifacts were built lean, and this gate refuses to conclude on that.
SENTINEL="crates/$CRATE/src/tests/lensfun_diagnostics.rs"

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

if ! grep -Fxq "$SENTINEL" "$WORK/deps_norm"; then
    echo "cannot conclude: $SENTINEL is not in any current dep-info" >&2
    echo "  that module is only reached with the default \`lensfun\` feature, so the" >&2
    echo "  artifacts above were built with a different feature set. Re-run with" >&2
    echo "  \`cargo test -p $CRATE\` (default features) instead of concluding falsely." >&2
    exit 2
fi

# ------------------------------------------------------------- 3. the comparison itself
orphans=0
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

echo "test_orphans OK ($seen files under ${TESTS_DIR#$ROOT/}, all $seen read by rustc through $fresh_count current dep-info unit(s), $stale_count stale unit(s) ignored)"
