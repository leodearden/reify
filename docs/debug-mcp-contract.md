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
