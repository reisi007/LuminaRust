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
#   * `--features gpu,lensfun` is NOT re-run here. It is covered twice over by
#     the individual `gpu` and `lensfun` checks ONLY for additive features; a
#     feature that only breaks WHEN combined would not be caught. The CI test
#     shard already builds `lumina-cli --features lensfun`, and this script
#     keeps the individual axis. Extending to pairs is a separate task if a
#     pairing defect is ever measured.
#   * This checks `cargo check`, not `cargo test`: compile breakage is the
#     subject here. Behavioural tests stay in the workspace test shards.
#
# Usage: sh scripts/check_cli_features.sh        (exit 0 = all combinations green)
#        CARGO="${CARGO:-cargo}" sh scripts/check_cli_features.sh
#
# Mutation proof (the gate is testable and this is the recipe): re-introduce
# the orphan attribute —
#     # add `#[cfg(feature = "lensfun")]` on the line above the
#     # `#[cfg(feature = "gpu")] use lumina_gpu::{…}` in main.rs
# — then `sh scripts/check_cli_features.sh` must exit 1 with
#     FAIL  lumina-cli --features gpu
# and, crucially, STILL exit 0 for `--features gpu,lensfun`. That asymmetry is
# the signature of the original defect and is why a combined-only gate is not
# enough. Removing the attribute restores exit 0.
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

if [ "$failed" -ne 0 ]; then
    echo "cli_feature_build FAILED: at least one declared CLI feature does not compile on its own" >&2
    exit 1
fi

# shellcheck disable=SC2086  # word-split is the feature list, intentional
count=$(printf '%s\n' $features | grep -c . || true)
echo "cli_feature_build OK ($((count - 1)) declared non-default features + default + no-default-features)"
