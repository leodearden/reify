#!/usr/bin/env bash
# scripts/verify.sh — unified verification entrypoint for Reify.
#
# Single source of truth shared by BOTH:
#   - dark-factory-orchestrator.yaml  (test_command / lint_command / type_check_command)
#   - hooks/project-checks + hooks/pre-merge-commit  (main-branch git gate)
# so the two can no longer drift apart.
#
# Usage:
#   verify.sh <test|lint|typecheck|all> [options]
#
# Options:
#   --profile debug|release|both   Which build profile(s) to TEST. Default: debug.
#                                  Ignored by lint/typecheck (single pass each).
#                                  'both' runs debug then release test passes.
#                                  When DF_VERIFY_ROLE=merge and no explicit --profile
#                                  is given, defaults to 'both' automatically so the
#                                  orchestrator merge path gets release coverage.
#                                  DF_VERIFY_ROLE=background (task 5210, dark-factory's
#                                  main-tip integrity sweep) = merge-level COMPLETENESS
#                                  (profile=both default, --scope forced to all, wholesale
#                                  infra pool — same guards as merge) at offline-level idle
#                                  CARGO_PRIO, but — unlike merge — gated (NON-exempt)
#                                  admission: it competes for the test-run semaphore/PSI
#                                  gates on the task FIFO, never the merge fast lane.
#   --scope   all|staged|branch    all     = verify everything (orchestrator / merges).
#                                  staged  = scope by `git diff --cached` (hook fast path).
#                                  branch  = scope by merge-base(main,HEAD) → working tree;
#                                            tracked changes only (committed, staged, unstaged
#                                            tracked modifications — untracked new files not
#                                            classified). Fails wide to all on error.
#                                  Default: all.
#   --narrow                       With --scope staged: narrow test/check/clippy passes to
#                                  the affected-crate set. No-op for --scope branch (already
#                                  narrowing) and --scope all (always full workspace, C1).
#   --include-infra                Also run the cheap static infra checks
#                                  (sync_comments / run_all on the test side;
#                                  pm-standardization / event-inventory on the lint side).
#   --print-plan                   Dry run: build the exact ordered command list and
#                                  print it (shell-quoted, one command per line, env as
#                                  '# ' comments), then exit 0 WITHOUT running anything.
#                                  This is a faithful oracle of what a real run executes:
#                                  the command list is built once and only the leaf
#                                  step (print vs eval) branches on --print-plan.
#   -h|--help                      Show usage.
#
# Environment baked in (mirrors dark-factory-orchestrator.yaml verify_env + .cargo/run-with-occt.sh):
#   - . ~/.cargo/env
#   - RUSTC_WRAPPER=sccache, CARGO_INCREMENTAL=0  (sccache cache shared across worktrees)
#   - CARGO_MAKEFLAGS=--jobserver-auth=fifo:<role-fifo>  ONLY when the role's FIFO exists
#     (else cargo uses its own per-process job pool). Role→FIFO selection:
#       merge → ${REIFY_JOBSERVER_MERGE_FIFO:-/tmp/reify-jobserver-merge}
#       task  → ${REIFY_JOBSERVER_TASK_FIFO:-/tmp/reify-jobserver-task}
#     Var-names and defaults match scripts/jobserver-balancer.py (α, task 4516).
#     This is a COMPILE-time concurrency control; TEST-execution concurrency is
#     bounded by a separate mechanism (the semaphore wrapper + --test-threads=1 below).
#   - OCCT LD_LIBRARY_PATH (snap + /opt/reify-deps). The .cargo/config.toml `runner`
#     remains the primary runtime-lib mechanism for `cargo test`/`cargo run`; this is
#     belt-and-braces for contexts the runner does not cover.
#
# PSI gate (inter-dispatch throttle for multi-worktree verify bursts):
#   REIFY_PSI_GATE_THRESHOLD        — CPU avg10 % ceiling; dispatch waits until below this
#                                      value. Default: 50.
#   REIFY_PSI_GATE_WINDOW           — minimum inter-dispatch spacing in seconds.  Default: 20.
#   REIFY_PSI_GATE_MAX_WAIT         — give-up timeout (seconds); exits 75 (EX_TEMPFAIL) so
#                                      the orchestrator retries.  Default: 1800.
#   REIFY_PSI_GATE_DISABLE          — set to 1 to bypass entirely (no wait, no dispatch touch).
#                                      Emergency break-glass; does not affect coordination state.
#   REIFY_PSI_GATE_POLL             — recheck interval in seconds.  Default: 5.
#                                      (testability knob; reduce in tests for faster runs)
#   REIFY_PSI_GATE_PROC_PATH        — CPU PSI source; defaults to /proc/pressure/cpu.
#                                      (testability knob; override to inject fixture files)
#   REIFY_PSI_GATE_DISPATCH_FILE    — shared coordination timestamp file.
#                                      Default: /tmp/reify-verify-last-dispatch.
#                                      (testability knob; isolate per test case)
#   Memory PSI second dimension (default-ON; backs off on CPU OR memory pressure):
#   REIFY_PSI_GATE_MEM_PROC_PATH    — memory PSI source; default /proc/pressure/memory.
#                                      (testability knob; override to inject fixture files)
#   REIFY_PSI_GATE_MEM_FULL_THRESHOLD — memfull avg10 % ceiling (primary signal: all
#                                        runnable tasks stalled on memory = actively paging).
#                                        Default: 10 (conservative; healthy hosts sit ~0%;
#                                        10% sustained is pathological; tunable).
#                                        Empty = memfull dimension OFF.
#   REIFY_PSI_GATE_MEM_SOME_THRESHOLD — memsome avg10 % ceiling (early-warning).
#                                        Default: empty (OFF). Set to opt-in.
#   Merge bypass, DISABLE break-glass, and admit-vs-requeue timeout semantics are
#   identical to the CPU dimension (shared machinery in cpu_admit; v1 = staggering only).
#   psi-gate action             — `verify.sh psi-gate` runs only the gate and exits;
#                                  used as the first test-phase plan entry (test/all).
#
# Compile-phase admission gate (task 4618 — soft PSI backpressure for clippy/check):
#   REIFY_COMPILE_GATE_THRESHOLD      — CPU avg10 % ceiling for compile admission.
#                                        Default: 85 (well above test gate's 50; a single
#                                        EXEMPT merge holding its reserved core fraction
#                                        does NOT by itself reach 85 — only sustained
#                                        multi-lane oversubscription does).
#                                        Host-portable: PSI avg10 is a kernel-normalized
#                                        stall-%, so no nproc-baked count is introduced.
#   REIFY_COMPILE_GATE_MAX_WAIT       — maximum seconds to wait before ADMITTING anyway
#                                        (fairness floor). Default: 300. On timeout the
#                                        gate returns 0 (admits + warning) — NEVER exit 75.
#                                        This is the fundamental difference from the test
#                                        gate: compile admission is soft backpressure; it
#                                        can delay/stagger a compile start but NEVER requeues.
#   REIFY_COMPILE_GATE_POLL           — recheck interval in seconds. Default: 5.
#                                        (testability knob; reduce in tests for faster runs)
#   REIFY_COMPILE_GATE_PROC_PATH      — CPU PSI source; defaults to /proc/pressure/cpu.
#                                        (testability knob; override to inject fixture files)
#   REIFY_COMPILE_GATE_DISABLE        — set to 1 to bypass entirely. Emergency break-glass.
#   Memory PSI second dimension (default-ON; backs off on CPU OR memory pressure):
#   REIFY_COMPILE_GATE_MEM_PROC_PATH    — memory PSI source; default /proc/pressure/memory.
#                                          (testability knob; override to inject fixtures)
#   REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD — memfull avg10 % ceiling (primary signal).
#                                            Default: 10 (conservative; same reasoning as
#                                            psi_gate; independently tunable). Empty = OFF.
#                                            Unlike the CPU threshold (85 vs 50), both gates
#                                            share the same memory default: memory pressure
#                                            is phase-agnostic and not produced by healthy
#                                            work, so no exemption ratio is needed.
#   REIFY_COMPILE_GATE_MEM_SOME_THRESHOLD — memsome avg10 % ceiling (early-warning).
#                                            Default: empty (OFF). Set to opt-in.
#   Admit-on-timeout (storm-proof) and merge bypass are identical to the CPU dimension.
#   compile-gate action          — `verify.sh compile-gate` runs only the compile gate
#                                  and exits; wired into build_plan() before cargo
#                                  check/clippy for lint/typecheck/all (not pure test).
#                                  DF_VERIFY_ROLE=merge → immediate bypass (CAVEAT 1).
#
# Host-relative compile timeout knobs (task 4621):
#   REIFY_VERIFY_TEST_TIMEOUT   — outer timeout for `cargo nextest run` passes.
#                                  Default 60m (workstation budget, η/4521 × 4.5).
#   REIFY_VERIFY_CLIPPY_TIMEOUT — outer timeout for `cargo clippy` and the
#                                  gui-feature `cargo check -p reify-gui` pass.
#                                  Default 45m.
#   REIFY_VERIFY_CHECK_TIMEOUT  — outer timeout for `cargo check --workspace --tests`.
#                                  Default 30m.
#   Values validated as ^[0-9]+[smhd]?$; invalid → default + stderr warning.
#   Unset → identical render on the workstation (no-op). The leo-laptop verify-only
#   host (16t) may widen these via its dispatch env for per-host-measured budgets.
#
# OCCT safety (task 4451):
#   OCCT C++ globals are PER-PROCESS; cross-process isolation is already provided by
#   cargo's per-test-binary process model (nextest). Intra-run concurrency is bounded
#   by the nextest `occt` test-group (max-threads = 4) in .config/nextest.toml; this
#   limits peak OCCT RSS to ≤4×~2GiB ≈ 8GiB, well within the 32GiB host headroom.
#   The OCCT-touching crate set is defined exactly once in scripts/occt-scope-lib.sh
#   and shared with the nextest.toml filter drift check.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Shared OCCT-scope logic (occt_declared_set / occt_touching_set).
if [ ! -f "$SCRIPT_DIR/occt-scope-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/occt-scope-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/occt-scope-lib.sh
source "$SCRIPT_DIR/occt-scope-lib.sh"

# Shared release-sensitivity scope logic (release_declared_set / release_sensitive_set).
if [ ! -f "$SCRIPT_DIR/release-scope-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/release-scope-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/release-scope-lib.sh
source "$SCRIPT_DIR/release-scope-lib.sh"

# Affected-crate reverse-closure (Phase-2 narrowing: maps changed files → workspace crates).
if [ ! -f "$SCRIPT_DIR/affected-crates-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/affected-crates-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/affected-crates-lib.sh
source "$SCRIPT_DIR/affected-crates-lib.sh"

# Test-run counting semaphore (PRD test-run-concurrency-semaphore §3A/§5 D2/D5/D6).
# Holds one slot (FD 9) across ALL test passes via @@SEMAPHORE_ACQUIRE@@/@@SEMAPHORE_RELEASE@@
# sentinels in the PLAN array (see add_test_passes / executor below).
# Bypassed on DF_VERIFY_ROLE=merge or REIFY_TEST_SEMAPHORE_DISABLE=1; knob
# REIFY_TEST_SEMAPHORE_CONCURRENCY controls the slot count (default 1).
#
# Clock-stop mode (PRD verify-admission-wait-clock-stop §3, task 4837):
#   REIFY_TEST_SEMAPHORE_WAIT=unlimited  — continuous blocking wait on the semaphore;
#                                          never exits 75 (EX_TEMPFAIL). Activates
#                                          clock-stop marker emission. Keep FINITE in
#                                          dark-factory-orchestrator.yaml until task 4838 deploys DF.
#   REIFY_CLOCK_HEARTBEAT_SECS           — interval (s) between @@REIFY_CLOCK_HEARTBEAT@@
#                                          emissions inside the semaphore poll loop.
#                                          Default 30.  Reduce in tests for faster runs.
#
# On any contended semaphore wait (first immediate acquire fails) the acquire path emits
# three markers to stderr via scripts/lib_clock_stop.sh:
#   @@REIFY_CLOCK_STOP@@      reason=test_slot_starvation pid=<pid>   (entering wait)
#   @@REIFY_CLOCK_HEARTBEAT@@ reason=test_slot_starvation waited=<s>  (each H secs)
#   @@REIFY_CLOCK_START@@     reason=test_slot_starvation waited=<s>  (wait over)
# The PSI gate (./scripts/verify.sh psi-gate) emits the same three markers with
# reason=psi_pressure when its requeue wait is contended.
# dark_factory:1916 (task 4838 deploy seam) consumes these markers to exclude the
# marked wait span from verify_command_timeout_secs — preventing spurious exit-124
# timeouts during legitimate slot starvation or PSI-pressure waits.
if [ ! -f "$SCRIPT_DIR/lib_test_semaphore.sh" ]; then
    echo "verify.sh: ERROR — scripts/lib_test_semaphore.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/lib_test_semaphore.sh
source "$SCRIPT_DIR/lib_test_semaphore.sh"

# Shared PSI-admission core (psi_gate / compile_gate thin wrappers; agent shim β).
if [ ! -f "$SCRIPT_DIR/cpu-admit.sh" ]; then
    echo "verify.sh: ERROR — scripts/cpu-admit.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/cpu-admit.sh
source "$SCRIPT_DIR/cpu-admit.sh"

# Process-group teardown + host-wide orphan reaper (task 4872).
if [ ! -f "$SCRIPT_DIR/lib_proc_reaper.sh" ]; then
    echo "verify.sh: ERROR — scripts/lib_proc_reaper.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/lib_proc_reaper.sh
source "$SCRIPT_DIR/lib_proc_reaper.sh"

# Single source of truth for the `heavy` nextest filterset (task 4912/A1).
# Provides REIFY_HEAVY_NEXTEST_FILTER, consumed here by the
# REIFY_GATE_EXCLUDE_HEAVY knob (task 4915/A4, PRD §6/§8 flip-seam contract:
# gate roles apply `-E "not ($REIFY_HEAVY_NEXTEST_FILTER)"` iff the knob is
# exactly "1") and by the sibling `offline` role (A2, not yet landed). The
# lib's own source guard makes a later double-source (once A2 also sources
# it) a harmless no-op.
if [ ! -f "$SCRIPT_DIR/heavy-test-filter-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/heavy-test-filter-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/heavy-test-filter-lib.sh
source "$SCRIPT_DIR/heavy-test-filter-lib.sh"

# Fail loudly at load time (not at a mid-run nextest parse error) if the
# sourced constant is somehow empty — an empty REIFY_HEAVY_NEXTEST_FILTER
# would make the REIFY_GATE_EXCLUDE_HEAVY=1 fragment below `-E "not ()"`,
# which nextest rejects. Mirrors the same check tests/infra/test_verify_gate_exclude_heavy.sh
# performs on its own copy of the sourced value.
if [ -z "${REIFY_HEAVY_NEXTEST_FILTER:-}" ]; then
    echo "verify.sh: ERROR — REIFY_HEAVY_NEXTEST_FILTER empty after sourcing heavy-test-filter-lib.sh" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Host-relative compile timeout resolver (task 4621)
# ---------------------------------------------------------------------------

# _resolve_timeout_knob <env_var_name> <default>
# Validates that the env var value matches ^[0-9]+[smhd]?$ (digits + optional
# single unit suffix: s/m/h/d).  Returns the env value verbatim if valid, else
# returns the default and emits a stderr warning (non-empty invalid only).
# Mirrors the strict-digit idiom in gen-nextest-config.sh / parse_debug_port;
# adapted to duration values with an optional unit suffix.
_resolve_timeout_knob() {
    local _name="$1" _default="$2"
    local _val="${!_name:-}"
    # Strip exactly one trailing unit character (if present) to isolate the
    # digit part.  After stripping, the remainder must be purely digits and
    # non-empty to be a valid duration.
    local _core
    case "$_val" in
        (*[smhd]) _core="${_val%[smhd]}" ;;  # strip one trailing unit
        (*)       _core="$_val" ;;
    esac
    case "$_core" in
        (''|*[!0-9]*)
            [ -n "$_val" ] && printf 'verify.sh: WARNING: invalid %s=%s; using default %s\n' \
                "$_name" "$_val" "$_default" >&2
            printf '%s' "$_default"
            ;;
        (*) printf '%s' "$_val" ;;
    esac
}

# Resolve three compile-budget tiers once at startup.  Defaults match the
# workstation-measured budgets (unset → identical render, no-op on workstation).
_VERIFY_TEST_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT 60m)"
_VERIFY_CLIPPY_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_CLIPPY_TIMEOUT 45m)"
_VERIFY_CHECK_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_CHECK_TIMEOUT 30m)"

usage() {
    sed -n '2,51p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# PSI gate — throttle per-task test phases under multi-worktree verify bursts
# ---------------------------------------------------------------------------

# psi_gate() — thin wrapper over cpu_admit requeue (scripts/cpu-admit.sh).
# Called directly via `verify.sh psi-gate` (testable entry point) and wired
# as the first test-phase plan entry by add_test_passes().
#
# Environment knobs (see header comment block for full doc):
#   REIFY_PSI_GATE_THRESHOLD    — avg10 ceiling to allow dispatch (default 50)
#   REIFY_PSI_GATE_WINDOW       — min seconds between dispatches (default 20)
#   REIFY_PSI_GATE_MAX_WAIT     — give-up timeout in seconds (default 1800)
#   REIFY_PSI_GATE_POLL         — recheck interval in seconds (default 5)
#   REIFY_PSI_GATE_PROC_PATH    — PSI source path (default /proc/pressure/cpu)
#   REIFY_PSI_GATE_DISPATCH_FILE— coordination timestamp file
#   REIFY_PSI_GATE_DISABLE      — set to 1 to bypass entirely (no touch)
psi_gate() {
    # DF_VERIFY_ROLE=merge bypass (and all other admission logic) is enforced
    # in cpu_admit; this wrapper just maps REIFY_PSI_GATE_* → _ca_* and delegates.
    #
    # Clock-stop: _ca_clock_reason="psi_pressure" enables cpu_admit's unlimited-mode
    # detection (REIFY_PSI_GATE_MAX_WAIT=unlimited → continuous blocking wait, never
    # exit 75) and the @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@ marker emission on any
    # contended wait via scripts/lib_clock_stop.sh.  The reason= field "psi_pressure"
    # is the canonical token consumed by dark_factory:1916 (task 4838 deploy seam).
    # HEARTBEAT interval: REIFY_CLOCK_HEARTBEAT_SECS (default 30).
    local _ca_threshold="${REIFY_PSI_GATE_THRESHOLD:-50}"
    local _ca_window="${REIFY_PSI_GATE_WINDOW:-20}"
    local _ca_max_wait="${REIFY_PSI_GATE_MAX_WAIT:-1800}"
    local _ca_poll="${REIFY_PSI_GATE_POLL:-5}"
    local _ca_proc_path="${REIFY_PSI_GATE_PROC_PATH:-/proc/pressure/cpu}"
    local _ca_dispatch="${REIFY_PSI_GATE_DISPATCH_FILE:-/tmp/reify-verify-last-dispatch}"
    local _ca_disable="${REIFY_PSI_GATE_DISABLE:-}"
    local _ca_log_prefix="verify.sh"
    local _ca_gate_name="PSI gate"
    local _ca_failopen_txt="PSI gate disabled"
    local _ca_mem_proc_path="${REIFY_PSI_GATE_MEM_PROC_PATH:-/proc/pressure/memory}"
    # Unset-only operator (no colon) is DELIBERATE: unset -> default-ON at 10;
    # an explicit REIFY_PSI_GATE_MEM_FULL_THRESHOLD="" must be preserved as
    # empty (the documented escape hatch, disabling the memory dimension via
    # _cpu_admit_mem_pressure_high's empty-check) rather than coerced back to
    # 10 by a colon-minus. Do not re-add the colon (mirrors task 4911's
    # cpu-admit.sh:~399 fix).
    local _ca_mem_full_threshold="${REIFY_PSI_GATE_MEM_FULL_THRESHOLD-10}"
    local _ca_mem_some_threshold="${REIFY_PSI_GATE_MEM_SOME_THRESHOLD:-}"
    local _ca_clock_reason="psi_pressure"
    cpu_admit requeue
}

# compile_gate() — thin wrapper over cpu_admit admit (scripts/cpu-admit.sh).
# Called directly via `verify.sh compile-gate` (testable entry point) and wired
# as a plan entry in build_plan() before cargo check/clippy (lint/typecheck/all).
#
# Key differences from psi_gate() (preserved via cpu_admit admit mode):
#   - Higher default threshold (85 vs 50): treats a lone exempt merge's core
#     reservation as expected-high-pressure baseline — only sustained multi-lane
#     oversubscription trips it.
#   - Continuous HOLD until PSI drops (task 4920 — admit-on-timeout removed);
#     NEVER exit 75.  compile-gate admission is soft backpressure; it can
#     delay/stagger a compile start but can NEVER requeue a task (storm-proof,
#     CAVEAT 2).  Now IN clock-stop scope (PRD D2 reversed — see below).
#   - RISK NOTE: under *permanent* host saturation (PSI stuck at/above
#     threshold for reasons unrelated to this verify) the hold is indefinite
#     by design — there is no admit-on-timeout floor left.  This applies to
#     EITHER dimension independently, and the two are NOT equally likely to
#     trip it: the memory ceiling (memfull avg10 >= 10% by default) is far
#     more conservative than the CPU ceiling (avg10 >= 85%), so ambient host
#     memory pressure alone — unrelated to this verify, easily reached on a
#     busy multi-tenant box — is the practically more likely indefinite-hold
#     trigger of the two.  Heartbeats keep DF's heartbeat-idle kill from
#     firing and the wait span stays clock-stop-excluded from
#     verify_command_timeout_secs, so a long-parked compile is a HOLD, not a
#     hang; operators triaging one should read /proc/pressure/{cpu,memory}
#     rather than assume a wedge.  This mirrors the PRD's accepted
#     limitation that indefinite starvation under permanent saturation is a
#     capacity problem no verify-layer scheme solves — the lever is
#     dispatch admission
#     (docs/prds/verify-admission-wait-clock-stop.md §6), not this gate.
#   - No WINDOW/dispatch-file/flock: compiles run concurrently under the jobserver.
#
# Environment knobs (see header comment block for full doc):
#   REIFY_COMPILE_GATE_THRESHOLD  — avg10 ceiling (default 85)
#   REIFY_COMPILE_GATE_MAX_WAIT   — inoperative for the hold (task 4920); kept only
#                                   as the defensive reason-less-admit fallback value
#   REIFY_COMPILE_GATE_POLL       — recheck interval in seconds (default 5)
#   REIFY_COMPILE_GATE_PROC_PATH  — PSI source path (default /proc/pressure/cpu)
#   REIFY_COMPILE_GATE_DISABLE    — set to 1 to bypass entirely
compile_gate() {
    # DF_VERIFY_ROLE=merge bypass (CAVEAT 1) and all other admission logic is
    # enforced in cpu_admit; this wrapper maps REIFY_COMPILE_GATE_* → _ca_* and
    # delegates.  No _ca_window / _ca_dispatch: compiles run concurrently under
    # the jobserver (serializing would recreate the throttling it already owns).
    #
    # Clock-stop (task 4920): _ca_clock_reason="psi_pressure" is now set (reused
    # from psi_gate() — dark_factory:1916 has recognized this token since the
    # 2026-06-27 clock-stop deploy, task 4838).  compile_gate's cpu_admit admit
    # mode no longer admits-on-timeout — it HOLDS until PSI drops on either the
    # CPU or memory dimension, emitting @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@
    # on any contended wait.  The wait span is excluded from
    # verify_command_timeout_secs by dark_factory:1916 (marker-based and
    # gate-agnostic, so this reversal needed no dark-factory change/restart —
    # PRD D2's "out of scope for clock-stop" is superseded by this task).
    # REIFY_COMPILE_GATE_MAX_WAIT is now inoperative for the hold (admit mode is
    # unconditionally unlimited once a clock reason is set); it is kept only as
    # the fallback value for cpu_admit's defensive reason-less-admit guard,
    # which compile_gate never takes (a reason is always set here).
    local _ca_threshold="${REIFY_COMPILE_GATE_THRESHOLD:-85}"
    local _ca_max_wait="${REIFY_COMPILE_GATE_MAX_WAIT:-300}"
    local _ca_poll="${REIFY_COMPILE_GATE_POLL:-5}"
    local _ca_proc_path="${REIFY_COMPILE_GATE_PROC_PATH:-/proc/pressure/cpu}"
    local _ca_disable="${REIFY_COMPILE_GATE_DISABLE:-}"
    local _ca_window=""
    local _ca_dispatch=""
    local _ca_log_prefix="verify.sh"
    local _ca_gate_name="compile-gate"
    local _ca_failopen_txt="compile-gate fail-open"
    local _ca_mem_proc_path="${REIFY_COMPILE_GATE_MEM_PROC_PATH:-/proc/pressure/memory}"
    # Unset-only operator (no colon) is DELIBERATE: unset -> default-ON at 10;
    # an explicit REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD="" must be preserved
    # as empty (the documented escape hatch, disabling the memory dimension
    # via _cpu_admit_mem_pressure_high's empty-check) rather than coerced
    # back to 10 by a colon-minus. Do not re-add the colon (mirrors task
    # 4911's cpu-admit.sh:~399 fix).
    local _ca_mem_full_threshold="${REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD-10}"
    local _ca_mem_some_threshold="${REIFY_COMPILE_GATE_MEM_SOME_THRESHOLD:-}"
    local _ca_clock_reason="psi_pressure"
    cpu_admit admit
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
ACTION=""
PROFILE="debug"
PROFILE_EXPLICIT=0   # set to 1 if --profile was given explicitly; keeps explicit authoritative
SCOPE="all"
NARROW=0             # --narrow: opt-in to affected-crate narrowing for --scope staged
INCLUDE_INFRA=0
PRINT_PLAN=0
TEST_THREADS=""      # --test-threads=N: test-execution parallelism cap (offline lane, task 5264). Empty = unset → plan unchanged.

while [ "$#" -gt 0 ]; do
    case "$1" in
        test|lint|typecheck|all|psi-gate|compile-gate)
            if [ -n "$ACTION" ]; then
                echo "verify.sh: ERROR — action already set to '$ACTION', got '$1'" >&2
                exit 64
            fi
            ACTION="$1"; shift ;;
        --profile)
            PROFILE="${2:?--profile requires an argument}"; PROFILE_EXPLICIT=1; shift 2 ;;
        --profile=*)
            PROFILE="${1#*=}"; PROFILE_EXPLICIT=1; shift ;;
        --scope)
            SCOPE="${2:?--scope requires an argument}"; shift 2 ;;
        --scope=*)
            SCOPE="${1#*=}"; shift ;;
        --narrow)
            NARROW=1; shift ;;
        --include-infra)
            INCLUDE_INFRA=1; shift ;;
        --print-plan)
            PRINT_PLAN=1; shift ;;
        --test-threads)
            TEST_THREADS="${2:?--test-threads requires an argument}"; shift 2 ;;
        --test-threads=*)
            TEST_THREADS="${1#*=}"; shift ;;
        -h|--help)
            usage; exit 0 ;;
        *)
            echo "verify.sh: ERROR — unknown argument '$1'" >&2
            usage >&2
            exit 64 ;;
    esac
done

if [ -z "$ACTION" ]; then
    echo "verify.sh: ERROR — missing action (test|lint|typecheck|all)" >&2
    usage >&2
    exit 64
fi
case "$PROFILE" in debug|release|both) ;; *)
    echo "verify.sh: ERROR — invalid --profile '$PROFILE' (want debug|release|both)" >&2; exit 64 ;;
esac
case "$SCOPE" in all|staged|branch) ;; *)
    echo "verify.sh: ERROR — invalid --scope '$SCOPE' (want all|staged|branch)" >&2; exit 64 ;;
esac
DF_VERIFY_ROLE="${DF_VERIFY_ROLE:-task}"
# Role-based PROFILE default: when no explicit --profile was given and the
# orchestrator merge path stamps DF_VERIFY_ROLE=merge, default to 'both' so
# release-only tests are exercised on every merge (matching the local
# hooks/pre-merge-commit gate which also runs --profile both). background
# (task 5210, main-tip integrity sweep) shares this merge-level completeness
# for the same reason: full dev+release coverage on every sweep.
# Explicit --profile always wins; task/unset roles keep debug (fast feedback).
if [ "$PROFILE_EXPLICIT" -eq 0 ] && { [ "$DF_VERIFY_ROLE" = "merge" ] || [ "$DF_VERIFY_ROLE" = "background" ]; }; then
    PROFILE="both"
elif [ "$PROFILE_EXPLICIT" -eq 0 ] && [ "$DF_VERIFY_ROLE" = "offline" ]; then
    # offline (task 4913/A2) is a single-profile deep-test lane: the heavy
    # filterset (PRD §3) only has release-relevant coverage, so 'both' would
    # duplicate the debug workspace pass for no benefit — release only.
    PROFILE="release"
fi
# Probe scheduling-tool availability once; degrade gracefully on non-Linux hosts
# where util-linux may not be installed.
_HAS_NICE=0; _HAS_IONICE=0
command -v nice   >/dev/null 2>&1 && _HAS_NICE=1
command -v ionice >/dev/null 2>&1 && _HAS_IONICE=1
# task 5210: single source of truth for the "idle" priority class shared by
# the offline and background roles below — both want byte-identical
# CARGO_PRIO output (nice -n 19 + ionice -c3, degrading gracefully), so the
# string is computed here once instead of being duplicated per-arm. Each
# caller passes its own role name so the graceful-degrade WARNING text still
# names the correct role; the case arms themselves stay separate (rather than
# folding into `offline|background)`) purely so offline's golden print-plan
# output is untouched — see the comment on the `background)` arm below.
_idle_cargo_prio() {
    local _idle_role="$1"
    if   [ "$_HAS_NICE" -eq 1 ] && [ "$_HAS_IONICE" -eq 1 ]; then
        CARGO_PRIO="nice -n 19 ionice -c3 "
    elif [ "$_HAS_NICE" -eq 1 ]; then
        echo "verify.sh: WARNING — ionice not found; ${_idle_role} role using nice only (no IO throttle)" >&2
        CARGO_PRIO="nice -n 19 "
    else
        echo "verify.sh: WARNING — nice/ionice not found; ${_idle_role} role running at normal priority" >&2
        CARGO_PRIO=""
    fi
}
case "$DF_VERIFY_ROLE" in
    task)
        if   [ "$_HAS_NICE" -eq 1 ] && [ "$_HAS_IONICE" -eq 1 ]; then
            CARGO_PRIO="nice -n 15 ionice -c 2 -n 7 "
        elif [ "$_HAS_NICE" -eq 1 ]; then
            echo "verify.sh: WARNING — ionice not found; task role using nice only (no IO throttle)" >&2
            CARGO_PRIO="nice -n 15 "
        else
            echo "verify.sh: WARNING — nice/ionice not found; task role running at normal priority" >&2
            CARGO_PRIO=""
        fi ;;
    merge)
        if [ "$_HAS_NICE" -eq 1 ]; then
            CARGO_PRIO="nice -n 5 "
        else
            echo "verify.sh: WARNING — nice not found; merge role running at normal priority" >&2
            CARGO_PRIO=""
        fi ;;
    offline)
        _idle_cargo_prio offline ;;
    background)
        # task 5210: background = merge-level completeness at offline-level
        # idle priority + gated (non-exempt) admission. Shares its CARGO_PRIO
        # string with offline via _idle_cargo_prio above (single source of
        # truth for the idle-class priority — see the comment there) — a
        # DEDICATED case arm still keeps offline's arm byte-for-byte
        # untouched, and passing its own role name keeps the graceful-degrade
        # WARNING text background-specific.
        _idle_cargo_prio background ;;
    *)  echo "verify.sh: ERROR — unknown DF_VERIFY_ROLE '$DF_VERIFY_ROLE' (want task|merge|offline|background)" >&2; exit 64 ;;
esac

# Gate-exclusion fragment (task 4915/A4, PRD §6/§8 flip-seam contract): gate
# roles (task/merge) apply `-E "not (<heavy>)"` IFF REIFY_GATE_EXCLUDE_HEAVY is
# EXACTLY the string "1" (exact string equality — never -n, -eq, or a glob).
# Any other value (unset/empty/"0"/garbage) leaves this fragment empty so the
# full test set keeps running unchanged — the strictly-additive-on-landing
# invariant: a malformed knob must never silently create a coverage hole.
# Scoped to gate roles explicitly (not "any role with the knob set") so a
# future `offline` role (A2, which applies the POSITIVE heavy filter) can
# never have this negation misfire against it. Part B (dark-factory
# flip-gate-exclude-heavy) flips this by setting the env var to "1" in
# dark-factory-orchestrator.yaml's verify env, with zero reify code change.
#
# Sequencing precondition (operational, NOT enforced by this script — it has
# no visibility into A2/A6 landing/scheduling state): Part B must not set
# REIFY_GATE_EXCLUDE_HEAVY=1 until the offline heavy-test lane (A2/A6) is
# landed AND actively scheduled. Flipping this knob first redistributes
# heavy coverage nowhere — a genuine coverage hole on main, not a
# redistribution. This is a deploy-runbook responsibility for Part B.
_GATE_HEAVY_EXCLUDE=""
if { [ "$DF_VERIFY_ROLE" = "task" ] || [ "$DF_VERIFY_ROLE" = "merge" ]; } \
    && [ "${REIFY_GATE_EXCLUDE_HEAVY:-}" = "1" ]; then
    _GATE_HEAVY_EXCLUDE=" -E \"not (${REIFY_HEAVY_NEXTEST_FILTER})\""
fi

# Offline positive heavy-filter fragment (task 4913/A2, PRD §3/§6): the
# offline role applies the POSITIVE heavy filter — `-E "(<heavy>)"` plus
# `--run-ignored all` so the #[ignore]'d convergence studies run too. Scoped
# to the offline role only, so it is mutually exclusive with the
# _GATE_HEAVY_EXCLUDE negation above (task/merge only) — both fragments can
# be appended in emit_nextest_pass without ever colliding.
_OFFLINE_HEAVY_SELECT=""
if [ "$DF_VERIFY_ROLE" = "offline" ]; then
    _OFFLINE_HEAVY_SELECT=" -E \"(${REIFY_HEAVY_NEXTEST_FILTER})\" --run-ignored all"
fi

# psi-gate is dispatched EARLY — before MERGE_HEAD check / cd / apply_env —
# so the integration test can drive it without triggering the cargo pipeline.
# Note: psi-gate is execute-only; --print-plan is intentionally ignored here.
# The parent test/all invocation prints the psi-gate command as a normal plan
# line; the psi-gate subprocess itself always executes the gate regardless of
# how it was invoked.
if [ "$ACTION" = "psi-gate" ]; then
    psi_gate
    exit $?
fi

# compile-gate is dispatched EARLY — same idiom as psi-gate: execute-only,
# hermetic, testable in isolation without triggering the cargo pipeline.
# DF_VERIFY_ROLE is already resolved above so the merge bypass works correctly.
if [ "$ACTION" = "compile-gate" ]; then
    compile_gate
    exit $?
fi

# A merge in progress cannot trust `git diff --cached` (the index reflects the
# merge result, not a curated stage), so force a full verification. Detected via
# the git-dir-relative MERGE_HEAD so it works correctly inside linked worktrees.
_MERGE_HEAD="$(git -C "$REPO_ROOT" rev-parse --git-path MERGE_HEAD 2>/dev/null || echo '')"
if [ -n "$_MERGE_HEAD" ] && [ -f "$_MERGE_HEAD" ] && [ "$SCOPE" != "all" ]; then
    echo "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)" >&2
    SCOPE="all"
fi

# Defensive belt-and-braces (contract C2): the merge gate never narrows. The
# dark-factory orchestrator's post-merge verify stamps DF_VERIFY_ROLE=merge;
# force --scope all so a future caller cannot hand the merge gate a narrowing
# scope (branch/staged). Independent of the role-driven --profile default above
# and of the affected-crate machinery. Mirrors the MERGE_HEAD force. background
# (task 5210, main-tip integrity sweep) shares this same never-narrow
# guarantee — an integrity gate must never silently under-cover main.
if { [ "$DF_VERIFY_ROLE" = "merge" ] || [ "$DF_VERIFY_ROLE" = "background" ]; } && [ "$SCOPE" != "all" ]; then
    echo "verify.sh: DF_VERIFY_ROLE=$DF_VERIFY_ROLE — forcing --scope all (integrity gate never narrows, contract C2)" >&2
    SCOPE="all"
fi

# Run all relative-path commands from the repo root, matching how both the
# orchestrator (project_root) and the git hook ($ROOT) invoke verification.
cd "$REPO_ROOT"

# --scope branch: resolve merge-base(main, HEAD) -> working tree diff.
# Fail WIDE (contract C5): detached HEAD / missing local 'main' ref / any
# git failure forces SCOPE=all (full plan) — under-verify ships breakage,
# over-verify just wastes CPU. Assignment inside `if` test keeps set -e clean.
_MERGE_BASE=""
if [ "$SCOPE" = "branch" ]; then
    if _MERGE_BASE="$(git -C "$REPO_ROOT" merge-base main HEAD 2>/dev/null)" && [ -n "$_MERGE_BASE" ]; then
        :
    else
        echo "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' (detached HEAD / missing local main ref / merge-base failure) — failing WIDE to --scope all (contract C5)" >&2
        SCOPE="all"
    fi
fi

# Action → which check families run.
case "$ACTION" in
    test)      DO_TEST=1; DO_LINT=0; DO_TYPECHECK=0 ;;
    lint)      DO_TEST=0; DO_LINT=1; DO_TYPECHECK=0 ;;
    typecheck) DO_TEST=0; DO_LINT=0; DO_TYPECHECK=1 ;;
    all)       DO_TEST=1; DO_LINT=1; DO_TYPECHECK=1 ;;
esac

# Profiles to TEST.
case "$PROFILE" in
    debug)   PROFILES=(debug) ;;
    release) PROFILES=(release) ;;
    both)    PROFILES=(debug release) ;;
esac

# ---------------------------------------------------------------------------
# Environment (process-level; inherited by every command in the plan)
# ---------------------------------------------------------------------------
ENV_LINES=()

# _jobserver_owner_live <fifo> — probe the sidecar owner stamp "<fifo>.owner"
# (written by scripts/jobserver-balancer.py after it opens <fifo> O_RDWR) to
# tell a live custodian apart from a FIFO left behind by a crashed balancer
# (task 5146).  Exit status:
#   0 = LIVE     stamp's pid is alive, and its boot_id matches the host's (or
#                boot_id info is unavailable on either side).
#   1 = STALE    stamp's pid is dead, OR its boot_id positively mismatches the
#                host's (guards post-reboot pid reuse).  Sets _JB_STALE_PID
#                (best-effort) to the stamp's pid field, for the caller's
#                WARNING message.
#   2 = UNKNOWN  no stamp exists (old/foreign balancer that predates this
#                contract) or the stamp is malformed (pid field doesn't match
#                ^[0-9]+$).  Ambiguous is not proof of death, so callers must
#                treat UNKNOWN the same as LIVE (existence-only fallback).
#
#   KNOWN GAP (review comment 3, round 2): a live /proc/<pid> is trusted as
#   proof the ORIGINAL custodian still holds the FIFO, but pid-alive is not
#   pid-identity.  If the balancer crashes and the kernel reuses its exact
#   pid for an unrelated live process within the SAME boot, this probe still
#   returns LIVE (the boot_id check only closes the post-reboot reuse
#   window, not same-boot reuse) and verify.sh would export the stale FIFO's
#   CARGO_MAKEFLAGS — the wedge this task exists to prevent.  This is a
#   residual gap, not a regression (pre-5146 was existence-only, strictly
#   weaker), and same-boot reuse of one specific pid is rare in practice.
#   Closing it fully would require the daemon to also stamp /proc/<pid>/stat
#   field 22 (process start time) and this probe to compare it — not done
#   here.
_jobserver_owner_live() {
    local fifo="$1"
    local stamp="${fifo}.owner"
    _JB_STALE_PID=""
    [ -e "$stamp" ] || return 2

    local pid boot
    read -r pid boot <"$stamp" 2>/dev/null || return 2

    case "$pid" in
        ''|*[!0-9]*) return 2 ;;  # malformed pid field — ambiguous, not proof of death
    esac

    if [ ! -d "/proc/$pid" ]; then
        _JB_STALE_PID="$pid"
        return 1  # STALE: pid not alive
    fi

    local cur
    cur="$(cat /proc/sys/kernel/random/boot_id 2>/dev/null || true)"
    if [ -n "$boot" ] && [ "$boot" != "-" ] && [ -n "$cur" ] && [ "$boot" != "$cur" ]; then
        _JB_STALE_PID="$pid"
        return 1  # STALE: boot_id mismatch (post-reboot pid reuse)
    fi

    return 0  # LIVE
}

apply_env() {
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
        ENV_LINES+=(". $HOME/.cargo/env")
    else
        ENV_LINES+=("# ~/.cargo/env not found — relying on ambient PATH for cargo")
    fi

    export RUSTC_WRAPPER=sccache
    ENV_LINES+=("export RUSTC_WRAPPER=sccache")
    export CARGO_INCREMENTAL=0
    ENV_LINES+=("export CARGO_INCREMENTAL=0")

    # Inherit the shared global jobserver ONLY when the role's FIFO exists AND
    # probes LIVE; otherwise leave CARGO_MAKEFLAGS unset so cargo manages its
    # own job pool. Exporting a stale fifo path when reify-jobserver.service
    # is down would wedge cargo (every rustc blocks forever on token
    # acquisition until the outer timeout fires — task 5146).
    # Role→FIFO selection: merge   → REIFY_JOBSERVER_MERGE_FIFO (default /tmp/reify-jobserver-merge)
    #                       task    → REIFY_JOBSERVER_TASK_FIFO  (default /tmp/reify-jobserver-task)
    #                       offline → neither (task 4913/A2, PRD §8 invariant): the offline
    #                                 lane runs off the merge jobserver entirely, so it never
    #                                 contends with either the task or merge FIFO.
    # Defaults/var-names match scripts/jobserver-balancer.py (α, task 4516).
    #
    # Liveness probe (task 5146): a bare `[ -p "$_jb_fifo" ]` only proves the
    # FIFO exists, not that a custodian holds it — a crashed balancer leaves
    # the FIFO behind with its buffered tokens discarded (task 4515 contract
    # C5), and every rustc would then block forever.  scripts/jobserver-
    # balancer.py stamps "${fifo}.owner" = "<pid> <boot_id>" right after
    # opening each FIFO O_RDWR (a clean exit unlinks both; a crash/SIGKILL
    # deliberately leaves the stamp naming the now-dead pid).  See
    # _jobserver_owner_live() above for the LIVE/STALE/UNKNOWN contract.
    # Break-glass: REIFY_JOBSERVER_SKIP_LIVENESS_PROBE=1 reverts to the
    # pre-5146 existence-only guard (mirrors REIFY_MAIN_GATE_BYPASS /
    # REIFY_PSI_GATE_DISABLE / REIFY_JOBSERVER_PRESSURE_DISABLE).
    local _jb_fifo=""
    if [ "$DF_VERIFY_ROLE" = "merge" ]; then
        _jb_fifo="${REIFY_JOBSERVER_MERGE_FIFO:-/tmp/reify-jobserver-merge}"
    elif [ "$DF_VERIFY_ROLE" != "offline" ]; then
        _jb_fifo="${REIFY_JOBSERVER_TASK_FIFO:-/tmp/reify-jobserver-task}"
    fi
    if [ "$DF_VERIFY_ROLE" = "offline" ]; then
        ENV_LINES+=("# CARGO_MAKEFLAGS left unset (offline role — off the merge jobserver, draws from neither task nor merge FIFO)")
    elif [ -p "$_jb_fifo" ]; then
        local _jb_live_rc=0
        if [ "${REIFY_JOBSERVER_SKIP_LIVENESS_PROBE:-}" != "1" ]; then
            # `||` (not a bare statement) so a non-zero STALE/UNKNOWN return
            # doesn't trip `set -e` and abort the whole script.
            _jobserver_owner_live "$_jb_fifo" || _jb_live_rc=$?
        fi
        if [ "$_jb_live_rc" -eq 1 ]; then
            echo "verify.sh: WARNING — stale jobserver FIFO $_jb_fifo (owner pid ${_JB_STALE_PID:-?} not alive / boot mismatch; balancer appears down) — falling back to plain cargo -j" >&2
            ENV_LINES+=("# CARGO_MAKEFLAGS left unset (stale FIFO $_jb_fifo — jobserver balancer down) — cargo uses its own job pool")
        else
            export CARGO_MAKEFLAGS="--jobserver-auth=fifo:$_jb_fifo"
            ENV_LINES+=("export CARGO_MAKEFLAGS=--jobserver-auth=fifo:$_jb_fifo")
        fi
    else
        ENV_LINES+=("# CARGO_MAKEFLAGS left unset (no $_jb_fifo FIFO) — cargo uses its own job pool")
    fi

    # OCCT shared-library search path (mirrors .cargo/run-with-occt.sh).
    local snap_lib="/snap/freecad/current/usr/lib"
    if [ -d "$snap_lib" ]; then
        export LD_LIBRARY_PATH="$snap_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
    local deps_lib="/opt/reify-deps/lib"
    if [ -d "$deps_lib" ] && ls "$deps_lib"/libTKernel.so* >/dev/null 2>&1; then
        export LD_LIBRARY_PATH="$deps_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
    ENV_LINES+=("export LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-}")
}
apply_env

# ---------------------------------------------------------------------------
# Scope decision: RUN_RUST / RUN_GUI / RUN_OCCT_GATE
# ---------------------------------------------------------------------------
RUN_RUST=0
RUN_GUI=0
# RUN_OCCT_GATE: diagnostic-only after task 4451 folded OCCT into the nextest pool.
# Still computed (gate=1 when OCCT-touching files change) and printed in the
# --print-plan header for observability; it no longer gates any test emission.
RUN_OCCT_GATE=0
CHANGED_FILES_RAW=""   # post-.task/ filtered file list; set by decide_scope for branch/staged

# is_occt_crate <crate-name> — true iff the crate is in the declared OCCT set.
_OCCT_DECLARED="$(occt_declared_set)"
is_occt_crate() {
    grep -qxF "$1" <<<"$_OCCT_DECLARED"
}

decide_scope() {
    if [ "$SCOPE" = "all" ]; then
        RUN_RUST=1; RUN_GUI=1; RUN_OCCT_GATE=1
        return
    fi

    # Classify the changed files for staged/branch scope, ignoring the agent
    # scratch dir (.task/). Source depends on scope:
    #   staged: git diff --cached (added/copied/modified/renamed index entries)
    #   branch: git diff "$_MERGE_BASE" (working tree vs merge-base(main,HEAD);
    #           tracked changes only — committed, staged, unstaged tracked
    #           modifications; untracked new files are not included)
    # Map each path to its impact:
    #   rust+gui+gate   workspace-global or OCCT-touching crate change
    #   rust+gui        a non-OCCT Rust crate / Tauri crate change (Rust ⊇ GUI)
    #   gui             frontend-only TS change (Rust ⊥ GUI)
    #   ignore          docs / markdown / yaml config
    #   conservative    anything unrecognised -> treat as rust+gui+gate
    local rust=0 gui=0 gate=0 f crate
    # Determine the changed-file list up front. For branch scope, check git diff's
    # exit status explicitly: if it fails after merge-base resolution (e.g. corrupt
    # object), fail WIDE rather than silently classifying nothing (contract C5).
    # The staged path keeps || true to absorb grep's harmless "no matches" exit-1.
    local _files="" _diff_out=""
    if [ "$SCOPE" = "branch" ]; then
        if ! _diff_out="$(git -C "$REPO_ROOT" diff --name-only --diff-filter=ACMRD "$_MERGE_BASE")"; then
            echo "verify.sh: WARNING — --scope branch git diff failed — failing WIDE to --scope all (contract C5)" >&2
            RUN_RUST=1; RUN_GUI=1; RUN_OCCT_GATE=1
            return
        fi
        _files="$(grep -v '^\.task/' <<< "$_diff_out" || true)"
    else
        _files="$(git -C "$REPO_ROOT" diff --cached --name-only --diff-filter=ACMRD | grep -v '^\.task/' || true)"
    fi
    while IFS= read -r f; do
        [ -z "$f" ] && continue
        case "$f" in
            crates/*)
                rust=1; gui=1
                crate="${f#crates/}"; crate="${crate%%/*}"
                if is_occt_crate "$crate"; then gate=1; fi
                ;;
            gui/src-tauri/*)
                # The Tauri Rust crate (reify-gui) is OCCT-clean by default features.
                rust=1; gui=1
                ;;
            Cargo.toml|Cargo.lock|.cargo/*)
                # Workspace-global: can affect any crate, including OCCT ones.
                rust=1; gui=1; gate=1
                ;;
            tree-sitter-reify/*)
                # Grammar drives the generated parser consumed by reify-eval (OCCT).
                rust=1; gui=1; gate=1
                ;;
            gui/*)
                # Any other GUI path (frontend src, sidecar, configs) — GUI only.
                gui=1
                ;;
            docs/*|*.md|*.yaml|*.yml)
                : # no heavy checks
                ;;
            *)
                # Unrecognised path: be conservative.
                rust=1; gui=1; gate=1
                ;;
        esac
    done <<< "$_files"

    # Capture for Phase-2 narrowing (after .task/ filter). scope=all returns early
    # above, leaving CHANGED_FILES_RAW="" (never narrowing-eligible).
    CHANGED_FILES_RAW="$_files"

    RUN_RUST=$rust
    # Any Rust change implies the (fast) GUI checks too.
    RUN_GUI=$(( rust | gui ))
    RUN_OCCT_GATE=$gate
}
decide_scope

# ---------------------------------------------------------------------------
# Selective infra test injection (task 4523).
#
# After decide_scope, read verify-pipeline-infra-tests.txt to derive
# SELECTED_INFRA_GLOBS: the set of infra-test globs whose artifact was changed
# on this branch/staged diff.  Empty under scope=all (CHANGED_FILES_RAW="").
#
# Design notes (see task 4523 decisions):
#   • Map is read inline (NOT via a sourced lib) so the throughput/gui_feature
#     auto-discovery greps don't flag it.  _VP_INFRA_MAP uses a variable
#     assignment; no 'source' directive for this map — fixture-check greps skip it.
#   • [ -f ] guard degrades gracefully in fixtures that omit the map.
#   • GLOB (not explicit names) so future test_verify_*.sh guards are
#     auto-covered without a map edit.
# ---------------------------------------------------------------------------
SELECTED_INFRA_GLOBS=""

select_infra_tests() {
    local _VP_INFRA_MAP="$SCRIPT_DIR/verify-pipeline-infra-tests.txt"
    # Graceful degradation: absent map or empty changed-file list -> empty.
    [ -f "$_VP_INFRA_MAP" ] || return 0
    [ -n "$CHANGED_FILES_RAW" ] || return 0
    local _artifact _glob _f _line
    while IFS= read -r _line; do
        # Each row: <artifact-path>  <infra-test-glob>
        read -r _artifact _glob <<< "$_line"
        [ -n "$_artifact" ] || continue
        [ -n "$_glob"     ] || continue
        while IFS= read -r _f; do
            [ -z "$_f" ] && continue
            if [ "$_f" = "$_artifact" ]; then
                # Append glob to selection if not already present (whole-token
                # dedup via space sentinels — prevents false dedup when one
                # glob is a substring of another, e.g. a specific path vs a
                # broader wildcard pattern).
                case " $SELECTED_INFRA_GLOBS " in
                    *" $_glob "*) : ;;
                    *) SELECTED_INFRA_GLOBS="${SELECTED_INFRA_GLOBS:+$SELECTED_INFRA_GLOBS }$_glob" ;;
                esac
                break
            fi
        done <<< "$CHANGED_FILES_RAW"
    done < <(grep -v '^\s*#' "$_VP_INFRA_MAP" | grep -v '^\s*$')
}
select_infra_tests

# ---------------------------------------------------------------------------
# Phase-2 narrowing: map changed files → affected crate set → -p flag strings.
#
# Eligible when: (scope=branch OR (scope=staged AND --narrow)) AND RUN_RUST=1.
# scope=all is structurally unreachable for narrowing (C1 — returns early in
# decide_scope, leaving CHANGED_FILES_RAW="", and the condition is never true).
# --narrow is a no-op for scope=branch (already narrowing) and scope=all
# (condition never true).
#
# REIFY_AFFECTED_CRATES_OVERRIDE — testability/operator knob (whitespace/newline-
# separated crate names). When set AND narrowing is eligible, used verbatim in
# place of calling affected_crates(). This mirrors the REIFY_PSI_GATE_PROC_PATH
# knob idiom and allows hermetic --print-plan assertions in the workspace-less
# fixture (where cargo metadata fails and affected_crates() always returns ALL).
# ---------------------------------------------------------------------------
AFFECTED=""
NARROW_ACTIVE=0
AFFECTED_ALL_FLAGS=""

_narrowing_eligible=0
if [ "$SCOPE" = "branch" ] && [ "$RUN_RUST" -eq 1 ]; then
    _narrowing_eligible=1
elif [ "$SCOPE" = "staged" ] && [ "$NARROW" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
    _narrowing_eligible=1
fi

if [ "$_narrowing_eligible" -eq 1 ]; then
    if [ -n "${REIFY_AFFECTED_CRATES_OVERRIDE:-}" ]; then
        # Operator/testability override: use verbatim crate list.
        AFFECTED="${REIFY_AFFECTED_CRATES_OVERRIDE}"
    elif [ -n "$CHANGED_FILES_RAW" ]; then
        # Real run: compute reverse-closure from the captured changed-file list.
        _af_args=()
        while IFS= read -r _af_f; do
            [ -n "$_af_f" ] && _af_args+=("$_af_f")
        done <<< "$CHANGED_FILES_RAW"
        if [ "${#_af_args[@]}" -gt 0 ]; then
            AFFECTED="$(affected_crates "${_af_args[@]}")"
        fi
    fi
    # NARROW_ACTIVE iff AFFECTED is non-empty and is NOT the sentinel "ALL".
    if [ -n "$AFFECTED" ] && [ "$AFFECTED" != "ALL" ]; then
        NARROW_ACTIVE=1
    fi
fi

if [ "$NARROW_ACTIVE" -eq 1 ]; then
    # Build the affected-crate -p flag string. Task 4451: no gated/ungated split;
    # all affected crates (including OCCT ones) go through the single nextest pass,
    # with the occt test-group (max-threads=24, env-driven) bounding their concurrency.
    # Word-split $AFFECTED (safe: Rust crate names never contain spaces).
    # shellcheck disable=SC2086
    for _nc in $AFFECTED; do
        [ -z "$_nc" ] && continue
        AFFECTED_ALL_FLAGS+=" -p $_nc"
    done
    AFFECTED_ALL_FLAGS="${AFFECTED_ALL_FLAGS# }"
    # Guard: a whitespace-only REIFY_AFFECTED_CRATES_OVERRIDE passes the non-empty check
    # above but word-splits to nothing, leaving all flag vars empty. Empty AFFECTED_ALL_FLAGS
    # with NARROW_ACTIVE=1 would cause narrowed cargo check/clippy to run with no -p selector
    # and narrowed test passes to emit zero commands (silent coverage gap). Fall back to
    # full-workspace to preserve the fail-wide invariant for a malformed knob value.
    if [ -z "$AFFECTED_ALL_FLAGS" ]; then
        NARROW_ACTIVE=0
    fi
fi

# ---------------------------------------------------------------------------
# Plan construction (built ONCE; print vs execute branches only at the leaves)
# ---------------------------------------------------------------------------
PLAN=()
add() { PLAN+=("$1"); }

# Release-sensitive crate flags: ALL release-sensitive crates in one nextest -p set.
# Task 4451: the gated/ungated split is gone; the nextest occt group (max-threads=24,
# env-driven) bounds intra-run concurrency for OCCT-touching release-sensitive crates (reify-eval).
# reify-kernel-occt, reify-cli, reify-config have zero release-sensitive tests and
# correctly drop out of the release pass; the debug full-workspace pass covers them.
_RELEASE_DECLARED="$(release_declared_set)"
_RELEASE_ALL_FLAGS=""
while IFS= read -r _rc; do
    [ -z "$_rc" ] && continue
    _RELEASE_ALL_FLAGS+=" -p $_rc"
done <<<"$_RELEASE_DECLARED"
_RELEASE_ALL_FLAGS="${_RELEASE_ALL_FLAGS# }"

# Test runner: prefer cargo-nextest (one global pool over ~hundreds of test
# binaries, OCCT concurrency bounded by the occt test-group) with a graceful
# fallback to plain `cargo test -- --test-threads=1` when nextest is not installed.
#
# Task 4971/esc-4959-57: `cargo nextest --version` returning non-zero is
# AMBIGUOUS — it fires both when nextest is genuinely uninstalled AND on a
# transient fork/exec failure under host pressure. Disambiguate via
# `command -v cargo-nextest` (a binary-presence check, independent of runtime
# pressure): genuine absence keeps the graceful cargo-test fallback exactly as
# before; a PRESENT-but-failing binary is instead treated as transient and
# retried up to 3x (bounded — a fixed retry count per spec, not a poll-until
# loop, so load_tolerant_attempts scaling does not apply) before this script
# hard-fails loudly rather than silently emitting a different (`-E`-less) plan.
#
# Scope note re: --print-plan hermeticity — this probe (including the retry
# loop and its sleeps) runs unconditionally in BOTH execute and --print-plan
# modes; it is NOT covered by the "pure, hermetic oracle (no subprocess, no
# temp file)" guarantee documented below at the nextest CONFIG FILE step
# (search "hermetic oracle" in this file) — that guarantee is scoped to the
# config-file generation only. The plan's `nextest=` header must reflect
# genuine availability, so --print-plan cannot skip this probe without
# risking a misleading plan. Worst case (cargo-nextest present but every
# probe failing) this forks cargo up to 4x and sleeps up to
# 2*REIFY_NEXTEST_PROBE_RETRY_SLEEP seconds before hard-failing; automation
# invoking --print-plan repeatedly should set REIFY_NEXTEST_PROBE_RETRY_SLEEP=0
# to avoid that cost, as tests/infra/test_verify_nextest_probe.sh does.
# Task 4971 review: single named constant for the retry budget so the loop
# bound and the sleep guard below (and the diagnostic they feed) can't
# silently diverge if this is ever edited.
_NEXTEST_PROBE_MAX_RETRIES=3
NEXTEST=0
if cargo nextest --version >/dev/null 2>&1; then
    NEXTEST=1
elif command -v cargo-nextest >/dev/null 2>&1; then
    # Binary present but the probe failed — retry, capturing the last rc/stderr
    # for a hard-fail diagnostic if every attempt is exhausted.
    # REIFY_NEXTEST_PROBE_RETRY_SLEEP is an env-overridable testability knob
    # (short default; tests set it to 0) — never a host-baked wall-clock
    # constant baked into an assertion.
    _NEXTEST_PROBE_RC=0
    _NEXTEST_PROBE_STDERR=""
    _NEXTEST_PROBE_ATTEMPTS=0
    while [ "$NEXTEST" -eq 0 ] && [ "$_NEXTEST_PROBE_ATTEMPTS" -lt "$_NEXTEST_PROBE_MAX_RETRIES" ]; do
        _NEXTEST_PROBE_ATTEMPTS=$((_NEXTEST_PROBE_ATTEMPTS + 1))
        _NEXTEST_PROBE_RC=0
        _NEXTEST_PROBE_STDERR="$(cargo nextest --version 2>&1 >/dev/null)" && NEXTEST=1 || _NEXTEST_PROBE_RC=$?
        # Sleep only before a subsequent retry attempt — not after the final
        # (3rd) one, which either just succeeded (no sleep needed) or falls
        # straight into the hard-fail below (a sleep there would only burn
        # wall-clock on the way to exit 1, with no probe left to benefit).
        if [ "$NEXTEST" -eq 0 ] && [ "$_NEXTEST_PROBE_ATTEMPTS" -lt "$_NEXTEST_PROBE_MAX_RETRIES" ]; then
            sleep "${REIFY_NEXTEST_PROBE_RETRY_SLEEP:-2}"
        fi
    done
    if [ "$NEXTEST" -eq 0 ]; then
        # Every retry exhausted and cargo-nextest is genuinely on PATH — a
        # loud, attributable hard failure beats silently emitting a
        # different (`-E`-less) plan (task 4971/esc-4959-57). Non-zero,
        # non-EX_TEMPFAIL exit: the 3 in-process retries already covered the
        # transient window, so an orchestrator retry (exit 75) would be
        # redundant and could still let an inconsistent plan slip through.
        echo "verify.sh: ERROR — cargo-nextest is present on PATH but the availability probe (\`cargo nextest --version\`) failed persistently across ${_NEXTEST_PROBE_ATTEMPTS}/${_NEXTEST_PROBE_MAX_RETRIES} retries ($(( _NEXTEST_PROBE_ATTEMPTS + 1 )) probes total, including the initial attempt; last retry rc=${_NEXTEST_PROBE_RC}) — refusing to silently fall back to the cargo-test plan (no -E support) while cargo-nextest is installed. Last probe stderr: ${_NEXTEST_PROBE_STDERR}" >&2
        exit 1
    fi
fi
# else: cargo-nextest genuinely absent from PATH — leave NEXTEST=0 (graceful
# cargo-test fallback, unchanged).

# wrap_subshell <dir> <minutes> <inner> — "(cd DIR && timeout … INNER)", using
# `bash -c '…'` only when INNER is a compound (&&) so the timeout governs it.
wrap_subshell() {
    local dir="$1" mins="$2" inner="$3"
    case "$inner" in
        *"&&"*)
            printf '(cd %s && timeout --kill-after=60 %sm bash -c '\''%s'\'')' "$dir" "$mins" "$inner" ;;
        *)
            printf '(cd %s && timeout --kill-after=60 %sm %s)' "$dir" "$mins" "$inner" ;;
    esac
}

# Memoized temp nextest config path (populated on first NEXTEST=1 execute-mode pass in
# emit_nextest_pass).  scripts/gen-nextest-config.sh writes a full copy of
# .config/nextest.toml with the occt literal rewritten to the REIFY_OCCT_NEXTEST_MAX_THREADS
# value (default 24).  nextest --config overrides CARGO config only (NO-OP for test-groups
# on 0.9.136); --config-file is required to actually override the occt group max-threads.
# In --print-plan mode the variable stays empty (no subprocess, no temp file — print mode
# is a hermetic, side-effect-free oracle; execute mode generates the real file).
# (This guarantee covers config-file generation only — the earlier nextest
# availability probe/retry loop, above the "Test runner:" comment near
# NEXTEST=0, is a deliberate exception: see its "Scope note re: --print-plan
# hermeticity".)
_NEXTEST_CONFIG_FILE=""

_verify_cleanup() {
    reaper_teardown || true
    if [ -n "$_NEXTEST_CONFIG_FILE" ] && [ -f "$_NEXTEST_CONFIG_FILE" ]; then
        rm -f "$_NEXTEST_CONFIG_FILE"
    fi
}
trap '_verify_cleanup' EXIT
trap '_verify_cleanup; exit 130' INT
trap '_verify_cleanup; exit 143' TERM
trap '_verify_cleanup; exit 129' HUP

# emit_nextest_pass <selector> <rel> <outer_timeout>
# Emit a single nextest (or cargo-test fallback) pass.
# selector: "--workspace" (full-workspace) or "-p crate1 -p crate2 ..." (narrowed/release)
# rel: "" (debug) or " --release"
# outer_timeout: e.g. "60m"
# Task 4451: replaces emit_gated_ungated; the flock-gated OCCT pass is dropped.
# Task 4503/γ: env-driven occt cap via REIFY_OCCT_NEXTEST_MAX_THREADS (default 24).
# Task 4862 revert: build+execution are one unbroken slot-held block; the 4839
# mode="compile" --no-run split is removed.
# scripts/gen-nextest-config.sh generates a temp nextest config (memoized in
# _NEXTEST_CONFIG_FILE) passed as --config-file; nextest --config overrides CARGO
# config only (NO-OP for test-groups on 0.9.136) so --config-file is required.
# In --print-plan mode a static placeholder path is emitted instead of a real temp
# path so --print-plan remains a pure, hermetic oracle (no subprocess, no temp file).
# (As above: this is scoped to config-file generation, not the earlier NEXTEST
# availability probe/retry loop, which is exempt — see its "Scope note re:
# --print-plan hermeticity".)
emit_nextest_pass() {
    local selector="$1" rel="$2" outer_timeout="$3"
    local cmd
    if [ "$NEXTEST" -eq 1 ]; then
        local _cfg_path
        if [ "$PRINT_PLAN" -eq 1 ]; then
            # Print mode: emit a representative placeholder so --print-plan is a
            # pure, hermetic oracle — no subprocess, no temp file created.
            # The placeholder preserves the 'reify-nextest-occt' prefix so plan-shape
            # assertions (tests/infra/test_occt_gated_scope.sh Test 9) can still
            # match the pattern without requiring a real file on disk.
            # This path is intentionally NOT re-runnable; only execute mode produces
            # a real config file (memoized in _NEXTEST_CONFIG_FILE).
            _cfg_path="${TMPDIR:-/tmp}/reify-nextest-occt.<print-plan-placeholder>"
        else
            # Execute mode: generate the nextest config once per process (memoized).
            # Produces a full copy of .config/nextest.toml with the occt cap rewritten
            # to the resolved env value; removed by _verify_cleanup on EXIT.
            if [ -z "$_NEXTEST_CONFIG_FILE" ]; then
                _NEXTEST_CONFIG_FILE="$("$SCRIPT_DIR/gen-nextest-config.sh")"
            fi
            _cfg_path="$_NEXTEST_CONFIG_FILE"
        fi
        cmd="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo nextest run ${selector}${rel}${_GATE_HEAVY_EXCLUDE}${_OFFLINE_HEAVY_SELECT} --config-file ${_cfg_path}"
    else
        # Fallback: single-threaded (OCCT serialization via the nextest occt group is
        # unavailable without nextest; use --test-threads=1 as the whole-workspace guard).
        cmd="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo test ${selector}${rel} -- --test-threads=1"
    fi
    # FD 9 is the held semaphore slot; close it for each gated child so daemon
    # processes (sccache/rustc) cannot inadvertently inherit the lock fd and
    # wedge the slot after the test pass exits (2026-04-20 wedge class).
    # Harmless no-op on the merge-exempt path.
    add "$cmd 9<&-"
}

add_test_passes() {
    # PSI gate: must pass before any cargo test work starts.
    # In execute mode: eval runs this as a subprocess that inherits DF_VERIFY_ROLE
    # and REIFY_PSI_GATE_*; exit 75 (EX_TEMPFAIL) propagates → orchestrator retries.
    # In --print-plan mode: printed faithfully as a normal plan line.
    add "./scripts/verify.sh psi-gate"

    # Compile-phase PSI admission gate (task 4853, repositioned by task 4862):
    # block-entry LOAD gate for the unified build+test block — sits after psi-gate
    # and BEFORE @@SEMAPHORE_ACQUIRE@@.  Reify test binaries statically link
    # OCCT/OpenVDB/gmsh/manifold (~1-3 GiB RSS/link); this gate provides an
    # admit-on-timeout PSI/RSS backstop before the slot is acquired, so the
    # whole build+test block is only entered under acceptable host load.
    # The gate also carries the memory-PSI dimension (cpu-admit.sh
    # _ca_mem_full_threshold default 10%) — the binding memory constraint (task 4862).
    # Emitted ROLE-INVARIANTLY; DF_VERIFY_ROLE=merge bypasses at RUNTIME inside
    # compile_gate()->cpu_admit so the merge gate NEVER waits.  Soft stagger:
    # admit-on-timeout, NEVER exit 75.  Bare string so W7 stays green.
    add "./scripts/verify.sh compile-gate"

    # Acquire the test-run semaphore slot after psi-gate and compile-gate.
    # Task 4862 revert: the slot now wraps the ENTIRE build+execution block
    # (compile + test execution run as one unbroken held block per profile).
    # Rationale: MEMORY is the binding constraint (memfull avg10 ~28%, ~161 GiB
    # swap on a 125 GiB host); the held slot's whole-block serialization is the
    # only implicit bound on concurrent RSS-heavy link waves.  4839's pipelining
    # only pays under memory headroom, which is gone.  The slot-cling-during-build
    # that 4839 band-aided is now fixed properly by the clock-stop seam (task 4838):
    # slot-wait is a graceful continuous in-process hold, not exit-75.
    # The executor calls test_semaphore_acquire here; the printer emits a comment.
    add "@@SEMAPHORE_ACQUIRE@@"

    # Emit one combined build+execution nextest pass per profile (slot held).
    # Outer timeout: single unified budget re-derived from η/4521's authoritative
    # real-load floor (task 4520/ζ′).
    # Floor: 798.9 s (worst-observed cold real-load, genuinely cold-cache, quiet box
    # with warm host sccache — see docs/prds/jobserver-merge-priority-balancer
    # .acceptance-report.md §"ζ′/4520 budget floor (authoritative)").
    # Derivation: ceil(798.9 × 4.5 production-weighted margin) = ceil(3595.05 s) =
    # 3596 s → rounded up to clean minute-granularity = 60m (3600 s).
    # Bound 3600 s > floor 798.9 s by construction.
    # NOTE: outer timeouts asserted in tests/infra/test_occt_flock_gate.sh
    # (Test 17 — debug pass, Test 17b — release pass) — keep in sync.
    local _profile _rel
    local _outer_timeout="${_VERIFY_TEST_TIMEOUT}"  # identical for all profiles
    for _profile in "${PROFILES[@]}"; do
        if [ "$_profile" = "release" ]; then
            _rel=" --release"
            # Release pass: ALL release-sensitive crates in one nextest pass (task 4451).
            # The nextest occt group (max-threads=24, env-driven) bounds concurrency for
            # OCCT-touching release-sensitive crates (e.g. reify-eval). Only crates with
            # debug_assertions/overflow-checks-dependent tests need to re-run in release;
            # the DEBUG full-workspace pass covers every other crate.
            # NARROW_ACTIVE is intentionally not applied to the release pass. It is scoped
            # by release-sensitivity (task/4390), not the affected-crate set (task/4060).
            # Over-running the full release-sensitive set on a rare --profile both
            # --scope branch is safe (fail-wide), and avoids entangling two orthogonal
            # scoping axes — do not "fix" this by narrowing the release pass.
            # offline (task 4913/A2): the positive -E "(<heavy>)" filter (applied via
            # _OFFLINE_HEAVY_SELECT inside emit_nextest_pass) is the SOLE membership
            # determinant for the offline lane — use --workspace instead of the
            # release-sensitive -p set so offline's heavy coverage never silently
            # narrows if a heavy crate is ever dropped from release-sensitive-crates.txt.
            if [ "$DF_VERIFY_ROLE" = "offline" ]; then
                emit_nextest_pass "--workspace" "$_rel" "$_outer_timeout"
            else
                emit_nextest_pass "$_RELEASE_ALL_FLAGS" "$_rel" "$_outer_timeout"
            fi
        else
            _rel=""
            # Debug pass.
            if [ "$NARROW_ACTIVE" -eq 1 ]; then
                emit_nextest_pass "$AFFECTED_ALL_FLAGS" "$_rel" "$_outer_timeout"
            else
                emit_nextest_pass "--workspace" "$_rel" "$_outer_timeout"
            fi
        fi
    done

    # Release the semaphore slot after all passes complete.
    # The executor calls test_semaphore_release; the printer emits a comment.
    # The slot is also freed automatically on any verify.sh exit (FD 9 closes),
    # so the failure path needs no explicit release sentinel.
    add "@@SEMAPHORE_RELEASE@@"
}

build_plan() {
    # tests/infra classification-manifest drift guard (task 5252): fail fast —
    # naming the offending file — when a tests/infra/test_*.sh exists with no
    # run-all-classification.manifest row (or a manifest row has no file). Cheap
    # (pure bash + filesystem, no cargo), so it is the FIRST plan entry, before
    # check-manifold-deps.sh and every compile/test pole. RUN_RUST=1 fires it
    # whenever tests/infra/*.sh changes (decide_scope's `*)` catch-all -> rust=1)
    # and always at the merge/scope=all tier, while keeping docs-only /
    # gui-src-only plans (RUN_RUST=0) at zero command leaves.
    if [ "$RUN_RUST" -eq 1 ]; then
        add "./scripts/check-infra-classification-manifest.sh"
    fi

    # manifold prebuilt guard: fail fast (with a clear "run the deps script"
    # message) if the prebuilt manifold libs that .cargo/config.toml's
    # [target.*.manifold] override links are missing or version-drifted —
    # before any multi-minute compile turns that into a cryptic linker error.
    if [ "$RUN_RUST" -eq 1 ]; then
        add "./scripts/check-manifold-deps.sh"
    fi

    # tree-sitter parser regeneration is a Rust-build prerequisite.
    if [ "$RUN_RUST" -eq 1 ]; then
        add "./scripts/tree-sitter-generate.sh"
    fi

    # Compile-phase PSI admission gate (task 4618): soft backpressure backstop
    # for the jobserver's implicit-token leak (FIFO pool tokens + 1 implicit
    # token per concurrent cargo) and non-cargo load.  Emitted only when
    # cargo check/clippy will actually run (lint or typecheck side).
    #
    # Design note — two compile-gate lines on action=all (tasks 4853/4862):
    #   1. This build_plan() line (HERE): fires immediately before clippy/check,
    #      as the admit-on-timeout backstop for the lint/typecheck compile wave.
    #   2. add_test_passes() line: fires after psi-gate and BEFORE
    #      @@SEMAPHORE_ACQUIRE@@, as the block-entry load gate for the unified
    #      build+test block (task 4862 revert: compile+execution back in one slot).
    #      The action=test path carries ONLY the add_test_passes() line
    #      (this build_plan() line is lint-only).
    # On action=all BOTH lines fire deliberately: the early one staggers the
    # clippy/check compile wave; the late one re-checks PSI/memory before
    # acquiring the slot (PSI can change materially across the long clippy/check
    # phase).  This is an intentional additional check, NOT an accidental
    # double-gate — the two gates address different compile waves separated by
    # significant elapsed time.
    # DF_VERIFY_ROLE=merge bypass is at RUNTIME inside compile_gate() (CAVEAT 1);
    # the plan line is still emitted in merge plans so the plan shape is
    # role-invariant (mirrors the psi-gate idiom).
    if [ "$RUN_RUST" -eq 1 ] && { [ "$DO_LINT" -eq 1 ] || [ "$DO_TYPECHECK" -eq 1 ]; }; then
        add "./scripts/verify.sh compile-gate"
    fi

    # typecheck (cargo check) only when NOT also linting — clippy --all-targets
    # is a strict superset of `cargo check`, so running both would be redundant.
    if [ "$DO_TYPECHECK" -eq 1 ] && [ "$DO_LINT" -eq 0 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 ${_VERIFY_CHECK_TIMEOUT} ${CARGO_PRIO}cargo check ${AFFECTED_ALL_FLAGS} --tests"
        else
            add "timeout --kill-after=60 ${_VERIFY_CHECK_TIMEOUT} ${CARGO_PRIO}cargo check --workspace --tests"
        fi
    fi

    # GUI ecosystem (npm). Rust changes imply these too; they are fast. Only
    # meaningful when there is a GUI check to run — the GUI has a test side
    # (npm test) and a typecheck (npm run typecheck) but no `cargo check`
    # analogue, so a pure typecheck action skips it entirely (verify.sh's own
    # `typecheck` action is cargo-check-only; the GUI ecosystem has no equivalent).
    #
    # The GUI typecheck (tsc --noEmit) now runs whenever this block runs — on the
    # TEST side as well as the lint side — not lint-only as before. Rationale: the
    # orchestrator's inner TDD loop runs `verify.sh test --scope branch` (npm test
    # = vitest), which never type-checks; a type-only break that renders fine at
    # runtime (e.g. a solid-js <Show> function-child rejected by the non-keyed
    # overload) therefore stayed invisible through development and only surfaced at
    # lint/merge time — by which point, since any Rust change forces RUN_GUI=1, it
    # blocks every task's branch verify on an inherited error. Putting tsc on the
    # test side catches this class in the cheap inner loop. The block is built ONCE
    # (not per-profile), so a single `&& npm run typecheck` means action=all runs it
    # exactly once — no double-run.
    #
    # FAIL-FAST: emitted BEFORE add_test_passes (the expensive pole) so a broken
    # gui tsc fails the plan in ~minutes, not after 85 min of Rust build+test.
    # (task #4448 / incident fix for #4446)
    #
    # BOUNDED node||cargo OVERLAP (task #4448, Leo's directive): when a rust
    # foreground cheap gate (clippy/gui-feature-check) is also emitted for this
    # action (DO_LINT=1 && RUN_RUST=1), background the node lane so it runs
    # concurrently with those gates. Node npm runs off the rustc jobserver →
    # zero jobserver contention. bg PID variable persists across plan entries
    # because the executor evals every entry in this shell (same-shell eval).
    # For action=test there is no rust foreground gate; the node lane stays plain
    # (pure fail-fast reorder, no overlap). For action=typecheck the node lane is
    # empty (gui block gated on test||lint) → unchanged.
    local _gui_cmd="" _sidecar_cmd="" _ts_cmd="" _node_lane=""
    if [ "$RUN_GUI" -eq 1 ] && { [ "$DO_TEST" -eq 1 ] || [ "$DO_LINT" -eq 1 ]; }; then
        # typecheck always (whenever the block runs, test OR lint); npm test only
        # on the test side.
        local gui_inner="npm ci && npm run typecheck"
        [ "$DO_TEST" -eq 1 ] && gui_inner+=" && npm test"
        _gui_cmd="if test -d gui; then $(wrap_subshell gui 15 "$gui_inner"); fi"

        # sidecar has no vitest side; both typecheck passes run whenever the block does.
        local sidecar_inner="npm ci && npm run typecheck && npm run typecheck:test"
        _sidecar_cmd="if test -f gui/sidecar/package-lock.json; then $(wrap_subshell gui/sidecar 10 "$sidecar_inner"); fi"

        _ts_cmd="if test -f tree-sitter-reify/package-lock.json; then $(wrap_subshell tree-sitter-reify 10 "npm ci"); fi"
        _node_lane="${_gui_cmd} && ${_sidecar_cmd} && ${_ts_cmd}"
    fi

    # Overlap path: background the node lane BEFORE the foreground rust cheap
    # gates (clippy + gui-feature-check) so they run concurrently. The bg PID
    # variable persists into the join entry below (same executor shell).
    #
    # Cleanup trap: registered in the same eval so it fires on any EXIT (success
    # or failure). If a foreground rust gate fails before the wait join, the
    # executor calls exit and the trap kills the still-running npm job instead of
    # orphaning it.
    #
    # The kill is wrapped in an `if ...; then :; fi` rather than a bare sequence.
    # On the happy path `wait` has already reaped the job before EXIT fires, so
    # the kill returns 1 (no such process). Under the script's `set -euo
    # pipefail`, a *bare* `kill ...; true` poisons the exit code: bash aborts the
    # trap body at the failing kill BEFORE reaching `true`, flipping a fully
    # passing run (rc=0 after "all checks passed") to rc=1 (regression from
    # commit 9b398f7a26; esc-3993-22, independently reproduced under bash 5.2 as
    # esc-4431-30). An `if` *condition* is exempt from set -e, so
    # `if kill ...; then :; fi` swallows the no-such-process failure without
    # aborting — and still reaps the job on the fail path (kill succeeds → `:`).
    # NOTE: "|| true" is intentionally avoided here — the npm ci hardening test
    # (test_npm_ci_hardening.sh Test 3) asserts that no plan line contains
    # "npm ci.*|| true", and the trap is on the same line as the npm ci call;
    # the `if`-guard achieves the same set -e safety without that token.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ] && [ -n "$_node_lane" ]; then
        add "{ ${_node_lane} ; } & _VERIFY_NODE_BG_PID=\$!; trap 'if kill \"\$_VERIFY_NODE_BG_PID\" 2>/dev/null; then :; fi; _verify_cleanup' EXIT"
    fi

    # lint: clippy over all targets, warnings-as-errors.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo clippy ${AFFECTED_ALL_FLAGS} --all-targets -- -D warnings"
        else
            add "timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo clippy --workspace --all-targets -- -D warnings"
        fi
    fi

    # gui-feature compile-check: type-check reify-gui's #[cfg(feature="gui")] code
    # (engine.rs, main.rs, tests/*) which is never reached by the workspace-wide
    # cargo check / clippy / nextest passes (all run without --features gui).
    #
    # Placed on the LINT side (DO_LINT=1 && RUN_RUST=1) because:
    #   - It is a compile-check, semantically adjacent to clippy.
    #   - LINT is the only action that fires on EVERY merge path (orchestrator
    #     lint_command, pre-merge-commit `all`, hooks/project-checks `all`).
    #   - Gating under RUN_RUST (not RUN_GUI) keeps frontend-only/docs-only
    #     commits fast — only Rust changes can break gui-gated Rust.
    #
    # ensure-gui-sidecar-placeholder.sh runs first because tauri_build::build()
    # (in gui/src-tauri/build.rs) validates bundle.externalBin and panics if
    # gui/src-tauri/sidecar/reify-sidecar-<triple> is absent from disk; the stub
    # satisfies the existence check without clobbering a real built sidecar.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        add "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo check -p reify-gui --features gui --tests; fi"
    fi

    # Overlap join: wait for the background node lane before infra checks / pole.
    # Maximises the concurrency window (join as late as possible while still
    # preceding the expensive pole and infra checks).
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ] && [ -n "$_node_lane" ]; then
        add 'wait "$_VERIFY_NODE_BG_PID"'
    fi

    # Plain path: node lane as sequential lines (no foreground rust gate, e.g. action=test).
    if [ -n "$_node_lane" ] && { [ "$DO_LINT" -eq 0 ] || [ "$RUN_RUST" -eq 0 ]; }; then
        add "$_gui_cmd"
        add "$_sidecar_cmd"
        add "$_ts_cmd"
    fi

    # Cheap static infra checks (opt-in). Test-side and lint-side, mirroring the
    # historical orchestrator split. Tied to RUN_RUST (the heavy gate) so a
    # frontend-only or docs-only staged commit stays fast.
    #
    # FAIL-FAST: emitted BEFORE add_test_passes (task #4448).
    if [ "$INCLUDE_INFRA" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$DO_TEST" -eq 1 ]; then
            add "if test -f tests/sync_comments_test.sh; then timeout --kill-after=60 10m bash tests/sync_comments_test.sh; else echo 'WARNING: sync_comments_test.sh not found, skipping'; fi"
        fi
        if [ "$DO_LINT" -eq 1 ]; then
            add "if test -f scripts/test_pm_standardization.sh; then timeout --kill-after=60 10m bash scripts/test_pm_standardization.sh; else echo 'WARNING: test_pm_standardization.sh not found, skipping'; fi"
            add "if test -f scripts/check_event_inventory.sh; then timeout --kill-after=60 5m bash scripts/check_event_inventory.sh; else echo 'WARNING: check_event_inventory.sh not found, skipping'; fi"
            add "if test -f scripts/check-nan-safe-ordering.sh; then timeout --kill-after=60 5m bash scripts/check-nan-safe-ordering.sh; else echo 'WARNING: check-nan-safe-ordering.sh not found, skipping'; fi"
        fi
    fi

    # Wholesale infra pool suite (task 5125): MERGE TIER ONLY. hooks/pre-merge-commit
    # runs `DF_VERIFY_ROLE=merge verify.sh all --profile both --scope all` WITHOUT
    # --include-infra, while EVERY per-task lane passes --include-infra — so gating
    # run_all.sh on INCLUDE_INFRA (as before) ran the full 103-test suite on every
    # task lane and NEVER at merge, starving the shared 16-slot pool (M-way
    # contention -> 30m timeout -> exit 124 -> BLOCKED). Gating on role instead
    # makes DF_VERIFY_ROLE=merge (stamped by both merge seams: hooks/pre-merge-commit
    # and the dark-factory merge-verify command) the single source of truth: the
    # full pool runs exactly once, at merge; per-task lanes get the cheap selective
    # subset below instead (exactly-one invariant, INV-5).
    #
    # TRADE-OFF, accepted deliberately (task 5125 review): this also moves the
    # reify-audit PTODO hard gate (CLAUDE.md's untracked/orphaned/bare-ignore
    # gate) from per-task feedback to merge-time-only feedback, since that gate
    # lives inside the run_all.sh pool (tests/infra/test_reify_audit_ptodo*.sh).
    # A change that touches only product source (no verify-pipeline artifact
    # from scripts/verify-pipeline-infra-tests.txt) and introduces an orphaned
    # TODO now passes its per-task verify and is only caught at the merge gate —
    # later feedback than before, but merge still blocks landing on main, so
    # this is a latency trade-off, not a coverage gap. It is the direct fix for
    # the M-way run_all pool contention above (INV-5); see this task's plan
    # design_decisions for the full rationale. A cheap per-task-only PTODO
    # precheck (skipping the other ~102 run_all tests) is a possible follow-up
    # if per-task PTODO latency proves costly in practice — not implemented
    # here to keep this task's fix scoped to the tiering mechanism itself.
    #
    # FAIL-FAST: emitted BEFORE add_test_passes (task #4448).
    #
    # RE-ENTRANCY GUARD (task 5125): suppress the wholesale run_all.sh line when
    # we are ALREADY executing inside an infra suite (REIFY_INFRA_SUITE_ACTIVE
    # set). Without this, an infra test that itself drives a real
    # DF_VERIFY_ROLE=merge verify — tests/infra/test_verify_semaphore_e2e.sh
    # Section B, which proves the merge-role semaphore bypass — would re-satisfy
    # this role==merge gate and re-emit run_all.sh, recursing unboundedly
    # (run_all -> semaphore-e2e -> merge-role verify -> run_all -> ...) until the
    # 30m wall SIGKILLs it. The gate keys on the INHERITED env var
    # DF_VERIFY_ROLE, so the break is also an inherited env var. It is set
    # NARROWLY, at the single recursion source (that Section-B spawn), NOT
    # broadcast onto the run_all.sh plan line: a broadcast leaks into all ~103
    # pool tests, suppressing run_all in their captured plans and tripping the
    # ambient-isolation guard (test_run_all_ambient_isolation.sh, task 4961).
    # background (task 5210, main-tip integrity sweep) shares the merge tier
    # here too: same full-pool completeness, gated by the same re-entrancy
    # sentinel (test_verify_semaphore_e2e.sh Section H sets it on its own
    # nested background spawn, mirroring Section B's merge spawn).
    if { [ "$DF_VERIFY_ROLE" = "merge" ] || [ "$DF_VERIFY_ROLE" = "background" ]; } && [ "$RUN_RUST" -eq 1 ] && [ "$DO_TEST" -eq 1 ] && [ -z "${REIFY_INFRA_SUITE_ACTIVE:-}" ]; then
        # task #4624: pre-build reify-audit OUTSIDE the run_all.sh wall (30m).
        # By the time run_all.sh runs, target/release/{reify-audit,ptodo-baseline-gen}
        # are fresh so the in-wall freshness guard finds them fresh and skips the cold
        # build.  sccache (RUSTC_WRAPPER) makes this cheap when already cached.
        # Timeout is 10m (distinct from the run_all wall) so the plan-shape test can assert
        # the pre-step is not the walled run_all.sh line.
        #
        # ADMISSION CONTROLS: this pre-step runs OUTSIDE compile_gate()/psi_gate()/
        # @@SEMAPHORE_ACQUIRE@@ — build_plan() emits this whole block (the
        # pre-builds AND the run_all.sh call below) BEFORE add_test_passes() is
        # invoked (~:1618), so no role passes through an admission gate here; it is
        # a structural consequence of where the block sits in the plan, not a
        # per-role exemption. The lint-side compile-gate line emitted earlier in
        # build_plan() (~:1297) targets the clippy/check compile wave immediately
        # following it and does not re-check PSI this far downstream (PSI "can
        # change materially across the long clippy/check phase" — see the
        # add_test_passes() design note on the two compile-gate lines).
        #
        # task 5210: DF_VERIFY_ROLE=background also reaches this block now (the
        # merge-level-completeness guard just above matches background too), and
        # background is explicitly NON-exempt elsewhere: lib_test_semaphore.sh:91
        # and cpu-admit.sh:223 stay strict `= "merge"`, so the test-run
        # semaphore/PSI gates still hold background everywhere else in the plan.
        # Rationale (1) below is a merge-only fact — do NOT read it as covering
        # background too; a non-exempt role reaching an admission-gate-free block
        # is exactly the task×compile contention the CLAUDE.md admission-control
        # invariant exists to bound. What actually bounds it here for background:
        # CARGO_PRIO is the offline-style IDLE class (`nice -n 19 ionice -c3`, set
        # above via _idle_cargo_prio) rather than merge's near-normal `nice -n 5`,
        # so these cargo build lines yield to any concurrently scheduled task
        # compile instead of contending with it head-on. That is scheduler-level
        # mitigation, not admission control — there is no wait if the host is
        # saturated, only reduced impact once running. If that proves
        # insufficient in practice, the fix is to route background through
        # compile_gate()/psi_gate() for this block specifically, not to lean on
        # merge's exemption.
        #
        # Rationale for merge (unchanged by task 5210; (1) does not extend to
        # background — see above): (1) DF_VERIFY_ROLE=merge is exempt from all
        # gates anyway; (2) sccache makes this a no-op when warm; (3) this plan
        # line emits in the infra block — after all main Rust compile phases — so
        # it does not race with the compile-gate window that guards clippy/check;
        # (4) the CLAUDE.md admission-control invariant is for task×compile
        # contention during the main psi-gate/slot region, which this small
        # pre-build does not enter.
        # task 5139: dropped -q — it swallowed compiler diagnostics, so the
        # 06-27/28 failure cluster (4763/4744/4822/4873) and esc-5077-1
        # pre-build failures archived with no usable evidence. Dropping -q
        # alone is insufficient: cargo writes ALL of its output (progress
        # AND error/warning diagnostics) to stderr, never stdout, and DF
        # archives verify.sh's stdout stream only (same premise as the
        # run_all.sh fix below). 2>&1 routes cargo's diagnostics into the
        # captured stream.
        add "if test -f crates/reify-audit/Cargo.toml; then timeout --kill-after=60 10m ${CARGO_PRIO}cargo build --release -p reify-audit 2>&1; fi"
        # Positive assertion: if the Cargo.toml exists but the pre-build did not
        # produce the binary, abort loudly rather than silently degrading to SKIP.
        # Guards against the pre-step being removed or reordered without updating
        # the REIFY_AUDIT_NO_COLD_BUILD backstop below.  Only fires if the
        # pre-step is present (Cargo.toml guard matches) but produces no output.
        # task 5139 (amendment review, reviewer_comprehensive
        # robustness_error_handling): this guard's own ERROR(#4624) diagnostic
        # was stderr-only (>&2), so a fired guard reproduced the exact
        # "archived with no usable evidence" gap 5139 closes for the
        # cargo/run_all lines above/below. `fi 2>&1` merges the whole
        # if-statement's stderr into the already-captured stdout stream
        # (applied to the compound command, so it also covers the internal
        # `>&2` on the echo) without touching the `false` exit code.
        add "if test -f crates/reify-audit/Cargo.toml && [ ! -f target/release/reify-audit ]; then echo 'ERROR(#4624): reify-audit binary missing after pre-build step — PTODO gate will silently SKIP; restore the pre-step above or remove this check deliberately' >&2; false; fi 2>&1"
        # task #5133: pre-build reify-cli and stamp target/.reify-bin-sha with
        # build-time HEAD, mirroring the reify-audit pre-build immediately
        # above. The PRD gate tests inside run_all.sh (test_prd_gate_corpus.sh,
        # test_prd_gate_objective_inheritance.sh) auto-discover whatever
        # target/{release,debug}/reify happens to exist; in this shared
        # merge-verify warm lane that binary can be a LEFTOVER built by a
        # different, sibling merge candidate that happened to build reify-cli
        # earlier in the same lane. The sidecar records the exact tree the
        # binary was built from so those tests can prove it matches the
        # current candidate (not a cross-candidate leftover) and refuse a
        # verdict (clean SKIP) when it doesn't. It MUST be emitted before the
        # run_all.sh line below so the sidecar exists by the time the
        # auto-discovered gate tests run inside it; cargo's per-tree
        # fingerprint means the freshly built target/release/reify here
        # matches HEAD's reify-cli cone, evicting any sibling leftover. The
        # stamp is guarded on that bin existing so a failed/absent pre-build
        # never stamps a false HEAD onto a missing binary.
        # task 5139: dropped -q and merged stderr into stdout via 2>&1 (same
        # rationale as the reify-audit pre-step above).
        add "if test -f crates/reify-cli/Cargo.toml; then timeout --kill-after=60 10m ${CARGO_PRIO}cargo build --release -p reify-cli 2>&1; fi"
        add "if test -f target/release/reify; then git rev-parse HEAD > target/.reify-bin-sha 2>/dev/null || true; fi"
        # Arm the budget-safe backstop: REIFY_AUDIT_NO_COLD_BUILD=1 tells the
        # freshness guard to skip rather than cold-build if somehow the pre-step
        # above was bypassed or narrowed (defense-in-depth; maps to SKIP exit 0).
        # task #3810/esc-3810-4: bumped 20m -> 30m. The infra suite grew past
        # the 20m wall after the warm-lane CoW-pool tests landed (they auto-run
        # heavy cargo blocks when TMPDIR is XFS-reflink, i.e. on the merge worker),
        # tipping a suite already near its budget over the wall (exit 124). 30m
        # restores headroom for the full --scope all / merge gate.
        # REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (task 5125): host-exclusive tests
        # (declared in tests/infra/run-all-classification.manifest) stay on their
        # cold `--scope host-infra` lane instead of double-running here.
        # REIFY_RUN_ALL_CONTENT_SKIP=1 (task 5273, merge-gate-riders γ): arms the
        # merge-tier content-addressed per-member skip engine in run_all.sh — a
        # drift-guard pool member whose declared tracked-file closure
        # (run-all-skip-closures.manifest) is byte-identical (git tree compare)
        # to its last-executed-green main sha is not re-run every merge. The
        # engine is a two-key + state-path INERT no-op unless run_all.sh ALSO
        # observes the inbound role == merge (_RA_INBOUND_ROLE snapshotted at
        # run_all.sh:230 — NOT the normalized DF_VERIFY_ROLE, which is forced to
        # `task` there) AND a non-empty REIFY_RUN_ALL_SKIP_STATE path. That
        # durable state path is wired in dark-factory-orchestrator.yaml verify_env
        # by the sibling activation task (δ / 5276); until then this flag is a
        # silent no-op and ships PRODUCTION-INERT. Fail-open by construction:
        # unmapped members, closure deltas, own-file changes, the
        # MAX_MERGES/MAX_AGE_HOURS backstop, and a corrupt/absent state file all
        # force a full run (the last emits one loud line). There is ONE shared
        # run_all.sh plan line (the single add() below), emitted for BOTH the
        # merge and background roles by the combined role branch at ~:1499 — not
        # two separate lines — so the flag always rides it. The background role
        # is neutralized inside run_all.sh by its inbound-role gate
        # (_RA_INBOUND_ROLE != merge ⇒ never skips), a second backstop, rather
        # than by a separate plan line here. Contract: INV-5′,
        # docs/prds/run-all-pool-contention-tiering-fix.md.
        # NB: this line must NOT export REIFY_INFRA_SUITE_ACTIVE (the re-entrancy
        # sentinel). run_all.sh runs ~103 tests; a broad ambient export leaks
        # into every one and (a) suppresses run_all in the plan captured by the
        # plan-shape tests (test_run_all_tiering / test_verify_scope /
        # test_verify_failfast_order), and (b) trips test_run_all_ambient_isolation
        # (task 4961 / esc-4906-45 — the "orchestration var leaked as ambient
        # export" guard). The sentinel is set narrowly by the ONE recursion
        # source, test_verify_semaphore_e2e.sh Section B, so only that nested
        # merge-role verify sees it (task 5125).
        # task 5139: run_all's stderr (INFO/progress lines plus the task-5123
        # _ra_discovery_diag ERR-trap diagnostic) was lost entirely from the
        # archived attempt-N.test-*.log. 2>&1 routes it into the same stream
        # DF already captures; run_all emits its Summary/FAILED classifier
        # markers to stdout, so the DF ^FAILED\s contract is preserved.
        # task 5139 (amendment review, reviewer_comprehensive
        # robustness_error_handling): merging the streams raised a theoretical
        # concern that a stdout classifier line could be torn mid-write by
        # interleaved stderr, corrupting the ^FAILED\s anchor. No change
        # made: atomicity holds because each marker is a single write() call;
        # regression-guarded by tests/infra/test_run_all.sh Tests 7 and 8a
        # (source of truth for marker text/locations — not restated here).
        add "if test -f tests/infra/run_all.sh; then REIFY_AUDIT_NO_COLD_BUILD=1 REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 REIFY_RUN_ALL_CONTENT_SKIP=1 timeout --kill-after=60 30m bash tests/infra/run_all.sh 2>&1; fi"
    fi

    # Selective infra injection (task 4523): task-level path runs the infra
    # drift-guards for any changed verify-pipeline artifact.  FAIL-FAST: emitted
    # BEFORE add_test_passes (the expensive long-pole).  One guarded for-loop
    # per glob — the glob literal is embedded in the emitted subshell command
    # and expands at EXECUTION time under CWD=REPO_ROOT.
    # set -f / set +f prevents the shell from pathname-expanding the token
    # during loop iteration here at build time, so the literal glob string
    # (e.g. tests/infra/test_verify_*.sh) always reaches the emitted plan.
    # Suppressed when DF_VERIFY_ROLE=merge (task 5125): run_all.sh already runs
    # the full suite there (a superset), so the selective subset would
    # double-run hermetic tests. Exactly-one invariant (INV-5): every verify
    # runs either the full pool (merge/background) XOR the selective subset
    # (task/offline). background (task 5210) is suppressed here for the same
    # reason: it gets the full pool above, so the selective subset must not
    # also fire.
    # RE-ENTRANCY GUARD (task 5125): also suppressed when already inside an infra
    # suite (REIFY_INFRA_SUITE_ACTIVE set). The per-task selective path runs
    # test_verify_*.sh, which matches test_verify_semaphore_e2e.sh — whose
    # Section B drives a real merge-role verify that would otherwise re-launch
    # run_all. That nested verify sets the sentinel itself, scoped at its own
    # spawn site, so this guard fires for it WITHOUT this line broadcasting the
    # sentinel to every selected test (which would leak it as an ambient export;
    # cf. test_run_all_ambient_isolation.sh, task 4961).
    if [ "$DO_TEST" -eq 1 ] && [ -n "$SELECTED_INFRA_GLOBS" ] && [ "$DF_VERIFY_ROLE" != "merge" ] && [ "$DF_VERIFY_ROLE" != "background" ] && [ -z "${REIFY_INFRA_SUITE_ACTIVE:-}" ]; then
        local _glob
        set -f  # disable pathname expansion: keep glob tokens as literals
        for _glob in $SELECTED_INFRA_GLOBS; do
            add "( for _vt in $_glob; do [ -f \"\$_vt\" ] || continue; timeout --kill-after=60 10m bash \"\$_vt\" || exit \$?; done )"
        done
        set +f
    fi

    # test: gated + ungated cargo passes, per profile.
    # Emitted LAST — this is the expensive long-pole (psi-gate + full cargo
    # nextest run + OCCT-gated passes). All cheap gates run before this.
    # (task #4448 fail-fast reorder)
    if [ "$DO_TEST" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        add_test_passes
    fi
}
build_plan

# ---------------------------------------------------------------------------
# Emit: print the plan (oracle) or execute it (&& semantics)
# ---------------------------------------------------------------------------
if [ "$PRINT_PLAN" -eq 1 ]; then
    echo "# verify.sh plan — action=$ACTION profile=$PROFILE scope=$SCOPE include_infra=$INCLUDE_INFRA nextest=$NEXTEST role=$DF_VERIFY_ROLE"
    # NOTE (task 5125 review): a manual --include-infra run outside the
    # merge/background gate no longer gets the wholesale infra pool suite
    # (moved to the merge/background tier above) — only the cheaper selective
    # per-artifact subset runs. Flagged here so this isn't mistaken for full
    # local infra coverage. background (task 5210) is excluded from this NOTE
    # for the same reason merge is: it gets the full pool, not the subset.
    if [ "$INCLUDE_INFRA" -eq 1 ] && [ "$DF_VERIFY_ROLE" != "merge" ] && [ "$DF_VERIFY_ROLE" != "background" ]; then
        echo "# NOTE: include_infra=1 under role=$DF_VERIFY_ROLE gets the selective per-artifact infra subset only (scripts/verify-pipeline-infra-tests.txt) — the wholesale infra pool suite now runs at the merge tier exclusively, not here"
    fi
    echo "# scope decision — RUN_RUST=$RUN_RUST RUN_GUI=$RUN_GUI RUN_OCCT_GATE=$RUN_OCCT_GATE"
    echo "# narrowing — NARROW_ACTIVE=$NARROW_ACTIVE affected=${AFFECTED:-}"
    echo "# --- environment (process-level; inherited by every command below) ---"
    for _e in "${ENV_LINES[@]}"; do echo "# $_e"; done
    echo "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
    if [ "${#PLAN[@]}" -eq 0 ]; then
        echo "# (no commands — nothing to verify for this action/scope)"
    fi
    for _cmd in "${PLAN[@]+"${PLAN[@]}"}"; do
        case "$_cmd" in
            '@@SEMAPHORE_ACQUIRE@@')
                printf '# >>> test-run semaphore: ACQUIRE held slot — clock-stop region BEGINS (TEST-EXECUTION gated, held in verify.sh)\n'
                printf '#     A contended wait emits @@REIFY_CLOCK_STOP@@/@@REIFY_CLOCK_HEARTBEAT@@/@@REIFY_CLOCK_START@@ markers\n'
                printf '#     to stderr (reason=test_slot_starvation). dark_factory:1916 excludes the marked wait span\n'
                printf '#     from verify_command_timeout_secs. REIFY_TEST_SEMAPHORE_WAIT=unlimited activates\n'
                printf '#     continuous blocking wait (clock-stop mode); task 4838 activates the DF seam.\n'
                ;;
            '@@SEMAPHORE_RELEASE@@')
                printf '# <<< test-run semaphore: RELEASE held slot — clock-stop region ENDS (TEST-EXECUTION gated region finished)\n'
                ;;
            './scripts/verify.sh psi-gate')
                printf '# PSI gate: contended wait emits @@REIFY_CLOCK_STOP@@/HEARTBEAT/START@@ markers (reason=psi_pressure);\n'
                printf '#   the clock-stop span is excluded from verify_command_timeout_secs by dark_factory:1916 (task 4838).\n'
                printf '%s\n' "$_cmd"
                ;;
            *)
                printf '%s\n' "$_cmd"
                ;;
        esac
    done
    exit 0
fi

if [ "${#PLAN[@]}" -eq 0 ]; then
    echo "verify.sh: nothing to verify (action=$ACTION scope=$SCOPE) — no commands in plan." >&2
    exit 0
fi

for _cmd in "${PLAN[@]}"; do
    case "$_cmd" in
        '@@SEMAPHORE_ACQUIRE@@')
            test_semaphore_acquire || {
                _rc=$?
                echo "verify.sh: FAILED (exit $_rc): test-run semaphore acquire" >&2
                exit "$_rc"
            }
            continue
            ;;
        '@@SEMAPHORE_RELEASE@@')
            test_semaphore_release || true
            continue
            ;;
    esac
    echo "verify.sh: + $_cmd" >&2
    case "$_cmd" in
        *'_VERIFY_NODE_BG_PID'*)
            # Node-lane plan lines set/read $_VERIFY_NODE_BG_PID in the main
            # shell's scope (background npm + overlap-join wait) and must not
            # be dispatched into a subshell via reaper_run_in_pgroup.
            eval "$_cmd" || {
                _rc=$?
                echo "verify.sh: FAILED (exit $_rc): $_cmd" >&2
                exit "$_rc"
            }
            ;;
        *)
            # All other plan commands — cargo (nextest run, check, clippy),
            # infra tests, GUI feature checks, etc. — run in a dedicated process
            # group so reaper_teardown can clean them up on EXIT/INT/TERM/HUP.
            reaper_run_in_pgroup "$_cmd" || {
                _rc=$?
                echo "verify.sh: FAILED (exit $_rc): $_cmd" >&2
                exit "$_rc"
            }
            ;;
    esac
done
echo "verify.sh: all checks passed (action=$ACTION profile=$PROFILE scope=$SCOPE)." >&2
