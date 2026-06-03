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
- **`scripts/run-gui-dev.sh <file.ri>`** — dev-mode launch. Starts vite dev server on `:1420` (with HMR), waits for readiness, builds the cargo binary in debug profile, sets `REIFY_DEBUG=1`, and runs `target/debug/reify-gui` as a child process. `REIFY_DEBUG=1` opens an MCP debug listener on `127.0.0.1:3939` (see `gui/src-tauri/src/main.rs`). The script reaps the vite background process via an EXIT trap when reify-gui exits.

If the `reify` binary is already built, two equivalent CLI entry points work without re-running the wrapper:

- **`reify gui --debug <file.ri>`** — `--mcp` is accepted as an alias for `--debug`.
- **`reify gui-debug <file.ri>`** — sugar for `gui --debug`; both route through the same code path and propagate `REIFY_DEBUG=1` to the spawned `reify-gui` subprocess.

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
