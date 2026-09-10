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

set -euo pipefail

# ── helpers ───────────────────────────────────────────────────────────────────
_info()  { echo "[install-jcodemunch-index-units] INFO:  $*" >&2; }
_ok()    { echo "[install-jcodemunch-index-units] OK:    $*" >&2; }
_warn()  { echo "[install-jcodemunch-index-units] WARN:  $*" >&2; }

# ── resolve paths ─────────────────────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# REIFY_TEST_REPO_ROOT allows hermetic tests to point the installer at a temp
# tree (e.g. to exercise the pre-flight failure path) without touching the real
# repo — same seam as install-warm-lane-units.sh.
REPO_ROOT="${REIFY_TEST_REPO_ROOT:-$(cd "$_SCRIPT_DIR/.." && pwd)}"

SERVICE_SRC="$REPO_ROOT/deploy/systemd/reify-jcodemunch-index.service"
TIMER_SRC="$REPO_ROOT/deploy/systemd/reify-jcodemunch-index.timer"

UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

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
