# Geometry-Handle Runtime

Status: contract (resolves GR-030 cluster C-28). Authored 2026-05-14 in interactive session, sibling to `docs/prds/v0_3/structure-instance-runtime.md` (GR-001) and `docs/prds/v0_3/compute-node-contract.md` (GR-002). Approved by Leo before queueing tasks.

Resolves cluster C-28 and gap GR-030 per `docs/architecture-audit/gap-register.md`. Touches GR-018 (unbounded primitives — separate PRD) via consumer surface.

## §0 — Purpose and supersession

This document is the **contract** for making `Type::Geometry` (surface-syntax alias `Solid`) a first-class Value variant. Today, `Type::Geometry` is explicitly **rejected** by `is_representable_cell_type` (`crates/reify-eval/src/engine_eval.rs:70`), and the runtime invariant `assert_value_cell_types_representable` panics if any value cell carries the type. The compiler papers over this with two special-case routes:

1. `is_solid_geometry_param` (`crates/reify-compiler/src/geometry.rs:150`) — when a structure-def param is `Solid` with a geometry-call default, value-cell creation is **skipped** and the param is routed as a realization op instead. Same bypass replicated at four consistent sites (entity.rs pre-pass + param loop, guards.rs register + compile).
2. Synthetic-ValueRef-then-skip (`crates/reify-compiler/src/entity.rs:1129-1147`, task 3454) — cross-sub geometry access produces a `CompiledExpr::value_ref` with `Type::Geometry`, then explicitly suppressed from value-cell creation by a narrowed predicate.

These bypasses successfully ship for structure-def-level slots with same-kind geometry-call defaults, but they break down at trait slots: the stdlib `Physical` spec shape (`docs/reify-stdlib-reference.md` §4) declares `param geometry : Solid` + `let mass = volume(geometry) * material.density` and the trait-defaulted `let` consumes a `Type::Geometry` value the runtime cannot represent. The stdlib's actual `structural_physical.ri:20-27` carries a documented v0.1 trade: flat scalar params (`volume : Real`, `centroid_x : Real`, etc.) standing in for the spec's geometry-and-material slots (audit `findings/stdlib-trait-breadth.md` M-007 DRIFT, M-013 ORPHAN).

The contract introduces a typed `Value::GeometryHandle` variant backed by a stable `RealizationNodeId` reference plus a session-scoped kernel handle, retires both bypass routes, extends `dependency_trace` for Realization→ValueCell edges, adds the cache-key fragment, and lands the first vertical slice: stdlib `Physical` restored to spec shape + the kernel queries (`volume`, `centroid`, `area`, `bounding_box`) wired end-to-end so a steel bracket evaluates to a non-Undef mass and centroid.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (`preferences_implementation_chain_naming`) — is what this contract is designed to prevent for the geometry-handle seam specifically. Resolution mode is **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline. The full-workspace `match Value` adapter sweep is high-priority wide-lock per `feedback_orchestrator_narrow_locks_favor_upfront_design`.

This document is named in `docs/architecture-audit/gap-register.md` GR-030's disposition and is the resolution mechanism for cluster C-28.

## §1 — What is settled

Resolved 2026-05-14 in interactive session:

- **Identity model: realization-ref backed.** `Value::GeometryHandle` carries (a) a `RealizationNodeId { entity: String, index: u32 }` (the stable, cross-restart logical identity, already defined at `crates/reify-types/src/identity.rs:164`), (b) an `upstream_values_hash: [u8; 32]` capturing the realization op's input values for cache + significance discrimination, and (c) a `kernel_handle: GeometryHandleId` (the session-scoped kernel resolution). The kernel handle is a fast indirection; the (RealizationNodeId, upstream_values_hash) pair is the stable cache key. Content-hashing the entire op tree is **not** pursued (rejected: redundant with realization-cache's existing entity_id keying; floating-point determinism concerns across kernels).
- **Solid does NOT imply Bounded.** Bare `param geometry : Solid` accepts any geometry handle, including (future) `half_space()` / `extrude_infinite()` from GR-018. Bounded-required slots use `param g : Bounded` — `Bounded` is a trait declared in `crates/reify-compiler/stdlib/geometry_traits.ri`; the conformance check at `crates/reify-compiler/src/conformance/mod.rs:660-689` has special routing that consults the per-op inference table (`geometry_traits_inference.rs`) for geometry-typed args. The existing machinery is wired; only the unbounded *producers* are absent, owned by GR-018. **G3 grammar gate verified 2026-05-14**: `param g : Bounded` parses and resolves; `Solid + Bounded` does NOT parse.
- **Snapshot resolution: lazy revalidation on first read.** Cloned snapshots and persisted-then-reloaded snapshots may carry stale `kernel_handle` values. On first read of a `Value::GeometryHandle`, the engine checks `kernel_handle` validity in the current Engine's registry; if stale, re-resolves from `RealizationNodeId`; if the realization is absent in the current Engine, returns `Value::Undef`. Matches Reify's demand-driven evaluation philosophy.
- **Significance filter policy: realization-ref + upstream-values-hash equality → Same.** Two `Value::GeometryHandle` values compare Same iff their `RealizationNodeId` matches AND `upstream_values_hash` matches. This allows compute-node caching (when geometry-bearing structures eventually route through it) to skip downstream re-eval when the geometry is semantically unchanged, even if the kernel re-realized to a different `kernel_handle`.
- **Phase 1 (A′ expanded): stdlib registrations + Physical spec-shape restored.** `structural_physical.ri` is rewritten to the spec shape with `param geometry : Solid` + `param material : Material` + computed lets. Compile-time typing passes; runtime evaluation of the computed lets returns Undef until Phase 6 (kernel dispatch) lands. Downstream FEA tests that read `bracket.mass` or `bracket.centroid` are temporarily marked `#[ignore]` with a comment pointing at this PRD's Phase 6 task; the stale flat-scalar reads are deleted.
- **Phase 6 (terminal): direct kernel dispatch, NOT ComputeNode.** OCCT volume/centroid/area/bounding_box queries are sub-50ms even on multi-million-poly bodies; below the compute-node-contract.md §6 ≥50ms heuristic. Direct kernel call through `reify-kernel-occt`. ComputeNode wrap is future-PRD work if profiling justifies it. Realization-level caching (existing `RealizationCache<V>` at `crates/reify-eval/src/realization_cache.rs`) keys these queries' results by `(entity_id, repr_kind, tol)`.
- **A′ stdlib registration list (Phase 1 deliverable, frozen).** Add to `crates/reify-compiler/src/units.rs` name → return-type table:
  - `volume(Solid) → Scalar<Volume>`
  - `area(Surface) → Scalar<Area>` + `area(Solid) → Scalar<Area>`
  - `length(Curve) → Scalar<Length>`
  - `perimeter(Surface) → Scalar<Length>`
  - `centroid(Solid) → Point3<Length>`
  - `bounding_box<G: Geometry>(g) → BoundingBox`
  - `distance<G1, G2>(a, b) → Scalar<Length>`
  - `contains(Solid, Point3<Length>) → Bool`
  - `intersects(Geometry, Geometry) → Bool`
  - `geo_equiv(Geometry, Geometry, Length) → Bool`
  - `angle(Vector3<Dimensionless>, Vector3<Dimensionless>) → Angle`
- **Curvature dimensional alias (Phase 1 sub-deliverable).** Add `Curvature` (= `Length^-1`) to `crates/reify-types/src/dimension.rs` `NAMED_DIMENSIONS`. Register `curvature(Curve, Point3<Length>) → Scalar<Curvature>` and `curvature(Surface, Point3<Length>) → Matrix<2, 2, Curvature>`.
- **Phase 1 hard prereq: SIR-α.** The restored `Physical` trait body uses `material.density` member access; that's wired by structure-instance-runtime.md's SIR-α foundation slice. Phase 1's first task carries an explicit `add_dependency` edge on SIR-α's task id (resolved at decompose-mode filing time per `preferences_cross_prd_deps_real_edges`).

Full rationale recorded in conversational session 2026-05-14 — do not re-open here.

## §2 — `Value::GeometryHandle` shape

**Variant.** Added to `Value` (definition at `crates/reify-types/src/value.rs:294`, alongside `Value::StructureInstance` from SIR):

```rust
pub enum Value {
    // ... existing variants ...
    GeometryHandle {
        realization_ref: RealizationNodeId,    // stable cross-restart identity
        upstream_values_hash: [u8; 32],        // blake3 of input values
        kernel_handle: GeometryHandleId,       // session-scoped resolution
    },
}
```

`RealizationNodeId { entity: String, index: u32 }` is **pre-existing** at `crates/reify-types/src/identity.rs:164` — no new type. `GeometryHandleId` is the existing per-kernel-session opaque u64 (`crates/reify-types/src/identity.rs` area; verify exact path during Phase 2 task). `upstream_values_hash` is computed once when the variant is constructed (Phase 3 lowering); subsequent reads do not recompute.

**Why realization-ref + upstream-values-hash, not pure content-hash of the op tree.**

The realization-cache (`crates/reify-eval/src/realization_cache.rs`) already keys realization outputs by `(entity_id, repr_kind, tol)`. The entity_id is `"<StructureName>__<member_name>"` (stable across restarts, derivable from structure-def member declarations). Reusing this as the identity backbone avoids inventing a parallel hashing mechanism.

The `upstream_values_hash` discriminates two `RealizationNodeId`-equal handles when their realization op's input values differ. For example: `param thickness : Length = auto` flows into a `box(width, height, thickness)` realization; same RealizationNodeId across solver iterations, but `upstream_values_hash` differs as `thickness` resolves. This lets the significance filter correctly mark such re-realizations as Different even though the realization-ref is stable.

**`kernel_handle` is the session-scoped fast path.** Within a single Engine session, reads dereference the kernel_handle directly via the kernel's registry. Across snapshot clone or persistent reload, the kernel_handle may be stale; the lazy-revalidation policy (§5) handles this.

**Why not full content-hashed op-tree identity.** Rejected because:
- Floating-point kernel determinism is per-kernel-family (OCCT vs Manifold may produce bit-different bytes for conceptually-identical geometry). A content hash backed by serialized op bytes is fragile across kernel versions.
- The existing realization-cache keying solves the "stable across restart" problem already, at lower mechanism cost.
- Future migration is additive — if a real need for cross-kernel content-hashing emerges, a `content_hash: Option<[u8; 32]>` field on the variant can be added without breaking the realization-ref-based path.

## §3 — Workspace adapter sweep

**Scope.** Every site that produces, consumes, or special-cases `Type::Geometry` becomes a real arm. Survey (2026-05-14, per agent-1 mapping):

**Production sites (5) — emit `Value::GeometryHandle` (or its absence as Undef) at these compile/runtime junctures:**

- `crates/reify-compiler/src/entity.rs:537` — Geometry let-binding registration. Currently registers `Type::Geometry` and the value cell is later suppressed via bypass. Post-sweep: register cell normally; the lowering path (§4) produces a real `Value::GeometryHandle`.
- `crates/reify-compiler/src/entity.rs:505-514` — Solid-typed param + geometry-call default. The `is_solid_geometry_param` bypass that routes to realization-op only is retired; param produces a value cell of `Type::Geometry` carrying a real handle.
- `crates/reify-compiler/src/guards.rs:179` — Guarded geometry let. Same treatment as entity.rs:537.
- `crates/reify-compiler/src/expr.rs:264` — Synthetic value-ref for cross-sub geometry access. The synthetic ValueRef shape becomes a real read of a `Value::GeometryHandle` cell.
- `crates/reify-compiler/src/type_resolution.rs:513` — Surface-syntax alias `"Solid" => Type::Geometry`. Unchanged.

**Consumption sites (7) — branch on `Type::Geometry`:**

- `crates/reify-eval/src/engine_eval.rs:70` — `is_representable_cell_type` rejection. **Removed** — `Type::Geometry` becomes representable.
- `crates/reify-eval/src/lib.rs:195-271` — `value_type_kind_matches`. **New arm**: `Value::GeometryHandle` matches `Type::Geometry` (true) and any other type (false). The Undef default-arm continues to apply to all variant/type pairs.
- `crates/reify-compiler/src/conformance/mod.rs:656` — Trait-conformance geometry-arg detection. Unchanged — the existing `matches!(effective_arg_type, Type::Geometry)` continues to route geometry args to the per-op inference table for Bounded/Connected/Convex checks.
- `crates/reify-compiler/src/geometry_traits_inference.rs:637, 652, 666` — Geometry-operand extraction in composition rules. Unchanged.
- `crates/reify-types/src/ty.rs:369` — Display impl. Unchanged.
- `crates/reify-kernel-openvdb/src/ingest.rs:809` — Type repr string. Unchanged.
- `crates/reify-eval/src/engine_admin.rs:51, 78` — Param-override validation calls `value_type_kind_matches`. **Automatically gains the new arm** via the change above.

**Bypass / special-case sites (6) — retired or simplified:**

- `crates/reify-compiler/src/entity.rs:1129-1147` (task 3454 bypass) — **Retired**. The cell-creation skip is removed; the synthetic ValueRef shape becomes a normal value cell.
- `crates/reify-compiler/src/geometry.rs:150-160` — `is_solid_geometry_param` — **Retired**. The function and all four call sites removed.
- `crates/reify-compiler/src/geometry.rs:287-311` — `try_resolve_cross_sub_geom_ref` — **Kept** for the geometry-call dispatch path (GeomRef::Sub used by boolean ops + sweep). The parallel value-ref production at `expr.rs:264` continues to exist for the Value-side; the two routes remain co-discovered via the shared predicate at `scope.rs:259-265`.
- `crates/reify-compiler/src/geometry.rs:95-140` — `is_geometry_let` — **Kept** unchanged; it's the geometry-call vs. non-geometry-call classifier, orthogonal to the value-cell representability change.
- `crates/reify-compiler/src/units.rs:194-228, 653-685` — `Type::List(Box::new(Type::Geometry))` for spread-geometry results (loft_all, union_all). **Updated**: cells of `List<Geometry>` now hold `Value::List(vec![Value::GeometryHandle { ... }, ...])` instead of `Value::Undef`. The dual-skip note (task 3454) is removed.
- `crates/reify-compiler/src/scope.rs:259-265` — Shared predicate `sub_member_is_cross_sub_geometry_or_forward_declared` — **Kept** unchanged.

**Value-cell adjacency sites (5) — gain `Value::GeometryHandle` exhaustiveness:**

- `crates/reify-eval/src/lib.rs:195-271` (`value_type_kind_matches`) — covered above.
- `crates/reify-eval/src/engine_eval.rs:66-106` (`is_representable_cell_type`) — covered above.
- `crates/reify-eval/src/engine_eval.rs:135-148` (`assert_value_cell_types_representable`) — invariant updated to no longer reject `Type::Geometry`.
- `crates/reify-eval/src/engine_admin.rs:51, 78` (`validate_param_override`) — automatically updated.
- `crates/reify-eval/src/engine_edit.rs:1207-1208` (edit-time recompile invariant call) — automatically updated.

**Sweep rollout.** Single **wide-lock task, high priority** per `feedback_orchestrator_narrow_locks_favor_upfront_design`. The Phase 2 task's `metadata.files` enumerates all of the above plus the cache + freshness sites from §5/§6, and is the orchestrator's lock charter. Per-arm policy is one of three:

1. **Behaves-like-other-Value sites** (display, hashing, equality, clone, kind-match): the `Value::GeometryHandle { realization_ref, upstream_values_hash, kernel_handle }` arm dispatches by realization-ref + upstream-values-hash equality (kernel_handle is excluded from `==` / `Hash` to preserve cross-snapshot stability).
2. **Adapter sites consulting the geometry kernel** (geometry_ops, snapshot, kernel queries): the new arm resolves `kernel_handle` (lazy revalidation against `realization_ref` if stale) and proceeds.
3. **Reject sites** (variants the consumer never expects): clean diagnostic + Undef, same as existing default-arm pattern.

## §4 — Compile lowering: retiring the bypasses

**Current state.** The two bypass routes (§3) work by *skipping* value cell creation when a name has `Type::Geometry`. Geometry "values" exist only as realization-op outputs; consumers that want a geometry handle reach into the realization graph via specialized resolution paths (`try_resolve_cross_sub_geom_ref` for boolean-op dispatch; `GeomRef::Sub` carrying entity+member through the op stream). Value cells of `Type::Geometry` are forbidden by `assert_value_cell_types_representable`.

**Lowering rule.** Post-sweep, when a structure-def member or let is `Type::Geometry`-typed:

- A normal value cell is created in the value-cell map. The cell's default expression is the geometry-producing expression (e.g. `box(10mm, 20mm, 30mm)`).
- During eval, the geometry-producing expression dispatches to the kernel (`reify-kernel-occt` / `reify-kernel-manifold` / etc.) which produces a `GeometryHandleId`.
- The Engine wraps the result in `Value::GeometryHandle { realization_ref: <derived from the cell's RealizationNodeId>, upstream_values_hash: <blake3 of input values fed into the op>, kernel_handle }` and stores in the cell.
- Subsequent reads return the wrapped value; consumers that need the kernel handle dereference via the lazy-revalidation path.

**Where the lowering lives.** Two cleanly-separated sites:

- `engine_eval.rs` `try_eval_function_call` (or the equivalent) detects geometry-producing calls (via the existing `is_geometry_function` predicate) and, upon successful kernel dispatch, wraps in `Value::GeometryHandle` instead of returning `Value::Undef`.
- The realization-op execution path (currently `engine_build.rs::dispatch_volume_mesh` and parallel sites in `engine_build.rs`) continues to populate the `RealizationCache` for kernel-output reuse; the Value::GeometryHandle's kernel_handle resolves through this cache.

**Default-value evaluation.** Structure_def field defaults that are themselves geometry-call expressions recurse through the same lowering. `param geometry : Solid = box(...)` inside a structure_def produces a normal value cell holding the wrapped handle. The realization-op is recorded as before; the only change is that the *value* of the cell is now non-Undef.

**Backwards compat.** Existing source files that worked under the bypass path continue to evaluate identically — the realization-op chain is unchanged; only the value-cell content changes from Undef to the wrapped handle. The pinning test at `crates/reify-compiler/tests/solid_param_tests.rs` should be extended (Phase 3 task) to assert the value cell holds a `Value::GeometryHandle` post-lowering; absence of such an assertion today is a coverage gap.

**Cross-sub geometry access.** `self.<sub>.<geom>` produces a `CompiledExpr::value_ref` at `expr.rs:264` (synthetic, today suppressed). Post-retirement: the value-ref resolves to a real value cell holding a real `Value::GeometryHandle`. The dual path `try_resolve_cross_sub_geom_ref` at `geometry.rs:287` continues to produce `GeomRef::Sub` for boolean-op dispatch (which consumes geometry via kernel handles, not Value reads); the two paths remain shape-consistent via the shared predicate at `scope.rs:259-265`.

## §5 — Freshness walk + edit donation extension

**Risk surface identified** (per agent-2 audit, surface 2 + surface 5): `dependency_trace.reads` today records ValueCell → ValueCell edges only. A value cell holding a `Value::GeometryHandle` has an implicit dependency on the upstream RealizationNode (the realization whose ID is in the handle's `realization_ref`). The freshness walk would compute the cell's freshness from its declared VC dependencies, missing that an upstream realization became Intermediate / removed.

**Mechanism.** Extend `dependency_trace` to record a third edge kind: Realization → ValueCell. The trace is populated during compile-time lowering: when a value cell's compiled expression resolves to a `Value::GeometryHandle`, the corresponding `RealizationNodeId` is recorded as a logical input. `derive_output_freshness_for_node_with_cause` (in `crates/reify-eval/src/cache.rs`) gains an arm: for cells whose `dependency_trace.realization_reads` is non-empty, the cell's freshness is the meet of (existing VC-input freshness, all referenced Realization freshness). Pending propagation works identically to the VC-VC case — the chain root forwards.

**Edit donation cascade.** The existing edit-time donation hook at `engine_edit.rs:2275-2301` invalidates the three node kinds (Value, Constraint, Realization) when source-edit diff returns a changed node. Today, no cross-kind cascade exists: a changed Realization invalidates the realization's own cache entry but not the value cells that hold handles backed by it. Post-Phase 5: the donation hook reads `dependency_trace.realization_reads` for each value cell; cells whose Realization-input is invalidated are also invalidated (their `Value::GeometryHandle` becomes stale; lazy revalidation handles the next read).

**Snapshot lazy revalidation.** When a cell holding a `Value::GeometryHandle` is read:
1. Engine consults the kernel registry for `kernel_handle` validity.
2. If valid: return the wrapped value as-is.
3. If invalid (cross-Engine snapshot, post-cache-reload, post-edit-cascade): look up `realization_ref` in the current Engine. If the realization is present and Final, re-resolve the kernel handle and update the cell's `kernel_handle` field (the `realization_ref` + `upstream_values_hash` are stable; only the kernel-side resolution is mutable). If the realization is absent or non-Final: return `Value::Undef`.

The revalidation cost is amortized: a single `kernel_handle.is_valid()` check per read (atomic load), with the slow path (re-resolution) firing only after a snapshot clone or persistent reload.

**Significance filter integration.** When a `Value::GeometryHandle` is compared for significance (per the policy in §1):

```rust
fn geometry_handle_significance(old: &Value, new: &Value) -> FilterOutcome {
    if let (Value::GeometryHandle { realization_ref: r_old, upstream_values_hash: h_old, .. },
            Value::GeometryHandle { realization_ref: r_new, upstream_values_hash: h_new, .. }) = (old, new) {
        if r_old == r_new && h_old == h_new {
            return FilterOutcome::Equivalent;
        }
        return FilterOutcome::Different;
    }
    /* fall through */
}
```

The `kernel_handle` field is **deliberately excluded** from the comparison: re-realization that produces a different handle id for semantically-identical geometry must not trigger downstream invalidation. This is the load-bearing rationale for the `upstream_values_hash` field — it's what distinguishes "semantically same" from "semantically different" handles backing the same realization-ref.

## §6 — Cache key composition + persistent cache

**Key fragment.** A `Value::GeometryHandle` serializes for cache-key purposes as the tuple:

```
("gh", entity: &str, index: u32, upstream_values_hash: [u8; 32])
```

where:
- `"gh"` is the variant discriminator (sibling to SIR's `"si"`).
- `entity` and `index` are the components of `realization_ref: RealizationNodeId`. Both stable across Engine restarts (structure name is stable; index is assigned at compile-time from the structure_def's realization-op list in declaration order, also stable).
- `upstream_values_hash` distinguishes two same-realization-ref handles produced from different input values.

**Sites.** Cache-key composition lives at `crates/reify-eval/src/cache.rs` (in-memory cache hashing) and `crates/reify-eval/src/persistent_cache.rs` (on-disk key serialization). Both grow a `Value::GeometryHandle` arm in the existing `value_to_cache_key` (or equivalent) function. The arm reads `realization_ref` and `upstream_values_hash` directly from the variant; no Engine-state lookup needed at key-composition time.

**Interaction with SIR's `Value::StructureInstance` cache-key composition.** SIR's `fields_hash` recurses through `cache_key_of(v)` for each field value. When a field is `Value::GeometryHandle`, the recursion produces the GH fragment above. A `Bracket` structure-instance with `geometry: <handle>` and `material: Steel_AISI_1045()` cache-keys composes as:

```
("si", "Bracket", 1, blake3([
    ("geometry", ("gh", "Bracket", 0, <upstream_hash>)),
    ("material", ("si", "Steel_AISI_1045", 1, <steel_fields_hash>)),
]))
```

Both PRDs' cache-key fragments compose uniformly; no special handling.

**Invariants.**
- Stable across Engine restarts: yes (RealizationNodeId is stable; upstream_values_hash is content-derived).
- Invalidated by editing the realization op (structure-def member rename, geometry-call rewrite): yes (entity name or index changes).
- Invalidated by changing input values to the realization: yes (upstream_values_hash changes).
- Invalidated by changing the kernel that produced the handle: **no, unless** the kernel's contributions are reflected in `upstream_values_hash`. The simplest approach is to include the active kernel name in the hash. Decision deferred to Phase 5 task; suggested default: include kernel name in upstream_values_hash so cross-kernel results are not silently cache-hit-confused.
- Invalidated by floating-point drift in identical-looking re-execution: no — same inputs produce same upstream_values_hash. Geometric output may differ at the bit level (kernel non-determinism); for v0.3 this is acceptable. If observed in practice, Phase 5 can add an explicit content-hash field (additive; see §2 future-migration note).

**Realization-level caching unchanged.** The kernel side continues to use `RealizationCache<V>` keyed by `(entity_id, repr_kind, tol)`. When a Value::GeometryHandle is constructed, the Engine looks up the realization cache for the kernel handle; a hit reuses the existing handle, a miss dispatches the kernel and stores the result. This is orthogonal to the value-cache machinery this PRD adds.

**Engine-version-hash interaction.** The existing `ENGINE_VERSION_HASH` (task 2970, done) bounds the overall cache namespace. Reify-engine rebuilds invalidate everything beneath. Intra-version-hash structure_def + realization edits are invalidated by the cache-key fragments described above. The two layers compose orthogonally.

## §7 — Boundary test sketch (cross-crate; facing both ways)

Tests live in `crates/reify-eval/tests/` (engine-level integration) and `crates/reify-types/src/value.rs::tests` + `crates/reify-eval/src/lib.rs::tests` + per-module unit suites. The seam is between `reify-types` (Value variant), `reify-eval` (lowering + adapters + cache + freshness), `reify-stdlib` (kernel-query trampolines for Phase 6), `reify-compiler` (registrations + Physical restoration), and `reify-kernel-occt` (kernel queries).

### 7.1 Producer-side (variant + adapter + lowering + freshness machinery looks outward at consumers)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Variant construction round-trip.** Construct `Value::GeometryHandle { realization_ref, upstream_values_hash, kernel_handle }` via Rust API; pass through Clone, PartialEq, Hash. | Variant exists; `RealizationNodeId` reused as-is. | Clone preserves all three fields. PartialEq compares `(realization_ref, upstream_values_hash)` only — `kernel_handle` is excluded. Hash mirrors PartialEq (matches HashMap invariant). Unit test in `crates/reify-types/src/value.rs::tests`. |
| **Adapter sweep coverage.** Every `match value` / `match Value::` site in the workspace has a `Value::GeometryHandle` arm. | Adapter sweep task done. | `cargo check --workspace` green. Rustc's exhaustiveness lint flags any future un-adapted site. |
| **`value_type_kind_matches` arm.** `Value::GeometryHandle` matches `Type::Geometry`; mismatches all other type variants. | Variant shipped; arm added. | Unit test in `crates/reify-expr/src/lib.rs::tests` (alongside existing `value_type_kind_matches_*` tests). |
| **`is_representable_cell_type` admits Type::Geometry.** | Sweep done. | Pre-existing test that asserts `Type::Geometry → false` is **flipped** to assert `→ true`. The companion rejection-comment in the source code is removed. |
| **Cache-key serialization.** Two `Value::GeometryHandle` values with same realization_ref + same upstream_values_hash serialize to the same cache key; different upstream_values_hash produces different keys; different realization_ref produces different keys. kernel_handle is excluded. | Cache-key arm shipped. | Unit test in `crates/reify-eval/src/cache.rs::tests`. |
| **Engine-restart cache-key stability.** Cache key for a `box(10mm, 20mm, 30mm)`-backed handle recovered after Engine drop + re-create matches the pre-drop key. | Cache-key uses realization_ref + upstream_values_hash, not kernel_handle. | Engine integration test asserts cache hit across restart. |
| **Lazy revalidation: stale kernel_handle re-resolves.** Construct handle in Engine A; drop A; reconstruct in Engine B with the same source program; first read of the cloned/persisted handle re-resolves kernel_handle from realization_ref. | Revalidation logic shipped. | Engine integration test pins re-resolution behavior; instrumentation confirms the slow path fires only once per snapshot/reload boundary. |
| **Lazy revalidation: missing realization returns Undef.** Construct handle in Engine A; drop A; reconstruct in Engine B with a *modified* source program where the originating realization no longer exists; first read returns Undef rather than panicking. | Revalidation logic shipped. | Engine integration test. Negative-path coverage. |
| **Freshness walk: Realization → ValueCell edge.** A value cell holding a `Value::GeometryHandle`; mark the upstream Realization as Intermediate; observe the cell becomes Pending. | dependency_trace extension shipped; derive_output_freshness updated. | Engine integration test in `crates/reify-eval/tests/`. Mirrors existing VC-VC pending-cause tests. |
| **Edit donation cascade.** Edit a structure-def member whose value is consumed by a downstream cell holding a Value::GeometryHandle; observe the downstream cell invalidates. | Donation hook extension shipped. | Engine integration test exercising the engine_edit.rs:2275-2301 path with a Realization → ValueCell edge. |
| **Significance filter: realization_ref + upstream_values_hash equality → Equivalent.** Re-realize a geometry that produces a different kernel_handle but same realization_ref + same upstream input values; observe FilterOutcome::Equivalent. | Significance filter arm shipped. | Unit test in `crates/reify-eval/src/significance_filter.rs::tests`. |
| **Significance filter: upstream_values_hash mismatch → Different.** Re-realize with a changed input value; observe Different. | Significance filter arm shipped. | Unit test. |

### 7.2 Consumer-side (FEA / stdlib / examples look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Spec-shape Physical typechecks.** `crates/reify-compiler/stdlib/structural_physical.ri` rewritten to spec shape: `trait Physical { param geometry : Solid; param material : Material; let mass = volume(geometry) * material.density; let centroid = centroid(geometry); }`. | Phase 1 done (registrations + Physical restore); SIR-α landed (material.density access). | `cargo test -p reify-compiler` green. The compile-time error "member access not yet supported" no longer fires. |
| **Spec-shape Physical instantiates without runtime panic.** `examples/spec-shape-physical.ri`: `structure def Bracket : Physical { param geometry : Solid = box(10mm, 20mm, 30mm); param material : Material = Steel_AISI_1045(); }` evaluates without the `unrepresentable cell_type` panic. | Phase 3 done (variant + lowering retires bypasses). | `reify eval examples/spec-shape-physical.ri` runs without panic. `bracket.geometry` returns inspectable `Value::GeometryHandle`. `bracket.mass` and `bracket.centroid` return `Value::Undef` (still — kernel dispatch not yet wired). |
| **List of geometries.** `examples/spread-geometry.ri`: `let parts : List<Solid> = [box(...), sphere(...), cylinder(...)]`. | Phase 3 done. | Cell holds `Value::List(vec![Value::GeometryHandle { .. }, ...])`. Member access `parts[0]` returns the first handle. |
| **Geometry-bearing structure cache round-trip.** `bracket.ri` evaluates; engine exits; engine restarts; same file re-evaluated. | Phase 5 done (cache-key composition). | First evaluation populates persistent cache. Second evaluation reads back identical Value (no re-realization; verified via instrumentation). `bracket.geometry` round-trips. |
| **Cache invalidation on geometry-call edit.** Same fixture; change `box(10mm, 20mm, 30mm)` to `box(20mm, 20mm, 30mm)`; restart. | Cache-key fragment includes upstream_values_hash. | Cache miss on the edited realization; fresh re-realization. The Bracket's `Value::StructureInstance` cache-key (SIR) also invalidates (fields_hash includes the new GH fragment). |
| **Edit cascade through structure-instance.** Edit a parameter that flows into a geometry call; observe both the geometry-handle cell AND the structure-instance cell invalidate. | Phase 4 done (freshness walk edges). | Engine integration test. The cascade traverses Param → Realization → GeometryHandle ValueCell → StructureInstance ValueCell. |
| **Bounded enforcement at Bounded-typed slots.** Source: `trait NeedsBoundedGeom { param g : Bounded }`; `structure def Bad : NeedsBoundedGeom { param g : Bounded = <unbounded-primitive>() }`. The `Bounded` trait at the param position is consumed by the existing conformance routing at `conformance/mod.rs:660-689`; `Type::TraitObject("Bounded")` flows to the per-op geometry-traits-inference table. | This PRD landed; GR-018 lands unbounded primitives. | Compile error `E_GEOMETRY_UNBOUNDED`. Negative-path test. **Pre-GR-018:** no unbounded primitive exists; the negative path is unexercised. Captured in §10 follow-up. |
| **Real `bracket.mass` and `bracket.centroid` evaluate.** `bracket.ri` evaluates `bracket.mass` to a real `Scalar<Mass>` value (e.g. `0.0468 kg` for a 10×20×30mm steel block); `bracket.centroid` to a real `Point3<Length>` (e.g. `(5mm, 10mm, 15mm)`). | Phase 6 done (OCCT kernel queries). | `reify eval examples/spec-shape-physical.ri` prints non-Undef structure-shaped Value with real numeric mass and centroid. CLI golden output committed. |
| **GR-031 composed-stress envelope unblock.** Existing task 3553 (SIR-γ envelope helpers, re-filed 2026-05-14 post-SIGABRT after the original #3468 was repurposed by curator recovery; deps `[3540]` today) — typed envelope helpers consuming geometry-bearing structures depend on SIR-α + this PRD. | Phase 6 done. | Task 3553 unblocks; its tests pin envelope `.max_von_mises` member access on a geometry-bearing structure. |
| **FEA round-trip with geometry-bearing Physical.** A multi-load-case-fea fixture builds `Bracket : Physical` with real geometry; runs `solve_elastic_static(bracket, load_case)`; reads `result.max_von_mises`. | Phase 6 done; ComputeNode contract η (FEA first consumer) done; multi-load-case-fea PRD's consumer-side tests adapted. | End-to-end FEA result on a geometry-bearing Bracket; within tolerance of analytical solution for the box-cantilever fixture. |
| **Mesh-morph with geometry-bearing structure.** Parametric design varies a non-structural parameter; `.geometry` handle re-realizes; freshness cascade triggers; mesh-morph runs (per mesh-morphing PRD); FEA result updates. | Phase 6 done; mesh-morphing PRD κ done. | Engine integration test pins the cascade end-to-end. |

## §8 — Decomposition DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its **user-observable signal**. Producer-only tasks closed in isolation are not tolerable per `feedback_task_chain_user_observable`.

Filing happens in a **separate session** after this PRD is committed (per `feedback_commit_prds_before_referencing_tasks`).

### Phase 1 — Stdlib registrations + Physical spec-shape restoration

- **Task GHR-α** — `units.rs` stdlib geometry-query registrations + Curvature dimensional alias + `structural_physical.ri` rewritten to spec shape + downstream-FEA-test #[ignore] markers.
  - **Observable signal:** `reify check crates/reify-compiler/stdlib/structural_physical.ri` succeeds (no "unresolved type" / "member access not yet supported" diagnostics). The integration test `crates/reify-compiler/tests/structural_physical_spec_shape.rs` (new) parses + typechecks a fixture that uses spec-shape Physical with `material.density` access and `volume(geometry)` / `centroid(geometry)` calls. The existing structural-physical conformance tests pass against the new shape. `cargo test --workspace` green.
  - **Priority:** medium (no critical-path racing; ships when SIR-α is done).
  - **Prereqs:** **SIR-α (structure-instance-runtime.md Task SIR-α)** — hard dep, wired via `add_dependency` at decompose-mode filing time per `preferences_cross_prd_deps_real_edges`.
  - **Crates touched:**
    - `crates/reify-types/src/dimension.rs` (add `Curvature` to `NAMED_DIMENSIONS`)
    - `crates/reify-compiler/src/units.rs` (~12 new function entries in the name → return-type table)
    - `crates/reify-compiler/stdlib/structural_physical.ri` (rewrite to spec shape; delete flat-scalar params)
    - `crates/reify-compiler/tests/structural_physical_spec_shape.rs` (new integration test)
    - `crates/reify-compiler/tests/structural_physical_tests.rs` (update existing tests against the new shape)
    - Downstream FEA test files (under `crates/reify-eval/tests/`) — search for `bracket.mass` / `bracket.centroid` reads and either #[ignore] with `Phase 6 will revive` comment OR adapt to the new shape if SIR-α's StructureInstance shape supports the read at compile time.

### Phase 2 — `Value::GeometryHandle` variant + adapter sweep + side-table

- **Task GHR-β** — Wide-lock foundation: variant + adapter sweep + value-cell representability flip + is_representable_cell_type update + assert_value_cell_types_representable update.
  - **Observable signal:** Construct a `Value::GeometryHandle` via Rust test harness; pass through Clone, PartialEq (excluding kernel_handle), Hash. Unit test pins. `cargo check --workspace` green (rustc exhaustiveness confirms every match site has the new arm). The `is_representable_cell_type` test that asserted Type::Geometry → false is flipped to true. **NO source-level evaluation behavior change yet** — this is producer-side machinery; consumer-side comes in Phase 3.
  - **Priority:** high (per `feedback_orchestrator_narrow_locks_favor_upfront_design`).
  - **Prereqs:** Phase 1 GHR-α done.
  - **Crates touched (lock charter):**
    - `crates/reify-types/src/value.rs` (`Value::GeometryHandle` variant added)
    - `crates/reify-eval/src/engine_eval.rs` (is_representable_cell_type + assert_value_cell_types_representable updated)
    - `crates/reify-eval/src/lib.rs` (`value_type_kind_matches` new arm)
    - `crates/reify-eval/src/engine_admin.rs` (validate_param_override picks up the arm automatically)
    - `crates/reify-eval/src/engine_edit.rs` (recompile invariant call picks up arm automatically; donation cascade extension placeholder)
    - `crates/reify-eval/src/geometry_ops.rs`, `engine_eval.rs`, `geometry.rs` adapter arms
    - `crates/reify-stdlib/src/{fea,geometry,joints,loop_closure,loop_closure_solver,mechanism,snapshot,supports,sweep}.rs` (match-site arms)
    - `crates/reify-expr/src/lib.rs` (value-flow adapter arms)
    - `crates/reify-types/src/ty.rs` (Display arm already exists for Type::Geometry; ensure Value::GeometryHandle Display is added)
    - `crates/reify-kernel-openvdb/src/ingest.rs` (type repr; no Value arm needed)
    - All test files that exhaustively match on Value variants
  - **Boundary tests:** §7.1 producer-side scenarios (variant round-trip, adapter coverage, `value_type_kind_matches`, `is_representable_cell_type` flip, cache-key serialization).

### Phase 3 — Compile lowering: retire the bypasses

- **Task GHR-γ** — Retire `is_solid_geometry_param` (geometry.rs:150-160) + the cross-sub bypass (entity.rs:1129-1147) + wire lowering to produce real `Value::GeometryHandle` cells.
  - **Observable signal:** `param body : Solid = box(10mm, 20mm, 30mm)` in a fixture produces a value cell whose evaluated value is `Value::GeometryHandle { realization_ref: RealizationNodeId { entity: "Widget", index: 0 }, upstream_values_hash: <stable hash>, kernel_handle: <session id> }`. CLI inspection: `reify eval examples/solid-param-direct.ri` prints the handle. Cross-sub access: `outer.child.body` (a `let` ref to a sub's geometry) evaluates to the same handle.
  - **Prereqs:** GHR-β.
  - **Crates touched:**
    - `crates/reify-compiler/src/entity.rs` (remove bypass at :1129-1147; delete the four call sites of `is_solid_geometry_param` at :505-514, :537, plus the corresponding guards.rs sites; update value-cell creation to register Type::Geometry cells normally)
    - `crates/reify-compiler/src/geometry.rs` (delete `is_solid_geometry_param` function at :150-160)
    - `crates/reify-compiler/src/guards.rs` (update geometry-let registration at :179 to create normal cells)
    - `crates/reify-eval/src/engine_eval.rs` (lowering: wrap kernel dispatch result in `Value::GeometryHandle`)
    - `crates/reify-eval/src/engine_build.rs` (cooperation with realization-cache: handle production routes through the same cache lookup as today)
    - `crates/reify-compiler/tests/solid_param_tests.rs` (extend to assert value-cell content)
    - `examples/solid-param-direct.ri` (new fixture)
  - **Boundary tests:** §7.2 consumer-side rows "Spec-shape Physical instantiates without runtime panic" and "List of geometries."

### Phase 4 — Freshness walk + edit donation cascade

- **Task GHR-δ** — Extend `dependency_trace` for Realization → ValueCell edges; update `derive_output_freshness_for_node_with_cause`; extend edit donation hook for cross-kind cascade; lazy-revalidation logic.
  - **Observable signal:** Integration test in `crates/reify-eval/tests/geometry_handle_freshness.rs` (new): construct a structure with `param geometry : Solid = box(width, ...)`; mark `width` dirty; observe the GeometryHandle ValueCell transitions to Pending; observe the downstream StructureInstance ValueCell also transitions to Pending (cascade through SIR's recursive cache_key_of via field-level dependency). Re-eval: cells return to Final with new values. Snapshot clone with stale kernel_handle: first read re-resolves (instrumentation confirms slow-path fires).
  - **Prereqs:** GHR-γ.
  - **Crates touched:**
    - `crates/reify-eval/src/cache.rs` (extend `dependency_trace` schema; update `derive_output_freshness_for_node_with_cause`)
    - `crates/reify-eval/src/freshness_walk.rs` (Realization → ValueCell edges in the walk)
    - `crates/reify-eval/src/engine_edit.rs:2275-2301` (donation cascade)
    - `crates/reify-eval/src/engine_eval.rs` or new helper (lazy-revalidation on read of `Value::GeometryHandle`)
    - `crates/reify-eval/src/snapshot.rs` (revalidation hook at read boundary; or document that revalidation is per-cell-read and snapshot just carries the variant as Clone)
  - **Boundary tests:** §7.1 "Lazy revalidation: stale kernel_handle re-resolves," "Lazy revalidation: missing realization returns Undef," "Freshness walk: Realization → ValueCell edge," "Edit donation cascade." Plus §7.2 "Edit cascade through structure-instance."

### Phase 5 — Cache key composition + significance filter

- **Task GHR-ε** — Cache-key fragment in `value_to_cache_key` (cache.rs + persistent_cache.rs); significance filter arm.
  - **Observable signal:** `examples/spec-shape-physical.ri` evaluates; engine exits; engine restarts; same file re-opens — first evaluation hits persistent cache for the geometry-bearing Bracket; no kernel re-dispatch (verified via instrumentation). Edit the `box(width, ...)` width literal in the source; restart — cache miss on the realization (cache-key changed); fresh kernel dispatch; downstream FEA re-runs. Significance filter unit tests pass.
  - **Prereqs:** GHR-δ.
  - **Crates touched:**
    - `crates/reify-eval/src/cache.rs` (cache-key arm for `Value::GeometryHandle`)
    - `crates/reify-eval/src/persistent_cache.rs` (on-disk key serialization arm)
    - `crates/reify-eval/src/significance_filter.rs` (comparison arm + Equivalent decision)
    - `crates/reify-eval/tests/geometry_handle_persistent_cache_round_trip.rs` (new)
  - **Boundary tests:** §7.1 "Cache-key serialization," "Engine-restart cache-key stability"; §7.2 "Geometry-bearing structure cache round-trip," "Cache invalidation on geometry-call edit."

### Phase 6 — Kernel dispatch: terminal user-observable signal

- **Task GHR-ζ** — OCCT kernel queries for volume + centroid + area + bounding_box (+ length, perimeter as stretch). Each query consumes the `Value::GeometryHandle`, dereferences `kernel_handle` (with lazy revalidation), dispatches to the OCCT FFI, returns a typed `Value::Scalar` / `Value::Point` / `Value::BoundingBox`.
  - **Observable signal:** `reify eval examples/spec-shape-physical.ri` prints:
    ```
    Bracket {
      geometry: <Value::GeometryHandle>,
      material: Steel_AISI_1045 { density: 7800 kg/m³, ... },
      mass: 0.0468 kg,
      centroid: (5mm, 10mm, 15mm),
    }
    ```
    where mass and centroid are real numeric values matching the expected outputs for a 10×20×30mm steel block. CLI golden output committed. Additionally: `examples/spec-shape-physical.ri` re-runs hit the realization cache (no kernel re-dispatch — verified via instrumentation).
  - **Prereqs:** GHR-ε.
  - **Crates touched:**
    - `crates/reify-kernel-occt/src/queries.rs` (or equivalent — add Volume/Centroid/Area/BoundingBox/Length/Perimeter query implementations via the OCCT `BRepGProp::VolumeProperties`, `BRepBndLib::Add`, etc.)
    - `crates/reify-stdlib/src/geometry.rs` or `snapshot.rs` (eval-time dispatch: when `volume()` / `centroid()` / etc. are called with a `Value::GeometryHandle` argument, route to the kernel query)
    - `crates/reify-eval/src/geometry_ops.rs` or similar (kernel-query post-process integration)
    - `examples/spec-shape-physical.ri` (new fixture; produces the terminal observable)
    - `crates/reify-eval/tests/geometry_query_kernel_dispatch.rs` (new — pins each query's numerical output against analytic expected values for box/sphere/cylinder fixtures)
  - **Boundary tests:** §7.2 "Real `bracket.mass` and `bracket.centroid` evaluate."

### Phase 7 — Companion gap-register + cross-PRD sweeps

- **Task GHR-η** — Gap-register update + cross-PRD prose adjustment + GR-031 unblock note.
  - **Observable signal:** `git diff docs/architecture-audit/gap-register.md` shows GR-030 disposition updated to point at this PRD; GR-031 Notes row referenced as functionally unblocked by Phase 6; GR-018 cross-PRD relationship recorded.
  - **Prereqs:** GHR-ζ.
  - **Crates touched:**
    - `docs/architecture-audit/gap-register.md`
    - `docs/architecture-audit/findings/stdlib-trait-breadth.md` (M-007 + M-013 status updates: DRIFT → RESOLVED, ORPHAN → WIRED)
    - `docs/architecture-audit/findings/geometry-traits.md` (M-006 sequencing note pointing at GR-018)

- **Task GHR-θ** — Downstream PRD prose corrections.
  - **Observable signal:** Adjustments to `docs/prds/v0_3/structural-analysis-fea.md` and `docs/prds/v0_3/multi-load-case-fea.md` reflecting that Physical now has the spec shape; remove any flat-scalar workarounds.
  - **Prereqs:** GHR-ζ.

### Dependency view

```
SIR-α (structure-instance-runtime.md)
   │
   ▼
GHR-α (registrations + Physical restore)
   │
   ▼
GHR-β (variant + adapter sweep, wide-lock high-pri)
   │
   ▼
GHR-γ (lowering: retire bypasses)
   │
   ▼
GHR-δ (freshness walk + edit cascade + lazy revalidation)
   │
   ▼
GHR-ε (cache-key composition + significance filter)
   │
   ▼
GHR-ζ (kernel dispatch: volume/centroid/area/bbox/length/perimeter)
   │
   ├─→ GHR-η (gap-register + findings updates)
   └─→ GHR-θ (downstream PRD prose corrections)

External consumers unblocked by this PRD's completion:
   - Task 3468 / GR-031 (composed-stress envelopes)
   - multi-load-case-fea consumers with geometry-bearing Physical
   - GR-018 (unbounded primitives) gains the Bounded-check negative-path consumer surface
```

## §9 — Open questions (surfaced but not decided in this session)

1. **`upstream_values_hash` derivation algorithm.** The hash should be blake3 of canonicalized input values to the realization op. Canonicalization choice: sort sub-expressions by some stable order; serialize Value variants per existing cache.rs `value_to_cache_key`. Re-use existing `value_to_cache_key` machinery; do not invent a parallel canonicalization. **Decide during GHR-β** when the variant lands and the field needs population.

2. **Should kernel name participate in `upstream_values_hash`?** If a Bracket realizes via OCCT and the cache stores its handle, then a future evaluation under the Manifold kernel reading the same cache entry would silently consume the OCCT-derived result. Suggested default: include the active kernel name in the hash inputs so cross-kernel cache mixing is impossible. **Decide during GHR-ε** when cache-key composition is implemented.

3. **`Value::GeometryHandle` Display format.** For CLI / golden-output reproducibility. Suggested: `<Geometry: Bracket#realization[0]>` showing the realization-ref but not the session-scoped kernel_handle (which would break golden tests across runs). **Decide during GHR-β.**

4. **Snapshot revalidation cost amortization.** The "lazy revalidation on first read" policy hits a per-read `is_valid()` atomic load. For graphs with thousands of geometry-handle cells, this could become a hot path. Suggested measurement during GHR-δ; if measurable in benchmarks, add a "validated-in-current-Engine" sticky flag to the variant (additive). **Defer until profiling shows a problem.**

5. **`Value::GeometryHandle` in `Value::List` and `Value::Map`.** The cache-key fragment for a `List<Geometry>` recurses through each element's cache_key. For large lists (e.g. union-all of 1000 primitives), hash composition is O(N). Acceptable for v0.3; consider eager hash caching if measured. **Defer; not a blocker.**

6. **Geometry handles inside `Value::StructureInstance` whose containing structure does not declare a Bounded constraint.** A `Physical` structure with bare `param geometry : Solid` accepts unbounded geometry (per the locked design — Solid does not imply Bounded). Downstream consumers (e.g. FEA solver) that REQUIRE Bounded geometry must declare so on their own parameters. Today the existing surface is `fn solve_elastic_static(bracket: Physical)` plus a separate `param g : Bounded` slot — i.e. the FEA solver's input structure type carries Bounded via a member declaration, not via type-level composition of `Physical + Bounded`. Multi-bound composition at structure-instance member access (`param bracket : Physical where bracket.geometry : Bounded`) is a separate language-design question; current grammar does not support cross-member type predicates. **Out of scope for this PRD;** noted for follow-up under GR-018 sequencing.

7. **`@version(N)` on geometry handles.** SIR's `Value::StructureInstance` carries an explicit `@version` annotation for cache invalidation. Geometry handles don't have a parallel concept — the realization op itself is the source of truth, and `upstream_values_hash` invalidates on any change. If a need emerges to version-tag specific realizations (e.g. "the bracket geometry uses algorithm v2 now"), an additive `realization_version: u32` field on the variant + `@version` lowering for geometry-producing lets is possible. **Not pursued now.**

8. **Mesh-morph as a value-cell consumer.** When mesh-morph runs on a structure whose geometry field is a `Value::GeometryHandle`, does the morphed mesh become a new GeometryHandle (different realization_ref, since it's a derived realization) or a refinement of the same handle? Suggested: new realization-ref via the existing morph-ComputeNode-wrap mechanism in compute-node-contract.md §6 and mesh-morphing-phase-2.md. **Owned by mesh-morphing PRDs;** no work here.

9. **GR-018 sequencing.** This PRD lands the consumer surface for unbounded handles (the `geometry_traits_inference.rs` machinery already exists; new arms not required). GR-018 lands the producers (`half_space()`, `extrude_infinite()`). The Bounded negative-path test in §7.2 cannot be exercised until GR-018 lands; until then, the test is captured as a follow-up. **No new work in this PRD;** cross-PRD seam recorded in §10.

## §10 — Gap-register companion edits

In conjunction with PRD commit (separate task GHR-η in this same session if Leo approves):

- **GR-030** — disposition updated from `investigate-further` to `accept-and-resolve-via PRD`. Add `#### Follow-up PRD: docs/prds/v0_3/geometry-handle-runtime.md` sub-section. Note: "Resolution mode B+H. Phase 1-6 decomposition lands the spec-shape `Physical` end-to-end with real volume/centroid via OCCT kernel queries. Closes M-007 + M-013 ORPHAN in findings/stdlib-trait-breadth.md. Phase 1 hard-depends on SIR-α; cross-PRD edge wired."
- **GR-018 (cluster C-15 unbounded primitives)** — Notes row appended: "Consumer surface for unbounded geometry now lives at `docs/prds/v0_3/geometry-handle-runtime.md` — Bounded-required trait slots can be exercised with the negative path once half_space() / extrude_infinite() land per GR-018. No new PRD work in geometry-handle-runtime.md."
- **GR-031 (cluster C-29 composed/derived stress recovery)** — Notes row appended: "Functional unblock mechanism revised: `docs/prds/v0_3/structure-instance-runtime.md` SIR-α + `docs/prds/v0_3/geometry-handle-runtime.md` GHR-ζ. Task 3553 (SIR-γ envelope helpers, already filed; re-filed 2026-05-14 post-SIGABRT after the original #3468 was repurposed by curator recovery) executes against the joint foundation; 3553 → GHR-ζ cross-PRD edge wired at decompose time."
- **Findings updates** (`docs/architecture-audit/findings/stdlib-trait-breadth.md`): M-007 status `DRIFT → RESOLVED` once GHR-α lands; M-013 status `ORPHAN → WIRED` once GHR-ε lands. The audit-doc-rot from M-002 is unchanged by this PRD.

## §11 — Cross-PRD relationship table (G4)

| Other PRD / GR | Direction | Mechanism crossing the seam | Owner |
|---|---|---|---|
| `structure-instance-runtime.md` (GR-001) | This PRD **consumes** | `Value::StructureInstance` variant + member access on StructureInstance + cache_key_of recursion. GHR-α's `material.density` access requires SIR-α's member-access wiring. GHR-ε's cache-key fragment composes uniformly with SIR's. | **SIR owns.** Hard prereq dependency on SIR-α at GHR-α decompose-time. |
| `compute-node-contract.md` (GR-002) | This PRD **does NOT consume** | Per §1 locked decision: kernel queries (volume/centroid/etc.) route DIRECTLY through `reify-kernel-occt`, not via ComputeNode. ComputeNode wrap is future-PRD work only if profiling justifies. | **No cross-PRD dependency.** Phase 6 is independent of CN-contract η. |
| GR-018 (unbounded primitives, cluster C-15) | This PRD **provides consumer surface** | The `geometry_traits_inference.rs` Bounded machinery is wired today; this PRD makes its negative-path test exercisable by making Type::Geometry flow through value cells. GR-018 produces the unbounded primitives that trigger the negative path. | **GR-018 owns its production**; this PRD's consumer surface is the existing inference machinery. No cross-PRD task ownership ambiguity. |
| `multi-load-case-fea.md` | This PRD **unblocks** | Multi-load-case-fea's `LoadCase + Bracket : Physical` shape with real geometry. Currently blocked by SIR + the absence of usable Solid in trait slots. | **multi-load-case-fea consumer-side adjustments** in GHR-θ (downstream PRD prose corrections). |
| `structural-analysis-fea.md` | This PRD **unblocks** | Similar shape: FEA's `Material` + `Physical` + `Load` + `Support` composition with geometry-bearing structures. | **structural-analysis-fea adjustments** in GHR-θ. |
| `mesh-morphing-phase-2.md` | Orthogonal | Mesh-morph produces a Volumetric realization output, not a Solid value-cell handle. No direct interaction. Future composability: a morphed bracket's geometry could be a `Value::GeometryHandle` whose realization_ref points at the morph realization. | **Owned by mesh-morphing PRDs;** future composability flagged in §9. |
| `persistent-fea-cache.md` (GR-032) | Composable | This PRD's persistent cache-key fragment for `Value::GeometryHandle` participates uniformly. No new mechanism. | **persistent-fea-cache PRD unchanged** by this PRD. |
| `node-traits-unification.md` (GR-038) | Orthogonal | Trait-typed nominal conformance machinery is shared; this PRD doesn't change the conformance contract. | **node-traits-unification owns its scope.** |

No reciprocal "the other owns it" ambiguity. Cross-PRD seam ownership is clean.
