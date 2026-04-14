# ADR-883: Move DiagnosticInfo and SourceLocationInfo to reify-types

**Status:** Accepted  
**Date:** 2026-04-10  
**Task:** #883

---

## Context

Task 835 consolidated GUI-local domain types by replacing `gui/src-tauri/src/types.rs`
presentation types with the versions defined in `crates/reify-mcp/src/types.rs`. While
this simplified the code, it introduced an architectural layering violation:

- `gui/src-tauri/src/engine.rs` (line 13) imports `use reify_mcp::{DiagnosticInfo, SourceLocationInfo};`
- `engine.rs` is a **domain-layer** module that knows nothing about the MCP protocol.
- `reify-mcp` is an **adapter-layer** crate sitting *above* both the engine and the IPC
  boundary.

Importing upward from engine.rs into reify-mcp violates the dependency direction that
the rest of the architecture enforces.

### Type characteristics

`DiagnosticInfo` and `SourceLocationInfo` are **presentation projection types**: they
hold human-readable line/column positions (1-based `u32`) derived from structural
`reify_types::SourceSpan` byte-offsets via `byte_offset_to_line_col`. They are not MCP
protocol artifacts — they carry no JSON-RPC or MCP-specific semantics.

Both types are already *produced inside `engine.rs`* (lines 286–389), which is why the
engine depends on them at all. The conversion logic (`byte_offset_to_line_col`) already
lives in `crates/reify-types/src/source_location.rs`.

---

## Decision

**Move `DiagnosticInfo` and `SourceLocationInfo` to `crates/reify-types`** (Option A below),
add serde as an optional feature on reify-types, and re-export both types from reify-mcp
to preserve its public API.

---

## Alternatives Considered

### Option A — Move to reify-types with optional serde (chosen)

Types live in reify-types. reify-mcp adds `reify-types` as a dependency and re-exports:
`pub use reify_types::{DiagnosticInfo, SourceLocationInfo};`. engine.rs imports from
reify_types directly. All other consumers (mcp_context.rs, reify-cli, commands.rs)
continue using `reify_mcp::` re-exports unchanged.

**Pros:** Fixes layering violation. No churn at MCP-boundary consumers. Types live with
their logical kin (SourceSpan, Severity, byte_offset_to_line_col). Optional serde keeps
non-serializing crates clean.

**Cons:** Adds a dependency edge `reify-mcp → reify-types`. Requires an optional serde
feature on reify-types. Slight increase in reify-types surface area.

### Option B — Keep as-is

Accept the minor coupling. No changes required.

**Pros:** Zero cost.

**Cons:** Layering violation persists permanently. Tempts future modules at the engine
layer to also import from reify-mcp, deepening the coupling.

### Option C — Re-introduce two-type seam

Add GUI-local `DiagnosticData`/`SourceLocationData` back in `gui/src-tauri/src/types.rs`,
map them in `mcp_context.rs`.

**Pros:** Strict boundary — engine never sees reify-mcp names.

**Cons:** Reverses Task 835's simplification. Duplicates identical structs. `mcp_context.rs`
grows mapping boilerplate for two types that differ only in name.

### Option D — New reify-domain crate

Create a fourth crate between reify-types and reify-mcp to hold presentation types.

**Pros:** Cleanest boundary for a large number of types.

**Cons:** YAGNI for two types. Adds a new crate boundary that must be maintained across
the workspace. Cargo.toml churn in every dependent crate.

---

## Consequences

### Positive
- The layering violation in engine.rs is eliminated. Searching `engine.rs` for `reify_mcp`
  returns zero matches.
- `DiagnosticInfo` and `SourceLocationInfo` now live with `SourceSpan`, `Severity`, and
  `byte_offset_to_line_col` — all the structural pieces they project from.
- reify-mcp's public API is unchanged: `reify_mcp::DiagnosticInfo` continues to resolve
  via the re-export.
- The optional-feature pattern documents that reify-types itself doesn't require serde;
  it is only activated at the outer edge (reify-mcp) where JSON output is produced.

### Negative / Trade-offs
- `reify-types` now has a slightly larger surface area (two presentation types).
- `reify-mcp` now depends on `reify-types`. This is a new dependency edge, but the
  direction is correct (adapter → domain), and no cycle is created.
- The optional serde feature on reify-types must be propagated to future types that need
  serde support — minor but real maintenance cost.

### Scope boundary

Only `DiagnosticInfo` and `SourceLocationInfo` are moved. The other `*Info` types
(`ParameterInfo`, `ConstraintInfo`, `OpenFileInfo`, `SelectionInfo`, `EvalStatusInfo`,
`SourceContent`, `UpdateResult`, `SetParamResult`) remain in reify-mcp — they are
produced and consumed exclusively at the MCP boundary by `mcp_context.rs` and
`reify-cli/src/mcp_context.rs`, and do not participate in the layering violation.

Only `engine.rs` and `engine_tests.rs` are updated to use `reify_types::` imports;
all other MCP-boundary consumers retain their `reify_mcp::` paths, making it clear
that those files are wiring at the MCP layer.

---

## Implementation Notes

1. Add `serde = { workspace = true, features = ["derive"], optional = true }` and
   `[features] serde = ["dep:serde"]` to `crates/reify-types/Cargo.toml`.
2. Add `DiagnosticInfo` to `crates/reify-types/src/diagnostics.rs` with
   `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`.
3. Add `SourceLocationInfo` to `crates/reify-types/src/source_location.rs` with
   the same conditional derive.
4. Re-export both from `crates/reify-types/src/lib.rs`.
5. Add `reify-types = { workspace = true, features = ["serde"] }` to
   `crates/reify-mcp/Cargo.toml`.
6. Replace local struct definitions in `crates/reify-mcp/src/types.rs` with
   `pub use reify_types::{DiagnosticInfo, SourceLocationInfo};`.
7. Update `gui/src-tauri/src/engine.rs` line 13 and `engine_tests.rs` to import from
   `reify_types::` instead of `reify_mcp::`.
