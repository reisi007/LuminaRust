#!/bin/sh
# CLI-FEATURE-BUILD-GATE (CLI-GPU-BUILD-1, Release 1.0): every feature DECLARED
# in crates/lumina-cli/Cargo.toml must compile ON ITS OWN — not only in the
# combination a developer happens to use.
#
# Why this gate exists (measured defect, 2026-10-02): a stray
# `#[cfg(feature = "lensfun")]` sat directly above
# `#[cfg(feature = "gpu")] use lumina_gpu::{…}` in `crates/lumina-cli/src/main.rs`
# and stacked onto it. `--features gpu,lensfun` and `--features lensfun`
# compiled; `--features gpu` alone died with 6 errors (`cannot find type
# GpuContext/Frame`, `unsupported_gpu_stages_with_context`), because two
# independent `#[cfg]` attributes on one item are AND-ed. CI only checked
# `cargo check -p lumina-gpu --features gpu` — never the CLI's own `gpu`
# feature — so the break was invisible.
#
# What it checks, per declared feature F (read from the manifest, not a
# hard-coded list, so a new feature is covered automatically):
#   `cargo check -p lumina-cli --features F`
# plus the two ends of the feature space:
#   `cargo check -p lumina-cli`                      (default features)
#   `cargo check -p lumina-cli --no-default-features`
# It is deliberately an INDIVIDUAL feature check, not a power set: the defect
# class is "a feature works only in the presence of another one".
#
# Named limits (honest, not a covered claim):
#   * The `gpu,lensfun` COMBINATION is checked below, but only as a TESTBUILD
#     check (`--all-targets`), not as a full feature-space sweep: a feature
#     that only breaks when combined with a THIRD feature would still not be
#     caught. The individual axis above stays the primary coverage; the
#     combination check is the measured extension (CI-CLI-GPU-TESTBUILD-68).
#   * This checks `cargo check`, not `cargo test`: compile breakage is the
#     subject here. Behavioural tests stay in the workspace test shards. The
#     `--all-targets` flag extends the compile check to test targets (unit
#     tests in `src/tests/`, integration tests in `tests/`), which was the
#     measured gap: three latent testbuild defects (missing `Geometry` import
#     in `lensfun_gpu.rs`, `unused_mut` in `matrix_mcp_parity_errors.rs`,
#     missing `RawMetadata` import under `gpu,lensfun`) were invisible
#     because no CI built the CLI test targets with `gpu`.
#
# Usage: sh scripts/check_cli_features.sh        (exit 0 = all combinations green)
#        CARGO="${CARGO:-cargo}" sh scripts/check_cli_features.sh
#
# Mutation proof (the gate is testable and this is the recipe):
#
# (a) Library axis — re-introduce the orphan attribute:
#     # add `#[cfg(feature = "lensfun")]` on the line above the
#     # `#[cfg(feature = "gpu")] use lumina_gpu::{…}` in main.rs
#     then `sh scripts/check_cli_features.sh` must exit 1 with
#       FAIL  lumina-cli --features gpu
#     and, crucially, STILL exit 0 for `--features gpu,lensfun`. That asymmetry
#     is the signature of the original defect and is why a combined-only gate
#     is not enough. Removing the attribute restores exit 0.
#
# (b) Testbuild axis (CI-CLI-GPU-TESTBUILD-68) — remove a test-only import:
#     # delete `use lumina_sidecar::Geometry;` from
#     #   crates/lumina-cli/src/tests/lensfun_gpu.rs (line 19)
#     then `sh scripts/check_cli_features.sh` must exit 1 with
#       FAIL  lumina-cli --features gpu --all-targets
#     The library axis above stays green (the import is test-only), which is
#     exactly why the testbuild checks are an extension and not a replacement.
#     Restoring the import restores exit 0.
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8 — je Defektklasse eine
# Schranke, Klasse und Anlass im Header benannt):
#   Defektklasse: Feature-Isolationsbruch — ein deklariertes Feature, das nur
#                  in Kombination mit einem anderen kompiliert.
#   Anlass:        2026-10-02, der gemessene Defekt oben (ein verwaistes
#                  `#[cfg(feature = "lensfun")]` auf dem `gpu`-Import liess
#                  `--features gpu` mit 6 Fehlern sterben, waehrend
#                  `--features gpu,lensfun` gruen blieb).
#   Ratchet-Richtung: Boolean-Gate (keine Baseline-Datei) — jedes deklarierte
#                  Feature muss allein kompilieren; der Zaehlwert ist heute 0
#                  und muss 0 bleiben. Die Feature-Liste kommt aus dem
#                  Manifest, ein neu deklariertes Feature ist damit automatisch
#                  gedeckt (kein Handbestand, der altern kann).
#   Nicht gedeckt:  die NAMED LIMITS oben (keine Dreier-Kombination, kein
#                  Laufzeitverhalten, keine Adapter-Tests) und alle Crates
#                  ausser `lumina-cli` (die trägt `scripts/check_feature_matrix.sh`).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/crates/lumina-cli/Cargo.toml"
CARGO="${CARGO:-cargo}"

if [ ! -f "$MANIFEST" ]; then
    echo "FAIL  $MANIFEST not found - did the crate move?"
    exit 1
fi

failed=0

# One combination: <label> <extra cargo args...>
check_combo() {
    label="$1"
    shift
    if ( cd "$ROOT" && "$CARGO" check -p lumina-cli "$@" ); then
        echo "OK    lumina-cli $label"
    else
        echo "FAIL  lumina-cli $label" >&2
        failed=1
    fi
}

# --- 1. every feature DECLARED in the [features] table, individually ---------
# Parsed from the manifest so a newly declared feature cannot be forgotten.
# Only the left-hand keys of the table are read (`name = [...]`), which skips
# the table header and blank lines. Feature names are `[A-Za-z0-9_-]`.
features=$(awk '
    /^\[features\]/ { in_feat = 1; next }
    /^\[/           { in_feat = 0; next }
    in_feat && /^[A-Za-z0-9_-]+[[:space:]]*=/ {
        key = $1
        sub(/[[:space:]]*=.*$/, "", key)
        print key
    }
' "$MANIFEST")

if [ -z "$features" ]; then
    echo "FAIL  no features parsed from $MANIFEST - the parser no longer matches the table" >&2
    exit 1
fi

for f in $features; do
    case "$f" in
        # `default` is the aggregate, exercised by the default build below; it
        # is not a feature one selects with `--features default`.
        default) continue ;;
    esac
    check_combo "--features $f" --features "$f"
done

# --- 2. the two ends of the feature space -----------------------------------
check_combo "(default features)"
check_combo "--no-default-features" --no-default-features

# --- 3. testbuild checks (CI-CLI-GPU-TESTBUILD-68) ---------------------------
# The individual axis above compiles the LIBRARY only. The CLI's own test
# targets (`src/tests/**`, `tests/**`) were invisible to every CI job:
# `cargo check -p lumina-gpu --features gpu` covers lumina-gpu, the test shard
# runs the CLI with `--features lensfun` only, and `check_cli_features.sh`
# itself checked `cargo check` without `--all-targets`. Three latent testbuild
# defects (missing `Geometry` import in `src/tests/lensfun_gpu.rs`,
# `unused_mut` in `tests/matrix_mcp_parity_errors.rs`, missing `RawMetadata`
# import under `gpu,lensfun`) were found by the B2 work and fixed — they
# stayed invisible because no gate compiled the CLI test targets with `gpu`.
#
# These two combinations are an EXTENSION, not a replacement: the individual
# axis above stays the primary feature coverage. `gpu` alone and `gpu,lensfun`
# are the two combinations the B2 work measured as broken-in-testbuild.
check_combo "--features gpu --all-targets" --features gpu --all-targets
check_combo "--features gpu,lensfun --all-targets" --features gpu,lensfun --all-targets

if [ "$failed" -ne 0 ]; then
    echo "cli_feature_build FAILED: at least one declared CLI feature does not compile on its own" >&2
    exit 1
fi

# shellcheck disable=SC2086  # word-split is the feature list, intentional
count=$(printf '%s\n' $features | grep -c . || true)
echo "cli_feature_build OK ($((count - 1)) declared non-default features + default + no-default-features)"
