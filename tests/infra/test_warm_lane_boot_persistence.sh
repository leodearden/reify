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


# ──────────────────────────────────────────────────────────────────────────────
# Block B — tracked orchestrator drop-in
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: tracked orchestrator drop-in ---"

# B1: drop-in file exists at the correct systemd drop-in location
assert "B1: deploy/systemd/orchestrator-reify.service.d/warm-lane.conf exists" \
    test -f "$DROPIN_SRC"

# B2: drop-in contains Wants=reify-warm-lane.service (soft pull-in, fail-open)
assert "B2: drop-in contains Wants=reify-warm-lane.service" \
    bash -c 'grep -q "^Wants=reify-warm-lane.service$" "$1"' _ "$DROPIN_SRC"

# B3: drop-in contains After=reify-warm-lane.service (ordering)
assert "B3: drop-in contains After=reify-warm-lane.service" \
    bash -c 'grep -q "^After=reify-warm-lane.service$" "$1"' _ "$DROPIN_SRC"

# B4: drop-in does NOT contain Requires=reify-warm-lane.service (fail-open DA5/inv.6)
assert "B4: drop-in does NOT contain Requires=reify-warm-lane.service (fail-open)" \
    bash -c '! grep -q "^Requires=reify-warm-lane.service$" "$1"' _ "$DROPIN_SRC"


# ──────────────────────────────────────────────────────────────────────────────
# Block C — installer happy-path
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: installer happy-path ---"

C_XDG="$(mktemp -d /tmp/test-warm-lane-persist-c-xdg-XXXXXX)"
_TMPDIRS+=("$C_XDG")

reset_calls
run_installer "$C_XDG"

# C1: installer exits 0
assert "C1: installer exits 0" test "$RC" -eq 0

# C2: unit file installed to correct path under XDG_CONFIG_HOME
assert "C2: unit installed at \$XDG_CONFIG_HOME/systemd/user/reify-warm-lane.service" \
    test -f "$C_XDG/systemd/user/reify-warm-lane.service"

# C3: drop-in installed to correct path
assert "C3: drop-in installed at \$XDG_CONFIG_HOME/systemd/user/orchestrator-reify.service.d/warm-lane.conf" \
    test -f "$C_XDG/systemd/user/orchestrator-reify.service.d/warm-lane.conf"

# C4: systemctl --user daemon-reload was called
assert "C4: systemctl --user daemon-reload was called" \
    bash -c 'grep -q "systemctl --user daemon-reload" "$1"' _ "$CALLS_FILE"

# C5: systemctl --user enable reify-warm-lane.service was called
assert "C5: systemctl --user enable reify-warm-lane.service was called" \
    bash -c 'grep -q "systemctl --user enable reify-warm-lane.service" "$1"' _ "$CALLS_FILE"

# C6: daemon-reload precedes enable (line-order check)
assert "C6: daemon-reload precedes enable in call order" \
    bash -c '
        reload_ln=$(grep -n "daemon-reload" "$1" | head -1 | cut -d: -f1)
        enable_ln=$(grep -n "enable reify-warm-lane.service" "$1" | head -1 | cut -d: -f1)
        [ -n "$reload_ln" ] && [ -n "$enable_ln" ] && [ "$reload_ln" -lt "$enable_ln" ]
    ' _ "$CALLS_FILE"


# ──────────────────────────────────────────────────────────────────────────────
# Block D — installer fail-open + idempotence
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: installer fail-open + idempotence ---"

# D — fail-open: no systemd --user bus
D_XDG_NOBUS="$(mktemp -d /tmp/test-warm-lane-persist-d-nobus-XXXXXX)"
_TMPDIRS+=("$D_XDG_NOBUS")

reset_calls
REIFY_TEST_NO_USER_BUS=1 run_installer "$D_XDG_NOBUS"

# D1: installer exits 0 even with no bus (fail-open / non-fatal)
assert "D1: installer exits 0 with no systemd --user bus (fail-open)" \
    test "$RC" -eq 0

# D2: stderr/stdout contains a warn/skip message mentioning the missing bus
assert "D2: installer warns about missing bus" \
    bash -c 'printf "%s\n" "$1" "$2" | grep -qiE "warn|skip|no systemd|no.*bus|missing"' _ "$ERR_OUT" "$OUT"

# D3: NO systemctl --user enable was called (bus-dependent steps skipped)
assert "D3: NO systemctl --user enable called when bus is absent" \
    bash -c '! grep -q "enable" "$1"' _ "$CALLS_FILE"

# D — idempotence: second run against same XDG_CONFIG_HOME exits 0 + files remain
D_XDG_IDEM="$(mktemp -d /tmp/test-warm-lane-persist-d-idem-XXXXXX)"
_TMPDIRS+=("$D_XDG_IDEM")

reset_calls
run_installer "$D_XDG_IDEM"
reset_calls
run_installer "$D_XDG_IDEM"

# D4: second run exits 0
assert "D4: second installer run exits 0 (idempotent)" \
    test "$RC" -eq 0

# D5: unit file still present after second run
assert "D5: unit file present after second installer run" \
    test -f "$D_XDG_IDEM/systemd/user/reify-warm-lane.service"

# D5b: drop-in still present after second run
assert "D5b: drop-in present after second installer run" \
    test -f "$D_XDG_IDEM/systemd/user/orchestrator-reify.service.d/warm-lane.conf"

# D6 — pre-flight: missing source files → non-zero exit (fail-CLOSED, not fail-open)
# This exercises the one intentional hard-fail path in the installer (exit 1 when
# the tracked unit/drop-in files are absent), distinguishing it from the fail-open
# no-bus path (which exits 0).  Use REIFY_TEST_REPO_ROOT to point the installer at
# an empty temp tree that lacks deploy/systemd/.
D_REPO_PF="$(mktemp -d /tmp/test-warm-lane-persist-d-pf-XXXXXX)"
_TMPDIRS+=("$D_REPO_PF")
D_XDG_PF="$(mktemp -d /tmp/test-warm-lane-persist-d-pf-xdg-XXXXXX)"
_TMPDIRS+=("$D_XDG_PF")

reset_calls
REIFY_TEST_REPO_ROOT="$D_REPO_PF" run_installer "$D_XDG_PF"

assert "D6: installer exits non-zero when source unit files are absent (fail-closed pre-flight)" \
    test "$RC" -ne 0


# ──────────────────────────────────────────────────────────────────────────────
# Block F — installer pins explicit ExecStart flags (task #4720)
# Asserts that the INSTALLED unit (not the tracked source) carries the three
# pinned flags: --img, --size-gib, --mount.  Also asserts idempotence: running
# the installer twice produces exactly ONE --img occurrence (no double-append).
# Regression guard: Block A (tracked unit bare ExecStart) and Blocks C/D must
# stay GREEN — the tracked unit is never modified.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: installer pins explicit ExecStart flags ---"

F_XDG="$(mktemp -d /tmp/test-warm-lane-persist-f-xdg-XXXXXX)"
_TMPDIRS+=("$F_XDG")

reset_calls
run_installer "$F_XDG"

# F1: installer exits 0
assert "F1: installer exits 0" test "$RC" -eq 0

# F2: installed unit ExecStart carries --img with the pinned NVMe path
assert "F2: installed unit ExecStart carries --img /media/leo/data_lv_1/leo/reify-warm-lanes.img" \
    bash -c 'grep "^ExecStart=" "$1/systemd/user/reify-warm-lane.service" \
             | grep -qF -- "--img /media/leo/data_lv_1/leo/reify-warm-lanes.img"' _ "$F_XDG"

# F3: installed unit ExecStart carries --size-gib 4096
assert "F3: installed unit ExecStart carries --size-gib 4096" \
    bash -c 'grep "^ExecStart=" "$1/systemd/user/reify-warm-lane.service" \
             | grep -qF -- "--size-gib 4096"' _ "$F_XDG"

# F4: installed unit ExecStart carries --mount /home/leo/src/warm-lanes
assert "F4: installed unit ExecStart carries --mount /home/leo/src/warm-lanes" \
    bash -c 'grep "^ExecStart=" "$1/systemd/user/reify-warm-lane.service" \
             | grep -qF -- "--mount /home/leo/src/warm-lanes"' _ "$F_XDG"

# F5 (idempotence): run installer a second time — ExecStart must have exactly ONE --img
F_XDG2="$(mktemp -d /tmp/test-warm-lane-persist-f2-xdg-XXXXXX)"
_TMPDIRS+=("$F_XDG2")

reset_calls
run_installer "$F_XDG2"
reset_calls
run_installer "$F_XDG2"

assert "F5: re-install is idempotent: ExecStart has exactly one --img occurrence" \
    bash -c '
        count=$(grep "^ExecStart=" "$1/systemd/user/reify-warm-lane.service" \
                | grep -o -- "--img" | wc -l)
        [ "$count" -eq 1 ]
    ' _ "$F_XDG2"


# ──────────────────────────────────────────────────────────────────────────────
# Block E — setup-dev.sh wiring (structural grep)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: setup-dev.sh wiring ---"

# E1: setup-dev.sh references install-warm-lane-units.sh
assert "E1: setup-dev.sh references install-warm-lane-units.sh" \
    bash -c 'grep -q "install-warm-lane-units.sh" "$1"' _ "$SETUP_DEV"

# E2 (tighter): the install-warm-lane-units.sh call must be inside the REIFY_PROVISION_WARM_LANES
# then-branch specifically — not just anywhere before fi.  Strategy: find the gate open
# (if.*REIFY_PROVISION_WARM_LANES), the outer fi (first unindented ^fi$ after gate), and the
# outer else (first unindented ^else$ between gate and fi).  The install call must fall between
# gate_ln and else_ln (= the then-branch boundary), or between gate_ln and fi_ln when no else.
assert "E2: install-warm-lane-units.sh call is inside REIFY_PROVISION_WARM_LANES then-branch" \
    bash -c '
        gate_ln=$(grep -n "if.*REIFY_PROVISION_WARM_LANES" "$1" | head -1 | cut -d: -f1)
        install_ln=$(grep -n "install-warm-lane-units.sh" "$1" | head -1 | cut -d: -f1)
        # Outer fi: first unindented ^fi$ after gate (inner fi lines are indented)
        fi_ln=$(awk "NR > ${gate_ln:-0} && /^fi$/ { print NR; exit }" "$1")
        # Outer else: first unindented ^else$ between gate and fi
        else_ln=$(awk "NR > ${gate_ln:-0} && NR < ${fi_ln:-999999} && /^else$/ { print NR; exit }" "$1")
        # install must be after gate and before else (or fi if there is no else)
        boundary=${else_ln:-$fi_ln}
        [ -n "$gate_ln" ] && [ -n "$install_ln" ] && [ -n "$boundary" ] \
            && [ "$install_ln" -gt "$gate_ln" ] && [ "$install_ln" -lt "$boundary" ]
    ' _ "$SETUP_DEV"

# E3: the install-warm-lane-units.sh invocation is non-fatal (else+warn, no bare exit 1
# in the surrounding 8-line context, matching the F3 pattern from provision test)
assert "E3: install-warm-lane-units.sh call is non-fatal (else+warn, no bare exit 1)" \
    bash -c '
        block=$(grep -A8 "install-warm-lane-units.sh" "$1")
        echo "$block" | grep -q "else" || exit 1
        echo "$block" | grep -q "warn" || exit 1
        ! echo "$block" | grep -qE "^[[:space:]]*(exit[[:space:]]+1)[[:space:]]*$" || exit 1
        exit 0
    ' _ "$SETUP_DEV"

# E4: regression guard — setup-dev.sh STILL references provision-warm-lane-fs.sh
# (so #4659 Block F F1 stays green)
assert "E4: setup-dev.sh still references provision-warm-lane-fs.sh (regression guard)" \
    bash -c 'grep -q "provision-warm-lane-fs.sh" "$1"' _ "$SETUP_DEV"

test_summary
