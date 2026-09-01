#!/usr/bin/env bash
# PREVIEW-CACHE-FEATURE (F-074): run the hybrid neighbor-preview-cache benchmarks
# and compare them against the preview-specific baseline/budget stores.
#
# The preview-cache primitives live in `lumina-gui` (native-only Criterion bench
# target `preview_cache`), so they are measured in their own group and stored in
# preview-specific stores under scripts/perf/ (the main perf/ stores stay owned
# by the F-074 core/batch/decode harness; see
# feature/quality/preview-cache.md Akzeptanzkriterium 8).
#
# Usage from the repo root:
#   bash scripts/perf/bench-preview-cache.sh              # run + report
#   bash scripts/perf/bench-preview-cache.sh --mode gate  # hard gate (local only)
set -euo pipefail

MODE="${1:---mode report}"
CRITERION_DIR="target/criterion"
REPORT_DIR="perf/results-preview-cache"
BASELINE="scripts/perf/preview-cache-baseline.json"
BUDGETS="scripts/perf/preview-cache-budgets.json"

# F-074 measurement profile: release, modest, deterministic fixtures.
cargo bench -p lumina-gui --bench preview_cache -- --sample-size 100

node scripts/perf/compare.mjs \
  "$MODE" \
  --baseline "$BASELINE" \
  --budgets "$BUDGETS" \
  --criterion-dir "$CRITERION_DIR" \
  --report-dir "$REPORT_DIR"