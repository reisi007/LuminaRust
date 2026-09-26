#!/bin/sh
# Regression suite for the GOLDEN-REF-30 guard: `scripts/golden_ref.sh` and the
# golden re-pin gate in `.githooks/pre-commit`.
#
# Why a shell suite and not a Rust test: the guard has no Rust test target, its
# whole contract is "exit code + refusal text", and it is shell code. A Rust
# test could only re-implement the shell semantics it is supposed to check.
#
# Sandbox discipline (this is a test, not a second way to break the repo):
#   * every lock file the suite reads or writes lives in a `mktemp -d` sandbox
#     OUTSIDE the repository, addressed through the documented test-only
#     `GOLDEN_REF_LOCK` override;
#   * the pre-commit matrix drives a throwaway `git init` repo in the same
#     sandbox, so the real index is never staged into;
#   * the committed `scripts/golden_ref.lock` and the real git index are
#     fingerprinted before the first and after the last case and compared
#     (see "sandbox discipline" at the end). The real lock is only ever READ.
#   * the trap removes the sandbox on success, on failure and on interrupt.
# Idempotent: every case rebuilds its own state from the sandbox base lock.
#
# Platform independence: the suite never asserts that THIS machine is the
# golden reference platform. It first records a synthetic lock from the current
# environment (`record` into the sandbox) and asserts against that. So it runs
# unchanged on the macOS reference machine and on a plain `ubuntu-latest` CI
# runner, where `sw_vers` / `system_profiler` / `rustc` are missing and every
# such value is legitimately `unavailable`.
#
# The price of that design, and the two places it is deliberately broken: a
# `record`/`check` pair only ever compares a value against a value the SAME code
# produced, so a self-consistent change to a *derivation* (how `png_size` reads
# the IHDR, how `ui.scale_factor` divides by the viewport, which files the golden
# inventory enumerates and which of them the digest hashes) moves both sides and
# stays green. The section "value detectors vs committed inputs" is the first
# exception: it feeds the real derivations a committed fixture whose bytes are
# stated in `scripts/fixtures/README.md` and asserts a LITERAL expected value.
# The section "golden inventory layer" is the second: it re-derives the golden
# inventory and the `goldens.digest` value OUTSIDE the guard, from `git ls-files`
# and the golden bytes, and asserts the shipped layer equals it.
#
# Usage: sh scripts/golden_ref_test.sh        (exit 0 = all cases passed)
# Normative spec: feature/quality/golden-references.md §4
#
# shellcheck disable=SC2329
# The `set_*` / `mc_*` helpers of the pre-commit matrix are invoked indirectly
# (their name is passed to mc_commit and then called), which shellcheck cannot
# follow. They are all used; see the matrix table below.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/golden_ref.sh"
HOOK="$ROOT/.githooks/pre-commit"
HOOKS_DIR="$ROOT/.githooks"
REAL_LOCK="$ROOT/scripts/golden_ref.lock"
GOLDENS_REL='crates/lumina-gui/tests/snapshots/base.png'
LOCK_REL='scripts/golden_ref.lock'

# The canonical key list, restated INDEPENDENTLY of the script under test. A
# test that parsed FINGERPRINT_KEYS out of golden_ref.sh would assert that the
# script equals itself; this copy makes a key added, removed or renamed there a
# test failure.
CANONICAL_KEYS="schema.golden_ref
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

BASE_REASON="guard test: synthetic lock recorded from the machine running this suite"
OTHER_REASON="guard test: a different, still legitimate written reason"
NL='
'
CR=$(printf '\r')

# --- the one committed input the guard suite checks against literals ---------
#
# A 33-byte IHDR probe, written from a literal byte list (provenance and
# regeneration command in `scripts/fixtures/README.md`). The guard's `png_size`
# reads the 8 bytes at file offset 16..23, which the PNG spec defines as width
# then height, each 4x u32 BIG endian.
PNG_IHDR_PROBE_REL='scripts/fixtures/png_ihdr_probe.png'
# The literal expectations, spelled out here as decimal and NOT computed: the
# committed bytes are 01 02 03 04 (0x01020304) and 05 06 07 08 (0x05060708).
# Every byte of both dimensions is non-zero and distinct, so a swapped byte
# pair, a swapped half, a little endian read and a truncated read all produce a
# different string and cannot pass by accident.
PNG_IHDR_PROBE_W=16909060
PNG_IHDR_PROBE_H=84281096

pass=0
fail=0
failed_labels=""
OUT=""
RC=0

SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/golden_ref_test.XXXXXX") || exit 1
case "$SANDBOX" in
  "$ROOT" | "$ROOT"/*)
    echo "REFUSING to run: sandbox '$SANDBOX' is inside the repository" >&2
    exit 1
    ;;
esac
cleanup() {
  case "$SANDBOX" in
    "${TMPDIR:-/tmp}"/golden_ref_test.*) rm -rf "$SANDBOX" ;;
  esac
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

section() {
  printf '\n== %s\n' "$1"
}
ok() {
  pass=$((pass + 1))
  printf '  ok    %s\n' "$1"
}
no() {
  fail=$((fail + 1))
  failed_labels="$failed_labels
  - $1"
  printf '  FAIL  %s\n' "$1"
  if [ $# -gt 1 ]; then
    printf '%s\n' "$2" | sed 's/^/          | /'
  fi
}
detail() {
  printf '  ..    %s\n' "$1"
}

# --- runners ----------------------------------------------------------------

# gr <args...>: run the guard against the synthetic base lock.
gr() {
  OUT=$(GOLDEN_REF_LOCK="$BASE_LOCK" sh "$SCRIPT" "$@" 2>&1) && RC=0 || RC=$?
}

# gr_lock <lockfile> <args...>
gr_lock() {
  gr_l=$1
  shift
  OUT=$(GOLDEN_REF_LOCK="$gr_l" sh "$SCRIPT" "$@" 2>&1) && RC=0 || RC=$?
}

# gr_env <lockfile> <VAR=val>... -- <args...>
# The VAR=val pairs are passed through `env` by WORD SPLITTING, so a value with
# a space is not supported here (use a direct prefix assignment for those).
gr_env() {
  gr_el=$1
  shift
  gr_ee=""
  while [ "${1:-}" != "--" ]; do
    gr_ee="$gr_ee$1 "
    shift
  done
  shift
  # Word splitting of the VAR=val pairs is deliberate; `env` takes them apart.
  # shellcheck disable=SC2086
  OUT=$(env GOLDEN_REF_LOCK="$gr_el" $gr_ee sh "$SCRIPT" "$@" 2>&1) && RC=0 || RC=$?
}

# --- assertions -------------------------------------------------------------

chk_rc() {
  # chk_rc <want-rc> <label>
  if [ "$RC" -eq "$1" ]; then
    ok "$2 (rc=$RC)"
  else
    no "$2" "expected rc=$1, got rc=$RC
--- output ---
$OUT"
  fi
}
chk_has() {
  # chk_has <substring> <label>
  case "$OUT" in
    *"$1"*) ok "$2" ;;
    *) no "$2" "expected output to contain: $1
--- output ---
$OUT" ;;
  esac
}
chk_lacks() {
  # chk_lacks <substring> <label>
  case "$OUT" in
    *"$1"*) no "$2" "expected output NOT to contain: $1
--- output ---
$OUT" ;;
    *) ok "$2" ;;
  esac
}
chk_true() {
  # chk_true <shell-condition-result 0/1> <label> [detail-text]
  if [ "$1" -eq 0 ]; then
    ok "$2"
  else
    no "$2" "${3:-}"
  fi
}

# sha256 of a file, using whichever helper exists (same fallback as the guard).
sha_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

# sha256 of stdin, same helper order as the guard. Prints the literal
# `no-sha-tool` when neither helper exists, so that a comparison against it fails
# loudly instead of comparing two empty strings (DoD §10).
sha_stdin() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    echo "no-sha-tool"
  fi
}

# A plausible sha256: 64 lowercase hex characters, nothing else. Used as the
# PRECONDITION of the "the real lock is byte-identical" guard: without it that
# guard compares '' with '' whenever the hashing tool cannot read the file, and
# passes.
is_sha256() {
  case "${1:-}" in
    '' | *[!0-9a-f]*) return 1 ;;
  esac
  [ "${#1}" -eq 64 ]
}

# A plausible git object id: 40 hex characters (sha1 repositories) or 64
# (sha256 repositories). Used as the PRECONDITION of the "the real git index is
# unchanged" guard, which otherwise compares the literal `no-index` with itself
# whenever `git write-tree` cannot run.
is_oid() {
  case "${1:-}" in
    '' | *[!0-9a-f]*) return 1 ;;
  esac
  case "${#1}" in
    40 | 64) return 0 ;;
    *) return 1 ;;
  esac
}

# The tree OID the index currently writes to, or NOTHING plus a non-zero exit
# when git cannot produce one.
#
# This helper deliberately does NOT end in `|| echo "no-index"`. That fallback is
# what made the end-of-run index guard vacuous: a suite run outside a git work
# tree captured `no-index` on both sides and reported "the real git index is
# unchanged" without ever having read an index. A capture that cannot be made is
# now reported as a failure of the capture, and the precondition assertion turns
# it red.
index_tree_oid() {
  git -C "$1" write-tree 2>/dev/null
}

# chk_inventory <label> <enumerated-by-the-guard> <expected-list> <what-it-is>
# Path-for-path equality, not a count: a list can hold the right number of
# entries and still have dropped one golden and picked up a file that is not a
# golden, which is precisely the "one missing classification row" case that
# `golden_ref.sh check` cannot see (feature/quality/golden-fixtures.md §4).
chk_inventory() {
  ci_lbl=$1
  ci_got=$2
  ci_want=$3
  ci_want_desc=$4
  ci_got_n=$(printf '%s\n' "$ci_got" | grep -c . || true)
  ci_want_n=$(printf '%s\n' "$ci_want" | grep -c . || true)
  if [ "$ci_got" = "$ci_want" ]; then
    ok "$ci_lbl ($ci_want_n paths)"
  else
    # `comm` and `diff` need real files - a multi-line list is not a filename -
    # and a path list is the only honest way to say WHICH golden is missing. Both
    # files land in the sandbox, which the trap removes.
    ci_gf="$SANDBOX/inventory.got"
    ci_wf="$SANDBOX/inventory.want"
    printf '%s\n' "$ci_got" >"$ci_gf"
    printf '%s\n' "$ci_want" >"$ci_wf"
    ci_extra=$(LC_ALL=C comm -23 "$ci_gf" "$ci_wf")
    ci_missing=$(LC_ALL=C comm -13 "$ci_gf" "$ci_wf")
    ci_extra_n=$(printf '%s\n' "$ci_extra" | grep -c . || true)
    ci_missing_n=$(printf '%s\n' "$ci_missing" | grep -c . || true)
    no "$ci_lbl" "the guard enumerated $ci_got_n paths, $ci_want_desc has $ci_want_n
inventored by the guard but not in $ci_want_desc: $ci_extra_n
$(printf '%s\n' "$ci_extra" | head -n 5)
in $ci_want_desc but NOT inventoried by the guard: $ci_missing_n
$(printf '%s\n' "$ci_missing" | head -n 5)"
  fi
}

# --- preflight: the tool under test must exist and be shell ------------------

section "preflight"
if [ ! -f "$SCRIPT" ]; then
  echo "cannot continue: the script under test does not exist: $SCRIPT" >&2
  exit 1
fi
ok "scripts/golden_ref.sh exists"
if [ -x "$HOOK" ]; then
  ok ".githooks/pre-commit is committed executable"
else
  no ".githooks/pre-commit is committed executable" \
     "a lost exec bit disables the gate silently for every clone that installed it"
fi

# Fingerprint the real lock and the real index; re-checked at the end.
#
# Both captures used to swallow their own failure. `sha_of` prints nothing when
# the hashing tool errors, and the index capture fell back to the literal
# `no-index`. Measured consequence: outside a git work tree the index guard
# compared `no-index` with `no-index`, and with an unreadable lock the byte
# guard compared '' with '' - both reported success without ever having compared
# anything. The two guards themselves are kept exactly as they are (they exist so
# that nobody mistakes this suite for verifying that *the suite itself* touched
# nothing); what is added is a PRECONDITION per guard, right here and at the end
# of the run: the captured value must be a plausible sha256 / object id. A
# broken git or a missing lock therefore fails loudly instead of passing
# silently.
REAL_LOCK_SHA_BEFORE=$(sha_of "$REAL_LOCK")
REAL_INDEX_BEFORE=$(index_tree_oid "$ROOT")

# Kills: "the real lock is byte-identical after the suite" passing on two empty
# strings because `sha_of` could not read the lock (deleted, unreadable, or no
# sha helper on PATH). With a plausible before-sha the comparison is a real
# byte comparison; without one the suite is red instead of green.
chk_true "$(is_sha256 "$REAL_LOCK_SHA_BEFORE" && echo 0 || echo 1)" \
  "precondition: the real lock's before-sha is a sha256 digest, not an empty string" \
  "sha_of $REAL_LOCK returned: '$REAL_LOCK_SHA_BEFORE'
Without a real digest the 'byte-identical' comparison at the end of this run
would compare two empty strings and pass without having read the lock."

# Kills: "the real git index is unchanged after the suite" passing on the
# `no-index` fallback constant. It also pins WHY that could not happen silently:
# outside a work tree, or with an index git cannot write a tree from (an
# unmerged index mid-rebase), the capture now fails and this assertion is red
# instead of the guard reporting a comparison of `no-index` with `no-index`.
chk_true "$(is_oid "$REAL_INDEX_BEFORE" && echo 0 || echo 1)" \
  "precondition: the real index's before-OID is a tree object id, not the no-index fallback" \
  "git -C $ROOT write-tree returned: '$REAL_INDEX_BEFORE'
A 'no-index' or an empty string here means the end-of-run index comparison would
compare a constant with itself (or nothing with nothing) and pass for the wrong
reason. Outside a work tree, or with an unmerged index, this suite cannot
establish that it touched nothing - resolve that state before running it."

# --- the synthetic base lock ------------------------------------------------

section "synthetic base lock (recorded from this machine, in the sandbox)"
mkdir -p "$SANDBOX/locks"
BASE_LOCK="$SANDBOX/locks/base.lock"
gr_lock "$BASE_LOCK" record --confirm "$BASE_REASON"
chk_rc 0 "record writes the sandbox base lock"
chk_has "WARNING: using non-default lock file" "record warns about a non-default lock path"
if [ -f "$BASE_LOCK" ]; then
  ok "base lock exists"
else
  no "base lock exists" "no file at $BASE_LOCK"
fi

# The base lock must contain exactly the independently restated key list, in
# that order. This is the anchor for the sweep below.
observed=$(grep -v '^[[:space:]]*#' "$BASE_LOCK" | grep -v '^[[:space:]]*$' |
  sed 's/=.*//')
if [ "$observed" = "$CANONICAL_KEYS" ]; then
  ok "base lock has the 21 canonical keys in canonical order"
else
  no "base lock has the 21 canonical keys in canonical order" \
     "observed:
$observed
expected:
$CANONICAL_KEYS"
fi
n_keys=$(printf '%s\n' "$CANONICAL_KEYS" | grep -c .)
chk_true "$([ "$n_keys" -eq 21 ] && echo 0 || echo 1)" \
  "the restated key list really has 21 keys (got $n_keys)"

gr check
chk_rc 0 "check passes against the base lock"
chk_has "OK: golden reference platform matches" "check announces the match"
chk_lacks "is not in canonical form" "check does not complain about the lock format"

# --- sweep: every one of the 21 keys must be load-bearing -------------------

section "21-key perturbation sweep (each key alone must turn check red)"
for k in $CANONICAL_KEYS; do
  pfile="$SANDBOX/locks/perturbed.lock"
  sed "s|^$k=.*|$k=perturbed-$k|" "$BASE_LOCK" >"$pfile"
  if cmp -s "$pfile" "$BASE_LOCK"; then
    no "sweep/$k" "the perturbation did not change the lock at all"
    continue
  fi
  gr_lock "$pfile" check
  if [ "$RC" -ne 1 ]; then
    no "sweep/$k" "expected rc=1, got rc=$RC
--- output ---
$OUT"
    continue
  fi
  blocks=$(printf '%s\n' "$OUT" | grep -c '^  [A-Za-z0-9._-]*:$' || true)
  if [ "$blocks" -ne 1 ]; then
    no "sweep/$k" "expected exactly 1 differing key in the diff, got $blocks
--- output ---
$OUT"
    continue
  fi
  case "$OUT" in
    *"  $k:"*) ok "sweep/$k -> rc=1, and the diff names exactly that key" ;;
    *) no "sweep/$k" "diff does not name the perturbed key
--- output ---
$OUT" ;;
  esac
done

# --- value detectors vs committed, hand-checkable inputs --------------------

section "value detectors vs committed inputs (the derivations, not the keys)"

# Everything above is self-consistent by design: `record` fills the base lock
# with the same code that `check` later reads, so a change to a derivation
# moves the expected value along with it. Two derivations were measured to be
# completely uncovered that way: swapping the two high width bytes in
# `png_size`, and forcing the `ui.scale_factor` case to always report `1.0`,
# each left the whole suite green.
#
# fd_probe closes that for both. It runs the REAL `emit_fingerprint` of the
# REAL `scripts/golden_ref.sh` - sourced, never copied, because re-typing
# `png_size` or the scale case into this suite would assert that the copy
# agrees with itself - with exactly two things replaced: the golden inventory
# (`list_goldens`, so the committed probe stands in for the real goldens) and
# the logical viewport (`VIEWPORT_W` / `VIEWPORT_H`). Every line of the
# derivation under test is the shipped one. The values land in $FD_PX
# (`ui.golden_px`) and $FD_SCALE (`ui.scale_factor`).
#
# The subshell matters: sourcing brings the script's own `set -eu` and its ~20
# globals with it, and the caller's positional parameters survive `.` (so the
# subcommand dispatch is steered with an explicit `set --`).
fd_probe() {
  FD_OUT=$(
    set -u
    fd_want_root=$1
    fd_want_w=$2
    fd_want_h=$3
    set -- print
    # Sourced, not copied. Shellcheck cannot follow a non-constant source, and
    # the file under test is linted in its own right by the same shellcheck run
    # - following it here would only re-report its assignments as changes to the
    # suite's own variables (SC2031). SC2034 on the two viewport lines: they are
    # read by the sourced `emit_fingerprint`, not by this suite.
    # shellcheck disable=SC1090
    . "$SCRIPT" >/dev/null 2>&1
    # The sourced script derives its repository root from `$0`, which is THIS
    # suite's path. That is not a contract, it is an accident of how `.` works,
    # so it is checked instead of assumed: a wrong root would silently make
    # png_size read a file that does not exist and report `?`.
    if [ "$ROOT" != "$fd_want_root" ]; then
      echo "fd_probe: sourced ROOT=$ROOT but the suite root is $fd_want_root" >&2
      exit 1
    fi
    list_goldens() { printf '%s\n' "$PNG_IHDR_PROBE_REL"; }
    # shellcheck disable=SC2034
    VIEWPORT_W=$fd_want_w
    # shellcheck disable=SC2034
    VIEWPORT_H=$fd_want_h
    emit_fingerprint
  ) || return 1
  FD_PX=$(printf '%s\n' "$FD_OUT" | grep '^ui\.golden_px=' | head -n 1)
  FD_SCALE=$(printf '%s\n' "$FD_OUT" | grep '^ui\.scale_factor=' | head -n 1)
  FD_PX=${FD_PX#ui.golden_px=}
  FD_SCALE=${FD_SCALE#ui.scale_factor=}
}

# (a) png_size: the IHDR byte order and endianness. `ui.golden_px` is the
# single-golden case of the real `goldens_size`, i.e. `png_size` verbatim.
if [ -f "$ROOT/$PNG_IHDR_PROBE_REL" ]; then
  ok "fixture/$PNG_IHDR_PROBE_REL is committed"
else
  no "fixture/$PNG_IHDR_PROBE_REL is committed" "missing: $ROOT/$PNG_IHDR_PROBE_REL"
fi
# The fixture's own bytes are read here straight from the file, without
# png_size, so the literal expectation below is not produced by the code under
# test and a corrupted fixture fails here first, with a readable message.
probe_hex=$(od -An -tx1 -j 16 -N 8 -v "$ROOT/$PNG_IHDR_PROBE_REL" 2>/dev/null | tr -d ' \n')
if [ "$probe_hex" = "0102030405060708" ]; then
  ok "fixture/$PNG_IHDR_PROBE_REL really carries the stated IHDR bytes 01 02 03 04 05 06 07 08"
else
  no "fixture/$PNG_IHDR_PROBE_REL really carries the stated IHDR bytes 01 02 03 04 05 06 07 08" \
     "bytes 16..23 read as: $probe_hex"
fi

fd_probe "$ROOT" 1024 720
chk_true "$([ "$FD_PX" = "$PNG_IHDR_PROBE_W"x"$PNG_IHDR_PROBE_H" ] && echo 0 || echo 1)" \
  "png_size reads the IHDR big endian: $PNG_IHDR_PROBE_W x $PNG_IHDR_PROBE_H" \
  "ui.golden_px was: $FD_PX (a byte swap, a swapped half or a little endian read would all differ)"

# (b) ui.scale_factor: the comparison against the logical viewport, reached with
# a synthetic one. A golden that matches the viewport is the unit case; a
# golden that does not is the documented `non-unit:<w>/<VIEWPORT_W>,<h>/<VIEWPORT_H>`
# form, and BOTH divisors have to come from the injected viewport.
fd_probe "$ROOT" "$PNG_IHDR_PROBE_W" "$PNG_IHDR_PROBE_H"
chk_true "$([ "$FD_SCALE" = "1.0" ] && echo 0 || echo 1)" \
  "ui.scale_factor is 1.0 when the golden matches the viewport ($PNG_IHDR_PROBE_W x $PNG_IHDR_PROBE_H)" \
  "ui.scale_factor was: $FD_SCALE"

fd_probe "$ROOT" 1024 720
chk_true "$([ "$FD_SCALE" = "non-unit:$PNG_IHDR_PROBE_W/1024,$PNG_IHDR_PROBE_H/720" ] && echo 0 || echo 1)" \
  "ui.scale_factor names both divisors for a non-matching golden (1024 x 720 viewport)" \
  "ui.scale_factor was: $FD_SCALE"

fd_probe "$ROOT" 800 600
chk_true "$([ "$FD_SCALE" = "non-unit:$PNG_IHDR_PROBE_W/800,$PNG_IHDR_PROBE_H/600" ] && echo 0 || echo 1)" \
  "ui.scale_factor reads the viewport out of the injected values, not out of a constant (800 x 600 viewport)" \
  "ui.scale_factor was: $FD_SCALE"

# --- the golden inventory layer: the digest backstop --------------------------

section "golden inventory layer (goldens.count / goldens.digest, the §3.1 backstop)"

# WHAT this section is for. `feature/quality/golden-references.md` §3.1 makes
# `goldens.digest` a hard part of the pin - "jede Änderung an einem committeten
# Golden erzeugt einen Mismatch" - and §11.1 names `goldens.digest` plus
# `goldens.count` as what is LEFT after a golden slipped in through
# cherry-pick/revert/rebase/merge or through an uninstalled hook.
# `feature/quality/golden-fixtures.md` §4 states the gap from the other side: the
# R/S1/S2 classification table has one row per committed golden, and
# "`golden_ref.sh check` vergleicht Digests, nicht diese Tabelle - eine fehlende
# Zeile fällt dort nicht auf".
#
# WHY the sections above cannot close it: they are self-consistent by design.
# `record` fills the base lock from the same code `check` reads, so any change to
# the inventory layer moves the expected value along with it. Three mutations of
# `scripts/golden_ref.sh` were applied one at a time; each of them left ALL 188
# pre-existing assertions green, and each is now caught (measured, one suite run
# per mutation):
#
#   M1  `list_goldens` reports only the `develop_*` subset (25 of the 66 goldens)
#       -> 198 passed, 3 failed: the count, the set and the digest below
#   M2  `goldens.digest` hashes only the FIRST golden
#       -> 200 passed, 1 failed: the digest below
#   M3  the digest mode is nailed to `walk:` although git is present
#       -> 199 passed, 2 failed: the git: mode and the mode-in-value below
#
# Every assertion below names the mutation it kills.
#
# Precondition 1 - kills: all four inventory assertions below "passing" on a
# machine where there is no git at all, because then the guard is CORRECTLY in
# its `walk:` fallback and none of them can say anything about mode selection.
# The suite needs git unconditionally anyway (the pre-commit matrix, the index
# fingerprint at the end of this run), so requiring a work tree adds no new
# environment assumption - it is asserted rather than assumed, so that a checkout
# without git reports WHY instead of quietly moving the layer under test.
chk_true "$(git -C "$ROOT" rev-parse --is-inside-work-tree 2>/dev/null |
    grep -qx true && echo 0 || echo 1)" \
  "precondition: git reports a work tree at the suite root" \
  "git -C $ROOT rev-parse --is-inside-work-tree said: $(git -C "$ROOT" rev-parse --is-inside-work-tree 2>&1)"

# The independent enumerations. Both are derived HERE, in the suite's own shell,
# with plain `git` and `find` - never through the sourced `list_tracked` /
# `list_goldens`. Comparing the guard's inventory with a list that came out of
# the same functions would only prove that the guard agrees with itself, which is
# the exact failure mode this section exists to remove.
COMMITTED_GOLDENS=$(git -C "$ROOT" ls-files --cached -- \
  'crates/lumina-gui/tests/snapshots/*.png' | LC_ALL=C sort)
COMMITTED_GOLDEN_N=$(printf '%s\n' "$COMMITTED_GOLDENS" | grep -c . || true)

# Precondition 2 - kills: `goldens.count == <n committed>` and the two inventory
# comparisons below passing on two EMPTY lists, which is what a repository
# without a golden baseline would look like.
chk_true "$([ "$COMMITTED_GOLDEN_N" -gt 0 ] && echo 0 || echo 1)" \
  "precondition: git lists a non-empty committed golden baseline ($COMMITTED_GOLDEN_N paths)" \
  "git ls-files --cached -- 'crates/lumina-gui/tests/snapshots/*.png' returned nothing.
Every count/list comparison in this section would then compare an empty side
with an empty side and pass without having covered a single golden."

# The same set from the FILESYSTEM, which is what the `walk:` fallback reads.
# The run-artefact exclusions are the ones §3.1 names (`*.new.png` / `*.diff.png`
# / `*.old.png` are egui_kittest output, never baseline). They are not cosmetic:
# this checkout really does carry such artefacts on disk (a kittest run leaves
# them next to the goldens), so a re-derivation without the exclusions would
# count 90 files where the guard counts 66 - and the exclusion itself is
# documented behaviour that nothing else checks.
DISK_GOLDENS=$(find "$ROOT/crates/lumina-gui/tests/snapshots" -maxdepth 1 -type f -name '*.png' |
  sed "s|^$ROOT/||" |
  awk '!/\.new\.png$/ && !/\.diff\.png$/ && !/\.old\.png$/' |
  LC_ALL=C sort)

# The expected `goldens.digest`, re-derived from the independent list above in
# the format §3.1 documents: `<mode>:<sha256>` over newline-terminated
# `<pfad> <sha256>` lines, a listed but absent file counted as `absent`. The
# format is duplicated here on purpose - it is the *only* thing that gives
# `goldens.digest` a meaning outside the code that produces it. Without this
# expectation a digest that hashes ONE golden is indistinguishable from a digest
# that hashes all of them, which is exactly mutation M2.
EXP_GOLDEN_LINES=$(printf '%s\n' "$COMMITTED_GOLDENS" |
  while IFS= read -r gl_rel; do
    [ -n "$gl_rel" ] || continue
    if [ -f "$ROOT/$gl_rel" ]; then
      printf '%s %s\n' "$gl_rel" "$(sha_of "$ROOT/$gl_rel")"
    else
      printf '%s absent\n' "$gl_rel"
    fi
  done)
EXP_GOLDEN_SHA=$(printf '%s\n' "$EXP_GOLDEN_LINES" | sha_stdin)

# gi_probe <suite root> <git-shim-dir-or-empty>
#
# Drives the REAL `emit_fingerprint` of the REAL `scripts/golden_ref.sh` with its
# inventory layer UNTOUCHED. The difference to `fd_probe` is the point of this
# section: nothing is overridden here - `list_goldens`, `digest_goldens` and the
# mode selection are the shipped code, because the inventory IS the code under
# test. Sourced, never copied, for the same reason as in `fd_probe`: re-typing
# `list_goldens` into this suite would assert that the copy agrees with itself.
# One replacement input only: `PATH`, optionally with a shim directory in front.
#
# The enumerated list is emitted between two marker lines so this suite can
# compare "what the guard enumerated" against its own enumeration, instead of
# trying to read a file list back out of a digest. `emit_fingerprint` runs the
# inventory twice on its own (once for `goldens.count`, once for the digest), so
# both values are taken from ONE run and are therefore known to describe the same
# inventory.
gi_probe() {
  GI_OUT=$(
    set -u
    gi_want_root=$1
    gi_git_shim=$2
    if [ -n "$gi_git_shim" ]; then
      # The subshell IS the isolation mechanism here: the shim must reach the
      # guard's `git` call and nothing else, and `$( )` is what keeps it from
      # leaking into the rest of the suite. Hence the SC2030.
      # shellcheck disable=SC2030
      PATH="$gi_git_shim:$PATH"
      export PATH
    fi
    set -- print
    # Sourced, not copied; see the SC1090 note in fd_probe above.
    # shellcheck disable=SC1090
    . "$SCRIPT" >/dev/null 2>&1
    # The sourced script derives its repository root from `$0`, which is THIS
    # suite's path. An accident of how `.` works, so it is checked, not assumed.
    if [ "$ROOT" != "$gi_want_root" ]; then
      echo "gi_probe: sourced ROOT=$ROOT but the suite root is $gi_want_root" >&2
      exit 1
    fi
    emit_fingerprint
    printf '%s\n' '### guard inventory begin'
    list_goldens
    printf '%s\n' '### guard inventory end'
  ) || return 1
  GI_COUNT=$(printf '%s\n' "$GI_OUT" | grep '^goldens\.count=' | head -n 1)
  GI_DIGEST=$(printf '%s\n' "$GI_OUT" | grep '^goldens\.digest=' | head -n 1)
  GI_COUNT=${GI_COUNT#goldens.count=}
  GI_DIGEST=${GI_DIGEST#goldens.digest=}
  GI_MODE=${GI_DIGEST%%:*}
  GI_SHA=${GI_DIGEST#*:}
  GI_LIST=$(printf '%s\n' "$GI_OUT" |
    sed -n '/^### guard inventory begin$/,/^### guard inventory end$/p' |
    sed '1d;$d')
}

# A git that does not work at all - the documented trigger for the fallback
# (§3.1): `list_tracked` is `git ls-files ... || true`, so a failing git leaves it
# empty and the inventory has to switch to `walk:` mode. Reaching the fallback
# through production code with the environment changed beats patching the
# selection. The shim lives in the sandbox and is prepended to PATH ONLY inside
# the probe subshell, so the rest of this suite - which drives the real hook with
# a real git - is unaffected.
NO_GIT_DIR="$SANDBOX/shim-no-git"
mkdir -p "$NO_GIT_DIR"
{
  echo '#!/bin/sh'
  echo '# Test shim: no usable git at all, i.e. the documented trigger for the'
  echo '# walk: digest fallback (golden-references.md §3.1).'
  echo 'echo "fatal: simulated: git is unavailable in this shim" >&2'
  echo 'exit 127'
} >"$NO_GIT_DIR/git"
chmod +x "$NO_GIT_DIR/git"

# --- run 1: git available, so the guard must be in git: mode -----------------
# Initialised so that a probe that cannot run leaves the run-2 assertions with
# readable empty values instead of tripping `set -u`.
gi_git_mode=
gi_git_sha=
gi_git_count=
gi_git_list=
gi_walk_mode=
gi_walk_sha=
gi_walk_list=

if gi_probe "$ROOT" ''; then
  gi_git_mode=$GI_MODE
  gi_git_sha=$GI_SHA
  gi_git_count=$GI_COUNT
  gi_git_list=$GI_LIST

  # Kills M3: the digest mode nailed to `walk:` although git is present. A
  # `walk:`-only pin would make the whole repository's golden history hash
  # through a fallback that cannot see the ignore rules, and it would look
  # perfectly stable.
  chk_true "$([ "$gi_git_mode" = git ] && echo 0 || echo 1)" \
    "inventory/git: is the selected digest mode while git can enumerate (kills a mode nailed to walk:)" \
    "goldens.digest was: $gi_git_mode:$gi_git_sha
Expected the git: mode, because 'git -C $ROOT ls-files' lists $COMMITTED_GOLDEN_N goldens."

  # Kills M1: `list_goldens` reporting only a subset (measured: the `develop_*`
  # subset, 25 of 66). A narrower inventory shrinks the count AND the set the
  # digest covers, so 41 changed goldens would no longer move the pin.
  chk_true "$([ "$gi_git_count" = "$COMMITTED_GOLDEN_N" ] && echo 0 || echo 1)" \
    "inventory/goldens.count covers every committed golden ($COMMITTED_GOLDEN_N)" \
    "goldens.count was: $gi_git_count
git ls-files --cached -- 'crates/lumina-gui/tests/snapshots/*.png' reports: $COMMITTED_GOLDEN_N
A smaller count means the inventory layer dropped committed goldens."

  # The same claim path-for-path instead of by count: a list can have the right
  # length and still have dropped one golden in favour of a file that is not a
  # golden. This is the assertion behind the sentence in
  # feature/quality/golden-fixtures.md §4 that `check` cannot see a missing row
  # of the classification table.
  chk_inventory "inventory/git: enumerated set is exactly the committed baseline" \
    "$gi_git_list" "$COMMITTED_GOLDENS" "the committed baseline (git ls-files)" \
    "NOTE: by §3.1 the inventory also covers untracked, non-ignored files, so a
FEWER entry usually means a filter dropped goldens and an EXTRA entry usually
means an untracked PNG in the snapshots dir that should be staged or removed."

  # Kills M2: `goldens.digest` hashing only the FIRST golden. The digest is the
  # durable backstop (§3.1, §11.1) - a change to 65 of 66 goldens has to move
  # it - and nothing in the self-consistent sections above can see that, because
  # `record` would simply pin the shortened digest.
  chk_true "$([ "$gi_git_sha" = "$EXP_GOLDEN_SHA" ] && echo 0 || echo 1)" \
    "inventory/goldens.digest is the digest of the WHOLE committed inventory, not of one golden" \
    "guard reported: $gi_git_mode:$gi_git_sha
independently derived over all $COMMITTED_GOLDEN_N committed goldens: git:$EXP_GOLDEN_SHA
A digest that covers fewer paths than the inventory has a checksum over less than
the baseline it is supposed to protect."
else
  no "inventory/git: probe could not run the real emit_fingerprint" \
     "the sourced scripts/golden_ref.sh produced no usable fingerprint output"
fi

# --- run 2: git cannot enumerate, so the guard must fall back to walk: -------

if gi_probe "$ROOT" "$NO_GIT_DIR"; then
  gi_walk_mode=$GI_MODE
  gi_walk_sha=$GI_SHA
  gi_walk_list=$GI_LIST

  # Kills the mirror image of M3: a mode selection that ignores `list_tracked`
  # and always reports `git:`, i.e. an untested fallback branch. A pin that can
  # only ever be written in one mode also means the fallback is never exercised.
  chk_true "$([ "$gi_walk_mode" = walk ] && echo 0 || echo 1)" \
    "inventory/walk: is the selected digest mode when git cannot enumerate (kills a mode nailed to git:)" \
    "with an unusable git on PATH, goldens.digest was: $gi_walk_mode:$gi_walk_sha
list_tracked returned nothing, so §3.1 requires the walk: fallback."

  # Kills a fallback that finds nothing (count 0, an empty digest over no paths)
  # or one that sweeps the egui_kittest run artefacts into the pin. The second is
  # not hypothetical: this checkout carries `*.new.png` / `*.diff.png` /
  # `*.old.png` next to the goldens, and they are excluded by the documented rule
  # §3.1 names, not by anything else.
  chk_inventory "inventory/walk: enumerated set is exactly the on-disk baseline" \
    "$gi_walk_list" "$DISK_GOLDENS" "the on-disk baseline (find, run artefacts excluded)" \
    "NOTE: a difference here is a wrong find/filter in the fallback, e.g. the
*.new.png / *.diff.png / *.old.png exclusions of §3.1 missing, or a maxdepth
that stops covering the subdirectories git mode would list."

  # The mode is PART of the digest value (§3.1: "ein Moduswechsel erscheint damit
  # als Mismatch und nicht als unerklärlicher Hashwert"). In this checkout both
  # modes enumerate the very same 66 files, so the two values differ exactly by
  # that prefix - which is the whole content of the claim: it pins "the mode is
  # in the value", not "the two modes see different files". A digest that dropped
  # the prefix would be identical in both runs and would let a mode switch hide
  # behind an unchanged hash.
  if [ "$gi_git_mode:$gi_git_sha" != "$gi_walk_mode:$gi_walk_sha" ]; then
    gi_digests_differ=yes
  else
    gi_digests_differ=no
  fi
  chk_true "$([ "$gi_digests_differ" = yes ] && echo 0 || echo 1)" \
    "inventory/the digest value carries the mode, so a mode switch shows up as a mismatch" \
    "git: mode: $gi_git_mode:$gi_git_sha
walk: mode: $gi_walk_mode:$gi_walk_sha
Both enumerations cover the same $COMMITTED_GOLDEN_N files here, so the mode
prefix is the only difference - and it is the difference the pin must carry."
else
  no "inventory/walk: probe could not run the real emit_fingerprint" \
     "the sourced scripts/golden_ref.sh produced no usable fingerprint output under the no-git shim"
fi

# --- canonical-form tamper variants -----------------------------------------

section "non-canonical lock variants (all must be refused with rc=1)"

tamper_build() {
  case "$1" in
    appended_duplicate)
      cat "$BASE_LOCK"
      printf 'os.cpu=appended-duplicate\n'
      ;;
    reordered)
      # Swap the first two key lines; the validator compares the whole sequence.
      awk '/^schema\.golden_ref=/{a=$0; getline b; print b; print a; next} {print}' \
        "$BASE_LOCK"
      ;;
    unknown_key)
      awk '/^schema\.golden_ref=/{print; print "unknown.key=value"; next} {print}' \
        "$BASE_LOCK"
      ;;
    missing_key)
      grep -v '^font\.egui=' "$BASE_LOCK"
      ;;
    malformed_line)
      awk '/^schema\.golden_ref=/{print; print "this line has no equals sign"; next} \
        {print}' "$BASE_LOCK"
      ;;
    trailing_whitespace)
      # Canonical as a KEY SEQUENCE, wrong as a VALUE: must be caught by the
      # value comparison, not reported as a format defect.
      sed 's|^schema\.golden_ref=1$|schema.golden_ref=1 |' "$BASE_LOCK"
      ;;
    bad_key_characters)
      awk '/^schema\.golden_ref=/{print; print "bad key=x"; next} {print}' \
        "$BASE_LOCK"
      ;;
    empty_key)
      awk '/^schema\.golden_ref=/{print; print "=orphan-value"; next} {print}' \
        "$BASE_LOCK"
      ;;
    *) return 1 ;;
  esac
}

for t in appended_duplicate reordered unknown_key missing_key malformed_line \
  trailing_whitespace bad_key_characters empty_key; do
  tfile="$SANDBOX/locks/tampered.lock"
  if ! tamper_build "$t" >"$tfile"; then
    no "tamper/$t" "no builder for this variant"
    continue
  fi
  if cmp -s "$tfile" "$BASE_LOCK"; then
    no "tamper/$t" "the variant is byte-identical to the base lock"
    continue
  fi
  gr_lock "$tfile" check
  chk_rc 1 "tamper/$t refused"
  case "$t" in
    trailing_whitespace)
      # Structural check passes, so the refusal must come from the value diff
      # and must name exactly the one key.
      chk_lacks "is not in canonical form" "tamper/$t refused by value, not format"
      chk_has "  schema.golden_ref:" "tamper/$t names the one differing key"
      ;;
    *)
      chk_has "is not in canonical form" "tamper/$t refused as a format defect"
      ;;
  esac
done

# --- legitimate lock forms --------------------------------------------------

section "legitimate lock forms (all must be accepted with rc=0)"

form_build() {
  case "$1" in
    as_recorded) cat "$BASE_LOCK" ;;
    keys_only) grep -v '^[[:space:]]*#' "$BASE_LOCK" ;;
    extra_header)
      printf '# hand written header\n# second line\n'
      cat "$BASE_LOCK"
      ;;
    mid_block_comment)
      awk '/^font\.egui=/{print "# --- section marker ---"} {print}' "$BASE_LOCK"
      ;;
    blank_lines)
      awk '/^gpu\.metal=/{print ""; print ""} {print}' "$BASE_LOCK"
      ;;
    crlf)
      # A CRLF lock is a legitimate spelling of the same lock: `lock_value`
      # normalises a trailing CR on read, so this must behave exactly like the
      # LF form instead of reporting 21 differing keys.
      sed 's/$/\r/' "$BASE_LOCK"
      ;;
    *) return 1 ;;
  esac
}

for f in as_recorded keys_only extra_header mid_block_comment blank_lines crlf; do
  ff="$SANDBOX/locks/form.lock"
  form_build "$f" >"$ff"
  gr_lock "$ff" check
  chk_rc 0 "form/$f accepted"
  chk_lacks "is not in canonical form" "form/$f is not a format defect"
  chk_lacks "differences:" "form/$f reports no value difference"
done

# A value that legitimately contains a space AND an '=' sign. LUMINA_GPU_BACKENDS
# is recorded verbatim, so it is the one key that can carry both. Recorded and
# checked with the SAME env, which is the documented contract (§9.6).
ENV_LOCK="$SANDBOX/locks/env.lock"
# A direct prefix assignment, not `gr_env`: the value contains a space, which
# the deliberately word-splitting `env` helper could not pass as one argument.
OUT=$(LUMINA_GPU_BACKENDS='Vulkan x=y' GOLDEN_REF_LOCK="$ENV_LOCK" \
  sh "$SCRIPT" record --confirm "$BASE_REASON" 2>&1) && RC=0 || RC=$?
chk_rc 0 "form/value-with-space-and-equals recorded"
if grep -q '^wgpu.backend=Vulkan x=y$' "$ENV_LOCK" 2>/dev/null; then
  ok "form/value-with-space-and-equals: the raw value really was written verbatim"
else
  no "form/value-with-space-and-equals: the raw value really was written verbatim" \
     "lock content:
$(cat "$ENV_LOCK" 2>/dev/null)"
fi
OUT=$(LUMINA_GPU_BACKENDS='Vulkan x=y' GOLDEN_REF_LOCK="$ENV_LOCK" \
  sh "$SCRIPT" check 2>&1) && RC=0 || RC=$?
chk_rc 0 "form/value-with-space-and-equals accepted by check"
# ...and it must not be accepted without that env (it pins an explicit backend).
gr_lock "$ENV_LOCK" check
chk_rc 1 "form/value-with-space-and-equals rejected once the env is gone"

# --- record refusals --------------------------------------------------------

section "record refusals (refused writes must exit 2 and touch nothing)"

# rec <label> <lockfile> <record args...>: sets $RC and $OUT.
rec() {
  rec_l=$2
  shift 2
  OUT=$(GOLDEN_REF_LOCK="$rec_l" sh "$SCRIPT" record "$@" 2>&1) && RC=0 || RC=$?
}

# Exactly 19 and exactly 20 characters (measured, not guessed): the documented
# boundary is "at least 20 characters".
R19='guard test reason 1'
R20='guard test reason 19'

refuse() {
  # refuse <label> <lockfile> <record args...>: a refused `record` must exit 2,
  # explain itself, and leave no lock file behind at all (no partial write).
  rf_lbl=$1
  rf_lf=$2
  shift 2
  rm -f "$rf_lf"
  rec "$rf_lbl" "$rf_lf" "$@"
  chk_rc 2 "refusal/$rf_lbl"
  chk_has "ERROR:" "refusal/$rf_lbl explains itself on stderr"
  if [ -e "$rf_lf" ]; then
    no "refusal/$rf_lbl left no lock file behind" "a refused record created $rf_lf"
  else
    ok "refusal/$rf_lbl left no lock file behind"
  fi
}

refuse no-confirm "$SANDBOX/locks/r1.lock"
refuse confirm-without-value "$SANDBOX/locks/r2.lock" --confirm
refuse empty-reason "$SANDBOX/locks/r3.lock" --confirm ''
refuse reason-19-chars "$SANDBOX/locks/r4.lock" --confirm "$R19"
refuse reason-with-LF "$SANDBOX/locks/r5.lock" --confirm "${R20}${NL}tail"
refuse reason-with-CR "$SANDBOX/locks/r6.lock" --confirm "${R20}${CR}tail"
refuse reason-with-CRLF "$SANDBOX/locks/r7.lock" --confirm "${R20}${CR}${NL}tail"
refuse reason-with-double-dash "$SANDBOX/locks/r8.lock" \
  --confirm 'guard test reason with -- in it'
refuse unexpected-argument "$SANDBOX/locks/r9.lock" --force
# The forms the spec says are fine. 19 vs 20 chars is the boundary, and a reason
# WITH spaces is the normal case, not a refusal.
wfile="$SANDBOX/locks/r10.lock"
rm -f "$wfile"
rec reason-20-chars "$wfile" --confirm "$R20"
chk_rc 0 "accept/reason-20-chars"
chk_has "recorded $wfile" "accept/reason-20-chars wrote the lock"
chk_has "old -> new diff" "accept/reason-20-chars printed the old -> new diff first"
chk_has "# Grund: $R20" "accept/reason-20-chars wrote the reason verbatim"
gr_lock "$wfile" check
chk_rc 0 "accept/reason-20-chars produces a lock that passes check"

# A reason that is exactly at the length boundary and contains spaces proves the
# spec sentence "Leerzeichen sind erlaubt" (a reason with spaces is accepted).
wfile2="$SANDBOX/locks/r11.lock"
rm -f "$wfile2"
rec reason-with-spaces "$wfile2" --confirm 'a reason of several words'
chk_rc 0 "accept/reason-with-spaces (a reason MAY contain spaces)"
gr_lock "$wfile2" check
chk_rc 0 "accept/reason-with-spaces produces a lock that passes check"

# Re-recording an existing lock with a different reason is legitimate and must
# keep the lock canonical (this is the normal re-pin path).
gr_lock "$BASE_LOCK" record --confirm "$OTHER_REASON"
chk_rc 0 "accept/re-record with a different reason"
gr check
chk_rc 0 "accept/re-record keeps the lock acceptable to check"
gr_lock "$BASE_LOCK" record --confirm "$BASE_REASON" >/dev/null 2>&1
detail "base lock restored for the remaining sections"

# --- the non-default lock warning, for all four subcommands -----------------

section "the GOLDEN_REF_LOCK override warns on every subcommand"
gr print
chk_has "WARNING: using non-default lock file" "warn/print"
gr check
chk_has "WARNING: using non-default lock file" "warn/check"
gr_lock "$BASE_LOCK" record --confirm "$BASE_REASON"
chk_has "WARNING: using non-default lock file" "warn/record"
gr gate -- true
chk_has "WARNING: using non-default lock file" "warn/gate"
chk_rc 0 "gate runs the command on a matching pin"

# --- UPDATE_SNAPSHOTS truthiness -------------------------------------------

section "UPDATE_SNAPSHOTS truthiness on a mismatching lock"
# A lock that cannot match on any machine, so the refusal path is reached
# regardless of the platform the suite runs on.
MIS_LOCK="$SANDBOX/locks/mismatch.lock"
sed "s|^os\.name=.*|os.name=deliberately-not-this-machine|" "$BASE_LOCK" >"$MIS_LOCK"
gr_lock "$MIS_LOCK" check
chk_rc 1 "the mismatching lock really mismatches"

MARKER="$SANDBOX/gate_marker"
for v in '' 0 false no off FALSE No OFF; do
  rm -f "$MARKER"
  gr_env "$MIS_LOCK" "UPDATE_SNAPSHOTS=$v" -- gate -- sh -c "touch '$MARKER'"
  label="update/falsy[$v]"
  if [ "$RC" -eq 1 ] && [ ! -f "$MARKER" ]; then
    ok "$label -> rc=1 and the gated command did not run"
  else
    no "$label -> rc=1 and the gated command did not run" \
       "rc=$RC marker=$([ -f "$MARKER" ] && echo present || echo absent)
--- output ---
$OUT"
  fi
  chk_lacks "REFUSING: UPDATE_SNAPSHOTS is set" "$label is silent (no UPDATE refusal)"
done

for v in 1 true yes on force garbage; do
  rm -f "$MARKER"
  gr_env "$MIS_LOCK" "UPDATE_SNAPSHOTS=$v" -- gate -- sh -c "touch '$MARKER'"
  label="update/truthy[$v]"
  if [ "$RC" -eq 1 ] && [ ! -f "$MARKER" ]; then
    ok "$label -> rc=1 and the gated command did not run"
  else
    no "$label -> rc=1 and the gated command did not run" \
       "rc=$RC marker=$([ -f "$MARKER" ] && echo present || echo absent)
--- output ---
$OUT"
  fi
  chk_has "REFUSING: UPDATE_SNAPSHOTS is set" "$label fires the UPDATE refusal"
  chk_has "no golden was written and no test was started" "$label says it wrote nothing"
done

# On the PINNED platform a truthy UPDATE_SNAPSHOTS must be allowed through: the
# documented contract is "beabsichtigt auf der Referenzplattform", not a blanket
# block (§5).
rm -f "$MARKER"
gr_env "$BASE_LOCK" "UPDATE_SNAPSHOTS=1" -- gate -- sh -c "touch '$MARKER'"
chk_rc 0 "update/truthy on a matching pin does not block the gated command"
if [ -f "$MARKER" ]; then
  ok "update/truthy on a matching pin really executed the command"
else
  no "update/truthy on a matching pin really executed the command" \
     "--- output ---
$OUT"
fi
chk_lacks "REFUSING: UPDATE_SNAPSHOTS is set" \
  "update/truthy on a matching pin prints no refusal"

# --- the committed lock and the real index ---------------------------------

section "the committed scripts/golden_ref.lock"
if [ -f "$REAL_LOCK" ]; then
  ok "the reference lock is committed/present"
  # Read-only: `check` with the default lock path. The VALUE comparison is
  # expected to fail on any machine that is not the reference platform (that is
  # the point of the pin), but the FORMAT check must pass everywhere, and a
  # non-canonical committed lock is a real defect on every platform.
  OUT=$(sh "$SCRIPT" check 2>&1) && RC=0 || RC=$?
  chk_lacks "is not in canonical form" "the committed lock is in canonical form"
  case "$OUT" in
    *"differences:"*) ok "a value mismatch on a foreign platform is reported as such (rc=$RC)" ;;
    *)
      if [ "$RC" -eq 0 ]; then
        ok "this machine IS the reference platform (check rc=0)"
      else
        no "check against the committed lock failed for an unexpected reason" \
           "rc=$RC
--- output ---
$OUT"
      fi
      ;;
  esac
else
  no "the reference lock is committed/present" "missing: $REAL_LOCK"
fi

# --- pre-commit gate matrix ------------------------------------------------

section "pre-commit gate matrix (throwaway git repo, real hook, sandbox only)"
REPO="$SANDBOX/repo"
mkdir -p "$REPO"
git -C "$REPO" init -q
git -C "$REPO" config user.name "guard test"
git -C "$REPO" config user.email "guard@example.invalid"
git -C "$REPO" config commit.gpgsign false
git -C "$REPO" config core.hooksPath "$HOOKS_DIR"

# Every row starts from the SAME tree (the seed commit), not from the moving
# HEAD: rows that commit a deletion or a rename would otherwise change what the
# next row's setup function sees. This keeps the matrix order-independent.
mc_reset() {
  git -C "$REPO" reset -q --hard "$SEED_SHA" >/dev/null 2>&1
  git -C "$REPO" clean -qfdx >/dev/null 2>&1
}
mc_lock() {
  # mc_lock <full text of the lock file to stage>
  printf '%s\n' "$1" >"$REPO/$LOCK_REL"
  git -C "$REPO" add -- "$LOCK_REL"
}
mc_repin() {
  # A re-pin: a new written reason in the staged lock.
  mc_lock "# Grund: $1"
}
# The base PNG content is irrelevant: the gate reads PATHS, never pixels. It must
# still be a REVISION counter, so that every mc_png call really changes the bytes
# - otherwise `git add` stages nothing and the case would assert nothing.
mc_seq=0
mc_png() {
  mc_seq=$((mc_seq + 1))
  printf 'png stub for the guard test, revision %s\n' "$mc_seq" >"$REPO/$1"
}

if git -C "$REPO" config core.hooksPath "$HOOKS_DIR"; then
  ok "sandbox repo uses the real .githooks as its hooksPath"
else
  no "sandbox repo uses the real .githooks as its hooksPath"
fi

# Seed commit: one golden + a lock with a reason line. The golden must be
# STAGED here, otherwise it stays untracked, `git clean` in mc_reset deletes it
# (and its directory), and every matrix row then runs against a missing file.
mkdir -p "$REPO/$(dirname "$GOLDENS_REL")" "$REPO/$(dirname "$LOCK_REL")"
mc_png "$GOLDENS_REL"
git -C "$REPO" add -- "$GOLDENS_REL"
mc_lock "# Grund: seed commit for the guard test matrix"
if git -C "$REPO" commit -q -m "seed" >/dev/null 2>&1 &&
  git -C "$REPO" cat-file -e "HEAD:$GOLDENS_REL" 2>/dev/null; then
  SEED_SHA=$(git -C "$REPO" rev-parse HEAD)
  ok "seed commit created and the golden is tracked in HEAD ($SEED_SHA)"
else
  no "seed commit created and the golden is tracked in HEAD" \
     "the sandbox repo is not in the expected state; every matrix row would be void"
  exit 1
fi

# mc_commit <label> <want-rc> <staged-setup-fn> [refusing?]
mc_commit() {
  mc_lbl=$1
  mc_want=$2
  mc_setup=$3
  mc_reset
  "$mc_setup"
  # Precondition: something must actually be staged. `git commit` exits 1 with
  # "nothing to commit" when the index is empty, which would make a want-rc=1
  # case pass for a completely unrelated reason.
  if git -C "$REPO" diff --cached --quiet; then
    no "hook/$mc_lbl precondition" \
       "nothing was staged, so this case would pass for the wrong reason"
    return 0
  fi
  OUT=$(git -C "$REPO" commit -q -m "$mc_lbl" 2>&1) && RC=0 || RC=$?
  if [ "$RC" -eq "$mc_want" ]; then
    ok "hook/$mc_lbl -> rc=$RC"
  else
    no "hook/$mc_lbl -> rc=$RC" "expected rc=$mc_want
--- hook output ---
$OUT"
  fi
}

set_golden_modified() {
  mc_png "$GOLDENS_REL"
  git -C "$REPO" add -- "$GOLDENS_REL"
}
set_golden_and_repin() {
  set_golden_modified
  mc_repin "a fresh justification for a changed golden in the guard test"
}
set_golden_no_lock() { set_golden_modified; }
set_golden_lock_unchanged_grund() {
  set_golden_modified
  # The lock IS staged, but only a comment is appended - the `# Grund:` line
  # itself is byte-identical to HEAD, which is exactly what the gate must catch.
  printf '# appended comment, reason untouched\n' >>"$REPO/$LOCK_REL"
  git -C "$REPO" add -- "$LOCK_REL"
}
set_golden_lock_no_grund() {
  set_golden_modified
  mc_lock "# no reason line at all in this staged lock"
}
set_golden_deleted_no_lock() {
  git -C "$REPO" rm -q -- "$GOLDENS_REL"
}
set_golden_deleted_repin() {
  git -C "$REPO" rm -q -- "$GOLDENS_REL"
  mc_repin "a golden was deliberately removed in the guard test"
}
set_golden_renamed_repin() {
  git -C "$REPO" mv "$GOLDENS_REL" 'crates/lumina-gui/tests/snapshots/renamed.png'
  mc_repin "a golden was deliberately renamed in the guard test"
}
set_golden_renamed_no_lock() {
  git -C "$REPO" mv "$GOLDENS_REL" 'crates/lumina-gui/tests/snapshots/renamed.png'
}
set_new_golden_subdir_repin() {
  mkdir -p "$REPO/crates/lumina-gui/tests/snapshots/sub"
  mc_png 'crates/lumina-gui/tests/snapshots/sub/added.png'
  git -C "$REPO" add -- 'crates/lumina-gui/tests/snapshots/sub/added.png'
  mc_repin "a golden was added in a subdirectory in the guard test"
}
set_new_golden_subdir_no_lock() {
  mkdir -p "$REPO/crates/lumina-gui/tests/snapshots/sub"
  mc_png 'crates/lumina-gui/tests/snapshots/sub/added.png'
  git -C "$REPO" add -- 'crates/lumina-gui/tests/snapshots/sub/added.png'
}
set_index_differs_from_worktree() {
  # Stage a changed golden, then put the ORIGINAL bytes back in the working
  # tree. `git diff --cached` must still see the change (the gate judges the
  # index, not the working copy); a worktree-based gate would see nothing.
  set_golden_modified
  git -C "$REPO" show "HEAD:$GOLDENS_REL" >"$REPO/$GOLDENS_REL" 2>/dev/null
}
# The LOCK side of that same clause, the mirror image of the row above. The
# staged lock keeps the `# Grund:` line it has in HEAD (only a comment is
# appended, so the lock IS staged and its reason is byte-identical), while the
# WORKING COPY is given a different reason that is never staged. Judged on the
# index, the reason is unchanged and the commit must be refused. A gate that
# read the working copy would accept a commit that carries no new written
# justification at all - the attack this row exists to close.
set_lock_index_differs_from_worktree() {
  set_golden_lock_unchanged_grund
  sed 's|^# Grund: .*|# Grund: the working copy claims this different reason|' \
    "$REPO/$LOCK_REL" >"$REPO/$LOCK_REL.worktree-differs" &&
    mv "$REPO/$LOCK_REL.worktree-differs" "$REPO/$LOCK_REL"
}
set_lock_only() {
  mc_repin "only the lock was re-pinned, no golden changed in the guard test"
}
set_non_golden_png() {
  # A PNG that is NOT under tests/snapshots must not trip the gate. The
  # fixtures/ dir does not exist in the seed repo, so create it first.
  mkdir -p "$REPO/crates/lumina-gui/tests/fixtures"
  mc_png 'crates/lumina-gui/tests/fixtures/not_a_golden.png'
  git -C "$REPO" add -- 'crates/lumina-gui/tests/fixtures/not_a_golden.png'
}

# The matrix. Golden-related rows without a re-pin must be REFUSED (rc=1);
# golden-related rows WITH a fresh `# Grund:` must pass (rc=0); a golden outside
# the snapshots dir and a lock-only change must pass (rc=0).
mc_commit "golden-modified-no-lock"        1 set_golden_no_lock
mc_commit "golden-modified-lock-unchanged"  1 set_golden_lock_unchanged_grund
mc_commit "golden-modified-no-grund-line"  1 set_golden_lock_no_grund
mc_commit "golden-modified-with-repin"     0 set_golden_and_repin
mc_commit "golden-deleted-no-lock"         1 set_golden_deleted_no_lock
mc_commit "golden-deleted-with-repin"      0 set_golden_deleted_repin
mc_commit "golden-renamed-no-lock"         1 set_golden_renamed_no_lock
mc_commit "golden-renamed-with-repin"      0 set_golden_renamed_repin
mc_commit "golden-new-in-subdir-no-lock"   1 set_new_golden_subdir_no_lock
mc_commit "golden-new-in-subdir-with-repin" 0 set_new_golden_subdir_repin
mc_commit "index-differs-from-worktree"    1 set_index_differs_from_worktree
mc_commit "lock-index-differs-from-worktree" 1 set_lock_index_differs_from_worktree
mc_commit "lock-only-no-golden"            0 set_lock_only
mc_commit "non-golden-png-no-lock"         0 set_non_golden_png

# Nothing staged at all must pass (the empty-diff path).
mc_reset
OUT=$(git -C "$REPO" commit -q --allow-empty -m "nothing staged" 2>&1) && RC=0 || RC=$?
chk_rc 0 "hook/nothing-staged -> rc=0"
chk_lacks "golden change detected" "hook/nothing-staged does not report a golden change"

# The refusals must actually be refusals, with the reason visible.
mc_reset
set_golden_no_lock
OUT=$(git -C "$REPO" commit -q -m "should refuse" 2>&1) && RC=0 || RC=$?
chk_rc 1 "hook/refusal exits 1 when the lock is missing"
chk_has "is not staged in the same commit" "hook/refusal names the missing-lock reason"

mc_reset
set_golden_lock_unchanged_grund
OUT=$(git -C "$REPO" commit -q -m "should refuse" 2>&1) && RC=0 || RC=$?
chk_rc 1 "hook/refusal exits 1 when the reason is unchanged"
chk_has "is unchanged" "hook/refusal names the unchanged-reason reason"

# The index/worktree row must refuse for THAT cause and not for some incidental
# one ("lock is not staged", a broken index, a missing reason line) - those
# would make the matrix row above pass for the wrong reason. The staged lock is
# staged, has a `# Grund:` line, and that line is the one from HEAD.
mc_reset
set_lock_index_differs_from_worktree
OUT=$(git -C "$REPO" commit -q -m "the index carries no new reason" 2>&1) && RC=0 || RC=$?
gr_cause=other
case "$OUT" in
  *"is unchanged"*) gr_cause=unchanged-reason ;;
esac
chk_true "$([ "$RC" -eq 1 ] && [ "$gr_cause" = unchanged-reason ] && echo 0 || echo 1)" \
  "hook/lock-index-differs-from-worktree refuses because the STAGED reason is the HEAD reason" \
  "rc=$RC, refusal cause: $gr_cause
--- hook output ---
$OUT"
mc_reset

# Fail-closed: if `git diff --cached` itself fails, the gate must NOT read that
# as "nothing staged" and let the commit through.
#
# A broken index is the WRONG way to test this: `git commit` refuses a commit
# with a missing index on its own, so such a case passes whether or not the
# hook has a fail-closed branch at all - it would prove nothing about the hook.
# Instead, inject the fault where it belongs: a `git` shim on PATH that fails
# ONLY for `diff --cached` and passes everything else through. `git commit`
# itself then works (its diff is built in, it does not shell out), the hook runs
# for real, and the hook's own `git diff --cached` is the thing that fails.
REALGIT=$(command -v git)
SHIM="$SANDBOX/shim"
mkdir -p "$SHIM"
{
  echo '#!/bin/sh'
  echo '# Test shim: fail "git diff --cached", pass every other invocation on.'
  # This echo block GENERATES a script; the single quotes below are deliberate
  # so that $a stays literal in the generated file.
  # shellcheck disable=SC2016
  echo 'for a in "$@"; do'
  # shellcheck disable=SC2016
  echo '  if [ "$a" = "--cached" ]; then'
  echo '    echo "fatal: simulated index failure" >&2'
  echo '    exit 128'
  echo '  fi'
  echo 'done'
  echo "exec \"$REALGIT\" \"\$@\""
} >"$SHIM/git"
chmod +x "$SHIM/git"

# Positive control first: the very same staged state must PASS with a real git,
# so the refusal below can only come from the injected failure.
mc_reset
set_golden_and_repin
OUT=$(cd "$REPO" && sh "$HOOK" 2>&1) && RC=0 || RC=$?
if [ "$RC" -eq 0 ]; then
  ok "hook/fail-closed control: the same state passes with a working git"
else
  no "hook/fail-closed control: the same state passes with a working git" \
     "rc=$RC - the control state is wrong, the fail-closed case below would prove nothing
--- hook output ---
$OUT"
fi

# The hook is invoked directly (from the repository root, exactly as git invokes
# it) because git PREPENDS its own git-core directory to the hook's PATH, so a
# PATH shim cannot shadow git's binary inside a real `git commit`. Calling the
# hook script itself is the unit boundary where "what does the hook do when its
# git call fails" is actually decided.
mc_reset
set_golden_and_repin
# SC2031: the shim PATH is meant to be local to this command substitution - it
# must reach the hook and nothing after it. shellcheck infers "a PATH modified in
# a subshell" from the gi_probe subshell above, not from this line.
# shellcheck disable=SC2031
OUT=$(cd "$REPO" && PATH="$SHIM:$PATH" sh "$HOOK" 2>&1) && RC=0 || RC=$?
chk_rc 1 "hook/git-diff-failure fails closed"
chk_has "fails closed" "hook/git-diff-failure explains that it fails closed"
chk_has "simulated index failure" "hook/git-diff-failure names the real git error"
mc_reset

# --- the golden fixture contract: the table the digest cannot see ------------

section "golden fixture contract (GOLDEN-FIXT-31: the table the digest cannot see)"

# WHAT this section is for. `feature/quality/golden-fixtures.md` §4 states the
# gap from its own side: the R/S1/S2 classification table has one row per
# committed golden, and "`golden_ref.sh check` vergleicht Digests, nicht diese
# Tabelle - eine fehlende Zeile fällt dort nicht auf". The acceptance criterion
# of GOLDEN-FIXT-31 that had no implementation anywhere (verification finding
# H3, measured as zero hits for `golden-fixtures` in every `.rs`, `scripts/*`
# and `.github/*` file) is the other half of the same sentence: "ein Test
# schlägt an, wenn eine Chrome-Invariante als Beleg für eine
# Bildpipeline-Regression herangezogen wird".
#
# The digest layer above cannot see either half, and not by accident: a
# `record` fills the lock from the same code `check` reads, so a golden nobody
# ever classified still hashes happily; and the classification table is prose
# that no code read. So everything below is derived HERE, from the committed
# files and from the document, with plain `git`, `find`, `awk` and `sed` - never
# through the guard's own `list_goldens` / `list_fixtures`. Comparing the guard
# with a list that came out of the same functions would only prove that the
# guard agrees with itself, which is the failure mode this section exists to
# remove.
#
# Two properties, deliberately kept apart:
#   A  completeness - the inventory table and the golden directory describe the
#      SAME set of files in BOTH directions, and the numbers the document claims
#      in prose are the numbers its table contains.
#   B  class discipline - a class-R row is backed by real rendered pixels, the
#      forbidden class token R-F does not occur, and every render-evidence
#      citation names a class-R row.
#
# B2 IS RED on the untouched tree, on purpose. Six committed class-R goldens
# still carry the LibRaw error banner and flat colour blocks as their expected
# state (golden-fixtures.md §5.6, verification finding H1). That is the finding
# this section exists to make visible; re-recording those goldens is
# GOLDEN-BASELINE-32's task. Lowering a threshold until the six pass would hide
# exactly the thing the check is for, so the thresholds are the ones the
# document derives from the measured populations and are left alone.
GFC_DOC="$ROOT/feature/quality/golden-fixtures.md"
GFC_SNAP_REL='crates/lumina-gui/tests/snapshots'
GFC_PROBE_REL='scripts/fixtures/png_pixel_probe.png'
GFC_PROBE="$ROOT/$GFC_PROBE_REL"

# The two class-R pixel thresholds, from golden-fixtures.md §2 rule 7. P1 is a
# floor on distinct colours, P2 a ceiling on the share of strongly-red pixels
# (r > 0.5, g < 0.25, b < 0.25). §5.6 of that document carries the measurement
# both numbers come from: the two populations sit at 570..1516 (error state) and
# 88426..104756 (healthy), and 10000 is the rounded geometric mean of the two
# extremes (11578) - a maximum-margin placement, not a number chosen to make
# today's set pass. It does not.
GFC_MIN_COLOURS=10000
GFC_MAX_RED=0.02

# Literal expectations for the committed measurement probe, counted by hand from
# the pixel list in `scripts/fixtures/README.md` and never derived from the tool
# under test (DoD §10, no self-referential expectation).
GFC_PROBE_COLOURS=4
GFC_PROBE_RED=0.375

gfc_is_uint() {
  # A non-empty run of digits and nothing else.
  case "${1:-}" in
    '' | *[!0-9]*) return 1 ;;
  esac
  return 0
}

gfc_is_fraction() {
  # A decimal in [0,1] as ImageMagick's %[fx:mean] prints it. Rejects the empty
  # string, a colour name, an error message, two dots and anything above 1, so
  # a broken tool cannot feed a plausible-looking number into P2.
  case "${1:-}" in
    '' | *[!0-9.]*) return 1 ;;
  esac
  case "${1#*.}" in
    *.*) return 1 ;;
  esac
  case "${1%%.*}" in
    0) case "$1" in 0 | 0.*) return 0 ;; esac ;;
    1) case "$1" in 1 | 1.0 | 1.00*) return 0 ;; esac ;;
  esac
  return 1
}

gfc_dec_gt() {
  # True when decimal $1 is greater than $2. POSIX sh has no floating point.
  awk -v a="$1" -v b="$2" 'BEGIN { exit !(a + 0 > b + 0) }'
}

gfc_fx_tool() {
  # The command that can evaluate -fx, or nothing. ImageMagick 7 ships `magick`,
  # ImageMagick 6 `convert`; both ship `identify`.
  if command -v magick >/dev/null 2>&1; then
    echo magick
  elif command -v convert >/dev/null 2>&1; then
    echo convert
  fi
}

gfc_info_tool() {
  if command -v identify >/dev/null 2>&1; then
    echo identify
  fi
}

gfc_measure() {
  # Sets GFC_COLOURS and GFC_RED, or returns 1 and leaves them unusable. The
  # caller MUST read a non-zero return as "no measurement", never as a zero -
  # that distinction is the whole point of the precondition below.
  gfc_m_fx=$(gfc_fx_tool)
  gfc_m_info=$(gfc_info_tool)
  GFC_MISSING_TOOLS=
  if [ -z "$gfc_m_fx" ] || [ -z "$gfc_m_info" ]; then
    GFC_MISSING_TOOLS="fx='$gfc_m_fx' identify='$gfc_m_info'"
    return 1
  fi
  GFC_COLOURS=$("$gfc_m_info" -format '%k' "$1" 2>/dev/null) || return 1
  GFC_RED=$("$gfc_m_fx" "$1" -alpha off \
    -fx 'u.r>0.5 && u.g<0.25 && u.b<0.25 ? 1 : 0' \
    -format '%[fx:mean]' info: 2>/dev/null) || return 1
  gfc_is_uint "$GFC_COLOURS" || return 1
  gfc_is_fraction "$GFC_RED" || return 1
  return 0
}

gfc_inventory_rows() {
  # "<golden> <class>" for every row of the two inventory tables. Only rows
  # whose FIRST cell is a number qualify, which excludes the header row, the
  # `| --- |` separator and the unrelated tables elsewhere in the document. The
  # class cell is taken verbatim apart from `*` and whitespace, so the forbidden
  # token R-F stays distinguishable from R - collapsing them here would make B1
  # unreachable by construction.
  awk -F'|' '
    $0 ~ /^[[:space:]]*\|[[:space:]]*[0-9]+[[:space:]]*\|/ {
      n = $3; c = $4
      gsub(/`/, "", n); gsub(/[[:space:]]/, "", n)
      gsub(/\*/, "", c); gsub(/[[:space:]]/, "", c)
      if (n != "") print n " " c
    }
  ' "$GFC_DOC"
}

gfc_ledger_rows() {
  # The goldens named in the render-evidence ledger (§4.4), read as the rows
  # between the `### 4.4` and `### 4.5` headings whose first cell is a
  # backticked name. The ledger deliberately carries NO class column; the class
  # is looked up in the inventory, so the two lists cannot drift apart.
  sed -n '/^### 4\.4 /,/^### 4\.5 /p' "$GFC_DOC" |
    awk -F'|' '$0 ~ /^[[:space:]]*\|[[:space:]]*`/ {
      n = $2
      gsub(/`/, "", n); gsub(/[[:space:]]/, "", n)
      if (n != "") print n
    }'
}

gfc_doc_int() {
  # The first number of the first line matching the ERE in $1, or nothing. Used
  # for the three numbers the document states in prose, so a claim in the text
  # is compared against the table instead of being taken on trust.
  #
  # awk, not a `sed 's/.*\(...\)/\1/p'` backreference, and that is a measured
  # choice: on this machine (BSD sed) the backreference form returns
  # "66 (gezählt mit" for the very line it is supposed to read, while the awk
  # form returns "66". A gate that misreads its own document on one of the two
  # platforms it runs on is worse than no gate, so the portable tool wins.
  # The patterns are EREs, and they are written with bracket expressions
  # ([*][*]) rather than `\*\*`: escaping an ordinary character is undefined in
  # POSIX ERE, and BSD awk rejects `\*\*` outright with "illegal primary". Both
  # spellings work with gawk, so this is exactly the kind of difference a gate
  # that only ever runs on the author's machine never meets.
  awk -v re="$1" '
    match($0, re) {
      seg = substr($0, RSTART, RLENGTH)
      if (match(seg, /[0-9]+/)) { print substr(seg, RSTART, RLENGTH); exit }
    }
  ' "$GFC_DOC"
}

gfc_chk_num() {
  # gfc_chk_num <label> <claim-from-the-document> <measured>
  gfc_n_lbl=$1
  gfc_n_claim=$2
  gfc_n_meas=$3
  if [ -z "$gfc_n_claim" ]; then
    no "$gfc_n_lbl" "the number could not be found in $GFC_DOC at all.
A count that cannot be read is not a count, so this stays red rather than
comparing an empty string with '$gfc_n_meas'."
    return 1
  fi
  if [ "$gfc_n_claim" = "$gfc_n_meas" ]; then
    ok "$gfc_n_lbl ($gfc_n_meas)"
    return 0
  fi
  no "$gfc_n_lbl" "the document claims $gfc_n_claim, the table contains $gfc_n_meas"
  return 1
}

gfc_chk_both_ways() {
  # gfc_chk_both_ways <label> <set-a> <set-b> <what-a> <what-b>
  # Equality as a SET, reported per name in both directions. The existing
  # chk_inventory helper does the same job for the guard's inventory, but its
  # message says "the guard enumerated"; these two sides are a directory listing
  # and a markdown table, so they get wording that names the right things
  # rather than a misleading one.
  gfc_b_lbl=$1
  gfc_b_a=$2
  gfc_b_b=$3
  gfc_b_an=$4
  gfc_b_bn=$5
  if [ "$gfc_b_a" = "$gfc_b_b" ]; then
    ok "$gfc_b_lbl ($(printf '%s\n' "$gfc_b_a" | grep -c . || true) names)"
    return 0
  fi
  gfc_b_af="$SANDBOX/gfc_side_a.txt"
  gfc_b_bf="$SANDBOX/gfc_side_b.txt"
  printf '%s\n' "$gfc_b_a" | LC_ALL=C sort >"$gfc_b_af"
  printf '%s\n' "$gfc_b_b" | LC_ALL=C sort >"$gfc_b_bf"
  gfc_b_onlya=$(LC_ALL=C comm -23 "$gfc_b_af" "$gfc_b_bf")
  gfc_b_onlyb=$(LC_ALL=C comm -13 "$gfc_b_af" "$gfc_b_bf")
  no "$gfc_b_lbl" "in $gfc_b_an but not in $gfc_b_bn: $(printf '%s\n' "$gfc_b_onlya" | grep -c . || true)
$(printf '%s\n' "$gfc_b_onlya" | head -n 5)
in $gfc_b_bn but not in $gfc_b_an: $(printf '%s\n' "$gfc_b_onlyb" | grep -c . || true)
$(printf '%s\n' "$gfc_b_onlyb" | head -n 5)"
  return 1
}

gfc_lean_path() {
  # A synthetic PATH that deliberately has NO image tool, while still carrying
  # the tools the inventory checks need - i.e. what a plain runner without
  # ImageMagick looks like.
  #
  # Why a synthetic bin and not "this machine's PATH minus every directory that
  # provides an image tool": on Ubuntu the runner installs ImageMagick 6, whose
  # `convert` and `identify` live in /usr/bin - the SAME directory as git, sed
  # and awk. Dropping that directory to remove ImageMagick removes every
  # coreutils binary too, so the assertion below fails for the wrong reason
  # (measured: CI job "Documentation checks", run 36265383058, on main@f2ed6da).
  # "Directory without image tools" is simply not the same statement as "PATH
  # without image tools" when the two sets share a directory. Strip-by-directory
  # therefore cannot express what is required and is not used.
  #
  # Building the bin from `command -v` makes the split by TOOL rather than by
  # directory. It is keyed to no install prefix and to no platform, and it is
  # deliberately NOT a filter: every required tool is resolved first and a
  # missing one is reported by name. An accidentally incomplete bin would make
  # the "git/sed/awk reachable" assertion fail (loud), and an accidentally
  # complete one would make the "tool absent" assertion pass vacuously - so the
  # unresolved-tool case must be loud, not an omission.
  #
  # Output contract: `ok <path>` with the bin directory, or `missing <names>`
  # with the unresolved tools. It never prints an empty path on error, so the
  # precondition below cannot turn the result into a vacuous empty PATH.
  #
  # If a required tool is absent on this machine:
  #   * sh/git/sed/awk absent -> "missing <name>"; the lean PATH cannot be built,
  #     the precondition below is red, and the "git/sed/awk reachable" assertion
  #     is red too. It never reads as "the tool is absent from the lean PATH".
  #   * an image tool (magick/convert/identify) absent -> it is NOT copied into
  #     the bin (there is nothing to copy), and the "tool absent" assertion is
  #     still red for exactly that tool - by construction, not by luck. The other
  #     image tools are still probed, so the assertion cannot pass while any of
  #     the three is resolvable.
  #
  # Never touches $PATH itself and never runs in a subshell: it only reads $PATH
  # via `command -v`, so the SC2031 disable the previous version needed for
  # reading $PATH no longer applies and is gone.
  gfc_lp_bin=$GFC_LEAN_BIN
  gfc_lp_ok=1
  gfc_lp_missing=
  # The tools `env PATH=<lean> sh -c` needs to start at all (`env` and the `sh`
  # it execs) plus the ones the assertions and the inventory checks probe for.
  for gfc_lp_t in env sh git sed awk; do
    gfc_lp_p=$(command -v "$gfc_lp_t" 2>/dev/null) || gfc_lp_p=
    case "$gfc_lp_p" in
      /*) [ -x "$gfc_lp_p" ] || gfc_lp_p= ;;
      *) gfc_lp_p= ;;
    esac
    if [ -z "$gfc_lp_p" ]; then
      gfc_lp_ok=0
      gfc_lp_missing="$gfc_lp_missing $gfc_lp_t"
      continue
    fi
    ln -sf "$gfc_lp_p" "$gfc_lp_bin/$gfc_lp_t"
  done
  # The image tools are deliberately NOT copied in: the whole point of the bin
  # is that it resolves env/sh/git/sed/awk and NO image tool. An earlier draft
  # symlinked magick/convert/identify here "when resolvable", which put exactly
  # the tools the assertion checks for back on the lean PATH and made the two
  # assertions below fail (measured locally, 2 red). The vacuity concern - "the
  # bin lacks ImageMagick because the machine lacks it, not because we stripped
  # it" - is already covered by the precondition above the section, which is red
  # when no image tool exists at all. That one assertion cannot be satisfied
  # vacuously here.
  if [ "$gfc_lp_ok" -eq 0 ]; then
    printf 'missing%s' "$gfc_lp_missing"
    return 1
  fi
  printf '%s' "$gfc_lp_bin"
  return 0
}

# --- preconditions ---------------------------------------------------------

chk_true "$([ -f "$GFC_DOC" ] && echo 0 || echo 1)" \
  "precondition: the fixture contract document exists" \
  "every check below reads $GFC_DOC. Without it they would compare empty lists
with empty lists and report success without having read a single golden."

GFC_ROWS=$(gfc_inventory_rows)
GFC_ROW_N=$(printf '%s\n' "$GFC_ROWS" | grep -c . || true)
chk_true "$([ "$GFC_ROW_N" -gt 0 ] && echo 0 || echo 1)" \
  "precondition: the inventory table parsed to at least one row ($GFC_ROW_N rows)" \
  "no line of $GFC_DOC matched the shape '| <number> | \`<golden>\` | <class> |'.
If the table's shape changed, A1/A2/A3/B1/B2 would all compare empty lists and
pass without covering a single golden."

GFC_TABLE_NAMES=$(printf '%s\n' "$GFC_ROWS" | awk '{print $1}' | LC_ALL=C sort)
GFC_DISK=$(find "$ROOT/$GFC_SNAP_REL" -maxdepth 1 -type f -name '*.png' |
  awk '!/\.new\.png$/ && !/\.diff\.png$/ && !/\.old\.png$/' |
  sed 's|.*/||' | LC_ALL=C sort)
GFC_DISK_N=$(printf '%s\n' "$GFC_DISK" | grep -c . || true)
chk_true "$([ "$GFC_DISK_N" -gt 0 ] && echo 0 || echo 1)" \
  "precondition: the snapshot directory holds at least one golden ($GFC_DISK_N files)" \
  "find $ROOT/$GFC_SNAP_REL -name '*.png' returned nothing, so A1/A2 would
compare an empty side against the table and 'pass' without covering a golden."

# --- A: completeness -------------------------------------------------------

# A1 and A2 in one comparison, because they are the two directions of the same
# statement: a golden with no row, and a row with no golden. `golden_ref.sh
# check` compares digests and sees neither.
gfc_chk_both_ways \
  "A1+A2: the inventory table and the snapshot directory name the same goldens, both ways" \
  "$GFC_DISK" "$GFC_TABLE_NAMES" "the snapshot directory" "the inventory table"

# A3: the three numbers the document states in prose must be the numbers its
# table contains. This is what stops a future edit from bumping "66" in one
# sentence and leaving the table at 65.
GFC_CLAIM_TOTAL=$(gfc_doc_int '[*][*][0-9]+[*][*] committete Golden-Dateien')
GFC_CLAIM_R=$(gfc_doc_int 'Bilanz: [0-9]+ Render-Invarianten')
GFC_CLAIM_C=$(gfc_doc_int '[0-9]+ Chrome-/Layout-Invarianten')
GFC_TAB_R=$(printf '%s\n' "$GFC_ROWS" | awk '$2 == "R"' | grep -c . || true)
GFC_TAB_C=$(printf '%s\n' "$GFC_ROWS" | awk '$2 == "C"' | grep -c . || true)
gfc_chk_num "A3: the document's golden total matches its table" \
  "$GFC_CLAIM_TOTAL" "$GFC_ROW_N"
gfc_chk_num "A3: the document's class-R total matches its table" \
  "$GFC_CLAIM_R" "$GFC_TAB_R"
gfc_chk_num "A3: the document's class-C total matches its table" \
  "$GFC_CLAIM_C" "$GFC_TAB_C"

# A4: the directory and the committed set agree, so "committed" is a fact rather
# than a claim - and so an untracked golden dropped into the directory is caught
# by A1 even though `git ls-files` cannot see it yet.
GFC_COMMITTED=$(git -C "$ROOT" ls-files --cached -- "$GFC_SNAP_REL/*.png" |
  awk '!/\.new\.png$/ && !/\.diff\.png$/ && !/\.old\.png$/' |
  sed 's|.*/||' | LC_ALL=C sort)
gfc_chk_both_ways \
  "A4: the snapshot directory matches the committed golden set" \
  "$GFC_DISK" "$GFC_COMMITTED" "the snapshot directory" "git ls-files"

# The class vocabulary, checked before B1 and B2 depend on it: a misspelt token
# would silently remove the row from the pixel check below, and a check that
# silently stops covering a golden is worse than no check.
GFC_ODD_CLASS=$(printf '%s\n' "$GFC_ROWS" |
  awk '$2 != "R" && $2 != "C" && $2 != "R-F" { print $1 " -> " $2 }')
if [ -z "$GFC_ODD_CLASS" ]; then
  ok "every inventory row carries a class token out of {R, C, R-F}"
else
  no "every inventory row carries a class token out of {R, C, R-F}" \
    "a token outside the vocabulary would drop that row out of the B2 pixel
check without any red assertion - that is the silent-coverage failure mode:
$GFC_ODD_CLASS"
fi

# --- B: class discipline ---------------------------------------------------

# B1: the forbidden token. R-F exists so the table can NAME the state that
# GOLDEN-BASELINE-32 has to fix; it may not be used as a classification.
GFC_RF=$(printf '%s\n' "$GFC_ROWS" | awk '$2 == "R-F" { print $1 }')
if [ -z "$GFC_RF" ]; then
  ok "B1: no inventory row is classified R-F (a render invariant whose expected state is a decode failure)"
else
  no "B1: no inventory row is classified R-F (a render invariant whose expected state is a decode failure)" \
    "these rows declare a decode failure as the expected state of a render
invariant, which golden-fixtures.md §2 rule 5 forbids for class R:
$(printf '%s' "$GFC_RF" | tr '\n' ' ')"
  # Two-sided: R-F must not become a way to smuggle a healthy golden past B2, so
  # each R-F row also has to actually show the failure in its pixels.
  for gfc_rf_name in $GFC_RF; do
    gfc_rf_path="$ROOT/$GFC_SNAP_REL/$gfc_rf_name"
    if [ ! -f "$gfc_rf_path" ]; then
      no "B1: the R-F row $gfc_rf_name is corroborated by its pixels" \
        "there is no file at $gfc_rf_path, so the claim is unverifiable"
      continue
    fi
    if gfc_measure "$gfc_rf_path"; then
      gfc_rf_seen=
      [ "$GFC_COLOURS" -lt "$GFC_MIN_COLOURS" ] && gfc_rf_seen="P1"
      if gfc_dec_gt "$GFC_RED" "$GFC_MAX_RED"; then
        gfc_rf_seen="$gfc_rf_seen P2"
      fi
      if [ -z "$gfc_rf_seen" ]; then
        no "B1: the R-F row $gfc_rf_name is corroborated by its pixels" \
          "measured distinct_colours=$GFC_COLOURS (>= $GFC_MIN_COLOURS) and
red_fraction=$GFC_RED (<= $GFC_MAX_RED): the pixels show a HEALTHY render, so
'R-F' is being used to exempt a golden from the pixel check rather than to
record a real finding."
      else
        ok "B1: the R-F row $gfc_rf_name is corroborated by its pixels (failed: $gfc_rf_seen)"
      fi
    else
      no "B1: the R-F row $gfc_rf_name is corroborated by its pixels" \
        "no usable measurement ($GFC_MISSING_TOOLS); the claim stays unverified"
    fi
  done
fi

# B2: the pixel proof for every class-R row. This is the check that turns
# "the six library goldens still show the LibRaw banner" from a thing somebody
# noticed into a red assertion with numbers attached.
GFC_R_NAMES=$(printf '%s\n' "$GFC_ROWS" | awk '$2 == "R" { print $1 }')
chk_true "$([ -n "$GFC_R_NAMES" ] && echo 0 || echo 1)" \
  "precondition: the table lists at least one class-R golden" \
  "with no class-R row the pixel check would pass over an empty loop."

GFC_FX=$(gfc_fx_tool)
GFC_INFO=$(gfc_info_tool)
chk_true "$([ -n "$GFC_FX" ] && [ -n "$GFC_INFO" ] && echo 0 || echo 1)" \
  "precondition: an image measuring tool is on PATH (fx='$GFC_FX', identify='$GFC_INFO')" \
  "B2 measures committed PNGs with ImageMagick, and that is an external
dependency, so it is asserted instead of assumed (DoD §10: no environment
assumption in a test). The correct behaviour without the tool is RED, never a
silent skip - a skipped check is a check that cannot fail.
Install it (macOS: brew install imagemagick, Debian/Ubuntu: apt-get install
imagemagick) or run the suite where it exists."

# The tool is validated against a committed probe whose pixels are countable by
# hand. Without this, a tool that answers every question plausibly but wrongly
# would turn B2 into a random number generator.
if gfc_measure "$GFC_PROBE"; then
  chk_true "$([ "$GFC_COLOURS" = "$GFC_PROBE_COLOURS" ] && echo 0 || echo 1)" \
    "precondition: the tool counts the committed probe's distinct colours correctly ($GFC_PROBE_COLOURS)" \
    "$GFC_PROBE was measured as distinct_colours=$GFC_COLOURS, but the literal
pixel list in scripts/fixtures/README.md has exactly $GFC_PROBE_COLOURS colours
(red, green, blue, black). A wrong count here means the P1 threshold is being
compared against a number from an unverified derivation."
  chk_true "$(awk -v a="$GFC_RED" -v b="$GFC_PROBE_RED" 'BEGIN { print (a + 0 == b + 0) ? 0 : 1 }')" \
    "precondition: the tool computes the committed probe's red fraction correctly ($GFC_PROBE_RED)" \
    "$GFC_PROBE was measured as red_fraction=$GFC_RED, but the literal pixel
list gives 3 of 8 pixels = $GFC_PROBE_RED. A tool that answered 0.0 (nothing is
red) or 1.0 (everything is red) would equally invalidate the P2 threshold."
else
  no "precondition: the tool counts the committed probe's distinct colours correctly ($GFC_PROBE_COLOURS)" \
    "the committed probe $GFC_PROBE_REL could not be measured ($GFC_MISSING_TOOLS)"
  no "precondition: the tool computes the committed probe's red fraction correctly ($GFC_PROBE_RED)" \
    "the committed probe $GFC_PROBE_REL could not be measured ($GFC_MISSING_TOOLS)"
fi

# --- the known-red list -----------------------------------------------------
#
# The exact names of the class-R goldens that are KNOWN to fail the pixel check
# today, and the task that owns fixing them. This is not an exemption; it is
# the difference between a finding and a permanently red gate. Without it the
# suite is red on every run, a red `main` gets ignored as noise, and the six
# broken goldens stop being news. The only ways to get a green suite WITHOUT
# fixing the pixels are all forbidden by this repository: lower
# GFC_MIN_COLOURS/GFC_MAX_RED, re-classify the rows as R-F, or delete the rows
# from the inventory table and the render-evidence ledger. All three make the
# finding invisible rather than fix it.
#
# What replaces those six unconditional failures is one bounded invariant:
#
#   every class-R golden either carries real render evidence, or its EXACT
#   name is on this list, and while it is on this list it still fails.
#
# The four ways that invariant goes red, each with its own assertion below:
#   * UNKNOWN failure - a class-R golden fails B2 and is NOT on this list. A
#     seventh broken golden can never be absorbed here.
#   * STALE entry - a name on this list now PASSES B2. The entry has to go,
#     and that is precisely the signal that GOLDEN-BASELINE-32 has landed.
#     This is what stops the list from becoming a permanent exemption: an entry
#     that outlives its defect is itself the failure.
#   * UNMEASURABLE entry - a name on this list cannot be measured at all (the
#     tool is gone, the file is gone). Not a stale entry: a different defect
#     with a different fix, and therefore a different assertion.
#   * MALFORMED entry - a name on this list is not a class-R row of the
#     inventory, or occurs twice, or is empty. A typo would otherwise be an
#     exemption that can never fire while still looking declared. These two
#     check the DECLARATION rather than the pixels.
#
# The finding these six carry: the committed goldens still show the LibRaw
# error banner (finding H1 of GOLDEN-FIXT-31, measured in
# feature/quality/golden-fixtures.md §5.5 and §5.6). Re-recording them is
# GOLDEN-BASELINE-32's one-shot task, and nothing in this file may pre-empt it:
# no threshold is lowered, no golden is rewritten, no inventory or ledger row
# is removed. Every listed golden keeps printing its measured numbers, here and
# in the recap at the end of this section - a named exception that hides its
# subject would defeat the purpose.
#
# The list lives HERE and nowhere else. golden-fixtures.md §5.5 names the same
# six in prose as the specification's account of the known limitation; that is
# not a second copy a gate reads, so there is nothing that can drift. This
# variable is the single place the check consults.
GFC_KNOWN_RED="library_compare.png
library_loupe.png
library_rated_badges.png
library_subfolder_badges.png
library_survey.png
library_stack_membership.png"
GFC_KNOWN_RED_FILE="$SANDBOX/gfc_known_red.txt"
printf '%s\n' "$GFC_KNOWN_RED" >"$GFC_KNOWN_RED_FILE"
# Non-empty entries, and the two shapes a malformed list can take. Both are
# counted before the loop so the assertions below can name a number.
GFC_LIST_N=$(grep -c . "$GFC_KNOWN_RED_FILE" || true)
GFC_LIST_BLANK=$(grep -c '^[[:space:]]*$' "$GFC_KNOWN_RED_FILE" || true)
GFC_LIST_DUP=$(LC_ALL=C sort "$GFC_KNOWN_RED_FILE" | LC_ALL=C uniq -d | tr '\n' ' ')
GFC_R_N=$(printf '%s\n' "$GFC_R_NAMES" | grep -c . || true)

printf '\n== B2: class-R pixel evidence; %s of %s class-R rows are NAMED known-red, owned by GOLDEN-BASELINE-32\n' \
  "$GFC_LIST_N" "$GFC_R_N"

# Per-row verdicts, collected for the set-level assertions that follow the loop.
# One pass over the rows cannot express "every unlisted row passes AND every
# listed row still fails", which is why the judgement is split: the loop keeps
# the per-row detail (with the measured numbers), the aggregates below own the
# invariant.
GFC_B2_UNKNOWN=
GFC_B2_STALE=
GFC_B2_LISTED_MEASURED=0
GFC_B2_LISTED_RED=0
GFC_B2_REPORT=

gfc_is_known_red() {
  # gfc_is_known_red <exact golden name>. Whole-LINE exact match, so no name
  # can be exempted by a prefix, a substring, a glob or a case difference - the
  # failure mode a `case $name in *library_*` test would have.
  grep -Fxq -- "$1" "$GFC_KNOWN_RED_FILE"
}

# Word splitting on purpose: golden file names contain no whitespace, and a
# `while read` pipeline would run the loop in a subshell where the pass/fail
# counters of ok/no are lost - the assertions would print and count nothing.
for gfc_r_name in $GFC_R_NAMES; do
  gfc_r_path="$ROOT/$GFC_SNAP_REL/$gfc_r_name"
  if [ ! -f "$gfc_r_path" ]; then
    gfc_is_known_red "$gfc_r_name" || GFC_B2_UNKNOWN="$GFC_B2_UNKNOWN$gfc_r_name "
    no "B2: $gfc_r_name carries real render evidence" \
      "the table classifies it as R but there is no file at $gfc_r_path"
    continue
  fi
  if ! gfc_measure "$gfc_r_path"; then
    gfc_is_known_red "$gfc_r_name" || GFC_B2_UNKNOWN="$GFC_B2_UNKNOWN$gfc_r_name "
    no "B2: $gfc_r_name carries real render evidence" \
      "no usable measurement ($GFC_MISSING_TOOLS). Either the tool is absent -
see the precondition above - or it returned something that is not a colour
count and a fraction in [0,1]."
    continue
  fi
  gfc_r_bad=
  [ "$GFC_COLOURS" -lt "$GFC_MIN_COLOURS" ] && gfc_r_bad="P1"
  if gfc_dec_gt "$GFC_RED" "$GFC_MAX_RED"; then
    gfc_r_bad="$gfc_r_bad P2"
  fi
  # Measured once, then counted as MEASURED - not as red. A listed row that
  # passes B2 has been measured just as much as one that fails, and conflating
  # the two would let the aggregate below report a stale entry as an unmeasurable
  # one, which is a different defect with a different fix.
  gfc_r_known=no
  if gfc_is_known_red "$gfc_r_name"; then
    gfc_r_known=yes
    GFC_B2_LISTED_MEASURED=$((GFC_B2_LISTED_MEASURED + 1))
  fi
  if [ -z "$gfc_r_bad" ]; then
    if [ "$gfc_r_known" = yes ]; then
      GFC_B2_STALE="$GFC_B2_STALE$gfc_r_name "
      no "B2: $gfc_r_name carries real render evidence" \
        "STALE ENTRY in the B2 known-red list. This golden is on the list of
class-R rows that are known to lack render evidence, and it PASSES the pixel
check right now:
  measured distinct_colours=$GFC_COLOURS   (>= $GFC_MIN_COLOURS)
  measured red_fraction=$GFC_RED           (<= $GFC_MAX_RED)
The list exists to NAME a defect that is still there. This one is not, so the
entry has to be removed - that removal is what the landing of
GOLDEN-BASELINE-32 looks like from here. Keeping it would turn the list into a
permanent exemption, which is the one outcome it must not become."
    else
      ok "B2: $gfc_r_name carries real render evidence (colours=$GFC_COLOURS >= $GFC_MIN_COLOURS, red=$GFC_RED <= $GFC_MAX_RED)"
    fi
  else
    if [ "$gfc_r_known" = yes ]; then
      GFC_B2_LISTED_RED=$((GFC_B2_LISTED_RED + 1))
      GFC_B2_REPORT="$GFC_B2_REPORT  $gfc_r_name: distinct_colours=$GFC_COLOURS (P1 needs >= $GFC_MIN_COLOURS), red_fraction=$GFC_RED (P2 needs <= $GFC_MAX_RED), failed $gfc_r_bad$NL"
      ok "B2: $gfc_r_name is a NAMED known-red class-R row owned by GOLDEN-BASELINE-32 - the render-evidence finding is OPEN, not fixed (failed $gfc_r_bad, colours=$GFC_COLOURS, red=$GFC_RED)"
    else
      GFC_B2_UNKNOWN="$GFC_B2_UNKNOWN$gfc_r_name "
      no "B2: $gfc_r_name carries real render evidence" \
        "failed predicate(s): $gfc_r_bad
  measured distinct_colours=$GFC_COLOURS   P1 requires >= $GFC_MIN_COLOURS
  measured red_fraction=$GFC_RED           P2 requires <= $GFC_MAX_RED
A class-R golden may not take a decode failure or an error placeholder as its
expected state (golden-fixtures.md §2 rules 5 and 7). Re-recording it is
GOLDEN-BASELINE-32's task; this threshold must not be lowered to hide it.
AND it is not one of the rows that task owns, so this is an UNKNOWN failure: a
class-R golden that lost its render evidence since the last re-record. The
known-red list is not a place to absorb it - see the B2-allowlist assertion
below."
    fi
  fi
done

# --- the B2-allowlist invariant, as five set-level assertions ---------------
#
# A1-A4, B1 and B3 are all statements about SETS, and so is this one. Judged
# row by row, a list can hide a failure; judged as a set, the states below are
# the only ones the gate distinguishes, and every one of them but the first is
# red.

# (1) UNKNOWN failure. The list is closed: nothing joins it by failing.
GFC_LISTED_N=$GFC_LIST_N
GFC_UNLISTED_N=$((GFC_R_N - GFC_LISTED_N))
if [ -z "$GFC_B2_UNKNOWN" ]; then
  ok "B2-allowlist: every class-R golden OUTSIDE the known-red list carries render evidence (unknown failures 0 of $GFC_UNLISTED_N unlisted rows)"
else
  no "B2-allowlist: every class-R golden OUTSIDE the known-red list carries render evidence" \
    "UNKNOWN failure - these class-R rows are not on the known-red list and do
not carry render evidence:
$GFC_B2_UNKNOWN  The per-row assertion above names the failed predicate and the
measured numbers for each of them.
This is a new regression, not the tracked finding H1, and the list must not
become the place where it disappears. Either the golden is re-recorded, or -
if it genuinely shows the known LibRaw banner - its exact name is added to
GFC_KNOWN_RED above, next to the task that owns it."
fi

# (2) STALE entry. The list is self-limiting: it can only shrink, and only by
# this assertion going red. An entry that no longer describes a defect is the
# signal that GOLDEN-BASELINE-32 has landed.
if [ -z "$GFC_B2_STALE" ]; then
  ok "B2-allowlist: no entry of the known-red list has gone stale ($GFC_B2_LISTED_RED of $GFC_LIST_N entries still red, none passing)"
else
  no "B2-allowlist: no entry of the known-red list has gone stale" \
    "STALE entry - listed as known-red but measurably PASSING the pixel check,
so the entry has to be deleted from GFC_KNOWN_RED:
$GFC_B2_STALE  An entry that outlives its defect is a permanent exemption, and
removing it is the intended reaction to GOLDEN-BASELINE-32 re-recording the
golden. The measured numbers are in the per-row assertion above."
fi

# (2b) Measurable. Kept apart from (2) on purpose: a listed row that cannot be
# measured is a missing tool or a broken repository, a DIFFERENT defect with a
# DIFFERENT fix, and folding it into the stale message would report it as
# something it is not.
if [ "$GFC_B2_LISTED_MEASURED" -eq "$GFC_LIST_N" ]; then
  ok "B2-allowlist: every entry of the known-red list could be measured ($GFC_B2_LISTED_MEASURED of $GFC_LIST_N)"
else
  no "B2-allowlist: every entry of the known-red list could be measured" \
    "$((GFC_LIST_N - GFC_B2_LISTED_MEASURED)) of $GFC_LIST_N entries of the known-red list could not be
measured at all, so their entry cannot be confirmed as still describing a
defect. The per-row assertions above report why - usually the image tool is
missing (see the tool precondition), or the file is gone. This is NOT a stale
entry: fixing it means restoring the measurement, not deleting the name."
fi

# (3) MALFORMED entry. Every declared name has to be a class-R row of the
# inventory, and no name may appear twice. A typo would otherwise be an
# exemption that can never fire while still reading as a declaration.
GFC_LIST_NOT_R=
for gfc_ln in $GFC_KNOWN_RED; do
  gfc_ln_class=$(printf '%s\n' "$GFC_ROWS" | awk -v n="$gfc_ln" '$1 == n { print $2 }')
  if [ "$gfc_ln_class" != "R" ]; then
    GFC_LIST_NOT_R="$GFC_LIST_NOT_R  $gfc_ln -> class [$gfc_ln_class] (no such row if empty)$NL"
  fi
done
if [ -z "$GFC_LIST_NOT_R" ]; then
  ok "B2-allowlist: every known-red entry names a class-R row of the inventory (all $GFC_LIST_N entries)"
else
  no "B2-allowlist: every known-red entry names a class-R row of the inventory" \
    "an entry that is not a class-R row of the inventory can never be checked by
the loop above, so it would be an exemption that never fires while still
looking like a declaration:
$GFC_LIST_NOT_R  Fix the spelling, or delete the entry if the golden is gone."
fi

# The third shape a list can take: a duplicated or empty entry.
if [ -z "$GFC_LIST_DUP" ] && [ "$GFC_LIST_BLANK" -eq 0 ]; then
  ok "B2-allowlist: the known-red list has no duplicated and no empty entry ($GFC_LIST_N entries)"
else
  no "B2-allowlist: the known-red list has no duplicated and no empty entry" \
    "duplicated entries: ${GFC_LIST_DUP:-none}
empty entries: $GFC_LIST_BLANK of $((GFC_LIST_N + GFC_LIST_BLANK)) lines
A duplicate counts the same golden twice, which makes the set-level counts
above lie about how many rows are actually covered; an empty entry is an
exemption of nothing at all."
fi

# --- the six known-red findings, spelled out with their numbers -------------
#
# The gate is green because the finding is named, bounded and owned - not
# because it is gone. This block is the reason: it prints every listed golden
# that is still red, with the measurement, every run, in the open. If this
# section ever comes out empty while entries are still on the list, the
# assertions above have already said so.
printf '\n== B2 known-red findings, tracked by GOLDEN-BASELINE-32 (still open, not fixed)\n'
if [ -n "$GFC_B2_REPORT" ]; then
  printf '%s' "$GFC_B2_REPORT"
  printf '  -> %s of %s class-R rows lack render evidence and are named above; the re-record belongs to GOLDEN-BASELINE-32.\n' \
    "$GFC_B2_LISTED_RED" "$GFC_LIST_N"
else
  printf '  (none: no listed golden is red right now - if entries are still on the list, the stale-entry assertion above is red)\n'
fi

# B3: the H3 criterion itself. A render-evidence citation has to name a row the
# inventory classifies R. A class-C row here is exactly "a Chrome invariant
# cited as evidence for an image-pipeline regression".
GFC_LEDGER=$(gfc_ledger_rows)
GFC_LEDGER_N=$(printf '%s\n' "$GFC_LEDGER" | grep -c . || true)
chk_true "$([ "$GFC_LEDGER_N" -gt 0 ] && echo 0 || echo 1)" \
  "precondition: the render-evidence ledger parsed to at least one citation ($GFC_LEDGER_N)" \
  "the ledger is the rows between '### 4.4' and '### 4.5' whose first cell is a
backticked name. Without a citation the H3 criterion would have nothing to
fire on, and an empty loop would report success."
# Word splitting on purpose: golden file names contain no whitespace, and a
# `while read` pipeline would run the loop in a subshell where the pass/fail
# counters of ok/no are lost - the assertions would print and count nothing.
for gfc_cite in $GFC_LEDGER; do
  gfc_cite_class=$(printf '%s\n' "$GFC_ROWS" | awk -v n="$gfc_cite" '$1 == n { print $2 }')
  if [ -z "$gfc_cite_class" ]; then
    no "B3: the render-evidence citation $gfc_cite names a class-R golden" \
      "it is not a row of the inventory table at all - a citation with no
classification cannot be checked for anything"
  elif [ "$gfc_cite_class" = "R" ]; then
    ok "B3: the render-evidence citation $gfc_cite names a class-R golden"
  else
    no "B3: the render-evidence citation $gfc_cite names a class-R golden" \
      "the inventory table classifies it as '$gfc_cite_class', and a
Chrome-/Layout-Invariante may never be cited as evidence for an
image-pipeline regression (golden-fixtures.md §3, §2 rule 9). This is
acceptance criterion H3 of GOLDEN-FIXT-31 firing."
  fi
done

# --- platform independence of this section ----------------------------------

# The inventory checks need env, sh, git, sed and awk and nothing else; the
# pixel check needs an image tool. Proving the second is detected as missing -
# rather than skipped - is what keeps the first claim honest on a runner that
# has no ImageMagick, which is the environment the CI job runs in.
GFC_LEAN_BIN="$SANDBOX/gfc_lean_path"
mkdir -p "$GFC_LEAN_BIN"
GFC_LEAN_PATH=$(gfc_lean_path) || GFC_LEAN_PATH=
chk_true "$([ -n "$GFC_LEAN_PATH" ] && echo 0 || echo 1)" \
  "precondition: a PATH without any image tool could be constructed" \
  "gfc_lean_path could not build its bin: it returned '$(gfc_lean_path)'.
Every tool the lean PATH needs (env, sh, git, sed, awk) must resolve to an
absolute executable path; the missing ones are named after 'missing'. This is
red rather than an empty PATH on purpose - an empty PATH would make the
'git/sed/awk reachable' assertion below fail for the wrong reason, hiding the
actual problem."

if env PATH="$GFC_LEAN_PATH" sh -c \
  'command -v magick >/dev/null 2>&1 || command -v convert >/dev/null 2>&1 || command -v identify >/dev/null 2>&1' 2>/dev/null; then
  no "the image-tool probe answers negative under a PATH without any image tool" \
    "magick, convert or identify is still reachable under PATH=$GFC_LEAN_PATH, so
the 'no tool' branch of gfc_measure was not exercised"
else
  ok "the image-tool probe answers negative under a PATH without any image tool"
fi

if env PATH="$GFC_LEAN_PATH" sh -c \
  'command -v git >/dev/null 2>&1 && command -v sed >/dev/null 2>&1 && command -v awk >/dev/null 2>&1' 2>/dev/null; then
  ok "git, sed and awk stay reachable under that PATH, so the inventory checks are platform independent"
else
  no "git, sed and awk stay reachable under that PATH, so the inventory checks are platform independent" \
    "PATH=$GFC_LEAN_PATH has no git/sed/awk, so A1-A4 could not run on a plain runner"
fi

# The two checks above ask the SHELL whether a tool is reachable. This one runs
# the production function itself, `gfc_measure`, and asserts the "no tool" branch
# is what actually answers - the difference between a probe that reports absence
# and a measurement that cannot silently invent a number.
if ( PATH="$GFC_LEAN_PATH"; gfc_measure "$GFC_PROBE" >/dev/null 2>&1 ); then
  no "gfc_measure itself reports 'no measurement' under a PATH without any image tool" \
    "gfc_measure returned success with no image tool on PATH=$GFC_LEAN_PATH, so
the branch B2 relies on for its refusal does not exist and a missing tool could
be read as a measurement of 0"
else
  ok "gfc_measure itself reports 'no measurement' under a PATH without any image tool"
fi

# Which major version CI actually gets. The `docs` job installs the runner's own
# `imagemagick`, whose apt set is expected to be ImageMagick 6 (`convert` +
# `identify`, no `magick`), while this machine has 7 (`magick`). gfc_fx_tool
# resolves both, so the suite runs on either - but only the IM6-shaped PATH is
# exercised by a real measurement, and that is the one CI will use. The shim
# below provides `convert`/`identify` and no `magick`, forwarding to the real
# binaries by ABSOLUTE path (the lean PATH has no image tool to forward to), so
# the IM6 branch is measured against the same committed probe literals instead of
# being assumed to work. What this proves is the RESOLUTION and the invocation
# shape; the arithmetic still comes from the installed binaries, which is why the
# CI step prints their version into the log rather than this file guessing it.
GFC_IM6_DIR="$SANDBOX/gfc_im6_path"
mkdir -p "$GFC_IM6_DIR"
gfc_tool_path() {
  # gfc_tool_path <command> -> the ABSOLUTE path it resolves to, or nothing.
  # Absolute is not a style preference here. A shim that exec'd the bare name
  # would exec ITSELF, because the shim directory is first on PATH - measured:
  # that version forked until the run was killed, because nothing about it
  # errors. A builtin or a function name, and any resolution that is not
  # absolute, are rejected for the same reason.
  gfc_tp=$(command -v "$1" 2>/dev/null) || return 1
  case "$gfc_tp" in
    /*) [ -x "$gfc_tp" ] || return 1 ;;
    *) return 1 ;;
  esac
  printf '%s' "$gfc_tp"
}
GFC_IM6_FX=$(gfc_tool_path "$(gfc_fx_tool)")
GFC_IM6_INFO=$(gfc_tool_path "$(gfc_info_tool)")
if [ -n "$GFC_IM6_FX" ] && [ -n "$GFC_IM6_INFO" ]; then
  printf '#!/bin/sh\nexec %s "$@"\n' "$GFC_IM6_FX" >"$GFC_IM6_DIR/convert"
  printf '#!/bin/sh\nexec %s "$@"\n' "$GFC_IM6_INFO" >"$GFC_IM6_DIR/identify"
  chmod +x "$GFC_IM6_DIR/convert" "$GFC_IM6_DIR/identify"
  # A subshell, because gfc_measure reports through globals and PATH must not
  # leak back into the rest of the suite.
  GFC_IM6_MEASURED=$(
    GFC_COLOURS=
    GFC_RED=
    PATH="$GFC_IM6_DIR:$GFC_LEAN_PATH"
    export PATH
    if gfc_measure "$GFC_PROBE" >/dev/null 2>&1; then
      printf '%s %s' "$GFC_COLOURS" "$GFC_RED"
    else
      printf 'unmeasurable'
    fi
  )
  if [ "$GFC_IM6_MEASURED" = "$GFC_PROBE_COLOURS $GFC_PROBE_RED" ]; then
    ok "the IM6-shaped tool path (convert+identify, no magick) resolves and measures the committed probe correctly ($GFC_PROBE_COLOURS / $GFC_PROBE_RED)"
  else
    no "the IM6-shaped tool path (convert+identify, no magick) resolves and measures the committed probe correctly ($GFC_PROBE_COLOURS / $GFC_PROBE_RED)" \
      "with only $GFC_IM6_DIR/convert and $GFC_IM6_DIR/identify on PATH, gfc_measure
returned: '$GFC_IM6_MEASURED'. This is the shape the docs CI job is expected to
get from the runner's own apt imagemagick package, and it would have been the
branch no measurement ever covered."
  fi
else
  no "the IM6-shaped tool path (convert+identify, no magick) resolves and measures the committed probe correctly ($GFC_PROBE_COLOURS / $GFC_PROBE_RED)" \
    "no image tool on this machine to build the shim from, or it does not resolve
to an absolute path (fx='$GFC_IM6_FX', identify='$GFC_IM6_INFO'), so the
resolution branch CI will use cannot be exercised here. The tool precondition
above is red for the same reason."
fi

# --- sandbox discipline ----------------------------------------------------

section "sandbox discipline (the real lock and index must be untouched)"
REAL_LOCK_SHA_AFTER=$(sha_of "$REAL_LOCK")
REAL_INDEX_AFTER=$(index_tree_oid "$ROOT")

# The after-side preconditions, mirroring the ones taken before the first case.
# They are what keeps the two comparisons below honest: if the after-capture
# itself degrades - a shim that is still on PATH, a sha helper that disappeared,
# a git that can no longer write a tree - the old code compared '' with '' and
# `no-index` with `no-index` and reported "unchanged" for a value it never read.
# With these two, the same situation is red.
#
# Kills: "scripts/golden_ref.lock is byte-identical after the suite" passing
# because the lock went MISSING during the run (two empty strings are equal).
chk_true "$(is_sha256 "$REAL_LOCK_SHA_AFTER" && echo 0 || echo 1)" \
  "precondition/after: the real lock's after-sha is a sha256 digest, not an empty string" \
  "sha_of $REAL_LOCK returned: '$REAL_LOCK_SHA_AFTER'
The comparison below would have reported the lock as byte-identical without ever
having read it."

# Kills: "the real git index is unchanged after the suite" passing on the
# `no-index` fallback constant.
chk_true "$(is_oid "$REAL_INDEX_AFTER" && echo 0 || echo 1)" \
  "precondition/after: the real index's after-OID is a tree object id, not the no-index fallback" \
  "git -C $ROOT write-tree returned: '$REAL_INDEX_AFTER'
The comparison below would have compared a fallback constant with itself (or
nothing with nothing) and passed for the wrong reason."

if [ "$REAL_LOCK_SHA_BEFORE" = "$REAL_LOCK_SHA_AFTER" ]; then
  ok "scripts/golden_ref.lock is byte-identical after the suite"
else
  no "scripts/golden_ref.lock is byte-identical after the suite" \
     "before=$REAL_LOCK_SHA_BEFORE
after=$REAL_LOCK_SHA_AFTER"
fi
# REAL_INDEX_AFTER was captured above, together with REAL_LOCK_SHA_AFTER and
# before the two preconditions. It is deliberately NOT captured a second time
# here: a second `git write-tree ... || echo "no-index"` would overwrite the
# helper's honest empty result with the fallback constant, and the comparison
# below would then compare '' with `no-index` - i.e. fail for a reason that has
# nothing to do with the index, while the real defect (git cannot read the
# index) would already be reported by the precondition above.
if [ "$REAL_INDEX_BEFORE" = "$REAL_INDEX_AFTER" ]; then
  ok "the real git index is unchanged after the suite"
else
  no "the real git index is unchanged after the suite" \
     "before=$REAL_INDEX_BEFORE
after=$REAL_INDEX_AFTER"
fi
# No temporary lock file may survive anywhere in the repo.
if ls "$ROOT"/scripts/*.tmp.* >/dev/null 2>&1; then
  no "no golden_ref.lock.tmp.* left in scripts/" "$(ls "$ROOT"/scripts/*.tmp.*)"
else
  ok "no golden_ref.lock.tmp.* left in scripts/"
fi

# --- summary ---------------------------------------------------------------

printf '\n== summary\n'
printf '  %d passed, %d failed\n' "$pass" "$fail"
if [ "$fail" -ne 0 ]; then
  printf '%s\n' "failed cases:$failed_labels"
  exit 1
fi
exit 0
