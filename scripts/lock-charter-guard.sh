#!/usr/bin/env bash
# lock-charter-guard.sh — syntactic directory-vs-file predicate for task lock charters.
#
# Classifies a declared path string as a directory declaration (REJECT) or a
# file-level/empty declaration (ACCEPT), per contracts C-P1..C-P4 in the
# task-lock-charter-lifecycle PRD (docs/prds/task-lock-charter-lifecycle.md §4.1).
#
# Subcommands:
#   classify <path>        — single-path predicate.
#                            exit 0 = ACCEPT (file-level declaration)
#                            exit 1 = REJECT (directory declaration)
#                            prints "ACCEPT <path>" or "REJECT <path>" to stdout.
#   check [path...]        — metadata.files-list gate.
#                            Reads paths from positional args; if none, reads
#                            newline-separated stdin.
#                            Empty list (the [] defer-to-architect value) → exit 0.
#                            All-file list → exit 0.
#                            Any directory path → exit 1 (prints each REJECT <path>).
#   --list-extensions      — prints the canonical extension allowlist sorted-unique,
#                            one extension per line (shared α/γ test vector, PRD §11 Q1).
#
# Exit-code contract:
#   0 — ACCEPT (file-level declaration or empty list)
#   1 — REJECT (directory declaration found)
#   2 — usage error (unknown subcommand, missing required argument)
#
# Mechanism (C-P3 — pure string, no stat, no model):
#   Strip trailing slash(es) → take final path segment (after last /) → case
#   suffix-match against the extension allowlist.
#   NO test -f / test -d / -e, NO network, NO LLM call anywhere.
#   Conservative-reject: an extension-less final segment is treated as a directory
#   (REJECT); extension-less real files (e.g. hooks/pre-merge-commit) are declared
#   via [] — the safe under-declaration direction (PRD §5.2).
#
# Cross-repo seam: γ (fused-memory / dark-factory submit_task backstop)
#   re-implements this predicate against the PRD §4.1 spec using the shared
#   --list-extensions test vector (PRD §11 Q1) rather than taking a runtime
#   dependency on this script.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Canonical extension allowlist (OQ#2 resolved — PRD §11 Q2).
# Single source of truth: used by _is_file_path(), classify, and --list-extensions.
# PRD-explicit: rs ri toml cpp c h hpp md json yaml yml lock py sh ts tsx js txt step stl
# Corpus-evidenced: css mjs html jsonc gcode service
# Common source siblings: cc cxx hh mts cts cjs jsx scss svg png
# ---------------------------------------------------------------------------
_EXTS="c cc cjs cpp css cts cxx gcode h hh hpp html js json jsonc jsx lock md mjs mts png py ri rs scss service sh step stl svg toml ts tsx txt yaml yml"

# ---------------------------------------------------------------------------
# _is_file_path <path>
# Pure-string predicate.  Returns 0 (true = file) or 1 (false = directory).
# No filesystem stat, no model call — C-P3 invariant.
# ---------------------------------------------------------------------------
_is_file_path() {
    local p="$1"
    # Strip all trailing slashes.
    while [ "${p%/}" != "$p" ]; do
        p="${p%/}"
    done
    # Extract the final path segment (everything after the last /).
    local seg="${p##*/}"
    # An empty segment (path was all slashes) → treat as directory.
    [ -z "$seg" ] && return 1
    # Extract extension: everything after the last dot in $seg.
    local ext="${seg##*.}"
    # If there's no dot in the segment (seg == ext), extension-less → REJECT.
    if [ "$ext" = "$seg" ]; then
        return 1
    fi
    # Check ext against _EXTS (space-separated word list).
    local e
    for e in $_EXTS; do
        [ "$ext" = "$e" ] && return 0
    done
    return 1
}

# ---------------------------------------------------------------------------
# Subcommand dispatch
# ---------------------------------------------------------------------------

_subcmd="${1:-}"

case "$_subcmd" in
    classify)
        if [ "${2+set}" != "set" ] || [ -z "${2:-}" ]; then
            printf 'Usage: %s classify <path>\n' "$(basename "$0")" >&2
            exit 2
        fi
        _path="$2"
        if _is_file_path "$_path"; then
            echo "ACCEPT $_path"
            exit 0
        else
            echo "REJECT $_path"
            exit 1
        fi
        ;;

    check)
        # step-6: Cycle 3 GREEN — check list-gate (scaffolded in step-2; Green after step-4 full allowlist).
        shift
        # Collect paths from positional args or stdin.
        if [ "$#" -gt 0 ]; then
            _paths_raw=$(printf '%s\n' "$@")
        else
            _paths_raw=$(cat)
        fi
        # Empty list → ACCEPT ([] defer-to-architect value).
        if [ -z "$_paths_raw" ]; then
            exit 0
        fi
        _rejected=0
        while IFS= read -r _p; do
            # Skip empty/whitespace-only tokens.
            [ -z "${_p// /}" ] && continue
            if ! _is_file_path "$_p"; then
                echo "REJECT $_p"
                _rejected=$((_rejected + 1))
            fi
        done <<< "$_paths_raw"
        if [ "$_rejected" -gt 0 ]; then
            exit 1
        fi
        exit 0
        ;;

    --list-extensions)
        # step-8: Cycle 4 GREEN — print canonical OQ#2 allowlist sorted-unique.
        # Shared α/γ test vector (PRD §11 Q1); drift-guarded by test_lock_charter_guard.sh.
        printf '%s\n' $_EXTS | sort -u
        exit 0
        ;;

    *)
        printf 'Usage: %s classify <path> | check [path...] | --list-extensions\n' "$(basename "$0")" >&2
        printf '  classify <path>    — exit 0=ACCEPT (file), exit 1=REJECT (directory)\n' >&2
        printf '  check [path...]    — exit 0=all-file/empty, exit 1=any-directory; reads stdin if no args\n' >&2
        printf '  --list-extensions  — print canonical extension allowlist (sorted-unique)\n' >&2
        exit 2
        ;;
esac
