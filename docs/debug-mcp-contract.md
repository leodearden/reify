# reify-debug MCP Contract

*Task 4293 (τ0) — maintained alongside the boundary tests in
`gui/src/__tests__/debugContract.test.ts` and
`gui/src-tauri/src/tests/debug_boundary_tests.rs`.*

## How this contract is validated

| Section | Guarding test |
|---------|--------------|
| §1 Tool-def → dispatch → handler wiring | [step-3] `debugContract.test.ts` — error-envelope + wiring |
| §2 JSON error envelope | [step-3] same file |
| §3 Coordinate convention | [step-5] `debugContract.test.ts` — coordinate convention |
| §4 Synthetic-event fidelity gaps | [step-7] `debugContract.test.ts` — pick↔raycast |
| §5 pick\_entity\_at ↔ raycast convention | [step-7] same file |

The Rust transport seam (query\_frontend ↔ resolve round-trip) is validated
separately by `gui/src-tauri/src/tests/debug_boundary_tests.rs` (steps 1–2).

---

## §1 Tool-def → dispatch → handler wiring

### Defining a new tool

A new frontend-mediated tool requires three coordinated changes:

1. **`gui/src-tauri/src/debug_server.rs` — `tool_defs()`**
   Add a `ToolDef { name, description, input_schema }` entry so the tool
   appears in MCP `tools/list` responses.

2. **`gui/src-tauri/src/debug_server.rs` — `dispatch_tool()`**
   The default arm delegates every unrecognised name to
   `DebugBridge::query_frontend(name, params)`.  Purely engine-side tools
   (e.g. `engine_state`, `mesh_stats`) add a named match arm instead.

3. **`gui/src/debug/bridge.ts` — `buildHandlers()`**
   Add a `command_name: (params) => result` entry in the handler map.
   The handler receives the JSON params object and returns either a value
   or a `{error: string}` envelope (see §2).

### Dispatch flow

```
MCP client
  → POST /mcp  { method:"tools/call", params:{name, arguments} }
      → dispatch_tool(state, name, args)
          → if engine-only arm matches: run directly in Rust
          → else (default arm): DebugBridge::query_frontend(name, args)
              → emits Tauri event "debug-request" { id, command, params }
                  → JS bridge (gui/src/debug/bridge.ts) listen handler
                      → buildHandlers()[command](params)
                      → invoke('debug_response', { id, result: JSON.stringify(result) })
              → DebugBridge::resolve(id, json) wakes the waiting oneshot
              → returns serde_json::from_str(json) : Value
  → MCP tool-result content block (text or image)
```

The `id` is a monotonically incrementing u64 assigned by `DebugBridge::next_id`
that pairs each request with exactly one response via a per-request oneshot
channel (see `gui/src-tauri/src/debug.rs`).

---

## §2 JSON error envelope

Three distinct error shapes exist depending on which layer the error originates.

### 2a — Frontend in-band `{error: string}`

**Source:** `gui/src/debug/bridge.ts`, handler functions inside `buildHandlers()`.

**Shape:** `{ "error": "<message>", ...optional extra fields }`

**Examples:**
```jsonc
// Unknown command (bridge dispatch):
{ "error": "unknown command: pick_entity_at" }

// Missing required parameter:
{ "error": "selector is required" }

// Invalid CSS selector (try/catch):
{ "error": "Failed to execute 'querySelector' on 'Document': ':::' is not a valid selector" }

// Screenshot too large (with extra fields):
{ "error": "screenshot too large", "size": 17825792, "limit": 16777216 }

// Viewport not ready:
{ "error": "viewport not ready" }
```

The Rust transport passes this object through verbatim: the JSON string
returned by the JS bridge is parsed by `DebugBridge::resolve` →
`serde_json::from_str`, so any extra fields survive intact.

**Guarded by:** `debugContract.test.ts` §error-envelope + wiring (step-3),
which asserts the exact `error` field for unknown-command and missing-param
cases, and confirms that invalid-selector produces `typeof result.error === 'string'`.

### 2b — Rust handler `Err(String)` → MCP `isError: true`

**Source:** `dispatch_tool()` in `debug_server.rs`, when a named Rust arm returns
`Err(e)` or when `DebugBridge::query_frontend` itself fails (timeout, channel drop,
JSON parse error).

**MCP wire shape (tools/call response):**
```jsonc
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "content": [{ "type": "text", "text": "Error: <e>" }],
    "isError": true
  }
}
```

The `isError: true` flag tells the MCP client that the tool invocation failed.
The error text is `"Error: "` + the Rust `String` from the `Err` variant.

**Source lines:** `debug_server.rs:802-811`.

### 2c — JSON-RPC method error

**Source:** `debug_server.rs`, unknown `method` field in the JSON-RPC request.

**Shape:**
```jsonc
{
  "jsonrpc": "2.0",
  "id": <id>,
  "error": { "code": -32601, "message": "method not found: <method>" }
}
```

This is a JSON-RPC 2.0 protocol error (not a tool-result error).  MCP clients
treat `response.error` as a transport-level failure, distinct from `isError:true`
inside a tool result.

**Source lines:** `debug_server.rs:815`.

### Summary table

| Origin | Shape | `isError` |
|--------|-------|-----------|
| JS bridge handler | `{ "error": "…" }` inside tool-result text | ✗ (not set) |
| Rust Err(String) | `{ content:[…], isError:true }` | ✓ |
| Unknown JSON-RPC method | `{ error: { code, message } }` | n/a (protocol layer) |

---

## §3 Coordinate convention

### Pixel frame

Every pixel tool uses **CSS logical pixels measured from the viewport (window) top-left**.
This is the same frame as `Element.getBoundingClientRect()` and `MouseEvent.clientX/Y`.

```
(0, 0) ──────────────────────────────► x  (clientX / rect.left / rect.x)
  │
  │   viewport origin
  │
  ▼
  y  (clientY / rect.top / rect.y)
```

- `get_layout_metrics(selector)` returns `bounds: { x, y, width, height }` where
  `x = rect.left` and `y = rect.top` from `getBoundingClientRect()`.
- `get_window_state()` returns `devicePixelRatio` (a number) so callers can convert
  CSS pixels to physical device pixels when needed (e.g. for canvas pixel-level ops).

### Canonical round-trip

```
bounds = get_layout_metrics(selector).bounds
center = { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 }
click_at(center)      ← I1 (future tool, wraps synthetic PointerEvent at clientX/clientY)
  → element's JS click handler fires with event.clientX === center.x
```

**Guarded by:** `debugContract.test.ts` §coordinate-convention (step-5),
which stubs `getBoundingClientRect` to `{x:100, y:50, width:80, height:40}`,
verifies `get_layout_metrics.bounds === {x:100, y:50, width:80, height:40}`,
then proves the derived center `(140, 70)` fires the element's click handler.

### Notes

- This convention is validated **arithmetically** in the unit tests.  The live
  `document.elementFromPoint(centerX, centerY)` hit-test (OS layout + compositing)
  is deferred to `click_at` (I1)'s real-GUI e2e scenario, which depends on H0 and
  the complete viewport being rendered.
- The canvas (viewport) coordinate frame is the same CSS-pixel frame: the NDC
  conversion in `createSelection` uses `rect = canvas.getBoundingClientRect()` as
  its origin (see §5).
- Downstream tools that introduce a new pixel frame MUST document the conversion
  and add a boundary test before landing.

---

## §4 Synthetic-event fidelity gaps

Synthetic `PointerEvent` / `MouseEvent` dispatch (via `element.dispatchEvent(...)`)
**fires JS event handlers** but has the following known gaps relative to real user
input:

| Capability | Synthetic events | Real user input |
|------------|-----------------|-----------------|
| JS event handlers | ✓ fires | ✓ fires |
| CSS `:hover` pseudo-class | ✗ NOT applied | ✓ applied |
| CSS `:active` pseudo-class | ✗ NOT applied | ✓ applied |
| Native drag-and-drop (`dragstart`, `drop`) | ✗ not triggered | ✓ triggered |
| OS / compositor hit-testing (`elementFromPoint`) | ✗ not involved | ✓ involved |
| `focus` / `blur` side-effects (click on input) | partial — only if `focus()` called explicitly | ✓ automatic |

**Practical implication:** tools that dispatch synthetic events can assert that
JS-registered handlers fire (click handlers, React `onClick`, Three.js pointer
listeners, etc.) but **cannot** assert CSS pseudo-class styling changes.  Tests
must reflect this: they assert handler invocation, not visual state.

This gap is accepted by the PRD (§0/§3 G6): the debug tools are designed for
programmatic control of application logic, not pixel-perfect CSS rendering
verification.

**Guarded by:** `debugContract.test.ts` §pick↔raycast (step-7), which drives the
real `createSelection` raycaster (a JS handler) and asserts `onSelect` was called —
not that any CSS changed.

---

## §5 pick\_entity\_at ↔ raycast convention

`pick_entity_at` (I2, future tool) answers: *which Three.js entity is under screen
pixel (clientX, clientY)?*  It wraps the **same** `createSelection` + `Raycaster`
path that is used for interactive mouse selection.

### NDC formula

Given a canvas whose `getBoundingClientRect()` returns `rect`:

```
NDC.x =  ((clientX - rect.left) / rect.width)  * 2 - 1
NDC.y = -((clientY - rect.top)  / rect.height) * 2 + 1
```

Source: `gui/src/viewport/selection.ts` — `computeNDC()`.

### Pick pipeline

```
screen pixel (clientX, clientY)
  → computeNDC(event, rect)           # CSS-pixel → [-1, +1] NDC
  → raycaster.setFromCamera(ndc, camera)
  → raycaster.intersectObjects(meshes)
  → intersections[0].object.name     # entity path string, or null if empty
```

`Mesh.prototype.raycast` is patched with `three-mesh-bvh`'s `acceleratedRaycast`
(see `gui/src/viewport/selection.ts:28`).  If no BVH tree has been built for the
geometry, `acceleratedRaycast` falls back to the standard Three.js face traversal
transparently — no caller change is needed.

### Query-only guarantee

`pick_entity_at` is **query-only**: it does NOT mutate selection state, fire
`onSelect`, or trigger any side-effects.  Its return value is the entity path string
(or `null`) that the raycaster would resolve for the given screen coordinate.

### Validation

**Guarded by:** `debugContract.test.ts` §pick↔raycast (step-7), which builds a
real `PerspectiveCamera` at `(0, 0, 5)` looking toward the origin, places a
`BoxGeometry(1,1,1)` mesh named `'entity/box'` at the origin, and drives
`createSelection` with:
- canvas center `(400, 300)` → NDC `(0, 0)` → ray along `-Z` → hits box →
  `onSelect('entity/box')`
- far corner `(5, 5)` → NDC `≈ (-0.988, +0.983)` → misses box →
  `onSelect(null)`

This pins the screen→NDC→raycast convention before `pick_entity_at` (I2) is
built on top of it.
