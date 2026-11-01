#!/usr/bin/env bash
# scripts/reify-audit-predone-wrapper.sh
#
# Pre-done hook wrapper for reify-audit (post-Taskmaster removal).
#
# WHY THIS WRAPPER EXISTS
# -----------------------
# reify-audit carries its own fused-memory MCP client (task 3736) and is the
# one task-corpus adapter (crates/reify-audit/src/fused_memory_client.rs). On
# --pre-done it fetches the one flipping task with get_task, so the hook no
# longer pulls the whole corpus. Until task 9269 this wrapper fetched that
# corpus itself with get_tasks (87 MB / 12.4 s under load, past its old 10 s
# curl budget) and mapped it through a jq filter into a --tasks-file snapshot.
#
# What the wrapper still owns:
#   - self-location, so the hook works from any checkout;
#   - the fail-open freshness guard and its durable advisory channel;
#   - the --runs-db / --project-root defaults, prepended ahead of the caller's
#     argv so that a caller's own copy wins (reify-audit parses duplicate flags
#     last-wins).
# The fused-memory endpoint is NOT one of them: reify-audit owns it
# (FUSED_MEMORY_URL, else its built-in default), and the wrapper passes no
# --fused-memory-url, so that env var reaches the binary unshadowed.
#
# DESIGN REFERENCE
# ----------------
# docs/architecture-audit/f-infra-design.md §11 (D-1 row), §11.1
# Root-cause: task 3731
#
# SYSTEMD WIRING (live since 2026-08-30; see design §11.1.3)
# ----------------------------------------------------------
# /home/leo/.config/systemd/user/fused-memory.service line 54 carries:
#   Environment=FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done
# Deploy/re-deploy that wiring with scripts/deploy-reify-audit-predone-hook.sh
# (task 6939) -- idempotent; installs the binary, asserts freshness, rewrites the
# unit with a backup, reloads + restarts, and re-probes end-to-end. Do NOT
# hand-edit the unit. Verify with: bash scripts/smoke-predone-hook.sh
#
# USAGE
# -----
#   reify-audit-predone-wrapper.sh --task <id> --pre-done [additional reify-audit flags...]
#   reify-audit-predone-wrapper.sh --help

set -euo pipefail

# ── Self-locate so the wrapper works from any worktree ───────────────────────
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Freshness guard ──────────────────────────────────────────────────────────
# Source the shared freshness guard library. The guard is called in WARN-OPEN
# mode after flag validation:
#   present + executable + stale -> loud advisory on stderr, rc 0 (FAIL OPEN:
#                                   the stale detector still runs and still
#                                   gates on its own findings)
#   absent / not executable      -> E_AUDIT_BIN_MISSING, rc 125
#   REIFY_AUDIT_FRESHNESS_STRICT=1 -> present-but-stale also refuses (rc 125)
#
# WHY NOT REFUSE (task #7139). This is a SYNCHRONOUS pre-done hook: dark-factory's
# fused_memory/middleware/pre_done_hook.py allows the status flip only on rc 0
# and blocks it on ANY non-zero rc. Under REFUSE, one stale binary therefore
# wedged EVERY done-flip in the project until a human reinstalled — and
# crates/reify-audit lands ~4 commits/day, so the freshness epoch advances
# several times a day and that outage recurred by construction (~15.7h over
# 2026-08-30/31). Running a STALE detector is strictly the pre-existing risk
# profile of the binary already installed; being unable to mark ANY work done
# is not. This also matches reify-audit's own house rule — p5_phantom_done.rs
# degrades a would-be-blocking High to an "[advisory - ...]" exit 0 on
# incomplete evidence, and binary age is weaker evidence than a degraded git
# leg, so fail-closed here was an inversion.
#
# WHY NOT REBUILD either: auto-install on the per-done-flip hot path cannot fit.
# The hook runs INSIDE fused-memory's per-project write lock with a 30s timeout
# (pre_done_hook.py:25-31, :151), while `cargo install --path
# crates/reify-audit` pulls the whole reify compiler stack via
# reify-test-support. It would convert a refusal into a TIMEOUT refusal and
# serialize every task mutation on the project behind a 30s stall per flip.
#
# The operator reinstall command:
#   cargo install --path crates/reify-audit --root ~/.cargo --force
#
# The advisory that fail-open emits is NOT delivered on stderr alone — see
# "Durable advisory channel" below for why stderr is a dead channel on an rc-0
# run through the live hook.
# shellcheck source=scripts/reify-audit-freshness.sh
source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

# ── Constants ────────────────────────────────────────────────────────────────
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-/home/leo/.cargo/bin/reify-audit}"
RUNS_DB="${REIFY_AUDIT_RUNS_DB:-$REPO_ROOT/data/orchestrator/runs.db}"
# One-line durable stamp for whatever the freshness guard says; see the
# "Durable advisory channel" block below for why stderr alone is not enough.
# Defined here (not there) because usage() interpolates it and --help
# short-circuits above that point.
ADVISORY_SENTINEL="${REIFY_AUDIT_ADVISORY_SENTINEL:-${TMPDIR:-/tmp}/reify-audit-predone-advisory.$(id -u)}"

# ── Usage ────────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: reify-audit-predone-wrapper.sh --task <id> --pre-done [OPTIONS...]

Runs the reify-audit freshness guard, then invokes reify-audit, whose own
live loader fetches the task from the fused-memory MCP.

Required flags (passed through to reify-audit):
  --task <id>        Task id to check
  --pre-done         Run P5 pre-done check only

Optional flags (passed through to reify-audit; each overrides its default):
  --since <date>     Window sweep from ISO date
  --pattern P1|P2|P5 Restrict to one detector
  --fused-memory-url <url> Override the MCP endpoint (default: reify-audit's own;
                     see reify-audit --help)
  --runs-db <path>   Override runs.db (default: $RUNS_DB)
  --project-root <path> Override repo root (default: $REPO_ROOT)

Wrapper-local flags (NOT passed to reify-audit):
  --help, -h         Show this help and exit 0

Environment overrides:
  FUSED_MEMORY_URL           MCP endpoint, read by reify-audit itself (the
                             wrapper passes no --fused-memory-url of its own)
  REIFY_AUDIT_BIN            Path to reify-audit binary (default: $REIFY_AUDIT_BIN)
  REIFY_AUDIT_RUNS_DB        Path to runs.db (default: $RUNS_DB)
  REIFY_AUDIT_FRESHNESS_STRICT  Set to 1 to REFUSE (125) on a stale binary
                             instead of falling open with an advisory.
                             Default (unset/0) is fail-open.
  REIFY_AUDIT_ADVISORY_SENTINEL  Path of the one-line durable advisory stamp
                             (default: $ADVISORY_SENTINEL). Written only when
                             the freshness guard has something to say; absent
                             means healthy. See "Durable advisory channel".

Exit codes mirror reify-audit:
  0       No High-severity findings
  1-254   Count of High-severity findings
  125     Infrastructure error: a missing flag; MCP unavailability (reported
          by reify-audit itself); or the freshness guard refusing: no
          runnable reify-audit binary [E_AUDIT_BIN_MISSING], or a stale one
          with REIFY_AUDIT_FRESHNESS_STRICT=1 armed [E_AUDIT_BIN_STALE].
          A merely-stale binary does NOT exit 125: it emits an
          E_AUDIT_BIN_STALE advisory and runs anyway (see Freshness guard).
EOF
}

# ── --help / -h short-circuit (before any MCP calls) ────────────────────────
# Note: --help/-h is matched anywhere in argv, including positions that would
# normally be flag values (e.g. `--task --help`). This is an intentional
# convenience — operators use --help to discover the interface, and the
# ambiguity is harmless in practice (the systemd hook never passes --help).
for arg in "$@"; do
    case "$arg" in
        --help|-h)
            usage
            exit 0
            ;;
    esac
done

# ── Validate --task is present ───────────────────────────────────────────────
task_id=""
for i in "$@"; do
    if [ "$i" = "--task" ]; then
        task_found_flag=1
    elif [ "${task_found_flag:-0}" = "1" ]; then
        task_id="$i"
        task_found_flag=0
    fi
done

if [ -z "$task_id" ]; then
    echo "reify-audit-predone-wrapper.sh: error: requires --task <id>" >&2
    echo "" >&2
    usage >&2
    exit 125
fi

# Reject flag-shaped task ids (e.g. `--task --pre-done` with no id supplied).
# The loop above would set task_id to the next argv token regardless of whether
# it looks like a flag; a leading `--` means the caller forgot the task id.
case "$task_id" in
    --*)
        echo "reify-audit-predone-wrapper.sh: error: --task value looks like a flag ('$task_id'); did you forget the task id?" >&2
        echo "" >&2
        usage >&2
        exit 125
        ;;
esac

# ── Durable advisory channel (task #7139 review) ────────────────────────────
# On the LIVE hook path, stderr is a DEAD channel for an rc-0 run. dark-factory's
# fused_memory/middleware/pre_done_hook.py launches this wrapper with
# stderr=PIPE and surfaces the captured text (clipped to 2000 chars) ONLY on a
# non-zero exit — `if returncode == 0: return None` discards it. The same trap
# is already on record for the sibling knob in
# docs/architecture-audit/f-infra-design.md §11.1.4: "warn-only makes the gate
# SILENT on the live hook path, not advisory ... captured and discarded ... A
# soak run through the live hook observes nothing."
#
# That matters precisely because the freshness guard now falls OPEN (rc 0) on a
# stale-but-runnable binary. On stderr alone the E_AUDIT_BIN_STALE alarm would
# be dropped on EVERY live done-flip, and since crates/reify-audit lands ~4
# commits/day the steady state would be a fleet permanently running a stale P5
# detector with zero operator-visible signal — the loud outage traded for a
# silent, indefinite degradation, with the token surfacing only under an
# attended deploy probe. So whatever the guard says is ALSO written to two
# channels the hook cannot swallow:
#
#   1. the systemd journal, via `logger -t reify-audit-predone`. This wrapper's
#      parent is fused-memory.service, so `journalctl --user -t
#      reify-audit-predone` shows it with no extra wiring.
#   2. a single-line sentinel file. TRUNCATE-written, never appended: the
#      condition persists across every done-flip for hours-to-days, so
#      appending would turn one stale binary into unbounded /tmp growth. Its
#      PRESENCE answers "is the fleet running a stale detector?" and its MTIME
#      answers "as of when?" for any read-only sweep. A healthy run writes
#      nothing, so absence means healthy — the file is never emptied in place.
#
# Both are strictly BEST-EFFORT and neither can change the exit code:
# observability that could block a done-flip would reintroduce exactly the
# outage this task removed.
#
# ADVISORY_SENTINEL itself is defined up in the Constants block, not here:
# usage() interpolates it, and the --help short-circuit runs before this point,
# so a definition here would make `--help` die on an unbound variable under
# `set -u`.

# reify_audit_record_advisory <file-containing-the-guard-stderr>
# Always returns 0.
reify_audit_record_advisory() {
    local msg_file="$1"
    local msg_1line
    # Collapse to ONE line: the guard's messages are deliberately multi-line for
    # a human reading stderr, but both channels here want a single record, and
    # the remedy ("cargo install ...") lives on a later line than the token — so
    # truncating to the first line would drop the fix.
    msg_1line=$(tr '\n' ' ' < "$msg_file" 2>/dev/null | tr -s ' ' || true)
    [ -n "$msg_1line" ] || return 0

    if command -v logger >/dev/null 2>&1; then
        logger -t reify-audit-predone -p user.warning -- "$msg_1line" 2>/dev/null || true
    fi

    # Atomic truncate-write (temp + mv) so a concurrent sweep never reads a
    # half-written record.
    local tmp="${ADVISORY_SENTINEL}.tmp.$$"
    if printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$msg_1line" > "$tmp" 2>/dev/null; then
        mv -f "$tmp" "$ADVISORY_SENTINEL" 2>/dev/null || rm -f "$tmp" 2>/dev/null || true
    else
        rm -f "$tmp" 2>/dev/null || true
    fi
    return 0
}

# ── Freshness check (fail-OPEN, before any MCP calls) ───────────────────────
# A stale-but-runnable REIFY_AUDIT_BIN emits a self-describing advisory and we
# run it anyway; only an UNRUNNABLE binary (or an operator who armed
# REIFY_AUDIT_FRESHNESS_STRICT=1) refuses. See the Freshness guard block at the
# top of this file for why refusing here is an outage, not a safeguard.
#
# The guard's stderr is CAPTURED rather than left to flow, so it can be teed to
# the durable channel above. It is then re-emitted verbatim and unconditionally:
# on the rc-125 path stderr IS surfaced by the hook, and it is what an attended
# or manual invocation reads. The rc is captured explicitly and re-raised, which
# preserves the previous `set -euo pipefail` behaviour (a 125 aborts before any
# MCP call) while keeping this recording step reachable. Do not collapse the
# `|| guard_rc=$?` back into a bare call, and do not add `|| true`.
GUARD_ERR=$(mktemp /tmp/reify-audit-guard-err-XXXXXX)
trap 'rm -f "$GUARD_ERR"' EXIT

guard_rc=0
reify_audit_guard "$REIFY_AUDIT_BIN" warn-open "$REPO_ROOT" 2>"$GUARD_ERR" || guard_rc=$?

if [ -s "$GUARD_ERR" ]; then
    cat "$GUARD_ERR" >&2
    reify_audit_record_advisory "$GUARD_ERR"
fi

rm -f "$GUARD_ERR"
trap - EXIT

if [ "$guard_rc" -ne 0 ]; then
    exit "$guard_rc"
fi

# ── Invoke reify-audit's live loader ────────────────────────────────────────
# Wrapper defaults first, the caller's "$@" last and unconsumed: reify-audit
# parses duplicate flags last-wins (tests/cli.rs duplicate_flags_last_wins), so
# a caller's own --runs-db/--project-root overrides these without the wrapper
# having to parse $@. No --fused-memory-url: reify-audit owns the endpoint.
#
# Idiomatic exit-code forwarding under `set -e`: reify-audit deliberately
# returns 1-254 to indicate the count of High-severity findings (the EXPECTED
# gating signal, not an error). A bare `cmd; rc=$?; exit $rc` would abort on
# `set -e` BEFORE `rc=$?` ran — the propagation would still work by accident
# (bash exits with the child's code on set-e abort), but `rc=$?; exit $rc`
# would be dead code, and any future cleanup/diagnostic code added between
# the invocation and `exit $rc` would be silently skipped. The `|| rc=$?`
# form makes the failure path explicit and keeps the post-invocation block
# reachable. See task 3731 review cycle 2.
rc=0
"$REIFY_AUDIT_BIN" \
    --runs-db "$RUNS_DB" \
    --project-root "$REPO_ROOT" \
    "$@" || rc=$?
exit "$rc"
