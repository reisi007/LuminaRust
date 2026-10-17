#!/bin/sh
# FEATURE-MATRIX-GATE-79 (Befund 2026-10-17, aus dem roten `main` nach der
# Q5-Kapselung): die CI kompiliert nur einen TEIL der deklarierten
# Feature-Kombinationen des Workspace.
#
# Why this gate exists (measured defect): the test module
# `crates/lumina-mcp/src/util.rs` is `#[cfg(all(test, feature = "gpu"))]` and
# NO CI job built `lumina-mcp --features gpu`. Four field accesses there survived
# the Q5 encapsulation unnoticed; they only became visible through a targeted
# `cargo check -p lumina-mcp --all-targets --features gpu` (exit 101). The fix
# run then drove 17 combinations once (all green except the named host gate
# below) — among them combinations no CI job builds and which therefore were
# only checked that one time.
#
# What it checks, per workspace crate (the feature list is read from the crate's
# Cargo.toml `[features]` table, not a hard-coded list, so a newly declared
# feature is covered automatically):
#   Axis 1 — UNION:  `cargo check -p <crate> --all-targets --all-features`
#                    compiles ALL declared features together (one call/crate).
#   Axis 2 — ALONE: `cargo check -p <crate> --all-targets --features <F>`
#                    for every declared non-`default` feature F, so a feature
#                    that only compiles next to another one is caught (the
#                    defect class of `scripts/check_cli_features.sh`).
# Crates without a `[features]` table declare no feature set; they are skipped
# by name (the default build is covered by `cargo check --workspace
# --all-targets`), except `lumina-gui-harness`, which is named below.
#
# NAMED LIMITS (honest, not a covered claim):
#   * Axis 1 does NOT prove a feature compiles WITHOUT the other features. That
#     is exactly what Axis 2 adds. A feature that only breaks when combined
#     with a THIRD feature (a pair, not an individual) is still not caught —
#     this is the same named limit `scripts/check_cli_features.sh` carries.
#   * The gate is COMPILE-ONLY (`cargo check`): no adapter, no runtime test, no
#     golden. Behavioural coverage stays with the workspace test shards.
#   * `--no-default-features` is deliberately NOT checked. Measured on HEAD
#     60dc794: `cargo check -p lumina-cli --all-targets --no-default-features`
#     exits 101 — the test target `tests/stage_parity_common/mod.rs` uses
#     `lumina_mcp` unconditionally, but `lumina_mcp` is only a dependency under
#     the default `mcp` feature. The empty feature set is not a DECLARED
#     feature set, so it is out of scope for this gate — but it is a real,
#     separately reportable gap, not a covered claim.
#   * `lumina-gui-harness` is macOS-only at the code level (`#[cfg(target_os =
#     "macos")]`). It IS buildable on Linux (it compiles to the documented stub
#     main), so the gate compiles it; it does NOT claim to cover the macOS
#     machinery.
#   * The onnx-rt host gate: `cargo test -p lumina-onnx --features onnx-rt` links
#     the ONNX Runtime prebuilt binaries, which need glibc >= 2.38; this host
#     has glibc 2.36 (Debian 12), so that TEST run fails at link. This gate uses
#     `cargo check` (no link), so the host gate cannot turn this step red.
#     Measured: `cargo check -p lumina-onnx --all-targets --all-features` exits 0.
#   * The workspace `members = [...]` list is parsed, not glob-expanded: a glob
#     member would not be picked up (the current list is explicit).
#
# Usage: sh scripts/check_feature_matrix.sh        (exit 0 = all sets green)
#        CARGO="${CARGO:-cargo}" sh scripts/check_feature_matrix.sh
#
# Mutation proof (the gate is testable; this is the recipe):
#   # in crates/lumina-mcp/src/util.rs, inside
#   # `#[cfg(all(test, feature = "gpu"))] mod routing_tests`, change
#   # `routed.pixels()` to `routed.pixels` (no parens) in
#   # `cpu_branch_matches_render_frame_byte_identically` (line 646)
#   sh scripts/check_feature_matrix.sh   # must exit 1 and print the file:line
#   # restore byte-identically (sha256sum before/after); never git checkout
set -eu

started_at=$(date +%s)

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKSPACE_MANIFEST="$ROOT/Cargo.toml"
CARGO="${CARGO:-cargo}"

if [ ! -f "$WORKSPACE_MANIFEST" ]; then
    echo "FAIL  $WORKSPACE_MANIFEST not found" >&2
    exit 1
fi

failed=0
checked_crates=0
checked_sets=0
skipped_no_features=0

tmpout=$(mktemp)
trap 'rm -f "$tmpout"' EXIT

run_check() {
    label="$1"; shift
    if ( cd "$ROOT" && "$CARGO" check "$@" ) >"$tmpout" 2>&1; then
        echo "OK    $label"
        checked_sets=$((checked_sets + 1))
    else
        echo "FAIL  $label" >&2
        sed 's/^/      /' "$tmpout" >&2
        failed=1
    fi
}

# Workspace members are read from the root manifest, not hard-coded.
members_raw=$(sed -n 's/^members[[:space:]]*=[[:space:]]*\[\(.*\)\][[:space:]]*$/\1/p' "$WORKSPACE_MANIFEST")
if [ -z "$members_raw" ]; then
    echo "FAIL  no 'members = [...]' list parsed from $WORKSPACE_MANIFEST" >&2
    exit 1
fi
members=$(printf '%s\n' "$members_raw" | tr ',' '\n' | tr -d '[]"' | grep -v '^[[:space:]]*$' || true)

for member in $members; do
    manifest="$ROOT/$member/Cargo.toml"
    if [ ! -f "$manifest" ]; then
        echo "FAIL  workspace member manifest missing: $member/Cargo.toml" >&2
        failed=1
        continue
    fi
    crate=$(awk '
        /^\[package\]/ { inp = 1; next }
        /^\[/         { inp = 0; next }
        inp && /^name[[:space:]]*=/ {
            line = $0
            sub(/^name[[:space:]]*=[[:space:]]*/, "", line)
            gsub(/"/, "", line)
            gsub(/[[:space:]]/, "", line)
            print line
            exit
        }
    ' "$manifest")

    features=$(awk '
        /^\[features\]/ { in_feat = 1; next }
        /^\[/           { in_feat = 0; next }
        in_feat && /^[A-Za-z0-9_-]+[[:space:]]*=/ {
            key = $1
            sub(/[[:space:]]*=.*$/, "", key)
            print key
        }
    ' "$manifest")

    if [ -z "$features" ]; then
        if [ "$crate" = "lumina-gui-harness" ]; then
            # Explicitly named by the task: macOS-only harness. It IS buildable
            # on Linux (stub main), so it is compiled, not silently skipped.
            run_check "$crate --all-targets --all-features" -p "$crate" --all-targets --all-features
            checked_crates=$((checked_crates + 1))
        else
            echo "SKIP  $crate: no features declared (default build covered by 'cargo check --workspace --all-targets')"
            skipped_no_features=$((skipped_no_features + 1))
        fi
        continue
    fi

    checked_crates=$((checked_crates + 1))

    # Axis 1: the union of all declared features (one call per crate).
    run_check "$crate --all-targets --all-features" -p "$crate" --all-targets --all-features

    # Axis 2: each declared feature on its own (default features stay on — the
    # same axis `scripts/check_cli_features.sh` uses for the CLI).
    for f in $features; do
        case "$f" in
            default) continue ;;
        esac
        run_check "$crate --all-targets --features $f" -p "$crate" --all-targets --features "$f"
    done
done

if [ "$failed" -ne 0 ]; then
    echo "feature_matrix FAILED: at least one declared workspace feature set does not compile" >&2
    exit 1
fi

elapsed=$(($(date +%s) - started_at))
echo "feature_matrix OK ($checked_crates crates, $checked_sets feature sets, $skipped_no_features crates skipped: no declared features, ${elapsed}s)"
