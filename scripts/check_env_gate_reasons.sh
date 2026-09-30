#!/bin/sh
# FIXTURE-SKIP-VISIBLE-2: an env-gated `#[ignore]` proof must say what is
# missing AND the exact command, and must report its skip to the real stderr.
#
# SCOPE, honestly stated: this checks the sites migrated under this task, named
# explicitly below — not every `#[ignore]` in the workspace. Applying the rule
# repo-wide would rewrite every existing `#[ignore = "…"]` reason, which is a
# different task; the count is printed at the end so the remaining gap is a
# measured number rather than a feeling.
#
# NAMED LIMIT, not a covered claim: the `#[ignore]` attribute is a string
# literal, while the runtime line is built by `lumina_testskip::env_gate(..)`.
# Nothing here compares the two, so they can still drift. Pinning it needs a
# Rust test that reads the attribute — that is a known open gap in
# FIXTURE-SKIP-VISIBLE-2, not something this script pretends to cover.
set -eu

root="${1:-.}"
failed=0

# The sites this task migrated: <file>|<test function>
sites="
crates/lumina-gui/src/matrix.rs|real_matrix_headless
crates/lumina-cli/tests/matrix_e2e.rs|real_matrix_against_committed_goldens
crates/lumina-onnx/tests/face_real_weights.rs|real_weights_match_the_adapter_contract
"

for entry in $sites; do
    file="${entry%%|*}"
    test_name="${entry##*|}"
    path="$root/$file"

    if [ ! -f "$path" ]; then
        echo "FAIL  $file: migrated site does not exist"
        failed=1
        continue
    fi

    # The attribute must be an `#[ignore = "…"]` with a reason, and that reason
    # must name what is missing and how to run it. A gate naming two different
    # commands is a gate nobody can reproduce.
    #
    # The attribute sits on the line DIRECTLY above the function, so the line is
    # addressed rather than pattern-matched: a `-A1` pipeline loses the attribute
    # exactly when it is the interesting line.
    fn_line=$(grep -n "fn ${test_name}(" "$path" | head -1 | cut -d: -f1)
    if [ -z "$fn_line" ]; then
        echo "FAIL  $file: fn $test_name not found - did the site move?"
        failed=1
        continue
    fi
    reason=$(sed -n "$((fn_line - 1))p" "$path" | sed 's/^[[:space:]]*#\[ignore = "//; s/"\][[:space:]]*$//')

    if [ -z "$reason" ]; then
        echo "FAIL  $file: no reasoned #[ignore = \"…\"] directly above fn $test_name"
        failed=1
        continue
    fi
    case "$reason" in
        *"needs "*) ;;
        *) echo "FAIL  $file: ignore reason does not name what is missing: $reason"; failed=1 ;;
    esac
    case "$reason" in
        *"run: "*) ;;
        *) echo "FAIL  $file: ignore reason does not name the exact command: $reason"; failed=1 ;;
    esac

    # The skip must go to the real stderr. `eprintln!` is the invisible channel
    # this task exists to end: libtest discards a passing test's captured output.
    if grep -q 'lumina_testskip::report_env_gate\|lumina_testskip::report_to_real_stderr' "$path"; then
        :
    else
        echo "FAIL  $file: reports its skip through neither the real stderr nor lumina_testskip"
        failed=1
    fi
done

if [ "$failed" -ne 0 ]; then
    echo "env_gate_reasons FAILED"
    exit 1
fi

# The measured gap, printed as a number: reasoned `#[ignore]` attributes in the
# workspace that have not been migrated to the visible channel yet.
migrated=3
total=$(grep -rho '#\[ignore = "' "$root/crates" --include='*.rs' 2>/dev/null | wc -l | tr -d ' ')
naked=$(grep -rho '#\[ignore\]$' "$root/crates" --include='*.rs' 2>/dev/null | wc -l | tr -d ' ')
echo "env_gate_reasons OK ($migrated of $total reasoned #[ignore] sites use the visible channel)"
echo "  NAMED GAP: $((total - migrated)) reasoned sites still report invisibly; $naked naked #[ignore] remain"
