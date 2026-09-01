#!/usr/bin/env bash
# Regenerate the hash-pinned ONNX behavior fixture
# (`crates/lumina-onnx/tests/fixtures/lumina-crafted-reducemax.onnx`) from the
# documented source-of-truth protobuf encoder.
#
# The standalone encoder source lives next to this script
# (`regenerate_onnx_fixture.rs`), byte-identical to the encoder helpers in
# `crates/lumina-onnx/tests/ort_backend.rs`. After regeneration the SHA-256
# pin MUST match the documented value in
# `crates/lumina-onnx/tests/fixtures/README.md` and
# `crates/lumina-onnx/tests/ort_backend.rs` — a drift is a hard test failure,
# never a silent adjustment.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURE_OUT="$SCRIPT_DIR/../crates/lumina-onnx/tests/fixtures/lumina-crafted-reducemax.onnx"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

rustc -O "$SCRIPT_DIR/regenerate_onnx_fixture.rs" -o "$WORK/gen_fixture"
"$WORK/gen_fixture" "$FIXTURE_OUT"

PIN="$(shasum -a 256 "$FIXTURE_OUT" | awk '{print $1}')"
echo "regenerated $FIXTURE_OUT"
echo "sha256=$PIN"
expected="2a2ede6659e8c59b3fd972242b27677ef23cb98d3c422616a1c65f50dcaca18d"
if [ "$PIN" != "$expected" ]; then
  echo "ERROR: fixture drift — expected pin $expected, got $PIN" >&2
  echo "Update the pins in tests/fixtures/README.md AND tests/ort_backend.rs on purpose." >&2
  exit 1
fi
echo "pin OK ($expected)"