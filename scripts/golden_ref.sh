#!/bin/sh
# Golden-Reference-Fingerprint (GOLDEN-REF-30, User-Entscheid 2026-09-25).
#
# The kittest goldens under `crates/lumina-gui/tests/snapshots/` are only
# reproducible on ONE named platform: macOS with the Metal backend. This
# script makes that platform machine-checkable:
#
#   print            show the detected fingerprint next to the pinned one
#   check            exit non-zero when the environment does not match the pin
#   record --confirm <reason>
#                    re-pin the fingerprint (never non-interactive by accident)
#   gate -- <cmd>    run `check` first, then exec <cmd> (regeneration hook)
#
# Normative spec: `feature/quality/golden-references.md`. The pinned values
# live in `scripts/golden_ref.lock` (format documented in that spec).
#
# POSIX sh only (macOS ships bash 3.2 without associative arrays), no network
# access, no dependencies beyond base OS tools: `uname`, `sw_vers`, `sysctl`,
# `system_profiler`, `pkg-config`, `rustc`, `shasum` (fallback `sha256sum`),
# `dd`, `od`, `awk`, `grep`, `sed`, `tr`, `head`, `wc`, `find`, `sort`, `diff`,
# `mv`, `git` (local only; without `git` the script visibly switches to the
# `walk:` digest mode, see feature/quality/golden-references.md §3.1). Every one
# of those is detected with `command -v` and degrades to a loud `unavailable`
# value, never to a silent default.
#
# Its own regression suite is `scripts/golden_ref_test.sh` (shell-only, runs on
# any platform: it records a synthetic lock, perturbs it and asserts exit
# codes; it never touches the committed lock or the real index).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEFAULT_LOCK="$ROOT/scripts/golden_ref.lock"
# `GOLDEN_REF_LOCK` is a documented test-only override so a scratch run can
# compare against a second lock file. Every use prints a loud warning, because
# a guard that can be pointed at an arbitrary file is only trustworthy when the
# operator can see which file was used.
LOCK="${GOLDEN_REF_LOCK:-$DEFAULT_LOCK}"
FIXTURES_DIR="$ROOT/crates/lumina-gui/tests/fixtures"
GOLDENS_DIR="$ROOT/crates/lumina-gui/tests/snapshots"
CARGO_LOCK="$ROOT/Cargo.lock"
TOOLCHAIN_TOML="$ROOT/rust-toolchain.toml"
GUI_SRC="$ROOT/crates/lumina-gui/src"

# Logical viewport pinned by `build_harness()` in
# `crates/lumina-gui/tests/kittest_snapshots_support/mod.rs`
# (`.with_size([1024.0, 720.0])`). The goldens are 1024x720 physical pixels at
# the default `pixels_per_point` of 1.0, so scale_factor = px / logical.
VIEWPORT_W=1024
VIEWPORT_H=720

# A CR that a CRLF checkout or a CRLF-editing editor can leave at the end of a
# lock line. Used to normalise on read (see `lock_value`).
LV_CR=$(printf '\r')

die() {
  echo "ERROR: $*" >&2
  exit 2
}

# --- hash helpers -----------------------------------------------------------

sha256_stdin() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    die "neither shasum nor sha256sum found - cannot fingerprint"
  fi
}

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    die "neither shasum nor sha256sum found - cannot fingerprint"
  fi
}

# --- small value detectors --------------------------------------------------

# Version of a package pinned in Cargo.lock (single source of truth for the
# wgpu/egui build that actually produces the golden pixels). An unreadable
# lock file yields `unavailable`, which never matches the pin - a hard,
# visible mismatch, never a silent default.
cargo_lock_version() {
  if [ ! -f "$CARGO_LOCK" ]; then
    echo "unavailable"
    return
  fi
  clv_found=$(awk -v want="$1" '
    /^name = / { n = $3; gsub(/"/, "", n); inpkg = (n == want) }
    inpkg && /^version = / { v = $3; gsub(/"/, "", v); print v; exit }
  ' "$CARGO_LOCK" 2>/dev/null || true)
  echo "${clv_found:-unavailable}"
}

# `channel = "stable"` from rust-toolchain.toml (the toolchain the goldens
# were built with; `rustc --version` alone is only the installed one).
toolchain_channel() {
  if [ ! -f "$TOOLCHAIN_TOML" ]; then
    echo "unavailable"
    return
  fi
  tc_found=$(awk -F'"' '/^channel[[:space:]]*=/ { print $2; exit }' "$TOOLCHAIN_TOML" 2>/dev/null || true)
  echo "${tc_found:-unavailable}"
}

macos_version() {
  if command -v sw_vers >/dev/null 2>&1; then
    mv_found=$(sw_vers -productVersion 2>/dev/null || true)
    echo "${mv_found:-unavailable}"
  else
    echo "unavailable"
  fi
}

cpu_model() {
  if command -v sysctl >/dev/null 2>&1; then
    cm_found=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || true)
    echo "${cm_found:-unavailable}"
  else
    echo "unavailable"
  fi
}

# Metal device name and supported Metal version, read from the running OS.
# wgpu 30 builds no Vulkan/DX12/GL backend for macOS targets, so the Metal
# device wgpu binds is exactly the chipset this reports - captured, not
# assumed. `system_profiler` is localized: the keys below are its English
# output. On a localized system the value degrades to `unavailable`, which
# never matches the pin (a loud mismatch), never a silently wrong pass.
gpu_os_facts() {
  command -v system_profiler >/dev/null 2>&1 || {
    echo "unavailable"
    echo "unavailable"
    return
  }
  sp_out=$(system_profiler SPDisplaysDataType 2>/dev/null || true)
  sp_adapter=$(echo "$sp_out" | awk -F': ' '/Chipset Model:/{print $NF; exit}')
  sp_metal=$(echo "$sp_out" | awk -F': ' '/Metal Support:/{print $NF; exit}')
  echo "${sp_adapter:-unavailable}"
  echo "${sp_metal:-unavailable}"
}

# Version of the native LibRaw the RAW path links against. The vendored
# `libraw-sys` build script resolves the `libraw_r` pkg-config module with
# `.atleast_version("0.22.0")`, so that is the module probed here.
libraw_version() {
  if ! command -v pkg-config >/dev/null 2>&1; then
    echo "unavailable"
    return
  fi
  lr_v=$(pkg-config --modversion libraw_r 2>/dev/null || true)
  if [ -z "$lr_v" ] && [ -d /opt/homebrew/opt/libraw/lib/pkgconfig ]; then
    lr_v=$(PKG_CONFIG_PATH=/opt/homebrew/opt/libraw/lib/pkgconfig \
      pkg-config --modversion libraw_r 2>/dev/null || true)
  fi
  echo "${lr_v:-unavailable}"
}

# Detect a GUI-side font override. `lumina-gui` installs no `FontDefinitions`
# today, so egui's bundled default stack (compiled into the binary) is what
# renders the goldens; a future override is a deliberate, detectable change.
font_resolution() {
  if [ ! -d "$GUI_SRC" ]; then
    echo "gui-source-missing"
    return
  fi
  fr_hits=$(grep -rl -E 'FontDefinitions|set_fonts|font_data\(' "$GUI_SRC" 2>/dev/null || true)
  if [ -z "$fr_hits" ]; then
    echo "egui-bundled-default"
  else
    echo "override-in:$(echo "$fr_hits" | sed "s|$ROOT/||" | tr '\n' ',' | sed 's/,$//')"
  fi
}

# Physical PNG dimensions from the IHDR chunk (bytes 16..23: 4x u32 BE width,
# then 4x u32 BE height). Derived from the committed goldens themselves, so a
# golden recorded at a different scale cannot hide behind a hardcoded 1.0.
png_size() {
  # `od` between `dd` and `awk` is mandatory: raw PNG bytes contain NUL and
  # newline values, which awk's text record reader would mangle.
  dd if="$1" bs=1 skip=16 count=8 2>/dev/null |
    od -An -tu1 |
    awk '{
           for (i = 1; i <= NF; i++) b[n++] = $i
         }
         END {
           if (n >= 8) {
             w = b[0] * 16777216 + b[1] * 65536 + b[2] * 256 + b[3]
             h = b[4] * 16777216 + b[5] * 65536 + b[6] * 256 + b[7]
             printf "%dx%d", w, h
           } else { printf "?" }
         }'
}

# The goldens' common pixel size, or `mixed`. Only the committed `<name>.png`
# baselines count; egui_kittest's `.new.png` / `.diff.png` / `.old.png`
# comparison artifacts are gitignored and never part of the baseline.
goldens_size() {
  gs_list=$(list_goldens)
  if [ -z "$gs_list" ]; then
    echo "none"
    return 0
  fi
  gs_size=$(png_size "$ROOT/$(printf '%s\n' "$gs_list" | head -n 1)")
  gs_mixed=no
  # Unquoted on purpose: default IFS splits the newline-separated list, so the
  # loop runs in this shell (a `| while` pipeline would run in a subshell and
  # could not report `gs_mixed` back).
  for gs_rel in $gs_list; do
    if [ "$(png_size "$ROOT/$gs_rel")" != "$gs_size" ]; then
      gs_mixed=yes
      break
    fi
  done
  if [ "$gs_mixed" = yes ]; then
    echo "mixed"
  else
    echo "$gs_size"
  fi
}

# Committed fixture inventory.
#
# Run artefacts must never enter the pin, and they are not a fixed suffix list:
# `GOLDEN-FIXT-31` stages licensed CR3 copies next to the fixtures at setup
# time and gitignores them, so a plain disk walk would flip between "before a
# test run" and "after a test run". `git ls-files --cached --others
# --exclude-standard` is exactly the right set: tracked files plus untracked
# files that are not run artefacts, honouring both the root `.gitignore` and
# `crates/lumina-gui/tests/fixtures/.gitignore`.
#
# The fallback (no git) is a disk walk with the known artefact list; the mode is
# part of the digest value (`git:` vs `walk:`) so a switch shows up as a
# mismatch instead of a mystery hash.
list_tracked() {
  # list_tracked <repo-relative dir> -> repo-relative paths
  # `|| true` keeps a missing `git` (or a non-git checkout) a graceful
  # fallback instead of a `set -e` abort: the caller switches to `walk:` mode.
  git -C "$ROOT" ls-files --cached --others --exclude-standard -- "$1" 2>/dev/null || true
}

list_fixtures() {
  lf_files=$(list_tracked "crates/lumina-gui/tests/fixtures")
  if [ -n "$lf_files" ]; then
    printf '%s\n' "$lf_files" | LC_ALL=C sort
    return 0
  fi
  find "$FIXTURES_DIR" -type f 2>/dev/null |
    sed "s|^$ROOT/||" |
    awk '!/\/\.lumina\// && !/\.lumina\.json$/ && !/\.lumina\.zdata$/ && !/\.lumina-preset\.json$/ && !/\.cr3$/ && !/\/\.DS_Store$/ && !/\/\.DS_Store$/' |
    LC_ALL=C sort
}

digest_fixtures() {
  if [ -n "$(list_tracked "crates/lumina-gui/tests/fixtures")" ]; then
    df_mode=git
  else
    df_mode=walk
  fi
  df_lines=$(list_fixtures | while IFS= read -r df_rel; do
    [ -n "$df_rel" ] || continue
    if [ -f "$ROOT/$df_rel" ]; then
      printf '%s %s\n' "$df_rel" "$(sha256_file "$ROOT/$df_rel")"
    else
      printf '%s absent\n' "$df_rel"
    fi
  done)
  printf '%s:%s\n' "$df_mode" "$(printf '%s\n' "$df_lines" | sha256_stdin)"
}

# Committed golden baselines. egui_kittest's gitignored comparison artifacts
# (`*.new.png`, `*.diff.png`, `*.old.png`, see
# `crates/lumina-gui/tests/snapshots/.gitignore`) are excluded: they are
# per-run output, never baseline.
list_goldens() {
  lg_files=$(list_tracked "crates/lumina-gui/tests/snapshots")
  if [ -n "$lg_files" ]; then
    printf '%s\n' "$lg_files" |
      awk '!/\/\.gitignore$/ && /\.png$/' |
      LC_ALL=C sort
    return 0
  fi
  find "$GOLDENS_DIR" -maxdepth 1 -type f -name '*.png' 2>/dev/null |
    sed "s|^$ROOT/||" |
    awk '!/\.new\.png$/ && !/\.diff\.png$/ && !/\.old\.png$/' |
    LC_ALL=C sort
}

digest_goldens() {
  if [ -n "$(list_tracked "crates/lumina-gui/tests/snapshots")" ]; then
    dg_mode=git
  else
    dg_mode=walk
  fi
  dg_lines=$(list_goldens | while IFS= read -r dg_rel; do
    [ -n "$dg_rel" ] || continue
    if [ -f "$ROOT/$dg_rel" ]; then
      printf '%s %s\n' "$dg_rel" "$(sha256_file "$ROOT/$dg_rel")"
    else
      printf '%s absent\n' "$dg_rel"
    fi
  done)
  printf '%s:%s\n' "$dg_mode" "$(printf '%s\n' "$dg_lines" | sha256_stdin)"
}

# --- the fingerprint --------------------------------------------------------

# Key order is part of the format. Values are single-line by construction.
FINGERPRINT_KEYS="schema.golden_ref
os.name
os.macos
os.arch
os.cpu
toolchain.channel
toolchain.rustc
wgpu.version
wgpu.backend
wgpu.backend_env
gpu.adapter
gpu.metal
font.resolution
font.egui
ui.golden_px
ui.scale_factor
libraw.version
fixtures.count
fixtures.digest
goldens.count
goldens.digest"

emit_fingerprint() {
  ef_sys=$(uname -s)
  case "$ef_sys" in
    Darwin) ef_os=macOS ;;
    *) ef_os="$ef_sys" ;;
  esac
  ef_arch=$(uname -m)
  ef_cpu=$(cpu_model)
  ef_macos=$(macos_version)
  ef_channel=$(toolchain_channel)
  if command -v rustc >/dev/null 2>&1; then
    ef_rustc=$(rustc --version | awk '{print $2}')
  else
    ef_rustc="unavailable"
  fi
  ef_wgpu=$(cargo_lock_version wgpu)
  ef_egui=$(cargo_lock_version egui)
  # wgpu 30 ships no Vulkan/GL/DX12 backend on macOS, so `Backends::all()`
  # resolves to Metal there. `LUMINA_GPU_BACKENDS` (honoured by
  # `lumina_gpu::select_backends`) can narrow the set and is recorded
  # separately so an explicit pin is visible in the fingerprint.
  if [ -n "${LUMINA_GPU_BACKENDS:-}" ]; then
    ef_backend="$LUMINA_GPU_BACKENDS"
  elif [ "$ef_sys" = Darwin ]; then
    ef_backend="all->metal"
  else
    ef_backend="all"
  fi
  ef_backend_env="${LUMINA_GPU_BACKENDS:-unset}"
  ef_gpu_facts=$(gpu_os_facts)
  ef_adapter=$(echo "$ef_gpu_facts" | sed -n 1p)
  ef_metal=$(echo "$ef_gpu_facts" | sed -n 2p)
  ef_font=$(font_resolution)
  ef_px=$(goldens_size)
  # Derive pixels_per_point from the committed goldens' own IHDR size divided by
  # the pinned logical viewport. `*[!0-9x]*` rejects every non-numeric shape
  # FIRST, including the literals `none` and `mixed` - `mixed` even contains an
  # `x`, so a plain `*x*` would mis-parse it as digits.
  case "$ef_px" in
    "${VIEWPORT_W}x${VIEWPORT_H}")
      ef_scale=1.0
      ;;
    *[!0-9x]*)
      ef_scale=unknown
      ;;
    *x*)
      ef_w=${ef_px%%x*}
      ef_h=${ef_px##*x}
      ef_scale="non-unit:${ef_w}/${VIEWPORT_W},${ef_h}/${VIEWPORT_H}"
      ;;
    *)
      ef_scale="unknown"
      ;;
  esac
  ef_libraw=$(libraw_version)
  ef_fx_count=$(list_fixtures | wc -l | tr -d ' ')
  ef_fx_digest=$(digest_fixtures)
  ef_gd_count=$(list_goldens | wc -l | tr -d ' ')
  ef_gd_digest=$(digest_goldens)

  for ef_key in $FINGERPRINT_KEYS; do
    case "$ef_key" in
      schema.golden_ref) ef_val=1 ;;
      os.name) ef_val="$ef_os" ;;
      os.macos) ef_val="$ef_macos" ;;
      os.arch) ef_val="$ef_arch" ;;
      os.cpu) ef_val="$ef_cpu" ;;
      toolchain.channel) ef_val="$ef_channel" ;;
      toolchain.rustc) ef_val="$ef_rustc" ;;
      wgpu.version) ef_val="$ef_wgpu" ;;
      wgpu.backend) ef_val="$ef_backend" ;;
      wgpu.backend_env) ef_val="$ef_backend_env" ;;
      gpu.adapter) ef_val="$ef_adapter" ;;
      gpu.metal) ef_val="$ef_metal" ;;
      font.resolution) ef_val="$ef_font" ;;
      font.egui) ef_val="$ef_egui" ;;
      ui.golden_px) ef_val="$ef_px" ;;
      ui.scale_factor) ef_val="$ef_scale" ;;
      libraw.version) ef_val="$ef_libraw" ;;
      fixtures.count) ef_val="$ef_fx_count" ;;
      fixtures.digest) ef_val="$ef_fx_digest" ;;
      goldens.count) ef_val="$ef_gd_count" ;;
      goldens.digest) ef_val="$ef_gd_digest" ;;
      *) ef_val="unknown-key" ;;
    esac
    # Strip CR/LF so a value can never break the one-key-per-line format.
    ef_clean=$(printf '%s=%s' "$ef_key" "$ef_val" | tr -d '\r\n')
    printf '%s\n' "$ef_clean"
  done
}

# --- lock file I/O ----------------------------------------------------------

lock_value() {
  # lock_value <key> -> pinned value (empty when the key is absent)
  [ -f "$LOCK" ] || return 0
  lv_line=$(grep "^$1=" "$LOCK" 2>/dev/null | head -n 1 || true)
  [ -n "$lv_line" ] || return 0
  # Normalise a CRLF line ending on READ, exactly like `lock_observed_keys`
  # already does on the structural side. Without this a lock written on Windows
  # (or edited by an editor that writes CRLF) passes the canonical-form check
  # but then reports all 21 keys as differing while pinned and current look
  # identical - a confusing diff instead of a clean match. `record` never emits
  # CR (values are stripped with `tr -d '\r\n'`), so this can only make a
  # hand-edited lock behave like the committed one, never the other way round.
  lv_line=${lv_line%"$LV_CR"}
  printf '%s\n' "${lv_line#*=}"
}

# Key of every non-comment, non-blank line, in file order. A line that is not
# `key=value`, or whose key is not a plain identifier, is reported as a
# structural defect (`!malformed: …` / `!badkey: …`) instead of being silently
# dropped - otherwise a hand-appended line could hide from the format check.
lock_observed_keys() {
  awk '
    { sub(/\r$/, "") }
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*$/ { next }
    {
      eq = index($0, "=")
      if (eq < 2) { print "!malformed: " $0; next }
      key = substr($0, 1, eq - 1)
      if (key !~ /^[A-Za-z0-9._-]+$/) { print "!badkey: " key; next }
      print key
    }
  ' "$LOCK"
}

# 0 when the lock is in canonical form. The key sequence must equal
# FINGERPRINT_KEYS exactly: same keys, same order, same count. That makes the
# documented "Schluesselreihenfolge ist Teil des Formats" actually normative -
# `lock_value` resolves by name and would otherwise take the first match of a
# duplicated or appended key without a word.
lock_is_canonical() {
  lok_observed=$(lock_observed_keys)
  lok_expected=$(printf '%s\n' "$FINGERPRINT_KEYS")
  if [ "$lok_observed" = "$lok_expected" ]; then
    return 0
  fi
  echo "FAIL: $LOCK is not in canonical form." >&2
  echo "FAIL: the format is exactly <key>=<value> once per canonical key, in" >&2
  echo "FAIL: canonical order, no duplicates, no unknown keys, comments with '#'." >&2
  echo "FAIL: observed key sequence:" >&2
  printf '%s\n' "$lok_observed" | sed 's/^/FAIL:   /' >&2
  echo "FAIL: re-pin deliberately with 'record --confirm \"<reason>\"'." >&2
  return 1
}

diff_fingerprints() {
  # One block per differing or missing key, canonical order, `---` separated.
  dk_had=0
  for dk_key in $FINGERPRINT_KEYS; do
    dk_pinned=$(lock_value "$dk_key")
    dk_current=$(printf '%s\n' "$CURRENT" | grep "^$dk_key=" | head -n 1 || true)
    dk_current=${dk_current#*=}
    if [ "$dk_pinned" != "$dk_current" ]; then
      [ "$dk_had" -eq 1 ] && echo "  ---"
      echo "  $dk_key:"
      echo "    pinned:  ${dk_pinned:-<absent>}"
      echo "    current: ${dk_current:-<absent>}"
      dk_had=1
    fi
  done
  if [ "$dk_had" -eq 0 ]; then
    echo "  (no differences)"
  fi
}

lock_warning() {
  if [ "$LOCK" != "$DEFAULT_LOCK" ]; then
    echo "WARNING: using non-default lock file $LOCK (test-only override GOLDEN_REF_LOCK)" >&2
  fi
}

# --- UPDATE_SNAPSHOTS guard -------------------------------------------------

# A truthy UPDATE_SNAPSHOTS means "rewrite the committed goldens". That must
# never happen on an unpinned machine, so the check refuses loudly and the
# caller gets a non-zero exit before any golden is touched.
update_requested() {
  case "$(printf '%s' "${UPDATE_SNAPSHOTS:-}" | tr '[:upper:]' '[:lower:]')" in
    '' | 0 | false | no | off) return 1 ;;
    *) return 0 ;;
  esac
}

refuse_update_on_mismatch() {
  update_requested || return 0
  if [ "$MATCHES" = no ]; then
    echo "REFUSING: UPDATE_SNAPSHOTS is set but this machine is not the pinned golden reference platform." >&2
    echo "REFUSING: no golden was written and no test was started." >&2
    echo "REFUSING: see feature/quality/golden-references.md - either record the platform" >&2
    echo "REFUSING: deliberately (\`sh scripts/golden_ref.sh record --confirm \"<reason>\"\`) or unset UPDATE_SNAPSHOTS." >&2
    exit 1
  fi
}

# --- reporting --------------------------------------------------------------

# MATCHES is `yes` when every key of the current fingerprint equals the pin.
compute_match() {
  MATCHES=yes
  for cm_key in $FINGERPRINT_KEYS; do
    cm_pinned=$(lock_value "$cm_key")
    cm_current=$(printf '%s\n' "$CURRENT" | grep "^$cm_key=" | head -n 1 || true)
    cm_current=${cm_current#*=}
    if [ "$cm_pinned" != "$cm_current" ]; then
      MATCHES=no
      return 0
    fi
  done
}

print_fingerprints() {
  echo "LuminaRust golden reference platform (GOLDEN-REF-30)"
  echo "pin file: $LOCK"
  echo
  echo "detected (this machine):"
  printf '%s\n' "$CURRENT" | sed 's/^/  /'
  echo
  echo "pinned (repo):"
  if [ -f "$LOCK" ]; then
    for pf_key in $FINGERPRINT_KEYS; do
      pf_val=$(lock_value "$pf_key")
      echo "  $pf_key=$pf_val"
    done
  else
    echo "  <missing: $LOCK does not exist>"
  fi
  echo
  echo "verdict: $1"
  if [ "$1" != MATCH ]; then
    echo "differences:"
    diff_fingerprints
  fi
}

# --- subcommands ------------------------------------------------------------

usage() {
  cat <<'EOF'
usage: sh scripts/golden_ref.sh <command> [args]

  print                        show the detected and pinned fingerprint (exit 0
                               even on mismatch - this is the diagnostic view)
  check                        exit 0 when the environment matches the pinned
                               golden reference platform AND the lock is in
                               canonical form, 1 on mismatch
  record --confirm "<reason>"  re-pin scripts/golden_ref.lock from the current
                               environment. Refuses without a single-line
                               --confirm reason of at least 20 characters, and
                               prints the old -> new diff it is about to pin.
                               (This is the `--force-record` of Agents.todo.md:
                               there is no separate flag, `record` is always
                               the deliberate re-pin.)
  gate -- <cmd> [args...]      run `check` first and only then exec <cmd>;
                               this is the hook for any golden regeneration

scope (what this script does NOT do):
  It does not intercept a bare `UPDATE_SNAPSHOTS=1 cargo test …`. The guard
  only covers invocations that go through `check` / `gate`. A golden change is
  anchored by the .githooks/pre-commit re-pin gate, not by this script.

environment:
  UPDATE_SNAPSHOTS             when set to anything but 0/false/no/off, `check`
                               and `gate` REFUSE to proceed on a machine that
                               does not match the pin (exit 1, nothing is run
                               and no golden is written)
  LUMINA_GPU_BACKENDS          recorded verbatim; pins the wgpu backend set
  GOLDEN_REF_LOCK              test-only override of the lock file path; every
                               subcommand warns loudly when it is in effect
EOF
}

case "${1:-print}" in
  print)
    lock_warning
    CURRENT=$(emit_fingerprint)
    compute_match
    if [ "$MATCHES" = yes ]; then print_fingerprints MATCH; else print_fingerprints MISMATCH; fi
    ;;
  check)
    lock_warning
    [ -f "$LOCK" ] || die "pinned fingerprint $LOCK is missing - the reference platform is not recorded in the repo"
    lock_is_canonical || exit 1
    CURRENT=$(emit_fingerprint)
    compute_match
    if [ "$MATCHES" = no ]; then
      print_fingerprints MISMATCH
      refuse_update_on_mismatch
      echo "FAIL: this machine is not the pinned golden reference platform." >&2
      echo "FAIL: see feature/quality/golden-references.md" >&2
      exit 1
    fi
    print_fingerprints MATCH
    echo "OK: golden reference platform matches $LOCK"
    ;;
  record)
    shift
    # Loud on the ONE path that writes, exactly like the three read paths.
    lock_warning
    rc_reason=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --confirm)
          shift
          [ $# -gt 0 ] || die "record: --confirm needs a non-empty reason"
          rc_reason=$1
          ;;
        --confirm=*)
          rc_reason=${1#--confirm=}
          ;;
        *) die "record: unexpected argument '$1' (usage: record --confirm \"<reason>\")" ;;
      esac
      shift
    done
    [ -n "$rc_reason" ] ||
      die "record refuses to run without an explicit, non-interactive --confirm \"<reason>\"; re-pinning the reference platform is never automatic"
    # The reason is written verbatim into the `# Grund:` header of a file that is
    # read back by name, so a newline in it could inject a second `key=value`
    # line and silently out-vote the real pin. Refuse, do not sanitise: a
    # silently rewritten reason would break the written justification that the
    # pre-commit gate and the human reviewer depend on.
    rc_nl=$(printf '\nx')
    rc_nl=${rc_nl%x}
    rc_cr=$(printf '\rx')
    rc_cr=${rc_cr%x}
    case "$rc_reason" in
      *"$rc_nl"*) die "record: reason must be a single line (it is written verbatim into the lock header and a newline could inject a key=value line)" ;;
      *"$rc_cr"*) die "record: reason must not contain CR" ;;
    esac
    case "$rc_reason" in
      *--*) die "record: reason must be plain text without dashes: $rc_reason" ;;
    esac
    # The reason IS the written justification (Agents.todo.md asks for one), so a
    # one-character confirmation is not a justification.
    rc_len=$(printf '%s' "$rc_reason" | wc -c | tr -d ' ')
    if [ "$rc_len" -lt 20 ]; then
      die "record: --confirm needs a justification of at least 20 characters (got $rc_len): $rc_reason"
    fi
    rc_tmp="$LOCK.tmp.$$"
    {
      echo "# golden_ref.lock - gepinnter Golden-Referenz-Fingerabdruck (GOLDEN-REF-30)."
      echo "# Erzeugt von: sh scripts/golden_ref.sh record --confirm \"<reason>\""
      echo "# Grund: $rc_reason"
      echo "#"
      echo "# Format: eine Zeile pro Schluessel als <key>=<value>, '#'-Zeilen sind"
      echo "# Kommentar, Schluesselreihenfolge ist Teil des Formats (kanonisch, von"
      echo "# \`check\` erzwungen). Der Referenz-SOLL steht in"
      echo "# feature/quality/golden-references.md. Diese Goldens sind ein LOKALES"
      echo "# macOS-Gate und werden in CI nie verifiziert (kein GPU-Runner)."
      echo "#"
      emit_fingerprint
    } >"$rc_tmp"
    # Show the exact old -> new diff of the LOCK BEFORE pinning it. This justifies
    # the re-pin itself: it shows which fingerprint keys moved. It does NOT
    # justify the new pixel content - `goldens.digest` is a single hash, so this
    # diff cannot tell a reviewer WHICH golden changed. That evidence is the
    # per-golden `<name>.diff.png` (feature/quality/golden-references.md §8.3).
    if [ -f "$LOCK" ]; then
      rc_old="$LOCK"
    else
      rc_old=/dev/null
    fi
    echo "record: old -> new diff of $LOCK that is about to be pinned:"
    if command -v diff >/dev/null 2>&1; then
      diff -u "$rc_old" "$rc_tmp" || true
    else
      echo "  (no diff(1) on PATH - key table instead; this is NOT a silent fallback)"
      CURRENT=$(grep -v '^[[:space:]]*#' "$rc_tmp" | grep -v '^[[:space:]]*$')
      compute_match
      diff_fingerprints
    fi
    mv "$rc_tmp" "$LOCK"
    echo "recorded $LOCK (reason: $rc_reason)"
    ;;
  gate)
    shift
    [ "${1:-}" = "--" ] && shift
    lock_warning
    [ -f "$LOCK" ] || die "pinned fingerprint $LOCK is missing - the reference platform is not recorded in the repo"
    lock_is_canonical || exit 1
    CURRENT=$(emit_fingerprint)
    compute_match
    if [ "$MATCHES" = no ]; then
      print_fingerprints MISMATCH
      refuse_update_on_mismatch
      echo "FAIL: refusing to run the gated command on an unpinned machine." >&2
      exit 1
    fi
    echo "OK: golden reference platform matches $LOCK - running: $*"
    if [ $# -eq 0 ]; then
      exit 0
    fi
    exec "$@"
    ;;
  help | -h | --help)
    usage
    ;;
  *)
    echo "ERROR: unknown command '$1'" >&2
    usage >&2
    exit 2
    ;;
esac
