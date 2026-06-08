#!/usr/bin/env bash
# scripts/setup-main-gate-worktree-config.sh — Seed per-worktree core.hooksPath
# via extensions.worktreeConfig so the main-gate landing gate stays live even when
# Claude Code's native worktree feature writes to shared .git/config.
#
# Claude Code's worktree-enter/exit rewrites the SHARED .git/config core.hooksPath
# to git's inert .git/hooks samples dir and never restores it.  That would darken
# hooks/reference-transaction (the main-move tripwire), hooks/pre-commit, and
# hooks/pre-merge-commit.  Defense-in-depth (B): once extensions.worktreeConfig is
# ON, each worktree can carry its OWN core.hooksPath in config.worktree.  Git reads
# config.worktree FIRST, so the per-worktree value beats any shared-config clobber.
#
# Usage:
#   scripts/setup-main-gate-worktree-config.sh [target_dir]
#
#   target_dir  Optional path to the git worktree root to configure.
#               Defaults to the repo root (one level up from this script).
#
# Idempotent: safe to run multiple times.  Exits 0 on success, non-zero on error.
# All diagnostics go to stderr; nothing is written to stdout.
#
# Order matters:
#   1. Enable extensions.worktreeConfig  (MUST come first)
#   2. Write config.worktree core.hooksPath = hooks
#
# If extensions.worktreeConfig is NOT enabled first, `git config --worktree` is
# identical to `--local` and writes to the shared .git/config — the exact file
# whose value this script is trying to stop depending on.

set -euo pipefail

# ── resolve target dir ────────────────────────────────────────────────────────

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    echo "Usage: $(basename "$0") [target_dir]" >&2
    echo "" >&2
    echo "  Seed per-worktree core.hooksPath=hooks via extensions.worktreeConfig." >&2
    echo "  target_dir defaults to the repo root (one level up from this script)." >&2
    exit 0
fi

if [ $# -gt 1 ]; then
    echo "Usage: $(basename "$0") [target_dir]" >&2
    exit 1
fi

TARGET="${1:-"$(cd "$_SCRIPT_DIR/.." && pwd)"}"

if [ ! -d "$TARGET" ]; then
    echo "ERROR: target directory does not exist: $TARGET" >&2
    exit 1
fi

# ── pre-flight leak guard ─────────────────────────────────────────────────────
# git's documentation for extensions.worktreeConfig requires that core.bare (when
# true) and core.worktree be moved out of shared config before the extension is
# enabled, or they get mis-scoped per-worktree.  Reify currently has neither, but
# a loud abort here protects against a future repo state.

_bare="$(git -C "$TARGET" config --get core.bare 2>/dev/null || true)"
if [ "$_bare" = "true" ]; then
    echo "ERROR: core.bare=true is set in shared config." >&2
    echo "  Move core.bare to the main worktree's config.worktree before enabling" >&2
    echo "  extensions.worktreeConfig, or the bare flag will be scoped per-worktree." >&2
    exit 1
fi

_worktree="$(git -C "$TARGET" config --get core.worktree 2>/dev/null || true)"
if [ -n "$_worktree" ]; then
    echo "ERROR: core.worktree is set in shared config (value: $_worktree)." >&2
    echo "  Move core.worktree to the main worktree's config.worktree before enabling" >&2
    echo "  extensions.worktreeConfig, or it will be scoped per-worktree." >&2
    exit 1
fi

# ── step 1: enable extensions.worktreeConfig ─────────────────────────────────
# Must be done BEFORE the --worktree write so the write lands in config.worktree
# rather than aliasing to --local (shared .git/config).

git -C "$TARGET" config extensions.worktreeConfig true

# ── step 2: seed per-worktree core.hooksPath ─────────────────────────────────
# Uses the relative value 'hooks' (resolves to <worktree_root>/hooks/) to match
# dark-factory's existing create_worktree write and to stay independent of fix (A)'s
# .git/hooks -> ../hooks symlink.

git -C "$TARGET" config --worktree core.hooksPath hooks
