# Shell-Extract Engine Bridge

Status: contract + decomposition plan (resolves cluster C-19 / GR-021 per `docs/architecture-audit/gap-register.md`). Authored 2026-05-12 in interactive session. Awaits Leo approval before queueing tasks.

## §0 — Purpose and supersession

This document is the **contract** for transporting `reify-shell-extract`'s producer-half output (mid-surface mesh, segmentation, per-vertex thickness, mid-surface naming attributes) into its two consumer-side surfaces: (1) `reify-solver-elastic`'s persistent-cache `ElasticResult`, and (2) the GUI viewport via the existing `mesh-update` event channel. Today the producer-half ships in isolation: `reify-shell-extract` is depended on by no other crate (`grep reify-shell-extract crates/*/Cargo.toml` returns only the crate's own manifest); its output never reaches FEA or IPC.

This PRD supersedes the **engine-integration half** of the parent v0.4 shells PRD's decomposition tasks T18 / T19 / T20 (tasks 3031, 3032, 3033). The integration design accreted in `docs/prds/v0_4/structural-analysis-shells.md` §"Decomposition plan" was authored before GR-001 (struct-instance runtime), GR-002 (ComputeNode contract), and GR-016 (GUI event channel inventory) landed; this PRD's vertical slices replace those tasks under the post-2026-05-12 seam ownership.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (memory `preferences_implementation_chain_naming`) — is what this contract is designed to prevent for the shell-extract seam specifically. Resolution mode is approach **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline. Per memory `feedback_orchestrator_narrow_locks_favor_upfront_design`, the cache-payload + GUI-payload shape decisions are taken up-front in §3 / §4 to avoid mid-implementation re-lock churn.

## §1 — GR-021 + cross-PRD chain

Gap-register row recorded in `docs/architecture-audit/gap-register.md` § GR-021 (line 314):

> Mid-surface mesh + segmentation + per-vertex thickness all produced in `reify-shell-extract`; never transported through IPC to GUI; never bridged to `reify-solver-elastic`'s persistent-cache ElasticResult. FICTION / F1. Disposition: **PRD-shape work — shell-extract engine integration PRD.**

Cross-PRD relationships (all landed; this PRD plugs into committed seams):

- **`docs/prds/v0_4/structural-analysis-shells.md`** (parent) — owns kernel-side shell solver, MITC3 element formulation, `ShellStress.top/mid/bottom` schema, mid-surface extractor crate, MPC plumbing, segmentation API. T18 / T19 / T20 here.
- **`docs/prds/v0_3/compute-node-contract.md`** (GR-002, landed) — owns the ComputeNode dispatch seam. Shell extraction routes through ComputeNode per §6's per-feature table ("Mid-surface / shell extraction (T18, shells PRD future): Yes — realization output, geometry-expensive, cacheable on geom-hash"). The trampoline contract from §4 is the registration shape; this PRD's §5 instantiates it.
- **`docs/prds/v0_3/structure-instance-runtime.md`** (GR-001, landed) — owns runtime evaluation of `Value::StructureInstance`. `ElasticOptions { shell_force, shell_threshold, … }`, `FixedSupport`, `PinnedSupport`, `Steel_AISI_1045()` become reachable through that PRD's foundation slice. This PRD reads them as the trampoline's `options: &Value` argument once SIR-α lands.
- **`docs/prds/v0_3/gui-event-channel-inventory.md`** (GR-016, landed) — §2.4 explicitly delegates **MeshData payload extensions** (`element_kind`, `region_tags`, `vector_channels`, `vonMises_top|mid|bottom` scalar keys, per-vertex `thickness`) to this PRD. No new event channel is introduced; the existing `mesh-update` channel transports the extended `MeshData`. GR-016 §3 lockstep-commit convention applies to every MeshData shape change in §4 below.
- **`docs/prds/v0_3/persistent-fea-cache.md`** (PARTIAL — task 2974 still open at this PRD's authoring time) — owns persistent-cache integration plumbing. This PRD bumps `ElasticResultHeader::FORMAT_VERSION` and extends the on-disk layout per §6.
- **`docs/prds/v0_3/engine-integration-norm.md`** (GR-017, **session-prompt-only at this PRD's authoring**) — when authored, this PRD becomes one of its worked examples. Until then, this PRD treats shell extraction as an internal-engine realization-dispatch consumer of GR-002 (i.e., a ComputeNode target invoked from realization dispatch, parallel to the mesh-morph case in compute-node-contract §6).
- **`docs/prds/v0_4/fea-gui-rendering-shells.md`** (sibling, deferred) — consumes the MeshData extensions this PRD adds. Pure-frontend rendering (display-mode toggle, top/mid/bottom stress-channel selector, shell-normal arrow overlay) stays there per GR-016 §2.5. This PRD ships the IPC payload it consumes.
- **`docs/prds/v0_5/varying-thickness-shells.md`** + **`docs/prds/v0_5/composite-laminated-shells.md`** + **`docs/prds/v0_5/structural-stability-buckling.md`** (deferred) — all consume `shell_channels: Option<ShellChannels>` and the segmentation/thickness pipeline this PRD wires. Varying-thickness's stated win ("never collapses per-vertex thickness to a scalar") is honoured here by `ShellExtractionResult` carrying the per-vertex `Vec<f64>` end-to-end.

## §2 — `ShellExtractionResult` type (the bundled producer-half output)

The shell-extract ComputeNode's `Completed.result` is a single `Value::StructureInstance` carrying every output the consumers need. Rust-side bundled struct held by the trampoline before lowering to `Value`:

```rust
pub struct ShellExtractionResult {
    /// Mid-surface triangle mesh in world coordinates. Vertices flattened
    /// XYZ; triangles as [i; 3] index triples. Length invariant:
    /// vertices.len() == 3 * (thickness.len()).
    pub mid_surface: MidSurfaceMesh,        // crates/reify-shell-extract/src/mid_surface.rs
    /// Segmentation labels per mid-surface triangle. region_classification[i]
    /// names the kind of region triangle i belongs to.
    pub segmentation: SegmentationResult,   // crates/reify-shell-extract/src/segmentation.rs
    /// Per-region naming records, ready to be folded into the
    /// TopologyAttributeTable (κ in §8 below).
    pub naming: MidSurfaceAttributes,       // crates/reify-shell-extract/src/mid_surface_naming.rs
    /// Cost metric for cache eviction (per compute-node-contract §4 / §5).
    pub solve_time_ms: u64,
    /// Diagnostics emitted during extraction (`MidSurfaceError`,
    /// `SegmentationError`, etc., demoted to `Diagnostic` shape). Hard-error
    /// extraction failures return `ComputeOutcome::Failed { diagnostics }`
    /// rather than populating this field.
    pub diagnostics: Vec<Diagnostic>,
}
```

`MidSurfaceMesh.thickness: Vec<f64>` is preserved end-to-end (per memory `preferences_implementation_chain_naming`'s "never collapse what the producer produces"; varying-thickness PRD M-002 already pins this on the producer side).

**Why a single bundled type rather than 3+ ComputeNodes (one per stage):** the four producers (medial mask → mid-surface → pruning → segmentation → naming) are an ordered pipeline whose intermediate states are not consumed independently. Bundling collapses the cache-key proliferation that per-stage caching would force (each stage would need an options hash + an input hash), and matches the parent shells PRD's T18 framing ("extraction cached as a ComputeNode keyed on geometry hash + extraction options").

**Persistent-cache implementation.** `ShellExtractionResult` implements `PersistentlyCacheable` (per persistent-fea-cache PRD): `serialize_to_writer` / `deserialize_from_reader` round-trip every f64 / f32 bit-exact via the bincode+zstd path; `FORMAT_VERSION` starts at 1; `uncompressed_byte_size` sums vertex/triangle/thickness/segmentation/naming buffer sizes; `solve_time_ms` flows from the field above.

## §3 — `ElasticResult` shell extension (the engine→solver bridge)

Confirmed by Leo: extend `ElasticResult` in `crates/reify-eval/src/persistent_cache.rs:450` in place.

```rust
pub struct ElasticResult {
    pub displacement: Vec<f64>,
    pub stress: Vec<f64>,                       // existing — aliases result.stress.mid
    pub shell_channels: Option<ShellChannels>,  // NEW — None for tet-only solves
    pub max_von_mises: f64,
    pub converged: bool,
    pub iterations: u32,
    pub solve_time_ms: u64,
}

pub struct ShellChannels {
    /// Per-element top-surface stress, flattened as the existing `stress`
    /// field is flattened. Indices align with `stress` (i.e. mid).
    pub top: Vec<f64>,
    /// Per-element bottom-surface stress. Same layout as `top`.
    pub bottom: Vec<f64>,
    /// Per-element local-to-global rotation, 9 * n_elements (row-major 3x3).
    /// Matches `shell_element_frame()` convention at `shell_result.rs:41`.
    pub frame: Vec<f64>,
}
```

**Backward-compat alias.** `result.stress` continues to alias `result.stress.mid` at the stdlib structure-def layer (per parent shells PRD M-016 promise). Concretely, the stdlib `solver_elastic.ri:325-328` `ShellStress.homogeneous(field).mid` round-trip remains the stable surface; this PRD does not introduce a new alias variant. Existing tet-only consumers (`result.stress`, `result.max_von_mises`) are unaffected — `shell_channels` is `None` and ignored.

**`FORMAT_VERSION` bump.** `ElasticResultHeader::FORMAT_VERSION` (`persistent_cache.rs:380`) increments by 1. The new header carries `shell_channels_present: bool` + (when present) `top_len: u64`, `bottom_len: u64`, `frame_len: u64`. Existing cache entries deserialize cleanly under the previous version (the format version field gates dispatch).

**Why not a sibling `ShellElasticResult`:** (a) the v0.4 shells PRD's `result.stress = result.stress.mid` backward-compat alias commits to one ElasticResult shape; (b) consumers that operate over a `Map<String, ElasticResult>` (multi-load-case-fea) would otherwise need a kind-dispatch wrapper; (c) GR-001 makes the `Value::StructureInstance` surface uniform either way — the Rust type's shape doesn't leak to user code.

## §4 — `MeshData` shell extension (the engine→GUI bridge)

Per GR-016 §2.4: these extensions land in lockstep Rust+TS commits on the existing `mesh-update` channel. No new event channel.

Rust-side at `gui/src-tauri/src/types.rs:177`:

```rust
pub struct MeshData {
    pub entity_path: String,
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    pub normals: Option<Vec<f32>>,
    pub scalar_channels: HashMap<String, Vec<f32>>,
    pub displaced_positions: Option<Vec<f32>>,
    // NEW (this PRD):
    /// Per-face element kind for mixed shell/tet bodies. 0 = tet face,
    /// 1 = shell triangle. Omitted from the wire when None (every face is
    /// the same kind — the dominant single-kind case). Length contract:
    /// element_kind.len() == indices.len() / 3 when Some.
    pub element_kind: Option<Vec<u8>>,
    /// Per-face region label from `segmentation::SegmentationResult`.
    /// Stable across re-evaluations of the same body. Omitted when None.
    /// Length contract: region_tags.len() == indices.len() / 3 when Some.
    pub region_tags: Option<Vec<u32>>,
    /// Per-vertex (or per-face — channel-defined) 3-component vector
    /// channels. Layout flat XYZ-XYZ-… in the entity's frame. Used for
    /// shell-normal arrow overlays, rigid-body-mode arrows, etc.
    /// Length contract: each entry's len() ∈ { 3 * vertex_count,
    /// 3 * face_count } enforced at serialization time.
    pub vector_channels: HashMap<String, Vec<f32>>,
}
```

TypeScript counterpart in `gui/src/types.ts` matches field-for-field per GR-016 §3.2. Contract violations (length mismatches) raise `S::Error::custom` at serialization time (existing pattern in `MeshData::serialize`).

**`scalar_channels` key additions.** Shell solves populate three additional keys when `shell_channels.is_some()`:
- `"vonMises_top"` — per-vertex top-surface von Mises stress.
- `"vonMises_mid"` — per-vertex mid-surface von Mises stress (equals existing `"vonMises"` key for shell solves to preserve the tet-default channel; for tet solves the new keys are absent).
- `"vonMises_bottom"` — per-vertex bottom-surface von Mises stress.

The keys are additive; existing TS code reading `scalar_channels["vonMises"]` continues to work unchanged.

**Per-vertex `thickness` channel.** Out of scope for this PRD. Owned by the v0.5 `varying-thickness-shells.md` PRD per GR-016 §2.4. The thickness data IS produced by `MidSurfaceMesh.thickness` and reaches `ShellExtractionResult`; populating it into `scalar_channels["thickness"]` is the v0.5 task. This PRD does not block that addition — it's a future `scalar_channels` key under the same field.

## §5 — ComputeNode integration

Per Leo's decision: shell extraction registers as a separate ComputeNode target.

**Target name.** `"shell-extract::extract"`. Convention matches compute-node-contract §6's per-feature table.

**Trampoline signature** per compute-node-contract §4:

```rust
fn shell_extract_compute_fn(
    value_inputs: &[Value],          // [options: Value::StructureInstance(ElasticOptions)]
    realization_inputs: &[RealizationReadHandle],  // [body_geom: BRep or Mesh]
    options: &Value,                 // unused on this target (options is in value_inputs[0])
    prior_warm_state: Option<&OpaqueState>,  // unused (extraction is one-shot, no warm state)
    cancellation: &CancellationHandle,
) -> ComputeOutcome
```

The trampoline:

1. Polls `cancellation.is_cancelled()` between phases per compute-node-contract §2 (poll discipline: between medial-mask, mid-surface, pruning, segmentation, naming).
2. Calls `reify_shell_extract::compute_medial_mask` → `extract_mid_surface` → `prune_branches` → `mesh_mid_surface` → `segment_regions` → `populate_mid_surface_attributes` in order, passing each previous output as input.
3. On any `*Error` enum variant: returns `ComputeOutcome::Failed { diagnostics }` with the error mapped to a `Diagnostic` shape per §7 below. The per-region failure-mapping table in §7 is the contract.
4. On success: returns `ComputeOutcome::Completed { result: shell_extraction_result.to_value(), new_warm_state: None, cost_per_byte: derived, diagnostics: [] }`.

**Registration.** `reify-eval/src/engine_compute.rs::register_compute_fns` (added by compute-node-contract §8 task γ) gains a `register_shell_extract_compute_fns(engine)` call. Alphabetic-order constraint per compute-node-contract §4 is satisfied ("shell-extract" sorts after "solver" — verify on registration; if not, the registration site reorders).

**Cache key composition.** Per compute-node-contract §4: the key is the standard `(target, value_inputs_hash, realization_inputs_hash, options_value_hash)`. The `realization_inputs_hash` covers the body geometry; the `value_inputs_hash` covers `ElasticOptions { shell_threshold, shell_voxel_size, shell_branch_prune_ratio, shell_force, … }`. This satisfies the parent shells PRD T18 contract ("extraction cached as a ComputeNode keyed on geometry hash + extraction options").

**Why no warm state.** Mid-surface extraction is one-shot per `(geometry, options)` pair. No incremental refinement, no Lanczos restart, no per-iteration state. `prior_warm_state` is always `None`; `new_warm_state` is always `None`. The cache holds the `result` directly.

**Cost-per-byte derivation.** `cost_per_byte = (solve_time_ms / uncompressed_byte_size).max(epsilon)`. Persisted via `ComputeOutcome::Completed.cost_per_byte`; consumed by the warm-state-eviction comparator (currently inactive per cluster C-42 / GR-040 — once that flips, this PRD's cost numbers feed in automatically).

## §6 — Persistent-cache integration

The shell-extract ComputeNode itself participates in the persistent-cache through `ShellExtractionResult: PersistentlyCacheable` (per §2). No new infrastructure: the existing `reify-eval/src/persistent_cache.rs::PersistentlyCacheable` trait + `ELASTIC_RESULT_FORMAT_VERSION` precedent applies verbatim.

`ElasticResult`'s extension (§3) requires a coordinated cache action:

1. `ElasticResultHeader` gains the four new fields (one bool, three u64).
2. `ElasticResult::serialize_to_writer` writes the new fields after the existing tail; `deserialize_from_reader` reads conditional on `shell_channels_present`.
3. `ELASTIC_RESULT_FORMAT_VERSION` (`persistent_cache.rs:359` or sibling const) bumps by 1.
4. The on-disk-layout-format-version round-trip test gains a v(N-1) → v(N) migration assertion: an existing-format entry deserializes to `ElasticResult { shell_channels: None, … }` without error, and a fresh-format entry round-trips bit-exactly.

This intersects persistent-fea-cache PRD task 2974 (M-011 in its findings) — that task's open scope was "wire ComputeNode → persistent-cache lookup/write integration." The compute-node-contract §8 task ι replaces 2974's open work; this PRD's cache extension is purely the payload-shape bump on top of ι.

## §7 — Extraction failure → diagnostic mapping (T19 superseder)

`ShellExtractionResult` is produced only on success. Failures are mapped to `Diagnostic` shape and returned via `ComputeOutcome::Failed { diagnostics }`. The trampoline's failure-handling policy follows the parent shells PRD §"Failure semantics":

| `@shell`-annotated body | Auto-classified body | Engine behaviour |
|---|---|---|
| YES | — | Hard error; surface diagnostic to user; do not fall back. |
| — | YES | Soft fallback: emit warning diagnostic; reroute to tet-only meshing; continue. |

The reroute on auto-classification fallback is handled by the dispatch site, not the ComputeNode itself: the ComputeNode reports `Failed`; the caller (the FEA dispatch path, §8 task δ) inspects the `@shell`/`@solid`/auto tag and decides.

**Diagnostic mapping table** (deriving from `MidSurfaceError`, `SegmentationError`, `PruneError`, `MesherError`):

| Producer error | Diagnostic code | User message (template) |
|---|---|---|
| `MidSurfaceError::GridValidation(EmptyAxisGrid)` | `E_SHELL_NO_VOXEL_GRID` | "Shell extraction requires a non-empty voxel grid; body `{entity}` had no realizable Regular3D field." |
| `MidSurfaceError::MaskVoxelOutOfBounds` | `E_SHELL_MEDIAL_MASK_OOB` | "Medial mask produced a voxel outside the grid extents; usually means SDF and grid sampling disagree." |
| `SegmentationError::InvalidThreshold` | `E_SHELL_BAD_THRESHOLD` | "shell_threshold = `{value}` must be in `(0.0, 1.0)`." |
| `PruneError::*` (branch-prune failures) | `E_SHELL_PRUNE_FAILED` | "Spurious-branch pruning could not converge in `{max_iter}` iterations; the medial graph may be malformed." |
| `MesherError::QualityBelowThreshold` | `E_SHELL_MESH_QUALITY` | "Mid-surface mesh quality below threshold; remediation (MMG2D-style remesh) is not yet available." |
| Thickness/extent ratio > `shell_threshold` everywhere | `E_SHELL_TOO_THICK` | "Body `{entity}` is too thick for shell treatment (ratio `{r}` > `{threshold}`); use `@solid` to force tet meshing." |
| No medial mask at all | `E_SHELL_NO_MEDIAL` | "Body `{entity}` has no detectable medial surface; geometry is fully solid or the voxel resolution is too coarse." |

The seven-diagnostic vocabulary above is the user-observable contract. Codes are PascalCase-equivalent (per cluster C-09 / GR-012 disposition `accept-and-document`) — actual variant names land as `Diagnostic::ShellNoVoxelGrid` etc. in `reify-types::Diagnostic`.

## §8 — Boundary-test sketch (cross-crate; facing both ways)

Per gates §G5, the H-component requires test scenarios facing both producer-side (kernel-providing crates) and consumer-side (engine/solver/GUI). Tests live in `crates/reify-eval/tests/`, `crates/reify-solver-elastic/tests/`, and `gui/src/__tests__/`.

### §8.1 Producer-side (reify-shell-extract looks outward at the engine)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **ShellExtractionResult round-trip via cache.** Build a `ShellExtractionResult` from synthetic inputs; serialize to bytes; deserialize. | `PersistentlyCacheable` impl landed. | Round-trip preserves every f64 / f32 bit-exact; `uncompressed_byte_size` matches the actual deserialized buffer total. |
| **Trampoline registration.** `Engine::register_compute_fn("shell-extract::extract", shell_extract_compute_fn)` succeeds. Inspect registry. | Engine constructed; `register_shell_extract_compute_fns` called. | `compute_dispatch("shell-extract::extract")` returns `Some(_)`; double-registration panics with named-registrant message (per compute-node-contract §4 contract). |
| **Synthetic-geometry extraction.** Insert a ComputeNode with target `shell-extract::extract` on a synthetic slab (SDF distance ≈ 1mm both sides, thickness 0.1mm). | Trampoline registered; synthetic `SampledField` realization input. | `ComputeOutcome::Completed` with `result` deserialized as a `ShellExtractionResult` whose `mid_surface.thickness` agrees with `2 × |φ|` per `MidSurfaceMesh` test contract. |
| **Cancellation during extraction.** Long-running synthetic input; cancel mid-segmentation. | Trampoline polls per 100 ms SLA (extraction phase boundaries). | Returns `ComputeOutcome::Cancelled` within 2× poll budget (200 ms). No partial `ShellExtractionResult` leaked; cache untouched. |
| **Cache-hit short-circuit.** Run the same `(geometry, options)` pair twice. | Persistent cache mounted. | Second run reads from cache; no trampoline invocation (verified via dispatch-count instrumentation, per compute-node-contract §7.2 pattern). |
| **Failure → diagnostic.** Inject `SegmentationError::InvalidThreshold { value: 1.5 }`. | Synthetic input that triggers the threshold check. | `ComputeOutcome::Failed` returns; diagnostic carries `E_SHELL_BAD_THRESHOLD` code; user-message template instantiated with `value=1.5`. |

### §8.2 Consumer-side (solver-elastic + GUI look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **ElasticResult round-trip with shell_channels.** Persist an `ElasticResult { shell_channels: Some(_), … }`; reopen engine; deserialize. | `ELASTIC_RESULT_FORMAT_VERSION` bumped. | Round-trip preserves every f64 bit-exact; `shell_channels.top.len()` etc. match pre-persist. |
| **ElasticResult round-trip across format version.** Persist under v(N-1); deserialize under v(N). | Backward-compat read path landed. | `ElasticResult { shell_channels: None, displacement, stress, … }` returns; no `shell_channels` populated; no error. |
| **Stdlib alias preserved.** `result.stress = result.stress.mid` invariant holds for a shell solve. | Shell solve invoked; GR-001 SIR-α landed (so `result` is a `Value::StructureInstance`). | `(result.stress).mid == result.stress` evaluates true for a shell result; both equal the existing `stress` field bytes. For tet results the same alias holds (mid = stress always). |
| **Cantilever-flexure shell solve.** A `.ri` file declares a steel flexure (50mm × 10mm × 1mm), `FixedSupport` on one end, tip load 10N on the other; `solve_elastic_static(...)`. | GR-001 SIR-α landed; compute-node-contract §8 task η landed (`solve_elastic_static` ComputeNode trampoline); this PRD's §8 task δ landed (auto-classification dispatch). | `result.stress.top.von_mises` at the fixed-support row matches analytical bending stress `σ = 6 PL / (b h²)` within 5% on the v0.4 widened bands (per parent shells PRD §"Updates" — bare-MITC3 v0.4 contract; tightening gates on task 3392). |
| **GUI shell MeshData transport.** Open the cantilever fixture in dev-mode GUI; viewport receives `mesh-update` event for the shell body. | This PRD's §8 task θ landed. | `MeshData.element_kind == Some(vec![1; n_faces])` for the all-shell body; `region_tags` populated; `scalar_channels["vonMises_top"]` / `"vonMises_mid"` / `"vonMises_bottom"` arrays of length `vertex_count`. Length contracts enforced per `gui/src-tauri/src/types.rs::MeshData::serialize`. |
| **Mixed shell/tet body.** Open a flexure-on-block fixture (parent shells PRD's canonical T22 test); viewport receives a single `mesh-update` event for the body. | Same. | `MeshData.element_kind` has both 0 and 1 entries; `region_tags` distinguishes regions; segmentation labels round-trip the per-region IDs assigned by `segment_regions`. |
| **Extraction failure → user-visible diagnostic.** A `@shell`-annotated body too thick to extract. | This PRD's §8 task ε landed. | Engine emits a `Diagnostic` with code `E_SHELL_TOO_THICK`; CLI evaluation surfaces it; GUI diagnostics panel shows the message; the solve does NOT fall back (hard error policy per §7). |
| **Persistent-naming fold-in.** `body.mid_surface().face("region_0")` selector resolves. | This PRD's §8 task κ landed; parent persistent-naming-v2 selector vocabulary registered. | Selector evaluates to a stable handle; round-trips across engine restarts (persistent-cache rehydrates the TopologyAttributeTable fold). |

## §9 — Decomposition DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its **user-observable signal** per memory `feedback_task_chain_user_observable`.

### Phase 1 — Type + cache contracts

- **Task α** — `ShellExtractionResult` Rust type + `PersistentlyCacheable` impl + bundled serialization.
  - **Observable signal:** Unit tests in `crates/reify-shell-extract/src/lib.rs::tests` pin: a synthetic `ShellExtractionResult` round-trips through `serialize_to_writer` / `deserialize_from_reader` bit-exact (all f64/f32 buffers); `uncompressed_byte_size` matches the actual sum.
  - **Prereqs:** None.
  - **Crates touched:** `reify-shell-extract` (new `result.rs` module + `lib.rs` re-export), `reify-types` (already exports `Diagnostic`).

- **Task β** — `ElasticResult.shell_channels: Option<ShellChannels>` extension + `ELASTIC_RESULT_FORMAT_VERSION` bump.
  - **Observable signal:** Unit tests in `reify-eval/src/persistent_cache.rs::tests` pin: (a) round-trip with `shell_channels: Some(_)` is bit-exact; (b) round-trip with `shell_channels: None` matches the existing tet-only round-trip; (c) v(N-1) → v(N) backward-compat read produces `shell_channels: None`. Format-version bump assertion in the existing pinning test.
  - **Prereqs:** None.
  - **Crates touched:** `reify-eval` (`persistent_cache.rs`).

### Phase 2 — ComputeNode trampoline slice (no FEA yet)

- **Task γ** — `shell-extract::extract` ComputeNode target + trampoline + registration.
  - **Observable signal:** Engine integration test in `crates/reify-eval/tests/` pins: registering the trampoline, inserting a ComputeNode with the target on a synthetic slab geometry, observing a `ShellExtractionResult` materialize in the output ValueCell; second run hits the in-memory ComputeNode cache (dispatch-count instrumentation per compute-node-contract §7.2). Failure-mapping test injects an invalid threshold and asserts `ComputeOutcome::Failed` with `E_SHELL_BAD_THRESHOLD`.
  - **Prereqs:** α; compute-node-contract §8 task γ (per-Engine dispatch registry — already filed as new task in §8 DAG).
  - **Crates touched:** `reify-eval` (`engine_compute.rs` registration call; new file under `crates/reify-eval/src/shell_extract_compute.rs`), `reify-shell-extract` (Cargo.toml gains no new deps; the trampoline lives engine-side and consumes the producer-side surface), `reify-types` (Diagnostic variants if not already present).

### Phase 3 — Auto-classification dispatch (T18 superseder)

- **Task δ** — Auto-classification dispatch wired into the FEA solve path: when a body is shell-classified (auto OR `@shell`), the FEA trampoline depends on a `shell-extract::extract` ComputeNode upstream; the trampoline reads the extracted mid-surface mesh + segmentation and routes assembly through the shell kernel.
  - **Observable signal:** Engine integration test (`crates/reify-eval/tests/shell_solve_e2e.rs`) pins: a steel-flexure `.ri` fixture (50mm × 10mm × 1mm cantilever) with `FixedSupport` and tip load 10N evaluates via `solve_elastic_static`; the returned `ElasticResult.shell_channels.top` per-element von Mises at the root row matches analytical `6PL/(bh²)` within 5%. Inspection confirms a `shell-extract::extract` ComputeNode and a `solver::elastic_static` ComputeNode in the graph with the former feeding the latter.
  - **Prereqs:** β, γ; compute-node-contract §8 task η (FEA `solve_elastic_static` trampoline); structure-instance-runtime §"Wave 1" foundation slice (SIR-α — so `FixedSupport`, `Steel_AISI_1045()` are runtime-evaluable); parent shells PRD T9 mesher (task 3019, landed), T11 mixed-element global assembly (task 3021, landed), T16 ElasticResult extension (task 3028, partial — landed at structure-def layer, this task closes the Rust-side bridge).
  - **Crates touched:** `reify-eval` (`engine_build.rs` realization dispatch site; the FEA trampoline gains a shell-extract upstream), `reify-solver-elastic` (shell-FEA glue producing `ShellChannels` from the per-element `ShellElementStress` data already computed at `shell_result.rs:78`).
  - **Supersedes:** Task **3031** (T18 auto-classification dispatch, currently pending).

### Phase 4 — Extraction failure handling (T19 superseder)

- **Task ε** — Extraction failure mapping + dispatch-site policy. The seven failure modes in §7 are wired through `ComputeOutcome::Failed` and surfaced to the user; `@shell` is hard error, `auto` falls back to tet meshing with a warning diagnostic.
  - **Observable signal:** Two new fixtures in `crates/reify-eval/tests/`: (a) `shell_too_thick_at_shell_annotation_errors.rs` — a thick body with `@shell` produces a `Diagnostic::ShellTooThick` and no fallback; the solve errors. (b) `shell_too_thick_at_auto_falls_back.rs` — same body without `@shell` produces a warning + falls back to tet meshing; the solve succeeds with `shell_channels: None`. CLI evaluation reproduces both.
  - **Prereqs:** γ, δ.
  - **Crates touched:** `reify-eval` (FEA dispatch-site policy; diagnostic emission), `reify-types` (Diagnostic variants per §7 table if not already wired).
  - **Supersedes:** Task **3032** (T19 extraction failure handling, currently pending).

### Phase 5 — Persistent-naming fold-in (T20 supersession)

- **Task ζ** — `MidSurfaceAttributes` folded into `TopologyAttributeTable` post-extraction; `body.mid_surface().face("region_0")` selector resolves to a stable handle.
  - **Observable signal:** Selector integration test in `crates/reify-eval/tests/`: a `.ri` file `body.mid_surface().face("region_0")` evaluates to a non-`Value::Undef` value; the value round-trips across an engine restart (persistent-cache rehydrates the fold). Existing `populate_mid_surface_attributes` test in `mid_surface_naming.rs` continues to pass.
  - **Prereqs:** γ; persistent-naming-v2 selector vocabulary v2 (cluster C-10 / GR-013 dispositions — registered names in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` per task 2699 work). If the selector vocabulary isn't fully wired at task-fire time, this task's leaf signal degrades to "table contains the records and `mid_surface()` resolves to the mid-surface entity; the further `.face("region_0")` lookup is gated on the upstream selector arm landing."
  - **Crates touched:** `reify-eval` (table-fold call site at the shell-extract ComputeNode's dispatch-complete hook).
  - **Supersedes / completes:** Task **3033** (T20 persistent-naming for derived mid-surface entities). Task 3033 was merged with the producer-side `populate_mid_surface_attributes` only; this task closes the engine-side fold-in.

### Phase 6 — GUI MeshData payload extension

- **Task η** — `MeshData` Rust+TS lockstep extension: `element_kind`, `region_tags`, `vector_channels` per §4.
  - **Observable signal:** Per GR-016 §6: (a) Rust-side roundtrip test in `gui/src-tauri/src/types.rs::tests` pins the new fields serialize to a frozen JSON-snapshot shape and length contracts trigger `S::Error::custom` on mismatch; (b) TS-side shape test in `gui/src/__tests__/bridge/mesh_update.test.ts` constructs a MeshData with the new fields, passes through the `mockTauriEvent` utility, asserts the consumer panel receives correctly-typed data; (c) `npm run typecheck` passes with the new TS interface entries.
  - **Prereqs:** None (Rust+TS lockstep edit; no upstream blockers).
  - **Crates touched:** `gui/src-tauri/src/types.rs`, `gui/src/types.ts`, `gui/src-tauri/src/diff.rs` (the existing `delta_to_events` emit site preserves backward-compat when the new fields are None — automatic via `#[serde(skip_serializing_if = "Option::is_none")]`).

- **Task θ** — Engine-side populator: when an extraction ComputeNode has a fresh `ShellExtractionResult` and an FEA ComputeNode has a fresh `ElasticResult { shell_channels: Some(_) }`, the engine populates the new `MeshData` fields on the body's `mesh-update` event.
  - **Observable signal:** GUI integration test in `gui/test/visual/`: open the cantilever-flexure shell fixture in dev-mode GUI; viewport receives a `mesh-update` event whose `MeshData.element_kind` is `Some(vec![1; n_faces])` and whose `scalar_channels["vonMises_top"]` length matches `vertex_count`. Debug-MCP `mesh_stats` returns `element_kind_count: { 1: <n> }`. (The pure-frontend display-mode toggle and per-channel selector are NOT in scope — they live in fea-gui-rendering-shells.)
  - **Prereqs:** δ, η. Plus the existing `mesh-update` channel infrastructure (per GR-016 §2.1 — already wired).
  - **Crates touched:** `gui/src-tauri/src/engine.rs` (the existing MeshData construction site at `engine.rs:944`; ingests from the engine-side FEA + extraction ComputeNodes), `reify-eval` (exposes the shell-extract / FEA results to the GUI engine adapter — likely a new accessor on `Engine`).

### Phase 7 — User-observable end-to-end leaf

- **Task ι** — End-to-end thin-walled-bracket example.
  - **Observable signal:** `examples/shells/thin_walled_bracket.ri` evaluates from a clean checkout via `reify run examples/shells/thin_walled_bracket.ri`: prints `max_von_mises` matching the analytical envelope for a thin bracket under tip load; the same file opens in `scripts/run-gui-dev.sh` and renders the mid-surface mesh with `vonMises_top` colormap. Viewport screenshot baseline added to `gui/test/visual/baselines/`.
  - **Prereqs:** δ, ε, ζ, θ.
  - **Crates touched:** `examples/shells/` (new), `gui/test/visual/` (baseline).
  - **Supersedes:** Task **3036** (T23 end-to-end thin-walled-bracket example, currently pending). Notes: parent shells PRD task T23's `param thickness : Length = auto` (M-026 cross-PRD breadcrumb) is auto-resolution scope — this PRD's leaf uses a concrete numeric thickness; the auto-resolution variant is a follow-up against the auto-resolution-backtracking PRD.

### Phase 8 — Companion correction tasks

- **Task κ** — Update parent shells PRD (`docs/prds/v0_4/structural-analysis-shells.md`) §"Decomposition plan" to cross-reference this PRD: T18 / T19 / T20 / T23 supersession recorded in the parent PRD's status/updates block; dispositions of tasks 3031, 3032, 3036 marked as `cancelled` (with `reopen_reason` pointing here); task 3033's done status stands, with the engine-side fold-in re-filed as this PRD's ζ.
  - **Observable signal:** `git diff docs/prds/v0_4/structural-analysis-shells.md` shows the supersession note; cross-link to this PRD's path.
  - **Prereqs:** Leo's approval of this PRD's DAG (so the supersession claim is real).
  - **Crates touched:** docs only.

- **Task λ** — Update gap-register GR-021 Notes row with a cross-link to this PRD.
  - **Observable signal:** `git diff docs/architecture-audit/gap-register.md` shows the row's Notes field gains "Resolution mechanism: `docs/prds/v0_4/shell-extract-engine-bridge.md`."
  - **Prereqs:** None.
  - **Crates touched:** docs only.

- **Task μ** — Cross-link from `docs/prds/v0_4/fea-gui-rendering-shells.md` to the MeshData payload-extension contract in this PRD's §4 (so when that PRD activates, its M-001/M-003/M-005/M-006/M-007/M-014 findings know which fields are now real).
  - **Observable signal:** `git diff docs/prds/v0_4/fea-gui-rendering-shells.md` cross-PRD section gains a row pointing here.
  - **Prereqs:** None.
  - **Crates touched:** docs only.

### Dependency view

```
α ─┐
   ├─→ γ ─┬─→ δ ─→ ε ─┐
β ─┘      │           ├─→ ι (end-to-end)
          └─→ ζ ──────┤
                     η ──→ θ ─┘

κ, λ, μ (independent doc edits)
```

Cross-PRD prereq edges that gate δ:
- compute-node-contract §8 task η (FEA solve_elastic_static ComputeNode trampoline)
- structure-instance-runtime §"Wave 1" foundation slice (SIR-α)

These are real `add_dependency` edges at decompose time per memory `preferences_cross_prd_deps_real_edges` reversal (2026-05-12 — the orchestrator scheduler reads dep edges, not metadata).

## §10 — Out of scope

- **Display-mode toggle (mid / extruded / both)** — pure-frontend; owned by fea-gui-rendering-shells PRD per GR-016 §2.5.
- **Top/mid/bottom stress-channel selector UI** — pure-frontend; same.
- **Shell-normal arrow overlay (rendering)** — pure-frontend; the IPC half (`vector_channels`) lands here, the rendering lands in fea-gui-rendering-shells.
- **Per-vertex `thickness` scalar channel** — owned by `varying-thickness-shells.md` (v0.5) per GR-016 §2.4. The data exists on `MidSurfaceMesh.thickness` end-to-end; surfacing it as a `scalar_channels["thickness"]` key is a future single-file addition under the same field.
- **MMG2D remeshing on `MesherError::QualityBelowThreshold`** — parent shells PRD M-009 PARTIAL state; deferred (no Rust FFI). The diagnostic surfaces; the fix is a future PRD or follow-up task against the parent shells PRD.
- **MITC3+ curved-element accuracy** — task **3392** (parent shells PRD §"Updates"); not in this PRD. The widened benchmark bands per task 3034 remain the v0.4 contract.
- **Mid-surface morphing under parameter changes** — mesh-morphing PRD scope; cited as a future compose by parent shells PRD but not in this PRD.
- **Composite-laminated and varying-thickness `Field<X,Y>`-typed result extensions** — v0.5 PRDs; the `ShellChannels { top, bottom, frame }` shape here is sufficient for the v0.4 single-material constant-thickness contract, and the per-vertex thickness is preserved upstream for future v0.5 consumers.
- **Buckling K_g / eigensolver** — GR-024 / structural-stability-buckling; not in this PRD.

## §11 — Open questions (surfaced but not decided in this session)

1. **Per-element vs per-vertex layout of `ShellChannels.top` / `bottom` / `frame`.** The Rust kernel produces per-element data (`ShellElementStress` at `shell_result.rs:78`); the GUI consumes per-vertex (matches `MeshData.scalar_channels` length contract `== vertex_count`). The conversion (volume-weighted nodal averaging, matching the v0.3 tet-stress recovery pattern at `result.rs::recover_nodal_stress_p1`) is needed somewhere. **Suggested resolution:** persist per-element in `ShellChannels` (smaller; native to the kernel); convert to per-vertex at the GUI-side populator (task θ) using the existing recovery helper. Decide during §9 task δ design step.

2. **OpenVDB B-rep→Voxel precondition.** Parent shells PRD M-025 (varying-thickness M-001 echo) notes that `BRep→Voxel` sampling is "deferred to a separate follow-up" in `reify-kernel-openvdb::register`. This PRD's task δ depends on a real `SampledField` flowing into the shell-extract trampoline — synthetic-input testing covers γ, but δ needs the live producer. **Suggested resolution:** the OpenVDB B-rep→Voxel work is GR-003's scope (cluster C-17 / GR-003); cross-PRD `add_dependency` from δ to that PRD's relevant task when filed. If GR-003 lags, task δ ships against a synthetic SampledField path and a follow-up task wires the OpenVDB producer.

3. **MeshData.element_kind cardinality.** Today `u8` is sufficient (0 = tet face, 1 = shell triangle). Future kernels (hex, wedge per `hex-wedge-meshing.md`; beam future) would extend this. **Suggested resolution:** `u8` for now; document the byte-value enum in `gui/src/types.ts` comment block; if cardinality grows past ~16, revisit by widening to `u16` or moving to a string-tag scheme. Decide during §9 task η decomposition planning if hex-wedge lands in parallel.

4. **`vector_channels` length-contract clarity.** §4 specifies per-vertex OR per-face length; the existing `scalar_channels` is per-vertex only. Two-mode contracts are weaker than one-mode; introducing the new HashMap as a separate `vector_channels_per_face: Option<HashMap<…>>` is cleaner but more verbose. **Suggested resolution:** single HashMap; encode "per-vertex" vs "per-face" in the channel name (e.g. `"shell_normal_per_face"`); document the convention in the field's docstring. Decide during §9 task η.

5. **Cancellation-during-extraction granularity.** Extraction is 5 phases (medial / mid-surface / pruning / mesher / segmentation / naming). The 100ms SLA from compute-node-contract §2 may be hard to meet between every phase (some phases are sub-100ms; others are tens of seconds for large grids). **Suggested resolution:** poll at every phase boundary and ALSO at every 1000-voxel inner-loop step; document the effective cancellation latency per phase in code comments. Decide during §9 task γ design step.

6. **Failure-diagnostic vocabulary in the wider corpus.** The seven codes in §7 (`E_SHELL_NO_VOXEL_GRID`, etc.) are new. Per cluster C-09 / GR-012 disposition `accept-and-document`, naming convention favors PascalCase Rust variants (`Diagnostic::ShellNoVoxelGrid`). The user-message templates are placeholder; final wording lives in the implementation task. **Suggested resolution:** mechanical naming; finalize messages during §9 task ε.

7. **Backward-compatibility window for `ELASTIC_RESULT_FORMAT_VERSION`.** Cache entries from before the bump deserialize cleanly to `shell_channels: None`. Open: do we retain v(N-1) read support indefinitely, or sunset after N+1 releases per memory pattern? **Suggested resolution:** retain v(N-1) read indefinitely (deserialization cost is negligible; users don't expect cache wipes on minor updates). Revisit only if the format diverges substantially. Decide during §9 task β.

8. **Foundation-task disposition for 3031 / 3032 / 3036.** Per §9 task κ: 3031 and 3032 are cancelled with reopen_reason pointing here; 3036 is cancelled (superseded by ι); 3033's already-done producer-side stands, with engine-side fold-in re-filed as ζ. **Confirm with Leo before task κ files the supersession** — the parent shells PRD's status block needs Leo's eyes.
