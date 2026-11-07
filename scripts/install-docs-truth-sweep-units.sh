#!/usr/bin/env bash
# scripts/install-docs-truth-sweep-units.sh — install the docs-truth sweep's
# systemd --user service + timer (deploy/systemd/reify-docs-truth-sweep.*) and
# enable the timer. Run it once, deliberately, on the orchestrator host after
# merge: the timer files into that host's LIVE escalation queue, so it is not a
# setup-dev.sh side effect. Rationale: docs/notes/docs-truth-sweep.md.
#
# Usage: scripts/install-docs-truth-sweep-units.sh
# Environment: XDG_CONFIG_HOME (default $HOME/.config); REIFY_TEST_REPO_ROOT
# (tests only: read the unit sources from another tree).
#
# Idempotent. Exits 0 on success or when there is no --user bus (fail-open);
# 1 when a unit source is missing or a copy/reload/enable fails; 2 on CLI misuse.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_systemd_user_install.sh
source "$SCRIPT_DIR/lib_systemd_user_install.sh"

TIMER=reify-docs-truth-sweep.timer

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    echo "Usage: $(basename "$0")" >&2
    echo "" >&2
    echo "  Install the reify docs-truth sweep systemd user units (fail-open," >&2
    echo "  idempotent): copies deploy/systemd/reify-docs-truth-sweep.{service,timer}" >&2
    echo "  into \${XDG_CONFIG_HOME:-\$HOME/.config}/systemd/user/, then runs" >&2
    echo "  systemctl --user daemon-reload and enable --now on the timer." >&2
    exit 0
fi
if [ $# -gt 0 ]; then
    echo "$(basename "$0"): unexpected argument: $1" >&2
    echo "Usage: $(basename "$0")" >&2
    exit 2
fi

REPO_ROOT="${REIFY_TEST_REPO_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
SERVICE_SRC="$REPO_ROOT/deploy/systemd/reify-docs-truth-sweep.service"
TIMER_SRC="$REPO_ROOT/deploy/systemd/$TIMER"

# Pre-flight BEFORE the bus probe, so fail-open can never mask a broken checkout.
for source in "$SERVICE_SRC" "$TIMER_SRC"; do
    if [ ! -f "$source" ]; then
        echo "ERROR: unit source not found: $source" >&2
        exit 1
    fi
done

if ! systemd_user_bus_available; then
    systemd_user_log_warn "no systemd --user bus available — skipping docs-truth sweep unit install (fail-open)"
    exit 0
fi

systemd_user_warn_unless_lingering "$TIMER"
systemd_user_install_and_enable_timer "$TIMER" "$SERVICE_SRC" "$TIMER_SRC"
systemd_user_log_ok "docs-truth sweep units installed; $TIMER enabled"
