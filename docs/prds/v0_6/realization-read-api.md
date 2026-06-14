# Realization-Read API — `RealizationReadHandle` content accessors

**Status:** authored 2026-06-10 (design session with Leo; forks resolved live).
**Owner surface:** `crates/reify-eval` (`engine_compute.rs`, `engine_eval.rs`, new projection module), `crates/reify-ir` (one new `GeometryKernel` method), `crates/reify-kernel-gmsh`.
**Shape:** B + H (contract + two-way boundary tests) — this PRD lives on the ComputeNode-dispatch seam (`engine-integration-norm.md` §3.4), a load-bearing seam per the audit portfolio.

## §0 — Context and supersession

`compute-node-contract.md` §9 item 8 left the realization read-handle type "to §8 task γ's design step … Defer; either works for the contract." Task γ (3422) shipped `RealizationReadHandle` carrying **only** `node_id`, with a doc comment deferring content accessors to "downstream slices (δ/ε/ζ)" and citing "§9 Q8". Both halves of that comment are dead:

- compute-node-contract δ/ε/ζ = tasks **3423/3424/3425** — all done; none ever scoped content accessors (they own Freshness::Pending, cancellation, OpaqueState warm-state).
- The PRD has no Q-labels; §9 item 8 never assigned an owner.

Separately, `shell_extract_compute.rs` promised "Tasks δ/ε will migrate [the SDF] to `realization_inputs[0]` once the realization-read API lands" — shell-extract-bridge δ (**3594**) is done **without** migrating, and ε (**3595**) is cancelled. (Different Greek namespace from compute-node-contract's δ/ε; cite task ids, not letters.)

This PRD is the owner. It completes compute-node-contract §9 item 8 and supersedes the stale deferral comments at `engine_compute.rs:102-115` and the `shell_extract_compute.rs` header/inline seam notes.

**Premise correction (recorded so nobody re-derives the dead sketch):** §9 item 8's "minimum-viable `(RealizationNodeId, &EvaluationGraph)` tuple" cannot work. The `EvaluationGraph` holds no content — `RealizationNodeData` carries `{id, operations, content_hash, produced_repr, geometry_cell, produced_kernel}` only. Realized content is kernel-owned, reachable via `Engine.realization_handles: HashMap<RealizationNodeId, GeometryHandleId>` (lib.rs) + `RealizationCache<KernelHandle>`. Any content accessor must therefore be fed by the Engine, which owns the kernels.

## §1 — Consumers and user-observable surface (G1)

In-engine seam: **§3.4 ComputeNode dispatch** (`engine-integration-norm.md`).

| Consumer | What it reads | Status |
|---|---|---|
| Task **4091** (structural-analysis-fea P1) | realized **VolumeMesh** for the elastic solve, replacing the synthetic Freudenthal box (`compute_targets/elastic_static.rs:144` `_realization_inputs`) | pending; dep edge → this PRD's γ |
| Task **3429** (compute-node-contract κ, mesh-morph ComputeNode at VolumeMesh realization dispatch) | source **VolumeMesh** content for the morph trampoline | pending; dep edge → this PRD's γ |
| Shell-extract trampoline (`shell_extract_compute.rs`) | body **SDF** (`SampledField`) via `realization_inputs[0]` instead of the `value_inputs[1]` smuggle | migration owned by this PRD (ε leaf) |
| Future: any `@optimized` fn taking geometry args | content per produced repr | the β lowering rule makes `Value::GeometryHandle` args flow into `realization_inputs` automatically |

**Explicit non-consumer:** task 4472 (re-scoped 2026-06-10 to a build-time pre-derivation pass). Content accessors deliver realized content, not eval-time kernel queries — that door stays closed.

User-observable surface, honestly stated: the in-batch end-to-end signal is an engine-API integration test driving a **real `.ri`-compiled body** through realization → projection → trampoline (η leaf). The full *user-level* surface (CLI/GUI behaviour difference on a `.ri` file) arrives when 4091 swaps the elastic solve onto the realized mesh — its dep edge onto this batch is wired at decompose so the chain cannot strand.

## §2 — Ground truth this design stands on (verified 2026-06-10)

- `ComputeNodeData.realization_inputs: Vec<RealizationNodeId>` already exists (`graph.rs:159`) — graph model needs no change.
- `compute_cache_key.rs` **already folds realization-input content hashes** (reorder-invariance, domain separation, panic-on-missing — all tested). Cache-key composition is NOT this PRD's scope; β merely starts feeding it real ids.
- `Value::GeometryHandle { realization_ref: RealizationNodeId, upstream_values_hash, kernel_handle }` (GHR work, landed) is the lowering bridge from args to realizations.
- `ComputeFn` is a plain `fn` pointer: `fn(&[Value], &[RealizationReadHandle], &Value, Option<&OpaqueState>, &CancellationHandle) -> ComputeOutcome`. **Hard constraint: it stays one.** Purity (determinism, cacheability, warm-state replay, cancellability) is the contract.
- All three production dispatch sites pass `realization_inputs: vec![]` / `&[]` today (`engine_eval.rs` @optimized, FEA, shell-extract upstream).
- The shell-extract production path feeds a **synthetic slab SDF by design** (`build_slab_sdf(height)`, per shell-extract-bridge PRD §11 OQ-2): the FEA signature carries no body geometry. The migration therefore has a real upstream gate (see ζ), not just plumbing.
- `GeometryKernel` has `tessellate(handle, tol) -> Result<Mesh, _>` and `ingest_mesh`, but **no handle→`VolumeMesh` projection method** — γ adds it.
- `reify_ir::geometry::VolumeMesh` (vertices/tet_indices/element_order) and `SampledField` (dense CPU grid, `Arc`-shared) both exist as content types.
- The seven `E_SHELL_*` diagnostic codes all exist in `reify-core/src/diagnostics.rs`; 3595's vocabulary is NOT orphaned. Only `ShellNoMedial` is defined-but-never-emitted → separate small filing, out of this PRD.

## §3 — Contract

### 3.1 Types

```rust
/// Immutable realized content, projected once per (node, content_hash) and
/// shared by Arc. Variants mirror ReprKind's *content-bearing* members.
pub enum RealizedContent {
    /// produced_repr ∈ {Sdf, Voxel}: a dense CPU-resident grid (densified
    /// from the Voxel kernel via the thickness-PRD machinery).
    Sdf(Arc<SampledField>),
    /// produced_repr = Mesh: surface mesh.
    SurfaceMesh(Arc<Mesh>),
    /// produced_repr = VolumeMesh: tet mesh (P1/P2 per element_order tag).
    VolumeMesh(Arc<VolumeMesh>),
}

pub struct RealizationReadHandle {
    /// Identity (unchanged from γ/3422).
    pub node_id: RealizationNodeId,
    /// The realization's content hash at projection time — lets trampolines
    /// and tests assert identity without re-hashing content. Mirrors the
    /// value compute_cache_key already folds.
    pub content_hash: ContentHash,
    /// None ⇔ BRep-only realization, projection unavailable (no kernel /
    /// cfg(not(has_openvdb)) / projection failure). Never fabricated.
    content: Option<RealizedContent>,
}

impl RealizationReadHandle {
    pub fn content(&self) -> Option<&RealizedContent>;
    pub fn sdf(&self) -> Option<&SampledField>;
    pub fn surface_mesh(&self) -> Option<&Mesh>;
    pub fn volume_mesh(&self) -> Option<&VolumeMesh>;
}
```

`content` is private; only the Engine-side constructor builds handles. `Clone` stays derived (Arc clones are cheap; handles may be captured into warm state or a future async driver without lifetime entanglement — this is why content is `Arc`'d, not borrowed).

### 3.2 Invariants (the purity contract, restated for content)

1. **No kernel access.** Accessors expose bytes, never `GeometryKernel` or `Engine`. The `ComputeFn` signature does not change.
2. **Determinism.** Same `content_hash` ⇒ byte-identical content. Projection is performed by the Engine before trampoline invocation; the trampoline sees a fixed snapshot.
3. **Cache coherence.** `compute_cache_key` already folds realization `content_hash`es; a geometry edit produces a new content_hash ⇒ new cache key ⇒ re-dispatch with re-projected content. No additional invalidation protocol is needed.
4. **Cancellation safety.** Projection completes (or degrades to `None`) strictly before `invoke_compute_trampoline`; a cancelled dispatch never leaves partial content in the store (store writes are whole-value inserts).
5. **Honest degradation.** Projection failure ⇒ `content = None` + a diagnostic surfaced at the lowering site. Trampolines decide their own policy (shell-extract falls back to the slab; strict consumers return `ComputeOutcome::Failed` with a clear diagnostic). Never a fabricated value.

### 3.3 Projection (Engine-side)

**Lazy at the lowering site, memoized** (resolved fork — see §4 D2). When a dispatch lowering encounters a node with non-empty `realization_inputs`, the Engine resolves each id:

1. Look up `(node_id, content_hash)` in a new Engine-owned projection store (`HashMap<(RealizationNodeId, ContentHash), RealizedContent>`); hit ⇒ clone the Arc.
2. Miss ⇒ resolve `node_id → KernelHandle` via `Engine.realization_handles` / `RealizationCache`, then project by `produced_repr`:
   - `VolumeMesh` → new `GeometryKernel::volume_mesh(handle) -> Result<VolumeMesh, QueryError>` (default impl: unsupported; gmsh implements) — γ.
   - `Mesh` → `kernel.tessellate(handle, achieved_tol)` (read-back of an already-meshed realization; tolerance semantics: §10 OQ-1) — γ.
   - `Sdf` / `Voxel` → densify to `SampledField` via the thickness-PRD machinery (4421 `densify_grid_to_sampled`; chains driven by 4422's Voxelize dispatcher stage). `cfg(not(has_openvdb))` ⇒ `None` — δ.
   - `BRep` → `None` (identity-only; consumers demand a converted realization upstream — resolved fork, §4 D1).
3. Memoize and construct the handle.

Stale entries (superseded content_hashes) are unreachable by construction; eviction policy is tactical (§10 OQ-2).

### 3.4 Lowering rule (β)

At the `@optimized` dispatch lowering, every argument that is a `Value::GeometryHandle` contributes its `realization_ref` to the node's `realization_inputs`, in argument order, no dedup (the cache key is already order/cardinality-sensitive and reorder-invariant where it should be). The value itself continues to flow through `value_inputs` unchanged — identity via value, content via handle. `run_compute_dispatch` callers stop passing `&[]` and pass handles built per §3.3.

## §4 — Resolved design decisions (Leo, 2026-06-10)

- **D1 — Repr coverage v1:** Sdf + VolumeMesh + Mesh content; **BRep = identity-only** (no auto-tessellation in the accessor; cross-repr conversion remains the dispatcher/demanded-repr machinery's job). Rejected: BRep auto-tessellate (bakes a tolerance choice into the accessor); minimal Sdf+VolumeMesh-only (Mesh is cheap, `tessellate` exists).
- **D2 — Projection phase:** lazy at the lowering site with an Engine-owned memo store. Build-phase pre-projection was rejected: `@optimized` ComputeNodes are inserted **and dispatched synchronously during eval**, so demand is unknowable at build time; pre-projecting everything pays memory for content nobody reads. Precedent: 4423's on-demand `realize_solid_sdf`. Purity is the contract, not phase.
- **D3 — Shell-extract migration:** **dual-source now** — trampoline prefers `realization_inputs[0]` SDF content when present, retains the `value_inputs[1]` slab fallback as the live production path; **plus a removal leaf (ζ)** that deletes the deprecated route as soon as its gates land (per Leo: file the cleanup now, dep-gated, don't let the dual-source linger unowned).
- **D4 — Seam claims:** dep edges 4091→γ and 3429→γ; Sdf machinery **reused** from the thickness PRD (4421/4422 dep edges, no parallel solid→SampledField path — avoids re-creating the C-17 duplicate); `ShellNoMedial` emission filed separately, outside this PRD; the kernel `volume_mesh()` method lives **here** (ratified via D1's "new kernel projection method"; pushing it into 3429 would recreate the unowned-seam pattern).

## §5 — Pre-conditions

| Pre-condition | State | Bearing |
|---|---|---|
| 4421 (thickness α: openvdb `ingest_mesh` + `densify_grid_to_sampled`) | pending | gates δ (Sdf projection arm) |
| 4422 (thickness β: dispatcher Voxelize stage) | pending | gates real-chain tests in δ/ε/η |
| gmsh kernel present (`Convert{from: Mesh} → VolumeMesh` claim) | on main | γ implements `volume_mesh()` against it |
| `compute_cache_key` realization folding | done on main | none — consumed as-is |
| GHR `Value::GeometryHandle` cells | done on main | β's lowering bridge |

No grammar work: this PRD introduces zero novel `.ri` syntax (G3 trivially green; all leaves `grammar_confirmed=true`).

## §6 — Out of scope

- Body-geometry-in-FEA-signature (who puts a body realization in reach of the FEA lowering) — owned by structural-analysis-fea (4091) / typed-fea axes. ζ consumes it, doesn't build it.
- Cross-repr conversion demands (BRep→Mesh→Voxel chains) — multi-kernel/thickness PRDs own conversion; the accessor only exposes what was produced.
- Eval-time kernel queries (mass properties etc.) — 4472's re-scoped pass; explicitly not this API.
- `Freshness` semantics, warm-state, cancellation — already contracted by compute-node-contract; this PRD only adds invariant 3.2-4 at the projection boundary.
- GUI/LSP display of realization content; persistent (on-disk) content caching.

## §7 — Cross-PRD seams (G4)

| Seam | Owner | Disposition |
|---|---|---|
| Accessor API + projection store + lowering rule | **this PRD** | α/β |
| `GeometryKernel::volume_mesh()` projection method | **this PRD** (γ) | consumed by 3429, 4091 |
| solid→Voxel→SampledField machinery | thickness PRD (4421/4422) | reused via dep edges; δ adds only the projection-store arm |
| VolumeMesh realization dispatch + mesh-morph ComputeNode | 3429 (compute-node-contract κ) | dep edge 3429→γ added at decompose |
| Elastic solve on realized VolumeMesh | 4091 (structural-analysis-fea P1) | dep edge 4091→γ added at decompose |
| Body reaches FEA/shell-extract lowering | structural-analysis-fea / typed-fea | gates ζ only |
| `E_SHELL_NO_MEDIAL` emission | separate small task (filed at decompose, outside this batch) | not this PRD |
| Stale comment sites (`engine_compute.rs:102-115`, `shell_extract_compute.rs` header + inline) | **this PRD** | α and ε respectively; comments repoint to this PRD by path |

## §8 — Boundary tests (H; two-way)

**Engine→trampoline direction** (does projection deliver correct content?): a probe trampoline registered in tests captures its `&[RealizationReadHandle]`:

- per-repr correctness: a `.ri`-compiled box realization converted along the production chains yields (γ) `volume_mesh()` with `tet_indices.len() % 4 == 0` and >0 tets, element_order tag preserved; (δ) `sdf()` a finite `SampledField` whose grid covers the body bounds, sign convention consistent with `compute_medial_mask`'s expectation (asserted structurally, not against a guessed closed form);
- memoization: two dispatches over the same content_hash observe `Arc::ptr_eq` content;
- invalidation: a param edit changes content_hash ⇒ re-projection (different content, new cache key — reuses the existing `compute_cache_key` tests' machinery);
- degradation: missing kernel / stub build ⇒ handle present, `content() == None`, diagnostic surfaced, no panic, no fabricated value.

**Trampoline→engine direction** (do consumers honour the contract?): the dual-source shell-extract trampoline consumes `sdf()` when present and falls back to slab when `None` (both arms tested); the `ComputeFn` signature is unchanged (purity is type-enforced — no `&Engine` anywhere reachable); a cancelled dispatch leaves the projection store coherent (insert-before-invoke).

## §9 — Decomposition plan

One Greek letter per leaf; signals are what the orchestrator's reviewer can observe. DAG: α → β → {γ, δ} → ε → ζ; η gates on γ/δ/ε.

- **α — Accessor types + API** (`engine_compute.rs`): `RealizedContent`, handle extension (content_hash + private content slot + constructor), typed accessors; repoint the dead "§9 Q8 / δ-ε-ζ" doc comment at this PRD. *Signal:* `cargo test -p reify-eval` — handle built with each content variant returns typed refs; Clone shares Arcs; `rg '§9 Q8' crates/` is empty.
- **β — Lowering + projection store** (`engine_eval.rs`, new `realization_content.rs`): `Value::GeometryHandle` args → `realization_inputs` population; memoized projection store; handle construction at the dispatch lowering sites; diagnostics on projection failure. Deps: α. *Signal:* integration test — an `@optimized` fn with a geometry arg yields `realization_inputs == [its node]`; probe trampoline receives a handle whose content_hash matches the graph node; geometry edit changes the dispatch cache key.
- **γ — VolumeMesh + Mesh projection arms** (`reify-ir` trait method, `reify-kernel-gmsh` impl, β's store): `GeometryKernel::volume_mesh()` (default unsupported), gmsh implementation, `SurfaceMesh` arm via `tessellate`. Deps: α, β. *Signal:* integration test — a Mesh→VolumeMesh converted realization read through the probe has >0 tets and the element_order tag; a Mesh-repr realization returns its surface mesh; consumers 4091/3429 are dep-wired onto this leaf.
- **δ — Sdf projection arm**: Voxel/Sdf-repr → `densify_grid_to_sampled` → `SampledField`, `cfg(has_openvdb)`-gated with honest `None` degradation. Deps: α, β, **4421**, **4422**. *Signal:* integration test — box → Voxel chain → `handle.sdf()` is `Some` and CPU-sampleable; stub build → `None` + diagnostic, no panic, no number.
- **ε — Shell-extract dual-source migration** (`shell_extract_compute.rs`): prefer `realization_inputs[0].sdf()`, retain slab fallback; repoint header + inline "γ-only seam" comments at this PRD. Deps: α, β, δ. *Signal:* integration test — extraction over a real body SDF produces a mid-surface tracking the body's extents (≠ slab); all existing slab-path tests stay green; `rg 'Tasks δ/ε will migrate' crates/` is empty.
- **ζ — Deprecated-route removal**: delete `build_slab_sdf` + the `value_inputs[1]` SDF contract once a real body realization reaches the shell-extract upstream inserter. Deps: ε, **4091** (body visibility at the FEA lowering), 4422. Pending-with-deps per scheduler norm — this is the D3 cleanup Leo mandated. *Signal:* `rg build_slab_sdf crates/` empty; trampoline rejects missing realization input with a clear diagnostic; FEA shell-route e2e green on real geometry.
- **η — Integration gate (critical)**: the §8 two-way boundary suite + the real-chain e2e (`.ri` fixture → build → Voxel/VolumeMesh realizations → ComputeNode dispatch → output reflects real geometry), degradation matrix across `cfg(has_openvdb)`. Deps: γ, δ, ε. *Signal:* the e2e runs in CI under `cargo test -p reify-eval`; honest statement: user-level (CLI/GUI on `.ri`) observability completes via 4091, dep-wired.

## §10 — Open questions (tactical; decided at implementation time)

1. **Mesh read-back tolerance** — for a Mesh-repr realization `tessellate(handle, tol)` should be an identity read; confirm the mesh kernels ignore `tol` on already-discrete handles, else thread the achieved tolerance from the build tables.
2. **Projection-store eviction** — unbounded in v1 (entries are keyed by content_hash and become unreachable on edit). If memory pressure shows up, tie into the existing cache budget/LRU machinery; measure first.
3. **Voxel spacing for δ's densify** — reuse 4423's policy when it lands; until then take the chain's default spacing. Keep δ's assertions structural so this stays a non-breaking knob.
4. **Duplicate `realization_inputs`** — the same handle passed twice is allowed (arg order preserved). If a real trampoline ever cares, add dedup at its own boundary, not in the lowering.
