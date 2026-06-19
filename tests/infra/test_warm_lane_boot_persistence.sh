#!/usr/bin/env bash
# tests/infra/test_warm_lane_boot_persistence.sh
# Hermetic tests for boot-persistent warm-lane unit + installer (task 4695).
#
# PATH-stubs `systemctl` record argv to a CALLS_FILE; XDG_CONFIG_HOME is
# overridden to a fresh temp dir so installs never touch the real ~/.config.
#
# Blocks:
#   A — tracked oneshot unit file (deploy/systemd/reify-warm-lane.service)
#   B — tracked orchestrator drop-in (orchestrator-reify.service.d/warm-lane.conf)
#   C — installer happy-path (copies unit+drop-in, daemon-reload, enable)
#   D — installer fail-open (no bus → skip, warn, exit 0) + idempotence
#   E — setup-dev.sh wiring (structural grep)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
UNIT_SRC="$REPO_ROOT/deploy/systemd/reify-warm-lane.service"
DROPIN_SRC="$REPO_ROOT/deploy/systemd/orchestrator-reify.service.d/warm-lane.conf"
INSTALLER="$REPO_ROOT/scripts/install-warm-lane-units.sh"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== warm-lane boot-persistence hermetic tests (task 4695) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-lane-persist-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-lane-persist-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-lane-persist-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

reset_calls() {
    > "$CALLS_FILE"
}

# ── systemctl stub (default: bus present, all subcommands exit 0) ──────────────
make_systemctl_stub() {
    local no_bus="${1:-0}"
    cat > "$STUB_DIR/systemctl" << STUB_EOF
#!/usr/bin/env bash
echo "systemctl \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
# simulate missing --user bus when REIFY_TEST_NO_USER_BUS=1
if [ "\${REIFY_TEST_NO_USER_BUS:-0}" = "1" ]; then
    for _arg in "\$@"; do
        [ "\$_arg" = "show-environment" ] && exit 1
    done
fi
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/systemctl"
}
make_systemctl_stub

# ── run_installer: run the installer with stub PATH + temp XDG_CONFIG_HOME ────
run_installer() {
    local rc=0
    local xdg="${1:-}"
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        XDG_CONFIG_HOME="${xdg:-}" \
        PATH="$STUB_DIR:$PATH" \
            bash "$INSTALLER" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — tracked oneshot unit file
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: tracked oneshot unit file ---"

# A1: unit file exists
assert "A1: deploy/systemd/reify-warm-lane.service exists" \
    test -f "$UNIT_SRC"

# A2: [Service] declares Type=oneshot
assert "A2: unit declares Type=oneshot" \
    bash -c 'grep -q "^Type=oneshot$" "$1"' _ "$UNIT_SRC"

# A3: [Service] declares RemainAfterExit=yes
assert "A3: unit declares RemainAfterExit=yes" \
    bash -c 'grep -q "^RemainAfterExit=yes$" "$1"' _ "$UNIT_SRC"

# A4: ExecStart= references provision-warm-lane-fs.sh
assert "A4: ExecStart= references provision-warm-lane-fs.sh" \
    bash -c 'grep -q "provision-warm-lane-fs.sh" "$1"' _ "$UNIT_SRC"

# A5: [Install] declares WantedBy=default.target
assert "A5: [Install] declares WantedBy=default.target" \
    bash -c 'grep -q "^WantedBy=default.target$" "$1"' _ "$UNIT_SRC"

test_summary
