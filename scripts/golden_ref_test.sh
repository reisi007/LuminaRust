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
REAL_LOCK_SHA_BEFORE=$(sha_of "$REAL_LOCK")
REAL_INDEX_BEFORE=$(git -C "$ROOT" write-tree 2>/dev/null || echo "no-index")

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
OUT=$(cd "$REPO" && PATH="$SHIM:$PATH" sh "$HOOK" 2>&1) && RC=0 || RC=$?
chk_rc 1 "hook/git-diff-failure fails closed"
chk_has "fails closed" "hook/git-diff-failure explains that it fails closed"
chk_has "simulated index failure" "hook/git-diff-failure names the real git error"
mc_reset

# --- sandbox discipline ----------------------------------------------------

section "sandbox discipline (the real lock and index must be untouched)"
REAL_LOCK_SHA_AFTER=$(sha_of "$REAL_LOCK")
if [ "$REAL_LOCK_SHA_BEFORE" = "$REAL_LOCK_SHA_AFTER" ]; then
  ok "scripts/golden_ref.lock is byte-identical after the suite"
else
  no "scripts/golden_ref.lock is byte-identical after the suite" \
     "before=$REAL_LOCK_SHA_BEFORE
after=$REAL_LOCK_SHA_AFTER"
fi
REAL_INDEX_AFTER=$(git -C "$ROOT" write-tree 2>/dev/null || echo "no-index")
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
