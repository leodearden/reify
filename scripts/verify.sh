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
#   --test-threads=N               Cap test-execution parallelism at N (a positive
#                                  integer). Threaded into `cargo nextest run
#                                  --test-threads=N`, or the libtest
#                                  `-- --test-threads=N` fallback when nextest is
#                                  absent. Default: UNSET — the emitted plan stays
#                                  byte-identical to today. Primarily the offline
#                                  deep-test lane's parallelism knob (task 5264;
#                                  docs/design/offline-deep-test-lane.md §6).
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
#   - REIFY_AMBIENT_LD_LIBRARY_PATH — the loader path as it stood BEFORE those OCCT
#     prepends. /opt/reify-deps/lib is a whole conda prefix that shadows hundreds of
#     system sonames, and LD_LIBRARY_PATH outranks DT_RUNPATH, so the export above is
#     hostile to non-Rust subprocesses. Non-cargo "tool" plan lines are therefore
#     emitted via add_tool(), which restores this captured value for that line only
#     (task 5730; see the SCOPE CONTRACT comment in apply_env()).
#
# PSI gate (inter-dispatch throttle for multi-worktree verify bursts):
#   REIFY_PSI_GATE_THRESHOLD        — CPU avg10 % ceiling; dispatch waits until below this
#                                      value. Default: 50.
#   REIFY_PSI_GATE_WINDOW           — minimum inter-dispatch spacing in seconds.  Default: 20.
#   REIFY_PSI_GATE_MAX_WAIT         — give-up timeout (seconds), OR "unlimited" (THE
#                                      DEFAULT) for a continuous clock-stopped hold.
#                                      NOTE: a finite value exits 75 (EX_TEMPFAIL), and
#                                      dark-factory does NOT requeue that — verify.py
#                                      _classify_failure falls exit-75 through to
#                                      unknown_test_failure → debugfix loop → BLOCKED
#                                      (docs/prds/verify-admission-wait-clock-stop.md §2
#                                      verified this directly in DF source; the older
#                                      "so the orchestrator retries" claim here was
#                                      false). Default "unlimited" for parity with
#                                      dark-factory-orchestrator.yaml's verify_env — see
#                                      psi_gate() below and task 6393.
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
#                                  used as the first *gating* test-phase plan entry
#                                  (test/all) — a merge attempt-0 emits the tree-OID
#                                  sidecar stamp just before it (see psi_gate()).
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
#   REIFY_VERIFY_TEST_TIMEOUT   — outer timeout for the DEBUG (`--workspace`)
#                                  `cargo nextest run` pass. Default 60m
#                                  (workstation budget, η/4521 × 4.5).
#   REIFY_VERIFY_TEST_TIMEOUT_RELEASE — outer timeout for the RELEASE (`--release`)
#                                  `cargo nextest run` pass ONLY. Default 90m. A cold
#                                  sccache native-kernel relink (OCCT/OpenVDB/gmsh/
#                                  manifold — a separate sccache profile from debug)
#                                  pushes the combined release build+exec past the 60m
#                                  debug budget (task 5382, esc-5370-2). The merge OUTER
#                                  wall that must not preempt this pass is
#                                  merge_verify_cold_command_timeout_secs (task 5383) in
#                                  dark-factory-orchestrator.yaml — that key's comment
#                                  block is the single source for the relationship.
#   REIFY_VERIFY_PREBUILD_TIMEOUT — outer timeout for the merge-path RELEASE
#                                  pre-builds (`cargo build --release -p reify-audit`
#                                  and `-p reify-cli`). Default 45m. These run ONLY on
#                                  the merge/background path and are the step that
#                                  actually COLD-BUILDS the release native-kernel cone
#                                  (manifold-csg-sys/manifold3d/OCCT) — the release
#                                  nextest pass afterwards inherits a warm cone, so this
#                                  budget is transferred FROM the release nextest budget,
#                                  not added to it.
#                                  DERIVATION — read this before re-tuning. Unlike the
#                                  debug 60m (η/4521's 798.9 s worst-observed COMPLETION
#                                  × 4.5), the cold pre-build's true duration is
#                                  UNMEASURED. The only datum is a strict LOWER bound:
#                                  the former fixed 10m ceiling SIGTERM'd reify-cli
#                                  mid-`Compiling reify-runtime` on a cold merge lane
#                                  with zero failing assertions (esc-5382-1, same class
#                                  as esc-5370-2) — so the true cold duration is >10m,
#                                  by an unknown amount. 45m applies this file's house
#                                  4.5× production-weighted margin idiom to that lower
#                                  bound (10m × 4.5 = 45m), landing on the same tier as
#                                  clippy — the other full-scale cold compile wave here.
#                                  Because the margin multiplies a TRUNCATION point and
#                                  not an observed completion, 45m carries strictly less
#                                  assurance than the debug 60m does: if a cold pre-build
#                                  ever SIGTERMs at 45m, the correct response is to
#                                  MEASURE the real duration and re-derive, NOT to
#                                  multiply again. The earlier 25m was reverse-derived
#                                  from a 10800 − 3600 − 5400 ceiling-sum residual; that
#                                  constraint is not real (see the
#                                  merge_verify_cold_command_timeout_secs comment in
#                                  dark-factory-orchestrator.yaml for why the outer wall
#                                  never bounded the sum of inner ceilings), so nothing
#                                  caps this knob at 25m.
#                                  NB this step runs at `nice -n 5` OUTSIDE
#                                  compile_gate()/psi_gate()/the test semaphore (see the
#                                  ADMISSION CONTROLS note at the pre-build block), so a
#                                  contended cold merge lane is exactly the case the
#                                  margin has to absorb.
#   REIFY_VERIFY_CLIPPY_TIMEOUT — outer timeout for `cargo clippy` and the
#                                  gui-feature `cargo check -p reify-gui` pass.
#                                  Default 45m.
#   REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT — outer timeout for the gui-feature
#                                  TEST-EXECUTION pass (`-p reify-gui --features gui`)
#                                  emitted at the tail of add_test_passes(). Default 45m,
#                                  sized from measurement on the workstation: a cold
#                                  `--features gui` build (tauri + webkit2gtk + OCCT link)
#                                  took 20m42s; a warm re-run took 137s total of which 59s
#                                  was nextest execution over 906 tests. Distinct from
#                                  REIFY_VERIFY_CLIPPY_TIMEOUT because that knob budgets a
#                                  compile-only pass — this one budgets build + execution.
#   REIFY_VERIFY_CHECK_TIMEOUT  — outer timeout for `cargo check --workspace --tests`.
#                                  Default 30m.
#   Values validated as ^[0-9]+[smhd]?$; invalid → default + stderr warning.
#   Unset → identical render on the workstation (no-op). The leo-laptop verify-only
#   host (16t) may widen these via its dispatch env for per-host-measured budgets.
#
# retry_failed_only (PRD docs/prds/verify-retry-failed-only.md §4, task α):
#   A merge-gate retry can re-run ONLY the did-not-pass tests against the warm
#   _merge-verify target/ instead of a full recompile + full ~20,280-test run.
#   reify ships the PRIMITIVE (the exact-match nextest filterset + tree-OID guard
#   + loud full-fallback + size ceiling), built at ONE construction site in
#   emit_nextest_pass (INV-5); dark-factory (D2) owns the subset CONTENT via the
#   newline-delimited exact-id filter file and sets these envs. All unset →
#   plan byte-for-byte identical to today.
#   REIFY_VERIFY_RETRY_SCOPE            — set to `failed_only` to activate the
#                                  narrowed retry. Any other value / unset → full.
#   REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE       — newline-delimited EXACT nextest
#                                  test IDs; each becomes a `test(=<id>)` term in
#                                  ONE `-E 'test(=…) | …'` filterset. EXACT match
#                                  ONLY (never substring) so the subset can never
#                                  silently pull in an unintended test (§6.3).
#   …_FILTER_FILE_DEBUG / …_FILTER_FILE_RELEASE  — per-profile overrides; the
#                                  profile's own var wins when non-empty, else the
#                                  base var is used (so --profile both can carry a
#                                  different subset per profile).
#                                  DF-side obligation, newly LIVE since task 5548
#                                  made a red attempt-0 retry-eligible: a profile
#                                  whose attempt-0 nextest pass NEVER EXECUTED
#                                  (attempt-0 died in an earlier profile) must not
#                                  be narrowed by the base var's fallback. DF must
#                                  set a per-profile filter file for EVERY profile
#                                  named in the sidecar's `profiles`, or fall back
#                                  to a full verify. reify cannot tell "passed" from
#                                  "never ran" — it never sees attempt-0's results —
#                                  so this is a seam obligation, not a guard reify
#                                  can add; the @@REIFY_RETRY_SCOPE=failed_only@@
#                                  marker's per-profile applied counts expose the
#                                  outcome either way.
#   REIFY_VERIFY_RETRY_TREE_OID         — the tree DF intends to retry (a `git
#                                  rev-parse HEAD:` TREE OID). Must equal the
#                                  attempt-0 sidecar's tree_oid, else the subset is
#                                  refused (tree-pin soundness, §5 INV-1).
#   REIFY_VERIFY_ATTEMPT_SIDECAR        — attempt-0 sidecar path (default
#                                  target/reify-verify-attempt.json). Schema is
#                                  PRD §4.1's {tree_oid, profiles, timestamp}
#                                  verbatim, recording the tree target/ was built
#                                  FROM — NOT proof any profile executed or passed
#                                  (the eligibility guard reads tree_oid only).
#                                  Stamped on a full DF_VERIFY_ROLE=merge attempt-0
#                                  (never on a failed_only retry), at the HEAD of
#                                  add_test_passes — see there for the single
#                                  authoritative survives/does-not-survive matrix
#                                  (task 5548). Survives a warm-lane reseed.
#   REIFY_VERIFY_RETRY_MAX_SUBSET       — tunable subset-size ceiling (default 5000,
#                                  a heuristic comfortably below the full-suite size,
#                                  §11): a subset larger than this is refused as a
#                                  likely construction bug (INV-4 storm escape).
#   Three LOUD full-fallback reasons (build-time `echo … >&2`, visible in both
#   --print-plan stderr and a real run, never silent — §4.3): `retry refused: tree
#   drift` (sidecar absent / tree_oid mismatch), `retry refused: no subset` (filter
#   file absent/empty/unreadable), `retry refused: subset too large` (> ceiling).
#   Out of α's scope: the @@REIFY_RETRY_SCOPE=failed_only@@ honest marker is
#   emitted by sibling task δ (tests/infra/test_verify_retry_failed_only.sh).
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
#                                          clock-stop marker emission. THE DEFAULT since
#                                          task 6393; the "keep FINITE until task 4838
#                                          deploys DF" gating is spent (deployed
#                                          2026-06-27). A finite value is still honoured
#                                          when set explicitly.
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

# Git repository-environment scrub for infra-test EXECUTION (task #7106).
# Provides REIFY_GIT_ENV_SCRUB_VARS / reify_git_env_scrub /
# reify_git_env_scrub_prefix; the last is interpolated into the selective-infra
# plan leaf below. The defect, its measurement and the variable list live in ONE
# place — scripts/lib_git_env_scrub.sh's header — and are deliberately NOT
# restated here; only this site's own reasoning is.
#
# Sourced HERE, at load time, and applied only at the test-execution leaf: the
# scope-derivation phase legitimately reads the hook environment
# (CHANGED_FILES_RAW comes from `git diff --cached`) and must stay untouched.
if [ ! -f "$SCRIPT_DIR/lib_git_env_scrub.sh" ]; then
    echo "verify.sh: ERROR — scripts/lib_git_env_scrub.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/lib_git_env_scrub.sh
source "$SCRIPT_DIR/lib_git_env_scrub.sh"

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

# Resolve the compile-budget tiers once at startup.  Defaults match the
# workstation-measured budgets (unset → identical render, no-op on workstation).
# The test budget splits PER PROFILE (task 5382): the DEBUG (--workspace) pass uses
# REIFY_VERIFY_TEST_TIMEOUT (60m); the RELEASE (--release) pass uses its own cold-aware
# REIFY_VERIFY_TEST_TIMEOUT_RELEASE (90m) — see add_test_passes for the derivation.
_VERIFY_TEST_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT 60m)"
_VERIFY_TEST_TIMEOUT_RELEASE="$(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE 90m)"
_VERIFY_PREBUILD_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_PREBUILD_TIMEOUT 45m)"
_VERIFY_CLIPPY_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_CLIPPY_TIMEOUT 45m)"
_VERIFY_GUI_FEATURE_TEST_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT 45m)"
_VERIFY_CHECK_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_CHECK_TIMEOUT 30m)"

usage() {
    sed -n '2,59p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# PSI gate — throttle per-task test phases under multi-worktree verify bursts
# ---------------------------------------------------------------------------

# psi_gate() — thin wrapper over cpu_admit requeue (scripts/cpu-admit.sh).
# Called directly via `verify.sh psi-gate` (testable entry point) and wired
# as the first *gating* test-phase plan entry by add_test_passes() — a merge
# attempt-0 emits the tree-OID sidecar stamp (a side-effect-only file write,
# not a gate) immediately before it, task 5548.
#
# Environment knobs (see header comment block for full doc):
#   REIFY_PSI_GATE_THRESHOLD    — avg10 ceiling to allow dispatch (default 50)
#   REIFY_PSI_GATE_WINDOW       — min seconds between dispatches (default 20)
#   REIFY_PSI_GATE_MAX_WAIT     — give-up timeout in seconds, or "unlimited"
#                                 (the default: continuous clock-stopped hold)
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
    local _ca_max_wait="${REIFY_PSI_GATE_MAX_WAIT:-unlimited}"
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
TEST_THREADS_SET=0   # 1 once --test-threads is seen; lets validation reject an explicit empty value ('--test-threads=') while an UNSET flag stays valid.

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
            TEST_THREADS="${2:?--test-threads requires an argument}"; TEST_THREADS_SET=1; shift 2 ;;
        --test-threads=*)
            TEST_THREADS="${1#*=}"; TEST_THREADS_SET=1; shift ;;
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
# --test-threads=N (task 5264): positive-integer validation, mirroring the
# --profile/--scope invalid-value exit-64 convention. Guard on TEST_THREADS_SET
# (not `[ -n ]`) so an explicit empty value ('--test-threads=') is rejected
# while an UNSET flag stays valid and leaves the default plan byte-identical.
# The '*[!0-9]*' arm rejects any non-digit (incl. '-' and '.'); '' rejects the
# empty value; '0*' rejects zero AND every leading-zero form ('0', '00', '007')
# — a leading zero would otherwise reach cargo/nextest as e.g. '--test-threads=00',
# which they parse as 0 and reject only at runtime, AFTER build work has started,
# defeating this parse-time fail-fast. Net effect: exactly ^[1-9][0-9]*$.
if [ "$TEST_THREADS_SET" -eq 1 ]; then
    case "$TEST_THREADS" in ''|*[!0-9]*|0*)
        echo "verify.sh: ERROR — invalid --test-threads '$TEST_THREADS' (want positive integer)" >&2; exit 64 ;;
    esac
fi
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

# retry_failed_only (task 5287, PRD verify-retry-failed-only §4/§6, task α):
# consume a dark-factory-supplied "failed-only" retry subset so a merge-gate
# retry re-runs ONLY the did-not-pass tests against the warm _merge-verify
# target/, instead of a full recompile + full ~20,280-test run. reify ships the
# PRIMITIVE (the exact-match nextest filterset + tree-OID guard + loud
# full-fallback + size ceiling); DF (D2) owns the subset CONTENT via the
# newline-delimited exact-id filter file and sets the consumed envs (INV-5,
# single construction site in emit_nextest_pass). Default (all retry envs
# unset) → every fragment empty → plan byte-for-byte identical to today.
#
# Path of the attempt-0 sidecar (written on every merge-role attempt-0, green
# or red — task 5548 — read here to tree-pin the retry). Overridable for
# hermetic tests. Relative default is resolved against REPO_ROOT (verify.sh
# cds there before build_plan/execute).
_ATTEMPT_SIDECAR_PATH="${REIFY_VERIFY_ATTEMPT_SIDECAR:-target/reify-verify-attempt.json}"
# Precomputed once in add_test_passes (before the profile loop); initialized
# here so emit_nextest_pass stays nounset-safe (set -u) on any call path.
# _RETRY_SUBSET_ELIGIBLE: the caller asked for the narrowed retry scope
# (failed_only) AND the on-disk attempt-0 sidecar tree_oid matches the tree DF
# intends to retry — i.e. the warm target/ provably corresponds to it. The
# scope=failed_only decision is re-read directly from REIFY_VERIFY_RETRY_SCOPE
# where it is independently needed (the sidecar-stamp guard at the HEAD of
# add_test_passes), so no separate "retry active" flag is carried.
_RETRY_SUBSET_ELIGIBLE=0
# Per-suite APPLIED subset sizes for the δ honest marker (task 5290). Recorded
# at each suite's SINGLE narrowing site (INV-5 no re-derivation): the nextest
# counts by emit_nextest_pass as it applies a within-ceiling subset per profile;
# the gui count by the gui block from its validated REIFY_GUI_RETRY_SPECS. Each
# stays 0 on any fallback / when the suite did not narrow, so the marker gate
# and its printed counts are honest (0 ⇒ that suite ran FULL). Initialized here
# so the end-of-build_plan emitter stays nounset-safe (set -u) on any path.
_RETRY_NEXTEST_DEBUG_APPLIED=0
_RETRY_NEXTEST_RELEASE_APPLIED=0
_RETRY_GUI_SUBSET_APPLIED=0
# Subset-size ceiling (INV-4 retry-storm escape / PRD §4.3): a subset that
# approaches the whole ~20,280-test suite means DF built a bad subset (a
# construction bug), so refuse it and run FULL rather than dressing a full run
# up as a subset. The default is a tunable HEURISTIC (PRD §11 open question),
# NOT a first-principles number — chosen comfortably below the full-suite size.
# Overridable via REIFY_VERIFY_RETRY_MAX_SUBSET.
_RETRY_MAX_SUBSET="${REIFY_VERIFY_RETRY_MAX_SUBSET:-5000}"

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
    #
    # SCOPE CONTRACT (task 5730, esc-4581-87 / task 5321) — read before touching:
    # /opt/reify-deps/lib is NOT an OCCT lib dir, it is a whole conda prefix.
    # Alongside the ~153 libTK* it carries libcrypto.so.3, libcurl.so.4,
    # libexpat.so.1, libz.so.1, libcairo.so.2, libEGL.so.1, libsqlite3.so.0 and
    # hundreds of other system sonames (477 measured 2026-07-28 by intersecting
    # its .so-bearing filenames with /usr/lib/x86_64-linux-gnu — a DATED
    # measurement of unversioned host state that drifts on every environment
    # refresh, not an invariant count). LD_LIBRARY_PATH outranks DT_RUNPATH and
    # ld.so.cache in the loader search order, so exporting it process-wide is
    # HOSTILE to every non-Rust subprocess of the gate: it silently substitutes
    # conda libraries under bare CLI tools. sqlite3 is merely the one that
    # self-checks its header/source hash and aborts loudly.
    #
    # The export stays because it is belt-and-braces for the Rust/cargo path
    # (see the header note above: .cargo/config.toml's `runner` and the
    # DT_RUNPATH baked into every bin/test binary are the primary mechanisms).
    # Non-cargo "tool" plan lines are instead emitted via add_tool(), which
    # prefixes them with a scrub restoring the value captured here.
    # REIFY_AMBIENT_LD_LIBRARY_PATH is the SINGLE SOURCE OF TRUTH for that
    # restore, so it must be captured HERE — before either prepend below —
    # or every scrub built from it degrades to a no-op. Restoring the captured
    # ambient rather than a hardcoded "" preserves any legitimate loader path
    # the operator or orchestrator set before invoking verify.sh.
    # Guarded by tests/infra/test_verify_ld_library_path_scope.sh.
    export REIFY_AMBIENT_LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}"
    ENV_LINES+=("export REIFY_AMBIENT_LD_LIBRARY_PATH=${REIFY_AMBIENT_LD_LIBRARY_PATH}")

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

# _RUST_COUPLED_RI_FIXTURES (task 5536) — the tests/prd-gate/fixtures/*.ri
# basenames NAMED BY a compiled Rust test target, and so EXCLUDED from
# decide_scope's no-heavy-checks carve-out for that directory:
#   proven runtime reads (a #[test] builds the path and opens the file):
#     geometry_let_selector_consumer.ri (pushed into no_stale_undef_invariant_
#     gate.rs's corpus_files()), geometry_let_selector_consumer_edit.ri,
#     stdlib_ns_buckling_mode_coexist.ri, unit_nm_torque_immediate.ri
#     (read via std::fs::read_to_string by torque_unit_tests.rs, task 5786),
#     unit_curated_labels_ascii.ri (likewise, by volume_unit_tests.rs, task 5788),
#     unit_middot_mul.ri (same idiom, unit_middot_mul_tests.rs, task 5784),
#     jacobian_column_members.ri (read via std::fs::read_to_string by
#     jacobian_column_member_access.rs's fixture_source(), task 6102),
#     damped_material_mixin_conformance.ri + damped_material_preset_conformance.ri
#     (both read via std::fs::read_to_string by materials_fea_tests.rs, task
#     6877 — the mixin one pins the Damped-mixin SUBSTRATE with probe-local
#     trait names, the preset one pins the LANDED stdlib surface)
#   compile-time embeds (include_str! bakes the bytes straight into the test
#   binary — a tighter coupling than a runtime read, since the fixture is a
#   build input of the target, not just a file it happens to open):
#     indexed_sub_coll_arm_baseline.ri, indexed_sub_forall_range_baseline.ri,
#     indexed_sub_inst_arm_baseline.ri, indexed_sub_spec_arm_baseline.ri
#     (the four sub-arm baselines held as indexed_sub_grammar_tests.rs's
#     existing_sub_arms_regression_floor, task 5481);
#     stdlib_ns_qualified_expr.ri, stdlib_ns_qualified_type.ri (the two
#     qualified-reference probes held as qualified_ref_grammar_tests.rs's
#     prd_gate_qualified_{expr,type}_fixture_parses_with_zero_errors, task 5495)
#   conservative, doc-comment mentions only today (listing a name is cheap, and
#   a doc mention is usually the first trace of a read about to exist):
#     compiler_type_hygiene_trait_args_silent_accept.ri, stdlib_ns_mode_member.ri,
#     cost_robustness_tradeoff_form.ri (task 5711: reify-constraints'
#     cost_robustness_tradeoff_blend.rs and solver.rs cite it as the .ri form
#     the in-Rust blend fixtures mirror — no read today)
# Deliberately NO file:line citations: nothing validates them and they rot on
# the first edit to those tests. Membership's SOURCE OF TRUTH is behavioural —
# tests/infra/test_verify_scope.sh's PG-DRIFT scenario derives the referenced
# set from the real repo (`git grep -o 'tests/prd-gate/fixtures/*.ri' over ALL
# tracked *.rs`) and asserts each still classifies RUN_RUST=1, so adding a new
# Rust reference without listing it here goes RED; a companion assertion there
# also fails if any *.rs names the fixtures DIRECTORY rather than a single
# <name>.ri leaf, which would void this arm's premise outright (see below).
# Space sentinels give whole-token matching
# (mirrors select_infra_tests/select_harness_kloc_guard) — required here
# because one name is a strict prefix of another
# (geometry_let_selector_consumer.ri vs …_consumer_edit.ri).
_RUST_COUPLED_RI_FIXTURES=" compiler_type_hygiene_trait_args_silent_accept.ri cost_robustness_tradeoff_form.ri damped_material_mixin_conformance.ri damped_material_preset_conformance.ri geometry_let_selector_consumer.ri geometry_let_selector_consumer_edit.ri indexed_sub_coll_arm_baseline.ri indexed_sub_forall_range_baseline.ri indexed_sub_inst_arm_baseline.ri indexed_sub_spec_arm_baseline.ri jacobian_column_members.ri r3b_displacement_at_selector_grammar.ri stdlib_ns_buckling_mode_coexist.ri stdlib_ns_mode_member.ri stdlib_ns_qualified_expr.ri stdlib_ns_qualified_type.ri unit_curated_labels_ascii.ri unit_middot_mul.ri unit_nm_torque_immediate.ri "

# GUI-COUPLED prd-gate fixtures (task 6435). Basenames PINNED in EXPECTED_CLEAN
# in gui/src/__tests__/reifyGrammarCorpus.test.ts — the grammar drift ledger,
# which sets CORPUS_ROOTS = ['examples', 'tests/prd-gate/fixtures'] and walks
# this directory RECURSIVELY (landed 2026-08-01, f709595212).
#
# WHY THIS LIST EXISTS. The ledger asserts that every pinned path still parses
# with zero ERROR nodes. Editing a pinned fixture into an unparseable state is
# therefore a real regression of a committed signal — but the arm below sets
# gui=0 for a fixture that is not RUST-coupled, so under --scope staged/branch
# the vitest suite that reads it never runs. On the ONE route that has no later
# gate (a hook-gated docs commit on `main`, which is the sanctioned way PRD
# fixtures land) that edit reaches main GREEN. This is the mirror image of the
# docs/prds/**/fixtures/ hole that task 6431 closes, and it is closed here the
# same way: name the coupled set, and let a drift guard keep the name list
# honest rather than a human.
#
# Kept honest by tests/infra/test_verify_scope.sh's PG-DRIFT-GUI scenario, which
# re-derives this set from the EXPECTED_CLEAN literal on every infra run and
# fails on any pinned fixture missing here. Do not hand-edit without re-running
# it; do not trust a copy of this list anywhere else.
#
# NOTE the two lists MOSTLY NEST: every _RUST_COUPLED_RI_FIXTURES member that is
# also a grammar-ledger pin is listed below as well, and the rust arm already
# sets gui=1, so it short-circuits them. They are retained here deliberately so
# that dropping a fixture from the rust list can never silently drop its gui
# coverage too. THREE members are not EXPECTED_CLEAN pins and so have no gui
# entry to retain: jacobian_column_members.ri (read by a compiled Rust target
# but never pinned, task 6102), and task 6877's
# damped_material_{mixin,preset}_conformance.ri (deliberately not pinned — an
# unpinned fixture is inert for that ledger, so pinning them would add the
# PG-DRIFT-GUI obligation for no added signal).
#
# Deliberately unnumbered: nothing validates a count in prose (PG-DRIFT checks
# MEMBERSHIP, PG-DRIFT-GUI checks the ledger), so a hard-coded size silently
# rots on the next addition — this one already had, still reading 10 after the
# list had grown past it, when task #5784 added unit_middot_mul.ri.
_GUI_COUPLED_RI_FIXTURES=" bare_angle_silently_accepted.ri collection_expr_index_resolves.ri collection_sub_at_placement_rejected.ri collection_sub_member_cell_consumable.ri collection_sub_per_member_cells.ri collection_sub_value_position_undef_baseline.ri compiler_type_hygiene_integration_gate.ri compiler_type_hygiene_mul_scale_guard_defeat.ri compiler_type_hygiene_mul_vec_silent_int.ri compiler_type_hygiene_trait_args_silent_accept.ri cost_min_money_objective.ri cost_robustness_tradeoff_form.ri cross_sub_geometry_ref.ri dcr_dimension_rejection_channel_fires.ri dcr_fn_force_param_already_rejects.ri dcr_langsurface_crossdim_silent.ri dcr_load_ctor_dimension_silent.ri dcr_load_retype_target_resolves.ri dcr_material_dimension_correct.ri dcr_material_dimension_silent.ri dcr_reader_ctor_dimension_silent.ri dcr_shaper_frequency_dimension_silent.ri dcr_solver_load_dropped_bare.ri dcr_solver_load_dropped_dimensioned.ri dcr_yield_stress_dimension_silent.ri engine_build_hardening_kappa_mixed_kernel_selector.ri expected_type_pushdown_arg.ri expected_type_pushdown_let.ri faces_by_normal_symbolic_eval_silent.ri forall_collection_resolves.ri forall_range_domain_rejected.ri geometry_let_selector_consumer_edit.ri geometry_let_selector_consumer.ri hand_placed_twin_two_subs_eval.ri indexed_sub_bare_member_resolves.ri indexed_sub_coll_arm_baseline.ri indexed_sub_forall_range_baseline.ri indexed_sub_inst_arm_baseline.ri indexed_sub_oob_computed_silent_undef.ri indexed_sub_oob_literal_silent_undef.ri indexed_sub_self_member_misrouted.ri indexed_sub_self_member_nogeom_unsupported.ri indexed_sub_silent_undef_baseline.ri indexed_sub_spec_arm_baseline.ri ir_clean_eval.ri objective_inherit_ambiguous.ri posed_subs_distance_query_unresolvable.ri purpose_nested_structure.ri quantifier_expr_int_domain_resolves.ri quantifier_expr_member_access_rejected.ri quantifier_expr_range_domain_rejected.ri r3b_displacement_at_selector_grammar.ri revolute_silent_accept.ri scalar_codomain_mismatch.ri self_collection_count_redirect_rejected.ri single_sub_pose_resolves.ri stdlib_ns_buckling_mode_coexist.ri stdlib_ns_mode_member_modal.ri stdlib_ns_mode_member.ri stdlib_ns_qualified_expr.ri stdlib_ns_qualified_type.ri stdlib_ns_std_nonexistent_import.ri stdlib_units_import_resolves.ri subbody_objective_ignored.ri transform3_unresolved.ri typeparam_member_access.ri uncons_box_no_error.ri unit_curated_labels_ascii.ri unit_middot_mul.ri unit_nm_torque_immediate.ri "

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
    # Rename SOURCES under the prd-gate carve-out directory (task 5536).
    # --name-only prints only a rename's DESTINATION (measured: `git mv a.ri
    # b.ri` yields just `b.ri`, while --name-status shows `R100 a.ri b.ri`), so
    # `git mv <coupled>.ri <new>.ri` would present a basename that is NOT in
    # _RUST_COUPLED_RI_FIXTURES and classify no-heavy — while the #[test] that
    # opens the OLD literal path is now broken, on the one route (a hook-gated
    # docs commit on `main`) with no later gate. Recover the source side of R
    # entries so the exclusion list sees it. Explicit -M makes this independent
    # of ambient diff.renames config: with rename detection OFF there are no R
    # entries and the primary list already carries the source as a `D`, so the
    # union is the same set either way. Failure here (after the primary diff
    # succeeded) is not expected — fail WIDE rather than silently lose the
    # source side (contract C5). An empty result is the normal case, not a
    # failure.
    local _rsrc="" _classify=""
    if [ "$SCOPE" = "branch" ]; then
        if ! _rsrc="$(git -C "$REPO_ROOT" diff --name-status --diff-filter=R -M "$_MERGE_BASE" | cut -f2)"; then
            echo "verify.sh: WARNING — --scope branch rename-source diff failed — failing WIDE to --scope all (contract C5)" >&2
            RUN_RUST=1; RUN_GUI=1; RUN_OCCT_GATE=1
            return
        fi
    else
        if ! _rsrc="$(git -C "$REPO_ROOT" diff --cached --name-status --diff-filter=R -M | cut -f2)"; then
            echo "verify.sh: WARNING — --scope staged rename-source diff failed — failing WIDE to --scope all (contract C5)" >&2
            RUN_RUST=1; RUN_GUI=1; RUN_OCCT_GATE=1
            return
        fi
    fi
    _rsrc="$(grep -E '^tests/prd-gate/fixtures/.*\.ri$' <<< "$_rsrc" || true)"
    # Classify over files + recovered rename sources, but leave CHANGED_FILES_RAW
    # (below) built from _files alone, so Phase-2 narrowing / infra-test
    # selection see the byte-identical list they saw before this arm existed.
    _classify="$_files"
    if [ -n "$_rsrc" ]; then
        _classify="$(printf '%s\n%s' "$_files" "$_rsrc")"
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
            tests/prd-gate/fixtures/*.ri)
                # PRD-gate probe fixtures (task 5536): no heavy checks, so the
                # /prd SOP's one-commit docs landing on `main` (PRD .md +
                # .capability-manifest.yaml + its .ri fixtures, gated by
                # hooks/pre-commit -> `--scope staged`) stays a seconds-long
                # hook instead of escalating to a full workspace nextest run.
                #   • Nothing globs this directory: reify-eval's
                #     no_stale_undef_invariant_gate.rs corpus_files() walks only
                #     its own tests/fixtures + examples/, then pushes ONE
                #     explicit prd-gate path — so ADDING a fixture provably
                #     cannot change any Rust target's inputs. EDITING one of the
                #     names in _RUST_COUPLED_RI_FIXTURES can, hence the exclusion
                #     below (a blanket rule would let such an edit reach `main`
                #     through the hook-gated docs path with no heavy checks and
                #     no later gate — a red-main class outage). That
                #     no-directory-glob premise is not left to this comment:
                #     PG-DRIFT fails if any *.rs names the directory itself
                #     rather than a `<name>.ri` leaf. That residual gate gap is not
                #     hypothetical: an UNCOUPLED prd-gate fixture carrying a
                #     phantom-tracking marker landed exactly this way on `main`
                #     108d1d9226, reddening post-merge verification for every task
                #     until 9ebebcec22 reworded it (task 6817).
                #     select_cheap_ptodo_gate (below) now runs the PTODO ratchet on
                #     this exact --scope staged hook path, closing that gap for
                #     both the coupled- and uncoupled-fixture shapes.
                #   • RENAMES reach this arm by BOTH names: --name-only shows
                #     only a rename's destination, so the source side is
                #     recovered up-front (see the rename-source block above) —
                #     `git mv <coupled>.ri <new>.ri` therefore still classifies
                #     conservatively instead of slipping through under a fresh
                #     basename.
                #   • The other consumers are the scripts/prd-capability-check.py
                #     / prd-decompose-verify.mjs probes, which are not cargo poles.
                #   • RESIDUAL: --scope staged is now covered by
                #     select_cheap_ptodo_gate (below), which runs the PTODO
                #     ratchet on this exact hook path (task 6817). --scope branch
                #     (per-task lanes) stays uncovered here: DF_VERIFY_ROLE=merge
                #     still forces --scope all (contract C2), so the merge gate's
                #     run_all.sh pool remains the wholesale authority there — the
                #     same accepted latency-not-coverage trade-off documented at
                #     the task 5125 block below.
                #   • GOTCHA: a bash `case` glob's `*` matches `/`, so a future
                #     nested path under fixtures/ also lands in this arm; the
                #     ${f##*/} basename test still applies and the direction of
                #     error stays conservative.
                case "$_RUST_COUPLED_RI_FIXTURES" in
                    *" ${f##*/} "*) rust=1; gui=1; gate=1 ;;  # runtime input to a compiled test target — keep today's conservative classification
                    *)
                        # Not read by a compiled Rust target. It may still be a
                        # PINNED signal in the GUI grammar drift ledger, whose
                        # suite would otherwise never run for this edit under
                        # --scope staged/branch (task 6435).
                        case "$_GUI_COUPLED_RI_FIXTURES" in
                            *" ${f##*/} "*) gui=1 ;;          # pinned in reifyGrammarCorpus.test.ts EXPECTED_CLEAN
                            *) : ;;                           # pure probe-data fixture — no heavy checks
                        esac
                        ;;
                esac
                ;;
            docs/*|*.md|*.yaml|*.yml)
                : # no heavy checks
                ;;
            *)
                # Unrecognised path: be conservative.
                rust=1; gui=1; gate=1
                ;;
        esac
    done <<< "$_classify"

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

# add_selected_infra_glob <glob-or-path> — append into SELECTED_INFRA_GLOBS
# with whole-token dedup via space sentinels (prevents false dedup when one
# glob is a substring of another, e.g. a specific path vs a broader wildcard
# pattern). Shared by every selector below (select_infra_tests,
# select_harness_kloc_guard, select_cheap_ptodo_gate, ...) so the sentinel
# trick is written once instead of hand-copied per selector.
add_selected_infra_glob() {
    case " $SELECTED_INFRA_GLOBS " in
        *" $1 "*) : ;;
        *) SELECTED_INFRA_GLOBS="${SELECTED_INFRA_GLOBS:+$SELECTED_INFRA_GLOBS }$1" ;;
    esac
}

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
                # Append glob to selection if not already present.
                add_selected_infra_glob "$_glob"
                break
            fi
        done <<< "$CHANGED_FILES_RAW"
    done < <(grep -v '^\s*#' "$_VP_INFRA_MAP" | grep -v '^\s*$')
}
select_infra_tests

# ---------------------------------------------------------------------------
# Branch-scope at-source trigger for the harness-kLOC guard (task #5328, PRD
# docs/prds/merge-gate-guard-diagnosability.md leaf R2).
#
# tests/infra/test_harness_kloc_cap.sh (the C2 anti-re-accretion guard) has no
# row in verify-pipeline-infra-tests.txt: its trigger isn't a single artifact
# path, it's *growing* a harness — a new top-level standalone
# crates/<c>/tests/*.rs file, OR an added/modified
# crates/<c>/tests/harness_<subsystem>.rs root or harness_<subsystem>/**
# nested file (recursive), in one of the 5 consolidatable crates. A nested
# path OUTSIDE a harness_*/ module dir (tests/common/, fixtures/, …) is
# deliberately excluded. This selector runs the guard ONLY under --scope
# branch (hermetic static reads, no cargo — seconds), so an introducer sees
# the violation on their own branch instead of first at the merge gate 40+
# min in (tasks 5213, 5053).
#
# What rule (a) actually measures TODAY, precisely: harness_layout_violations()
# in test_harness_kloc_cap.sh globs only the TOP-LEVEL "$tests_dir"/*.rs
# (non-recursive) and runs `wc -l` on each harness_*.rs ROOT alone — it never
# descends into a harness_<subsystem>/ dir and never sums nested-file lines
# into the root's measured value. So a nested harness_*/** file does NOT
# itself move today's landed rule-(a) number; only the root does. Task #5463
# (in-progress, tracked, un-landed) is what will change rule (a) to sum the
# root plus its whole harness_<subsystem>/ subtree recursively. The
# harness_*/* arm below is kept ahead of that landing for two reasons: (i) it
# is the forward-compatible trigger for #5463's future behaviour, so this
# verify-pipeline artifact (which forces the full merge gate to edit) doesn't
# need an immediate follow-up the day #5463 lands; (ii) even today it is a
# cheap, redundant belt-and-braces net for a nested edit whose C1-required
# root `#[path] mod` companion edit is somehow missing from the same branch
# diff — the ordinary C1-compliant case is already caught by the harness_*.rs
# root arm, since adding or growing a nested module requires a corresponding
# root edit.
#
# Diff-status policy (git diff --diff-filter=AM), per path shape: (i) a
# harness root (harness_*.rs) accepts BOTH A and M — the root's own line count
# IS what rule (a) measures today, and a modified root is also the only
# visible diff trace a rename-based consolidation absorb leaves (a `git mv
# tests/foo.rs tests/harness_x/foo.rs` plus the C1-required `#[path =
# "harness_x/foo.rs"] mod foo;` root edit — git's --diff-filter excludes R
# entries, verified empirically); (ii) a nested harness-module file
# (harness_*/**) ALSO accepts BOTH A and M, for the forward-looking /
# belt-and-braces reasons above — NOT because it moves today's measure; (iii)
# a top-level non-harness standalone accepts ONLY A — rule-(b) re-accretion is
# an ADD by construction, so widening to M would fire on ordinary edits to an
# already-grandfathered file for zero possible signal. Either way, the
# residual latency-not-a-coverage-hole doctrine is unchanged: the merge gate
# remains the wholesale authority, so anything this selector misses is still
# caught there.
#
# Appends into the SAME SELECTED_INFRA_GLOBS the selective-infra block above
# populates, so it inherits for free: (a) merge/background suppression — the
# selective emission block is suppressed when DF_VERIFY_ROLE=merge|background
# (run_all.sh already runs this guard wholesale there — exactly-once, INV-5);
# scope=branch is structurally impossible under merge|background role (forced
# to scope=all at :644), so this selector can never double-fire against the
# merge-tier wholesale run; (b) the REIFY_INFRA_SUITE_ACTIVE re-entrancy
# guard; (c) fail-fast ordering before the cargo poles.
#
# Keep the 5-crate list below in sync with CONSOLIDATABLE_CRATES in
# tests/infra/test_harness_kloc_cap.sh (that guard is the source of truth).
# Drift is non-catastrophic in either direction: the merge gate remains the
# wholesale authority, so an omitted/stale crate here still gets caught at
# merge (latency, not a coverage hole).
# ---------------------------------------------------------------------------
select_harness_kloc_guard() {
    [ "$SCOPE" = "branch" ] || return 0
    [ -n "$_MERGE_BASE" ] || return 0
    local _changed _status _path _rest _crate _tail
    _changed="$(git -C "$REPO_ROOT" diff --name-status --diff-filter=AM "$_MERGE_BASE" 2>/dev/null)" || return 0
    [ -n "$_changed" ] || return 0
    while IFS=$'\t' read -r _status _path; do
        [ -n "$_path" ] || continue
        case "$_path" in
            crates/*/tests/*.rs) : ;;
            *) continue ;;
        esac
        _rest="${_path#crates/}"
        _crate="${_rest%%/*}"
        _tail="${_path#crates/$_crate/tests/}"
        # Ordered classification — harness_*/* MUST come first: a bash `case`
        # glob's `*` matches `/`, so this one arm covers arbitrary nesting
        # depth under a harness_<subsystem>/ module dir (same gotcha
        # documented in harness_layout_in_scope_standalone's header comment
        # in tests/infra/harness-layout-lib.sh — cited by function name, not
        # line range, since a line-range cite into another file re-rots on
        # every edit to that file). harness_*.rs
        # (a root has no slash, so neither prior arm can catch it) is checked
        # after the generic */* reject. harness_*.rs and harness_*/* both
        # accept A or M (see the block header for why the nested arm's A|M is
        # a forward-looking/belt-and-braces trigger rather than a claim that
        # it moves today's rule-(a) measure); a top-level standalone stays
        # adds-only.
        case "$_tail" in
            harness_*/*)  : ;;                       # nested module file: A or M (see block header)
            */*)          continue ;;                # nested, not a harness module dir
            harness_*.rs) : ;;                       # harness root compile unit: A or M
            *)  [ "$_status" = "A" ] || continue ;;  # top-level standalone: adds only
        esac
        case " reify-cli reify-syntax reify-kernel-occt reify-eval reify-compiler " in
            *" $_crate "*)
                add_selected_infra_glob "tests/infra/test_harness_kloc_cap.sh"
                return 0
                ;;
        esac
    done <<< "$_changed"
}
select_harness_kloc_guard

# ---------------------------------------------------------------------------
# Cheap PTODO ratchet on the hook-gated --scope staged path (task 6817).
#
# hooks/pre-commit -> hooks/project-checks is the ONLY production caller of
# `--scope staged` (grep -rn -- "--scope staged" over tracked files confirms
# this); every per-task lane uses `--scope branch`, and merge/background force
# `--scope all` (contract C2). A hook-gated main commit is therefore the one
# and only landing path with NO later gate: task 5125 moved the PTODO ratchet
# (tests/infra/test_reify_audit_ptodo.sh, scenario (a) — live ptodo-baseline-gen
# fingerprints must be a subset of crates/reify-audit/ptodo-baseline.txt) to the
# MERGE-tier run_all.sh pool only, so a docs-landing commit staging an
# uncoupled tests/prd-gate/fixtures/*.ri (task 5536's no-heavy-checks carve-out
# above) got neither the full pool nor a selective subset. 108d1d9226's
# phantom-tracking marker landed on `main` through exactly this gap and
# reddened post-merge verification for every task until 9ebebcec22 reworded it
# (task 6817). Deliberately NOT extended to `--scope branch`: that would be
# doing 5125's explicitly deferred follow-up ("a cheap per-task-only PTODO
# precheck ... is a possible follow-up if per-task PTODO latency proves costly
# in practice") and would add ~3.4s to every task lane for a signal the merge
# gate already provides there.
#
# Appends into the SAME SELECTED_INFRA_GLOBS the selective-infra block above
# populates (mirrors select_harness_kloc_guard's precedent paragraph just
# above), so it inherits for free: (a) merge/background suppression — the
# selective emission block (add_tool site below) is suppressed when
# DF_VERIFY_ROLE=merge|background, where run_all.sh already runs this exact
# file wholesale (exactly-once, INV-5); (b) the REIFY_INFRA_SUITE_ACTIVE
# re-entrancy guard; (c) fail-fast ordering before the cargo poles; (d)
# add_tool's LD_LIBRARY_PATH scrub, so no new plain-`add` call site appears
# for tests/infra/test_verify_ld_library_path_scope.sh to police. A plain path
# is a degenerate glob, so no emission-side change is needed at all.
#
# Measured cost: tests/infra/test_reify_audit_ptodo.sh is 22 assertions,
# 0m3.362s warm on 2026-08-28 (main checkout, target/release/{reify-audit,
# ptodo-baseline-gen} already built) — a single cheap leaf, not a cargo pole.
#
# Extension arm landed at reify-audit's FULL swept-extension set
# (crates/reify-audit/src/ptodo.rs::is_swept_ext) in one piece (task 6817
# step-4), not built up path-by-path: keying on the whole extension set
# rather than the .ri-only tests/prd-gate/fixtures/ path closes the
# same-class docs/**/*.ri and gui/**/*.{ts,tsx,js} holes in one rule, instead
# of enumerating decide_scope's no-heavy case arms one at a time. Kept honest
# by a derive-from-source drift guard, test_verify_scope.sh's PT-DRIFT
# scenario — see the case arm below for exactly what direction that guard
# covers.
#
# ACCEPTED RESIDUAL: REIFY_AUDIT_NO_COLD_BUILD is deliberately NOT set on this
# path (the merge tier sets it, paired with a pre-build and a positive
# existence assertion). If target/release/reify-audit is stale here,
# reify_audit_guard's rebuild-budget-safe path self-heals via `cargo build
# --release -q -p reify-audit` inside the 10m selective-infra wall — setting
# the knob would make the guard SKIP budget-safely instead, silently
# reopening exactly the hole this selector closes. A cold build that blows
# the wall fails the commit loudly, which is the correct direction of error
# for a gate protecting a main landing.
#
# A THIRD outcome is accepted too, not just the two above: if
# reify_audit_guard's rebuild attempt still leaves the binary judged stale
# (rc=125 — e.g. a cargo no-op fingerprint match against an on-disk mtime
# older than the last crates/reify-audit commit, such as a warm-lane target/
# with stamped mtimes) while REIFY_AUDIT_BIN stays executable,
# tests/infra/test_reify_audit_ptodo.sh sets RATCHET_SKIP=1 and skips exactly
# scenario (a)+(b) — the gen-driven fingerprint ratchet this selector exists
# to run — while still executing its (c)-(f) exit-code hard gate, which is
# High-severity-only. phantom-tracking is MEDIUM, so that hard gate does not
# catch it: this path can exit GREEN on a main landing without the ratchet
# having run at all. Left accepted rather than closed here because closing it
# needs a change to test_reify_audit_ptodo.sh, outside this task's scope
# (scripts/verify.sh + tests/infra/test_verify_scope.sh) — e.g. an opt-in
# REIFY_PTODO_RATCHET_REQUIRED that turns the rc=125-with-present-binary case
# into a hard failure instead of RATCHET_SKIP=1. Filed as follow-up work
# rather than done inline (task 6817 amendment pass).
# ---------------------------------------------------------------------------
select_cheap_ptodo_gate() {
    [ "$SCOPE" = "staged" ] || return 0
    [ -n "$CHANGED_FILES_RAW" ] || return 0
    local _f
    while IFS= read -r _f; do
        [ -n "$_f" ] || continue
        # ${_f,,} (bash case-fold) mirrors is_swept_ext's path.to_lowercase().
        # This extension list is a DERIVED COPY of is_swept_ext in
        # crates/reify-audit/src/ptodo.rs (cited by FUNCTION NAME, not line
        # number — a line cite rots on the next edit to that file). Its
        # source of truth is BEHAVIOURAL, not this comment:
        # tests/infra/test_verify_scope.sh's PT-DRIFT scenario re-derives the
        # set from is_swept_ext's source on every infra run and goes RED if
        # is_swept_ext GAINS an extension this list lacks. That check is
        # ONE-DIRECTIONAL: nothing asserts the reverse (this list still
        # carrying an extension is_swept_ext later drops), so the direction
        # of error on THAT side is over-selection (one extra ~3.4s leaf),
        # never a silent coverage hole. Note `*.ts` also matches `*.tsx`
        # under a bash glob; both are listed anyway so this reads as a
        # faithful mirror of the Rust function rather than a minimal set.
        case "${_f,,}" in
            *.rs|*.ri|*.sh|*.py|*.ts|*.tsx|*.js) : ;;
            *) continue ;;
        esac
        add_selected_infra_glob "tests/infra/test_reify_audit_ptodo.sh"
        return 0
    done <<< "$CHANGED_FILES_RAW"
}
select_cheap_ptodo_gate

# ---------------------------------------------------------------------------
# Phase-2 narrowing: map changed files → affected crate set → -p flag strings.
#
# Eligible when: (scope=branch OR (scope=staged AND --narrow)) AND RUN_RUST=1.
# scope=all is structurally unreachable for narrowing (C1 — returns early in
# decide_scope, leaving CHANGED_FILES_RAW="", and the condition is never true).
# --narrow is a no-op for scope=branch (already narrowing) and scope=all
# (condition never true).
#
# AFFECTED_CLOSURE — the raw reverse-dependency closure, COMPUTED whenever
# RUN_RUST=1 AND SCOPE != all, i.e. deliberately WIDER than narrowing
# eligibility above, so a consumer can make a membership test against the
# closure WITHOUT activating narrowing. Its one consumer, and the full rationale
# for reading SCOPE/AFFECTED_CLOSURE instead of inferring from NARROW_ACTIVE,
# live on the decision itself: see the "NARROWED on the same affected-crate
# axis" bullet in add_test_passes. Not restated here.
#
# Splitting COMPUTATION from ACTIVATION is value-preserving: AFFECTED,
# NARROW_ACTIVE and AFFECTED_ALL_FLAGS end up with the same values on every
# path, so the --workspace coupling that tests/infra/test_verify_scope.sh's
# B9-default scenario pins is untouched.
#
# COST — affected_crates() is invoked at most ONCE per run (hence hoisting it
# here rather than adding a second call site), and RUN_RUST=0 (a docs-only
# hook-gated commit) skips it entirely. The call shells out to an UNGUARDED
# `cargo metadata --format-version 1` in affected-crates-lib.sh's
# _reverse_closure: 0.53s warm, but on a cold cache or a stale Cargo.lock it
# performs dependency resolution, can reach the network, and can rewrite
# Cargo.lock — now on the pre-commit-hook tier and under --print-plan, neither
# of which previously shelled out to cargo on the staged path. Mitigated because
# a RUN_RUST=1 hook run goes on to invoke cargo clippy/check anyway. Passing
# --locked/--offline there is the real fix and is filed as follow-up; the
# existing `|| { echo ALL; return 0; }` already fails wide if it errors.
#
# REIFY_AFFECTED_CRATES_OVERRIDE — testability/operator knob (whitespace/newline-
# separated crate names). When set AND the closure is eligible, used verbatim in
# place of calling affected_crates(). This mirrors the REIFY_PSI_GATE_PROC_PATH
# knob idiom and allows hermetic --print-plan assertions in the workspace-less
# fixture (where cargo metadata fails and affected_crates() always returns ALL).
# ---------------------------------------------------------------------------
AFFECTED=""
AFFECTED_CLOSURE=""
NARROW_ACTIVE=0
AFFECTED_ALL_FLAGS=""

_narrowing_eligible=0
if [ "$SCOPE" = "branch" ] && [ "$RUN_RUST" -eq 1 ]; then
    _narrowing_eligible=1
elif [ "$SCOPE" = "staged" ] && [ "$NARROW" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
    _narrowing_eligible=1
fi

# Wider than _narrowing_eligible by design — see the AFFECTED_CLOSURE note in
# this block's header.
_closure_eligible=0
if [ "$RUN_RUST" -eq 1 ] && { [ "$SCOPE" = "branch" ] || [ "$SCOPE" = "staged" ]; }; then
    _closure_eligible=1
fi

if [ "$_closure_eligible" -eq 1 ]; then
    if [ -n "${REIFY_AFFECTED_CRATES_OVERRIDE:-}" ]; then
        # Operator/testability override: use verbatim crate list.
        AFFECTED_CLOSURE="${REIFY_AFFECTED_CRATES_OVERRIDE}"
    elif [ -n "$CHANGED_FILES_RAW" ]; then
        # Real run: compute reverse-closure from the captured changed-file list.
        _af_args=()
        while IFS= read -r _af_f; do
            [ -n "$_af_f" ] && _af_args+=("$_af_f")
        done <<< "$CHANGED_FILES_RAW"
        if [ "${#_af_args[@]}" -gt 0 ]; then
            AFFECTED_CLOSURE="$(affected_crates "${_af_args[@]}")"
        fi
    fi
fi

if [ "$_narrowing_eligible" -eq 1 ]; then
    # Narrowing eligibility ⊂ closure eligibility, so AFFECTED_CLOSURE is already
    # computed here — consumed, never recomputed (one affected_crates() call, one
    # `cargo metadata`, per run).
    AFFECTED="$AFFECTED_CLOSURE"
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

# add_tool — emit a NON-CARGO plan line with the OCCT loader path scrubbed back
# to the ambient apply_env() captured (task 5730; esc-4581-87 / task 5321).
#
# WHICH ONE DO I USE? If the line reaches cargo at all, use `add` — cargo needs
# the OCCT search dir, and losing it is a hard link/load failure across the
# whole gate. Everything else (shell scripts, npm, git, node) uses `add_tool`:
# /opt/reify-deps/lib is a whole conda prefix that shadows hundreds of system
# sonames and outranks DT_RUNPATH, so a tool inheriting it gets conda libraries
# substituted underneath it (see the SCOPE CONTRACT comment in apply_env()).
# The rule is deliberately conservative at the boundary: a MIXED shell+cargo
# line (the gui-sidecar compile-check below) stays on `add`.
#
# _LD_SCRUB is SINGLE-quoted so the variable NAME, not its value, lands in the
# plan: the restore then happens at EXECUTION time and --print-plan stays a
# hermetic, host-independent oracle with no absolute host path baked into plan
# text that tests and dark-factory compare.
#
# A leading `export ...;` STATEMENT is shape-agnostic — it composes ahead of
# `if`, `{`, `(` and `for` alike, so no plan line needs restructuring — and it
# does not leak forward, because reaper_run_in_pgroup normally runs each plan
# line in its own background subshell (`set -m; eval "$cmd" &`).
#
# That containment is NOT unconditional. Under the break-glass knob
# REIFY_PROC_REAPER_DISABLE=1, lib_proc_reaper.sh evaluates each plan line in
# the MAIN shell instead, so a head-of-line export leaks forward into every
# later line, cargo lines included. Expect degradation rather than a red gate:
# .cargo/config.toml's `runner` re-exports the OCCT path before exec and
# DT_RUNPATH is baked into every bin and test binary, so the Rust lines
# re-derive it downstream. The knob is unset by default and is documented as
# break-glass only (docs/notes/orphaned-test-binary-reaper.md); it is called
# out here so the invariant is not read as absolute. Wrapping the scrub in a
# subshell would close it, but the run_all.sh line's scrub must stay at the
# LITERAL head of the line to stay outside test_run_all_ambient_isolation.sh's
# `then`-anchored ledger window, so that trade is deliberately not taken.
#
# The executor also routes any plan line whose text CONTAINS the substring
# `_VERIFY_NODE_BG_PID` to a main-shell eval. That is two lines today — the
# backgrounded node lane and the `wait` that joins it. The node lane puts the
# export inside its own `{ ... ; } &` braces instead of using add_tool; the
# `wait` is a builtin needing no scrub. Never route a line matching that
# substring through add_tool.
#
# Placement also matters at the head of the line specifically: see the
# run_all.sh emission site for the ledger-guard constraint.
#
# Enforced by tests/infra/test_verify_ld_library_path_scope.sh, which
# ENUMERATES every plain-`add` call site in this file and requires each to
# carry a trailing `# ld-ok: <reason>` marker — so a NEW tool plan line added
# with plain `add` fails the guard even though the guard has never heard of
# its name. The marker is checked, never the payload text: an interim draft
# inferred neutrality from the text and was defeated by, among others, a
# pure-shell line merely NAMING cargo. (Before task 5730's
# review pass the guard was a hardcoded needle list, and this comment
# overclaimed: the INV-FEA-1 trampoline line added by task 5076 slipped
# through unscrubbed precisely because no needle named it.)
# `${VAR-}` (not a bare `$VAR`) so a plan line stays replayable OUTSIDE this
# script. Inside verify.sh the variable is always set — apply_env() runs
# unconditionally before any plan line executes — but --print-plan output is
# routinely pasted into a shell by hand and consumed by dark-factory, and under
# `set -u` a bare reference would abort with "unbound variable". The `-` form
# degrades to an empty loader path, which is the intended scrub anyway.
_LD_SCRUB='export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; '
add_tool() { PLAN+=("${_LD_SCRUB}$1"); }

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

# ---------------------------------------------------------------------------
# Delta-conditional release-pass skip (task 5279 / PRD docs/prds/merge-gate-riders.md
# task ε, rider 3). SWEEP-GATED, default-OFF. When REIFY_RELEASE_DELTA_SKIP=1 AND
# DF_VERIFY_ROLE=merge AND the merge delta is derivable-and-clean (its reverse-closed
# affected crate set is disjoint from the release-sensitive set), the ~17-min release
# nextest pass is replaced by a frozen marker in add_test_passes below, and release
# re-execution is deferred to the main-tip background sweep (contract C2 profile-axis
# carve-out; role=background re-runs the full release-sensitive set on cadence).
#
# _RELEASE_DELTA_SKIP defaults 0 => knob-off plan is byte-identical (K2 —
# test_occt_flock_gate.sh Tests 17/17b stay green). Fail-open/fail-wide ladder
# (§5.1): role=background NEVER skips (the sweep IS the backstop); an underivable
# delta with no override never consults the predicate => stays 0 => RUN; the ALL
# sentinel and an unreadable declared set resolve to REQUIRED inside the predicate.
# The decision runs once here at plan-build time, consistent with the narrowing
# block above (REIFY_AFFECTED_CRATES_OVERRIDE keeps --print-plan hermetic).

# _derive_merge_delta — print the merge delta's changed files (one per line) for
# the skip decision, or return non-zero (underivable => caller stays fail-open).
#   hook path (merge in progress): MERGE_HEAD present => git diff HEAD MERGE_HEAD
#   DF lane (speculative merge committed): HEAD has >=2 parents =>
#       git diff HEAD^1 HEAD  (first-parent diff = what the merge introduces)
#   anything else (unborn/linear/detached HEAD, any git failure) => non-zero.
# All git stderr suppressed; any failure is underivable (fail-open RUN).
_derive_merge_delta() {
    local _mh
    _mh="$(git -C "$REPO_ROOT" rev-parse --git-path MERGE_HEAD 2>/dev/null || echo '')"
    if [ -n "$_mh" ] && [ -f "$_mh" ]; then
        git -C "$REPO_ROOT" diff --name-only HEAD MERGE_HEAD 2>/dev/null || return 1
        return 0
    fi
    local _parents _arr
    _parents="$(git -C "$REPO_ROOT" rev-list --parents -n1 HEAD 2>/dev/null)" || return 1
    # rev-list --parents -n1 prints "<sha> <parent1> <parent2> ..."; >=3 tokens
    # means the commit plus at least two parents (a merge commit).
    # shellcheck disable=SC2206
    _arr=($_parents)
    if [ "${#_arr[@]}" -ge 3 ]; then
        git -C "$REPO_ROOT" diff --name-only HEAD^1 HEAD 2>/dev/null || return 1
        return 0
    fi
    return 1
}

_RELEASE_DELTA_SKIP=0
if [ "${REIFY_RELEASE_DELTA_SKIP:-0}" = "1" ] && [ "$DF_VERIFY_ROLE" = "merge" ]; then
    if [ -n "${REIFY_AFFECTED_CRATES_OVERRIDE:-}" ]; then
        # Hermetic/operator override: the predicate reads the affected set verbatim
        # (no git, no cargo). Short-circuits derivation, as the narrowing block does.
        release_delta_requires_pass || _RELEASE_DELTA_SKIP=1
    elif _delta_raw="$(_derive_merge_delta)"; then
        # Real merge: consult the predicate ONLY on a successful derivation.
        # Underivable never reaches here => _RELEASE_DELTA_SKIP stays 0 => RUN.
        _delta_files=()
        while IFS= read -r _df; do
            [ -n "$_df" ] && _delta_files+=("$_df")
        done <<< "$_delta_raw"
        if [ "${#_delta_files[@]}" -gt 0 ]; then
            release_delta_requires_pass "${_delta_files[@]}" || _RELEASE_DELTA_SKIP=1
        else
            # Empty derivable delta (merge changed nothing) => delta-clean => skip.
            release_delta_requires_pass || _RELEASE_DELTA_SKIP=1
        fi
    fi
fi

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
    # --test-threads=N (task 5264): test-execution parallelism cap wired by the
    # dark-factory offline deep-test lane. Empty when the flag is unset, so the
    # emitted command is byte-for-byte identical to today — the same
    # empty-or-leading-space idiom as the adjacent _GATE_HEAVY_EXCLUDE /
    # _OFFLINE_HEAVY_SELECT fragments.
    local _tt_flag=""
    [ -n "$TEST_THREADS" ] && _tt_flag=" --test-threads=${TEST_THREADS}"
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
        # retry_failed_only (task 5287): when the subset is eligible, build ONE
        # exact-match nextest filterset from the DF-written filter file at this
        # SINGLE construction site (INV-5). EXACT `test(=<id>)` ONLY — never the
        # substring form `test(<id>)` — so the subset can never silently pull in
        # an unintended test (PRD §6.3 / D3 soundness). File-sourced so the
        # expression is ARG_MAX-safe. Empty when inactive → byte-identical
        # default. Per-profile filter precedence and the loud absent/empty and
        # size-ceiling full-fallbacks are layered on by later steps.
        local _retry_filter_frag=""
        # Effective (local) copies of the heavy-filter fragments. Default to the
        # module-scope originals; when an eligible retry subset is folded into a
        # single `-E` below, these are cleared/stripped so the heavy filterset is
        # emitted exactly ONCE (inside the combined term) rather than as a second
        # `-E` that nextest 0.9.136 would UNION (OR) with the subset — which would
        # run (heavy-filter) OR (subset) = the whole (non-)heavy suite, silently
        # defeating the "retry only the did-not-pass tests" contract. When retry
        # is inactive/ineligible these stay byte-identical to the originals.
        local _eff_gate_exclude="$_GATE_HEAVY_EXCLUDE"
        local _eff_offline_select="$_OFFLINE_HEAVY_SELECT"
        if [ "$_RETRY_SUBSET_ELIGIBLE" -eq 1 ]; then
            # Profile derived from arg2 `rel` (" --release" → release, else
            # debug); emit_nextest_pass is called once per profile from
            # add_test_passes. Used for the per-profile loud fallback message
            # (and, in a later step, per-profile filter-file precedence).
            local _retry_profile="debug"
            case "$rel" in *release*) _retry_profile="release" ;; esac
            # Per-profile filter precedence with base fallback: the profile's
            # own REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG / _RELEASE var
            # overrides the base REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE when
            # non-empty, so a --profile both retry can carry a different subset
            # per profile (each pass resolves its own). When the per-profile var
            # is unset/empty it falls back to the base var; when all are unset the
            # resolved path is empty → the loud no-subset fallback below. The
            # absent/empty/unreadable and ceiling guards operate on this RESOLVED
            # path unchanged.
            local _retry_filter_file="${REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE:-}"
            if [ "$_retry_profile" = "release" ]; then
                _retry_filter_file="${REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE:-$_retry_filter_file}"
            else
                _retry_filter_file="${REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG:-$_retry_filter_file}"
            fi
            # Collect the non-blank exact test IDs so the size ceiling can be
            # applied BEFORE the expression is built.
            local -a _retry_ids=()
            local _line
            if [ -n "$_retry_filter_file" ] && [ -r "$_retry_filter_file" ]; then
                while IFS= read -r _line || [ -n "$_line" ]; do
                    # Trim leading/trailing whitespace, then skip a now-empty
                    # line. This drops blank AND whitespace-only lines (a stray
                    # "   " would otherwise become a malformed, unmatchable
                    # test(=   ) term) and normalizes any accidental surrounding
                    # whitespace on a real id. DF owns the file content so this
                    # is belt-and-suspenders; a well-formed id (no surrounding
                    # whitespace) is unchanged, keeping the fragment identical.
                    _line="${_line#"${_line%%[![:space:]]*}"}"
                    _line="${_line%"${_line##*[![:space:]]}"}"
                    [ -n "$_line" ] || continue
                    _retry_ids+=("$_line")
                done < "$_retry_filter_file"
            fi
            local _retry_n="${#_retry_ids[@]}"
            if [ "$_retry_n" -eq 0 ]; then
                # LOUD no-subset full-fallback (PRD §4.3): eligible, but the
                # resolved filter file is absent / unreadable / empty (no
                # non-blank lines) — run THIS profile FULL and say so. Per-profile
                # (each profile resolves its own file), so emitted here rather
                # than in the add_test_passes precompute. Guarded inside the
                # eligible (⇒ scope=failed_only) branch → default byte-identical.
                echo "verify.sh: retry refused: no subset — filter file ${_retry_filter_file:-<unset>} absent/empty/unreadable (profile=${_retry_profile}, full verify)" >&2
            elif [ "$_retry_n" -gt "$_RETRY_MAX_SUBSET" ]; then
                # LOUD subset-too-large full-fallback (INV-4 storm escape): a
                # subset ≈ the whole suite means DF built a bad subset, so run
                # THIS profile FULL rather than dress a full run up as a subset.
                echo "verify.sh: retry refused: subset too large — ${_retry_n} > ceiling ${_RETRY_MAX_SUBSET} (profile=${_retry_profile}, full verify)" >&2
            else
                # δ honest marker (task 5290): record the APPLIED subset size at
                # this SINGLE per-profile construction site (INV-5 no
                # re-derivation) — the count of exact ids that actually narrowed
                # THIS pass. Every fallback above (no subset / too large) returns
                # without reaching here, so the recorded count stays 0 for a
                # profile that ran FULL, keeping the end-of-build_plan marker honest.
                if [ "$_retry_profile" = "release" ]; then
                    _RETRY_NEXTEST_RELEASE_APPLIED="$_retry_n"
                else
                    _RETRY_NEXTEST_DEBUG_APPLIED="$_retry_n"
                fi
                # Build ONE exact-match filterset from the collected IDs.
                local _retry_expr="" _id
                for _id in "${_retry_ids[@]}"; do
                    if [ -z "$_retry_expr" ]; then
                        _retry_expr="test(=${_id})"
                    else
                        _retry_expr="${_retry_expr} | test(=${_id})"
                    fi
                done
                # Compose the subset with any ACTIVE heavy filterset into a
                # SINGLE `-E` term via the intersection operator ` & ` (valid
                # filterset DSL — REIFY_HEAVY_NEXTEST_FILTER itself uses
                # `package(..) & binary(..)`). nextest 0.9.136 UNIONs multiple
                # `-E` expressions, so a second `-E` here would WIDEN, not
                # narrow (the union bug). Derive the active gate expr and
                # SUPPRESS its separate fragment (via the _eff_* copies):
                #   - gate-exclude (task/merge + REIFY_GATE_EXCLUDE_HEAVY=1):
                #     `not (<heavy>)`  → clear _eff_gate_exclude entirely.
                #   - offline positive select: `(<heavy>)` → strip the
                #     `-E "(…)"` filterset but PRESERVE the trailing
                #     non-filterset flag(s) (` --run-ignored all`).
                # Semantically sound: under heavy-exclude attempt-0 never ran a
                # heavy test, so no did-not-pass id can be heavy — intersecting
                # with `not (heavy)` drops nothing attempt-0 ran (and
                # conservatively excludes any stray heavy id). Single-quote
                # wrapping is eval-safe (the heavy value and the DF exact ids
                # are single-quote-free). When NO gate expr is active the
                # retry-only single-quoted form is unchanged (byte-identical).
                local _gate_expr=""
                if [ -n "$_eff_gate_exclude" ]; then
                    _gate_expr="not (${REIFY_HEAVY_NEXTEST_FILTER})"
                    _eff_gate_exclude=""
                elif [ -n "$_eff_offline_select" ]; then
                    _gate_expr="(${REIFY_HEAVY_NEXTEST_FILTER})"
                    _eff_offline_select="${_eff_offline_select##* -E \"*\"}"
                fi
                if [ -n "$_gate_expr" ]; then
                    _retry_filter_frag=" -E '(${_gate_expr}) & (${_retry_expr})'"
                else
                    _retry_filter_frag=" -E '${_retry_expr}'"
                fi
            fi
        fi
        cmd="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo nextest run ${selector}${rel}${_eff_gate_exclude}${_eff_offline_select}${_tt_flag}${_retry_filter_frag} --config-file ${_cfg_path}"
    else
        # LOUD no-nextest full-fallback (never-silent invariant, PRD §4.3): the
        # cargo-test fallback plan has no `-E` filterset support, so an eligible
        # retry subset cannot be applied and this profile runs FULL. Say so —
        # every other retry refusal (tree drift / no subset / subset too large)
        # is loud, so this path must not be the one silent exception. Gated on
        # _RETRY_SUBSET_ELIGIBLE (a subset WOULD have applied on the nextest
        # path), so a tree-drift/ineligible retry — already announced once in
        # add_test_passes — is not re-diagnosed here. In practice unreachable on
        # a merge host: the NEXTEST probe above hard-fails rather than fall back
        # while cargo-nextest is installed, so NEXTEST=0 means it is genuinely
        # absent from PATH (no retry consumer runs there).
        if [ "$_RETRY_SUBSET_ELIGIBLE" -eq 1 ]; then
            echo "verify.sh: retry refused: no nextest — cargo-test fallback has no -E filterset support (full verify)" >&2
        fi
        # Fallback (no nextest): the nextest occt test-group that serializes
        # OCCT's thread-unsafe tests is unavailable here, so the ONLY OCCT guard
        # is process-wide single-threading. When --test-threads is UNSET we
        # therefore default to 1 (${TEST_THREADS:-1}), preserving the historical
        # whole-workspace serial guard byte-for-byte. When the caller passes an
        # explicit --test-threads=N>1 it is honored verbatim, which BYPASSES the
        # OCCT serialization the nextest path would provide: on a nextest-less
        # host the caller must then guarantee no OCCT tests run concurrently.
        # (The offline deep-test lane — the sole N-setting consumer — runs with
        # nextest present and N=1, so this footgun is not reachable in practice.)
        cmd="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo test ${selector}${rel} -- --test-threads=${TEST_THREADS:-1}"
    fi
    # FD 9 is the held semaphore slot; close it for each gated child so daemon
    # processes (sccache/rustc) cannot inadvertently inherit the lock fd and
    # wedge the slot after the test pass exits (2026-04-20 wedge class).
    # Harmless no-op on the merge-exempt path.
    add "$cmd 9<&-"  # ld-ok: cargo — $cmd is the built nextest/cargo test command; needs OCCT
}

add_test_passes() {
    # retry_failed_only (task 5287): attempt-0 sidecar stamp. On a FULL merge
    # gate (DF_VERIFY_ROLE=merge AND NOT a failed_only retry), record the tree
    # target/ was BUILT FROM, so a later retry can tree-pin its narrowed
    # subset against the warm _merge-verify target/. Emitted as the FIRST
    # plan line of add_test_passes — after build_plan's lint/compile wave, the
    # gui block, and the merge pre-build/run_all.sh poles, but BEFORE every
    # test pole (psi-gate, compile-gate, every nextest pass) — so the pin
    # survives a RED psi-gate/compile-gate/nextest pole too, not only a fully
    # green attempt-0 (task 5548, PRD verify-retry-failed-only α2). It does NOT
    # survive a red clippy/gui/run_all.sh pole: those precede add_test_passes,
    # so the plan executor's first-failure exit means such a red attempt-0
    # never reaches this line either — stamped once attempt-0 reaches the test
    # phase, not on every possible red. The old tail position (after
    # @@SEMAPHORE_RELEASE@@) was unreachable on any retry-eligible run: the
    # plan executor exits on the first failing command, and a narrowed retry
    # by definition follows a RED attempt-0. target/ is already built from
    # THIS tree by this point — the check/clippy wave and the `cargo build
    # --release -p reify-audit` pre-build precede add_test_passes, and a warm
    # lane pre-seeds target/ — so `test -d target` is satisfiable and the
    # built-from-this-tree claim holds even here. Schema is PRD §4.1's
    # {tree_oid, profiles, timestamp} VERBATIM — DF's D2b pins its unit fixtures
    # to these exact bytes, so no field may be renamed here without moving the
    # PRD's §4.1/§244 schema rows in the SAME change. tree_oid (git rev-parse
    # HEAD: = the TREE OID) and timestamp are computed at RUN time; `profiles`
    # is the build-time-baked ${PROFILES[*]} — the profiles this attempt PLANNED
    # to run, NOT proof any of them executed or passed (the eligibility guard
    # reads tree_oid only), so a red attempt-0 can name a profile whose nextest
    # pass never ran. The consumer-side obligation that follows from that
    # (never narrow a profile that never ran) is DF's, documented at the
    # …_FILTER_FILE_DEBUG/_RELEASE header entry. Guarded on `test -d target` and
    # tolerant of git failure (|| echo unknown) and write failure (|| true),
    # mirroring the target/.reify-bin-sha stamp (task 5133). Survives a warm-lane
    # reseed (under target/, git clean -xfd -e target). A retry (scope=failed_only)
    # deliberately does NOT re-stamp — DF re-runs a full attempt-0 for a new tree.
    if [ "$DF_VERIFY_ROLE" = "merge" ] && [ "${REIFY_VERIFY_RETRY_SCOPE:-}" != "failed_only" ]; then
        add_tool "if test -d target; then printf '{\"tree_oid\":\"%s\",\"profiles\":\"%s\",\"timestamp\":\"%s\"}\\n' \"\$(git rev-parse HEAD: 2>/dev/null || echo unknown)\" \"${PROFILES[*]}\" \"\$(date -u +%Y-%m-%dT%H:%M:%SZ)\" > \"${_ATTEMPT_SIDECAR_PATH}\" 2>/dev/null || true; fi"
    fi

    # PSI gate: must pass before any cargo test work starts.
    # In execute mode: eval runs this as a subprocess that inherits DF_VERIFY_ROLE
    # and REIFY_PSI_GATE_*. At the default REIFY_PSI_GATE_MAX_WAIT=unlimited the gate
    # HOLDS (clock-stopped, heartbeating) and never exits 75. Under an explicitly
    # FINITE MAX_WAIT it exits 75 and that propagates — which dark-factory does NOT
    # requeue: verify.py _classify_failure routes exit-75 to unknown_test_failure →
    # debugfix → BLOCKED (PRD verify-admission-wait-clock-stop §2).
    # In --print-plan mode: printed faithfully as a normal plan line.
    add_tool "./scripts/verify.sh psi-gate"

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
    add_tool "./scripts/verify.sh compile-gate"

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
    add "@@SEMAPHORE_ACQUIRE@@"  # ld-ok: sentinel — executor intercepts @@SEMAPHORE_*@@ by case, never eval'd

    # Emit one combined build+execution nextest pass per profile (slot held).
    # Outer timeout: PER-PROFILE budgets (task 5382 restored the pre-4520 split).
    #  - DEBUG (--workspace): _VERIFY_TEST_TIMEOUT (60m). Re-derived from η/4521's
    #    real-load floor (task 4520/ζ′): 798.9 s worst-observed cold real-load on a
    #    quiet box with WARM host sccache (docs/prds/jobserver-merge-priority-balancer
    #    .acceptance-report.md §"ζ′/4520 budget floor (authoritative)");
    #    ceil(798.9 × 4.5 production-weighted margin) = ceil(3595.05 s) = 3596 s →
    #    rounded up to 60m (3600 s). Bound 3600 s > floor 798.9 s by construction.
    #  - RELEASE (--release): _VERIFY_TEST_TIMEOUT_RELEASE (90m). The 798.9 s floor
    #    was measured cold-TARGET / WARM-sccache; it NEVER included a cold-SCCACHE
    #    native-kernel relink (OCCT/OpenVDB/gmsh/manifold — a SEPARATE sccache profile
    #    from debug, so a cleared debug pass does not warm the release compile), which
    #    pushed the combined release build+exec past the unified 60m and SIGTERM'd it
    #    at the inner timeout (esc-5370-2, false "integration_skew"). 90m is the
    #    cold-aware release budget. The merge OUTER wall that must not preempt these
    #    two inner budgets is merge_verify_cold_command_timeout_secs (sibling task
    #    5383) in dark-factory-orchestrator.yaml. That key's comment block is the
    #    SINGLE SOURCE for the inner/outer relationship — what it does and does NOT
    #    model, and why the inner ceilings are not summed against it. Do not restate
    #    its arithmetic here; it drifted once already (esc-5382-1 amendment review).
    #    tests/infra/test_occt_flock_gate.sh T10 mechanises that relationship.
    # NOTE: outer timeouts asserted in tests/infra/test_occt_flock_gate.sh
    # (Test 17 — debug pass, Test 17b — release pass; T1/T2/T8/T9 knob behavior) — keep in sync.
    local _profile _rel
    local _outer_timeout="${_VERIFY_TEST_TIMEOUT}"             # debug (--workspace) budget
    local _rel_outer_timeout="${_VERIFY_TEST_TIMEOUT_RELEASE}" # release (--release) budget

    # retry_failed_only (task 5287) — precompute subset eligibility ONCE, before
    # the profile loop, so emit_nextest_pass just consumes the decision.
    #   _RETRY_SUBSET_ELIGIBLE — the caller asked for scope=failed_only AND the
    #     on-disk attempt-0 sidecar's tree_oid equals the (non-empty) tree DF
    #     intends to retry (REIFY_VERIFY_RETRY_TREE_OID). Equality proves the
    #     warm target/ corresponds to the retried tree (PRD §5 INV-1 tree-pin
    #     soundness); a rebase makes DF pass a new OID that mismatches the
    #     surviving sidecar → fallback (DF also full-verifies on rebase
    #     independently, M4). A mismatched/absent sidecar leaves it 0 → all
    #     profiles run FULL (the loud "tree drift" diagnostic is emitted below).
    _RETRY_SUBSET_ELIGIBLE=0
    if [ "${REIFY_VERIFY_RETRY_SCOPE:-}" = "failed_only" ]; then
        local _sidecar_tree_oid=""
        if [ -f "$_ATTEMPT_SIDECAR_PATH" ]; then
            # Tolerant extractor: pull the "tree_oid":"<x>" value without a JSON
            # parser (the sidecar is verify.sh's own single-line stamp).
            _sidecar_tree_oid="$(sed -n 's/.*"tree_oid"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$_ATTEMPT_SIDECAR_PATH" 2>/dev/null | head -n1)"
        fi
        local _want_tree_oid="${REIFY_VERIFY_RETRY_TREE_OID:-}"
        if [ -n "$_want_tree_oid" ] && [ -n "$_sidecar_tree_oid" ] && [ "$_sidecar_tree_oid" = "$_want_tree_oid" ]; then
            _RETRY_SUBSET_ELIGIBLE=1
        else
            # LOUD tree-drift full-fallback (PRD §4.3 / INV-4 storm escape): the
            # warm target/ cannot be proven to correspond to the tree DF wants
            # to retry (sidecar absent, or its tree_oid != the wanted OID), so
            # run FULL and say so — never silently. Emitted ONCE here (the
            # decision is profile-independent), mirroring the build-time
            # MERGE_HEAD / scope-branch `echo … >&2` diagnostics. Guarded inside
            # the scope=failed_only branch so the default plan stays byte-identical.
            echo "verify.sh: retry refused: tree drift — sidecar tree_oid ${_sidecar_tree_oid:-<absent>} != REIFY_VERIFY_RETRY_TREE_OID ${_want_tree_oid:-<unset>} (full verify)" >&2
        fi
    fi

    for _profile in "${PROFILES[@]}"; do
        if [ "$_profile" = "release" ]; then
            # Delta-conditional release-pass skip (task 5279 / merge-gate-riders ε
            # rider 3): on a delta-clean merge (_RELEASE_DELTA_SKIP=1, decided once at
            # plan-build time above) emit the FROZEN machine-greppable marker in place
            # of the release nextest pass and skip the pass — release re-execution is
            # deferred to the main-tip background sweep (contract C2 profile-axis
            # carve-out). The marker is a K5 frozen constant consumed by DF
            # classification / runs.db mining / activation leaf ζ (task 5280); do NOT
            # reword it. Default-off => this branch is never taken => plan byte-identical.
            if [ "${_RELEASE_DELTA_SKIP:-0}" -eq 1 ]; then
                add "echo 'RELEASE-PASS: skipped (delta-clean)'"  # ld-ok: builtin — echo only, no external binary to shadow
                continue
            fi
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
            # scoping axes — do not "fix" this by narrowing the release pass along the
            # affected-crate (crate-set) axis.
            # The ONE sanctioned exception is orthogonal to that crate-set prohibition:
            # the delta-conditional PROFILE-axis carve-out handled just above (task 5279
            # / merge-gate-riders ε rider 3). When _RELEASE_DELTA_SKIP=1 the whole
            # release pass is skipped for a delta-clean merge and 'RELEASE-PASS: skipped
            # (delta-clean)' is emitted instead, deferring release re-execution to the
            # main-tip background sweep — see the _RELEASE_DELTA_SKIP decision block and
            # docs/prds/verify-scope-contract.md C2. That carve-out is knob-gated
            # (REIFY_RELEASE_DELTA_SKIP, default OFF) and only activated once the
            # background sweep is demonstrably healthy (ζ / task 5280, merge-gate-riders
            # §5.3); it is a whole-pass profile-axis skip, NOT crate-set narrowing.
            # offline (task 4913/A2): the positive -E "(<heavy>)" filter (applied via
            # _OFFLINE_HEAVY_SELECT inside emit_nextest_pass) is the SOLE membership
            # determinant for the offline lane — use --workspace instead of the
            # release-sensitive -p set so offline's heavy coverage never silently
            # narrows if a heavy crate is ever dropped from release-sensitive-crates.txt.
            if [ "$DF_VERIFY_ROLE" = "offline" ]; then
                emit_nextest_pass "--workspace" "$_rel" "$_rel_outer_timeout"
            else
                emit_nextest_pass "$_RELEASE_ALL_FLAGS" "$_rel" "$_rel_outer_timeout"
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

    # gui-feature TEST-EXECUTION pass (task 5076; PRD docs/prds/compute-fea-hardening.md
    # task A5).  reify-gui's #[cfg(feature = "gui")] code — gui_feature_tests in
    # tests/engine_tests.rs, gui_tests in kernel_status_tests.rs, the gui-gated #[test]
    # fns in event_bus_tests.rs / claude_bridge.rs, and the wholly gui-gated
    # debug_server / event_bus modules — is reached by NO workspace pass above (all run
    # without --features gui).  Until this pass existed it was only COMPILE-checked, by
    # the `cargo check -p reify-gui --features gui --tests` line in build_plan, so a
    # change that flipped engine.rs's cfg(feature="gui") arm to
    # MorphRegistration::Unavailable compiled clean and passed every CI pass silently.
    #
    # PLACEMENT — inside the semaphore bracket, at the tail of the profile loop:
    #   * Inside the slot because a --features gui build links tauri + webkit2gtk +
    #     OCCT, i.e. exactly the RSS-heavy link wave the held slot exists to bound
    #     (see the ACQUIRE-block comment above: memory is the binding constraint on
    #     this host and whole-block serialization is its only implicit bound).
    #   * OUTSIDE the profile loop so `--profile both` emits it exactly ONCE.
    #     --features is a FEATURE axis, not a profile axis; a second release-profile
    #     copy would cost another cold link for zero added coverage.
    #   * Skipped for DF_VERIFY_ROLE=offline, whose plan runs the heavy #[ignore]
    #     partition only.
    #   * NARROWED on the same affected-crate axis every other narrowed pass uses.
    #     THREE EXPLICIT ARMS, read off SCOPE and AFFECTED_CLOSURE directly:
    #       1. SCOPE=all            -> emit.  The merge gate never narrows; that is
    #                                  a CONTRACT, not a side effect.
    #       2. closure unavailable  -> emit.  FAIL WIDE.  "Unavailable" covers the
    #                                  ALL sentinel (a C4 workspace-global file, a
    #                                  C5 cargo-metadata failure, an unmappable
    #                                  path), an empty CHANGED_FILES_RAW, and a
    #                                  malformed REIFY_AFFECTED_CRATES_OVERRIDE.
    #                                  The empty case is genuinely OVERLOADED —
    #                                  decide_scope's git-failure fail-wide paths
    #                                  also return RUN_RUST=1 with
    #                                  CHANGED_FILES_RAW="" — so conflating it
    #                                  with "provably no crates" is CORRECT here
    #                                  and cannot be tightened without a separate
    #                                  closure-available sentinel.
    #       3. otherwise            -> emit iff reify-gui ∈ AFFECTED_CLOSURE.
    #     This is NOT keyed on NARROW_ACTIVE.  NARROW_ACTIVE is a narrowing-
    #     ACTIVATION flag, not a scope oracle: NARROW_ACTIVE=0 ALSO holds for
    #     `--scope staged` without `--narrow` and for the two fail-wide resets, so
    #     the old "emitted whenever NARROW_ACTIVE=0 (scope=all, i.e. the merge
    #     gate)" reading was a false equivalence.  Its cost was concrete and
    #     measured: `hooks/project-checks` execs
    #     `verify.sh all --profile debug --scope staged --include-infra`, and a
    #     staged crates/reify-doc/src/lib.rs emitted 1 gui-feature pass — the
    #     per-commit hook paying the full `--features gui` link for a closure that
    #     cannot reach reify-gui.  Arm 3 now covers that shape too.
    #     WHAT ACTUALLY BENEFITS is narrower than "every hook run", and the
    #     difference is measured, not assumed: only a staged diff whose paths ALL
    #     map to crates AND whose reverse closure excludes reify-gui takes arm 3.
    #     A scripts-only staged diff yields the ALL sentinel (C5/unmappable) and a
    #     tests/infra-only one yields an EMPTY closure (affected-crates-lib treats
    #     tests/infra/* as non-crate, while decide_scope's conservative arm still
    #     sets RUN_RUST=1) — both take arm 2 and keep paying the link.
    #     Arm 1 keeps the merge gate unconditional, so a hook-tier miss can only
    #     ever be LATENCY, never a coverage hole.
    #     affected_crates() is a REVERSE-dependency closure, so a change anywhere in
    #     reify-gui's dependency graph puts reify-gui in the set — measured on this
    #     tree: crates/reify-eval/src/lib.rs and crates/reify-mesh-morph/src/lib.rs
    #     both yield sets containing reify-gui; crates/reify-doc/src/lib.rs does not.
    #     Emitting unconditionally made a `-p reify-doc` branch plan pay a full
    #     tauri + webkit2gtk + OCCT feature-unification build (20m42s cold / ~137s
    #     warm, and NOT shareable with any other pass's artifacts) that no reify-doc
    #     change can regress — while that same plan already narrowed reify-gui's
    #     UNGATED tests away, so running its gui-GATED ones would have been
    #     incoherent.  Membership is the reverse closure rather than a hand-listed
    #     trigger set (reify-gui/reify-eval/reify-mesh-morph) so a change to an
    #     indirect dependency (reify-syntax, reify-ir, …) cannot silently fall out
    #     of the trigger.
    #
    # NOT reusing emit_nextest_pass — nor extending it with an optional extra-flags
    # parameter, which was considered and rejected.  Three structural mismatches, not
    # one missing slot: (a) this pass is wrapped in an `if test -f …; then … fi` guard
    # with a sidecar-placeholder prefix, so it needs a prefix AND a suffix hook, not a
    # trailing-flags one; (b) emit_nextest_pass unconditionally interpolates
    # ${_eff_gate_exclude}${_eff_offline_select}${_retry_filter_frag}, each of which
    # can emit an `-E` filterset — and an `-E` here would narrow away the very
    # gui-gated tests this pass exists to run (asserted by (b8) in
    # tests/infra/test_compute_trampoline_registration_wired.sh), so they would all
    # have to be suppressed for this caller; (c) ~30 plan-shape tests assert against
    # that single construction site.  The two arms below therefore mirror its
    # nextest/cargo-test if/else by hand so this pass is shape-identical on a
    # nextest-less host.  What IS shared with it is kept in lockstep deliberately and
    # is individually asserted: the memoized _NEXTEST_CONFIG_FILE + its lazy init,
    # the --test-threads fragment, and the trailing ` 9<&-`.
    #
    # Three non-obvious requirements, each pinned by a live guard:
    #  (i)  trailing ` 9<&-` — FD 9 is the held slot; tests/infra/test_verify_semaphore_
    #       wiring.sh (1k) fails when ANY cargo test/nextest line lacks it.  This is a
    #       direct `add`, so the token is NOT inherited from emit_nextest_pass's single
    #       append site and is appended explicitly (valid shell: a redirection on a
    #       compound command, kept on the same plan line the grep inspects).
    #  (ii) --config-file — scripts/gen-nextest-config.sh's header records that plain
    #       `--config` is a NO-OP for nextest test-groups on 0.9.136, so --config-file is
    #       the only mechanism that applies the occt test-group cap, the global
    #       [profile.default] test-threads pool cap and the per-binary priority
    #       overrides.  Without it this pass would run UNCAPPED inside the held slot.
    #       The SAME memoized _NEXTEST_CONFIG_FILE is reused (one temp file, one
    #       _verify_cleanup EXIT removal — generating a second would leak it), with
    #       emit_nextest_pass's lazy-init guard repeated so the pass cannot silently
    #       degrade to an uncapped run in a scope that emitted no workspace pass.
    #       --print-plan keeps the hermetic placeholder for the same reason it does
    #       there (no subprocess, no temp file).
    #  (iii) NO `-E` filterset.  Running the whole -p reify-gui --features gui suite is
    #       what makes ALL gui-gated code execute; an enumerated name filter would
    #       silently drift away from that set as modules are added.
    # Outer timeout: its own _VERIFY_GUI_FEATURE_TEST_TIMEOUT knob (45m default) —
    # see the REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT header entry for the measurement.
    # ensure-gui-sidecar-placeholder.sh runs first for the same reason it does at the
    # build_plan compile-check: tauri_build::build() validates bundle.externalBin and
    # panics when gui/src-tauri/sidecar/reify-sidecar-<triple> is absent from disk.
    local _emit_gui_feature_pass=0 _gui_ac
    # Normalize the closure ONCE, into a word ARRAY, so that arm 2 below sees
    # every malformed-knob shape as "unavailable" rather than as a crate list.
    # A malformed REIFY_AFFECTED_CRATES_OVERRIDE must fail WIDE, never narrow —
    # the same invariant, for the same reason, as the AFFECTED_ALL_FLAGS-empty
    # reset in the Phase-2 narrowing block.  Three shapes, each measured to
    # narrow the pass AWAY before this normalization existed:
    #   * whitespace-only ("   ") — non-empty and not the ALL sentinel, so it
    #     reached the membership loop, split to NOTHING, and never ran the loop
    #     body.  Closed by the EMPTY-array test.
    #   * glob-bearing ("*") — the split is an UNQUOTED expansion, so with
    #     pathname expansion live it expanded against the CWD into directory
    #     entries ("crates scripts") instead of collapsing; measured on a
    #     hermetic fixture, `*` on --scope staged printed `closure=*` and
    #     emitted ZERO gui-feature passes.  Closed by `set -f` ACROSS the split
    #     (the caller's -f state is saved and restored — verify.sh does not
    #     otherwise run with noglob).
    #   * any token outside cargo's package-name grammar — a surviving glob
    #     metacharacter, a path fragment, a stray flag.  A real affected_crates()
    #     closure only ever holds crate names, so a token that cannot BE one
    #     means the knob is malformed, not that the closure excludes reify-gui.
    #     Closed by the grammar check, which routes to arm 2 rather than arm 3.
    local -a _gui_closure_words=()
    local _gui_closure_malformed=0 _gui_noglob_was=1
    case "$-" in *f*) ;; *) _gui_noglob_was=0 ;; esac
    set -f
    # shellcheck disable=SC2086
    for _gui_ac in $AFFECTED_CLOSURE; do
        _gui_closure_words+=("$_gui_ac")
        case "$_gui_ac" in
            *[!A-Za-z0-9_.-]*) _gui_closure_malformed=1 ;;
        esac
    done
    if [ "$_gui_noglob_was" -eq 0 ]; then set +f; fi
    if [ "$DF_VERIFY_ROLE" != "offline" ]; then
        if [ "$SCOPE" = "all" ]; then
            # Merge gate: unconditional BY CONTRACT, not as a side effect of
            # narrowing happening to be inactive.
            _emit_gui_feature_pass=1
        elif [ "${#_gui_closure_words[@]}" -eq 0 ] \
            || [ "$_gui_closure_malformed" -eq 1 ] \
            || { [ "${#_gui_closure_words[@]}" -eq 1 ] && [ "${_gui_closure_words[0]}" = "ALL" ]; }; then
            # Closure unavailable — the ALL sentinel from a C4 global file / C5
            # metadata failure / unmappable path, an empty CHANGED_FILES_RAW, or
            # a malformed knob (see the normalization above).  Fail WIDE.
            _emit_gui_feature_pass=1
        else
            for _gui_ac in "${_gui_closure_words[@]}"; do
                if [ "$_gui_ac" = "reify-gui" ]; then
                    _emit_gui_feature_pass=1
                    break
                fi
            done
        fi
    fi
    if [ "$_emit_gui_feature_pass" -eq 1 ]; then
        local _gui_feat_cmd
        # --test-threads=N (task 5264) applies here for the SAME reason it applies
        # to every other test pass: an explicit --test-threads caps test-execution
        # parallelism, and this pass — a tauri + webkit2gtk + OCCT link inside the
        # held slot — is the last one that should run uncapped while the workspace
        # passes are capped.  Empty when the flag is unset, so the emitted line is
        # byte-for-byte unchanged by default (the same empty-or-leading-space idiom
        # emit_nextest_pass uses for _tt_flag).  The cargo-test arm below already
        # honoured ${TEST_THREADS:-1}; the two arms are now consistent.
        local _gui_tt_flag=""
        [ -n "$TEST_THREADS" ] && _gui_tt_flag=" --test-threads=${TEST_THREADS}"
        if [ "$NEXTEST" -eq 1 ]; then
            local _gui_cfg_path
            if [ "$PRINT_PLAN" -eq 1 ]; then
                _gui_cfg_path="${TMPDIR:-/tmp}/reify-nextest-occt.<print-plan-placeholder>"
            else
                if [ -z "$_NEXTEST_CONFIG_FILE" ]; then
                    _NEXTEST_CONFIG_FILE="$("$SCRIPT_DIR/gen-nextest-config.sh")"
                fi
                _gui_cfg_path="$_NEXTEST_CONFIG_FILE"
            fi
            _gui_feat_cmd="${CARGO_PRIO}cargo nextest run -p reify-gui --features gui${_gui_tt_flag} --config-file ${_gui_cfg_path}"
        else
            _gui_feat_cmd="${CARGO_PRIO}cargo test -p reify-gui --features gui -- --test-threads=${TEST_THREADS:-1}"
        fi
        add "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 ${_VERIFY_GUI_FEATURE_TEST_TIMEOUT} ${_gui_feat_cmd}; fi 9<&-"  # ld-ok: cargo — MIXED shell+cargo (gui-feature nextest pass, task 5076); needs OCCT
    fi

    # Release the semaphore slot after all passes complete.
    # The executor calls test_semaphore_release; the printer emits a comment.
    # The slot is also freed automatically on any verify.sh exit (FD 9 closes),
    # so the failure path needs no explicit release sentinel.
    add "@@SEMAPHORE_RELEASE@@"  # ld-ok: sentinel — executor intercepts @@SEMAPHORE_*@@ by case, never eval'd
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
        add_tool "./scripts/check-infra-classification-manifest.sh"
    fi

    # harness-layout baseline-registration drift gate (task 5300): fail fast —
    # naming the offending crate/file — when THIS diff ADDS a standalone
    # crates/<c>/tests/<f>.rs to one of the 5 consolidatable crates WITHOUT a
    # matching harness-layout-baseline.manifest row (the task 4370 drift). Unlike
    # test_harness_kloc_cap.sh's whole-tree live scan (which re-fires on every
    # innocent downstream rebaser once such drift is on main — the 5260/5266/5288
    # thrash), this gate is DIFF-SCOPED (--from-git derives the added-file set),
    # so it fires only on the offending diff and leaves rebasers green. Cheap
    # (pure bash + git, no cargo) and fail-open on any underivable base, so it
    # sits among the early fail-fast poles, before check-manifold-deps.sh /
    # psi-gate / run_all.sh. RUN_RUST=1 keeps docs-only / gui-src-only plans at
    # zero command leaves.
    if [ "$RUN_RUST" -eq 1 ]; then
        add_tool "./scripts/check-harness-baseline-registration.sh --from-git"
    fi

    # manifold prebuilt guard: fail fast (with a clear "run the deps script"
    # message) if the prebuilt manifold libs that .cargo/config.toml's
    # [target.*.manifold] override links are missing or version-drifted —
    # before any multi-minute compile turns that into a cryptic linker error.
    if [ "$RUN_RUST" -eq 1 ]; then
        add_tool "./scripts/check-manifold-deps.sh"
    fi

    # tree-sitter parser regeneration is a Rust-build prerequisite.
    if [ "$RUN_RUST" -eq 1 ]; then
        add_tool "./scripts/tree-sitter-generate.sh"
    fi

    # tree-sitter COMPILED-parser freshness (task #5629, esc-5392-1). The leaf
    # above refreshes tree-sitter-reify/src/ ON DISK but does not by itself make
    # cargo recompile it: cargo re-runs a build script only for paths declared
    # via rerun-if-changed, and cc emits none of its own. So without this leaf
    # the gate can link a libtree_sitter_reify.a built from different bytes than
    # the tree it is verifying — a false GREEN for an external-scanner change.
    # `ensure` repairs (bumps the watched inputs' mtime so cargo must rebuild)
    # rather than hard-failing; `check` is the assert-only mode for a checkpoint.
    #
    # Placement is load-bearing, both halves:
    #   AFTER tree-sitter-generate.sh  — src/parser.c must be current on disk
    #     before it is fingerprinted, or the verdict describes a stale input set.
    #   BEFORE `verify.sh compile-gate` and every cargo leaf — a force applied
    #     after the compile repairs nothing.
    # Guarded on RUN_RUST exactly as the generate leaf is, so docs-only /
    # gui-src-only plans keep zero command leaves.
    # Pinned by tests/infra/test_tree_sitter_pipeline.sh's
    # test_verify_plan_includes_freshness_after_generation.
    if [ "$RUN_RUST" -eq 1 ]; then
        add_tool "./scripts/tree-sitter-freshness.sh ensure"
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
        add_tool "./scripts/verify.sh compile-gate"
    fi

    # typecheck (cargo check) only when NOT also linting — clippy --all-targets
    # is a strict superset of `cargo check`, so running both would be redundant.
    if [ "$DO_TYPECHECK" -eq 1 ] && [ "$DO_LINT" -eq 0 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 ${_VERIFY_CHECK_TIMEOUT} ${CARGO_PRIO}cargo check ${AFFECTED_ALL_FLAGS} --tests"  # ld-ok: cargo — needs OCCT
        else
            add "timeout --kill-after=60 ${_VERIFY_CHECK_TIMEOUT} ${CARGO_PRIO}cargo check --workspace --tests"  # ld-ok: cargo — needs OCCT
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
        # on the test side. On a merge-gate narrowed retry, dark-factory sets
        # REIFY_GUI_RETRY_SPECS to the space-separated, gui-root-relative vitest
        # spec paths that failed (e.g. `src/__tests__/foo.test.ts`); forward them
        # so the block runs ONLY those specs via `npm test -- <specs>` (== `vitest
        # run <specs>`; the block cd's into gui/, so gui-relative positionals
        # match). Unset OR empty => full `npm test`, byte-identical to the
        # non-retry path (the §4.3 loud full-fallback for an empty gui subset).
        # PRD docs/prds/verify-retry-failed-only.md §4.2 leaf γ.
        #
        # SHELL-SAFETY: the specs are interpolated RAW into gui_inner, which
        # wrap_subshell then embeds into a `bash -c '…'` string, so at exec time
        # the tokens are literal script text — subject to word-splitting AND
        # pathname globbing AND command substitution / metacharacter evaluation,
        # not merely the single-quote break-out. The trusted source (dark-factory
        # verify_env) only ever emits plain vitest paths, but as defense-in-depth
        # reject any value carrying a character outside [A-Za-z0-9._/ -] (a
        # `$(…)`, backtick, `;`, `&`, glob, quote, …) so a future/untrusted caller
        # cannot smuggle shell through this seam. A rejected (non-empty) value
        # falls back to the full `npm test` with a stderr warning — the same §4.3
        # loud full-fallback as the empty/unset case. The allowlisted value is
        # still interpolated raw (mirroring _GATE_HEAVY_EXCLUDE), but now provably
        # holds only path/separator characters. A further check (below) rejects
        # any token beginning with '-', since the character allowlist alone
        # would still pass an option-like token (e.g. `--run`) straight through
        # to vitest's argument parser.
        local gui_inner="npm ci && npm run typecheck"
        if [ "$DO_TEST" -eq 1 ]; then
            local _gui_retry_specs="${REIFY_GUI_RETRY_SPECS:-}"
            local _gui_retry_ok=0
            # -z on the allowlist-stripped residue => every char is allowed.
            if [ -n "$_gui_retry_specs" ] && [ -z "${_gui_retry_specs//[A-Za-z0-9._\/ -]/}" ]; then
                _gui_retry_ok=1
                # The character allowlist alone still permits a leading '-'
                # (e.g. `--run`, `-x`), which would forward cleanly as
                # `npm test -- --run` and be parsed by vitest as an option,
                # not a spec path. Reject the whole value (same loud
                # full-fallback below) if ANY space-delimited token starts
                # with '-'. Unquoted word-splitting here is safe: the
                # allowlist above already excludes glob/metacharacters, so
                # this can only split on spaces, never glob or expand.
                local _gui_retry_tok
                for _gui_retry_tok in $_gui_retry_specs; do
                    case "$_gui_retry_tok" in -*) _gui_retry_ok=0; break ;; esac
                done
            fi
            if [ "$_gui_retry_ok" -eq 1 ]; then
                gui_inner+=" && npm test -- $_gui_retry_specs"
                # δ honest marker (task 5290): count the VALIDATED specs at this
                # SINGLE narrowing site (INV-5 — reuse the allowlist result, so
                # an ignored/invalid REIFY_GUI_RETRY_SPECS reports gui=0, matching
                # the loud full-fallback below). Unquoted split is safe: the
                # allowlist above already excludes glob/metacharacters, so this
                # word-splits only on spaces (never globs or expands).
                local -a _gui_retry_toks=($_gui_retry_specs)
                _RETRY_GUI_SUBSET_APPLIED=${#_gui_retry_toks[@]}
            else
                [ -n "$_gui_retry_specs" ] && echo "verify.sh: WARNING — REIFY_GUI_RETRY_SPECS contains characters outside [A-Za-z0-9._/ -] or a token beginning with '-'; ignoring the subset and running the full gui suite" >&2
                gui_inner+=" && npm test"
            fi
        fi
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
    #
    # LOADER SCRUB — the ONE add_tool() exception (task 5730). The node lane is
    # a tool line and needs the scrub, but this plan line is the only one the
    # executor eval's in its MAIN shell (everything else goes through
    # reaper_run_in_pgroup's `set -m; eval "$cmd" &` subshell). A leading
    # `export ...;` here would therefore persist into every SUBSEQUENT plan
    # line, including the cargo ones — silently stripping the OCCT search dir
    # from the rest of the gate. It goes INSIDE the `{ ... ; } &` braces, which
    # are themselves a background subshell, so it scopes to the npm work alone.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ] && [ -n "$_node_lane" ]; then
        add "{ ${_LD_SCRUB}${_node_lane} ; } & _VERIFY_NODE_BG_PID=\$!; trap 'if kill \"\$_VERIFY_NODE_BG_PID\" 2>/dev/null; then :; fi; _verify_cleanup' EXIT"  # ld-ok: self-scrubbed — carries _LD_SCRUB inside its own { ...; } & braces (main-shell eval, so a head-of-line export would leak forward)
    fi

    # lint: clippy over all targets, warnings-as-errors.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo clippy ${AFFECTED_ALL_FLAGS} --all-targets -- -D warnings"  # ld-ok: cargo — needs OCCT
        else
            add "timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo clippy --workspace --all-targets -- -D warnings"  # ld-ok: cargo — needs OCCT
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
        add "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo check -p reify-gui --features gui --tests; fi"  # ld-ok: cargo — MIXED shell+cargo (gui sidecar compile check); needs OCCT
    fi

    # The tree-sitter freshness POST-CONDITION leaf does NOT belong here, after
    # the clippy/gui-check wave — it must follow the LAST cargo leaf that can
    # compile the parser (add_test_passes), or it attests a fingerprint dir that
    # no test binary links. It is emitted at the end of build_plan; see the
    # `check` block there before moving it back up.

    # Overlap join: wait for the background node lane before infra checks / pole.
    # Maximises the concurrency window (join as late as possible while still
    # preceding the expensive pole and infra checks).
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ] && [ -n "$_node_lane" ]; then
        add 'wait "$_VERIFY_NODE_BG_PID"'  # ld-ok: builtin — `wait`, and main-shell eval'd; must never take a head-of-line export
    fi

    # Plain path: node lane as sequential lines (no foreground rust gate, e.g. action=test).
    if [ -n "$_node_lane" ] && { [ "$DO_LINT" -eq 0 ] || [ "$RUN_RUST" -eq 0 ]; }; then
        add_tool "$_gui_cmd"
        add_tool "$_sidecar_cmd"
        add_tool "$_ts_cmd"
    fi

    # Cheap static infra checks (opt-in). Test-side and lint-side, mirroring the
    # historical orchestrator split. Tied to RUN_RUST (the heavy gate) so a
    # frontend-only or docs-only staged commit stays fast.
    #
    # FAIL-FAST: emitted BEFORE add_test_passes (task #4448).
    if [ "$INCLUDE_INFRA" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$DO_TEST" -eq 1 ]; then
            add_tool "if test -f tests/sync_comments_test.sh; then timeout --kill-after=60 10m bash tests/sync_comments_test.sh; else echo 'WARNING: sync_comments_test.sh not found, skipping'; fi"
        fi
        if [ "$DO_LINT" -eq 1 ]; then
            add_tool "if test -f scripts/test_pm_standardization.sh; then timeout --kill-after=60 10m bash scripts/test_pm_standardization.sh; else echo 'WARNING: test_pm_standardization.sh not found, skipping'; fi"
            add_tool "if test -f scripts/check_event_inventory.sh; then timeout --kill-after=60 5m bash scripts/check_event_inventory.sh; else echo 'WARNING: check_event_inventory.sh not found, skipping'; fi"
            add_tool "if test -f scripts/check-nan-safe-ordering.sh; then timeout --kill-after=60 5m bash scripts/check-nan-safe-ordering.sh; else echo 'WARNING: check-nan-safe-ordering.sh not found, skipping'; fi"
            add_tool "if test -f scripts/check-compute-trampoline-registration.sh; then timeout --kill-after=60 5m bash scripts/check-compute-trampoline-registration.sh; else echo 'WARNING: check-compute-trampoline-registration.sh not found, skipping'; fi"
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
        # Timeout is _VERIFY_PREBUILD_TIMEOUT (45m default, esc-5382-1 — see the
        # REIFY_VERIFY_PREBUILD_TIMEOUT knob-doc above for the derivation), which is
        # distinct from the run_all wall so the plan-shape test
        # (tests/infra/test_verify_failfast_order.sh) can still assert the pre-step is
        # not the walled run_all.sh line.
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
        # task 5382 (esc-5382-1): budget via _VERIFY_PREBUILD_TIMEOUT (25m), not a
        # fixed 10m. These two pre-builds are the merge path's COLD release
        # native-kernel build; 10m SIGTERM'd reify-cli mid-compile with zero failing
        # assertions. See the knob-doc block in the header for the derivation.
        add "if test -f crates/reify-audit/Cargo.toml; then timeout --kill-after=60 ${_VERIFY_PREBUILD_TIMEOUT} ${CARGO_PRIO}cargo build --release -p reify-audit 2>&1; fi"  # ld-ok: cargo — needs OCCT
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
        add "if test -f crates/reify-audit/Cargo.toml && [ ! -f target/release/reify-audit ]; then echo 'ERROR(#4624): reify-audit binary missing after pre-build step — PTODO gate will silently SKIP; restore the pre-step above or remove this check deliberately' >&2; false; fi 2>&1"  # ld-ok: builtin — test/echo/false only
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
        add "if test -f crates/reify-cli/Cargo.toml; then timeout --kill-after=60 ${_VERIFY_PREBUILD_TIMEOUT} ${CARGO_PRIO}cargo build --release -p reify-cli 2>&1; fi"  # ld-ok: cargo — needs OCCT
        add_tool "if test -f target/release/reify; then git rev-parse HEAD > target/.reify-bin-sha 2>/dev/null || true; fi"
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
        #
        # LOADER SCRUB PLACEMENT (task 5730) — add_tool()'s scrub must stay a
        # leading statement at the HEAD of this line, ahead of the `if`, and
        # must NEVER be rewritten into a `KEY=VALUE` prefix token beside the
        # three REIFY_* tokens after `then`. tests/infra/test_run_all_ambient_isolation.sh
        # derives the live injected-var set from exactly that `then`-anchored
        # window and cross-checks SET EQUALITY against
        # tests/infra/run-all-ambient-vars.manifest; a fourth token there would
        # trip the ledger drift guard and drag in a per-var hostile-ambient
        # sub-case. Both known bites of the leak (esc-4581-87, task 5321) were
        # infra tests reached through this line.
        add_tool "if test -f tests/infra/run_all.sh; then REIFY_AUDIT_NO_COLD_BUILD=1 REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 REIFY_RUN_ALL_CONTENT_SKIP=1 timeout --kill-after=60 30m bash tests/infra/run_all.sh 2>&1; fi"
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
        # Git repository-environment scrub (task #7106), interpolated from
        # reify_git_env_scrub_prefix so this leaf and the runner helper can never
        # disagree about the variable set. It sits INSIDE the leaf, between the
        # timeout and the bash it wraps — NOT in add_tool's _LD_SCRUB, which
        # prefixes EVERY plan line and would rewrite every plan string, breaking
        # the --print-plan byte-identity oracles (test_occt_flock_gate.sh Tests
        # 17/17b, test_verify_scope.sh, test_occt_gated_scope.sh).
        local _git_scrub
        _git_scrub="$(reify_git_env_scrub_prefix)"
        set -f  # disable pathname expansion: keep glob tokens as literals
        for _glob in $SELECTED_INFRA_GLOBS; do
            add_tool "( for _vt in $_glob; do [ -f \"\$_vt\" ] || continue; timeout --kill-after=60 10m $_git_scrub bash \"\$_vt\" || exit \$?; done )"
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

    # tree-sitter freshness POST-CONDITION (task #5629, review rounds 2-3).
    # The `ensure` leaf near the top of the plan runs BEFORE the cargo wave and
    # only ATTEMPTS the repair — it bumps mtimes and trusts cargo to act on them,
    # and by design it never fails for a condition it believes it repaired. So
    # without this line the gate carried no evidence the rebuild actually
    # happened: if the mtime force failed to trigger one, the run went green
    # having linked an archive it never compiled — the same false-GREEN class the
    # task exists to close, one level up. `check` closes it by ASSERTING, after
    # the fact, that the archives cargo built match the sources on disk.
    #
    # EMITTED LAST — after add_test_passes — and that position is load-bearing
    # (review round 3). Round 2 placed it right after the clippy / `cargo check -p
    # reify-gui` wave, which attested the WRONG archive: clippy compiles into a
    # different fingerprint dir than the test-profile build, and under
    # `--profile both` the debug and release nextest passes each compile the
    # parser again, all of them AFTER that point. The assertion has to follow the
    # last cargo leaf that can compile the parser, or it attests an archive no
    # test binary ever linked.
    #
    # Guard: RUN_RUST && (lint || typecheck || test). The `test` arm was missing
    # until the amendment pass on #5629, on the reasoning that "action=test has no
    # compile leaf before this pole, so asserting there would hard-fail a
    # repairable pre-build condition". That reasoning contradicted this leaf's own
    # position: it is emitted AFTER add_test_passes, and on an action=test plan
    # add_test_passes emits `cargo nextest run --workspace`, which COMPILES the
    # parser. So every action=test plan forced a rebuild via `ensure` and then
    # asserted nothing — leaving the whole test-only tier carrying exactly the
    # one-level-up false GREEN this leaf was added to close.
    #
    # RUN_RUST is what keeps docs-only / gui-src-only plans at zero command leaves;
    # with RUN_RUST=1 at least one of the three action flags is always set, so the
    # inner disjunction is documentation of intent rather than a live filter — it
    # keeps the leaf tied to "something compiled the parser", which is what makes
    # the assertion meaningful.
    #
    # `check` hard-asserts over every fingerprint dir whose build-script run marker
    # advanced during THIS run (the epoch `ensure` stamped), so the multi-dir
    # debug+release case is covered rather than just the single newest dir. Dirs
    # untouched by this run stay dormant `note:` lines — a checkout carries 7-9 of
    # them, stale forever, so a whole-tree assertion would be permanently RED.
    # Pinned by tests/infra/test_tree_sitter_pipeline.sh's
    # test_verify_plan_includes_freshness_after_generation.
    if [ "$RUN_RUST" -eq 1 ] \
        && { [ "$DO_LINT" -eq 1 ] || [ "$DO_TYPECHECK" -eq 1 ] || [ "$DO_TEST" -eq 1 ]; }; then
        add_tool "./scripts/tree-sitter-freshness.sh check"
    fi

    # retry_failed_only HONEST MARKER (task 5290 / PRD verify-retry-failed-only
    # δ §4.4, INV-6). At plan-BUILD time, announce to STDOUT that this run is a
    # genuinely-narrowed failed_only retry, so dark-factory runtime mining can
    # distinguish a real subset gate from a full re-verify. Emitted via a direct
    # `echo` — NOT via `add` — so it is a one-time build-time announcement, never
    # an executed plan command. build_plan runs in BOTH --print-plan and execute
    # modes, so the marker lands on stdout in a real DF retry (D5 captures it)
    # AND is a faithful hermetic oracle under --print-plan. Mirrors the
    # lib_clock_stop.sh `@@TOKEN@@` marker grammar, but inline and to STDOUT
    # (only verify.sh emits it, so no sourced lib — PRD §4.4), distinct from α's
    # `retry refused:` refusal lines (STDERR) and the clock markers (STDERR).
    # The single line carries the per-suite APPLIED subset size — sourced from
    # the SAME single narrowing sites (INV-5 no re-derivation): nextest_debug /
    # nextest_release recorded by emit_nextest_pass as it applied each profile's
    # within-ceiling subset (0 when that profile fell back / is not in the plan);
    # gui recorded by the gui block from its validated REIFY_GUI_RETRY_SPECS;
    # run_all the POST-VALIDATION matched count sourced from run_all.sh's OWN
    # count-only probe (task 5373): verify.sh forks run_all.sh in a hermetic
    # count-only mode so run_all.sh — which owns the member-skip predicate — is
    # the single source of truth (INV-5), and a stale/renamed basename it would
    # drop no longer inflates the count. See the counting site below.
    # Fire IFF scope=failed_only AND ≥1 suite ACTUALLY narrowed — the honest-
    # events gate (INV-6): a within-ceiling nextest subset applied for ≥1
    # profile (_RETRY_NEXTEST_*_APPLIED>0 ⇒ eligible AND usable), OR a non-empty
    # REIFY_RUN_ALL_MEMBER_SUBSET, OR ≥1 validated REIFY_GUI_RETRY_SPECS. The
    # three arms are an OR, each independent of the nextest tree-OID gate
    # (run_all/gui narrow on their own env), so the marker is SUPPRESSED on
    # every nextest full-fallback/refusal path (tree drift / no subset / subset
    # too large / no nextest) UNLESS run_all/gui narrowed — and never emitted on
    # a non-retry. This is what stops DF's runtime mining from miscounting a
    # full re-verify as a failed_only green gate. Default byte-identical: no
    # REIFY_VERIFY_RETRY_SCOPE=failed_only ⇒ no echo, so the ~30 existing
    # plan-shape tests stay green.
    if [ "${REIFY_VERIFY_RETRY_SCOPE:-}" = "failed_only" ]; then
        # run_all subset size: the POST-VALIDATION matched count, sourced from
        # run_all.sh's OWN count-only probe rather than a raw word-count of the
        # DF-supplied member list (task 5373, closing task 5290's accepted gap).
        # Previously this was a BEST-EFFORT word-count: a supplied member
        # basename absent under run_all's INFRA_DIR (renamed/deleted between
        # DF's attempt-0 failure-set discovery and this retry's dispatch) is
        # WARNed-and-dropped by run_all.sh ("member '...' not found in
        # $INFRA_DIR (ignored)") while the word-count still counted it —
        # inflating run_all=N vs. what actually ran (INV-6 honesty gap). Now
        # verify.sh forks run_all.sh's hermetic count-only probe
        # (REIFY_RUN_ALL_SUBSET_COUNT_ONLY=1), which applies run_all.sh's own
        # member-skip predicate and reports the matched count WITHOUT running
        # any member — so run_all.sh is the single source of truth (INV-5) and
        # the gap is CLOSED. A defensive word-count fallback (below) still
        # fires the marker on the impossible-in-tree case where run_all.sh is
        # missing or predates the probe.
        local _mk_run_all=0
        if [ -n "${REIFY_RUN_ALL_MEMBER_SUBSET:-}" ]; then
            # Fork the count-only probe. It resolves cwd-relative because
            # build_plan runs after the `cd "$REPO_ROOT"` near verify.sh's top,
            # exactly like the run_all.sh plan line above (both guard on
            # `test -f tests/infra/run_all.sh`). REIFY_RUN_ALL_MEMBER_SUBSET is
            # forwarded explicitly (it is already in the environment, but
            # forwarding is robust to a non-exported caller). The pipe is
            # terminated with `|| true` to stay set -e/pipefail-safe, and the
            # anchored grep pulls exactly the machine token.
            local _mk_ra_tok=""
            if [ -f tests/infra/run_all.sh ]; then
                _mk_ra_tok="$(REIFY_RUN_ALL_SUBSET_COUNT_ONLY=1 \
                    REIFY_RUN_ALL_MEMBER_SUBSET="${REIFY_RUN_ALL_MEMBER_SUBSET}" \
                    bash tests/infra/run_all.sh 2>/dev/null \
                    | grep -oE '@@REIFY_RUN_ALL_SUBSET_MATCHED=[0-9]+@@' | head -n1 || true)"
            fi
            if [ -n "$_mk_ra_tok" ]; then
                # Strip the @@…=<n>@@ wrapper down to the bare integer.
                _mk_ra_tok="${_mk_ra_tok#@@REIFY_RUN_ALL_SUBSET_MATCHED=}"
                _mk_run_all="${_mk_ra_tok%@@}"
            else
                # Defensive fallback — impossible in-tree (run_all.sh present
                # and 5373-aware always emits the token). Only reached if
                # run_all.sh is missing or predates task 5373, so it prints no
                # token: use the prior noglob-guarded word-count so the marker
                # still FIRES (INV-6 fire-when-narrowed) rather than
                # under-reporting to 0. set -f around the split so a stray glob
                # in a member name cannot pathname-expand (members are .sh
                # basenames; belt-and-suspenders), then RESTORE the caller's
                # prior noglob state rather than unconditionally clearing it.
                local -a _mk_ra_toks
                local _mk_had_f=0
                case $- in *f*) _mk_had_f=1 ;; esac
                set -f
                _mk_ra_toks=(${REIFY_RUN_ALL_MEMBER_SUBSET})
                [ "$_mk_had_f" -eq 1 ] || set +f
                _mk_run_all=${#_mk_ra_toks[@]}
            fi
        fi
        if [ "$_RETRY_NEXTEST_DEBUG_APPLIED" -gt 0 ] \
            || [ "$_RETRY_NEXTEST_RELEASE_APPLIED" -gt 0 ] \
            || [ "$_mk_run_all" -gt 0 ] \
            || [ "$_RETRY_GUI_SUBSET_APPLIED" -gt 0 ]; then
            echo "@@REIFY_RETRY_SCOPE=failed_only@@ nextest_debug=${_RETRY_NEXTEST_DEBUG_APPLIED} nextest_release=${_RETRY_NEXTEST_RELEASE_APPLIED} run_all=${_mk_run_all} gui=${_RETRY_GUI_SUBSET_APPLIED}"
        fi
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
    # `closure=` is APPENDED, never inserted: tests/infra/test_verify_scope.sh
    # greps this line as the unanchored substrings "NARROW_ACTIVE=1 affected=…" /
    # "NARROW_ACTIVE=0 affected=ALL", and plan_capture_lib.sh's plan_narrow_active
    # matches NARROW_ACTIVE=([0-9]+); all three survive a trailing append and none
    # survives a reordering.  A `#` comment line, so plan_count_noncomment_lines
    # (`^[^#]`) — the oracle behind the THROUGHPUT-COUNTS sentinel — cannot see it.
    echo "# narrowing — NARROW_ACTIVE=$NARROW_ACTIVE affected=${AFFECTED:-} closure=${AFFECTED_CLOSURE:-}"
    echo "# --- environment (process-level; inherited by every command below EXCEPT where a command overrides it inline — see the LD_LIBRARY_PATH scrub on non-cargo lines) ---"
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
            # Glob, not an exact string: task 5730 prefixes this plan line with
            # the add_tool() loader-path scrub, so an exact match silently went
            # dead and took both annotation lines below out of --print-plan.
            # `psi-gate` is the last token of the command, so the trailing `*`
            # only absorbs a future suffix and cannot also catch compile-gate.
            *'./scripts/verify.sh psi-gate'*)
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
