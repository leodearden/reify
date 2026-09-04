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
| `get_diagnostics` | `{}` | `{compile:[], compileCount, lsp:[], lspCount}` |
| `ui_outline` | `{}` | `{outline:[…], count}` — rendered DOM tree summary |

### R3 — Selectors & console

| Tool | Args | Returns |
|------|------|---------|
| `wait_for_selector` | `{testId, state, viewportId?}` | `{ok}` — waits until element matches state; `viewportId` scopes the wait to one pane. Caveat: under `state:'gone'` a `viewportId` naming a pane that does not exist (unmounted, or a typo) resolves immediately — confirm the pane exists before treating a gone-wait as proof of teardown |
| `list_console_errors` | `{}` | `{errors:[{message,stack}], count}` |

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
