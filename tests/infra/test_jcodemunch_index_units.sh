#!/usr/bin/env bash
# tests/infra/test_jcodemunch_index_units.sh
# Hermetic tests for the reify-owned jcodemunch index-warming units + installer
# (task 6920, PRD docs/prds/jcodemunch-substrate-restoration.md tasks ζ and η).
#
# PATH-stubs `systemctl` to record argv to a CALLS_FILE; XDG_CONFIG_HOME is
# overridden to a fresh temp dir so installs never touch the real ~/.config.
# Nothing here needs a live --user bus, so this suite classifies `pool`.
#
# Blocks:
#   A — tracked service unit (deploy/systemd/reify-jcodemunch-index.service)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

SERVICE_SRC="$REPO_ROOT/deploy/systemd/reify-jcodemunch-index.service"
TIMER_SRC="$REPO_ROOT/deploy/systemd/reify-jcodemunch-index.timer"
INSTALLER="$REPO_ROOT/scripts/install-jcodemunch-index-units.sh"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"
SMOKE="$REPO_ROOT/scripts/smoke-jcodemunch-serve.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== jcodemunch index-warming units + installer (task 6920) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-jc-index-units-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-jc-index-units-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-jc-index-units-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

reset_calls() {
    > "$CALLS_FILE"
}

# ── systemctl stub (default: bus present, all subcommands exit 0) ─────────────
# REIFY_TEST_NO_USER_BUS=1 makes `show-environment` exit 1, which is exactly the
# probe the installer's fail-open branch uses.
cat > "$STUB_DIR/systemctl" << 'STUB_EOF'
#!/usr/bin/env bash
echo "systemctl $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_NO_USER_BUS:-0}" = "1" ]; then
    for _arg in "$@"; do
        [ "$_arg" = "show-environment" ] && exit 1
    done
fi
exit 0
STUB_EOF
chmod +x "$STUB_DIR/systemctl"

# ── run_installer: stub PATH + throwaway XDG_CONFIG_HOME, capturing OUT/ERR/RC ─
# The XDG_CONFIG_HOME argument is MANDATORY and must be non-empty: an empty value
# would let the installer's `${XDG_CONFIG_HOME:-$HOME/.config}` fall through to
# the real home and install units onto the developer's host from a test run.
run_installer() {
    local xdg="${1:-}"
    [ -n "$xdg" ] || { echo "run_installer: XDG_CONFIG_HOME argument is required" >&2; return 99; }
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        XDG_CONFIG_HOME="$xdg" \
        PATH="$STUB_DIR:$PATH" \
            bash "$INSTALLER" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — tracked service unit
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: tracked service unit ---"

# A1: the unit file exists
assert "A1: deploy/systemd/reify-jcodemunch-index.service exists" \
    test -f "$SERVICE_SRC"

# A2: [Unit] + [Service] sections and a Documentation=file:// pointer
assert "A2: service has [Unit], [Service] and a Documentation=file:// line" \
    bash -c '
        grep -q "^\[Unit\]$"            "$1" || exit 1
        grep -q "^\[Service\]$"         "$1" || exit 1
        grep -q "^Documentation=file://" "$1" || exit 1
    ' _ "$SERVICE_SRC"

# A3: oneshot — the timer drives it; it must not linger as a daemon
assert "A3: service declares Type=oneshot" \
    bash -c 'grep -q "^Type=oneshot$" "$1"' _ "$SERVICE_SRC"

# A4: exactly ONE ExecStart=, and it is the bare script with no trailing flags.
# Guards the "invoke the script, never re-derive its command" invariant
# (esc-6107-7): scripts/jcodemunch-index-reify.sh owns the project root and the
# identity lever, so any flag pinned here would be a second, drifting copy.
assert "A4: exactly one ExecStart=, bare scripts/jcodemunch-index-reify.sh with no flags" \
    bash -c '
        n=$(grep -c "^ExecStart=" "$1")
        [ "$n" = "1" ] || exit 1
        line=$(grep "^ExecStart=" "$1")
        [ "$line" = "ExecStart=/home/leo/src/reify/scripts/jcodemunch-index-reify.sh" ]
    ' _ "$SERVICE_SRC"

# A5: no re-derived argv anywhere in the unit — the failure A4 exists to pin,
# restated as a whole-file token ban so it cannot creep back in via a comment
# that a later edit promotes to a directive.
assert "A5: service names no uvx / watch / --once / jcodemunch-mcp token" \
    bash -c '
        ! grep -qE -- "uvx|watch|--once|jcodemunch-mcp" "$1"
    ' _ "$SERVICE_SRC"

# A6: the JCODEMUNCH_GIT_ROOT_IDENTITY lever belongs to the script alone.
# Two copies drift, and a drifted copy silently indexes the wrong repo identity.
assert "A6: service carries no Environment= line for JCODEMUNCH_GIT_ROOT_IDENTITY" \
    bash -c '
        ! grep -E "^Environment=" "$1" | grep -q "JCODEMUNCH_GIT_ROOT_IDENTITY"
    ' _ "$SERVICE_SRC"

# A7: output is journal-captured on both streams, matching reify-warm-lane-gc.service
assert "A7: service sets StandardOutput=journal and StandardError=journal" \
    bash -c '
        grep -q "^StandardOutput=journal$" "$1" || exit 1
        grep -q "^StandardError=journal$"  "$1" || exit 1
    ' _ "$SERVICE_SRC"

# A8: the ExecStart target must actually exist and be executable. The unit names
# the main checkout by absolute path (host convention), so resolve it relative to
# THIS repo root — that keeps the assertion true inside a warm-lane worktree while
# still proving the unit can never name a script that does not exist.
assert "A8: ExecStart target exists and is executable (resolved against this repo root)" \
    bash -c '
        line=$(grep "^ExecStart=" "$1")
        abs=${line#ExecStart=}
        rel=${abs#/home/leo/src/reify/}
        [ "$rel" != "$abs" ] || exit 1
        test -x "$2/$rel"
    ' _ "$SERVICE_SRC" "$REPO_ROOT"

test_summary
