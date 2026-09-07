#!/usr/bin/env bash
# Unit tests for scripts/reify-audit-freshness.sh — the shared freshness
# guard library that routes both the predone wrapper and the /audit skill
# through a single staleness check.
#
# Tests:
#   1-2: Script exists and is sourceable
#   3:   reify_audit_crate_commit_epoch prints a positive integer
#   4-7: reify_audit_is_stale: stale bin, fresh bin, missing bin, non-git repo
#   8-9: reify_audit_guard refuse-mode: stale exits non-zero with message,
#        fresh exits 0 silently
#  10:   reify_audit_guard rebuild-mode: fake cargo that touches bin → exit 0
#  11:   reify_audit_guard rebuild-mode: fake cargo that does NOT freshen → non-zero
#  12:   is_stale warns (stderr) when inside a git repo with no crates/reify-audit
#        history — fail-open (fresh) but not silent (likely renamed crate path)
#  13-15: reify_audit_guard rebuild-budget-safe mode (task #4624)
#  16:   reify_audit_guard warn-open mode: a present, executable, stale binary
#        FAILS OPEN (exit 0) with a loud, greppable alarm on stderr, and an
#        UNKNOWN mode string is reported (E_AUDIT_GUARD_BAD_MODE) and treated
#        as warn-open rather than falling through to a refusal (task #7139)
#  17:   warn-open still REFUSES (125) when there is nothing runnable to fall
#        open onto (absent, or present-but-not-executable), + a regression pin
#        that `refuse` mode is unchanged (task #7139)
#  18:   REIFY_AUDIT_FRESHNESS_STRICT=1 restores fail-closed behaviour under
#        warn-open — the operator's opt-in escape hatch (task #7139)
#  19:   the warn-open advisory AND refusal messages are SELF-DESCRIBING —
#        they name their own cause, their own fix, and what they are NOT
#        (task #7139)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FRESHNESS_LIB="$REPO_ROOT/scripts/reify-audit-freshness.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Temp dir for fake binaries — cleaned up on EXIT.
TMPDIR_FRESHNESS=$(mktemp -d /tmp/test-freshness-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS"' EXIT

echo "=== reify-audit-freshness.sh unit tests ==="

# ==============================================================================
# Check 1: freshness lib exists
# ==============================================================================
echo ""
echo "--- Check 1: reify-audit-freshness.sh exists ---"

assert "scripts/reify-audit-freshness.sh exists" \
    test -f "$FRESHNESS_LIB"

# ==============================================================================
# Check 2: freshness lib is sourceable
# ==============================================================================
echo ""
echo "--- Check 2: reify-audit-freshness.sh is sourceable ---"

assert "reify-audit-freshness.sh can be sourced without error" \
    bash -c "source '$FRESHNESS_LIB'"

# ==============================================================================
# Check 3: reify_audit_crate_commit_epoch prints a positive integer
# ==============================================================================
echo ""
echo "--- Check 3: reify_audit_crate_commit_epoch prints positive integer ---"

assert "reify_audit_crate_commit_epoch prints a positive integer epoch" \
    bash -c "source '$FRESHNESS_LIB' && epoch=\$(reify_audit_crate_commit_epoch '$REPO_ROOT') && [[ \"\$epoch\" =~ ^[0-9]+\$ ]] && [ \"\$epoch\" -gt 0 ]"

# ==============================================================================
# Check 4: reify_audit_is_stale — bin touched to old epoch → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 4: is_stale returns stale for an old binary ---"

# Create a fake binary touched to 2000-01-01 00:00 (epoch 946684800).
STALE_BIN="$TMPDIR_FRESHNESS/reify-audit-stale"
touch "$STALE_BIN"
touch -t 200001010000 "$STALE_BIN"

assert "is_stale returns stale (exit 0) for a binary older than crate commit" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '$STALE_BIN' '$REPO_ROOT'"

# ==============================================================================
# Check 5: reify_audit_is_stale — bin touched to now → fresh (exit 1)
# ==============================================================================
echo ""
echo "--- Check 5: is_stale returns fresh for a current binary ---"

FRESH_BIN="$TMPDIR_FRESHNESS/reify-audit-fresh"
touch "$FRESH_BIN"
# File was just created — mtime is now, which is after any historical commit.

assert "is_stale returns fresh (exit 1) for a binary newer than crate commit" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_is_stale '$FRESH_BIN' '$REPO_ROOT'"

# ==============================================================================
# Check 6: reify_audit_is_stale — missing bin → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 6: is_stale returns stale for a missing binary ---"

assert "is_stale returns stale (exit 0) for a missing binary path" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '/tmp/nonexistent-reify-audit-$$' '$REPO_ROOT'"

# ==============================================================================
# Check 7: reify_audit_is_stale — non-git repo_root → fail-open (fresh, exit 1)
# ==============================================================================
echo ""
echo "--- Check 7: is_stale fails open (fresh) for undeterminable epoch ---"

NON_GIT_DIR=$(mktemp -d /tmp/test-nongit-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR"' EXIT
touch "$NON_GIT_DIR/fake-bin"

assert "is_stale fails open (returns fresh/exit 1) when repo_root is not a git dir" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_is_stale '$NON_GIT_DIR/fake-bin' '$NON_GIT_DIR'"

# ==============================================================================
# Check 8: reify_audit_guard refuse — stale bin → exits non-zero + stderr message
# ==============================================================================
echo ""
echo "--- Check 8: guard refuse-mode exits non-zero and prints stale message ---"

assert "guard refuse-mode: stale binary exits non-zero" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_guard '$STALE_BIN' refuse '$REPO_ROOT' 2>/dev/null"

assert "guard refuse-mode: stale binary stderr contains 'stale'" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$STALE_BIN' refuse '$REPO_ROOT' 2>&1 | grep -qi 'stale'"

assert "guard refuse-mode: stale binary stderr contains 'cargo install'" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$STALE_BIN' refuse '$REPO_ROOT' 2>&1 | grep -q 'cargo install'"

# Exit code must be 125 specifically.
set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$STALE_BIN" refuse "$REPO_ROOT") 2>/dev/null
GUARD_EXIT=$?
set -e
assert "guard refuse-mode: stale binary exits with code 125" \
    bash -c "[ '$GUARD_EXIT' -eq 125 ]"

# ==============================================================================
# Check 9: reify_audit_guard refuse — fresh bin → exits 0 silently
# ==============================================================================
echo ""
echo "--- Check 9: guard refuse-mode exits 0 silently for a fresh binary ---"

assert "guard refuse-mode: fresh binary exits 0" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$FRESH_BIN' refuse '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 10: reify_audit_guard rebuild — fake cargo that freshens bin → exit 0
# ==============================================================================
echo ""
echo "--- Check 10: guard rebuild-mode succeeds when fake cargo freshens the bin ---"

# The rebuild branch calls `cargo build --release -q -p reify-audit` inside
# REPO_ROOT. We shim cargo with a script that touches the test bin (making
# it fresh), then exits 0. The guard should re-check is_stale and return 0.
REBUILD_TMPDIR=$(mktemp -d /tmp/test-rebuild-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR" "$REBUILD_TMPDIR"' EXIT

# Stale bin for rebuild test
REBUILD_BIN="$TMPDIR_FRESHNESS/reify-audit-for-rebuild"
touch "$REBUILD_BIN"
touch -t 200001010000 "$REBUILD_BIN"

# Fake cargo: touch the rebuild bin (making it fresh) and exit 0
FAKE_CARGO_TOUCH="$REBUILD_TMPDIR/cargo"
cat > "$FAKE_CARGO_TOUCH" <<EOF
#!/usr/bin/env bash
# Fake cargo for rebuild test — freshens REBUILD_BIN
touch '$REBUILD_BIN'
exit 0
EOF
chmod +x "$FAKE_CARGO_TOUCH"

assert "guard rebuild-mode: fake cargo that freshens bin → exit 0" \
    env PATH="$REBUILD_TMPDIR:$PATH" bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$REBUILD_BIN' rebuild '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 11: reify_audit_guard rebuild — fake cargo that does NOT freshen → non-zero
# ==============================================================================
echo ""
echo "--- Check 11: guard rebuild-mode fails when fake cargo does NOT freshen bin ---"

# Re-stale the bin
STUBBORN_BIN="$TMPDIR_FRESHNESS/reify-audit-stubborn"
touch "$STUBBORN_BIN"
touch -t 200001010000 "$STUBBORN_BIN"

# Fake cargo: exits 0 but never touches the bin
FAKE_CARGO_NOOP="$REBUILD_TMPDIR/cargo-noop"
cat > "$FAKE_CARGO_NOOP" <<'EOF'
#!/usr/bin/env bash
# Fake cargo for rebuild test — exits 0 but does NOT freshen the bin
exit 0
EOF
chmod +x "$FAKE_CARGO_NOOP"

# Swap in the no-op fake cargo (replace cargo symlink in REBUILD_TMPDIR)
mv "$FAKE_CARGO_TOUCH" "$REBUILD_TMPDIR/cargo-touch-bak"
cp "$FAKE_CARGO_NOOP" "$REBUILD_TMPDIR/cargo"
chmod +x "$REBUILD_TMPDIR/cargo"

assert "guard rebuild-mode: fake cargo that does NOT freshen bin → exits non-zero" \
    env PATH="$REBUILD_TMPDIR:$PATH" bash -c "source '$FRESHNESS_LIB' && ! reify_audit_guard '$STUBBORN_BIN' rebuild '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 12: reify_audit_is_stale — git repo with no crates/reify-audit history
#           → fail-open (fresh, exit 1) AND emits a stderr warning
#           This exercises the guard's renamed-crate-path detection (suggestion 3):
#           a non-git dir is legitimately silent; a git tree with no such history
#           is likely a misconfiguration and must warn so the silent disable is
#           visible.
# ==============================================================================
echo ""
echo "--- Check 12: is_stale warns when git repo has no crates/reify-audit history ---"

# Create a minimal git repo that has no crates/reify-audit history at all.
GIT_NO_HIST_DIR=$(mktemp -d /tmp/test-git-nohist-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR" "$REBUILD_TMPDIR" "$GIT_NO_HIST_DIR"' EXIT
git -C "$GIT_NO_HIST_DIR" init -q
touch "$GIT_NO_HIST_DIR/placeholder"
git -C "$GIT_NO_HIST_DIR" add placeholder
git -C "$GIT_NO_HIST_DIR" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
touch "$GIT_NO_HIST_DIR/fake-bin"

# 12a: Still returns fresh (fail-open) — guard must not block in this case.
assert "is_stale fails open (fresh/exit 1) in git repo with no crates/reify-audit history" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_is_stale '$GIT_NO_HIST_DIR/fake-bin' '$GIT_NO_HIST_DIR' 2>/dev/null"

# 12b: But emits a warning to stderr (not silent like the non-git case).
# Pattern: source freshness lib, then run is_stale with stderr→stdout, pipe to grep.
# Pipeline exit code = grep's exit code (0 if warning found).
assert "is_stale emits a stderr warning in git repo with no crates/reify-audit history" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '$GIT_NO_HIST_DIR/fake-bin' '$GIT_NO_HIST_DIR' 2>&1 | grep -qi 'crates/reify-audit'"

# 12c: Non-git dir (Check 7) is still silent — confirm no regression.
# Capture stderr from is_stale; a silent path leaves the var empty.
assert "is_stale is silent (no warning) for a non-git repo_root" \
    bash -c "source '$FRESHNESS_LIB'; warn=\$(reify_audit_is_stale '$NON_GIT_DIR/fake-bin' '$NON_GIT_DIR' 2>&1 >/dev/null); [ -z \"\$warn\" ]"

# ==============================================================================
# Check 13-15: reify_audit_guard rebuild-budget-safe mode (task #4624)
#   13: REIFY_AUDIT_NO_COLD_BUILD=1 + stale bin → exit 75, cargo NOT invoked
#   14: REIFY_AUDIT_NO_COLD_BUILD unset + stale bin + freshening shim → exit 0
#   15: Fresh bin → exit 0 regardless of REIFY_AUDIT_NO_COLD_BUILD
#
# These three checks FAIL until impl-freshness-budget-mode adds the new mode
# (unknown mode → guard returns 125 today, not 75/0).
# Fake-cargo-on-PATH shim ensures NO real workspace compile ever runs.
# ==============================================================================
echo ""
echo "--- Check 13: rebuild-budget-safe + REIFY_AUDIT_NO_COLD_BUILD=1 → exit 75, no cargo ---"

# Setup: stale bin + shim cargo that touches the bin (freshens it) AND writes
# a marker file when invoked.  If cargo is NOT invoked the marker is absent.
BS_TMPDIR=$(mktemp -d /tmp/test-budget-safe-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR" "$REBUILD_TMPDIR" "$GIT_NO_HIST_DIR" "$BS_TMPDIR"' EXIT

BS_STALE_BIN="$TMPDIR_FRESHNESS/reify-audit-budget-safe-stale"
touch "$BS_STALE_BIN"
touch -t 200001010000 "$BS_STALE_BIN"

BS_MARKER="$BS_TMPDIR/cargo-was-invoked"

# Shim cargo: writes marker + freshens bin + exits 0.
cat > "$BS_TMPDIR/cargo" <<EOF
#!/usr/bin/env bash
touch '$BS_MARKER'
touch '$BS_STALE_BIN'
exit 0
EOF
chmod +x "$BS_TMPDIR/cargo"

# 13a: exit code must be 75
set +e
BS_RC=$(env PATH="$BS_TMPDIR:$PATH" REIFY_AUDIT_NO_COLD_BUILD=1 bash -c \
    "source '$FRESHNESS_LIB' && reify_audit_guard '$BS_STALE_BIN' rebuild-budget-safe '$REPO_ROOT'" 2>/dev/null)
BS_EXIT=$?
set -e

assert "rebuild-budget-safe: REIFY_AUDIT_NO_COLD_BUILD=1 + stale bin → exit 75" \
    bash -c "[ '$BS_EXIT' -eq 75 ]"

# 13b: cargo shim must NOT have been invoked (no marker, bin still stale)
assert "rebuild-budget-safe: REIFY_AUDIT_NO_COLD_BUILD=1 → cargo NOT invoked (no marker file)" \
    bash -c "[ ! -f '$BS_MARKER' ]"

# 13c: bin timestamp must still be stale (cargo never freshened it)
assert "rebuild-budget-safe: REIFY_AUDIT_NO_COLD_BUILD=1 → bin remains stale (not touched by cargo)" \
    env PATH="$BS_TMPDIR:$PATH" bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '$BS_STALE_BIN' '$REPO_ROOT'"

# ==============================================================================
echo ""
echo "--- Check 14: rebuild-budget-safe + REIFY_AUDIT_NO_COLD_BUILD unset + freshening shim → exit 0 ---"

BS_REBUILD_BIN="$TMPDIR_FRESHNESS/reify-audit-budget-safe-rebuild"
touch "$BS_REBUILD_BIN"
touch -t 200001010000 "$BS_REBUILD_BIN"

# Shim cargo: freshens the bin and exits 0 (no cold build in practice, just a shim).
cat > "$BS_TMPDIR/cargo-rebuild" <<EOF
#!/usr/bin/env bash
touch '$BS_REBUILD_BIN'
exit 0
EOF
chmod +x "$BS_TMPDIR/cargo-rebuild"

# Swap shim: replace cargo with the rebuild-freshening shim
cp "$BS_TMPDIR/cargo-rebuild" "$BS_TMPDIR/cargo"
chmod +x "$BS_TMPDIR/cargo"

assert "rebuild-budget-safe: REIFY_AUDIT_NO_COLD_BUILD unset + freshening shim → exit 0 (falls through to rebuild)" \
    env PATH="$BS_TMPDIR:$PATH" bash -c "unset REIFY_AUDIT_NO_COLD_BUILD; source '$FRESHNESS_LIB' && reify_audit_guard '$BS_REBUILD_BIN' rebuild-budget-safe '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
echo ""
echo "--- Check 15: rebuild-budget-safe + fresh bin → exit 0 regardless of REIFY_AUDIT_NO_COLD_BUILD ---"

BS_FRESH_BIN="$TMPDIR_FRESHNESS/reify-audit-budget-safe-fresh"
touch "$BS_FRESH_BIN"
# Fresh bin: mtime is now, well after any historical commit.

assert "rebuild-budget-safe: fresh bin + REIFY_AUDIT_NO_COLD_BUILD=1 → exit 0 (fast path)" \
    bash -c "REIFY_AUDIT_NO_COLD_BUILD=1 bash -c \"source '$FRESHNESS_LIB' && reify_audit_guard '$BS_FRESH_BIN' rebuild-budget-safe '$REPO_ROOT' 2>/dev/null\""

assert "rebuild-budget-safe: fresh bin + REIFY_AUDIT_NO_COLD_BUILD unset → exit 0 (fast path)" \
    bash -c "unset REIFY_AUDIT_NO_COLD_BUILD; source '$FRESHNESS_LIB' && reify_audit_guard '$BS_FRESH_BIN' rebuild-budget-safe '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 16: reify_audit_guard warn-open — present + executable + stale binary
#           FAILS OPEN (exit 0) with a loud alarm on stderr (task #7139)
#
# WHY THIS MODE EXISTS. The predone wrapper is a SYNCHRONOUS pre-done hook:
# dark-factory's fused_memory/middleware/pre_done_hook.py allows the status
# flip only on rc 0 and blocks it on ANY non-zero rc, so `refuse` mode's 125
# wedges EVERY done-flip in the project until a human reinstalls the binary.
# crates/reify-audit lands ~4 commits/day, so the freshness epoch advances
# several times a day and that outage recurs by construction (it ran ~15.7h
# over 2026-08-30/31). Falling open runs the STALE detector rather than no
# detector — strictly the pre-existing risk profile of the binary that is
# already installed, and a far weaker risk than a project-wide inability to
# mark work done. This also aligns the shell guard with reify-audit's OWN
# house rule: p5_phantom_done.rs degrades a would-be-blocking High to an
# "[advisory - ...]" exit 0 whenever its evidence is incomplete. Binary age is
# strictly WEAKER evidence than a degraded git leg, so fail-closed here was an
# inversion.
# ==============================================================================
echo ""
echo "--- Check 16: guard warn-open fails OPEN for a present, executable, stale binary ---"

WO_TMPDIR=$(mktemp -d /tmp/test-warn-open-XXXXXX)
# Cumulative trap rewrite — bash EXIT traps do not stack, so the whole list
# must be re-declared (same as :104, :152, :213, :254 above).
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR" "$REBUILD_TMPDIR" "$GIT_NO_HIST_DIR" "$BS_TMPDIR" "$WO_TMPDIR"' EXIT

# Executability is LOAD-BEARING here, unlike the empty non-executable fakes
# used by Checks 4-15: warn-open splits on `[ -x "$bin" ]` (Check 17), so a
# non-executable fixture would exercise the refusal leg instead. chmod first,
# then `touch -t` — chmod moves ctime, not mtime, so the staling must come last.
WO_STALE_BIN="$WO_TMPDIR/reify-audit"
touch "$WO_STALE_BIN"
chmod +x "$WO_STALE_BIN"
touch -t 200001010000 "$WO_STALE_BIN"

# 16a: exit code must be 0 — the done-flip is NOT blocked.
set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$WO_STALE_BIN" warn-open "$REPO_ROOT") 2>/dev/null
WO_RC_16A=$?
set -e

assert "warn-open: present+executable+stale binary exits 0 (FAILS OPEN)" \
    bash -c 'test "$1" -eq 0' -- "$WO_RC_16A"

# 16b: falling open must be LOUD, not silent. Capture stderr only, using the
# `2>&1 >/dev/null` ordering (stderr to the pipe, stdout to /dev/null).
set +e
WO_ERR_16=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
set -e

assert "warn-open: stale binary emits a NON-EMPTY alarm on stderr (loud, not silent)" \
    bash -c 'test -n "$1"' -- "$WO_ERR_16"

# 16c: the alarm carries a stable machine token. Consumers (the deploy probe,
# the wrapper suite, a triaging agent's grep) must branch on this token rather
# than on message prose — the pub-const convention from
# crates/reify-audit/src/jcodemunch_index.rs:522-543.
assert "warn-open: stale-binary alarm carries the stable token E_AUDIT_BIN_STALE" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_BIN_STALE"' -- "$WO_ERR_16"

# 16d/16e: an UNKNOWN mode must NOT reinstate the outage (#7139 review).
# Before the mode-validation arm there was none: any unrecognised mode string
# fell through every `if [ "$mode" = ... ]` test to the terminal `return 125`
# refuse path once the binary was judged stale. So a one-character slip at the
# wrapper's call site — `warm-open` is literally the spelling this task's own
# RED steps used — would block every done-flip again, with the OLD
# non-self-describing message and none of the tokens above. Nothing else in
# this suite can catch that: every other assertion passes a VALID mode.
set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$WO_STALE_BIN" warm-open "$REPO_ROOT") 2>/dev/null
WO_RC_16D=$?
set -e

assert "warn-open: a TYPO'd mode ('warm-open') on a stale binary still exits 0, not 125" \
    bash -c 'test "$1" -eq 0' -- "$WO_RC_16D"

set +e
WO_ERR_16D=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warm-open '$REPO_ROOT'" 2>&1 >/dev/null)
set -e

assert "warn-open: a TYPO'd mode names ITSELF via the token E_AUDIT_GUARD_BAD_MODE" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_GUARD_BAD_MODE"' -- "$WO_ERR_16D"

# ==============================================================================
# Check 17: warn-open still REFUSES when there is nothing runnable to fall
#           open ONTO (task #7139)
#
# Fail-open means "run the stale detector anyway". That is only meaningful
# when a detector EXISTS. reify_audit_is_stale treats a MISSING binary as
# stale (see "Missing binary is always stale", freshness.sh:123-126), so a
# warn-open branch that returned 0 unconditionally would hand an absent
# binary back to the wrapper, which would then exec nothing and block the
# done-flip anyway — with a worse, less diagnosable rc (127 from the shell,
# not a guard code).
#
# So warn-open must split on `[ -x "$bin" ]`. That is exactly the
# presence-ambiguity contract the library ALREADY documents for rc 75
# (:161-175) and rc 125 (:196-204): "Callers that refuse on 125 must split on
# `[ -x "$bin" ]` first". This check makes the library honour its own
# documented contract rather than pushing it onto every caller.
# ==============================================================================
echo ""
echo "--- Check 17: warn-open refuses (125) when there is nothing runnable ---"

# 17a: ABSENT path.
set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$WO_TMPDIR/does-not-exist" warn-open "$REPO_ROOT") 2>/dev/null
WO_RC_17A=$?
set -e

assert "warn-open: ABSENT binary exits 125 (nothing to fall open onto)" \
    bash -c 'test "$1" -eq 125' -- "$WO_RC_17A"

# 17b: distinct token, so a consumer can tell "ran a stale detector" (16c)
# from "ran nothing at all".
set +e
WO_ERR_17A=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_TMPDIR/does-not-exist' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
set -e

assert "warn-open: ABSENT binary refusal carries the token E_AUDIT_BIN_MISSING" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_BIN_MISSING"' -- "$WO_ERR_17A"

# 17c: PRESENT but NOT executable — equally unrunnable, and reify_audit_is_stale's
# own presence check is only `-f` (:124), so the guard must be the stricter one.
# Deliberately NO chmod +x here.
WO_NOEXEC_BIN="$WO_TMPDIR/reify-audit-noexec"
touch "$WO_NOEXEC_BIN"
touch -t 200001010000 "$WO_NOEXEC_BIN"

set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$WO_NOEXEC_BIN" warn-open "$REPO_ROOT") 2>/dev/null
WO_RC_17C=$?
WO_ERR_17C=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_NOEXEC_BIN' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
set -e

assert "warn-open: PRESENT-but-not-executable binary exits 125" \
    bash -c 'test "$1" -eq 125' -- "$WO_RC_17C"

assert "warn-open: PRESENT-but-not-executable refusal carries E_AUDIT_BIN_MISSING" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_BIN_MISSING"' -- "$WO_ERR_17C"

# 17d REGRESSION PIN: `refuse` mode is UNCHANGED. The very same
# present+executable+stale binary that warn-open lets through (16a) must still
# exit 125 under `refuse`. This proves the new mode is purely ADDITIVE and that
# Check 8's existing contract — which is library API, not just an internal
# detail — is intact.
set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$WO_STALE_BIN" refuse "$REPO_ROOT") 2>/dev/null
WO_RC_17D=$?
set -e

assert "regression pin: refuse mode STILL exits 125 for the same binary warn-open passes" \
    bash -c 'test "$1" -eq 125' -- "$WO_RC_17D"

# ==============================================================================
# Check 18: REIFY_AUDIT_FRESHNESS_STRICT=1 restores fail-closed behaviour
#           under warn-open (task #7139)
#
# The opt-in escape hatch for an operator who would rather block done-flips
# than run a stale detector. It is an ENV KNOB, not a fourth mode, because the
# wrapper's call site is fixed in the script while the systemd unit is where
# an operator can actually set something — the same delivery path
# REIFY_AUDIT_PREDONE_WARN_ONLY already uses
# (docs/architecture-audit/f-infra-design.md:371-376).
#
# NOTE for future editors: do NOT add REIFY_AUDIT_FRESHNESS_STRICT to
# tests/infra/run-all-ambient-vars.manifest. That ledger records only vars
# ambiently INJECTED into the run_all.sh pool (from verify.sh's plan line or
# dark-factory-orchestrator.yaml's verify_env); this var is injected by
# neither, and test_run_all_ambient_isolation.sh asserts SET EQUALITY, so an
# unwarranted row would RED the gate.
# ==============================================================================
echo ""
echo "--- Check 18: REIFY_AUDIT_FRESHNESS_STRICT=1 restores fail-closed under warn-open ---"

# 18a: strict armed + present/executable/stale → 125, NOT 0.
set +e
env REIFY_AUDIT_FRESHNESS_STRICT=1 bash -c \
    "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warn-open '$REPO_ROOT'" 2>/dev/null
WO_RC_18A=$?
set -e

assert "warn-open + REIFY_AUDIT_FRESHNESS_STRICT=1: stale binary exits 125 (fail-closed)" \
    bash -c 'test "$1" -eq 125' -- "$WO_RC_18A"

# 18b: the strict refusal must stay DISTINGUISHABLE from the unrunnable-binary
# refusal (17b/17c). Both exit 125, so only the token separates them — and a
# consumer must never misread "the operator chose strict" as "no binary on
# disk". The binary IS present here, so E_AUDIT_BIN_STALE must be the token
# and E_AUDIT_BIN_MISSING must be ABSENT.
set +e
WO_ERR_18=$(env REIFY_AUDIT_FRESHNESS_STRICT=1 bash -c \
    "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
set -e

assert "warn-open strict refusal carries E_AUDIT_BIN_STALE (the binary exists, it is merely old)" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_BIN_STALE"' -- "$WO_ERR_18"

assert "warn-open strict refusal does NOT carry E_AUDIT_BIN_MISSING (nothing is missing)" \
    bash -c '! printf "%s" "$1" | grep -qF "E_AUDIT_BIN_MISSING"' -- "$WO_ERR_18"

# 18c: only the literal "1" arms strict. Anything else (unset, empty, "0",
# "true") leaves the default fail-open in force. Follows Check 13/14's
# precedent of testing both the set and the unset state of an env knob.
set +e
env REIFY_AUDIT_FRESHNESS_STRICT=0 bash -c \
    "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warn-open '$REPO_ROOT'" 2>/dev/null
WO_RC_18C=$?
set -e

assert "warn-open + REIFY_AUDIT_FRESHNESS_STRICT=0: stale binary exits 0 (default fail-open)" \
    bash -c 'test "$1" -eq 0' -- "$WO_RC_18C"

# 18d: strict must not break the fresh fast path — a FRESH binary is not stale
# at all, so the guard returns before any policy decision is reached.
WO_FRESH_BIN="$WO_TMPDIR/reify-audit-fresh"
touch "$WO_FRESH_BIN"
chmod +x "$WO_FRESH_BIN"

set +e
env REIFY_AUDIT_FRESHNESS_STRICT=1 bash -c \
    "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_FRESH_BIN' warn-open '$REPO_ROOT'" 2>/dev/null
WO_RC_18D=$?
set -e

assert "warn-open + REIFY_AUDIT_FRESHNESS_STRICT=1: FRESH binary still exits 0 (fast path intact)" \
    bash -c 'test "$1" -eq 0' -- "$WO_RC_18D"

# ==============================================================================
# Check 19: the stale/missing messages are SELF-DESCRIBING (task #7139)
#
# This is a RUNTIME-BEHAVIOUR contract on a diagnostic the hook path actually
# surfaces, not a docstring pin. dark-factory's
# fused_memory/middleware/pre_done_hook.py clips the hook's captured stderr to
# _STDERR_CLIP = 2000 chars (:51) and surfaces it to the MCP caller (:225-229).
# So this text IS what a triaging agent reads.
#
# It has to be much better than it was. The three escalations this task was
# filed over — esc-7042-2, esc-6315-2, esc-6120-5 — all blamed "stale
# metadata.files" and the done_provenance ancestor check. Both are false
# leads: the condition is a stale BINARY, an infrastructure fact with nothing
# to do with task records. The message must make that misreading impossible.
# ==============================================================================
echo ""
echo "--- Check 19: warn-open messages are self-describing ---"

# Capture both forms. ADVISORY = present+executable+stale, rc 0.
# REFUSAL = the same binary with strict armed, rc 125 (the 125 form the hook
# actually surfaces to the caller).
set +e
WO_MSG_ADVISORY=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
WO_MSG_REFUSAL=$(env REIFY_AUDIT_FRESHNESS_STRICT=1 bash -c \
    "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_STALE_BIN' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
WO_MSG_MISSING=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$WO_TMPDIR/does-not-exist' warn-open '$REPO_ROOT'" 2>&1 >/dev/null)
set -e

# 19a: the LEGACY substring. Load-bearing, not cosmetic —
# scripts/deploy-reify-audit-predone-hook.sh:402 greps for exactly this, so
# preserving it keeps that probe working across this change.
for _form in ADVISORY REFUSAL MISSING; do
    eval "_msg=\$WO_MSG_$_form"
    assert "19a [$_form]: message contains the legacy substring 'reinstall with: cargo install'" \
        bash -c 'printf "%s" "$1" | grep -qF "reinstall with: cargo install"' -- "$_msg"
done

# 19b: the FULL remedy, so the reader can paste one line and be done.
for _form in ADVISORY REFUSAL MISSING; do
    eval "_msg=\$WO_MSG_$_form"
    assert "19b [$_form]: message contains the full remedy command" \
        bash -c 'printf "%s" "$1" | grep -qF "cargo install --path crates/reify-audit --root ~/.cargo --force"' -- "$_msg"
done

# 19c: the NEGATIVE disambiguator — the message must name what it is NOT.
# Pin the presence of the two false leads plus the word "infrastructure"; do
# not pin whole sentences (prose is allowed to improve).
for _form in ADVISORY REFUSAL MISSING; do
    eval "_msg=\$WO_MSG_$_form"
    assert "19c [$_form]: message disclaims metadata.files (the esc-7042-2 false lead)" \
        bash -c 'printf "%s" "$1" | grep -qF "metadata.files"' -- "$_msg"
    assert "19c [$_form]: message disclaims done_provenance (the esc-6120-5 false lead)" \
        bash -c 'printf "%s" "$1" | grep -qF "done_provenance"' -- "$_msg"
    assert "19c [$_form]: message says 'infrastructure' (names its own category)" \
        bash -c 'printf "%s" "$1" | grep -qF "infrastructure"' -- "$_msg"
done

# 19d: the two OBSERVED numbers, already computed at freshness.sh:156-158, so a
# triager can confirm the diagnosis without rerunning anything. Only the two
# forms that HAVE a readable mtime are checked — the MISSING form has no binary
# and therefore no mtime to report.
WO_EPOCH=$(bash -c "source '$FRESHNESS_LIB' && reify_audit_crate_commit_epoch '$REPO_ROOT'")
WO_BTIME=$(bash -c "source '$FRESHNESS_LIB' && portable_mtime '$WO_STALE_BIN'")

for _form in ADVISORY REFUSAL; do
    eval "_msg=\$WO_MSG_$_form"
    assert "19d [$_form]: message reports the observed binary mtime ($WO_BTIME)" \
        bash -c 'printf "%s" "$1" | grep -qF "$2"' -- "$_msg" "$WO_BTIME"
    assert "19d [$_form]: message reports the observed crate epoch ($WO_EPOCH)" \
        bash -c 'printf "%s" "$1" | grep -qF "$2"' -- "$_msg" "$WO_EPOCH"
done

# 19e: BLAST RADIUS, on the rc-125 forms only. A refusal blocks every done-flip
# in the project until fixed, and saying so is what turns a cryptic 125 into an
# actionable one. The ADVISORY must NOT claim it — nothing is blocked there,
# and a false blast-radius claim would send a triager chasing an outage that
# is not happening.
for _form in REFUSAL MISSING; do
    eval "_msg=\$WO_MSG_$_form"
    assert "19e [$_form]: rc-125 message names the blast radius (blocks done-flips project-wide)" \
        bash -c 'printf "%s" "$1" | grep -qiE "block[a-z]* (every|all) done-flip"' -- "$_msg"
done

assert "19e [ADVISORY]: rc-0 message does NOT claim anything is blocked" \
    bash -c '! printf "%s" "$1" | grep -qiE "block[a-z]* (every|all) done-flip"' -- "$WO_MSG_ADVISORY"

# 19f: nothing important may be lost to dark-factory's _STDERR_CLIP = 2000.
for _form in ADVISORY REFUSAL MISSING; do
    eval "_msg=\$WO_MSG_$_form"
    assert "19f [$_form]: whole message is under 2000 chars (survives _STDERR_CLIP intact)" \
        bash -c 'test "${#1}" -lt 2000' -- "$_msg"
done

# -- Summary ------------------------------------------------------------------
test_summary
