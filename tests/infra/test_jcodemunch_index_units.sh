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
#   D — installer CLI guard, source pre-flight, and fail-open
#   E — repo-side retirement invariants for the old serve unit (task η)
#   F — smoke-script connection-failure hint contract
#   H — smoke-script query-budget contract (sits with F; both cover $SMOKE)
#   G — setup-dev.sh wiring (structural grep, no execution)
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

# ── loginctl stub (default: lingering OFF) ────────────────────────────────────
# Stubbed rather than left to the host so the linger advisory is deterministic:
# the real loginctl answers whatever THIS developer's account happens to be set
# to, which would make the assertion below pass or fail by accident. Deliberately
# does NOT append to CALLS_FILE — that file is the systemctl argv ledger, and
# `enable-linger` contains "enable", which would collide with D5's assertion that
# no `enable` was attempted.
cat > "$STUB_DIR/loginctl" << 'STUB_EOF'
#!/usr/bin/env bash
[ "${1:-}" = "show-user" ] && { echo "${REIFY_TEST_LINGER:-no}"; exit 0; }
exit 0
STUB_EOF
chmod +x "$STUB_DIR/loginctl"

# ── run_installer <xdg> [args...] ─────────────────────────────────────────────
# Runs the installer with the stub PATH and a throwaway XDG_CONFIG_HOME,
# capturing OUT / ERR_OUT / RC. Any arguments after <xdg> are forwarded to the
# installer, so the CLI-guard cases share this one entry point.
#
# The XDG_CONFIG_HOME argument is MANDATORY and must be non-empty: an empty value
# would let the installer's `${XDG_CONFIG_HOME:-$HOME/.config}` fall through to
# the real home and install units onto the developer's host from a test run.
run_installer() {
    local xdg="${1:-}"
    [ -n "$xdg" ] || { echo "run_installer: XDG_CONFIG_HOME argument is required" >&2; return 99; }
    shift
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        XDG_CONFIG_HOME="$xdg" \
        PATH="$STUB_DIR:$PATH" \
            bash "$INSTALLER" "$@" 2>"$ERR_FILE"
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

# C9/C10: the linger advisory. A --user timer only fires while the user manager
# runs, so without lingering the daily pass silently never happens on an
# unattended host — the exact staleness this timer exists to prevent, and the one
# place this installer had already drifted from install-warm-lane-units.sh.
# Asserted in BOTH directions so the advisory cannot degrade into an always-on
# banner that operators learn to ignore.
C9_XDG="$(mktemp -d /tmp/test-jc-index-units-c9-xdg-XXXXXX)"
_TMPDIRS+=("$C9_XDG")

reset_calls
REIFY_TEST_LINGER=no run_installer "$C9_XDG"

# ADVISORY, not a gate: exit 0 and a fully-installed unit dir are asserted
# alongside the warning, so a future edit cannot promote this into a hard refusal.
assert "C9: lingering off → warns naming 'loginctl enable-linger', and still installs (exit 0)" \
    bash -c '
        [ "$1" = "0" ] || exit 1
        printf "%s" "$2" | grep -qi "linger" || exit 1
        printf "%s" "$2" | grep -q  "loginctl enable-linger" || exit 1
        [ -f "$3/systemd/user/reify-jcodemunch-index.timer" ]
    ' _ "$RC" "$ERR_OUT" "$C9_XDG"

C10_XDG="$(mktemp -d /tmp/test-jc-index-units-c10-xdg-XXXXXX)"
_TMPDIRS+=("$C10_XDG")

reset_calls
REIFY_TEST_LINGER=yes run_installer "$C10_XDG"

assert "C10: lingering on → exit 0 and NO linger warning (advisory is conditional)" \
    bash -c '
        [ "$1" = "0" ] || exit 1
        ! printf "%s" "$2" | grep -qi "linger"
    ' _ "$RC" "$ERR_OUT"


# ──────────────────────────────────────────────────────────────────────────────
# Block D — installer CLI guard, source pre-flight, and fail-open
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: installer CLI guard, pre-flight, fail-open ---"

D_XDG="$(mktemp -d /tmp/test-jc-index-units-d-xdg-XXXXXX)"
_TMPDIRS+=("$D_XDG")

# D1: both help spellings exit 0 and print a usage line naming the script
for _flag in --help -h; do
    reset_calls
    run_installer "$D_XDG" "$_flag"
    assert "D1: '$_flag' exits 0 and prints a usage line naming the script" \
        bash -c '
            [ "$1" = "0" ] || exit 1
            printf "%s" "$2" | grep -qi "usage" || exit 1
            printf "%s" "$2" | grep -q "install-jcodemunch-index-units.sh"
        ' _ "$RC" "$ERR_OUT$OUT"
done

# D2: an unexpected positional exits 2 and the message names the offending
# argument. Exit 2 specifically, matching install-warm-lane-units.sh — the two
# installers must not disagree on what a CLI misuse exit code means.
reset_calls
run_installer "$D_XDG" --frobnicate

assert "D2: unexpected argument exits 2 and the message names it" \
    bash -c '
        [ "$1" = "2" ] || exit 1
        printf "%s" "$2" | grep -q -- "--frobnicate"
    ' _ "$RC" "$ERR_OUT$OUT"

# ── D3/D4: source pre-flight. A missing tracked unit must fail LOUDLY and name
# the path; a silent skip here would "succeed" while installing nothing.
# Sets _PARTIAL_REPO rather than echoing the path: a `$(...)` call site would run
# this in a SUBSHELL, and the `_TMPDIRS+=` registration would die with it — the
# EXIT trap would then reclaim nothing and every suite run would leak three
# directories into /tmp. Assign through the global and call it as a statement.
_PARTIAL_REPO=""
_make_partial_repo() {
    # $1 = which source to omit ("service" or "timer"); sets $_PARTIAL_REPO
    local omit="$1" root
    root="$(mktemp -d /tmp/test-jc-index-units-pf-XXXXXX)"
    _TMPDIRS+=("$root")
    mkdir -p "$root/deploy/systemd"
    [ "$omit" = "service" ] || cp "$SERVICE_SRC" "$root/deploy/systemd/"
    [ "$omit" = "timer" ]   || cp "$TIMER_SRC"   "$root/deploy/systemd/"
    _PARTIAL_REPO="$root"
}

D3_XDG="$(mktemp -d /tmp/test-jc-index-units-d3-xdg-XXXXXX)"
_TMPDIRS+=("$D3_XDG")
_make_partial_repo service; D3_REPO="$_PARTIAL_REPO"

reset_calls
REIFY_TEST_REPO_ROOT="$D3_REPO" run_installer "$D3_XDG"

# The ERROR:-prefixed line is the discriminator, not just "the path appears in
# stderr": without an explicit pre-flight, `cp` failing under `set -e` also names
# the path, so a laxer assertion would pass on an installer that has no pre-flight
# at all and merely crashes partway through.
assert "D3: missing .service source exits 1 with an ERROR: line naming the missing path" \
    bash -c '
        [ "$1" = "1" ] || exit 1
        printf "%s" "$2" | grep -q "ERROR.*reify-jcodemunch-index[.]service"
    ' _ "$RC" "$ERR_OUT"

D4_XDG="$(mktemp -d /tmp/test-jc-index-units-d4-xdg-XXXXXX)"
_TMPDIRS+=("$D4_XDG")
_make_partial_repo timer; D4_REPO="$_PARTIAL_REPO"

reset_calls
REIFY_TEST_REPO_ROOT="$D4_REPO" run_installer "$D4_XDG"

assert "D4: missing .timer source exits 1 with an ERROR: line naming the missing path" \
    bash -c '
        [ "$1" = "1" ] || exit 1
        printf "%s" "$2" | grep -q "ERROR.*reify-jcodemunch-index[.]timer"
    ' _ "$RC" "$ERR_OUT"

# D5: fail-open on a bus-less host must be a GENUINE skip, not a half-install:
# exit 0 with a warning, nothing copied, and no daemon-reload/enable attempted.
# (The stub still records the show-environment probe itself, so this asserts on
# the two mutating verbs specifically rather than on an empty calls file.)
D5_XDG="$(mktemp -d /tmp/test-jc-index-units-d5-xdg-XXXXXX)"
_TMPDIRS+=("$D5_XDG")

reset_calls
REIFY_TEST_NO_USER_BUS=1 run_installer "$D5_XDG"

assert "D5: no --user bus → exit 0, WARN naming the bus, nothing copied, no daemon-reload/enable" \
    bash -c '
        [ "$1" = "0" ] || exit 1
        printf "%s" "$2" | grep -qi "warn" || exit 1
        printf "%s" "$2" | grep -qi "bus"  || exit 1
        [ ! -e "$3/systemd/user/reify-jcodemunch-index.service" ] || exit 1
        [ ! -e "$3/systemd/user/reify-jcodemunch-index.timer" ]   || exit 1
        ! grep -q "daemon-reload" "$4" || exit 1
        ! grep -q "enable"        "$4" || exit 1
    ' _ "$RC" "$ERR_OUT" "$D5_XDG" "$CALLS_FILE"

# D6: pre-flight ORDERING is load-bearing. A broken checkout must be reported
# even on a bus-less host — if the fail-open ran first it would mask the missing
# source behind a cheerful exit 0 and the operator would never learn.
D6_XDG="$(mktemp -d /tmp/test-jc-index-units-d6-xdg-XXXXXX)"
_TMPDIRS+=("$D6_XDG")
_make_partial_repo service; D6_REPO="$_PARTIAL_REPO"

reset_calls
REIFY_TEST_REPO_ROOT="$D6_REPO" REIFY_TEST_NO_USER_BUS=1 run_installer "$D6_XDG"

assert "D6: missing source is reported even with no bus (pre-flight precedes fail-open)" \
    bash -c '
        [ "$1" = "1" ] || exit 1
        printf "%s" "$2" | grep -q "ERROR.*reify-jcodemunch-index[.]service"
    ' _ "$RC" "$ERR_OUT"


# ──────────────────────────────────────────────────────────────────────────────
# Block E — serve-unit retirement invariants (task η) AND the operator-doc
#           record-correction invariants that followed it (task 6117, μ)
#
# SCOPE: two layers. The deploy/scripts layer (E1-E5b, η) and the operator-facing
# doc layer (E-DOC*, E-SKILL*, E-CLI*, E-MODES*, μ). Every doc-layer assertion
# names its file EXPLICITLY in the `git grep … -- <path>` pathspec; none of them
# is repo-wide, and that is load-bearing for two independent reasons:
#
#   (a) scripts/smoke-jcodemunch-serve.sh and scripts/with-jcodemunch-serve.sh
#       legitimately carry `jcodemunch-serve` in their own basenames and bodies,
#       so a repo-wide absence grep for that token false-REDs a healthy tree.
#       E3 already carves them out by name for exactly this reason.
#   (b) docs/legibility/confusion-codebook.yaml:13890 — an absence-check test
#       that itself carries the forbidden token becomes the only remaining source
#       occurrence, and so self-defeats a manifest `expect: absent` grep the
#       moment that grep is broadened. THIS FILE carries every token banned
#       below, so it must never fall inside any absence assertion's path set.
#
# Assertions run over TRACKED files via `git grep`, so a stray build artifact or
# an untracked scratch file can neither mask nor manufacture a violation.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: serve-unit retirement invariants (η) ---"

# E1: the unit is gone from the working tree AND from the index
assert "E1: deploy/systemd/jcodemunch-serve.service is absent from the tree and untracked" \
    bash -c '
        [ ! -e "$1/deploy/systemd/jcodemunch-serve.service" ] || exit 1
        ! git -C "$1" ls-files --error-unmatch deploy/systemd/jcodemunch-serve.service >/dev/null 2>&1
    ' _ "$REPO_ROOT"

# E2: nothing under deploy/ names the retired unit — catches the unit itself and
# any stale cross-reference from a sibling unit (e.g. an After=/Wants= ordering
# directive left pointing at a unit that no longer ships).
assert "E2: no tracked file under deploy/ names jcodemunch-serve" \
    bash -c '! git -C "$1" grep -q "jcodemunch-serve" -- deploy/' _ "$REPO_ROOT"

# E3: nothing under scripts/ names the retired UNIT. The assertion targets the
# ".service" suffix, not the bare stem, because scripts/smoke-jcodemunch-serve.sh
# and scripts/with-jcodemunch-serve.sh legitimately keep their own basenames.
#
# with-jcodemunch-serve.sh is excluded by name: its header line 11 reads
# "D5 retires the persistent `deploy/systemd/jcodemunch-serve.service` unit" —
# prose ABOUT this retirement, which stays accurate once η lands rather than
# becoming a dangling pointer, and which belongs to δ's design rationale rather
# than to η. That file is also outside this task's assigned scope (esc-6920-6).
assert "E3: no tracked file under scripts/ names jcodemunch-serve.service (except with-jcodemunch-serve.sh's own retirement note)" \
    bash -c '
        hits=$(git -C "$1" grep -l "jcodemunch-serve[.]service" -- scripts/ 2>/dev/null \
                 | grep -v "^scripts/with-jcodemunch-serve[.]sh$" || true)
        [ -z "$hits" ]
    ' _ "$REPO_ROOT"

# E4: the layer-rules file's schema-provenance comment no longer points at the
# deleted unit as its deployed reference
assert "E4: .jcodemunch.jsonc does not name deploy/systemd/jcodemunch-serve.service" \
    bash -c '! grep -q "deploy/systemd/jcodemunch-serve[.]service" "$1/.jcodemunch.jsonc"' _ "$REPO_ROOT"

# E5: watcher regression guard. jcodemunch-watcher.service is `enabled enabled`
# on this host and serves five other repos, so the retirement must have swept the
# serve unit and left the watcher untouched — asserted in both directions: the
# smoke script's assertion-3 site still references it, and this diff gave no unit
# under deploy/ a reference to it.
assert "E5: smoke script still references jcodemunch-watcher.service (retirement did not spill into the watcher)" \
    bash -c 'grep -q "jcodemunch-watcher" "$1"' _ "$SMOKE"

assert "E5b: no tracked file under deploy/ references jcodemunch-watcher" \
    bash -c '! git -C "$1" grep -q "jcodemunch-watcher" -- deploy/' _ "$REPO_ROOT"


echo ""
echo "--- Block E (doc layer): operator-record truth invariants (μ) ---"

# E-DOC1..E-DOC4 pin docs/architecture-audit/jcodemunch-serve-activation.md to the
# substrate that actually landed. The pair is deliberately absence + PRESENCE:
# an absence-only pair would go green if the whole section were simply deleted,
# and μ's charter is to CORRECT the record, not to erase it.

# E-DOC1: the runbook must not assert the retired persistent unit is live.
assert "E-DOC1: activation doc does not name jcodemunch-serve.service" \
    bash -c '! git -C "$1" grep -qE "jcodemunch-serve\.service" -- docs/architecture-audit/jcodemunch-serve-activation.md' _ "$REPO_ROOT"

# E-DOC2: pattern and path are byte-identical to the capability manifest's
# `nonexistent-db-claim-removed` delivered_check, so the in-repo gate and the
# PRD gate cannot drift apart. It bans the DB *filename* only — naming the
# retired IDENTITY `leodearden/reify` in prose is what the 2026-08-17 premise
# correction requires, so no assertion here forbids it.
assert "E-DOC2: activation doc does not name the retired DB filename" \
    bash -c '! git -C "$1" grep -q "leodearden-reify[.]db" -- docs/architecture-audit/jcodemunch-serve-activation.md' _ "$REPO_ROOT"

# E-DOC3: the replacement identity is stated positively.
assert "E-DOC3: activation doc names the per-path identity reify actually uses (local/reify-)" \
    bash -c 'git -C "$1" grep -q "local/reify-" -- docs/architecture-audit/jcodemunch-serve-activation.md' _ "$REPO_ROOT"

# E-DOC4: the load-bearing half of the 2026-08-17 premise correction — the doc
# must justify the per-path identity by the FORCED LEVER (jcodemunch ships
# git_root_identity: True by default; reify overrides it), never by a
# "no such index exists" claim, which was false.
assert "E-DOC4: activation doc names the forced lever JCODEMUNCH_GIT_ROOT_IDENTITY" \
    bash -c 'git -C "$1" grep -q "JCODEMUNCH_GIT_ROOT_IDENTITY" -- docs/architecture-audit/jcodemunch-serve-activation.md' _ "$REPO_ROOT"

# E-DOC5/E-DOC6 pin the prior PRD's comparison table at
# docs/prds/reify-audit-p1-jcodemunch-substrate.md. Task 4109 landed the fail-soft
# fall-back to NoopJCodemunchOps; the §10 table's jcodemunch-dependency row was
# never updated and has claimed exit 125 ever since.

# E-DOC5: pattern and path are byte-identical to the manifest's
# `stale-degradation-contract-removed` delivered_check, so the in-repo gate and
# the PRD gate cannot drift apart.
assert "E-DOC5: prior PRD does not claim the detectors degrade to exit 125 when serve is down" \
    bash -c '! git -C "$1" grep -q "degrades to exit 125 when serve is down" -- docs/prds/reify-audit-p1-jcodemunch-substrate.md' _ "$REPO_ROOT"

# E-DOC6: regression guard on E-DOC5's fix, not an independent RED. The file
# already names the fail-soft seam at :147; this pins that the corrected row keeps
# pointing at the landed mechanism instead of being blanked, which is the cheap
# way to satisfy an absence check.
assert "E-DOC6: prior PRD still names the landed fail-soft seam (NoopJCodemunchOps)" \
    bash -c 'git -C "$1" grep -q "NoopJCodemunchOps" -- docs/prds/reify-audit-p1-jcodemunch-substrate.md' _ "$REPO_ROOT"

# E-SKILL1..3 pin .claude/skills/audit/SKILL.md — the operator-facing entry point
# for /audit. Asserted per FILE rather than over `.claude/skills`, so a regression
# in either skill document names the file that regressed (E-CLI1..3 mirror these
# one-for-one against references/cli-invocation.md).
#
# THERE IS DELIBERATELY NO expect-absent ASSERTION ON `leodearden/reify` ANYWHERE
# UNDER .claude/skills, and a later hardening pass must not add one. μ is a
# correct-the-record task: explaining why the retired git identity is NOT what
# reify uses requires NAMING it, so an absence check there fights the very edit it
# would be guarding. The retired DB FILENAME is banned instead (E-DOC2), which is
# the claim that was actually false.
#
# E-SKILL1 bans the `.service` suffix, not the bare stem, for the same reason E3
# does: `scripts/with-jcodemunch-serve.sh` is the replacement these documents must
# be free to cite by name, and its basename contains the stem.

assert "E-SKILL1: SKILL.md does not name the retired jcodemunch-serve.service unit" \
    bash -c '! git -C "$1" grep -qE "jcodemunch-serve\.service" -- .claude/skills/audit/SKILL.md' _ "$REPO_ROOT"

assert "E-SKILL2: SKILL.md documents --jcodemunch-index-dir" \
    bash -c 'git -C "$1" grep -q -- "--jcodemunch-index-dir" .claude/skills/audit/SKILL.md' _ "$REPO_ROOT"

# The PREFIX, not a single token: γ landed three codes (_STALE, _EMPTY,
# _UNREADABLE) and the prefix keeps a fourth from silently escaping the gate.
assert "E-SKILL3: SKILL.md documents the E_JC_INDEX_* refusal codes" \
    bash -c 'git -C "$1" grep -q "E_JC_INDEX_" -- .claude/skills/audit/SKILL.md' _ "$REPO_ROOT"

# E-CLI1..3 mirror E-SKILL1..3 one-for-one against the file SKILL.md sends the
# reader to for "full flag documentation". Correcting only SKILL.md would leave
# the authoritative flag reference still advertising the retired default and the
# retired unit — a half-correction that reads as deliberate. Asserted separately
# rather than by widening E-SKILL* to `-- .claude/skills`, so a regression names
# the file that regressed.

assert "E-CLI1: cli-invocation.md does not name the retired jcodemunch-serve.service unit" \
    bash -c '! git -C "$1" grep -qE "jcodemunch-serve\.service" -- .claude/skills/audit/references/cli-invocation.md' _ "$REPO_ROOT"

assert "E-CLI2: cli-invocation.md documents --jcodemunch-index-dir" \
    bash -c 'git -C "$1" grep -q -- "--jcodemunch-index-dir" .claude/skills/audit/references/cli-invocation.md' _ "$REPO_ROOT"

assert "E-CLI3: cli-invocation.md documents the E_JC_INDEX_* refusal codes" \
    bash -c 'git -C "$1" grep -q "E_JC_INDEX_" -- .claude/skills/audit/references/cli-invocation.md' _ "$REPO_ROOT"


# ──────────────────────────────────────────────────────────────────────────────
# Block F — smoke-script connection-failure hint contract
#
# scripts/smoke-jcodemunch-serve.sh's "start the serve first" hint was stale on
# three independent axes: an unresolvable pinned git source, a wrong Python
# version, and a systemd unit this diff deletes.
#
# F1 bans the SHAPE of that defect — an inline jcodemunch invocation — rather
# than the three specific stale literals. A literal-by-literal ban goes green on
# a recipe re-introduced at the CURRENT pin, which is the same fifth-copy-of-the-
# pin failure with a fresh version number on it; only the shape ban survives a
# bump without an edit.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: smoke-script hint contract ---"

# F1: the script builds NO inline jcodemunch invocation, of any vintage.
# scripts/with-jcodemunch-serve.sh:248-257 enumerates the four sites that copy
# the pin and warns against a fifth. Every token banned here is one a fifth copy
# would have to use, and none of them is a version literal — so this assertion
# never needs editing when the pin moves.
assert "F1: smoke script builds no inline jcodemunch invocation (no uvx / --from / == pin / --python / git+ source)" \
    bash -c '
        ! grep -q    "uvx"                    "$1" || exit 1
        ! grep -q -- "--from jcodemunch-mcp"  "$1" || exit 1
        ! grep -q    "jcodemunch-mcp=="       "$1" || exit 1
        ! grep -q -- "--python"               "$1" || exit 1
        ! grep -q    "git+https://github.com" "$1" || exit 1
    ' _ "$SMOKE"

# F2: axis 3 — the unit this diff deletes
assert "F2: smoke script names no jcodemunch-serve.service unit" \
    bash -c '! grep -q "jcodemunch-serve[.]service" "$1"' _ "$SMOKE"

# F3: the hint must actually tell the operator what to run instead. Deleting the
# stale recipe without supplying the replacement would leave a worse hint than
# the stale one — the failure mode this block is really guarding against.
assert "F3: connection-failure hint names scripts/with-jcodemunch-serve.sh as the replacement recipe" \
    bash -c 'grep -q "with-jcodemunch-serve[.]sh" "$1"' _ "$SMOKE"

# F4: ...and naming the wrapper does NOT by itself make the recipe runnable.
# The wrapper spawns its serve under JCODEMUNCH_GIT_ROOT_IDENTITY=0, so that
# serve answers for the per-path local/reify-<hash> index, while this script's
# default REPO_ID is the leodearden/reify husk. A recipe that omitted --repo
# would clear assertion 1 and then fail assertion 2 for a non-obvious identity
# reason — a misleading hint of exactly the class Block F exists to retire.
# Anchored to the hint block itself, not the whole file, so the header's copy of
# the recipe cannot satisfy it on the hint's behalf.
assert "F4: the connection-failure hint's recipe passes --repo with the per-path identity" \
    bash -c '
        hint=$(sed -n "/FAIL \[1\]: curl to/,/See: docs/p" "$1")
        printf "%s" "$hint" | grep -q    "with-jcodemunch-serve[.]sh --port" || exit 1
        printf "%s" "$hint" | grep -q -- "--repo local/reify-"
    ' _ "$SMOKE"

# F5: the flag the recipe prints must be one the script really implements —
# proven by RUNNING it, not by grepping for the string. --help must document it;
# a valueless --repo must be REFUSED rather than falling back to the default
# husk, which is how a nominally-runnable recipe would go silently vacuous again;
# and an unknown flag must be rejected rather than ignored.
assert "F5: --repo is implemented — documented by --help, refused without a value, unknown flags rejected (exit 2)" \
    bash -c '
        bash "$1" --help 2>&1 | grep -q -- "--repo" || exit 1
        rc=0; bash "$1" --repo   >/dev/null 2>&1 || rc=$?
        [ "$rc" = "2" ] || exit 1
        rc=0; bash "$1" --bogus  >/dev/null 2>&1 || rc=$?
        [ "$rc" = "2" ]
    ' _ "$SMOKE"

# F6: the edit must leave a script that still parses and still answers --help,
# proven without needing a live serve.
assert "F6: smoke script parses and --help exits 0" \
    bash -c '
        bash -n "$1" || exit 1
        bash "$1" --help >/dev/null 2>&1
    ' _ "$SMOKE"

# F7: watcher guardrail — the hint rewrite must not spill into assertion 3's site
assert "F7: assertion-3 site still names jcodemunch-watcher.service" \
    bash -c 'grep -q "jcodemunch-watcher[.]service is not active" "$1"' _ "$SMOKE"


# ──────────────────────────────────────────────────────────────────────────────
# Block H — smoke-script query-budget contract
#
# Block F proved the printed recipe NAMES the wrapper. This block proves the
# recipe can RUN TO COMPLETION: a hint that gets the operator as far as
# assertion 2 and then dies on a timeout is still a hint that does not work.
#
# WHY the query cannot share the handshake's budget: the retired long-lived
# jcodemunch-serve.service kept an in-process cache warm ACROSS smoke runs, so a
# query came back in 3-7 s. scripts/with-jcodemunch-serve.sh spawns a FRESH serve
# per invocation and this script issues exactly ONE query per serve — so under
# the transient-serve model that single query is ALWAYS the cold one (measured
# 34-100 s across four observations on two hosts). A handshake-sized budget
# SIGTERMs it mid-flight and prints "curl … failed", which reads exactly like the
# connection-refused case Block F just finished fixing.
#
# Structural greps over $SMOKE only — no live serve, same style as F1-F7.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: smoke-script query budget ---"

# The curl invocation starting at <anchor>, through its closing `2>/dev/null)`.
# Per-site extraction is what makes "the query does not spend MCP_TIMEOUT" and
# "the handshake still does" separable assertions: a whole-file grep cannot tell
# the two sites apart, and would go green on the wholesale s/15/180/ H3 rejects.
smoke_curl_block() {
    awk -v anchor="$1" '
        index($0, anchor) == 1 { f = 1 }
        f { print }
        f && /2>\/dev\/null\)/ { exit }
    ' "$SMOKE"
}

INIT_CURL="$(smoke_curl_block 'http_code=$(curl')"
QUERY_CURL="$(smoke_curl_block 'http_code2=$(curl')"

# H1: the query site spends a budget of its own, not the handshake's.
assert "H1: the get_changed_symbols query does not spend the handshake budget" \
    bash -c '
        [ -n "$1" ] || exit 1
        mt=$(printf "%s\n" "$1" | grep -- "--max-time") || exit 1
        ! printf "%s\n" "$mt" | grep -q "MCP_TIMEOUT"
    ' _ "$QUERY_CURL"

# H2: ...and that budget is big enough for a cold query. Asserted as a FLOOR
# rather than against the current literal, so raising it after a slow-host
# observation is a one-line change that leaves this suite green.
assert "H2: the query budget's default is at least 120 s (cold query measured 34-100 s)" \
    bash -c '
        line=$(grep -E "^QUERY_TIMEOUT=" "$1" | head -1) || exit 1
        n=$(printf "%s" "$line" | grep -oE "[0-9]+" | tail -1)
        [ -n "$n" ] || exit 1
        [ "$n" -ge 120 ]
    ' _ "$SMOKE"

# H3: the split must stay a SPLIT. Raising MCP_TIMEOUT wholesale would satisfy
# H1 and H2 while making a genuinely dead port take three minutes to report
# itself — so the handshake budget stays tight AND the initialize site keeps
# spending it.
assert "H3: the handshake budget stays tight (<= 30 s) and the initialize site still uses it" \
    bash -c '
        line=$(grep -E "^MCP_TIMEOUT=" "$2" | head -1) || exit 1
        n=$(printf "%s" "$line" | grep -oE "[0-9]+" | tail -1)
        [ -n "$n" ] || exit 1
        [ "$n" -le 30 ] || exit 1
        [ -n "$1" ] || exit 1
        printf "%s\n" "$1" | grep -- "--max-time" | grep -q "MCP_TIMEOUT"
    ' _ "$INIT_CURL" "$SMOKE"

# H4: a slow host must be able to buy more time without editing the script.
assert 'H4: the query budget is env-overridable — QUERY_TIMEOUT="${SMOKE_QUERY_TIMEOUT:-<int>}"' \
    bash -c '
        line=$(grep -E "^QUERY_TIMEOUT=" "$1" | head -1) || exit 1
        rest=${line##*SMOKE_QUERY_TIMEOUT:-}
        [ "$rest" != "$line" ] || exit 1
        dflt=${rest%%\}*}
        [ -n "$dflt" ] || exit 1
        case "$dflt" in *[!0-9]*) exit 1 ;; esac
    ' _ "$SMOKE"

# H5: every --max-time in the file names a budget. This is what stops a curl
# site added later from carrying a fresh hard-coded 15 alongside the two named
# ones — the defect H1 repairs, re-committed one site over.
assert "H5: no --max-time takes a bare integer literal (every site names a budget)" \
    bash -c '! grep -qE -- "--max-time[[:space:]]+[0-9]" "$1"' _ "$SMOKE"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — setup-dev.sh wiring (structural grep, no execution)
#
# Asserted structurally rather than by running setup-dev.sh: that script installs
# toolchains and mutates the host, so executing it from a test is not an option.
# Same approach as test_warm_lane_boot_persistence.sh's Block E.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: setup-dev.sh wiring ---"

# G1: setup-dev.sh invokes the installer at all
assert "G1: setup-dev.sh invokes install-jcodemunch-index-units.sh" \
    bash -c 'grep -q "install-jcodemunch-index-units.sh" "$1"' _ "$SETUP_DEV"

# G2: the invocation is non-fatal — a failed unit install must never abort dev
# setup, which is the whole point of the if/then-ok/else-warn shape used at
# setup-dev.sh:283-287 for the warm-lane installer.
assert "G2: the invocation is non-fatal (else + warn, and no exit in the failure branch)" \
    bash -c '
        block=$(grep -A8 "install-jcodemunch-index-units.sh" "$1")
        echo "$block" | grep -q "else" || exit 1
        echo "$block" | grep -q "warn" || exit 1
        ! echo "$block" | grep -qE "^[[:space:]]*exit[[:space:]]+[0-9]+[[:space:]]*$"
    ' _ "$SETUP_DEV"

# G3: index freshness is UNCONDITIONAL — the call must NOT sit inside the
# REIFY_PROVISION_WARM_LANES=1 block. Gating it behind the warm-lane flag would
# mean a developer who never provisions warm lanes silently gets a stale index.
# Asserted by line number: the call site must fall outside [gate, matching fi].
assert "G3: the invocation is OUTSIDE the REIFY_PROVISION_WARM_LANES block (index freshness is unconditional)" \
    bash -c '
        gate_ln=$(grep -n "if \[ \"\${REIFY_PROVISION_WARM_LANES:-}\" = \"1\" \]" "$1" | head -1 | cut -d: -f1)
        [ -n "$gate_ln" ] || exit 1
        # Outer fi: the first unindented ^fi$ after the gate (inner fis are indented)
        fi_ln=$(awk "NR > $gate_ln && /^fi\$/ { print NR; exit }" "$1")
        [ -n "$fi_ln" ] || exit 1
        install_ln=$(grep -n "install-jcodemunch-index-units.sh" "$1" | head -1 | cut -d: -f1)
        [ -n "$install_ln" ] || exit 1
        [ "$install_ln" -lt "$gate_ln" ] || [ "$install_ln" -gt "$fi_ln" ]
    ' _ "$SETUP_DEV"

# G4: the edit leaves setup-dev.sh parsing
assert "G4: setup-dev.sh parses" \
    bash -n "$SETUP_DEV"

test_summary
