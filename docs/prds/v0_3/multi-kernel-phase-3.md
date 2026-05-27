# Multi-Kernel Phase 3 — ReprKind Chain Coverage

Status: contract (extends `docs/prds/v0_2/multi-kernel.md` whose Phases 1+2 shipped 2026-04-28). Authored 2026-05-12 in interactive session. Pending Leo approval before queueing tasks.

Resolves cluster C-18 / gap GR-020 per `docs/architecture-audit/gap-register.md`. Folds in GR-034 (cluster C-32 — long-chain diagnostic / per-stage tolerance budget unreachable). Hosts the multi-kernel half of GR-003 (OpenVDB consumer wiring) per the 2026-05-12 contested-ownership disposition.

## §0 — Purpose and supersession

This document is the **contract** for the multi-kernel dispatch seam at op-execution time. v0.2 shipped:

- The `CapabilityDescriptor { supports: Vec<(Operation, ReprKind)> }` shape (audit M-002).
- The static-`inventory` kernel registration mechanism (M-003).
- The `dispatcher::dispatch` BFS algorithm and `DispatchPlan` type (M-004 isolated).
- The `RealizationCache` two-level keying on `(repr_kind, entity_id, tol)` (M-006).
- Four kernel adapters (OCCT, Manifold, Fidget, OpenVDB) submitting capability descriptors (M-009/M-010/M-011).
- The `is_long_chain_realization` predicate + `long_chain_diagnostic` builder (M-017).

Phase 3 lights up the **consumer half**. The 2026-05-12 architecture audit found seven structural gaps that share one root cause — the v0.2 build-out stopped at the dispatcher boundary and never wired into `execute_realization_ops`:

1. **No Convert edges declared** by any production capability descriptor (M-007). The BFS algorithm is exercised only by tests.
2. **`execute_realization_ops` ignores the dispatcher** — it forwards every op through a single startup-picked kernel (M-004 evidence at `engine_build.rs:1720`).
3. **`RealizationCache` keying hard-codes `ReprKind::BRep`** at every call site (M-006 evidence at `engine_build.rs:1666`, `1966`).
4. **`Engine.geometry_kernel: Option<Box<dyn GeometryKernel>>`** holds one kernel, not the registered set (M-005). The `with_registered_kernel` constructor picks one via `pick_lexmin_brep_kernel` and pins it for the engine's life.
5. **No per-realization `produced_repr` tag** — realization nodes don't record which `ReprKind` they ended up at (M-014 stack pattern fiction).
6. **`force_tet` and similar option fields are not in any compute key** (hex-wedge M-024). Two solves on identical geometry with different `force_tet` values today share a cache slot.
7. **`#kernel(...)` pragma is parsed and dropped** (M-016). `reify.toml` project pin parses but is unconsumed (M-013).

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (`preferences_implementation_chain_naming`) — is what this contract is designed to prevent for the multi-kernel seam specifically. Resolution mode is **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline. Cross-crate blast radius is ≥ 6 (reify-eval, reify-types, reify-kernel-occt/-manifold/-fidget/-openvdb, reify-config, reify-compiler) and the seam is load-bearing (FEA, shells, hex/wedge, imported-field-source all depend on it), so bare B is insufficient.

This document is named in `docs/architecture-audit/gap-register.md` GR-020 and GR-034. The cross-PRD relationship with `compute-node-contract.md` is settled here (§4 / §6): the two are **separate dispatch surfaces** that meet at the cache-key boundary; neither subsumes the other.

## §1 — What this PRD owns vs. defers

**Owns.** The op-execution dispatch surface for geometry kernels: BFS-driven kernel selection per op, Convert-edge inventory in production capability descriptors, multi-handle engine model, per-realization `ReprKind` tagging, cache-key composition over conversion chains, the `force_tet` / per-op-option fold-in, `#kernel(...)` pragma activation, `reify.toml` pin consumer-side enforcement, long-chain diagnostic wiring (GR-034), and the OpenVDB consumer arm in `engine_eval.rs::elaborate_field` (GR-003).

**Defers.**

- **Mesh→BRep** conversion (research-grade). Out of scope. Mesh-output ops produce realizations that consume downstream as Mesh; no surface reconstruction path. Any user need for BRep-from-mesh is explicitly an error diagnostic in v0.3.
- **HDF5 / CSV** imported-field-source. GR-003's HDF5/CSV sibling extends this contract after OpenVDB lands; tracked under `docs/prds/v0_3/imported-field-source-hdf5-csv.md` whose preconditions land here.
- **Manifold `propagate_attributes` MeshGL walk** — owned by `persistent-naming-v2.md` per the 2026-05-12 GR-004 disposition. The trait wiring is here (Manifold's `attribute_hook` returns `Some(self)`), the body lives there.
- **Truck** kernel — explicitly dropped from v0.2 and not revisited.
- **GPU offload, dylib plugin loading, runtime kernel discovery** — v0.4+ if they materialise.
- **The ComputeNode dispatch surface** — separate per `compute-node-contract.md`. See §6 below.
- **Stack-pattern and patchwork-pattern assembly-level orchestration** for heterogeneous assemblies (audit M-014/M-015). Per-op dispatch through chains lands here; the assembly-level abstraction that threads heterogeneous handles through ordered ops is a v0.4 concern. This PRD makes the assembly-level abstraction **possible** (the cache, dispatcher, and per-realization tagging are in place) without **shipping** it.

## §2 — Convert-edge inventory

The dispatcher's BFS expansion needs production-side conversion edges; v0.2 ships zero (M-007). Phase 3 ships the minimum set that closes the cluster C-18 gaps without speculation.

**First-class edges (have explicit registry entries):**

| Edge | Producer kernel | Status today | Notes |
|---|---|---|---|
| `Convert { from: BRep } → Mesh` | OCCT | FFI exists (`tessellate(...)`); descriptor entry missing | Unblocks Manifold-via-OCCT booleans (M-009). OCCT `register.rs:26-34` documents the missing entry as a v0.3 forward-compat plan. |
| `Convert { from: Mesh } → Voxel` | OpenVDB | FFI exists (`kernel_real.rs:82 realize_voxel_from_mesh`); descriptor entry missing | Unblocks the shells M-025 BRep→Mesh→Voxel chain. |
| `Convert { from: Voxel } → Mesh` | OpenVDB | Documented as deferred follow-up (`register.rs:30-33`); FFI work needed | Marching-cubes / level-set surfacing. Required for downstream consumers that need to surface a Voxel back to Mesh (e.g. visualization, FEA seed mesh). |
| `Convert { from: Mesh } → VolumeMesh` | Gmsh | Already declared (`crates/reify-kernel-gmsh/src/register.rs:96`) | Pre-existing; lifted into this inventory for completeness. |
| `Convert { from: Sdf } → Mesh` | Fidget | FFI gap; declared as v0.3 follow-up | Iso-surface meshing of Fidget SDFs. Required for visualization and for any consumer that needs a Mesh from a `field def` SDF. |

**Derived edges (composed from primitives at dispatch time; no explicit registry entry needed):**

- `BRep → Voxel` = OCCT-tessellate → OpenVDB-mesh-to-voxel. Dispatcher's BFS handles this from the first-class edges.
- `BRep → VolumeMesh` = OCCT-tessellate → Gmsh-surface-to-volume. Dispatcher's BFS handles this.
- `Sdf → Voxel` = Fidget-iso-mesh → OpenVDB-mesh-to-voxel. Dispatcher's BFS handles this.

**Out of scope (v0.3 explicit non-goals; user-visible diagnostic on demand):**

- `Mesh → BRep` (research-grade reconstruction; no kernel claims this).
- `Voxel → BRep` (would chain through Mesh → BRep; same blocker).
- `VolumeMesh → BRep` (same).
- `VolumeMesh → Sdf` (no consumer demand).

When a dispatch demand walks the BFS and exhausts visited states without reaching `demanded`, the dispatcher returns `None`. The op-execution caller surfaces `Diagnostic::NoKernelChain` (new variant) naming the demand and the available reprs. The error is user-visible — failing closed is the failure mode, not silently routing to a wrong kernel.

**Edges that EXIST as descriptor entries but DO NOT execute** — e.g. Fidget declares `(BooleanUnion, Sdf)` but the Fidget kernel's `execute` arm for Boolean ops is wired against the tree-construction API. The full execution-side validation of each declared edge is a per-task acceptance check; the audit's M-007/M-009/M-010/M-011 findings document the current state by kernel.

## §3 — Per-realization `ReprKind` tracking

**Tracking shape: per-realization tag on `RealizationNodeData`.** Today, `RealizationNodeData` carries `value_inputs`, `resolution_inputs`, `realization_inputs` — but no `produced_repr`. Add:

```rust
pub struct RealizationNodeData {
    // ... existing fields ...
    pub produced_repr: ReprKind,    // NEW — what ReprKind this realization ended up at
}
```

**Why per-realization, not per-handle.**

The audit's open question Q-MK3-3 asked: `Map<HandleId, ReprKind>` on Engine, per-realization tag, or both? Answer: **per-realization only.**

- Kernel-internal handles (OCCT `TopoDS_Shape`, Manifold `MeshGL`, OpenVDB grid handles) are short-lived value-objects local to each kernel. Engine-level handle tracking would duplicate state already implicit in the kernel adapters.
- The realization node is the cache-key boundary (M-006). Tagging it with `produced_repr` lets the dispatcher and cache co-operate without a separate map.
- A single op's output can be one realization in one repr; if a downstream op demands a different repr, a Convert edge produces a **new** realization node with the new tag, keyed by the new `repr_kind` in the cache.

**Engine model: multi-kernel.** `Engine.geometry_kernel: Option<Box<dyn GeometryKernel>>` becomes:

```rust
pub struct Engine {
    // ... existing fields ...
    geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>>,    // NEW shape
    // OBSOLETE: geometry_kernel: Option<Box<dyn GeometryKernel>>,
}
```

`with_registered_kernels` (new constructor) instantiates one adapter per registered descriptor, keyed by kernel name. `pick_lexmin_brep_kernel` retires — it's a v0.2 single-kernel artifact. `execute_realization_ops` consults the registry per op via `dispatch(...)`. The BTreeMap ordering preserves the determinism contract (`dispatcher.rs:23-31`).

**Backward compatibility.** No existing public API breaks: `Engine::with_registered_kernel` is renamed to `with_registered_kernels` (plural) at the same call sites; the old name becomes a deprecation alias for one minor cycle.

## §4 — Cache-key composition over ReprKind chains

The `RealizationCache` keys by `(repr_kind, entity_id, tol)` already. Phase 3 makes the keying **work** by ensuring every call site passes the actual `repr_kind` rather than the hard-coded `ReprKind::BRep` (the M-006 evidence).

**Chain composition is implicit via the realization node graph.** A BRep→Mesh→Voxel chain materialises as three realization nodes (one per step), each cached at its own `(repr_kind, entity_id, tol)` slot. Reuse follows from the graph: a second consumer demanding `Mesh` for the same entity at the same tol hits the cache at the Mesh node; no re-tessellate.

This is the answer to Q-MK3-2 (chain cache-key shape): the per-step shape **IS** the chain shape. There is no separate "chain key" type; the graph is the chain. The dispatcher produces a `DispatchPlan` naming the conversion stages, the executor materialises one realization node per stage, and the existing partial-order `(repr_kind, entity_id, tol)` keying does the rest.

**Per-op option folding into the cache key.** v0.2 doesn't fold `force_tet` (hex-wedge M-024) or analogous per-op option fields into the realization cache key. The result: two solves on identical geometry with different `force_tet` values share a slot, returning wrong results.

Phase 3 extends the realization cache key from `(repr_kind, entity_id, tol)` to `(repr_kind, entity_id, tol, options_hash)`:

```rust
pub struct RealizationCacheKey {
    pub entity_id: String,
    pub repr_kind: ReprKind,
    pub tol: f64,
    pub options_hash: ContentHash,    // NEW — folds force_tet, openvdb voxel_size, marching-cubes iso_level, etc.
}
```

`options_hash` is computed at op-execution time by hashing the per-op option struct (`ElasticOptions`, `VolumeMeshOptions`, `MarchingCubesOptions`, etc.) deterministically. **Each op type names its options-hash producer** — analogous to how `ComputeNodeData.options_hash` is computed by upstream consumers per `compute_cache_key.rs:54-66`. The kernel adapter that executes the op is responsible for hashing its own options struct; this avoids `RealizationCache` knowing about every kernel's option vocabulary.

**Per-op options-hash producer registry (mirrors the kernel-registration pattern):**

| Operation | Options struct | Producer crate |
|---|---|---|
| `Operation::MeshSurfaceToVolume` | `VolumeMeshOptions { force_tet, require_hex_wedge, gmsh_2d, sweep_step }` | `reify-kernel-gmsh` |
| `Operation::Convert { from: Mesh } → Voxel` | `MeshToVoxelOptions { voxel_size, narrow_band }` | `reify-kernel-openvdb` |
| `Operation::Convert { from: Voxel } → Mesh` | `MarchingCubesOptions { iso_level, adaptive }` | `reify-kernel-openvdb` |
| `Operation::Convert { from: BRep } → Mesh` | `TessellateOptions { angular_deflection, linear_deflection }` | `reify-kernel-occt` |
| `Operation::Convert { from: Sdf } → Mesh` | `IsoMeshOptions { iso_value, target_edge_length }` | `reify-kernel-fidget` |
| `Operation::BooleanUnion/Difference/Intersection` | (no options today; pin `ContentHash(0)`) | per-kernel |

`options_hash = ContentHash(0)` is the explicit "no options" sentinel for ops that have no parameterisation; mismatched-by-zero is bit-exact, matching the existing `compute_cache_key.rs` convention.

**Relationship to `ComputeNode.options_hash`.** Both surfaces use the same `ContentHash`-of-options-struct pattern; the two cache keys are independent (realization cache vs. compute cache) but the hashing convention is shared. ComputeNode consumers that wrap a realization op (e.g. `solve_elastic_static` consuming a `VolumeMesh` realization) pass the realization's `options_hash` through their own `options_hash` composition — no double-counting, but full transitive invalidation when an upstream option changes.

## §5 — `#kernel(...)` pragma and project-pin consumers

Both mechanisms parse correctly (audit M-013, M-016) but no engine code reads them. Phase 3 wires both.

**`#kernel(...)` pragma — per-op-site override.**

```
#kernel(manifold)
let result = a | b;     // Boolean union; pragma forces Manifold-Mesh path
```

Today, `module.kernel_pragma: Option<String>` populated by `module_pragmas.rs:682-758` is dropped at the engine seam. Phase 3:

- `module.kernel_pragma` propagates to `execute_realization_ops` via the existing realization-op carriage (the realization node carries its source-site metadata already).
- The dispatcher gains a `prefer_kernel: Option<&str>` parameter. When `Some(name)`:
  - If the named kernel is in the registry AND its capability descriptor supports `(op, demanded)`, prefer it.
  - Otherwise emit `Diagnostic::KernelPragmaUnsatisfiable { pragma_kernel, op, demanded }` (warning, not error — fall through to default lex-min selection so the user's design still evaluates).

This preserves the determinism contract: pragma steers; absent pragma, lex-min picks.

**`reify.toml` project pin — startup consistency check.**

Today, `Manifest::kernel_pins` is parsed but unconsumed (M-013). Phase 3:

- `Engine::with_registered_kernels` reads `Manifest::kernel_pins` (when a manifest path is supplied).
- At engine construction, compare the parsed pin set against the registered (compile-time-feature-gated) adapter set.
  - **Pin name not in registry** → `Diagnostic::PinnedKernelMissing { kernel_id }` (error; engine refuses to start). The user pinned a kernel that this build doesn't include.
  - **Registry name not pinned** → `Diagnostic::UnpinnedKernelLoaded { kernel_id }` (warning; engine starts). The user didn't pin a kernel that this build includes; non-fatal.
  - **Pin version mismatch with adapter `VERSION` constant** → `Diagnostic::KernelVersionMismatch { kernel_id, pinned, actual }` (error). Determinism contract enforcement.

The cache does not need to know about kernel versions because a version mismatch refuses to start the engine — the v0.2 PRD's determinism rationale ("a version change forces a process restart") gains the missing enforcement.

## §6 — Relationship to ComputeNode dispatch (Q-MK3-4)

Multi-kernel dispatch is a **separate dispatch surface** from ComputeNode. They are **not** unified.

**Rationale.** `compute-node-contract.md` §6 names the consumer policy: ComputeNode admits features that are (1) graph-participant Value or Realization outputs AND (2) expensive (≥ ~50 ms wall-clock heuristic). The §6 table explicitly lists **Builtin OCCT ops (boolean fuse, fillet, chamfer)** as **not** routing through ComputeNode — "Already realization-cached at content-hash granularity; no warm-state benefit; per-op duration below threshold."

The same reasoning extends to all kernel-internal ops: tessellation, Boolean Mesh, Voxel rasterise, marching-cubes meshing. They are realization-cached at the `RealizationCache` granularity; they have no warm-state to preserve across runs; their per-op duration is below the ComputeNode threshold; and they are dispatched per-op via the `dispatcher::dispatch` BFS, not via `Engine::insert_compute_node`.

**Where the two surfaces meet.** At the cache-key boundary, transitively. A ComputeNode whose `realization_inputs` reference a Gmsh-VolumeMesh realization picks up the VolumeMesh realization's `options_hash` (force_tet, hex/wedge classification) through `compute_cache_key.rs::compute_cache_key`'s `realization_bucket_hash` field. Cache invalidation propagates correctly: changing `force_tet` re-keys the VolumeMesh realization, which re-keys the FEA ComputeNode that consumes it.

**Where they explicitly do NOT meet.**

- No `Operation::Convert { from: X } → Y` routes through ComputeNode. Conversions are kernel-internal dispatch.
- No kernel adapter is registered via `register_compute_fn`. Kernel registration uses `inventory::submit!` per §4 of the v0.2 PRD; ComputeNode registration uses `register_compute_fn` per `compute-node-contract.md` §4.
- The long-chain diagnostic (M-017, GR-034) is a multi-kernel concern; ComputeNode's cancellation / pending mechanism is orthogonal.

**Per-feature disposition (v0.3 corpus, mirroring `compute-node-contract.md` §6):**

| Feature | Multi-kernel dispatch? | ComputeNode? | Rationale |
|---|---|---|---|
| Mesh Boolean (Manifold) | Yes | No | Per-op kernel selection; below ComputeNode threshold. |
| BRep Boolean (OCCT) | Yes (default kernel) | No | Same. |
| OCCT tessellate (BRep→Mesh) | Yes (Convert edge) | No | Same. |
| Gmsh surface→volume mesh | Yes (Convert edge) | No | Per-op; below threshold. |
| OpenVDB mesh→voxel rasterise | Yes (Convert edge) | No | Same. |
| OpenVDB voxel→mesh marching cubes | Yes (Convert edge) | No | Same. |
| Fidget SDF iso-meshing | Yes (Convert edge) | No | Same. |
| `solve_elastic_static` | No | Yes | FEA-scale, seconds; warm-state benefit. (compute-node-contract §6 row.) |
| Mesh-morph | No (its inputs are dispatched) | Yes | Realization output, mesh-size-expensive. (compute-node-contract §6 row.) |
| OpenVDB ingest from external file | No | Yes (per compute-node-contract §6) | One-time load; persistent-cache tier. The **dispatcher** is the kernel-side wiring; the **ComputeNode** wraps the ingest-as-trampoline. Two seams meet here cleanly. |

## §7 — Boundary test sketch (cross-crate; facing both ways)

Tests live in `crates/reify-eval/tests/` (engine-level integration) and per-kernel `crates/reify-kernel-*/tests/dispatcher_integration.rs` (one already exists per kernel; extend them). The seam is between `reify-eval` (engine + dispatcher + realization cache) and each kernel crate. Tests cross from each side.

### 7.1 Producer-side (reify-eval looks outward at registered kernel descriptors)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Single-edge dispatch.** Demand `(BooleanUnion, Mesh)` with available `{Mesh}`. | Manifold registered; OCCT registered. | Dispatcher returns `DispatchPlan { kernel: "manifold", conversions: [] }`. No conversion stages. |
| **One-step Convert.** Demand `(BooleanUnion, Mesh)` with available `{BRep}`. | OCCT registered with `Convert { from: BRep } → Mesh`; Manifold registered with `(BooleanUnion, Mesh)`. | Plan: `kernel: "manifold", conversions: [("occt", BRep, Mesh)]`. Execution materialises an intermediate Mesh realization; second consumer of `Mesh` for same entity at same tol hits the cache. |
| **Two-step Convert.** Demand `(BooleanUnion, Voxel)` with available `{BRep}`. | OCCT `BRep → Mesh`; OpenVDB `Mesh → Voxel`; OpenVDB `(BooleanUnion, Voxel)`. | Plan: 2-stage chain through Mesh. Three realization nodes materialise: BRep, Mesh, Voxel. |
| **Unreachable demand.** Demand `(BooleanUnion, BRep)` with available `{Mesh}`. | No kernel declares `Convert { from: Mesh } → BRep`. | Dispatcher returns `None`. Op execution emits `Diagnostic::NoKernelChain { op, demanded, available }` and the realization transitions to `Freshness::Failed`. |
| **Determinism under registry order.** Two builds with identical `Cargo` features and identical `reify.toml` pins; randomized hash-seed environment. | All four kernels registered; both OCCT and Manifold support `(BooleanUnion, Mesh)`. | Same `DispatchPlan` across 100 runs (lex-min on kernel name: `manifold` < `occt`). |
| **Pragma steering.** `#kernel(occt)` in module scope; demand `(BooleanUnion, Mesh)` with available `{Mesh}`. | Both Manifold and OCCT support; OCCT supports via `tessellate`-then-something pathway (test fixture). | Plan picks OCCT (pragma) when supported; emits `Diagnostic::KernelPragmaUnsatisfiable` and falls through to lex-min when pragma kernel doesn't support `(op, demanded)`. |
| **Project-pin enforcement.** `reify.toml` pins manifold=0.1.x; Manifold adapter VERSION = "0.2.0". | Engine construction. | Returns `Err(Diagnostic::KernelVersionMismatch)`. Engine does not start. |
| **Project-pin missing kernel.** `reify.toml` pins `truck` which is not in the registry. | Engine construction. | Returns `Err(Diagnostic::PinnedKernelMissing)`. |
| **Cache key option-folding.** Identical geometry, two solves with `force_tet={true,false}`. | Gmsh `Operation::MeshSurfaceToVolume` registered; `VolumeMeshOptions.force_tet` hashed into `options_hash`. | Two distinct cache slots; second solve does not return first solve's result. |
| **Long-chain diagnostic firing.** Synthetic 3-stage chain (BRep → Mesh → Voxel → Mesh) where total wall time > 500 ms. | Chain count and wall time tracked through `execute_realization_ops`. | `Diagnostic::LongChainRealization` emitted with the chain naming. (GR-034 fold-in.) |
| **Long-chain diagnostic non-firing on short chain.** 1-stage chain; identical wall time. | Same instrumentation. | No diagnostic. |
| **Per-realization tag.** Demand `(BooleanUnion, Mesh)` with available `{BRep}`. | OCCT `Convert { from: BRep } → Mesh`; Manifold `(BooleanUnion, Mesh)`. | The intermediate realization's `RealizationNodeData.produced_repr == ReprKind::Mesh`. Final realization's `produced_repr == ReprKind::Mesh`. |

### 7.2 Consumer-side (kernel crates + downstream PRDs look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Manifold mesh-boolean end-to-end.** A `.ri` file declares two BRep solids and computes their union; result is a `Value::Geometry` with `produced_repr == ReprKind::Mesh`. CLI evaluation emits a parameter-rounded vertex count consistent with Manifold's output. | OCCT `Convert { from: BRep } → Mesh` edge; Manifold `(BooleanUnion, Mesh)`. No `#kernel(...)` pragma. | Result count matches reference within 1%; engine inspection confirms intermediate Mesh realization in graph. Re-running same `.ri` hits realization cache (instrumentation). |
| **OpenVDB imported-field consumer.** A `.ri` file with `field def fea_stress : ... source = imported { path = "fixture.vdb"; format = OpenVDB; grid = "vonMises"; units = MPa }`. CLI evaluation produces a `Value::Field` whose interior sampling matches the fixture file at sampled points within tolerance. | `engine_eval.rs:621` `CompiledFieldSource::Imported` arm wires through `reify-kernel-openvdb::ingest`; OpenVDB adapter registered. | `result.sample(pt) == expected_value` within ε. Imported-file-content-hash invalidates cache when the file changes. (GR-003 disposition.) |
| **Shells BRep→Voxel chain end-to-end.** `examples/shells/thin_walled_bracket.ri` (the shells PRD T23 end-to-end demo) produces a Voxel realization for the volumetric-extraction stage. | OCCT `BRep→Mesh`; OpenVDB `Mesh→Voxel`; OpenVDB `(SampleField, Voxel)` (or equivalent shells consumer op). | The shells PRD's mid-surface segmentation seeds from a real OpenVDB voxel grid (not synthetic `SampledField`). Lifts shells M-025 from PARTIAL to WIRED. |
| **Hex-wedge `force_tet` cache discipline.** Two `solve_elastic_static` invocations on identical geometry: first with `force_tet=true`, second with `force_tet=false`. | `VolumeMeshOptions` registered as options-hash producer. | Two distinct VolumeMesh realizations; FEA results differ measurably; instrumentation confirms second solve did NOT reuse first solve's realization. (Lifts hex-wedge M-024 from PARTIAL to WIRED.) |
| **`#kernel(...)` pragma round-trip.** A `.ri` file with `#kernel(manifold)` at module scope performing a Boolean union; engine inspection confirms Manifold was selected. | Manifold and OCCT both registered; both support `(BooleanUnion, Mesh)` post-Convert-edge. | Realization graph reports `kernel: "manifold"` on the boolean realization; CLI `--verbose` confirms. Without the pragma, lex-min selects Manifold anyway (lex order); test pins both behaviors. |
| **Project-pin happy path.** `reify.toml` pins {occt=0.1, manifold=0.1, fidget=0.1, openvdb=0.1}; all adapter VERSIONs match. | Engine construction. | Engine starts. No diagnostics emitted at startup. |
| **Project-pin unpinned-loaded warning.** `reify.toml` pins {occt, manifold}; fidget is built into the binary. | Engine construction. | Warning diagnostic; engine starts. |

## §8 — Integration DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its **user-observable signal**. Producer-only tasks closed in isolation are no longer tolerable (`feedback_task_chain_user_observable`).

The DAG threads through `compute-node-contract.md` η (FEA first real consumer) only at the **hex-wedge `force_tet` cache discipline** slice (Phase 6 task ξ), which validates that ComputeNode's transitive cache-key composition through the realization layer is sound. Otherwise, the two PRDs are parallelisable.

### Phase 1 — Foundation supplements (small; unblock the rest)

- **Task α** — `RealizationNodeData.produced_repr: ReprKind` field added; written at op-execution time; readable via engine inspection.
  - **Observable signal:** Unit test in `reify-eval` pins: after `execute_realization_ops`, every realization node's `produced_repr` matches the actual ReprKind of its stored value (verified against the kernel adapter's output type).
  - **Prereqs:** None.
  - **Crates touched:** reify-eval (graph.rs), reify-types (RealizationNodeData re-export if needed).

- **Task β** — `RealizationCacheKey` extended to include `options_hash: ContentHash`.
  - **Observable signal:** Unit test pins `RealizationCache::insert`/`lookup` round-trip with two distinct `options_hash` values returning two distinct slots; partial-order tolerance lookup still works within a fixed `options_hash`.
  - **Prereqs:** None.
  - **Crates touched:** reify-eval (realization_cache.rs, tolerance_bucket.rs).

- **Task γ** — `Diagnostic::NoKernelChain { op, demanded, available }`, `Diagnostic::KernelPragmaUnsatisfiable { ... }`, `Diagnostic::PinnedKernelMissing { ... }`, `Diagnostic::UnpinnedKernelLoaded { ... }`, `Diagnostic::KernelVersionMismatch { ... }` typed codes added to `reify-types::DiagnosticCode`.
  - **Observable signal:** Unit tests in `reify-types` pin the code → message mapping; each diagnostic surfaces under `reify check` with a clean message.
  - **Prereqs:** None.
  - **Crates touched:** reify-types.

### Phase 2 — Vertical slice (BRep→Mesh→Manifold-Boolean end-to-end)

- **Task δ** — OCCT `Convert { from: BRep } → Mesh` capability-descriptor entry added; OCCT `execute` arm dispatches to `tessellate(...)` for the new entry; per-op `TessellateOptions` hashed into `options_hash`.
  - **Observable signal:** `crates/reify-kernel-occt/tests/dispatcher_integration.rs` test pins: dispatcher returns `DispatchPlan { kernel: "occt", conversions: [("occt", BRep, Mesh)] }` for `(BooleanUnion, Mesh)` with available `{BRep}`; OCCT's `execute` produces a Mesh value from a BRep input via tessellation.
  - **Prereqs:** α, β, γ.
  - **Crates touched:** reify-kernel-occt (register.rs, kernel.rs).

- **Task ε** — `Engine.geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>>` shape; `with_registered_kernels` constructor; `execute_realization_ops` consults `dispatcher::dispatch` per op and routes to the named kernel from the BTreeMap.
  - **Observable signal:** `crates/reify-eval/tests/multi_handle_engine_dispatch.rs` (synthetic registries + recording kernels) pins: `execute_realization_ops` consults `dispatcher::dispatch` per op, routes each op to the named kernel from the `geometry_kernels` BTreeMap, and writes the produced `ReprKind` to each `RealizationNodeData.produced_repr`; re-running hits the realization cache (dispatch-count instrumentation). At the ε baseline (demanded repr = BRep) dispatch returns a 0-conversion plan; non-empty conversion chains are surfaced via a diagnostic rather than executed.
  - **Premise note (G6, esc-3436-210):** ε delivers the dispatch-routing seam *only*. The BRep→Mesh→Manifold-Boolean end-to-end output is **not** observable at ε — the Manifold execute arm is task ζ and the OpenVDB consumer is task η, both of which *depend on* ε. That end-to-end signal is owned by ζ's leaf (`examples/multi_kernel/manifold_boolean.ri`), not ε. The original ε signal demanded a Mesh/Manifold output unbuildable from ε's dependency set (δ supplies the `Convert` *descriptor* only; α supplies the `produced_repr` field) — corrected here per G6's dependency-set trace.
  - **Prereqs:** δ, α (so realization tags populate correctly).
  - **Crates touched:** reify-eval (engine_admin.rs, engine_build.rs, lib.rs).
  - **Supersedes:** the v0.2 single-kernel `Engine.geometry_kernel: Option<Box<dyn GeometryKernel>>` shape.

### Phase 3 — Manifold consumer wired

- **Task ζ** — Manifold adapter's `execute` arm for `(BooleanUnion/Difference/Intersection, Mesh)` validated against `manifold3d` and integrated with realization-graph output.
  - **Observable signal:** `examples/multi_kernel/manifold_boolean.ri` (new fixture) computes a non-trivial Boolean (two interlocking gear-like shapes) where the OCCT path historically fails. Manifold path produces a manifold-output result; CLI `--verbose` confirms `kernel: manifold` was selected. Result vertex count is within Manifold's tolerance of the analytical-equivalent.
  - **Prereqs:** ε.
  - **Crates touched:** reify-kernel-manifold (kernel.rs validation of FFI), reify-eval (output-value wrap).

### Phase 4 — OpenVDB consumer wired (GR-003 fold-in)

- **Task η** — OpenVDB `Convert { from: Mesh } → Voxel` capability-descriptor entry; FFI is `realize_voxel_from_mesh` (already exists at `kernel_real.rs:82`); per-op `MeshToVoxelOptions` hashed.
  - **Observable signal:** `crates/reify-kernel-openvdb/tests/dispatcher_integration.rs` test pins: dispatcher returns 2-stage chain for `(SampleField, Voxel)` with available `{BRep}`; OpenVDB's `execute` produces a Voxel grid from a Mesh input via the FFI.
  - **Prereqs:** δ, ε.
  - **Crates touched:** reify-kernel-openvdb (register.rs, kernel_real.rs wrapper).

- **Task θ** — `engine_eval.rs:621` `CompiledFieldSource::Imported` arm replaced — instead of `Value::Undef`, route through `reify-kernel-openvdb::ingest::load_field_from_path` (consumer of M-011's existing ingest infrastructure).
  - **Observable signal:** `examples/imported_field/openvdb_stress.ri` declares `field def fea_stress : ... source = imported { path = "fixtures/sample.vdb"; format = OpenVDB; ... }`; CLI evaluation samples the field at a few coordinates and prints values that match the fixture file's expected samples within ε.
  - **Prereqs:** η. Plus the v0.2 imported-field-source PRD's tasks 2667/2668 (parser-side; already wired per M-001).
  - **Crates touched:** reify-eval (engine_eval.rs), reify-kernel-openvdb (ingest.rs API surface stable).
  - **Resolves:** GR-003 (cluster C-17) per the 2026-05-12 contested-ownership disposition.

### Phase 5 — Voxel→Mesh + Sdf→Mesh follow-on convert edges

- **Task ι** — OpenVDB `Convert { from: Voxel } → Mesh` (marching cubes) capability descriptor + FFI implementation; per-op `MarchingCubesOptions` hashed.
  - **Observable signal:** `examples/multi_kernel/voxel_to_mesh.ri` materialises an OpenVDB voxel grid (via the imported pipeline from θ) and surfaces it to a Mesh; CLI prints output triangle count; viewport-debug-MCP `mesh_stats` confirms vertices > 0.
  - **Prereqs:** θ.
  - **Crates touched:** reify-kernel-openvdb (kernel_real.rs new FFI, register.rs).

- **Task κ** — Fidget `Convert { from: Sdf } → Mesh` capability descriptor + FFI integration (fidget's `mesh_render`); per-op `IsoMeshOptions` hashed.
  - **Observable signal:** A `.ri` file declares `field def sphere_sdf : Point3<Length> -> Length = ...` (SDF closed form); a downstream consumer demands `Mesh`; CLI evaluation produces a Mesh value; viewport-debug-MCP confirms vertices > 0.
  - **Prereqs:** δ, ε.
  - **Crates touched:** reify-kernel-fidget (kernel.rs, register.rs).

### Phase 6 — Cache-discipline integration (hex-wedge fold-in)

- **Task ξ** — Gmsh `Operation::MeshSurfaceToVolume` registered as `VolumeMeshOptions` options-hash producer; `engine_build.rs::dispatch_volume_mesh` threads `options_hash` into `RealizationCacheKey`.
  - **Observable signal:** Two `solve_elastic_static` invocations on identical geometry with `force_tet={true,false}` produce **measurably different** results (the tet path vs. the sweep path); instrumentation confirms two distinct VolumeMesh realizations in the graph. The hex-wedge M-024 regression that didn't exist (because the cache slot was shared) is now testable; M-024 lifts from PARTIAL → WIRED.
  - **Prereqs:** β, ε. **Plus** `compute-node-contract.md` η (FEA first real consumer) — this slice validates that ComputeNode's transitive cache-key composition through the realization layer is sound. Cross-PRD seam.
  - **Crates touched:** reify-eval (engine_build.rs), reify-kernel-gmsh (cache_key.rs, register.rs).

### Phase 7 — Pragma + project-pin consumers

- **Task ο** — `#kernel(...)` pragma propagated from `module.kernel_pragma` to `execute_realization_ops`; dispatcher gains `prefer_kernel: Option<&str>` parameter; pragma-mismatched-op emits `Diagnostic::KernelPragmaUnsatisfiable` and falls through.
  - **Observable signal:** `examples/multi_kernel/pragma_override.ri` with `#kernel(manifold)` performs a Boolean union; engine inspection confirms Manifold was chosen even though OCCT also supports the op via tessellate-chain. Without the pragma, lex-min selects (test pins both cases).
  - **Prereqs:** ζ (so both kernels can serve the same op).
  - **Crates touched:** reify-eval (engine_build.rs, dispatcher.rs), reify-compiler (module pragma propagation — currently emits `kernel_pragma`; just thread it through).

- **Task π** — `reify.toml` `[kernels]` pin consumer-side enforcement: at `Engine::with_registered_kernels`, compare pin set vs. registry; emit the four pin diagnostics from γ.
  - **Observable signal:** CLI `reify check` on a `reify.toml` with a typo (`occt = "0.1"` vs. registered VERSION `0.2`) emits `Diagnostic::KernelVersionMismatch` and exits non-zero. Lifts M-013 from PARTIAL → WIRED.
  - **Prereqs:** ε.
  - **Crates touched:** reify-eval (engine_admin.rs), reify-config (existing API surface stable).

### Phase 8 — Long-chain diagnostic wiring (GR-034 fold-in)

- **Task ρ** — `is_long_chain_realization` + `long_chain_diagnostic` called from `execute_realization_ops` for every multi-stage dispatch; wall-time tracked via `Instant::now()` brackets around the chain; threshold from `LONG_CHAIN_DEFAULT_THRESHOLD_MS` env-var-overridable.
  - **Observable signal:** A synthetic 3-stage chain fixture (BRep → Mesh → Voxel → Mesh, with one stage rigged to be slow) emits `Diagnostic::LongChainRealization` naming the chain; a 1-stage chain at the same wall time does NOT. Test pins both. Lifts M-017 from PARTIAL → WIRED.
  - **Prereqs:** ε, ι (for the multi-stage chain to be reachable in production).
  - **Crates touched:** reify-eval (engine_build.rs, dispatcher.rs — the builders already exist).
  - **Resolves:** GR-034 (cluster C-32).

### Phase 9 — Companion correction tasks

- **Task μ** — v0.2 multi-kernel PRD `ReprKind` count correction. The PRD says "four entries"; the as-built enum has five (`VolumeMesh` appended in v0.3 per M-001). Edit `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions" to acknowledge the additive extension; cross-link this PRD.
  - **Observable signal:** PRD updated; no code changes; doc lint passes.
  - **Prereqs:** None (independent doc edit).

- **Task ν** — `docs/prds/v0_2/imported-field-source.md` cross-reference to this PRD §4 (GR-003 wiring point) added; status moved from "deferred to v0.2" to "v0.3 fold-in via multi-kernel Phase 3".
  - **Observable signal:** PRD updated; cross-reference to this document; no code changes.
  - **Prereqs:** None.

- **Task τ** — `docs/architecture-audit/gap-register.md` GR-020 / GR-034 / GR-003 Notes updated with disposition pointers to this PRD.
  - **Observable signal:** Gap register updated; cross-links bidirectional.
  - **Status:** **performed in the 2026-05-12 PRD-authoring session** alongside the PRD save; verify at decompose time that the three GR entries point at this PRD and at §8 tasks θ / ρ; if so, mark this task `done` without a separate worktree.
  - **Prereqs:** None.

### Dependency view

```
α ─┐
β ─┼─→ δ ─→ ε ─┬─→ ζ ─→ ο
γ ─┘          │
              ├─→ η ─→ θ ─→ ι
              │
              ├─→ κ
              │
              └─→ ξ ←── (compute-node-contract.md η)
              
ι ─→ ρ
ε ─→ π

μ, ν, τ (independent doc edits)
```

ComputeNode contract task η must land before multi-kernel ξ (the hex-wedge `force_tet` cache discipline slice). All other tasks are ComputeNode-independent.

## §9 — Open questions (surfaced but not decided in this session)

1. **`with_registered_kernel` → `with_registered_kernels` rename — deprecation cycle length.** Contract specifies one minor cycle of alias-keep. **Suggested resolution:** keep through v0.3.x; remove in v0.4. Decide during task ε.

2. **OCCT `TessellateOptions` field set.** v0.2 OCCT shipped `tessellate(...)` with hard-coded deflection parameters; v0.3 needs `angular_deflection` + `linear_deflection` as user-visible options to drive the cache key correctly. **Suggested resolution:** start with two fields matching OCCT's BRepMesh primitive; extend if downstream consumers demand. Decide during task δ.

3. **`MarchingCubesOptions.adaptive` — boolean knob or feature-flag.** OpenVDB supports both uniform-grid and adaptive marching cubes. **Suggested resolution:** boolean knob (`adaptive: bool`); default to false (uniform) per the v0.2 PRD's "simplest correct" tilt. Decide during task ι.

4. **Pragma scope — module-scope only vs. expression-attribute form.** v0.2 parses `#kernel(...)` at module scope. Per-expression `@kernel(...)` would enable mixing kernels within a single module. **Suggested resolution:** module-scope only for v0.3; per-expression form deferred to v0.4 if demand surfaces. Decide during task ο.

5. **Project-pin `Pinned but unloaded` strictness.** Today the contract says warning for "registry name not pinned" (build has more than pin requires); error for "pin name not in registry" (pin demands more than build has). **Suggested resolution:** as specified; flip to error only if a user case shows the warning was missed. Decide during task π.

6. **Long-chain wall-time vs. CPU-time.** The v0.2 PRD specified `wall-time > 500 ms`; this contract preserves wall-time. CPU-time would be more deterministic across machines but less informative about user-experienced latency. **Suggested resolution:** wall-time; bump threshold in a follow-up if false positives accumulate. Decide during task ρ.

7. **`Operation::Convert` execute-side validation per kernel.** Each kernel's `execute` arm must handle every declared Convert edge. Audit M-009/M-010/M-011 found declared edges with no execute path. **Suggested resolution:** per-task acceptance check (each kernel-edge task validates `execute` returns a non-stub Value); add a workspace-level test that asserts every declared `(Convert, ReprKind)` in every capability descriptor has a corresponding `execute` arm that returns `Value::Geometry` (not `Value::Undef` and not a stub-message error). Decide during task δ (set the pattern; subsequent kernel tasks follow).

8. **Realization-graph backward-compat under multi-handle engine.** Changing `Engine.geometry_kernel` → `geometry_kernels` may affect serialised engine state if any persistent-cache layer captures kernel identity. **Suggested resolution:** verify `persistent_cache.rs` doesn't serialise `Engine.geometry_kernel`; if it does, version-bump the persistent format and migrate. Decide during task ε.

## §10 — Out of scope for this PRD

- **Mesh → BRep / Voxel → BRep / VolumeMesh → BRep** reconstruction (research-grade; no v0.3 demand).
- **HDF5 / CSV imported field source** — separate PRD `docs/prds/v0_3/imported-field-source-hdf5-csv.md` extends this contract once OpenVDB consumer arm (θ) lands.
- **Stack-pattern / patchwork-pattern assembly-level orchestration** — v0.4 concern. This PRD makes the pattern **possible**; it does not ship the abstraction.
- **GPU-offloaded kernels, dylib plugin loading, runtime kernel discovery** — v0.4+.
- **Manifold `propagate_attributes` MeshGL walk** — owned by `persistent-naming-v2.md` (GR-004).
- **`@optimized` ComputeNode dispatch** — separate surface per `compute-node-contract.md`; the two seams meet only at the cache-key boundary (§6).
- **Per-expression `@kernel(...)` pragma** — module-scope only for v0.3; per-expression deferred to v0.4.
- **Cost-aware kernel selection** (PRD v0.2 explicit non-goal — `cost_hint` / `error_factor` rejected without telemetry).

## §11 — Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_2/multi-kernel.md` | extends | Phases 1+2 shipped; this is Phase 3 | this-prd | wired (parent) |
| `docs/prds/v0_3/compute-node-contract.md` | adjacent | `RealizationCacheKey.options_hash` ⟶ `ComputeNodeData.options_hash` transitivity (§4, §6) | shared boundary; this-prd owns realization side, compute-node-contract owns ComputeNode side | wired |
| `docs/prds/v0_2/imported-field-source.md` | consumes | OpenVDB `CompiledFieldSource::Imported` arm at `engine_eval.rs:621` | this-prd (Phase 4 task θ) | queued |
| `docs/prds/v0_3/imported-field-source-hdf5-csv.md` | produces | Extension of θ for HDF5/CSV after OpenVDB lands | other-prd | blocked-on-θ |
| `docs/prds/v0_3/structural-analysis-shells.md` | consumes | BRep→Voxel chain for mid-surface extraction (M-025) | shells-prd (consumes); this-prd ships the chain | queued |
| `docs/prds/v0_3/hex-wedge-meshing.md` | consumes | `force_tet`-in-cache-key discipline (M-024) | this-prd (Phase 6 task ξ) | queued |
| `docs/prds/v0_3/mesh-morphing.md` | adjacent | Mesh-output ops produce realizations mesh-morph consumes | mesh-morphing-prd (consumes); this-prd ships the producer side | wired |
| `docs/prds/v0_2/persistent-naming-v2.md` | adjacent | `KernelAttributeHook::propagate_attributes` Manifold body (GR-004) | pnv2-prd | blocked-on-pnv2 (separate gate; this PRD is parallel to the MeshGL walk) |
| `docs/prds/v0_2/per-purpose-tolerance.md` | adjacent | `per_stage_tolerance_for_plan` and long-chain threshold (§8 task ρ) | per-purpose-tolerance-prd (owns vocabulary); this-prd owns dispatcher wiring | wired |

No reciprocal "the other owns it" cycles after the 2026-05-12 dispositions (GR-003 → multi-kernel, GR-004 → PNv2).
