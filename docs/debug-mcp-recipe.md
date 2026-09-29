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
# Only while another worktree's vite holds :1420 — reify-gui follows the port:
#   export REIFY_VITE_PORT=5174
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
build — set `REIFY_GUI_SKIP_PREFLIGHT=1` to bypass those two checks. An
occupied vite port is resolved by freeing it or by rerunning with
`REIFY_VITE_PORT=<free port>`: reify-gui loads whatever port the launcher gave
vite.

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

> **Concurrency: lanes can run e2e smokes at once.** Each harness or smoke run
> picks its own free vite and debug ports (`REIFY_VITE_PORT` /
> `REIFY_DEBUG_PORT`; a valid caller value is honoured), and reify-gui retargets
> `tauri.conf.json`'s `devUrl` to `REIFY_VITE_PORT` at startup
> (`gui/src-tauri/src/dev_url.rs`). `REIFY_GUI_SKIP_PREFLIGHT=1` is still not a
> remedy for an occupied port: it skips the refusal and lets the launch attach
> to the foreign listener.

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
| `wait_for_selector` | `{testId, state, viewportId?}` | `{ok}` — waits until element matches state; `viewportId` scopes the wait to one pane. Caveat: under `state:'gone'` a `viewportId` naming a pane that does not exist (unmounted, or a typo) resolves immediately — confirm the pane exists before treating a gone-wait as proof of teardown. The wait covers every match in scope: `visible` needs ANY visible match, `gone` needs EVERY match hidden or absent — so an UNSCOPED green is not proof about any one pane; see [wait_for_selector: the unscoped-wait trap](#wait_for_selector-the-unscoped-wait-trap) below |
| `list_console_errors` | `{}` | `{errors:[{message,stack}], count}` |

#### wait_for_selector: the unscoped-wait trap

A wait quantifies over EVERY match of the testid in its scope — document-wide
when unscoped, inside the named pane when scoped. `state:'visible'` holds once
SOME match is visible (and with `text`, that same match's trimmed text must
equal it); `state:'gone'` holds once EVERY match is hidden or absent. Nothing is
picked first, so a hidden copy early in document order neither blocks a
`visible` wait nor satisfies a `gone` wait on its own.

The one remaining trap: an unscoped green can come from a pane other than the
one you act on next, and the response carries no pane keys to say which. A
harness that waits unscoped and then acts scoped on a pane that is still
mounting gets a green wait and then a `notFoundForViewport` on the action. Scope
the wait whenever the follow-up action is scoped.

This subsection is the canonical statement — the tool's own `viewportId` schema
description and `buildSelectorPredicate` in `gui/src/debug/bridge.ts` each carry
the one-line rule and point here. The rule and the remaining trap are pinned by
cases (h)–(l2) of `gui/src/__tests__/waitFor.test.ts`. The drive tools
(`click_element` and friends) are a different question: they stay first-match
plus a reported `viewportId`/`matchCount`, by #5891's back-compat contract.

### I1 — Editor interaction

| Tool | Args | Returns |
|------|------|---------|
| `scroll` | `{target:'editor'\|'preview', top}` or `{testId, top, viewportId?}` | `{ok, scrollTop}` — `viewportId` applies to the testId (DOM) form only |
| `type_in_editor` | `{text}` | `{ok}` |
| `keyboard` | `{key, modifiers?}` | `{ok}` |

### I2 — Canvas interaction

All of these accept an optional `viewportId` (e.g. `'design-main'`, `'def-preview'`);
when omitted, the first populated viewport is targeted.

| Tool | Args | Returns |
|------|------|---------|
| `pick_entity_at` | `{x?, y?}` | `{hit, entityPath?, point?:{x,y,z}, distance?}` — ray-cast into 3-D viewport; omitted `x`/`y` default to canvas centre |
| `orbit_camera` | `{dazimuth?, delevation?}` | `{ok, azimuth, polar, azimuthDelta, polarDelta, camera:{position}}` — radians |
| `pan_camera` | `{dx, dy}` | `{ok, target:{x,y,z}, camera:{position}}` |
| `zoom_camera` | `{scale}` | `{ok, distance, distanceDelta, camera:{position}}` — `scale` is **multiplicative** and must be `> 0`: `<1` closer, `>1` farther |
| `set_camera` | `{position, target, up?, zoom?}` | `{ok, applied:{position, target, up, zoom}}` — `applied` is read back from the **live** camera after OrbitControls applies its constraints |
| `fit_to_view` | `{}` | `{ok}` — frames all geometry **and** establishes the orbit distance **floor** from the resulting bounds (near limit only; the far limit is a fixed absolute) |

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

## 6. screenshot → set_camera → pick → identify recipe

`/verify` and `/review` above both terminate at `screenshot` and never frame the
camera, so neither can answer *"what is that feature I can see?"*. Use this sequence
to go from a pixel in a capture to the entity behind it:

```
1. fit_to_view                → frame all geometry; ALSO sets the orbit distance
                                FLOOR from the model bounds (near limit only)
2. screenshot                 → locate the region of interest
3. set_camera({position, target})  → close in on that region
4. screenshot                 → re-capture; THIS is the frame whose pixels you may
                                address in step 5
5. pick_entity_at({x, y})     → CSS-px from the step-4 capture → {hit, entityPath, …}
6. select_entity({entityPath}) → commit the selection (pick_entity_at is query-only)
```

**Two traps this sequence is built to avoid:**

- **Pixel coordinates are only valid against the MOST RECENT screenshot.** Any camera
  move invalidates the previous capture's coordinates. Always re-`screenshot` after
  `set_camera` and read `x`/`y` off that frame. No settle step or intervening render is
  needed between `set_camera` and `pick_entity_at` — the raycast uses the live camera
  pose (`docs/debug-mcp-contract.md` §5, #6496).
- **`distanceDelta: 0` from `zoom_camera` means the request SATURATED a distance
  limit** — the dolly did nothing. It is not an error and `ok` is still `true`. Reach
  for `fit_to_view` first if you have not framed the model, since that is what derives
  the floor from its bounds; before that floor tracked the model, a fitted 75 mm
  part was held at a fixed 0.5 m floor and every dolly into it reported exactly
  `{distance: 0.5, distanceDelta: 0}`.

Compare your `set_camera` request against the returned `applied` to see whether the
controls relocated the pose. Allow a small tolerance on `applied.position` (it
round-trips through spherical coordinates and can differ by ~1 ulp); `applied.target`
is exact.

*Provenance: `found_during:dogfood:printer_v01` (2026-08-23) and the litter-tray
round-3 probe (2026-09-03).*

---

## 7. In-band error handling

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

## 8. The in-app assistant's tool surface

The GUI's Claude sidecar calls this server's tools as `mcp__reify-debug__<name>`.
Its system prompt, `gui/sidecar/src/system-prompt.ts`, advertises a curated,
design-facing subset of `tool_defs()`: tools to inspect the design, to change it
(including the five AI write tools, §3 W) and to look at the result. That file
is the list; it is not restated here.

Every other tool stays callable, because `ALLOWED_TOOLS` in
`gui/sidecar/src/session.ts` grants the whole `mcp__reify-debug__*` glob, but is
deliberately not advertised.

`gui/src/__tests__/sidecarPromptParity.test.ts` enforces the split. A new
`ToolDef` must get a row in the prompt's tool table or be added to that file's
`NOT_ADVERTISED_TO_SIDECAR`, or the gui suite goes red (see the checklist in
[debug-mcp-contract.md](debug-mcp-contract.md) §1 "Defining a new tool").

**Reachability caveat.** The sidecar reaches these tools only while the GUI runs
with `REIFY_DEBUG=1`, because `gui/src-tauri/src/main.rs` spawns the debug
server only then. Release launches (`scripts/run-gui.sh`) have none, yet the
prompt still advertises them. This is known and tracked by #7816.
