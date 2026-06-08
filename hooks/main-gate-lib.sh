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

# main_gate_logfile — path to the append-only audit log of main-ref moves.
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

# main_gate_enforce_on — is ENFORCE active? (return 0 = yes → abort unsanctioned
# main moves). Resolved from a DURABLE, multi-source switch because an env var
# does not reliably reach the `git update-ref` subprocess this hook fires under
# (the orchestrator merge worker is a long-lived process; task 4367). Precedence:
#   1. env REIFY_MAIN_GATE_ENFORCE — override when set non-empty: "1" => on,
#      any other non-empty value => off (overrides a durable on).
#   2. git config --bool reify.mainGate.enforce == true
#   3. flag file  <git-common-dir>/reify-main-gate-enforce  exists
# Unset/empty env falls through to the durable sources (config, then flag file).
main_gate_enforce_on() {
    case "${REIFY_MAIN_GATE_ENFORCE:-__unset__}" in
        1) return 0 ;;
        __unset__) ;;                  # unset/empty → consult durable sources
        *) return 1 ;;                 # any explicit non-1 value → force off
    esac
    [ "$(git config --bool --get reify.mainGate.enforce 2>/dev/null)" = "true" ] && return 0
    [ -e "$(git rev-parse --git-common-dir 2>/dev/null)/reify-main-gate-enforce" ] && return 0
    return 1
}

# main_gate_bypass_on — is BYPASS active? (return 0 = yes → always allow,
# break-glass). Same durable precedence shape as main_gate_enforce_on, over the
# bypass sources: env REIFY_MAIN_GATE_BYPASS / git config reify.mainGate.bypass /
# flag file <git-common-dir>/reify-main-gate-bypass.
main_gate_bypass_on() {
    case "${REIFY_MAIN_GATE_BYPASS:-__unset__}" in
        1) return 0 ;;
        __unset__) ;;
        *) return 1 ;;
    esac
    [ "$(git config --bool --get reify.mainGate.bypass 2>/dev/null)" = "true" ] && return 0
    [ -e "$(git rev-parse --git-common-dir 2>/dev/null)/reify-main-gate-bypass" ] && return 0
    return 1
}
