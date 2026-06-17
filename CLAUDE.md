# Reify

## Local Dev Setup

The orchestrator verify pipeline requires `sccache` on PATH (install via `cargo install sccache`). `orchestrator.yaml` sets `RUSTC_WRAPPER=sccache` and `CARGO_INCREMENTAL=0` to share a rustc cache across worktrees; rationale and design in `~/.claude/plans/playful-hopping-nygaard.md`.

### Manifold prebuilt C++ libs

All four native kernels link **prebuilt** libs from `/opt/reify-deps` — OCCT, OpenVDB, and gmsh always have, and manifold now joins them. `manifold-csg-sys`'s `build.rs` would otherwise clone `elalish/manifold` and cmake-build the whole C++ tree (manifold + builtin TBB + builtin Clipper2 + manifoldc) from source in **every** worktree (~4× per worktree, 56 MB clone + ~227 MB OUT_DIR each), which on a cold cache pushed merge verifies past their inner timeouts (exit 124).

Instead, `scripts/build-manifold-deps.sh` builds those libs **once per host** into `/opt/reify-deps/manifold/lib`, and a `links`-name override in `.cargo/config.toml` (`[target.x86_64-unknown-linux-gnu.manifold]`) makes Cargo **skip the from-source build entirely** and link the prebuilt static libs. `setup-dev.sh` runs the build script for you. The override is target-scoped to `x86_64-unknown-linux-gnu`, so wasm/emscripten builds keep their own FetchContent path.

- **A `manifold-csg-sys` pin bump requires re-running `scripts/build-manifold-deps.sh`** (it tracks the version in `Cargo.lock` + the upstream `MANIFOLD_VERSION` tag and stamps `/opt/reify-deps/manifold/VERSION`).
- `scripts/check-manifold-deps.sh` is a preflight guard wired into `verify.sh` as the first Rust step: it fails verify with a clear "run the deps script" message if the prebuilt is missing or version-drifted, instead of a cryptic linker error mid-compile.

### Single-command GUI launch

From a clean checkout, two wrapper scripts collapse the full sidecar → npm install → npm build → cargo build → launch pipeline into a single command. Both scripts export `LD_LIBRARY_PATH` for OCCT's bundled snap shared libraries automatically — no need to set it yourself.

- **`scripts/run-gui.sh <file.ri>`** — release-mode launch (default). Builds `gui/dist`, the cargo `--release` binary, and execs `target/release/reify-gui`. No vite, no devtools, no `:3939` debug listener — matches what end users will eventually run from a bundled distribution.
- **`scripts/run-gui-dev.sh <file.ri>`** — dev-mode launch. Starts vite dev server on `:1420` (with HMR), waits for readiness, builds the cargo binary in debug profile, sets `REIFY_DEBUG=1`, and runs `target/debug/reify-gui` as a child process. `REIFY_DEBUG=1` opens an MCP debug listener on `127.0.0.1:${REIFY_DEBUG_PORT:-3939}` (see `gui/src-tauri/src/main.rs`). Set `REIFY_DEBUG_PORT` to a different value per worktree to avoid port collisions when running concurrent GUI smokes; the static `.mcp.json` stays at the default 3939. The script reaps the vite background process via an EXIT trap when reify-gui exits.

If the `reify` binary is already built, two equivalent CLI entry points work without re-running the wrapper:

- **`reify gui --debug <file.ri>`** — `--mcp` is accepted as an alias for `--debug`.
- **`reify gui-debug <file.ri>`** — sugar for `gui --debug`; both route through the same code path and propagate `REIFY_DEBUG=1` to the spawned `reify-gui` subprocess.

### Per-worktree debug-port wiring for dispatched agents

A dispatched agent (factory-launched Claude in a worktree) reads the static `<worktree>/.mcp.json` for its MCP server URLs. Without intervention the `reify-debug` entry is hard-pinned to `:3939`, so the agent's MCP client connects to whichever foreign GUI holds that port (the bug described in esc-4202-61). `scripts/setup-worktree-debug-port.sh` fixes this at provisioning time:

```bash
# Factory tooling runs this once per worktree before dispatching the agent:
port=$(scripts/setup-worktree-debug-port.sh [worktree_dir])
export REIFY_DEBUG_PORT=$port
# Then: scripts/run-gui-dev.sh binds $REIFY_DEBUG_PORT → agent's .mcp.json targets the same port.
```

**Stdout contract:** the script prints only the resolved port integer (a bare decimal, `^[0-9]+$`, 1–65535) to stdout; all diagnostics go to stderr. This makes `port=$(...)` safe.

**Port resolution** (mirrors `parse_debug_port` / `resolveDebugPort` / `resolveReifyDebugUrl`):
- If `REIFY_DEBUG_PORT` is already a valid port (strict `^[0-9]+$`, value 1–65535, no whitespace), it is used verbatim.
- Otherwise (unset, empty, non-digit, whitespace-padded, 0, or > 65535) a free ephemeral port is allocated via `allocate_free_port()` in `scripts/lib_portable.sh`.

**Single-allocation invariant:** the port is written to BOTH `.mcp.json` (so the agent's MCP client targets the right GUI) AND stdout (so the caller can `export REIFY_DEBUG_PORT=$port` and `run-gui-dev.sh` binds the same port). These two consumers MUST agree — splitting the allocation would recreate esc-4202-61.

**git skip-worktree hygiene:** after patching `.mcp.json`, the script runs `git update-index --skip-worktree .mcp.json` (guarded by `git rev-parse --is-inside-work-tree`) so the per-worktree ephemeral port is invisible to `git status`/diffs and never lands in a task commit or trips `land.sh`'s clean-tree gate. The committed `.mcp.json` default (`:3939`) is unchanged.
- Undo with: `git update-index --no-skip-worktree .mcp.json`
- Outside a git work tree the git step is a guarded no-op — the script succeeds normally.

**G4 provisioning seam:** the *trigger* for this script lives upstream in factory tooling (a separate task for Leo). The reify-side deliverable is the script itself; factory tooling invokes it and injects the printed port into the dispatched agent's environment.

## Landing on main

Prefer the orchestrator's merge queue (`/merge-queue`) to land a task branch. When the orchestrator is congested or down and you must land directly, use **`scripts/land.sh <task-branch>`** — the *only* sanctioned manual-landing path:

- It refuses to run unless you are on `main` with a **clean working tree** (the `pre-merge-commit` gate verifies the *whole* working tree, so unrelated dirt would otherwise force a false-negative — the original reason direct landings reached for `--no-verify`).
- It runs a real `git merge --no-ff` (**not** `--no-verify`), so `hooks/pre-merge-commit` runs the full `--scope all --profile both` gate.
- It marks the main-gate sentinel so `hooks/reference-transaction` records the resulting `refs/heads/main` move as **sanctioned**.

**Never** land on `main` with raw `git merge --no-verify`, `git update-ref refs/heads/main`, `git reset`, or `commit-tree`+`update-ref` plumbing. Those skip the verify gate *and* trip the `reference-transaction` tripwire (which logs every unsanctioned `main` move, and hard-aborts it once `REIFY_MAIN_GATE_ENFORCE=1` is set). The tripwire ships **warn-only** by default; `REIFY_MAIN_GATE_BYPASS=1` is the break-glass allow. The gate fires only when git hooks are wired (`core.hooksPath=hooks`).

**Per-worktree core.hooksPath isolation:** Claude Code's native worktree feature rewrites the SHARED `.git/config` `core.hooksPath` to git's inert `.git/hooks` samples dir on every worktree enter, which would otherwise darken the gate. Two complementary defenses are wired in by `scripts/setup-dev.sh`: **(A)** a `<common-git-dir>/hooks → ../hooks` symlink so that even linked worktrees lacking a `config.worktree` override resolve the absolute `.git/hooks` fallback to the real gate; **(B)** `scripts/setup-main-gate-worktree-config.sh` enables `extensions.worktreeConfig` and seeds main's `.git/config.worktree` with `core.hooksPath = hooks`. Git reads `config.worktree` first, so the per-worktree value beats any shared-config clobber — the gate stays live even when Claude Code owns the shared value. The dark-factory `create_worktree` per-worktree write (so dispatched agents' worktrees also get the override) is a cross-repo seam handled separately.

## Deploying the orchestrator (config/code changes)

The orchestrator loads `orchestrator.yaml` **ONCE at startup** — there is no hot-reload, SIGHUP, or file-watch. It also enforces a **dirty-start guard**: it refuses to start with uncommitted tracked changes in `project_root` (the `--config` path, i.e. `/home/leo/src/reify`). A crash-loop self-arrests after `StartLimitBurst=10` in 600s, then stays DOWN.

**Invariant: COMMIT/LAND FIRST, then restart.** Any config or code change must be committed and landed on `main` (via `/merge-queue` or `scripts/land.sh`) before the orchestrator is restarted. Restarting with a dirty `project_root` causes a crash-loop outage.

**A task running under the orchestrator must NOT `systemctl restart orchestrator-reify.service` directly** — that sends SIGTERM to its own agent mid-run (self-kill), leaving incomplete state.

### Safe restart procedure: `scripts/orchestrator-redeploy-restart.sh`

Use `scripts/orchestrator-redeploy-restart.sh` from a task agent to schedule a safe detached restart:

```bash
scripts/orchestrator-redeploy-restart.sh
```

**What it does:**

1. **Schedule mode (default):** Checks `project_root` is clean (`git status --porcelain --untracked-files=no`). If dirty, exits non-zero immediately with a "commit/land first" message — schedules NOTHING. If clean, best-effort pre-cleans any stale transient unit, then invokes:

   ```
   systemd-run --user --on-active=<ORCH_RESTART_DELAY> --unit=<ORCH_TRANSIENT_UNIT> \
     --collect --setenv=ORCH_UNIT=… --setenv=ORCH_PROJECT_ROOT=… \
     <script> --exec-restart
   ```

   The transient unit is a child of the **USER systemd manager** (not the orchestrator), so it fires **after the triggering agent has exited** — no self-kill.

2. **Exec mode (`--exec-restart`, run by the transient unit at fire time):** Re-checks `project_root` is clean. If clean → blocking `systemctl --user stop <unit>` THEN `systemctl --user start <unit>`. **NEVER `systemctl restart`** — the unit's `TimeoutStopSec=90` graceful-stop window (cancel in-flight tasks, reap agents, release the fcntl lock) causes `systemctl restart`'s start-half to be cancelled mid-window, leaving the service down. If dirty at fire time → leaves the old orchestrator RUNNING, logs a warning, exits 0 (not stopping avoids a crash-loop outage).

### `project_root` is the MAIN checkout

The dirty-start guard targets `/home/leo/src/reify` (the `--config` project_root, i.e. the main checkout) — NOT the task worktree. Task worktrees are always dirty with WIP; the clean-check uses `--untracked-files=no` to mirror the orchestrator's "uncommitted tracked changes" semantics and avoid false-positives from benign untracked files.

### Merge worker fast-path for config-only changes

The merge worker's **trivial-pass** fast-path (scope=config, diff touches only non-Rust/non-TS files) lands config-only changes (e.g. `orchestrator.yaml` tweaks) without a full `--scope all` verify. This makes the commit/land-first step fast for pure config deploys.

**Drift-guard exception — verify-pipeline files are NOT trivially config-only.** Changes touching `scripts/verify.sh`, its live `source`d libs (`occt-scope-lib.sh`, `release-scope-lib.sh`, `affected-crates-lib.sh`, `lib_test_semaphore.sh`), or the verify-pipeline data files (`.config/nextest.toml`, `scripts/occt-touching-crates.txt`, `scripts/release-sensitive-crates.txt`, `scripts/verify-pipeline-infra-tests.txt`, `scripts/gen-nextest-config.sh`) are NOT safe to fast-path even though they are non-Rust/non-TS — these files load-bear the `--scope all` plan, and a plan-count change that skips the full gate ambushes the next Rust task with a RED `tests/infra/test_verify_throughput.sh` (root-caused via esc-4288-206; the #4618/#4624 → #4288 ambush is the canonical incident).

The canonical source of truth for the load-bearing set is:
- `scripts/verify-pipeline-paths.txt` — static manifest of non-`source`-derivable deps
- verify.sh's live `source "$SCRIPT_DIR/..."` lines — auto-derived, self-healing for future additions

The consultable oracle is `scripts/verify-pipeline-guard.sh`:
```
bash scripts/verify-pipeline-guard.sh requires-full-gate <changed-files...>
```
Exit 0 → route to the full `--scope all` gate (or at minimum run `tests/infra/test_verify_throughput.sh` + `tests/infra/test_verify_scope.sh`). Exit 1 → fast-path safe. Exit 2 → usage error.

**Cross-repo seam:** the merge-worker trivial-pass classifier is dark-factory code and **must be wired to consult this script** before taking the config-only fast-path (the same class of seam as the `advance_main`/`main_gate_mark_command` notes above). Reify ships the oracle; dark-factory does the wiring (tracked separately as a non-blocking follow-up to esc-4288-206).

### Env knobs

| Variable | Default | Purpose |
|---|---|---|
| `ORCH_UNIT` | `orchestrator-reify.service` | Orchestrator systemd unit |
| `ORCH_PROJECT_ROOT` | `/home/leo/src/reify` | Main checkout to guard |
| `ORCH_RESTART_DELAY` | `60s` | on-active delay before restart fires |
| `ORCH_TRANSIENT_UNIT` | `orch-redeploy-restart` | Name of the transient systemd-run unit |

### Origin

This mechanism was introduced in the 2026-06-15 agent-cargo-jobserver deploy (task 4620) as the vehicle for deploying dark-factory follow-ups (agent CPU de-prioritization, merge-verify log archival) that required an orchestrator restart. The failure mode of `systemctl restart` under `TimeoutStopSec=90` was learned on that deploy.

## Test concurrency

The verify pipeline is governed by three admission controls that layer in order: **`compile_gate()`** (compile-phase PSI backpressure, task 4618) → **`psi_gate()`** (test-phase PSI backoff) → **held-slot semaphore** (hard test×test cap) → run passes.

- **`compile_gate()`** (`scripts/verify.sh`, task 4618): soft PSI admission backstop for the **clippy/check/compile** phases (lint/typecheck/all actions). Wired via `verify.sh compile-gate` as a plan line immediately before cargo check/clippy, after the tree-sitter prereq. Reads `/proc/pressure/cpu` avg10 and backs off while `avg10 >= THRESHOLD` (default 85 %). **Admit-on-timeout** (fairness floor): on `MAX_WAIT` (default 300 s) the gate **admits and logs a warning — NEVER exits 75**. This is the fundamental difference from `psi_gate`: compile admission is soft backpressure (delays/staggers a compile start) and can **never requeue a task** — structurally storm-proof. No WINDOW/dispatch-file/flock (compiles run concurrently under the jobserver). `DF_VERIFY_ROLE=merge` → immediate bypass (CAVEAT 1: merge never waits). Pure `test` action omits the compile-gate (the nextest compile is already inside the psi-gate + semaphore region; no double-gate). Introduces **zero host-baked constants**: only a PSI % + durations — host-portable by kernel normalization (no nproc-derived count).
- **`psi_gate()`** (`scripts/verify.sh`): pressure-reactive admission backoff for the **test-execution** phase. Reads `/proc/pressure/cpu` avg10 and blocks until CPU pressure drops below a threshold (default 50 %) and a spacing window (default 20 s) has elapsed. Guards **test × compile** contention — any concurrent verify phase counts, not just test passes.
- **Held-slot semaphore** (`scripts/lib_test_semaphore.sh`): hard **test × test** concurrency cap. Holds an exclusive flock on FD 9 across all test passes so at most **N** verifies run their test-execution phase simultaneously (default `N=1`). Compile, check, clippy, infra steps, and `psi_gate()` itself are **outside** the gated region.

**Why the compile-gate threshold is 85 (not 50):** The dual-pool jobserver is merge-favored — `task_baseline = max(1, nproc//4)` of tokens are reserved for task lanes (e.g. 8 task / 24 merge at nproc=32; scales with the host). During a healthy EXEMPT merge, the box legitimately runs hot. A lone merge holding its reserved core fraction does NOT by itself drive avg10 to 85 (PSI measures runnable-task stall, not utilization); only sustained multi-lane oversubscription does. The jobserver-balancer already holds task pools at avg10 ≥ 50 (mirroring `psi_gate`'s threshold); the compile-gate at 85 is a deliberately coarser verify.sh-layer backstop for when the hold + jobserver cap are insufficient (implicit-token leak + non-cargo load). The threshold is a tunable knob — no empirical level is frozen into any test.

**Compose order:** `compile-gate` (lint/typecheck/all: before clippy/check) → `psi-wait` (test/all: before nextest) → `acquire-slot` → `run-test-passes-with-slot-held` → `release-slot`. The `@@SEMAPHORE_ACQUIRE@@` sentinel is emitted by `add_test_passes()` (`verify.sh`) AFTER the `psi_gate()` entry, so the slot is not occupied during a pressure wait. `@@SEMAPHORE_RELEASE@@` marks the end of the gated region. Both sentinels are handled in the executor and annotated by `--print-plan`.

**Knobs — compile-gate** (`scripts/verify.sh compile_gate()`):
- **`REIFY_COMPILE_GATE_THRESHOLD`** — avg10 % ceiling (default `85`; host-portable PSI %)
- **`REIFY_COMPILE_GATE_MAX_WAIT`** — admit-on-timeout seconds (default `300`; never exit 75)
- **`REIFY_COMPILE_GATE_POLL`** — recheck interval in seconds (default `5`)
- **`REIFY_COMPILE_GATE_PROC_PATH`** — PSI source (default `/proc/pressure/cpu`; testability knob)
- **`REIFY_COMPILE_GATE_DISABLE`** — set to `1` for total bypass (break-glass)

**Knobs — test semaphore** (`scripts/lib_test_semaphore.sh`):
- **`REIFY_TEST_SEMAPHORE_CONCURRENCY`** — slot count N (default `1`)
- **`REIFY_TEST_SEMAPHORE_WAIT`** — max seconds to wait for a slot (default `1800`)
- **`REIFY_TEST_SEMAPHORE_LOCK`** — base path for slot files (default `${TMPDIR:-/tmp}/reify-test-semaphore-$(id -u).lock`)
- **`REIFY_TEST_SEMAPHORE_DISABLE`** — set to `1` for a total bypass (no slot acquired)

**`DF_VERIFY_ROLE=merge` exemption:** all three admission controls (`compile_gate`, `psi_gate`, `test_semaphore_acquire`) skip acquisition when `DF_VERIFY_ROLE=merge`. The merge gate **never waits behind a task slot**. This exemption fires on both paths: the orchestrator queue merge path (orchestrator injects `DF_VERIFY_ROLE=merge`) and the local `land.sh`/`pre-merge-commit` path.

**Backpressure — exit 75 (EX_TEMPFAIL):** when no slot is acquired within `REIFY_TEST_SEMAPHORE_WAIT` seconds, `test_semaphore_acquire` returns 75 and `verify.sh` propagates `return 75` — the same EX_TEMPFAIL `psi_gate()` emits on timeout. The orchestrator treats exit 75 as retry-capped transient infra (same class as OCCT-slot/ENOSPC) and requeues the task; no spurious task failure occurs. **The compile-gate NEVER exits 75** — it only delays and admits. **No dark-factory / orchestrator-code change is required** (PRD §6/§7): `DF_VERIFY_ROLE=merge` injection and exit-75 requeue are pre-existing orchestrator behaviours the semaphore reuses verbatim.

Canonical reference: `docs/prds/test-run-concurrency-semaphore.md` (§1 motivation, §2 design decisions D1/D2/D5/D6, §6 no-dark-factory-change, §7 seam table). PRD §2 originally cited `verify.sh:161` (merge bypass) and `verify.sh:228` (exit-75) — those lines have since drifted; prefer stable function names (`compile_gate`, `psi_gate`, `test_semaphore_acquire`, `@@SEMAPHORE_ACQUIRE@@`/`@@SEMAPHORE_RELEASE@@`) over line numbers for durable code links.

## Memory Usage

### When to read memory
- **Session start** — search for project context, recent decisions, active conventions
- **Encountering unfamiliar entities** — `get_entity` to understand relationships
- **Before architectural decisions** — search for prior decisions and rationale
- **Tasks with memory_hints** — execute hint queries via `search`, look up hint entities via `get_entity`

### When to write memory
- **Decisions made** — immediately, don't wait until session end
- **Conventions discovered** — coding patterns, naming rules, project norms
- **Session end** — reflect and write observations, summaries of what was accomplished

### Write operations

| Operation | Cost | When to use |
|-----------|------|-------------|
| `add_memory` | 0-3 LLM calls | Discrete, distilled facts — **prefer this** |
| `add_episode` | 5-15 LLM calls | Raw content needing extraction — use sparingly |

### Category routing

| Category | Primary Store | Use for |
|----------|--------------|---------|
| `entities_and_relations` | Graphiti | Facts about things and connections |
| `temporal_facts` | Graphiti | State that changes over time |
| `decisions_and_rationale` | Graphiti | Choices made and why |
| `preferences_and_norms` | Mem0 | Conventions, style rules |
| `procedural_knowledge` | Mem0 | Workflows, how-to steps |
| `observations_and_summaries` | Mem0 | High-level takeaways, session recaps |

## Write-Tagging Convention

Always pass these parameters on write operations:
- **`project_id`**: `"reify"`
- **`agent_id`**: descriptive identifier, e.g. `"claude-interactive"`, `"claude-task-7"`, `"reconciliation-stage-1"`

## Task Routing

All task operations go through **fused-memory MCP tools** — not the Taskmaster CLI or Taskmaster MCP directly. This ensures the TaskInterceptor emits reconciliation events for state transitions.

Use `project_root: "/home/leo/src/reify"` for all task operations.

Status transitions (`done`, `blocked`, `cancelled`, `deferred`) trigger targeted reconciliation automatically.

## Session Lifecycle

### Starting a session
1. Search memory for project context: `search(query="project overview and current status", project_id="reify")`
2. Check task tree: `get_tasks(project_root="/home/leo/src/reify")`
3. If working on a specific task, check its `memory_hints` and execute the hint queries

### During a session
- Write decisions and discoveries immediately via `add_memory` — don't batch until the end
- Use `search` before making architectural choices to check for prior decisions

### Ending a session
Reflect and write each as a separate `add_memory` call:
- What decisions were made and why
- What conventions were discovered or established
- Brief session summary (what was accomplished, what's left)

Use `/memory` for detailed guidance on writing effective memories.

## Vendored sandbox helpers

`gui/src-tauri/sandbox/landlock.py` and `gui/src-tauri/sandbox/landlock_exec.py` are vendored verbatim from dark-factory@86e54a8498fda03060c2418b4583d6d1ad4ee97d.

### Refresh procedure

```
cp /home/leo/src/dark-factory/orchestrator/src/orchestrator/agents/landlock.py gui/src-tauri/sandbox/landlock.py
cp /home/leo/src/dark-factory/orchestrator/src/orchestrator/agents/landlock_exec.py gui/src-tauri/sandbox/landlock_exec.py
# Update the VENDORED_FROM SHA header in each file to match the new commit
```

### Why not bwrap?

bwrap is **not** vendored — it is known broken on this kernel: Bun v1.3.13 + kernel 6.17 triggers a uid-map self-init segfault inside `bwrap`. Landlock sidesteps this by not using user namespaces at all.

### Sandbox scope

Landlock is FS-only — it bounds **writes**, not reads. `/etc/passwd` and other read-only paths remain readable by sandboxed processes. The sandbox prevents Claude from writing outside the designated workspace, `~/.claude`, and `/tmp`; it does not prevent exfiltration via reads or network.

**Known limitation:** `/tmp` write access is granted wholesale (`FS_V1_ALL`), which means a sandboxed Claude process can also write to other same-UID temp files under `/tmp` — including the sidecar's own MCP-config tmpdir (`reify-mcp-*`). This is an accepted v1 limitation; a future narrowing could grant writes only to a per-session tmp subdir (e.g. `mkdtempSync(…,'reify-agent-tmp-')`) but adds session-startup complexity with minimal practical security benefit given the existing trust model.

### Tauri bundling

`gui/src-tauri/tauri.conf.json` includes `bundle.resources: ["sandbox/landlock.py", "sandbox/landlock_exec.py"]` so packaged builds ship the helpers. In dev, the helpers resolve via `app.path().resource_dir()` → `target/<profile>/sandbox/`. In bundled builds they go into the AppImage/AppDir resource directory.

## TODO citation convention

Every `TODO`/`FIXME`/`HACK` comment, `todo!()`/`unimplemented!()` macro stub, and blocker `#[ignore]` reason in tracked source must cite a **live, non-terminal task** using the canonical form `#NNNN`:

### Canonical forms

```
// TODO(#4593): brief description
// FIXME(#4593): brief description
// HACK(#4593): brief description
todo!("brief description #4593")       // cite on the same line
unimplemented!("brief description")    // cite on the line directly above: // TODO(#4593):
#[ignore = "blocked on #4593 — brief description"]
```

For `todo!()`/`unimplemented!()` the cite goes **on the same macro line** or on the **line directly above** the macro call. For `#[ignore]` reasons the cite belongs inside the string.

### Banned cite forms (resolve to `malformed-cite` in PTODO)

| Form | Why banned |
|------|-----------|
| `task δ` / `task ε` / `task ζ` | Greek-letter alias — not a task ID |
| `task-5` / `step-3` | PRD-relative index — ambiguous across PRDs |
| `task 4553` / `task_4553` | Legacy prose/underscore — not the canonical `#NNNN` form |

### The one-line invariant

> Every tracked TODO/FIXME/HACK/todo!()/unimplemented!()/blocker-#[ignore] must cite a live, non-terminal task via `#NNNN`. Cited ≠ tracked — a done/cancelled cite is orphaned.

### Hard gate (as of task η, #4559)

The invariant is enforced by a **hard gate**: an `untracked`, `orphaned`, or `bare-ignore` violation makes `reify-audit --pattern PTODO` exit non-zero (exit code = High-severity count) and hard-fails the `tests/infra` verify step. `malformed-cite`, `phantom-tracking`, and `unknown-id` remain Medium (advisory, exit-neutral). `task-cites-deleted-path` stays advisory.

### Inline escape

When a source file legitimately contains a pattern string (e.g. a test that assembles `"TODO"` as a variable, or a detector source that matches `"TODO("`) that would falsely trip the PTODO sweep, add a trailing `// ptodo:allow` comment on the line:

```rust
let marker = "TODO(pending)"; // ptodo:allow — pattern-string, not a real stub
```

### References

- **Grammar**: `docs/prds/reify-audit-ptodo-detector.md` §8 (normative grammar and violation taxonomy)
- **Default sweep**: PTODO runs in the no-`--pattern` default `/audit` sweep (task ε, #4557). `untracked`/`orphaned`/`bare-ignore` emit High (hard gate, task η #4559); `malformed-cite`/`phantom-tracking`/`unknown-id` emit Medium; `task-cites-deleted-path` stays advisory. See `/audit` and `--pattern PTODO`
