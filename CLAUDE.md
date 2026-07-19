# Reify

Parametric engineering-design DSL (`.ri` files): Rust workspace (`crates/`) + Tauri GUI (`gui/`). Dev setup: `scripts/setup-dev.sh` (builds prebuilt native deps, wires git hooks); the verify pipeline requires `sccache` on PATH (`dark-factory-orchestrator.yaml` sets `RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0` to share a rustc cache across worktrees).

This file holds **invariants and pointers only**. Mechanism detail lives in the canonical docs/scripts in the Pointers table — read those before working on a subsystem.

## Invariants

### Landing on main
- Land task branches via the orchestrator merge queue (`/merge-queue`). The only sanctioned manual path is `scripts/land.sh <task-branch>`: requires being on `main` with a clean working tree, runs a real `git merge --no-ff` so `hooks/pre-merge-commit` runs the full `--scope all --profile both` gate, and marks the main-gate sentinel.
- **Never** move `main` via `git merge --no-verify`, `git update-ref`, `git reset`, or `commit-tree` plumbing — that skips the verify gate and trips the `reference-transaction` tripwire (warn-only by default; hard-aborts under `REIFY_MAIN_GATE_ENFORCE=1`; `REIFY_MAIN_GATE_BYPASS=1` is break-glass).
- Never commit `.task/` on main.
- Claude Code's worktree feature clobbers the shared `core.hooksPath` on every worktree enter. `setup-dev.sh` wires two defenses so the gate stays live: a `<common-git-dir>/hooks → ../hooks` symlink, and per-worktree `config.worktree` overrides via `scripts/setup-main-gate-worktree-config.sh`.

### Deploying the orchestrator
- `dark-factory-orchestrator.yaml` loads **once at startup** (no hot-reload) and a dirty-start guard refuses uncommitted tracked changes in project_root (= `/home/leo/src/reify`, the MAIN checkout — not the task worktree). **Commit/land first, then restart**; a dirty restart is a crash-loop outage.
- A task running under the orchestrator must **not** `systemctl restart orchestrator-reify.service` (SIGTERMs its own agent). Use `scripts/orchestrator-redeploy-restart.sh`: schedules a detached transient unit that re-checks cleanliness and does stop-then-start after the agent exits. Never `systemctl restart` even manually — the `TimeoutStopSec=90` stop window cancels restart's start half. Modes and `ORCH_*` knobs: script header.
- Config-only changes land fast via the merge worker's trivial-pass, **but verify-pipeline files are never trivially config-only**. Consult the oracle: `bash scripts/verify-pipeline-guard.sh requires-full-gate <changed-files...>` — exit 0 means the full `--scope all` gate is required (manifest: `scripts/verify-pipeline-paths.txt`).

### Native deps
- A `manifold-csg-sys` pin bump requires re-running `scripts/build-manifold-deps.sh` (all four native kernels — OCCT, OpenVDB, gmsh, manifold — link prebuilt libs from `/opt/reify-deps`; a `links`-override in `.cargo/config.toml` skips the from-source build, scoped to `x86_64-unknown-linux-gnu` so wasm keeps FetchContent). `scripts/check-manifold-deps.sh` preflights this as the first Rust verify step.
- `scripts/build-manifold-deps.sh` also materializes a tbb-only RUNPATH pin dir at `/opt/reify-deps/tbb-pin` (a `libtbb.so.12` symlink to the deps lib) alongside the four kernels, preflighted by `scripts/check-manifold-deps.sh`; each workspace binary (`reify`, `reify-gui`) prepends it FIRST in its own RUNPATH plus a forced direct `NEEDED libtbb.so.12` (mechanism A″), so bare `./reify` / `./reify-gui` launches resolve the deps oneTBB symbols without `LD_LIBRARY_PATH`.

### Warm lanes
- One consumer per lane at a time — enforced at RECLAIM time (`thin-warm-lane.sh`, `warm-lane-gc.sh`) and, since #5223, at ACQUIRE time too (`seed-warm-lane.sh --fresh-checkout --lane-lock` holds `flock -x <lane_dir>.lock` across the reseed, refusing/queuing on a live consumer — §9.5 inv.11); `acquire_lane` always re-seeds from the base; only the `_merge-verify` lane's clean landed-commit `target/` may advance the base (`refresh-warm-base.sh --landed-commit <sha>`) — task-lane WIP must **never** advance it. Full lifecycle, invariants, and pool sizing: `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5.
- `scripts/warm-lane-audit.sh` reports each lane's assigned/free state, backing-task recoverability, and a RECLAIMABLE/LEAKED/PRESERVED-OK classification plus a pool-wide HEADROOM line — read-only, never mutates a lane, never gates dispatch/reclaim/merge. Runbook: `docs/notes/warm-lane-audit-runbook.md`.
- `warm-lane-disk-guard.sh check --soft` emits an exit-3 throttle sentinel above the existing exit-75 hard floor when free space/inodes drop below `soft_free_gib`/`soft_free_inodes`, so dispatch prefers reclaiming/reusing a FREE lane (or defers) before ENOSPC — backpressure only, never a requeue/escalation.
- `release_lane` free-first thins a released lane's divergent `target/` via `scripts/thin-warm-lane.sh` immediately on release (not just at the next acquire), so a FREE lane holds no divergent target — safe because `acquire_lane` always re-seeds from base regardless. Pool sizing/audit/admission design: `docs/prds/warm-lane-pool-sizing-lifecycle.md`.

### Per-worktree debug ports
- `scripts/setup-worktree-debug-port.sh` resolves the `reify-debug` MCP port once and writes it to BOTH `.mcp.json` and stdout (bare integer; diagnostics on stderr) — the two consumers must agree or esc-4202-61 recurs. It also `git update-index --skip-worktree`s `.mcp.json` so the ephemeral port never lands in a commit. Contract details: script header.

## TODO citation convention

Every `TODO`/`FIXME`/`HACK` comment, `todo!()`/`unimplemented!()` stub, and blocker `#[ignore]` reason in tracked source must cite a **live, non-terminal task** in the canonical form `#NNNN` — e.g. `// TODO(#NNNN): …`, `todo!("… #NNNN")` (same line or the line directly above), `#[ignore = "blocked on #NNNN — …"]`. Cited ≠ tracked: a done/cancelled cite is orphaned. Greek-letter aliases (`task ε`), PRD-relative indices (`task-5`), and prose forms (`task 4553`) resolve to `malformed-cite`. Hard gate: `untracked`/`orphaned`/`bare-ignore` fail `reify-audit --pattern PTODO` and the tests/infra verify step. Escape a legitimate pattern string with a trailing `// ptodo:allow — reason`. Grammar and taxonomy: `docs/prds/reify-audit-ptodo-detector.md` §8.

## GUI launch

- `scripts/run-gui.sh <file.ri>` — release-mode build+launch (what end users run). `scripts/run-gui-dev.sh <file.ri>` — vite dev server + debug binary with `REIFY_DEBUG=1`, which opens an MCP debug listener on `127.0.0.1:${REIFY_DEBUG_PORT:-3939}`; set `REIFY_DEBUG_PORT` per worktree to avoid collisions. Both export OCCT's `LD_LIBRARY_PATH` automatically.
- With a built binary: `reify gui --debug <file.ri>` (`--mcp` is an alias) or `reify gui-debug <file.ri>`.

## Memory & tasks

- All task operations go through **fused-memory MCP tools** (never the Taskmaster CLI/MCP directly), with `project_root: "/home/leo/src/reify"`. Status transitions trigger reconciliation automatically.
- Memory writes: prefer `add_memory` (distilled fact) over `add_episode` (full extraction pipeline); always tag `project_id: "reify"` and a descriptive `agent_id`. Write decisions immediately, not at session end; `search` before architectural choices. Session start: `search(query="project overview and current status", project_id="reify")` + `get_tasks`; execute a task's `memory_hints` when present. See `/memory` for detailed guidance.
- The cargo/rustc test+build output condensation wrapper (skim; the PreToolUse hook that rewrites `cargo test`/`cargo build` to condensed `PASS: N | FAIL: M | SKIP: K` / `BUILD OK | warnings: N | errors: N` output) and its bypasses (`--no-run --message-format=json`, absolute cargo path, `env cargo`, etc.) is a well-established gotcha with a canonical `procedural_knowledge` memory entry — it has been independently rediscovered and re-written as a near-duplicate memory 6 times. Do not cite its Mem0 UUID here; it changes on every consolidation round. Before assuming build/test output is broken or writing a new memory about it, run `search(project_id="reify", categories=["procedural_knowledge"], query="cargo skim output wrapper condensation")`.

## Vendored sandbox helpers

`gui/src-tauri/sandbox/{landlock,landlock_exec}.py` are vendored verbatim from dark-factory (commit in each file's `VENDORED_FROM` header; refresh = `cp` from `/home/leo/src/dark-factory/orchestrator/src/orchestrator/agents/` + bump the header). Landlock bounds **writes only** — reads and network are unbounded, and `/tmp` is writable wholesale (accepted v1 limitation). bwrap is deliberately not used: Bun 1.3.13 + kernel 6.17 segfaults in bwrap's uid-map self-init. `tauri.conf.json bundle.resources` ships the helpers into packaged builds; dev resolves them via `resource_dir()`.

## Pointers

| Topic | Canonical source |
|---|---|
| Verify-pipeline admission gates, knobs, clock-stop markers, agent-spawn CPU axis | `docs/notes/verify-pipeline-knobs.md` (operational digest); PRDs `verify-admission-wait-clock-stop.md` (authoritative), `cpu-load-admission-control.md` |
| Warm-lane CoW pool lifecycle & invariants | `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5; sizing, audit & admission: `docs/prds/warm-lane-pool-sizing-lifecycle.md` |
| Warm-lane audit CLI, output fields & run cadence | `docs/notes/warm-lane-audit-runbook.md` (operational digest); `scripts/warm-lane-audit.sh` header |
| Orphaned test-binary reaper (two layers + `REIFY_REAPER_*` knobs) | `docs/notes/orphaned-test-binary-reaper.md`; `scripts/lib_proc_reaper.sh` |
| Orchestrator safe-restart modes & knobs | `scripts/orchestrator-redeploy-restart.sh` header |
| Debug-port provisioning contract | `scripts/setup-worktree-debug-port.sh` header |
| PTODO grammar & violation taxonomy | `docs/prds/reify-audit-ptodo-detector.md` §8 |
| sccache / cross-worktree build-cache design | `~/.claude/plans/playful-hopping-nygaard.md` |

Cross-repo seams follow one pattern: **reify ships the primitive, dark-factory wires the invocation** (debug-port trigger, cpu-governance ζ, reaper sweep, warm-lane consumers, verify-pipeline-guard consult). Each seam's contract lives with its topic above.
