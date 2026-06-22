#!/usr/bin/env bash
# scripts/install-warm-lane-units.sh — Install warm-lane systemd user units.
#
# Copies the tracked reify-warm-lane.service unit and the
# orchestrator-reify.service.d/warm-lane.conf drop-in into the user systemd
# directory, then daemon-reloads and enables the unit for boot-persistence.
#
# Called by scripts/setup-dev.sh when REIFY_PROVISION_WARM_LANES=1.
# Standalone installer, hermetically testable (mirror of setup-main-gate-worktree-config.sh).
#
# Fail-open: if no systemd --user bus is available, prints a warning and exits 0
# (no daemon-reload or enable attempted — safe to call in non-systemd environments).
# Idempotent: cp overwrites, mkdir -p is safe, systemctl enable is idempotent.
#
# ExecStart hardening (task #4720): after copying the tracked unit, the installer
# rewrites the installed unit's ExecStart line to pin the three explicit flags:
#   --img  /media/leo/data_lv_1/leo/reify-warm-lanes.img
#   --size-gib  4096
#   --mount  /home/leo/src/warm-lanes
# The tracked unit keeps its bare "ExecStart=.../provision-warm-lane-fs.sh" line
# (no flags) — this is intentional.  The rewrite happens on the INSTALLED copy
# only, and is idempotent: cp resets to the bare tracked unit each run, and the
# sed pattern's trailing .* strips any pre-existing flags before re-appending.
# This decouples the deployed boot unit from future script-default drift — the
# footgun this task closes.
#
# Usage:
#   scripts/install-warm-lane-units.sh
#
# Environment:
#   XDG_CONFIG_HOME   Override user config dir (default: $HOME/.config)
#
# Exits 0 on success or when the bus is absent (fail-open).
# Exits non-zero if a file copy fails or daemon-reload/enable fails (real errors).

set -euo pipefail

# ── helpers ───────────────────────────────────────────────────────────────────
_info()  { echo "[install-warm-lane-units] INFO:  $*" >&2; }
_ok()    { echo "[install-warm-lane-units] OK:    $*" >&2; }
_warn()  { echo "[install-warm-lane-units] WARN:  $*" >&2; }

# ── CLI guard ─────────────────────────────────────────────────────────────────
if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    echo "Usage: $(basename "$0")" >&2
    echo "" >&2
    echo "  Install warm-lane systemd user units (fail-open, idempotent)." >&2
    echo "  Copies deploy/systemd/reify-warm-lane.service and" >&2
    echo "  deploy/systemd/orchestrator-reify.service.d/warm-lane.conf" >&2
    echo "  into \${XDG_CONFIG_HOME:-\$HOME/.config}/systemd/user/, then" >&2
    echo "  runs systemctl --user daemon-reload and enable." >&2
    exit 0
fi

if [ $# -gt 0 ]; then
    echo "$(basename "$0"): unexpected argument: $1" >&2
    echo "Usage: $(basename "$0")" >&2
    exit 2
fi

# ── resolve paths ─────────────────────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# REIFY_TEST_REPO_ROOT allows hermetic tests to point the installer at a temp tree
# (e.g. to exercise the pre-flight failure path) without touching the real repo.
REPO_ROOT="${REIFY_TEST_REPO_ROOT:-$(cd "$_SCRIPT_DIR/.." && pwd)}"

UNIT_SRC="$REPO_ROOT/deploy/systemd/reify-warm-lane.service"
DROPIN_SRC="$REPO_ROOT/deploy/systemd/orchestrator-reify.service.d/warm-lane.conf"

UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
DROPIN_DIR="$UNIT_DIR/orchestrator-reify.service.d"

# ── pinned ExecStart flags (task #4720 hardening) ─────────────────────────────
# Hardcoded here by design: the deployed boot unit must be immune to future drift
# in provision-warm-lane-fs.sh's defaults — re-installing must NOT silently derive
# whatever the script default happens to be.  These values coincide with the script
# defaults today but are intentionally independent going forward.
WARM_LANE_IMG="/media/leo/data_lv_1/leo/reify-warm-lanes.img"
WARM_LANE_SIZE_GIB=4096
WARM_LANE_MOUNT="/home/leo/src/warm-lanes"

# ── pre-flight: source files must exist ──────────────────────────────────────
if [ ! -f "$UNIT_SRC" ]; then
    echo "ERROR: unit source not found: $UNIT_SRC" >&2
    exit 1
fi
if [ ! -f "$DROPIN_SRC" ]; then
    echo "ERROR: drop-in source not found: $DROPIN_SRC" >&2
    exit 1
fi

# ── fail-open: no systemd --user bus → warn and skip ─────────────────────────
if ! systemctl --user show-environment &>/dev/null; then
    _warn "no systemd --user bus available — skipping warm-lane unit install (fail-open)"
    exit 0
fi

# ── linger advisory: boot-persistence requires user lingering ─────────────────
# Without `loginctl enable-linger <user>`, an enabled user unit starts only at
# first login — NOT at boot.  If the orchestrator host hasn't enabled lingering
# the boot-persistence goal is silently not achieved.  Warn; do not fail.
if command -v loginctl &>/dev/null; then
    _linger="$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null || true)"
    if [ "$_linger" != "yes" ]; then
        _warn "user lingering is NOT enabled — reify-warm-lane.service will start"
        _warn "  at first login only, not at boot.  Enable once with:"
        _warn "    sudo loginctl enable-linger $(id -un)"
    fi
fi

# ── copy unit and drop-in (idempotent: cp overwrites) ────────────────────────
mkdir -p "$UNIT_DIR"
mkdir -p "$DROPIN_DIR"

_info "copying $UNIT_SRC → $UNIT_DIR/"
cp "$UNIT_SRC" "$UNIT_DIR/"

# Pin explicit flags onto the INSTALLED unit's ExecStart (task #4720 hardening).
# The tracked unit keeps its bare "ExecStart=.../provision-warm-lane-fs.sh" line.
# The trailing '.*' strips any flags written by a prior install run, so this is
# idempotent even without the cp reset.  Uses '|' delimiter to avoid escaping '/'.
sed -i -E "s|^(ExecStart=.*/provision-warm-lane-fs\.sh).*|\1 --img ${WARM_LANE_IMG} --size-gib ${WARM_LANE_SIZE_GIB} --mount ${WARM_LANE_MOUNT}|" \
    "$UNIT_DIR/reify-warm-lane.service"

_info "copying $DROPIN_SRC → $DROPIN_DIR/"
cp "$DROPIN_SRC" "$DROPIN_DIR/"

# ── reload and enable ─────────────────────────────────────────────────────────
_info "systemctl --user daemon-reload"
systemctl --user daemon-reload

_info "systemctl --user enable reify-warm-lane.service (boot-persistence)"
systemctl --user enable reify-warm-lane.service

_ok "warm-lane systemd units installed and enabled for boot-persistence"
