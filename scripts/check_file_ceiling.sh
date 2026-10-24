#!/bin/sh
# Compatibility entry point for the Ratchet v0.5.0 1000-line gate.
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RATCHET="${RATCHET_BIN:-ratchet}"

if ! command -v "$RATCHET" >/dev/null 2>&1; then
    echo "ERROR: Ratchet v0.5.0 is required (set RATCHET_BIN or install ratchet on PATH)." >&2
    exit 2
fi
version=$("$RATCHET" --version)
if [ "$version" != "ratchet 0.5.0" ]; then
    echo "ERROR: expected Ratchet v0.5.0, got: $version" >&2
    exit 2
fi

python3 "$ROOT/scripts/check_ratchet_exceptions.py"
RATCHET_BIN="$RATCHET" python3 "$ROOT/scripts/test_ratchet_file_ceiling.py"
cd "$ROOT"
RATCHET_NO_UPDATE_CHECK=1 "$RATCHET" check --rule rust-file-line-ceiling
