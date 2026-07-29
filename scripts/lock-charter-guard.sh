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
#
#   STATUS 2026-07-29 (#5726): the γ copies still pin the pre-#5726 36-entry
#   vector and are NOT yet updated — dark-factory shared/src/shared/locking.py
#   ::CODE_EXTENSIONS, fused-memory/src/fused_memory/middleware/lock_charter_guard.py
#   ::CODE_EXTENSIONS, and fused-memory/tests/test_lock_charter_guard.py
#   ::_CANONICAL_EXTENSIONS (plus shared/tests/test_locking.py::_CANONICAL_EXTENSIONS).
#   TRACKED BY dark_factory:3117 — "Mirror the widened lock-charter extension
#   allowlist (36 -> 58) into the three dark-factory copies", status pending,
#   priority high, external_deps ["reify:5726"] (re-verified live 2026-07-29 via
#   get_task(id=3117, project_root=/home/leo/src/dark-factory); an earlier sweep
#   that reported this id absent queried the wrong tracker).  Its predecessor
#   reify #5737 was cancelled as a cross-repo misfile and drained INTO 3117 —
#   cite 3117, never #5737.  (Origin ticket: tkt_0RRT3KW6B9KF72BHDY5Q038R7Y.)
#   Until 3117 lands: (i) fused-memory's Tier-2 drift test
#   test_extension_drift_guard_vs_reify_script compares this script's output
#   against 36 entries and FAILS in the dark-factory .worktrees/<n>/ layout
#   (where Path(__file__).parents[5] resolves to <src>/ and finds this script);
#   (ii) submit_task / scheduler still REJECT the extensions added below, so
#   declaring e.g. tests/infra/run-all-classification.manifest in a lock charter
#   still fails at the γ enforcement point.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Canonical extension allowlist (OQ#2 resolved — PRD §11 Q2).
# Single source of truth for the α (reify) enforcement point: used by
# _is_file_path(), classify, and --list-extensions.  It is NOT globally
# authoritative while the γ copies lag — see the "Cross-repo seam: γ" status
# note in the header above.
# PRD-explicit: rs ri toml cpp c h hpp md json yaml yml lock py sh ts tsx js txt step stl
# Corpus-evidenced: css mjs html jsonc gcode service
# Common source siblings: cc cxx hh mts cts cjs jsx scss svg png
# git-ls-files sweep 2026-07-28 (#5726) — 22 tracked-file extensions across reify +
# dark-factory that this list misclassified as directories; supersedes #4676's
# "OQ#2 resolved" completeness claim FOR THIS LIST, which was a 22-extension
# undercount:
#   conf diff envrc example example-systemd-config gitattributes gitignore gitkeep
#   gitmodules golden grammar icns ico jq jsonl log manifest npmrc python-version
#   template timer typed
# ---------------------------------------------------------------------------
_EXTS="c cc cjs conf cpp css cts cxx diff envrc example example-systemd-config gcode gitattributes gitignore gitkeep gitmodules golden grammar h hh hpp html icns ico jq js json jsonc jsonl jsx lock log manifest md mjs mts npmrc png py python-version ri rs scss service sh step stl svg template timer toml ts tsx txt typed yaml yml"

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
            # Strip trailing carriage return (CRLF-encoded stdin).
            _p="${_p%$'\r'}"
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
