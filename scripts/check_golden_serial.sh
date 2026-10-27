#!/bin/sh
# GOLDEN-SERIAL-RUN-98: require serial execution for ignored GUI test runs.
#
# The active scan covers text under scripts/ and feature/, Rust sources under
# crates/lumina-gui/tests/, active unchecked entries in Agents.todo.md, DoD.md,
# and the scheduled Golden workflow. Shell backslash continuations and Markdown
# inline-code spans can wrap physically; adjacent command fragments are joined
# only when their code spans are whitespace-separated within a paragraph.
# --files runs the exact same checker against explicit fixture files.
#
# This is a text gate only: it never launches cargo and has no Python, Node, or
# Git dependency. Only a matching command line may be exempted, and then only
# by a same-logical-line GOLDEN-SERIAL-HISTORICAL: marker with a reason.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FILE_MODE=0

if [ "${1:-}" = "--files" ]; then
    FILE_MODE=1
    shift
    if [ "$#" -eq 0 ]; then
        echo "Usage: sh scripts/check_golden_serial.sh [--files FILE ...]" >&2
        exit 2
    fi
    for file do
        if [ ! -f "$file" ]; then
            echo "FEHLER: Eingabedatei fehlt: $file" >&2
            exit 2
        fi
    done
else
    if [ "$#" -ne 0 ]; then
        echo "Usage: sh scripts/check_golden_serial.sh [--files FILE ...]" >&2
        exit 2
    fi

    # Scan repository files under scripts/ and feature/, plus Rust test sources
    # under lumina-gui/tests/. The checker and its regression-test fixture are
    # excluded by the shared AWK path. The plan is limited to unchecked task
    # entries by the AWK parser. Explicit policy/workflow files stay included.
    set -f
    OLD_IFS=$IFS
    NL=$(printf '\nx')
    NL=${NL%x}
    IFS=$NL
    set -- $(find "$ROOT/scripts" "$ROOT/feature" -type f ! -name '*.png' -print)
    set -- "$@" $(find "$ROOT/crates/lumina-gui/tests" -type f -name '*.rs' -print)
    IFS=$OLD_IFS
    set +f
    set -- "$@" "$ROOT/Agents.todo.md" "$ROOT/DoD.md" "$ROOT/.github/workflows/golden-gate.yml"
fi

LC_ALL=C awk -v root="$ROOT" -v file_mode="$FILE_MODE" -f "$ROOT/scripts/check_golden_serial.awk" "$@"
