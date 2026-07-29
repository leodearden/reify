#!/usr/bin/env bash
# tests/infra/test_verify_ld_library_path_scope.sh — task 5730.
#
# Guards the SCOPE of the OCCT loader path that scripts/verify.sh apply_env()
# exports (esc-4581-87, task 5321).
#
# The hazard: /opt/reify-deps/lib is not an OCCT lib dir, it is a whole conda
# prefix. Alongside the ~153 libTK* it carries libcrypto.so.3, libcurl.so.4,
# libexpat.so.1, libz.so.1, libcairo.so.2, libEGL.so.1, libsqlite3.so.0, ... —
# hundreds of system sonames (477 measured 2026-07-28 by intersecting its
# .so-bearing filenames with /usr/lib/x86_64-linux-gnu; unversioned host state
# that drifts on every environment refresh, so treat that as a dated
# measurement, not an invariant). LD_LIBRARY_PATH is searched BEFORE DT_RUNPATH
# and before ld.so.cache, so a process-wide export hands every one of those
# libraries to EVERY subprocess of the gate. sqlite3 is merely the one that
# self-checks its header/source hash and aborts loudly; the rest fail silently.
#
# The fix this guards: apply_env() captures the pre-OCCT ambient loader path
# into REIFY_AMBIENT_LD_LIBRARY_PATH, and non-cargo ("tool") plan lines are
# emitted through add_tool(), which prefixes them with a scrub statement
# restoring that ambient. Rust/cargo plan lines keep the OCCT export untouched
# — the .cargo/config.toml `runner` and the DT_RUNPATH baked into every bin/test
# binary are the primary mechanisms there, so tool lines lose nothing.
#
# Oracle: verify.sh --print-plan (hermetic — never runs cargo/npm/tests), plus a
# read-only, host-gated `sqlite3 --version` probe for the behavioural half.
# Shape mirrors tests/infra/test_verify_failfast_order.sh and
# tests/infra/test_run_all_ambient_isolation.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"
[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

echo "=== verify.sh LD_LIBRARY_PATH scope guard (task 5730) ==="

# Fork-free "haystack does NOT contain needle" predicate. A function (not a
# pipeline) so `assert` runs it in this shell with only a redirect, and so no
# pipe/subshell EINTR surface is introduced (esc-4574-42).
not_contains() {
    case "$1" in
        *"$2"*) return 1 ;;
        *)      return 0 ;;
    esac
}
contains() {
    case "$1" in
        *"$2"*) return 0 ;;
        *)      return 1 ;;
    esac
}

# ---------------------------------------------------------------------------
# Capture: merge-role plan. DF_VERIFY_ROLE=merge selects the tier carrying the
# run_all.sh pool line; REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 keeps the
# nextest-availability probe fast. env -u REIFY_INFRA_SUITE_ACTIVE is
# belt-and-braces for an in-pool capture (mirrors
# test_run_all_ambient_isolation.sh:153-155).
# ---------------------------------------------------------------------------
PLAN_DUMP=""
capture_print_plan PLAN_DUMP 3 \
    env -u REIFY_INFRA_SUITE_ACTIVE DF_VERIFY_ROLE=merge REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan || true

echo ""
echo "--- Section A: --print-plan environment block ---"

# Non-vacuity. verify.sh prints each ENV_LINES entry as "# <entry>", so the
# OCCT export appears as "# export LD_LIBRARY_PATH=". Asserting it FIRST means a
# plan-format change fails loudly here rather than silently emptying the
# extractions below.
assert "env block still carries the OCCT '# export LD_LIBRARY_PATH=' line (non-vacuity: a plan-format change must fail HERE, not silently empty the extractions below)" \
    plan_match "$PLAN_DUMP" '^# export LD_LIBRARY_PATH='

# Extract the ambient-capture line, fork-free (no sed/awk/pipe/subshell).
AMBIENT_ENV_LINE=""
while IFS= read -r _line; do
    case "$_line" in
        "# export REIFY_AMBIENT_LD_LIBRARY_PATH="*) AMBIENT_ENV_LINE="$_line" ;;
    esac
done <<< "$PLAN_DUMP"

assert "env block carries '# export REIFY_AMBIENT_LD_LIBRARY_PATH=' (apply_env() captures the pre-OCCT loader path as the single source of truth for restoring a clean one)" \
    test -n "$AMBIENT_ENV_LINE"

# The whole point of the captured value: it is the loader path as it stood on
# ENTRY to apply_env(), i.e. BEFORE either OCCT prepend. If the conda prefix
# leaked into it, the capture ran too late and every scrub built from it would
# be a no-op.
assert "captured ambient value does NOT contain /opt/reify-deps/lib (capture must happen BEFORE the OCCT prepends, else every scrub built from it is a no-op)" \
    not_contains "$AMBIENT_ENV_LINE" '/opt/reify-deps/lib'

assert "captured ambient value does NOT contain the snap OCCT dir either (same reason)" \
    not_contains "$AMBIENT_ENV_LINE" '/snap/freecad/current/usr/lib'

# ---------------------------------------------------------------------------
# Section B: behavioural half — the original bite, host-gated.
#
# Dual-conditional by nature: it reproduces only where BOTH the conda
# libsqlite3 exists AND the resolved sqlite3 CLI links a DIFFERENT version. A
# dev shell with a matching (or differently-ordered) sqlite3 legitimately
# passes, so absence of either precondition is a SKIP, never a FAIL.
#
# The gate is computed STRUCTURALLY (compare the deps soname's version suffix
# against the CLI's own reported version) rather than by "run the hostile probe
# and skip if it succeeded" — the latter would be circular and would silently
# vacuify the assertion on exactly the hosts that matter.
#
# rc-masking hazard (found at revalidation): do NOT probe through a pipeline.
# `sqlite3 --version 2>&1 | head` yields *head's* rc, which is 0 even when
# sqlite3 aborts. Both probes below are unpiped command substitutions, and the
# hostile one asserts on rc AND on the message substring, so neither a future rc
# change nor a message rewording can quietly turn this green.
# ---------------------------------------------------------------------------
echo ""
echo "--- Section B: behavioural reproduction (host-gated) ---"

DEPS_SQLITE_SO="/opt/reify-deps/lib/libsqlite3.so.0"
_skip_reason=""

if [ ! -e "$DEPS_SQLITE_SO" ]; then
    _skip_reason="$DEPS_SQLITE_SO absent (no conda libsqlite3 to shadow with)"
elif ! command -v sqlite3 >/dev/null 2>&1; then
    _skip_reason="no sqlite3 CLI on PATH"
fi

SCRUBBED_OUT=""
SCRUBBED_RC=0
if [ -z "$_skip_reason" ]; then
    SCRUBBED_OUT="$(LD_LIBRARY_PATH="" sqlite3 --version 2>&1)" || SCRUBBED_RC=$?

    # Deps soname version: readlink libsqlite3.so.0 -> libsqlite3.so.3.53.1 -> 3.53.1
    _deps_target="$(readlink "$DEPS_SQLITE_SO" 2>/dev/null || true)"
    _deps_ver="${_deps_target#libsqlite3.so.}"
    # CLI version: first whitespace-delimited field of `sqlite3 --version`.
    _cli_ver="${SCRUBBED_OUT%% *}"

    if [ "$SCRUBBED_RC" -ne 0 ]; then
        _skip_reason="sqlite3 --version fails even with a scrubbed loader path (rc=$SCRUBBED_RC) — host sqlite3 is broken independently of this leak"
    elif [ -z "$_deps_ver" ] || [ "$_deps_ver" = "$_deps_target" ]; then
        _skip_reason="could not derive a version from $DEPS_SQLITE_SO (target='${_deps_target:-<none>}')"
    elif [ "$_deps_ver" = "$_cli_ver" ]; then
        _skip_reason="deps libsqlite3 ($_deps_ver) matches the CLI's own version ($_cli_ver) — no header/source mismatch is possible on this host"
    fi
fi

if [ -n "$_skip_reason" ]; then
    echo "  SKIP: behavioural reproduction — $_skip_reason"
else
    echo "  (host gate open: deps libsqlite3 $_deps_ver vs CLI sqlite3 $_cli_ver)"

    assert "scrubbed loader path: 'sqlite3 --version' succeeds (rc=0)" \
        test "$SCRUBBED_RC" -eq 0

    HOSTILE_OUT=""
    HOSTILE_RC=0
    HOSTILE_OUT="$(LD_LIBRARY_PATH=/opt/reify-deps/lib sqlite3 --version 2>&1)" || HOSTILE_RC=$?

    assert "hostile loader path (/opt/reify-deps/lib): 'sqlite3 --version' FAILS with non-zero rc — this is the esc-4581-87 / task-5321 bite, captured unpiped so no pipeline rc masks it" \
        test "$HOSTILE_RC" -ne 0

    assert "hostile loader path: failure is specifically 'SQLite header and source version mismatch' (asserted alongside rc so a message rewording cannot quietly turn this green)" \
        contains "$HOSTILE_OUT" 'header and source version mismatch'
fi

test_summary
