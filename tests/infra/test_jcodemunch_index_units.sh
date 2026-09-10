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
#   B — tracked timer unit   (deploy/systemd/reify-jcodemunch-index.timer)
#   C — installer happy path, idempotence, and the watcher guardrail
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


# ──────────────────────────────────────────────────────────────────────────────
# Block B — tracked timer unit
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: tracked timer unit ---"

# B1: the timer file exists
assert "B1: deploy/systemd/reify-jcodemunch-index.timer exists" \
    test -f "$TIMER_SRC"

# B2: [Unit] + [Timer] + [Install] sections
assert "B2: timer has [Unit], [Timer] and [Install] sections" \
    bash -c '
        grep -q "^\[Unit\]$"    "$1" || exit 1
        grep -q "^\[Timer\]$"   "$1" || exit 1
        grep -q "^\[Install\]$" "$1" || exit 1
    ' _ "$TIMER_SRC"

# B3: an OnCalendar= schedule. The `daily` VALUE is an operational tunable —
# same stance as reify-warm-lane-gc.timer's "tests assert only the directive
# presence" comment — but the directive's PRESENCE is the contract, because
# B4 below depends on the schedule being calendar-based rather than monotonic.
assert "B3: timer carries an OnCalendar= directive (value 'daily' is a tunable, presence is the contract)" \
    bash -c 'grep -q "^OnCalendar=daily$" "$1"' _ "$TIMER_SRC"

# B4: Persistent=true AND a calendar schedule, asserted as a PAIR.
# systemd.timer(5): "Persistent= only has an effect on timers configured with
# OnCalendar=" — so Persistent=true on a monotonic schedule is inert, and
# either half alone would pass while the catch-up behaviour silently did not
# exist. Catch-up is the point: a missed tick after host downtime is exactly
# the stale index this timer is here to prevent.
assert "B4: Persistent=true is paired with an OnCalendar= schedule (Persistent= is inert on a monotonic one)" \
    bash -c '
        grep -q "^Persistent=true$" "$1" || exit 1
        grep -q "^OnCalendar="      "$1" || exit 1
    ' _ "$TIMER_SRC"

# B5: the timer drives the service unit this diff also ships
assert "B5: timer sets Unit=reify-jcodemunch-index.service" \
    bash -c 'grep -q "^Unit=reify-jcodemunch-index.service$" "$1"' _ "$TIMER_SRC"

# B6: that Unit= target exists as a tracked file — no dangling reference.
# A timer pointing at a unit nobody ships is precisely the defect task η is
# retiring elsewhere in this same diff; this keeps the new pair from repeating it.
assert "B6: the Unit= target exists as a tracked file under deploy/systemd/" \
    bash -c '
        line=$(grep "^Unit=" "$1")
        target=${line#Unit=}
        test -f "$2/deploy/systemd/$target"
    ' _ "$TIMER_SRC" "$REPO_ROOT"

# B7: [Install] wires the timer into timers.target so `enable` has an effect
assert "B7: [Install] declares WantedBy=timers.target" \
    bash -c 'grep -q "^WantedBy=timers.target$" "$1"' _ "$TIMER_SRC"

# B8: both unit basenames carry the reify- prefix, so neither can ever be
# confused with — or shadow — the host's jcodemunch-OWNED units
# (jcodemunch-index-gc.timer, jcodemunch-watcher.service), which serve other
# repos and must never be touched by anything in this repo.
assert "B8: both unit basenames carry the reify- prefix (never shadow jcodemunch's own units)" \
    bash -c '
        case "$(basename "$1")" in reify-*) ;; *) exit 1 ;; esac
        case "$(basename "$2")" in reify-*) ;; *) exit 1 ;; esac
    ' _ "$TIMER_SRC" "$SERVICE_SRC"


# ──────────────────────────────────────────────────────────────────────────────
# Block C — installer happy path, idempotence, and the watcher guardrail
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: installer happy path ---"

# C1: the installer exists and is executable
assert "C1: scripts/install-jcodemunch-index-units.sh exists and is executable" \
    test -x "$INSTALLER"

C_XDG="$(mktemp -d /tmp/test-jc-index-units-c-xdg-XXXXXX)"
_TMPDIRS+=("$C_XDG")

reset_calls
run_installer "$C_XDG"

# C2: a clean run succeeds
assert "C2: installer exits 0 on the happy path" \
    test "$RC" = "0"

# C3/C4: the INSTALLED copies are byte-identical to the tracked sources.
# `cmp` rather than "file exists": this is the assertion that fails the moment
# anyone reintroduces a sed ExecStart rewrite of the kind
# install-warm-lane-units.sh performs. These units have no host-specific value
# to pin, so a rewritten installed copy could only be drift.
assert "C3: installed .service is byte-identical to the tracked source (no ExecStart rewrite)" \
    cmp -s "$SERVICE_SRC" "$C_XDG/systemd/user/reify-jcodemunch-index.service"

assert "C4: installed .timer is byte-identical to the tracked source (no rewrite)" \
    cmp -s "$TIMER_SRC" "$C_XDG/systemd/user/reify-jcodemunch-index.timer"

# C5: the manager is told to re-read the units it was just handed
assert "C5: installer runs systemctl --user daemon-reload" \
    bash -c 'grep -q "^systemctl --user daemon-reload" "$1"' _ "$CALLS_FILE"

# C6: the TIMER is what gets enabled (--now accepted but not required) —
# enabling the .service directly would give it an untimed activation path.
assert "C6: installer enables reify-jcodemunch-index.timer" \
    bash -c '
        grep "^systemctl --user enable" "$1" | grep -q "reify-jcodemunch-index.timer"
    ' _ "$CALLS_FILE"

# C7: idempotence — a second run against the same XDG_CONFIG_HOME must succeed
# and leave both installed copies still byte-identical to the tracked sources.
reset_calls
run_installer "$C_XDG"

assert "C7: a second run exits 0 and leaves both installed copies byte-identical (idempotent)" \
    bash -c '
        [ "$1" = "0" ] || exit 1
        cmp -s "$2" "$4/systemd/user/reify-jcodemunch-index.service" || exit 1
        cmp -s "$3" "$4/systemd/user/reify-jcodemunch-index.timer"   || exit 1
    ' _ "$RC" "$SERVICE_SRC" "$TIMER_SRC" "$C_XDG"

# C8: watcher guardrail. jcodemunch-watcher.service is `enabled enabled` on this
# host and serves five other repos. This install path must be incapable of
# disabling, stopping or overwriting it — asserted both on what the run actually
# did (no systemctl call names it) and on the installer's own source text (the
# token does not appear at all, so no future branch can reach it either).
assert "C8: no systemctl call from the installer names jcodemunch-watcher" \
    bash -c '! grep -q "jcodemunch-watcher" "$1"' _ "$CALLS_FILE"

assert "C8b: the installer source contains no jcodemunch-watcher token at all" \
    bash -c '! grep -q "jcodemunch-watcher" "$1"' _ "$INSTALLER"

test_summary
