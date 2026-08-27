#!/usr/bin/env bash
# hooks/main-gate-lib.sh — shared helpers for the main-branch landing gate.
#
# Sourced by hooks/pre-commit, hooks/pre-merge-commit, hooks/reference-transaction
# and scripts/land.sh. Implements a tiny file-sentinel handshake so the
# reference-transaction tripwire can tell a *sanctioned* move of refs/heads/main
# (one made right after a verify gate passed, or via scripts/land.sh) from an
# *unsanctioned* one (a raw `git update-ref` / `git reset` / fast-forward that
# skips the pre-commit / pre-merge-commit verify gates entirely — the exact gap
# through which an unverified GUI-typecheck break reached main).
#
# Why a file and not an env var: git fires sibling hook processes that do not
# share an environment, so the sanctioning step (pre-commit / pre-merge-commit /
# land.sh) and the consuming step (reference-transaction) cannot hand off a flag
# in memory. The sentinel lives in the *common* git dir (git rev-parse
# --git-common-dir) so it is one shared location across every linked worktree.

# main_gate_sentinel — path to the one-shot "this main move is sanctioned" marker.
main_gate_sentinel() {
    echo "$(git rev-parse --git-common-dir)/reify-main-gate-ok"
}

# main_gate_logfile — path to the append-only audit log of gated ref moves.
# Shared by BOTH arms of hooks/reference-transaction: refs/heads/main landings
# (unprefixed messages) and refs/stash pushes (messages prefixed "stash-guard:",
# so the two event classes stay greppable apart in one file). One log means one
# path to keep durable across every linked worktree and one thing to rotate.
main_gate_logfile() {
    echo "$(git rev-parse --git-common-dir)/reify-main-gate.log"
}

# main_gate_mark — create the sentinel. Called ONLY after a verify gate passes
# (pre-commit / pre-merge-commit) or by scripts/land.sh just before its
# sanctioned merge. An empty file is enough; presence is the signal.
main_gate_mark() {
    : > "$(main_gate_sentinel)"
}

# main_gate_log MSG — append a timestamped line to the audit log and echo it to
# stderr. Failures to write the log never abort the caller.
main_gate_log() {
    local _msg="$1" _ts _line
    _ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || echo '?')"
    _line="main-gate ${_ts}: ${_msg}"
    echo "$_line" >> "$(main_gate_logfile)" 2>/dev/null || true
    echo "$_line" >&2
}

# gate_switch_on <ENV_VAR_NAME> <git-config-key> <flag-file-basename> — the ONE
# implementation of a staged-rollout switch (return 0 = on). Resolved from a
# DURABLE, multi-source switch because an env var does not reliably reach the
# `git update-ref` subprocess a hook fires under (the orchestrator merge worker
# is a long-lived process; task 4367). Precedence:
#   1. env <ENV_VAR_NAME> — override when set non-empty: "1" => on, any other
#      non-empty value => off (overrides a durable on).
#   2. git config --bool <git-config-key> == true
#   3. flag file  <git-common-dir>/<flag-file-basename>  exists
# Unset/empty env falls through to the durable sources (config, then flag file).
#
# The env var is read by INDIRECT expansion with a default (${!name:-__unset__}),
# which keeps the empty-value-falls-through semantics AND stays safe under a
# caller's `set -u`.
gate_switch_on() {
    local _env_name="$1" _config_key="$2" _flag_file="$3"
    case "${!_env_name:-__unset__}" in
        1) return 0 ;;
        __unset__) ;;                  # unset/empty → consult durable sources
        *) return 1 ;;                 # any explicit non-1 value → force off
    esac
    [ "$(git config --bool --get "$_config_key" 2>/dev/null)" = "true" ] && return 0
    [ -e "$(git rev-parse --git-common-dir 2>/dev/null)/$_flag_file" ] && return 0
    return 1
}

# main_gate_enforce_on — is the LANDING-gate ENFORCE active? (return 0 = yes →
# abort unsanctioned refs/heads/main moves).
main_gate_enforce_on() {
    gate_switch_on REIFY_MAIN_GATE_ENFORCE reify.mainGate.enforce reify-main-gate-enforce
}

# main_gate_bypass_on — is the LANDING-gate BYPASS active? (return 0 = yes →
# always allow, break-glass).
main_gate_bypass_on() {
    gate_switch_on REIFY_MAIN_GATE_BYPASS reify.mainGate.bypass reify-main-gate-bypass
}

# stash_guard_enforce_on / stash_guard_bypass_on — the same staged rollout for
# the refs/stash arm of hooks/reference-transaction (the guard against
# `git stash` in a shared checkout; task 5981).
#
# DELIBERATELY SEPARATE SWITCHES, not a reuse of the main-gate pair: these are
# two unrelated rollouts. Coupling them would mean arming the landing gate
# silently armed the stash guard, and a break-glass bypass taken for a red-main
# rollback silently disarmed it. They share the resolution helper above (one
# implementation, no four copies to rot apart) and nothing else.
stash_guard_enforce_on() {
    gate_switch_on REIFY_STASH_GUARD_ENFORCE reify.stashGuard.enforce reify-stash-guard-enforce
}

stash_guard_bypass_on() {
    gate_switch_on REIFY_STASH_GUARD_BYPASS reify.stashGuard.bypass reify-stash-guard-bypass
}
