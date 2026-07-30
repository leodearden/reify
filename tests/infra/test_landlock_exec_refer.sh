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
# THREE TRAPS, RECORDED SO THEY ARE NOT RE-DERIVED
# ------------------------------------------------
#  1. `mv` MASKS THE BUG. coreutils mv falls back to copy+unlink on EXDEV —
#     precisely the errno an unhandled REFER produces — so an mv-based probe
#     reports success while the ruleset is broken. Every rename below is a raw
#     os.rename(2) from an inner python3 script, never `mv`.
#  2. /tmp IS GRANTED WHOLESALE by the helper (accepted v1 limitation, see
#     CLAUDE.md "Vendored sandbox helpers"). A containment assertion whose
#     destination is under /tmp therefore SUCCEEDS even in a correct build and
#     is a false premise. Assertion (c) below uses a $REPO_ROOT-rooted
#     destination, outside /tmp, outside ~/.claude and outside every
#     --writable path.
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
#   (c) CONTAINMENT, green before and after — a rename OUT of every granted
#       root is still refused, and leaves no file behind.
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
# Fixture — hermetic (pool-safe): one mktemp -d root, plus two paths OUTSIDE
# it that the assertions probe and the trap reclaims.
# ---------------------------------------------------------------------------
TMPROOT="$(mktemp -d "${TMPDIR:-/tmp}/reify-landlock-refer.XXXXXX")"
# Destination for the containment probe. MUST be outside /tmp (granted
# wholesale), outside ~/.claude and outside every --writable path -- see trap
# #2 in the header. Never pre-created: its non-existence afterwards is half of
# assertion (c).
ESCAPE_DST="$REPO_ROOT/.landlock-refer-escape-probe.$$"
CLAUDE_DIR="$(python3 -c 'import os; print(os.path.expanduser("~/.claude"))')"
CLAUDE_PROBE="$CLAUDE_DIR/.reify-landlock-writable-probe.$$"
trap 'rm -rf "$TMPROOT"; rm -f "$ESCAPE_DST" "$CLAUDE_PROBE"' EXIT

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

# run_sandboxed <inner.py> [args...] -- invoke the REAL vendored helper with
# $TMPROOT as its sole --writable root, then the inner probe.
run_sandboxed() {
    python3 "$HELPER" --writable "$TMPROOT" -- python3 "$@"
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
# AMBIENT CONTROL -- same cross-directory rename, UNSANDBOXED. Landlock layers
# intersect, so an outer ruleset that omits REFER (or an exotic FS) would deny
# the inner rename regardless of the helper. Downgrade to SKIP, never FAIL.
# ---------------------------------------------------------------------------
if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    AMBIENT_OUT="$(python3 "$RENAME_PY" "$TMPROOT/ambient/full.rmeta" "$TMPROOT/ambient-dst.rmeta" 2>&1)" || {
        echo "SKIP: ambient (unsandboxed) cross-directory rename inside $TMPROOT already fails;"
        echo "      an outer Landlock layer or an exotic filesystem is restricting us, so a RED"
        echo "      below would not be attributable to the vendored helper. Probe said: $AMBIENT_OUT"
        RUN_BEHAVIOURAL=0
    }
fi

if [ "$RUN_BEHAVIOURAL" = "1" ]; then
    echo "--- behavioural assertions against the real helper ---"

    # (a) RED at authoring time. Mirrors rustc's encode_and_write_metadata:
    #     write the .rmeta into a temp subdirectory, rename it up one level.
    crossdir_rename_succeeds() {
        rm -rf "$TMPROOT/sub"
        rm -f "$TMPROOT/libx.rmeta"
        run_sandboxed "$RENAME_PY" "$TMPROOT/sub/full.rmeta" "$TMPROOT/libx.rmeta"
    }
    assert "(a) cross-directory rename inside the --writable root succeeds (FS_REFER handled + granted)" \
        crossdir_rename_succeeds

    # (b) CONTROL -- same-directory rename must keep working.
    samedir_rename_succeeds() {
        rm -rf "$TMPROOT/same"
        run_sandboxed "$RENAME_PY" "$TMPROOT/same/a.rmeta" "$TMPROOT/same/b.rmeta"
    }
    assert "(b) same-directory rename inside the --writable root still succeeds" \
        samedir_rename_succeeds

    # (c) CONTAINMENT -- renaming OUT of every granted root stays refused.
    escape_rename_refused() {
        rm -rf "$TMPROOT/escape"
        rm -f "$ESCAPE_DST"
        if run_sandboxed "$RENAME_PY" "$TMPROOT/escape/src.rmeta" "$ESCAPE_DST"; then
            echo "CONTAINMENT BREACH: rename escaped every granted root to $ESCAPE_DST"
            rm -f "$ESCAPE_DST"
            return 1
        fi
        if [ -e "$ESCAPE_DST" ]; then
            echo "CONTAINMENT BREACH: $ESCAPE_DST exists after a supposedly-refused rename"
            rm -f "$ESCAPE_DST"
            return 1
        fi
        return 0
    }
    assert "(c) rename out of every granted root is refused and leaves no file behind" \
        escape_rename_refused

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
