#!/usr/bin/env bash
# scripts/install-jcodemunch-index-units.sh — Install the reify-owned jcodemunch
# index-warming service + timer as systemd --user units.
#
# Copies the two tracked units from deploy/systemd/ into the user systemd
# directory, then daemon-reloads and enables the TIMER (which owns activation;
# the service carries no [Install] section).
#
# PRD: docs/prds/jcodemunch-substrate-restoration.md (task ζ).
# Precedent: scripts/install-warm-lane-units.sh — same shape, two deliberate
# departures:
#
#   1. No ExecStart sed pin. The warm-lane installer rewrites its units'
#      ExecStart lines to pin host-specific paths, because those scripts'
#      defaults could drift out from under the deployed boot unit. Here there
#      is no host-specific value to pin: scripts/jcodemunch-index-reify.sh
#      already hard-codes the canonical project root, so a pinned
#      --project-root could only be a second, drifting copy of it. The units
#      are installed by plain `cp`, byte-identical to the tracked sources.
#   2. No Environment= duplication of JCODEMUNCH_GIT_ROOT_IDENTITY. That lever
#      is load-bearing (it decides whether the index is keyed to the canonical
#      checkout or to the upstream remote identity) and the script owns it.
#
# Scope: this installer touches ONLY the two reify-*.{service,timer} units it
# ships. jcodemunch's own host units are never named, enabled, disabled or
# overwritten by this path — they serve other repos.
#
# Usage:
#   scripts/install-jcodemunch-index-units.sh
#
# Environment:
#   XDG_CONFIG_HOME   Override user config dir (default: $HOME/.config)
#
# Idempotent: cp overwrites, mkdir -p is safe, systemctl enable is idempotent.
#
# Exits 0 on success or when the --user bus is absent (fail-open).
# Exits 1 if a tracked unit source is missing, or a copy/reload/enable fails.
# Exits 2 on CLI misuse (matching install-warm-lane-units.sh).

set -euo pipefail

# ── helpers ───────────────────────────────────────────────────────────────────
_info()  { echo "[install-jcodemunch-index-units] INFO:  $*" >&2; }
_ok()    { echo "[install-jcodemunch-index-units] OK:    $*" >&2; }
_warn()  { echo "[install-jcodemunch-index-units] WARN:  $*" >&2; }

# ── CLI guard ─────────────────────────────────────────────────────────────────
if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    echo "Usage: $(basename "$0")" >&2
    echo "" >&2
    echo "  Install the reify jcodemunch index-warming systemd user units" >&2
    echo "  (fail-open, idempotent).  Copies" >&2
    echo "  deploy/systemd/reify-jcodemunch-index.{service,timer} into" >&2
    echo "  \${XDG_CONFIG_HOME:-\$HOME/.config}/systemd/user/, then runs" >&2
    echo "  systemctl --user daemon-reload and enable --now on the timer." >&2
    exit 0
fi

if [ $# -gt 0 ]; then
    echo "$(basename "$0"): unexpected argument: $1" >&2
    echo "Usage: $(basename "$0")" >&2
    exit 2
fi

# ── resolve paths ─────────────────────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# REIFY_TEST_REPO_ROOT allows hermetic tests to point the installer at a temp
# tree (e.g. to exercise the pre-flight failure path) without touching the real
# repo — same seam as install-warm-lane-units.sh.
REPO_ROOT="${REIFY_TEST_REPO_ROOT:-$(cd "$_SCRIPT_DIR/.." && pwd)}"

SERVICE_SRC="$REPO_ROOT/deploy/systemd/reify-jcodemunch-index.service"
TIMER_SRC="$REPO_ROOT/deploy/systemd/reify-jcodemunch-index.timer"

UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

# ── pre-flight: both tracked sources must exist ──────────────────────────────
# Deliberately BEFORE the bus check below, so a broken checkout is reported even
# on a host with no --user bus. The other order would let fail-open mask a
# missing unit behind a cheerful exit 0.
if [ ! -f "$SERVICE_SRC" ]; then
    echo "ERROR: service unit source not found: $SERVICE_SRC" >&2
    exit 1
fi
if [ ! -f "$TIMER_SRC" ]; then
    echo "ERROR: timer unit source not found: $TIMER_SRC" >&2
    exit 1
fi

# ── fail-open: no systemd --user bus → warn and skip ─────────────────────────
# Placed before any mkdir/cp so the skip is total: a bus-less host (CI, a
# container) gets no half-installed unit directory it would then never reload.
if ! systemctl --user show-environment &>/dev/null; then
    _warn "no systemd --user bus available — skipping index-unit install (fail-open)"
    exit 0
fi

# ── install (plain cp — see departure 1 in the header) ───────────────────────
mkdir -p "$UNIT_DIR"

_info "copying $SERVICE_SRC → $UNIT_DIR/"
cp "$SERVICE_SRC" "$UNIT_DIR/"

_info "copying $TIMER_SRC → $UNIT_DIR/"
cp "$TIMER_SRC" "$UNIT_DIR/"

# ── reload and enable ─────────────────────────────────────────────────────────
_info "systemctl --user daemon-reload"
systemctl --user daemon-reload

# The timer, not the service: the service has no [Install] section, and enabling
# it directly would give it an activation path independent of the schedule.
_info "systemctl --user enable --now reify-jcodemunch-index.timer"
systemctl --user enable --now reify-jcodemunch-index.timer

_ok "jcodemunch index-warming units installed; timer enabled"
