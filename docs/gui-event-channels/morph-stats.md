# Per-Channel Spec: `morph_stats` (debug-MCP RPC)

> **Channel type:** Debug-MCP RPC (request/response) — not a Tauri event channel.
> This document adapts the per-channel template from
> [`_template.md`](./_template.md) for the RPC variant.
>
> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md)
> §2.3 (audit finding M-013) + §9 task θ (task 3547).
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §3 `morph_stats` row
> (committed in task 3536 / α — no lockstep update needed here).

---

## 1. RPC name + handler/consumer locations

- **RPC name:** `morph_stats` (snake_case per PRD §3 RPC convention)
- **MCP tool name:** `mcp__reify-debug__morph_stats`
- **Rust handler:** `gui/src-tauri/src/debug_server.rs` — `handle_morph_stats(_params: Value)`
- **Tool registry:** `gui/src-tauri/src/debug_server.rs` — `tool_defs()` (entry adjacent to `mesh_stats`)
- **Stats accessor:** `crates/reify-mesh-morph/src/stats.rs` — `snapshot() -> MorphStats`
- **MCP client:** Claude debug session (`mcp__reify-debug__morph_stats`)

---

## 2. Request shape + Response shape

**Request:** `()` (no args) or `{ body_id: String }` (optional — currently ignored; see §3).

```json
{}
```

or

```json
{ "body_id": "Bracket.body" }
```

**Response (Rust `MorphStats` struct):**

```rust
/// Mesh-morph runtime statistics DTO.
/// Response shape for the `morph_stats` debug-MCP RPC.
/// Per PRD §3.2 field names match exactly — no `#[serde(rename_all)]`.
/// `last_rejection_reason` is serialised `skip_serializing_if = "Option::is_none"`,
/// so it is absent from the JSON payload when no rejection has been recorded.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct MorphStats {
    pub morph_count: u32,
    pub remesh_count: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_rejection_reason: Option<String>,
}
```

**Response (TypeScript `MorphStats` interface, `gui/src/types.ts`):**

```typescript
/**
 * Mesh-morph runtime statistics — response shape for the
 * `morph_stats` debug-MCP RPC (GR-016 / docs/prds/v0_3/gui-event-channel-inventory.md §2.3).
 */
export interface MorphStats {
  morph_count: number;
  remesh_count: number;
  last_rejection_reason?: string;
}
```

---

## 3. RPC handler site and invocation trigger

- **Handler:** `gui/src-tauri/src/debug_server.rs::handle_morph_stats(_params: Value)`
- **Trigger:** Handler invoked on MCP `tools/call` for tool name `morph_stats`; dispatched from
  `dispatch_tool(state, name, params)` via the `"morph_stats" => handle_morph_stats(params).await`
  arm. No `run_on_engine` lock is taken — morph stats are process-global, not engine state.
- **Stats population:** Counters are incremented by:
  - `record_morph_attempt()` — increments `morph_count`
  - `record_remesh()` — increments `remesh_count`
  - `record_rejection(reason)` — sets `last_rejection_reason` (latest-wins overwrite)
  
  Call sites are wired by mesh-morphing PRD tasks #2947-#2949 (currently pending). Until those
  tasks land, the RPC returns zero counters from real engine activity, but the infrastructure is
  fully testable in isolation by calling recorder functions directly.

- **`body_id` parameter:** Accepted in the JSON schema (optional string) but intentionally
  ignored by the handler. Per-body filtering is deferred to the engine-wiring tasks (2947-2949).
  Both the `()` and `{ body_id: ... }` request forms return the same global snapshot. This is a
  forward-compatible API — existing MCP clients that don't pass `body_id` work today; clients
  that pass it also get the correct response today and a non-breaking upgrade path when
  per-body filtering lands.

---

## 4. Consumer site

- **Consumer:** Claude debug-MCP session via `mcp__reify-debug__morph_stats` tool call.
- **No `bridge.ts` wrapper:** This is an RPC response shape, not a Tauri event payload. There is
  no frontend `onMorphStats(callback)` listener. The response flows directly from the debug-MCP
  server (axum on `127.0.0.1:${REIFY_DEBUG_PORT:-3939}`) to the MCP client.
- **No frontend subscription:** Not subscribed by any panel or store (RPC, not push event).

---

## 5. Versioning policy

Default per PRD §3.3: no `version` field. Lockstep Rust+TS for breaking changes. Field names
are stable per PRD §3.2 (no `#[serde(rename_all)]`).

---

## 6. Error semantics

- **Handler error:** `handle_morph_stats` returns `Err(String)` on `serde_json::to_value`
  failure. The MCP dispatch shim translates this to a `{ isError: true, content: [...] }`
  MCP error response (per JSON-RPC 2.0 / MCP spec). In practice, serde failure on `MorphStats`
  is impossible (all fields are primitive serializable types), so this path is defensive only.
- **No frontend malformed-payload behaviour:** No frontend listener exists — the RPC response
  is consumed directly by the MCP client (Claude debug session), which handles tool errors
  via the standard MCP error protocol.
- **Missing emitter (zero counters):** Not an error — counters report zero until engine-wiring
  tasks 2947+ land. This is the intended state for the infrastructure-only phase.

---

## 7. Test pointers

- **Stats unit tests:** `crates/reify-mesh-morph/src/stats.rs` inline `#[cfg(test)] mod tests`:
  - `snapshot_returns_zeros_and_none_by_default` — default-zero snapshot after `reset_for_test()`
  - `morph_stats_serde_roundtrip` — JSON roundtrip for a fully-populated `MorphStats`
  - `record_morph_attempt_increments_morph_count` — recorder side effects via `snapshot()`
  - `record_remesh_increments_remesh_count` — recorder side effects + counter independence
  - `record_rejection_sets_last_reason_and_overwrites` — latest-wins overwrite semantics

- **Debug-server tests:** `gui/src-tauri/src/debug_server.rs` inline `#[cfg(test)] mod tests`:
  - `tool_defs_registers_morph_stats` — asserts `tool_defs()` has an entry with `name == "morph_stats"`,
    `input_schema.type == "object"`, non-empty description, `body_id` not in `required`
  - `handle_morph_stats_returns_morph_stats_shape` — calls `handle_morph_stats(json!({}))` directly
    (state-free, not through dispatch), asserts `morph_count == 0`, `remesh_count == 0`,
    `last_rejection_reason` absent, and that the `{ body_id: "..." }` form returns an identical response
  - `dispatch_stateless_tool_handles_morph_stats_arm` — calls
    `dispatch_stateless_tool("morph_stats", &json!({}))` and asserts the result equals calling
    `handle_morph_stats` directly; a typo or removal of the `"morph_stats"` arm string causes this
    test to fail (returns `None`, unwrap panics)

- **TS compile-fence:** `gui/src/__tests__/types.typecheck.ts` — import + four shape assertions
  for `MorphStats`; fails `tsc --noEmit` unless the interface is exported from `gui/src/types.ts`
