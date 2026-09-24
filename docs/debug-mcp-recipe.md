# reify-debug MCP Recipe

*How to wire the reify-debug MCP tools into /verify and /review GUI workflows.*

For the coordinate/transport/error-envelope contract see
[docs/debug-mcp-contract.md](debug-mcp-contract.md). This doc covers workflow
recipes and tool catalogue; it does not repeat the contract.

---

## 1. Boot the debug server

```bash
# Dev mode (HMR + debug listener on REIFY_DEBUG_PORT, default :3939)
scripts/run-gui-dev.sh path/to/fixture.ri

# Per-worktree port isolation (prevents collision with other worktrees)
port=$(scripts/setup-worktree-debug-port.sh)
export REIFY_DEBUG_PORT=$port
scripts/run-gui-dev.sh path/to/fixture.ri
```

The debug server accepts MCP `tools/call` JSON-RPC on `http://127.0.0.1:${REIFY_DEBUG_PORT:-3939}/mcp`.

The launcher now self-defends against a hostile environment, so the
`env -u LD_LIBRARY_PATH WEBKIT_DISABLE_DMABUF_RENDERER=1 scripts/run-gui-dev.sh ...`
prefix that used to be required is no longer needed: it preserves an inherited
`LD_LIBRARY_PATH` but prepends `/opt/reify-deps/tbb-pin` ahead of it (the loader
searches `LD_LIBRARY_PATH` before `DT_RUNPATH`, so an inherited `/usr/lib` path
would otherwise bind system libtbb 12.11 over the deps 12.18), and it defaults
`WEBKIT_DISABLE_DMABUF_RENDERER=1` itself. It also preflights the display and
the vite port *before* the build, so a headless shell or a port another worktree
already serves fails in milliseconds instead of after a multi-minute cargo
build — set `REIFY_GUI_SKIP_PREFLIGHT=1` to bypass those two checks.

---

## 2. Run the e2e value-assertion suite

```bash
# From repo root — runs all VALUE_SCENARIOS against a live reify-gui
npm --prefix gui run test:e2e
# equivalently: tsx gui/test/visual/run.ts value
```

The suite boots reify-gui automatically via `scripts/run-gui-dev.sh`, runs all
`VALUE_SCENARIOS` from `gui/test/visual/assertions.ts`, and exits 0 (all pass) /
1 (any fail) / 2 (fatal harness error). **Not CI-gated** — needs a live GUI per
PRD §4.10/§5. Run manually or from a /verify session with a real reify-gui.

> **Concurrency: the e2e smoke needs an unoccupied `:1420`, so two lanes cannot
> run it at once.** Since #7254 the launcher refuses (exit 1, before any build)
> when something already answers on the vite port, and neither
> `gui/test/visual/run.ts` nor `gui/test/visual/lib_e2e_smoke.sh` sets
> `REIFY_VITE_PORT` — nor could they usefully: reify-gui's `devUrl` is baked to
> `http://localhost:1420` at compile time (`gui/src-tauri/tauri.conf.json`), so
> moving vite would leave the GUI loading the *foreign* listener. The refusal is
> the correct behaviour — previously the second run silently attached to the
> first lane's vite and asserted against another worktree's build — but the
> consequence is a real serialisation constraint: **serialise concurrent e2e
> smokes across lanes, or free `:1420` first** (the error names the listener pid
> and `ls -l /proc/<pid>/cwd` shows which worktree it serves). Lifting it needs
> the GUI-side half — a build-time `devUrl` override via `TAURI_CONFIG`, or
> reading the env var in the Rust shell — as noted in `scripts/run-gui-dev.sh`'s
> `REIFY_VITE_PORT` comment. `REIFY_GUI_SKIP_PREFLIGHT=1` bypasses the check but
> restores the silent-wrong-vite behaviour, so it is not a fix.

### The AI-write integration gate (task 5098)

```bash
# From repo root — drives printer_v01's Y-rail lengthening through reify_set_parameter
npm --prefix gui run test:smoke:rail-lengthening
```

Self-launching, like the other `test:smoke:*` runners in `gui/package.json`
(that file is the list — this section names only the one gate). It exercises
PRD `ai-native-editing.md` §7 rows B1–B3, B5 and B7 end to end: two
`reify_set_parameter` edits (`CoreXY.y_rail_len`, then `AFrame.rail_span_m`,
both 800mm → 1100mm), asserting the viewport and property panel follow WITHOUT
a file reload, the `.ri` on disk carries the new default literal
unit-preservingly, the rail-span pin flips `violated` and back to `satisfied`,
the post-debounce watcher re-read adds no churn, and two refused writes leave
disk byte-identical. **Not CI-gated** — needs a live GUI, same as §2.

**It drives a COPY.** `reify_set_parameter` rewrites the `.ri` source on disk,
which is the point of the on-disk assertion, so the runner copies
`prj/printer_v01/` to a `mkdtemp` dir and removes it in a `finally`. The tracked
design is never the subject; a crashed run leaves no half-edited engineering
model behind.

**What it does and does not claim.** It asserts the GUI value-flow chain on the
two edited cells and their LIVE DEPENDENTS — `AFrame.travel_avail` (510mm →
810mm), the two named constraint pins, and the fields beyond `meshes`/`values`.
It does **not** claim printer_v01 re-derives as a whole, and three known places
do not follow the rails: the tendon web's `rail_half` is its own `400mm` literal
hand-kept equal to `BearingRod.length / 2` (`printer.ri`), the rear-web spans are
`#6592`-inert (per-instance sizing does not thread to the sub-bearing level), and
the interim socket bridges INVERT at 1100mm rails — `brf_y1` is independent of
`rail_span_m`, so the box depth goes negative rather than merely to zero, which
takes `AFrame.vol_vs_analytic` `indeterminate`. A green run means the value-flow
chain carried the edit, not that the design is consistent at the new length.

**B1 is an ORDERING property, not a payload property.** "Without a file reload"
holds only because every read that observes engine-held state precedes the
phase's own `reify_open_file`, which re-reads the file from disk
(`open_path_into_engine`, `debug_server.rs:1525`). Hoist any read above them and
the row passes whatever the write did — silently, in the direction that PASSES.
Two things enforce the order: `observeThenExtras`' thunk seam in
`railLengtheningGate.mjs` (runtime, vitest-covered) and the
`awaited-extras-literal` convention in `smokeDriverConventions.ts` (source-level,
CI-gated); `observeThenExtras`' docblock is where the mechanism is derived. Edit
the driver with both in view.

Its decision function is pure and IS CI-gated, separately:
`gui/test/visual/railLengtheningGate.mjs` is covered by
`railLengtheningGate.test.ts` on every verify run, so a regression in what the
gate *decides* is caught without a GUI — only the live *execution* needs one.

---

## 3. Tool catalogue by group

### R1 — State inspection

| Tool | Args | Returns |
|------|------|---------|
| `store_state` | `{}` | Full Solid store snapshot (`engine`, `editor`, `selection`, …) |
| `get_window_state` | `{}` | `{devicePixelRatio, innerWidth, innerHeight, …}` |
| `get_layout_metrics` | `{selector}` | `{exists, width, height, overflow:{horizontal,vertical}}` |

### R2 — Diagnostics & outline

| Tool | Args | Returns |
|------|------|---------|
| `get_diagnostics` | `{}` | `{compile:[], tessellation:[], compileCount, tessellationCount}` |
| `ui_outline` | `{}` | `{outline:[…], count}` — rendered DOM tree summary |

### R3 — Selectors & console

| Tool | Args | Returns |
|------|------|---------|
| `wait_for_selector` | `{testId, state, viewportId?}` | `{ok}` — waits until element matches state; `viewportId` scopes the wait to one pane. Caveat: under `state:'gone'` a `viewportId` naming a pane that does not exist (unmounted, or a typo) resolves immediately — confirm the pane exists before treating a gone-wait as proof of teardown. Caveat: an UNSCOPED wait is not proof about any one pane in either direction — see [wait_for_selector: the unscoped-wait trap](#wait_for_selector-the-unscoped-wait-trap) below |
| `list_console_errors` | `{}` | `{errors:[{message,stack}], count}` |

#### wait_for_selector: the unscoped-wait trap

An unscoped wait resolves the testid to the FIRST element in document order and
evaluates the state on THAT one — not on the first element that SATISFIES the
wait. The selection happens BEFORE the state is consulted, which gives the trap
three faces, one per arm of the predicate:

1. it goes green off a pane you did not mean, and the response carries no pane
   keys to say which;
2. `state:'visible'` times out on a hidden first match while a visible copy sits
   in a LATER pane;
3. `state:'gone'` goes green off a first match that is merely HIDDEN while a
   visible copy is still mounted in a later pane — a teardown reported that did
   not happen.

Face 3 is the one to fear: face 2 fails loudly (a timeout the caller has to look
at), while face 3 hands back a green for a teardown that never happened. Scope
the wait whenever the follow-up action is scoped.

This subsection is the canonical enumeration — the tool's own `viewportId` schema
description and `buildSelectorPredicate` in `gui/src/debug/bridge.ts` each carry
the one-line rule and point here. The three faces are pinned as behaviour by
cases (h)/(i)/(j) of `gui/src/__tests__/waitFor.test.ts`. Known limitation rather
than intended behaviour — the fix (quantify the unscoped predicate over ALL
matches, on the observe path only; the drive tools stay first-match by #5891's
back-compat contract) is tracked by #6564.

### I1 — Editor interaction

| Tool | Args | Returns |
|------|------|---------|
| `scroll` | `{target:'editor'\|'preview', top}` or `{testId, top, viewportId?}` | `{ok, scrollTop}` — `viewportId` applies to the testId (DOM) form only |
| `type_in_editor` | `{text}` | `{ok}` |
| `keyboard` | `{key, modifiers?}` | `{ok}` |

### I2 — Canvas interaction

| Tool | Args | Returns |
|------|------|---------|
| `pick_entity_at` | `{x?, y?}` | `{hit, entityPath?}` — ray-cast into 3-D viewport |
| `orbit_camera` | `{dazimuth?, delevation?}` | `{ok, azimuthDelta, elevationDelta}` |
| `pan_camera` | `{dx, dy}` | `{ok}` |
| `zoom_camera` | `{delta}` | `{ok}` |

### C1 — Chrome & menus

| Tool | Args | Returns |
|------|------|---------|
| `open_menu` | `{name}` | `{ok, open}` — clicks `[data-testid=menu-trigger-<name>]` |
| `click_element` | `{testId, viewportId?}` | `{ok}` — `viewportId` picks which pane's control to click |

### C2 — Layout

| Tool | Args | Returns |
|------|------|---------|
| `resize_panes` | `{editorWidth?}` | `{ok, layout:{editorWidth,…}}` — writes layoutStore (L0) |
| `get_computed_style` | `{selector, property}` | `{value}` |
| `expand_tree_node` | `{path, panel?}` | `{ok, path, expanded}` — `panel` selects `'design'` (default) or `'constraint'`; idempotent, no click dispatched if already expanded. In the constraint panel a non-expandable row never toggles, so requesting expansion still dispatches a click but `expanded` comes back `false` — detect the no-op by checking `expanded === true`. `panel:'constraint'` — any dispatched click also fires the row's `onConstraintSelect`, so the current constraint selection changes as a side effect |
| `collapse_tree_node` | `{path, panel?}` | `{ok, path, expanded}` — `panel` selects `'design'` (default) or `'constraint'`; idempotent, no click dispatched if already collapsed. `panel:'constraint'` — any dispatched click also fires the row's `onConstraintSelect`, so the current constraint selection changes as a side effect |

### F1 — Fixtures & state injection

| Tool | Args | Returns |
|------|------|---------|
| `load_fixture` | `{name}` | `{ok}` — loads a named fixture from debug_server.rs catalogue |
| `open_file` | `{path}` | `{ok}` — opens an arbitrary .ri path |
| `inject_diagnostics` | `{diagnostics:[…], source}` | `{ok}` |
| `reset_app_state` | `{}` | `{ok}` — clears openFiles + selection |
| `element_screenshot` | `{testId, viewportId?}` | `{data}` — base64 PNG of a single element; `viewportId` picks which pane to crop. A call matching more than one element also returns `{viewportId, matchCount}` naming the pane it guessed — scoped or not, since a testId can repeat within one pane — delivered over MCP as a second `text` content block after the image |
| `screenshot` / `screenshot_window` | `{}` | `{data}` — full viewport PNG |

### F2 — LSP probes

| Tool | Args | Returns |
|------|------|---------|
| `hover_at` | `{line, col}` | `{markdownLength}` |
| `completion_at` | `{line, col}` | `{itemCount, items:[…]}` |
| `definition_at` | `{line, col}` | `{range:{start,end}, uri}` |

### W — AI write tools (task 5097)

| Tool | Args | Returns |
|------|------|---------|
| `reify_set_parameter` | `{cell_id, value}` | `{success, new_value, unit, diagnostics}` — `value` is a unit-bearing literal (`'120mm'`); rewrites the parameter's default literal in the `.ri` on disk |
| `reify_update_source` | `{file_path, content}` | `{success, diagnostics_count, diagnostics}` — active file only, in memory; writes no disk |
| `reify_open_file` | `{file_path}` | `{success, source}` |
| `reify_save_file` | `{file_path?}` | `{success}` — saves the active file when `file_path` is omitted |
| `reify_export` | `{format, output_path}` | `{success, path}` — `format` is `step`, `stp` or `stl` |

Their write semantics are specified in
[debug-mcp-contract.md](debug-mcp-contract.md) §0 "AI write tools" and are not
restated here.

---

## 4. /verify recipe

Use this sequence to verify a change to the GUI in a live session:

```
1. open_file / load_fixture   → load the fixture under test
2. wait_for_idle              → wait for engine + renderer to settle
3. store_state                → assert engine.meshCount, selection, openFiles
4. get_diagnostics            → assert no unexpected compile/LSP errors
5. ui_outline                 → assert expected DOM structure is present
6. screenshot / element_screenshot  → visual sanity check
```

**In-band error detection:** `wait_for_idle` may return `{error:'timeout'}` or
`{error:'engine_phase', phase:'…'}` if the renderer/engine is stuck. These are
surfaced as `ok:false` by `parseRpcResponse` (see `gui/test/visual/rpc.ts` and
`docs/debug-mcp-contract.md §2a`), so a stuck engine is caught immediately.

**Running the full suite:**
```bash
npm --prefix gui run test:e2e
```

---

## 5. /review recipe

Use this sequence to review layout/diagnostic regressions:

```
1. load_fixture               → load the fixture being reviewed
2. wait_for_idle              → settle
3. ui_outline                 → inspect DOM structure for unexpected nodes
4. get_layout_metrics         → check for overflow (overflow.horizontal/vertical)
5. list_console_errors        → assert count === 0 (or known baseline)
6. screenshot                 → full-viewport visual capture
```

For per-element capture when reviewing a specific component:
```
element_screenshot({testId: 'diagnostics-dialog'})
```

---

## 6. In-band error handling

Debug handlers return failures as `Ok({error: "<msg>", …})` — no MCP `isError`
flag is set. `parseRpcResponse` in `gui/test/visual/rpc.ts` detects this via the
`inBandError(v)` helper (non-null object with a string `.error` field) and maps
it to `{ok: false, error}`.

Known in-band error strings from `wait_for_idle`:
- `"timeout"` — renderer did not settle within `timeout_ms`
- `"engine_phase"` — engine is in an error phase (`.phase` field gives details)
- `"engine_not_started"` — engine has not been initialised

See [docs/debug-mcp-contract.md](debug-mcp-contract.md) §2a for the full
transport and error-envelope specification.

---

## 7. The in-app assistant's tool surface

The GUI's Claude sidecar calls this server's tools as `mcp__reify-debug__<name>`.
Its system prompt, `gui/sidecar/src/system-prompt.ts`, advertises a curated,
design-facing subset of `tool_defs()`: tools to inspect the design, to change it
(including the five AI write tools, §3 W) and to look at the result. That file
is the list; it is not restated here.

Every other tool stays callable, because `ALLOWED_TOOLS` in
`gui/sidecar/src/session.ts` grants the whole `mcp__reify-debug__*` glob, but is
deliberately not advertised.

`gui/src/__tests__/sidecarPromptParity.test.ts` enforces the split. A new
`ToolDef` must be named in the prompt or added to that file's
`NOT_ADVERTISED_TO_SIDECAR`, or the gui suite goes red (see the checklist in
[debug-mcp-contract.md](debug-mcp-contract.md) §1 "Defining a new tool").

**Reachability caveat.** The sidecar reaches these tools only while the GUI runs
with `REIFY_DEBUG=1`, because `gui/src-tauri/src/main.rs` spawns the debug
server only then. Release launches (`scripts/run-gui.sh`) have none, yet the
prompt still advertises them. This is known and tracked by #7816.
