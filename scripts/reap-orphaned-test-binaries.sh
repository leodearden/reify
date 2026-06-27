#!/usr/bin/env bash
# scripts/reap-orphaned-test-binaries.sh — host-wide orphaned test-binary reaper.
#
# Thin self-documenting wrapper that execs lib_proc_reaper.sh reap-orphans.
# Designed as a cron/seam entry point (mirrors scripts/cargo-audit-orphans
# → scripts/audit-orphan-producers.sh pattern).
#
# Usage:
#   scripts/reap-orphaned-test-binaries.sh [--dry-run]
#
# Candidates: processes owned by the current UID whose resolved executable
# (/proc/<pid>/exe) is under */target/{debug,release}/deps/*, whose PPID is
# PID 1 or whose parent comm is in {systemd,init}, and whose age exceeds
# REIFY_REAPER_MIN_AGE_SECS (default 7200 s = verify_command_timeout_secs).
#
# Environment knobs (see scripts/lib_proc_reaper.sh for the full reference):
#   REIFY_REAPER_DEPS_GLOB     glob patterns for candidate exe paths
#   REIFY_REAPER_MIN_AGE_SECS  minimum process age in seconds (default 7200)
#   REIFY_REAPER_ORPHAN_PPIDS  space-separated PPIDs considered orphan parents
#                              (default: 1)
#   REIFY_REAPER_COMMS         space-separated comm names of orphan-parent procs
#                              (default: systemd init)
#   REIFY_REAPER_UID           UID to filter by (default: current user)
#   REIFY_PROC_REAPER_DISABLE  set to 1 to disable (break-glass)
#
# Cross-repo seam: dark-factory should wire this script (or the lib's
# reap-orphans subcommand) as a periodic cron/post-cancel sweep to catch
# orphans from SIGKILL'd verify parents — the same seam class as
# cpu-governance and warm-lane-pool (see docs/notes/orphaned-test-binary-reaper.md).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/lib_proc_reaper.sh" reap-orphans "$@"
