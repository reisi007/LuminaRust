#!/bin/sh
# Compatibility entry point for the Ratchet v0.6.2 1000-line gate.
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RATCHET="${RATCHET_BIN:-ratchet}"

if ! command -v "$RATCHET" >/dev/null 2>&1; then
    echo "ERROR: Ratchet v0.6.2 is required (set RATCHET_BIN or install ratchet on PATH)." >&2
    exit 2
fi
version=$("$RATCHET" --version)
if [ "$version" != "ratchet 0.6.2" ]; then
    echo "ERROR: expected Ratchet v0.6.2, got: $version" >&2
    exit 2
fi

FILTERED_CONFIG=$(mktemp "$ROOT/.ratchet-population.XXXXXX")
cleanup() { rm -f "$FILTERED_CONFIG"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
python3 "$ROOT/scripts/rust_source_population.py" \
    --ratchet-config "$ROOT/ratchet.config.json" --output "$FILTERED_CONFIG"

python3 "$ROOT/scripts/check_ratchet_exceptions.py"
RATCHET_BIN="$RATCHET" python3 "$ROOT/scripts/test_ratchet_file_ceiling.py"
cd "$ROOT"
RATCHET_NO_UPDATE_CHECK=1 "$RATCHET" check \
    --config "$FILTERED_CONFIG" --no-config \
    --rule rust-file-line-ceiling --no-update-check
