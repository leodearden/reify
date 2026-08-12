#!/usr/bin/env bash
# scripts/jcodemunch-index-reify.sh
#
# The SINGLE `watch --once` index primitive for the canonical reify checkout.
#
# Design: docs/prds/jcodemunch-substrate-restoration.md §4.4
#         docs/prds/jcodemunch-substrate-restoration.capability-manifest.md §2/β
#
# Runs one bounded, non-daemon jcodemunch index pass over the PRODUCTION reify
# identity and refuses loudly if the resulting index is missing, empty, or
# silently truncated. Everything else in the substrate chain (γ staleness, δ/ε/ζ
# consumers) builds on this one primitive, so its refusals carry stable
# greppable markers rather than prose:
#
#   E_JC_INDEX_MISSING     the index DB file does not exist
#   E_JC_INDEX_EMPTY       DB unreadable / no `symbols` table / 0 symbol rows
#   E_JC_INDEX_TRUNCATED   indexed file count hit the max_folder_files cap
#   E_JC_INDEX_RUN_FAILED  the indexer child exited non-zero
#
# Usage:
#   scripts/jcodemunch-index-reify.sh                    # index + assert
#   scripts/jcodemunch-index-reify.sh --dry-run          # print the argv, exit 0
#   scripts/jcodemunch-index-reify.sh --check-only       # assert an existing index
#   scripts/jcodemunch-index-reify.sh --project-root DIR # override the target
#
# WHY THE DEFAULT PROJECT ROOT IS THE CANONICAL CHECKOUT, NOT THE INVOKING CWD:
# index identity is per-path and derived (`storage/git_root.py::_local_repo_name`)
# as `local/<basename>-<sha1(abspath)[:8]>`. Running this from a warm lane would
# mint a lane-private index (`local/_lane-NN-…`) and leave the production
# identity `local/reify-4ae45bbd` untouched — silently reproducing PRD §2.2's
# "103 per-agent worktree indexes, no reify index". This script's whole purpose
# is the PRODUCTION identity, so it names it explicitly.
#
# BANNED FLAG — the `--paths-from` option is NEVER constructed here (PRD §4.4).
# It short-circuits discovery (`index_folder.py:1505-1511`) and then DELETEs
# every previously-indexed file absent from the list (`sqlite_store.py:1698`,
# `:1480-1483`) with no warning. It is only reachable on the `index` subparser
# (`server.py:6505`), never on `watch`, so this script uses the `watch`
# subcommand exclusively and constructs the bare one-shot form and nothing more.
#
# Prerequisites: uvx (https://docs.astral.sh/uv/), sqlite3, jq.

set -euo pipefail

# The canonical checkout whose index identity this script exists to maintain.
DEFAULT_PROJECT_ROOT="/home/leo/src/reify"

usage() {
    cat <<'USAGE'
Usage: scripts/jcodemunch-index-reify.sh [--project-root DIR] [--dry-run] [--check-only]

Runs one bounded `watch --once` jcodemunch index pass over the canonical reify
checkout, then asserts the resulting index is present, non-empty, and not
silently truncated by the max_folder_files cap.

  --project-root DIR  Index DIR instead of the canonical /home/leo/src/reify.
                      Index identity is per-path, so this changes which index
                      is written (local/<basename>-<sha1(abspath)[:8]>).
  --dry-run           Print the exact indexer argv that would be run, exit 0.
  --check-only        Skip the indexer; run identity resolution and the index
                      assertions against the already-present DB only.
  -h, --help          Show this help and exit.

Refusal markers (stderr, always non-zero exit):
  E_JC_INDEX_MISSING     the index DB file does not exist
  E_JC_INDEX_EMPTY       DB unreadable / no `symbols` table / 0 symbol rows
  E_JC_INDEX_TRUNCATED   indexed file count hit the max_folder_files cap
  E_JC_INDEX_RUN_FAILED  the indexer child exited non-zero
USAGE
}

PROJECT_ROOT="$DEFAULT_PROJECT_ROOT"
DRY_RUN=0
CHECK_ONLY=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --project-root)
            [ "$#" -ge 2 ] || { echo "jcodemunch-index-reify.sh: --project-root requires a DIR argument" >&2; usage >&2; exit 64; }
            PROJECT_ROOT="$2"; shift 2 ;;
        --project-root=*) PROJECT_ROOT="${1#*=}"; shift ;;
        --dry-run)    DRY_RUN=1; shift ;;
        --check-only) CHECK_ONLY=1; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)
            echo "jcodemunch-index-reify.sh: unknown argument '$1'" >&2
            usage >&2
            exit 64
            ;;
    esac
done

say() { printf 'jcodemunch-index-reify: %s\n' "$*"; }
die() { printf 'jcodemunch-index-reify: %s\n' "$*" >&2; exit 1; }

# ── Identity resolution ──────────────────────────────────────────────────────
#
# Mirrors jcodemunch_mcp/storage/git_root.py::_local_repo_name at the pinned
# 1.108.54:
#     f"{folder_path.name}-{sha1(str(folder_path)).hexdigest()[:8]}"
# where folder_path is the root AFTER `Path(p).expanduser().resolve()` (which
# watcher.py::sync_folders applies before handing it on). The sha1 is therefore
# over the RESOLVED ABSOLUTE PATH STRING — `readlink -f` is the bash equivalent
# (absolutise + resolve symlinks, non-strict on a missing leaf).
#
# The DB path derives from that via IndexStore._db_path -> _repo_slug(owner,
# name) = f"{owner}-{name}", i.e. `local-<repo-name>.db` under CODE_INDEX_PATH.
#
# Deliberately does NOT stat the project root: identity is a PURE FUNCTION of
# the path string, and keeping it so is what lets the whole contract be tested
# against throwaway paths. Existence is checked later, only where it matters —
# immediately before the indexer actually runs.
sha1_hex() {
    if command -v sha1sum >/dev/null 2>&1; then
        printf '%s' "$1" | sha1sum | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then
        printf '%s' "$1" | shasum -a 1 | cut -d' ' -f1
    else
        die "neither sha1sum nor 'shasum -a 1' is available; cannot derive the index identity"
    fi
}

# Expand a leading `~` ourselves: the path may arrive quoted (from a config or
# another script), in which case the caller's shell never expanded it, and
# readlink -f would resolve a literal './~' relative to cwd.
case "$PROJECT_ROOT" in
    "~")   PROJECT_ROOT="$HOME" ;;
    "~/"*) PROJECT_ROOT="$HOME/${PROJECT_ROOT#\~/}" ;;
esac
PROJECT_ROOT="$(readlink -f -- "$PROJECT_ROOT")" \
    || die "could not resolve --project-root to an absolute path"

REPO_NAME="$(basename -- "$PROJECT_ROOT")-$(sha1_hex "$PROJECT_ROOT" | cut -c1-8)"
REPO_ID="local/$REPO_NAME"
CODE_INDEX_DIR="${CODE_INDEX_PATH:-$HOME/.code-index}"
DB_PATH="$CODE_INDEX_DIR/local-$REPO_NAME.db"

# Printed on EVERY run, including the refusal paths below: which index identity
# was touched is the single most load-bearing fact this script reports (PRD §2.2
# was a whole fleet writing to the wrong ones), so it must never be conditional
# on success.
say "project-root  $PROJECT_ROOT"
say "repo-id       $REPO_ID"
say "db-path       $DB_PATH"
