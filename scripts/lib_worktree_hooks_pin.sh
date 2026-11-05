#!/usr/bin/env bash
# scripts/lib_worktree_hooks_pin.sh — the ONE definition of a worktree's
# core.hooksPath PIN: its canonical value, where it lives, and how to read it.
#
# Designed to be sourced, not executed directly.  Consumers:
#   scripts/setup-main-gate-worktree-config.sh   writes the pin
#   scripts/hooks-armed-guard.sh                 checks it
#
# THE PIN is core.hooksPath in a worktree's OWN config.worktree, set to
# WORKTREE_HOOKS_PIN.  Git anchors a relative core.hooksPath at the worktree
# root, so the value names that worktree's own hooks/ dir; it also matches
# dark-factory's create_worktree write.
#
# READ ORDER: extensions.worktreeConfig first.  With the extension off,
# `git config --worktree` aborts when the store has several worktrees and
# reads the shared .git/config when it has one, so neither answers "what does
# THIS worktree pin".

WORKTREE_HOOKS_PIN=hooks

# worktree_config_enabled <dir> — is extensions.worktreeConfig on in <dir>'s store?
worktree_config_enabled() {
    [ "$(git -C "$1" config --local --bool --get extensions.worktreeConfig 2>/dev/null || true)" = "true" ]
}

# worktree_hooks_pin <dir> — print <dir>'s own config.worktree core.hooksPath,
# or nothing when it has none or the extension is off.
worktree_hooks_pin() {
    worktree_config_enabled "$1" || return 0
    git -C "$1" config --worktree --get core.hooksPath 2>/dev/null || true
}
