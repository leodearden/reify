#!/usr/bin/env bash
# scripts/ensure-warm-base.sh — Autonomous-first self-heal ladder for the
# warm-lane CoW pool base (<mount>/base/target), run as a boot oneshot.
#
# An absent/empty base leaves the warm pool dead until the next merge-landing
# (refresh-warm-base.sh is normally the only builder). This script self-heals
# at boot via a 4-rung ladder where escalation is the LAST rung, not the
# first — autonomous remediation is always attempted before any human is
# paged.
#
# The ladder:
#   Rung 1 — base present & non-empty (warm-lane-preflight.sh Check 3
#            predicate, symlink-following) -> exit 0, silent no-op.
#            Idempotent: safe to run repeatedly.
#   Rung 2 — base absent/dangling AND a warm source survives
#            (<merge-verify>/target non-empty, clean git worktree) -> CHEAP
#            reflink reseed via refresh-warm-base.sh --landed-commit
#            "$(git -C <mv> rev-parse HEAD)" <mv>/target <base> (seconds).
#            On success -> exit 0 silent, NO cold-build, NO escalation.
#            On reseed failure / base still absent -> escalate + exit non-zero
#            (exceptional FS/source fault; P2 forbids a silent full-copy
#            fallback).
#   Rung 3 — base absent AND no warm source (fresh/wiped partition) -> AUTO
#            cold-build via seed-warm-base-initial.sh, in the BACKGROUND by
#            default (REIFY_WARM_BASE_HEALTH_BUILD_ASYNC=1) so the unit/
#            orchestrator is never blocked for the multi-hour build (inv.6
#            fail-open). Logs loudly at operational level; exits 0 (a known
#            deterministic remedy is underway — NOT an escalation). If the
#            backgrounded build later fails, the background wrapper emits the
#            rung-4 escalation signal to the journal.
#   Rung 4 — remediation FAILS (cold-build errors, or base still absent after
#            a remediation attempt) -> emit exactly ONE escalation signal +
#            exit non-zero.
#
# Escalation-signal contract (reify ships the primitive; dark-factory wires
# the issue-filing): a boot-time systemd oneshot has no MCP/orchestrator
# context, so it cannot call escalate_blocker directly. Following the
# warm-lane-ref-check.sh stdout-contract precedent (stdout carries the single
# machine-consumable value; all diagnostics on stderr), escalate() emits
# EXACTLY ONE once-guarded stdout token line:
#     REIFY_WARM_BASE_HEALTH_ESCALATION <reason>
# plus a loud stderr diagnostic, and the script exits non-zero. The companion
# dark-factory BASE_ABSENT task greps the unit's journal for this token and
# files the one host-scoped issue. Rungs 1-3 keep stdout EMPTY (grep -c == 0);
# only rung 4 emits the one token (grep -c == 1).
#
# Usage:
#   scripts/ensure-warm-base.sh [OPTIONS]
#
# Options (env defaults shown):
#   --mount DIR         Warm-lane mount point (env: REIFY_WARM_LANE_MOUNT)
#   --base-dir DIR      Base directory to heal (env: REIFY_WARM_LANE_BASE;
#                       default: <mount>/base/target)
#   --merge-verify DIR  _merge-verify worktree, the rung-2 warm source
#                       (default: <mount>/worktrees/_merge-verify)
#   --sync              Run rung-3's cold-build inline instead of backgrounding
#                       it (equivalent to REIFY_WARM_BASE_HEALTH_BUILD_ASYNC=0)
#   -h, --help          Print this message and exit 0.
#
# Env:
#   REIFY_WARM_BASE_HEALTH_REFRESH_CMD   Override the refresh-warm-base.sh
#                                        invocation (default: $_SCRIPT_DIR/
#                                        refresh-warm-base.sh). Test seam.
#   REIFY_WARM_BASE_HEALTH_SEED_CMD      Override the seed-warm-base-initial.sh
#                                        invocation (default: $_SCRIPT_DIR/
#                                        seed-warm-base-initial.sh). Test seam.
#   REIFY_WARM_BASE_HEALTH_BUILD_ASYNC   1 (default) = rung 3 backgrounds the
#                                        cold build; 0 = run inline (--sync).
#
# Stdout: EMPTY on rungs 1-3. Rung 4 emits exactly one
#         "REIFY_WARM_BASE_HEALTH_ESCALATION <reason>" line.
# Stderr: all progress/diagnostic messages.
#
# Exit codes: 0 = healthy or autonomously remediated (or a rung-3 cold-build
#             was kicked off in the background); non-zero = rung 4 escalation,
#             or a usage error.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { printf '\033[1;33m[hint]\033[0m  %s\n' "$*" >&2; }

# ── locate script dir ──────────────────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── unimplemented skeleton: no CLI/ladder logic yet ───────────────────────────
# Filled in by the TDD steps in .task/plan.json. Gives the hermetic test
# harness a real executable target path before any behavior is wired up.
err "ensure-warm-base.sh: not yet implemented (skeleton only)."
exit 1
