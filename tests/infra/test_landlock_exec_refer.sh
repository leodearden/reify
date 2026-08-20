#!/usr/bin/env bash
# Infrastructure test for task #5820 (vendored Landlock helper: FS_REFER),
# following esc-5017-6.
#
# SUBJECT: gui/src-tauri/sandbox/landlock_exec.py — reify's VENDORED copy of
# dark-factory's landlock_exec.py, the helper the GUI's claude sidecar wraps
# around its child process. This guard is BEHAVIOURAL: it spawns the real
# helper and exercises real rename(2) calls inside the resulting ruleset. It
# does not grep the source, so a refactor that preserves the behaviour keeps
# it green and a behaviour regression turns it red regardless of how the file
# is written.
#
# THE LANDLOCK BACK-COMPAT RULE THIS EXISTS TO PIN
# ------------------------------------------------
# Landlock ABI 2 added LANDLOCK_ACCESS_FS_REFER (1 << 13): permission to
# rename(2)/link(2) a file ACROSS directories (reparenting). For backward
# compatibility, a ruleset that does NOT list REFER in handled_access_fs has
# every cross-directory rename/link denied UNCONDITIONALLY — and the kernel
# surfaces that denial as EXDEV ("Invalid cross-device link"), NOT EACCES, so
# that applications with a cross-device copy+unlink fallback keep working.
# Same-directory renames stay allowed. Consequence: a v1-only mask silently
# breaks every reparenting rename on the very same filesystem. rustc's
# encode_and_write_metadata writes its .rmeta into a temp subdirectory and
# renames it up one level, so a v1-only mask breaks EVERY cargo
# build/check/test under the sandbox.
#
# REFER must be set on handled_access_fs AND granted on every writable
# per-path rule (both endpoints of a rename). Handling REFER without granting
# it is strictly WORSE than not handling it: Landlock then enforces it
# explicitly and denies reparenting on every path that lacks the grant.
#
# FOUR TRAPS, RECORDED SO THEY ARE NOT RE-DERIVED
# ------------------------------------------------
#  1. `mv` MASKS THE BUG. coreutils mv falls back to copy+unlink on EXDEV —
#     precisely the errno an unhandled REFER produces — so an mv-based probe
#     reports success while the ruleset is broken. Every rename below is a raw
#     os.rename(2) from an inner python3 script, never `mv`.
#  2. /tmp IS GRANTED WHOLESALE by the helper (accepted v1 limitation, see
#     CLAUDE.md "Vendored sandbox helpers"). A containment assertion whose
#     destination is under /tmp therefore SUCCEEDS even in a correct build and
#     is a false premise. Assertion (c)'s destination is CHECKED against the
#     granted roots rather than assumed safe: a lane provisioned in tmpfs, or a
#     CI checkout under $TMPDIR, puts $REPO_ROOT itself inside a granted root,
#     and (c) would then cry "CONTAINMENT BREACH" at a perfectly correct helper.
#     Every candidate destination clears _granted_root_hit or is rejected.
#  4. A CROSS-FILESYSTEM ESCAPE DESTINATION MAKES (c) VACUOUS — the mirror image
#     of trap #2, and the reason trap #2's own reasoning was not sufficient. An
#     unhandled/ungranted REFER denial surfaces as EXDEV; so does a GENUINE
#     cross-device rename, rejected by the VFS before Landlock is ever
#     consulted. The two are indistinguishable at the probe: both reach
#     os.rename(2), both print `RENAME_DENIED errno=18`, both leave no file at
#     the destination. So the RENAME_DENIED marker — which correctly rules out a
#     broken fixture — does NOT rule this out. MEASURED on the merge-gate host:
#     /tmp is dev 66312 (ext4 on /) while a warm lane's worktree is dev 1821
#     (xfs on a loop device), so the original $TMPROOT -> $REPO_ROOT escape
#     rename returned EXDEV from the VFS and (c) passed unconditionally — it
#     stayed green even with the helper invoked as `--writable $TMPROOT
#     --writable $REPO_ROOT`, i.e. with containment deliberately granted away.
#     Two independent repairs, both applied below: (i) the destination must now
#     be on the SAME DEVICE as $TMPROOT (`stat -c %d`) — /var/tmp is the
#     preferred candidate precisely because the helper's own comment says it is
#     deliberately NOT granted, and it usually shares a filesystem with /tmp —
#     plus an AMBIENT CONTROL that runs the identical escape rename UNSANDBOXED
#     and SKIPs (c) when it already fails; and (ii) assertion (e), a plain
#     create+write outside every granted root. A write has no EXDEV failure
#     mode at all, so (e) tests containment directly and stays live no matter
#     how the host's mounts are laid out.
#  3. A /tmp-ROOTED FIXTURE BLINDS ASSERTION (a) — the mirror image of trap #2.
#     Trap #2 is about the rename DESTINATION; this one is about the SOURCE
#     ROOT. Landlock path_beneath rules apply down the hierarchy, so a
#     --writable root minted under /tmp (or under ~/.claude) already inherits
#     that root's wholesale fs_writable_all — including REFER — no matter what
#     the helper's `for path in ns.writable` loop grants. MEASURED on this host
#     (ABI 6), regressing ONLY that loop to the v1-only mask:
#         pristine  + root under /tmp -> OK  | pristine  + root outside /tmp -> OK
#         REGRESSED + root under /tmp -> OK  | REGRESSED + root outside /tmp -> errno=18
#     i.e. a /tmp-rooted fixture stays GREEN on a broken ruleset. Assertion (a)
#     therefore mints its own root OUTSIDE /tmp ($REPO_ROOT/target by default).
#     This is not hypothetical for production: the GUI sidecar's real writable
#     root is REIFY_WORKSPACE — the user's project dir, per compute_sidecar_env
#     in gui/src-tauri/src/claude_bridge.rs — which is not under /tmp.
#
# ASSERTIONS
#   (a) RED at authoring time — a cross-directory rename INSIDE the single
#       --writable root succeeds. Measured with the vendored helper before the
#       fix: errno=18 (EXDEV). Measured with dark-factory's fixed helper: OK.
#       That root is deliberately OUTSIDE /tmp (see trap #3), so this assertion
#       observes the PER-PATH --writable REFER grant rather than /tmp's
#       wholesale one. It therefore catches BOTH failure modes: the wholesale
#       revert that drops REFER from handled_access_fs (its authoring-time RED,
#       which denies reparenting everywhere) and the narrower regression that
#       handles REFER but stops granting it on --writable paths.
#   (b) CONTROL, green before and after — a SAME-directory rename inside the
#       writable root succeeds. Guards against the fix over-tightening the
#       mask.
#   (c) CONTAINMENT (rename axis), green before and after — a rename OUT of
#       every granted root is still refused, and leaves no file behind. An
#       INVERTED assertion, hence one that can pass for the wrong reason, in
#       TWO distinct ways, each with its own guard:
#         * a broken fixture (the helper dying before exec, python3
#           unresolvable inside the ruleset, open(src) denied) also exits
#           non-zero with the rename never attempted — guarded by requiring the
#           probe's own RENAME_DENIED marker, which reports INCONCLUSIVE rather
#           than passing on an untested code path;
#         * a genuine cross-device rename also reaches os.rename(2) and also
#           reports EXDEV — guarded by the same-device destination check and the
#           ambient control, see trap #4.
#   (e) CONTAINMENT (write axis), green before and after — a plain create+write
#       at a path outside every granted root is refused and leaves no file
#       behind. Deliberately redundant with (c) on the contract and deliberately
#       NOT redundant on the failure modes: open(2)/write(2) have no EXDEV
#       fallback semantics and no filesystem-boundary confound, so (e) cannot be
#       silenced by the host's mount layout the way (c) was (trap #4). If the
#       two ever disagree, (e) is the one to believe about containment.
#   (d) CHARACTERIZATION / ANTI-BLIND-`cp` GUARD, green at authoring time —
#       ~/.claude is still WRITABLE under the vendored helper. This is a
#       REIFY-ONLY DIVERGENCE from dark-factory: upstream dropped the blanket
#       ~/.claude grant in favour of a per-task CLAUDE_CONFIG_DIR, which
#       reify's sidecar never sets (compute_sidecar_env in
#       gui/src-tauri/src/claude_bridge.rs emits only REIFY_WORKSPACE /
#       REIFY_LANDLOCK_EXEC / REIFY_DEBUG_PORT). So refreshing this vendored
#       file with a blind `cp` from dark-factory would leave the GUI sidecar's
#       claude-CLI OAuth/session state read-only — a regression traded for a
#       fix. This assertion goes RED the instant that happens.
#
# SKIP LADDER (never a false FAIL on a host that cannot answer the question):
#   - python3 absent                     -> skip the whole file.
#   - kernel Landlock ABI < 2 (or absent) -> skip the behavioural block; below
#     ABI 2 the v1-only mask is the correct mask and REFER cannot be set at
#     all (landlock_create_ruleset returns EINVAL).
#   - no usable non-/tmp base for assertion (a)'s root -> skip ONLY (a), with a
#     diagnostic naming every candidate tried and why it was rejected;
#     (b)/(c)/(d) still run off $TMPROOT. A silently-skipped (a) would be as
#     bad as the blind one trap #3 describes, so the message is deliberately
#     loud and self-diagnosing.
#   - no escape destination that is BOTH outside every wholesale-granted root
#     (trap #2 — there the rename succeeds legitimately, so asserting a breach
#     would be a false FAIL) AND on the same device as $TMPROOT (trap #4 —
#     there the rename fails for a reason that has nothing to do with Landlock,
#     so asserting containment would be a false PASS) -> skip ONLY (c), naming
#     every candidate tried and which of the two checks rejected it. (e) has no
#     device constraint and keeps covering containment on such a host.
#   - no path outside every wholesale-granted root at all -> skip ONLY (e).
#   - AMBIENT CONTROL fails -> skip the affected assertion(s) with a
#     diagnostic. Landlock rulesets layer by INTERSECTION, so if the process
#     running this test is itself already inside a ruleset that does not hand
#     out the access under test, the inner probe is denied no matter how
#     correct the vendored helper is -- the inner ruleset can only ever
#     SUBTRACT. Running each probe unsandboxed FIRST makes a RED attributable
#     to the ruleset under test rather than to the ambient environment or an
#     exotic filesystem. Three ambient controls, one per axis:
#       * cross-directory rename inside $TMPROOT, gating (b)/(c);
#       * cross-directory rename inside $REFERROOT, gating (a) -- a separate
#         control because that root sits outside /tmp and so may fall outside a
#         sandboxed agent role's write set even when $TMPROOT does not;
#       * a write to ~/.claude, gating (d).
#     MEASURED: under a dark-factory-sandboxed agent role, the ambient ~/.claude
#     write is already refused with errno=13 -- DF's compute_write_set grants
#     ~/.claude/fleet/ but NOT ~/.claude wholesale -- so (d) correctly SKIPs
#     there instead of false-FAILing. Verify subprocesses are NOT sandboxed
#     (see tests/infra/test_sandbox_cache_writability_seam.sh), so (d) runs
#     for real on the merge gate, which is where it must be live.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== landlock_exec FS_REFER cross-directory rename guard (task 5820) ==="

HELPER="$REPO_ROOT/gui/src-tauri/sandbox/landlock_exec.py"

assert "vendored helper gui/src-tauri/sandbox/landlock_exec.py exists" \
    test -f "$HELPER"

if ! command -v python3 >/dev/null 2>&1; then
    echo "SKIP: python3 not available; cannot exercise the Landlock helper"
    test_summary
    exit 0
fi

# ---------------------------------------------------------------------------
# Fixture — hermetic (pool-safe): two mktemp -d roots, plus two paths OUTSIDE
# them that the assertions probe and the trap reclaims.
# ---------------------------------------------------------------------------
TMPROOT="$(mktemp -d "${TMPDIR:-/tmp}/reify-landlock-refer.XXXXXX")"
CLAUDE_DIR="$(python3 -c 'import os; print(os.path.expanduser("~/.claude"))')"
CLAUDE_PROBE="$CLAUDE_DIR/.reify-landlock-writable-probe.$$"
# Two paths that must sit OUTSIDE every wholesale-granted root: assertion (c)'s
# rename destination (trap #2) and assertion (a)'s own --writable root (trap
# #3). Both are resolved below by the SAME granted-root checks -- neither is
# assumed safe by construction. Initialised empty so the EXIT trap, installed
# before either is resolved, tolerates the unresolved case. ESCAPE_DST is never
# pre-created: its non-existence afterwards is part of assertion (c).
ESCAPE_DST=""
WRITE_ESCAPE_DST=""
REFERROOT=""
cleanup() {
    rm -rf "$TMPROOT"
    rm -f "$CLAUDE_PROBE"
    if [ -n "${ESCAPE_DST:-}" ]; then rm -f "$ESCAPE_DST"; fi
    if [ -n "${WRITE_ESCAPE_DST:-}" ]; then rm -f "$WRITE_ESCAPE_DST"; fi
    if [ -n "${REFERROOT:-}" ]; then rm -rf "$REFERROOT"; fi
    return 0
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Assertion (a)'s non-/tmp fixture root. See trap #3 in the header: a root
# minted under /tmp (or under ~/.claude) inherits that root's WHOLESALE
# fs_writable_all grant, so it cannot observe whether the helper's
# `for path in ns.writable` loop grants REFER -- the grant that actually
# matters in production, where the sidecar's writable root is REIFY_WORKSPACE.
#
# Candidate ladder, first qualifying entry wins:
#   1. $REIFY_LANDLOCK_TEST_ROOT -- explicit override for exotic hosts / CI.
#   2. $REPO_ROOT/target         -- default: gitignored (.gitignore "/target"),
#                                   so scratch there never dirties the worktree;
#                                   preserved by the lane `git clean -xfd -e
#                                   target`; always writable; outside /tmp.
#   3. $HOME.
# A candidate qualifies only if it is an existing directory that, realpath'd,
# is neither /tmp, $TMPDIR nor ~/.claude, nor beneath any of them, AND under
# which `mktemp -d` actually succeeds -- writability is part of qualifying, so
# an unwritable rung falls through to the next instead of skipping (a).
# ---------------------------------------------------------------------------
REAL_TMP="$(realpath -m /tmp 2>/dev/null || echo /tmp)"
REAL_TMPDIR=""
if [ -n "${TMPDIR:-}" ]; then REAL_TMPDIR="$(realpath -m "$TMPDIR" 2>/dev/null || true)"; fi
REAL_CLAUDE="$(realpath -m "$CLAUDE_DIR" 2>/dev/null || true)"

# _is_under <realpath> <realpath-prefix> -- true when <path> IS <prefix> or
# lies beneath it. The trailing slash is what stops /tmpfoo matching /tmp.
_is_under() {
    local path="$1" prefix="$2"
    if [ -z "$prefix" ]; then return 1; fi
    if [ "$path" = "$prefix" ]; then return 0; fi
    case "$path" in
        "${prefix%/}"/*) return 0 ;;
    esac
    return 1
}

# _granted_root_hit <realpath> -- succeed, printing "<name> (<realpath>)", when
# <realpath> is at or beneath a root the helper grants WHOLESALE; fail silently
# when it is outside all of them. Landlock path_beneath rules apply down the
# hierarchy, so anything under one of these inherits its full fs_writable_all
# -- which is what makes such a path useless BOTH as assertion (a)'s writable
# root (trap #3: the per-path grant becomes unobservable) and as assertion
# (c)'s escape destination (trap #2: the rename legitimately succeeds).
# $TMPDIR is in the list because that is where $TMPROOT itself is minted.
_granted_root_hit() {
    local path="$1" spec pname pval
    for spec in "/tmp:$REAL_TMP" "\$TMPDIR:$REAL_TMPDIR" "~/.claude:$REAL_CLAUDE"; do
        pname="${spec%%:*}"
        pval="${spec#*:}"
        if _is_under "$path" "$pval"; then
            printf '%s (%s)' "$pname" "$pval"
            return 0
        fi
    done
    return 1
}

REFER_BASE=""
REFER_REJECTS=""
# _consider_base <label> <path> -- try one rung of the ladder. WRITABILITY IS
# PART OF QUALIFYING: the mktemp is attempted here, so a base that passes the
# path checks but cannot be written falls through to the next candidate rather
# than skipping (a) outright. MEASURED: under a dark-factory-sandboxed agent
# role $HOME is not writable (mktemp -> EACCES), which is exactly the rung this
# fall-through exists for.
_consider_base() {
    local label="$1" cand="$2" real hit
    if [ -n "$REFER_BASE" ]; then return 0; fi
    if [ -z "$cand" ]; then
        REFER_REJECTS="$REFER_REJECTS
        - $label: unset or empty"
        return 0
    fi
    if [ ! -d "$cand" ]; then
        REFER_REJECTS="$REFER_REJECTS
        - $label ($cand): not an existing directory"
        return 0
    fi
    real="$(realpath -m "$cand" 2>/dev/null || true)"
    if [ -z "$real" ]; then
        REFER_REJECTS="$REFER_REJECTS
        - $label ($cand): realpath -m failed"
        return 0
    fi
    if hit="$(_granted_root_hit "$real")"; then
        REFER_REJECTS="$REFER_REJECTS
        - $label ($real): at or under $hit -- would inherit its wholesale grant (trap #3)"
        return 0
    fi
    if ! REFERROOT="$(mktemp -d "$real/reify-landlock-refer-nontmp.XXXXXX" 2>"$TMPROOT/mktemp.err")"; then
        REFER_REJECTS="$REFER_REJECTS
        - $label ($real): mktemp -d failed: $(cat "$TMPROOT/mktemp.err" 2>/dev/null || true)"
        REFERROOT=""
        # Falling through past a candidate that existed but could not be
        # written is worth one visible line -- otherwise an explicit
        # $REIFY_LANDLOCK_TEST_ROOT could be silently ignored.
        echo "NOTE: assertion (a) base candidate $label ($real) is not writable; trying the next rung"
        return 0
    fi
    REFER_BASE="$real"
    return 0
}

mkdir -p "$REPO_ROOT/target" 2>/dev/null || true
_consider_base '$REIFY_LANDLOCK_TEST_ROOT' "${REIFY_LANDLOCK_TEST_ROOT:-}"
_consider_base '$REPO_ROOT/target' "$REPO_ROOT/target"
_consider_base '$HOME' "${HOME:-}"

REFER_SKIP_REASON=""
if [ -z "$REFERROOT" ]; then
    REFER_SKIP_REASON="no candidate base qualified. Tried:$REFER_REJECTS"
fi

# ---------------------------------------------------------------------------
# The two CONTAINMENT destinations -- (c)'s rename target and (e)'s write
# target. Both must sit outside every root the helper grants WHOLESALE (trap
# #2, applied to the DESTINATION with the same checks trap #3 applies to (a)'s
# SOURCE ROOT); a lane provisioned in tmpfs, or a CI checkout under $TMPDIR,
# puts $REPO_ROOT itself inside such a root, and the probe would then
# legitimately succeed against a perfectly correct helper. So no candidate is
# assumed safe -- every one clears _granted_root_hit or is rejected by name.
#
# (c) carries the ADDITIONAL constraint of trap #4: the destination must be on
# the SAME DEVICE as $TMPROOT, or the rename is a genuine cross-device rename
# that the VFS rejects with EXDEV before Landlock is consulted -- the exact
# errno a REFER denial produces, so the assertion would pass unconditionally.
# /var/tmp leads that ladder because the helper's own source comment says it is
# deliberately NOT granted ("/tmp only -- avoid /var/tmp so worktrees placed
# there stay restricted") and it typically shares a filesystem with /tmp, which
# is where $TMPROOT is minted. (e) has no such constraint: open(2)/write(2)
# have no EXDEV failure mode, which is precisely why (e) exists.
#
# Both ladders concern the WHOLESALE roots only: the destination must also lie
# outside every --writable path, which it does by construction, since (c)/(e)
# run through run_sandboxed and so hand the helper $TMPROOT as its sole
# writable root -- and the explicit _is_under check below re-confirms it rather
# than inferring it from the /tmp / $TMPDIR rejection.
# ---------------------------------------------------------------------------
REAL_TMPROOT="$(realpath -m "$TMPROOT" 2>/dev/null || echo "$TMPROOT")"
TMPROOT_DEV="$(stat -c %d "$TMPROOT" 2>/dev/null || true)"

# Ladder state, reset before each of the two runs below. Bash namerefs would
# let this return a value properly, but the file targets plain bash and the
# rest of the fixture already uses this global-accumulator idiom.
_ESC_PICK=""
_ESC_PICK_LABEL=""
_ESC_REJECTS=""
_ESC_REQUIRE_SAME_DEV=0

# _consider_escape_dir <label> <path> -- try one rung. First qualifying entry
# wins. WRITABILITY IS PART OF QUALIFYING, as in _consider_base: a candidate
# that passes every path check but cannot be written falls through to the next
# rung rather than skipping the assertion outright.
_consider_escape_dir() {
    local label="$1" cand="$2" real hit dev probe
    if [ -n "$_ESC_PICK" ]; then return 0; fi
    if [ -z "$cand" ]; then
        _ESC_REJECTS="$_ESC_REJECTS
        - $label: unset or empty"
        return 0
    fi
    if [ ! -d "$cand" ]; then
        _ESC_REJECTS="$_ESC_REJECTS
        - $label ($cand): not an existing directory"
        return 0
    fi
    real="$(realpath -m "$cand" 2>/dev/null || true)"
    if [ -z "$real" ]; then
        _ESC_REJECTS="$_ESC_REJECTS
        - $label ($cand): realpath -m failed"
        return 0
    fi
    if hit="$(_granted_root_hit "$real")"; then
        _ESC_REJECTS="$_ESC_REJECTS
        - $label ($real): at or under $hit -- granted wholesale, so the probe would
          legitimately succeed and report a bogus breach (trap #2)"
        return 0
    fi
    if _is_under "$real" "$REAL_TMPROOT"; then
        _ESC_REJECTS="$_ESC_REJECTS
        - $label ($real): inside \$TMPROOT ($REAL_TMPROOT), the sole --writable root"
        return 0
    fi
    if [ "$_ESC_REQUIRE_SAME_DEV" = "1" ]; then
        dev="$(stat -c %d "$real" 2>/dev/null || true)"
        if [ -z "$TMPROOT_DEV" ] || [ -z "$dev" ]; then
            _ESC_REJECTS="$_ESC_REJECTS
        - $label ($real): stat -c %d unavailable, cannot prove same-filesystem (trap #4)"
            return 0
        fi
        if [ "$dev" != "$TMPROOT_DEV" ]; then
            _ESC_REJECTS="$_ESC_REJECTS
        - $label ($real): device $dev != \$TMPROOT device $TMPROOT_DEV -- a rename there
          is a GENUINE cross-device rename, EXDEV from the VFS before Landlock is
          consulted, so (c) would pass without testing anything (trap #4)"
            return 0
        fi
    fi
    probe="$real/.landlock-escape-writecheck.$$"
    if ! touch "$probe" 2>/dev/null; then
        _ESC_REJECTS="$_ESC_REJECTS
        - $label ($real): not writable (touch failed)"
        return 0
    fi
    rm -f "$probe"
    _ESC_PICK="$real"
    _ESC_PICK_LABEL="$label"
    return 0
}

# (c)'s rename destination -- same-device REQUIRED (trap #4).
_ESC_PICK=""; _ESC_PICK_LABEL=""; _ESC_REJECTS=""; _ESC_REQUIRE_SAME_DEV=1
_consider_escape_dir '$REIFY_LANDLOCK_ESCAPE_DIR' "${REIFY_LANDLOCK_ESCAPE_DIR:-}"
_consider_escape_dir '/var/tmp' '/var/tmp'
_consider_escape_dir '$REPO_ROOT' "$REPO_ROOT"
_consider_escape_dir '$REFER_BASE' "${REFER_BASE:-}"
ESCAPE_SKIP_REASON=""
if [ -n "$_ESC_PICK" ]; then
    ESCAPE_DST="$_ESC_PICK/.landlock-refer-escape-probe.$$"
    echo "--- assertion (c) escape destination: $ESCAPE_DST"
    echo "    (base: $_ESC_PICK_LABEL; outside every granted root, device $TMPROOT_DEV == \$TMPROOT's) ---"
else
    ESCAPE_SKIP_REASON="no candidate qualified. Tried:$_ESC_REJECTS"
fi

# (e)'s write destination -- device irrelevant, so the ladder prefers
# $REPO_ROOT (the closest analogue of a real out-of-sandbox target) and only
# then falls back. This is the assertion that survives a host where every
# out-of-root path lives on another filesystem.
_ESC_PICK=""; _ESC_PICK_LABEL=""; _ESC_REJECTS=""; _ESC_REQUIRE_SAME_DEV=0
_consider_escape_dir '$REIFY_LANDLOCK_ESCAPE_DIR' "${REIFY_LANDLOCK_ESCAPE_DIR:-}"
_consider_escape_dir '$REPO_ROOT' "$REPO_ROOT"
_consider_escape_dir '/var/tmp' '/var/tmp'
_consider_escape_dir '$REFER_BASE' "${REFER_BASE:-}"
WRITE_ESCAPE_SKIP_REASON=""
if [ -n "$_ESC_PICK" ]; then
    WRITE_ESCAPE_DST="$_ESC_PICK/.landlock-containment-probe.$$"
    echo "--- assertion (e) write destination: $WRITE_ESCAPE_DST (base: $_ESC_PICK_LABEL) ---"
else
    WRITE_ESCAPE_SKIP_REASON="no candidate qualified. Tried:$_ESC_REJECTS"
fi

RENAME_PY="$TMPROOT/rename_probe.py"
cat > "$RENAME_PY" << 'PYEOF'
"""Raw os.rename(2) probe, run INSIDE the ruleset under test.

Deliberately os.rename(2) and never `mv`: coreutils mv falls back to
copy+unlink on EXDEV -- exactly the errno an unhandled LANDLOCK_ACCESS_FS_REFER
produces -- so an mv-based probe would report success on a broken ruleset.

Usage: rename_probe.py <src> <dst>
Exits 0 after printing RENAME_OK, or 1 after printing RENAME_DENIED errno=<n>.
"""

import os
import sys

src, dst = sys.argv[1], sys.argv[2]
parent = os.path.dirname(src)
if parent:
    os.makedirs(parent, exist_ok=True)
with open(src, 'wb') as fh:
    fh.write(b'reify-landlock-refer-probe\n')
try:
    os.rename(src, dst)
except OSError as exc:
    print('RENAME_DENIED errno=%s (%s) %s -> %s' % (exc.errno, exc.strerror, src, dst))
    sys.exit(1)
print('RENAME_OK %s -> %s' % (src, dst))
sys.exit(0)
PYEOF

WRITE_PY="$TMPROOT/write_probe.py"
cat > "$WRITE_PY" << 'PYEOF'
"""Create + write + unlink a probe file, run INSIDE the ruleset under test.

Usage: write_probe.py <path>
Exits 0 after printing WRITE_OK, or 1 after printing WRITE_DENIED errno=<n>.
"""

import os
import sys

path = sys.argv[1]
try:
    with open(path, 'wb') as fh:
        fh.write(b'reify-landlock-writable-probe\n')
    os.unlink(path)
except OSError as exc:
    print('WRITE_DENIED errno=%s (%s) %s' % (exc.errno, exc.strerror, path))
    sys.exit(1)
print('WRITE_OK %s' % path)
sys.exit(0)
PYEOF

# run_sandboxed_in <root> <inner.py> [args...] -- invoke the REAL vendored
# helper with <root> as its SOLE --writable root, then the inner probe. Sole,
# so that for assertion (a) the per-path grant on <root> is the only thing that
# can authorize the rename. The probe scripts themselves live under $TMPROOT
# and stay READABLE regardless: the helper grants '/' FS_RO.
run_sandboxed_in() {
    local root="$1"
    shift
    python3 "$HELPER" --writable "$root" -- python3 "$@"
}

# run_sandboxed <inner.py> [args...] -- the $TMPROOT-rooted form, used by
# (b)/(c)/(d).
run_sandboxed() {
    run_sandboxed_in "$TMPROOT" "$@"
}

# ---------------------------------------------------------------------------
# SKIP guard: kernel Landlock ABI. landlock_create_ruleset(NULL, 0,
# LANDLOCK_CREATE_RULESET_VERSION) returns the ABI version instead of creating
# a ruleset; a negative return means Landlock is unavailable. REFER needs >= 2.
# ---------------------------------------------------------------------------
LANDLOCK_ABI="$(python3 -c "import ctypes; libc = ctypes.CDLL('libc.so.6', use_errno=True); print(libc.syscall(444, None, 0, 1))" 2>/dev/null || true)"

RUN_BEHAVIOURAL=1
if ! printf '%s' "$LANDLOCK_ABI" | grep -Eq '^-?[0-9]+$'; then
    echo "SKIP: could not probe the kernel Landlock ABI (got '${LANDLOCK_ABI}'); skipping behavioural assertions"
    RUN_BEHAVIOURAL=0
elif [ "$LANDLOCK_ABI" -lt 2 ]; then
    echo "SKIP: kernel Landlock ABI is $LANDLOCK_ABI (< 2); FS_REFER does not exist there and the v1-only mask is correct"
    RUN_BEHAVIOURAL=0
else
    echo "--- kernel Landlock ABI = $LANDLOCK_ABI (>= 2: FS_REFER available) ---"
fi

# ---------------------------------------------------------------------------
# AMBIENT CONTROLS -- the same probes, UNSANDBOXED. Landlock layers intersect,
# so an outer ruleset that omits REFER (or an exotic FS) would deny the inner
# probe regardless of the helper. Downgrade to SKIP, never FAIL.
# ---------------------------------------------------------------------------
if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    AMBIENT_OUT="$(python3 "$RENAME_PY" "$TMPROOT/ambient/full.rmeta" "$TMPROOT/ambient-dst.rmeta" 2>&1)" || {
        echo "SKIP: ambient (unsandboxed) cross-directory rename inside $TMPROOT already fails;"
        echo "      an outer Landlock layer or an exotic filesystem is restricting us, so a RED"
        echo "      below would not be attributable to the vendored helper. Probe said: $AMBIENT_OUT"
        RUN_BEHAVIOURAL=0
    }
fi

# Assertion (a) runs off $REFERROOT, not $TMPROOT, so it needs its own gate:
# the non-/tmp base may not have resolved at all, and even when it did it can
# fall outside a sandboxed agent role's write set while $TMPROOT does not.
RUN_REFER=0
if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    if [ -z "$REFERROOT" ]; then
        echo "SKIP: assertion (a) -- could not mint a --writable root outside /tmp:"
        echo "      $REFER_SKIP_REASON"
        echo "      (a) needs such a root to observe the PER-PATH FS_REFER grant; a /tmp-rooted"
        echo "      one inherits /tmp's wholesale grant (trap #3) and would stay GREEN on a"
        echo "      broken ruleset. Set \$REIFY_LANDLOCK_TEST_ROOT to a writable non-/tmp dir to"
        echo "      re-enable it. (b)/(c)/(d) still run off \$TMPROOT."
    elif ! AMBIENT_REFER_OUT="$(python3 "$RENAME_PY" "$REFERROOT/ambient/full.rmeta" "$REFERROOT/ambient-dst.rmeta" 2>&1)"; then
        echo "SKIP: assertion (a) -- ambient (unsandboxed) cross-directory rename inside"
        echo "      $REFERROOT already fails, so a RED there would not be attributable to the"
        echo "      vendored helper. Probe said: $AMBIENT_REFER_OUT"
    else
        echo "--- assertion (a) writable root: $REFERROOT (base: $REFER_BASE, outside /tmp) ---"
        RUN_REFER=1
    fi
fi

# Assertion (c)'s own ambient control -- and the single most important one in
# this file, because (c) is INVERTED: for (a)/(b)/(d) a missing ambient control
# risks a false FAIL, but for (c) it risks a false PASS, which is silent. See
# trap #4: an ambient EXDEV here means source and destination are on different
# filesystems (or an outer layer already denies the rename), so the sandboxed
# form would be refused by something other than the vendored helper and (c)
# would report a containment it never observed. The same-device check above
# already rejects the common case by construction; this control is the
# behavioural backstop for everything it cannot see (bind mounts, overlayfs,
# an outer Landlock layer, a read-only remount).
RUN_ESCAPE=0
if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    if [ -z "$ESCAPE_DST" ]; then
        echo "SKIP: assertion (c) -- no usable rename destination: $ESCAPE_SKIP_REASON"
        echo "      (c) needs a destination that is BOTH outside every wholesale-granted root"
        echo "      (trap #2 -- else the rename legitimately succeeds) AND on \$TMPROOT's own"
        echo "      filesystem (trap #4 -- else EXDEV comes from the VFS, not from Landlock,"
        echo "      and (c) passes without testing anything). Set \$REIFY_LANDLOCK_ESCAPE_DIR"
        echo "      to such a directory to re-enable it. (e) still covers containment."
    elif ! AMBIENT_ESCAPE_OUT="$(python3 "$RENAME_PY" "$TMPROOT/ambient-escape/src.rmeta" "$ESCAPE_DST" 2>&1)"; then
        rm -f "$ESCAPE_DST"
        echo "SKIP: assertion (c) -- the ambient (unsandboxed) escape rename to $ESCAPE_DST"
        echo "      ALREADY fails, so its sandboxed form would be refused by something other"
        echo "      than the vendored helper and (c) could only pass for the wrong reason"
        echo "      (trap #4). An errno=18 here specifically means source and destination are"
        echo "      on different filesystems. Probe said: $AMBIENT_ESCAPE_OUT"
    else
        # The ambient rename SUCCEEDED, so it left the file behind; reclaim it
        # before (c) asserts on the destination's non-existence.
        rm -f "$ESCAPE_DST"
        RUN_ESCAPE=1
    fi
fi

# Assertion (e)'s ambient control. Same reasoning, one axis over: if the
# unsandboxed write already fails (an outer ruleset, a read-only mount, a
# permissions quirk), the sandboxed refusal proves nothing about the helper.
# WRITE_PY unlinks what it writes, so a successful control leaves no residue.
RUN_WRITE_ESCAPE=0
if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    if [ -z "$WRITE_ESCAPE_DST" ]; then
        echo "SKIP: assertion (e) -- no path outside every wholesale-granted root:"
        echo "      $WRITE_ESCAPE_SKIP_REASON"
    elif ! AMBIENT_WRITE_ESCAPE_OUT="$(python3 "$WRITE_PY" "$WRITE_ESCAPE_DST" 2>&1)"; then
        rm -f "$WRITE_ESCAPE_DST"
        echo "SKIP: assertion (e) -- the ambient (unsandboxed) write to $WRITE_ESCAPE_DST"
        echo "      already fails, so a sandboxed refusal would not be attributable to the"
        echo "      vendored helper. Probe said: $AMBIENT_WRITE_ESCAPE_OUT"
    else
        RUN_WRITE_ESCAPE=1
    fi
fi

if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    echo "--- behavioural assertions against the real helper ---"

    # (a) RED at authoring time. Mirrors rustc's encode_and_write_metadata:
    #     write the .rmeta into a temp subdirectory, rename it up one level.
    #     Rooted at $REFERROOT -- OUTSIDE /tmp -- so that the helper's per-path
    #     `for path in ns.writable` grant is the only thing that can authorize
    #     the rename. See trap #3: a $TMPROOT-rooted form of this assertion
    #     rides /tmp's wholesale grant and stays green on a broken ruleset.
    if [ "$RUN_REFER" = "1" ]; then
        crossdir_rename_succeeds() {
            rm -rf "$REFERROOT/sub"
            rm -f "$REFERROOT/libx.rmeta"
            run_sandboxed_in "$REFERROOT" "$RENAME_PY" "$REFERROOT/sub/full.rmeta" "$REFERROOT/libx.rmeta"
        }
        assert "(a) cross-directory rename inside a --writable root OUTSIDE /tmp succeeds (per-path FS_REFER grant, not /tmp's wholesale grant)" \
            crossdir_rename_succeeds
    fi

    # (b) CONTROL -- same-directory rename must keep working.
    samedir_rename_succeeds() {
        rm -rf "$TMPROOT/same"
        run_sandboxed "$RENAME_PY" "$TMPROOT/same/a.rmeta" "$TMPROOT/same/b.rmeta"
    }
    assert "(b) same-directory rename inside the --writable root still succeeds" \
        samedir_rename_succeeds

    # (c) CONTAINMENT, rename axis -- renaming OUT of every granted root stays
    # refused.
    #
    # An INVERTED assertion, and therefore one that can pass for the wrong
    # reason: (a)/(b)/(d)/(e)'s ambient forms pass on success, so a broken
    # fixture turns them red, but here a broken fixture looks exactly like a
    # refusal. Three guards, each closing a different way to pass vacuously:
    #   1. A bare non-zero exit is not proof -- the inner probe also exits
    #      non-zero when its own setup fails (open(src) denied, python3
    #      unresolvable inside the ruleset) and the helper exits non-zero when
    #      it dies before exec (bad argv, create_ruleset/restrict_self
    #      failing). In every one of those cases the rename was never
    #      attempted. Requiring the probe's own RENAME_DENIED marker pins the
    #      pass to "os.rename(2) was reached and the kernel refused it".
    #   2. That marker does NOT distinguish a Landlock REFER denial from a
    #      genuine cross-device EXDEV -- both reach os.rename(2) and both print
    #      errno=18. Closed by the same-device destination check (trap #4).
    #   3. ...and by the ambient control above, which proves the identical
    #      rename SUCCEEDS unsandboxed on this host.
    if [ "$RUN_ESCAPE" = "1" ]; then
        escape_rename_refused() {
            local out rc
            rm -rf "$TMPROOT/escape"
            rm -f "$ESCAPE_DST"
            out="$(run_sandboxed "$RENAME_PY" "$TMPROOT/escape/src.rmeta" "$ESCAPE_DST" 2>&1)" && rc=0 || rc=$?
            if [ "$rc" = "0" ]; then
                echo "CONTAINMENT BREACH: rename escaped every granted root to $ESCAPE_DST"
                echo "  probe said: $out"
                rm -f "$ESCAPE_DST"
                return 1
            fi
            if ! printf '%s\n' "$out" | grep -q 'RENAME_DENIED errno='; then
                echo "INCONCLUSIVE: the sandboxed probe exited $rc WITHOUT reaching os.rename(2),"
                echo "  so containment was never exercised -- a fixture failure, not a refusal."
                echo "  (c) fails rather than passing on an untested code path. Probe said: $out"
                rm -f "$ESCAPE_DST"
                return 1
            fi
            if [ -e "$ESCAPE_DST" ]; then
                echo "CONTAINMENT BREACH: $ESCAPE_DST exists after a supposedly-refused rename"
                echo "  probe said: $out"
                rm -f "$ESCAPE_DST"
                return 1
            fi
            return 0
        }
        assert "(c) rename out of every granted root is refused (probe reached os.rename and was denied) and leaves no file behind" \
            escape_rename_refused
    fi

    # (e) CONTAINMENT, write axis -- the confound-free half of the containment
    # contract. (c) can be silenced by the host's mount layout because EXDEV is
    # overloaded (trap #4); open(2)/write(2) has no EXDEV failure mode at all,
    # so the only thing that can deny this write is an access-control decision.
    # It still requires the probe's own WRITE_DENIED marker for guard #1 above
    # -- a fixture that dies before reaching open(2) must not read as
    # containment.
    if [ "$RUN_WRITE_ESCAPE" = "1" ]; then
        escape_write_refused() {
            local out rc
            rm -f "$WRITE_ESCAPE_DST"
            out="$(run_sandboxed "$WRITE_PY" "$WRITE_ESCAPE_DST" 2>&1)" && rc=0 || rc=$?
            if [ "$rc" = "0" ]; then
                echo "CONTAINMENT BREACH: wrote $WRITE_ESCAPE_DST, outside every granted root"
                echo "  probe said: $out"
                rm -f "$WRITE_ESCAPE_DST"
                return 1
            fi
            if ! printf '%s\n' "$out" | grep -q 'WRITE_DENIED errno='; then
                echo "INCONCLUSIVE: the sandboxed probe exited $rc WITHOUT reaching open(2),"
                echo "  so containment was never exercised -- a fixture failure, not a refusal."
                echo "  (e) fails rather than passing on an untested code path. Probe said: $out"
                rm -f "$WRITE_ESCAPE_DST"
                return 1
            fi
            if [ -e "$WRITE_ESCAPE_DST" ]; then
                echo "CONTAINMENT BREACH: $WRITE_ESCAPE_DST exists after a supposedly-refused write"
                echo "  probe said: $out"
                rm -f "$WRITE_ESCAPE_DST"
                return 1
            fi
            return 0
        }
        assert "(e) create+write outside every granted root is refused (probe reached open(2) and was denied; no EXDEV confound) and leaves no file behind" \
            escape_write_refused
    fi

    # (d) CHARACTERIZATION -- the anti-blind-`cp` guard. See the header.
    #
    # Its own AMBIENT CONTROL, for the same layering reason as the rename one
    # above: an outer ruleset that does not grant ~/.claude wholesale (DF's
    # compute_write_set grants only ~/.claude/fleet/) denies the write no
    # matter what the vendored helper grants, because layers only ever
    # subtract. MEASURED under a sandboxed agent role: errno=13.
    if [ ! -d "$CLAUDE_DIR" ]; then
        echo "SKIP: $CLAUDE_DIR does not exist; cannot exercise the reify-only ~/.claude grant"
    elif ! AMBIENT_CLAUDE_OUT="$(python3 "$WRITE_PY" "$CLAUDE_PROBE" 2>&1)"; then
        echo "SKIP: ambient (unsandboxed) write to $CLAUDE_DIR already fails, so the inner"
        echo "      helper's grant cannot be observed -- an outer Landlock layer that does not"
        echo "      grant ~/.claude wholesale subtracts it regardless. Probe said: $AMBIENT_CLAUDE_OUT"
    else
        claude_dir_writable() {
            rm -f "$CLAUDE_PROBE"
            run_sandboxed "$WRITE_PY" "$CLAUDE_PROBE"
        }
        assert "(d) ~/.claude stays writable under the vendored helper (reify-only divergence; a blind cp from dark-factory would drop this grant)" \
            claude_dir_writable
    fi
fi

test_summary
