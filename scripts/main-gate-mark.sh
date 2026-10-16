#!/usr/bin/env bash
# scripts/main-gate-mark.sh — the main-gate mark/unmark CLI for out-of-process
# markers: dark-factory's advance_main / recover_red_main, through
# dark-factory-orchestrator.yaml git.main_gate_{mark,unmark}_command. The token
# format lives in hooks/main-gate-lib.sh; this file only dispatches to it.
#
# Usage: scripts/main-gate-mark.sh mark|unmark
#   mark     bind the main-gate sentinel to the current main tip
#   unmark   remove the sentinel only if it is bound to the current main tip
# Exit: 0 ok · 1 the mark could not be written · 2 hooks/main-gate-lib.sh missing
#       · 64 usage error

set -uo pipefail

usage() {
    echo "usage: scripts/main-gate-mark.sh mark|unmark"
}

_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/hooks/main-gate-lib.sh"
if [ ! -f "$_lib" ]; then
    echo "main-gate-mark: missing $_lib" >&2
    exit 2
fi
# shellcheck source=hooks/main-gate-lib.sh
. "$_lib"

case "${1:-}" in
    mark) main_gate_mark ;;
    unmark) main_gate_unmark ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 64 ;;
esac
