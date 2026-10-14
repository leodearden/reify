# Analysis stratum: compute targets in `reify-analysis`, one descriptor per target, the contract below the solver

**Milestone:** v0_6 · **Status:** active (authored 2026-10-13; decomposed 2026-10-14). The leaf ids are in §10 and the sidecar: α #8984, β #8986, γ #8991, δ #8988 (high), ε #8994, ζ #8996, ι #8990, η #8997, κ₁ #8999, κ₂ #9001, λ #9002, μ #9003, ν #9004, ξ #9006, ο #9007, π #9008, ρ #9009, σ #9010 (close). Adopted: #8525. Subsumed and cancelled: #4935. · **Date:** 2026-10-13 · **Approach:** B + H (contract and two-way boundary tests). FEA and ComputeNode dispatch are overlay-listed load-bearing seams, and the blast radius is 9 crates plus gui.

**Program:** `plans/review-all-program-reify-20261012.md`, stream **S5** (wave 2, spawn, size L, risk medium). The stream's remedy, its deletable list and its seams were ruled in that document's "S5 Analysis stratum" decision, and this PRD does not reopen them. This session raised design questions, and Leo ruled on them on 2026-10-13 (§13). An adversarial critic seat reviewed the draft (1 blocker, 6 majors, 8 minors). A fold-verifier and DAG checker then reviewed the folded draft (4 FAILs, 12 WARNs). Every finding from both is folded in below.

**Code anchors** were verified against main `b1a7be2982` (2026-10-13). Every S5 path is byte-identical between the review's pinned tree `a030ee7528` and `b1a7be2982` (`git diff a030ee7528..b1a7be2982` over each path is empty). Main moves fast, and a large wave-1 and wave-2 batch rewrites these files before S5 dispatches (§8). Cite by symbol, and re-locate each symbol at implementation time.

**Normative substrate:** `docs/legibility/design-invariants.md`.
- This PRD gives INV candidate 4 (S5's half) its checks. That half says `@optimized` targets and Rust-minted result types agree with their `.ri` declarations, under INV-PD-2 `result-fields-populated-or-owned`.
- It complies with **INV-SF-6** (`diagnostics-carry-codes`), **INV-SF-5** (`placeholders-owned-and-loud`) and **INV-ARCH-1** (no module SCC grows).
- Every extracted primitive ships with an adoption-ratchet row (INV candidate 5).
- Quality heuristics are cited by name from `/home/leo/src/dark-factory/docs/code-quality.md`.

---

## 1. Goal

Compute targets get a stratum of their own. After this PRD:

1. **One crate holds the analysis code.**
   - Every compute target lives in a new crate, `reify-analysis`, together with its trampoline, its warm-state cache and its cache key. Today these are split between `reify-stdlib` and `reify-eval` by dependency accident.
   - `reify-analysis` sits below `reify-eval` and never depends on it. The front ends inject it into the Engine as a static descriptor table.
   - At the end state `reify-eval` has no normal dependency on it. `reify-eval` loses `reify-fdm`, `faer` and `rustc-hash`, and stops naming any target.
2. **One descriptor per target.**
   - A target is one `ComputeTargetDescriptor` row in one table. The row carries a typed `TargetId`, the `ComputeFn`, the realization demand, the persistence codec, the significance comparator, the completion hook, the upstream producer, the cancellation support and the `.ri` surface.
   - These all derive from the row: `PERSISTABLE_TARGETS`, the persistent lookup/write arms, the significance opt-in list, the demand sets, the shell-extract fold sites, the `solver::elastic_static` upstream branch, the LSP target list and the `.ri` `@optimized` parity check.
   - A new persistable, fold-hooked target is one row, not 4–6 string tables.
3. **The contract sits below the solver.**
   - `reify-compute-contract` keeps the ABI: `ComputeFn`, `ComputeOutcome`, `DispatchError`, `CancellationHandle`, `RealizationReadHandle`, a contract-owned structured detail type, the descriptor types and the progress channel.
   - It drops its `reify-solver-elastic` dependency, so its closure no longer reaches the FEA solver.
4. **One cancellation channel.** The GUI cancel button stops every cooperative target, not only `solve_elastic_static`. The `ComputeFn` `cancellation` argument carries a handle linked to the engine's active-solve handle, and no trampoline reads a cancel handle from a thread-local.
5. **One shell-extract codec.** The `ShellExtractionResult` projection has one typed encoder/decoder. The persistence bridge, the topology fold and the GUI mesh view all use it. A warm persistent-cache hit returns the same Value as the cold dispatch.
6. **Identity and schema agree with `.ri`.**
   - A test fails, naming the target, when a stdlib `@optimized` string has no registered descriptor, or when a registered target has neither a `.ri` caller nor an owned exemption.
   - Every Rust-minted result type is either declared in stdlib, where S15's builder parity covers it, or listed in one engine-internal register whose codec round-trips.
   - `solver::tensegrity_load` gets its declared `.ri` surface.

**Consumers (G1).**
- The Engine's ComputeNode dispatch (`docs/prds/v0_3/engine-integration-norm.md` §3.4) consumes the descriptor table.
- The front ends inject it: `reify-cli` (`register_compute_trampolines`, and `run_tests` for `reify test`) and the GUI engine session.
- The GUI consumes the cancellation channel (the `cancel_solve` Tauri command) and the shell mesh view (`EngineSession::set_active_fea_case`, `build_gui_state`).
- reify-lsp's posture-lock test consumes the registered target set.
- **S1** consumes the injection point for the one Engine constructor and its Registration posture.
- **S16** consumes the moved `elastic_static` and modal code and the elastic descriptor's upstream planner, which its route enum absorbs.
- **#4936** (merge-gate-compile-cost D-D) consumes the moved crate to measure the relink-wave shrink.

## 2. Background (verified on main 2026-10-13)

The finding records in `plans/review-all-reify-20261010.json` carry the full measurements. Five read-only scout seats re-measured them at `b1a7be2982`. A critic seat re-verified the load-bearing ones. The facts that carry the design are below.

- **The analysis code barely touches the engine (fk-6e78ea52b691).**
  - The analysis set is 21 files: 16 `compute_targets/*`, `modal_ops`, the trampoline half of `dynamics_ops`, `trajectory_ops` and `shell_extract_compute`. It has 19,562 production lines; the first-`#[cfg(test)]` rule gives 18,226 and undercounts four files.
  - It needs only these from `reify-eval`:
    - registration on `&mut Engine`;
    - the `solver_progress` thread-local, read by `elastic_static` only;
    - `topology_selectors::{nodes_for_faces, resolve_against_carried_topology}`, pure over the analysis-owned `CarriedTopology`;
    - `topology_selectors::resolve`, reached only through the production-dead `bc_resolve::resolve_selector_faces`;
    - `persistent_cache::BucklingResultCache`, a plain value type;
    - re-exported contract and `reify_ir` items.
  - Nothing reads a `Snapshot` or eval state.
- **The reverse couplings and the shared modules (critic F1/F4).**
  - `reify-eval` reaches into the set through about 25 symbols in 8 modules:
    - `compute_persist`'s six codec calls;
    - `engine_compute`'s two fold calls;
    - `engine_eval`'s shell-route helpers;
    - `engine_build`'s `bc_resolve::build_face_anchors`, which meshing-service ε #8288 deletes, and `dynamics_ops::{try_eval_body_mass_props, derive_mechanism_mass_props}`, which are realization-time kernel helpers, not compute targets;
    - `topology_selectors`' import of `CarriedTopology`, which S13 α #8747 removes;
    - `test_runner`.
  - `dynamics_psd` is reached only from `detectors` and the mass-props half, so it is not analysis code.
  - Some modules are shared across families:
    - `compute_targets/mod.rs` value helpers: `point3_length` serves FDM and form-find, `scalar_list` serves tensegrity and modal, the `sampled_*` helpers serve the FEA files.
    - S15's `stdlib_inputs` decoders, used by elastic, buckling, modal and as-printed.
    - `result_topology` (`CarriedTopology`), used by modal, FEA and, until #8747, `topology_selectors`.

    No family can move without them.
- **The stdlib halves.**
  - `modal/trampoline.rs` (`ModalCacheKey`, `TransientCacheKey`), `modal/free_vibration.rs` (pure f64 helpers) and `dynamics/trampoline.rs` (`InverseDynamicsCacheKey`, `body_solid_hashes`) have no caller inside reify-stdlib.
  - `modal/transient.rs` is also called by `trajectory/simulate.rs`, so it stays.
  - `trajectory/trampoline.rs` also backs `eval_builtin` intrinsics. Only its two cache keys are engine-facing.
- **The contract's solver edge is four symbols (fk-8cd376f72fca).**
  - `StructuredComputeDetail::Fea(reify_solver_elastic::FeaDiagnosticDetail)`. That is a 3-variant enum with no serde; its persisted form is a reify-eval mirror in `persistent_cache/diagnostics_wire.rs`.
  - From `elastic_result.rs`: `BudgetReason`, `ConvergenceStatus` (the public field type of `AposterioriEstimate.convergence_status`) and `progressive::PartialElasticResult`.
  - `elastic_result.rs` is 2,563 of the crate's 2,883 lines.
  - The orphan rule ties `ElasticResult` only to `reify_core::PersistentlyCacheable`.
  - `reify-eval` is the contract's only consumer crate.
- **Per-target behaviour is string-keyed in 11 production sites (fk-34bddf1fcbce, fk-7d4f2be7b634).**
  - `run_compute_dispatch`'s two shell-extract fold sites, one in the persistent-HIT arm and one in the Completed arm. A prose NOTE asks future paths to mirror them.
  - `engine_eval`'s two `target == "solver::elastic_static"` upstream branches. S4 η #8864 unifies them into one `CellHooks` site.
  - `engine_admin::shell_gui_mesh_data`'s classifier.
  - `compute_persist`'s `PERSISTABLE_TARGETS` and its 3 + 3 structurally identical lookup/write arms.
  - `significance_filter`'s `is_opted_in` and buckling branch.
  - Beyond those 11, the two volume-mesh demand sets are per-target tables too. meshing-service ε #8288 deletes the boundary one, and η #8290 plans a third (`register_volume_mesh_options(target, extractor)`).
  - `OptimizedComputeDispatcher` snapshots `fns` only.
- **Target identity (fk-250d45d9d1f4).**
  - Stdlib has 16 `@optimized` strings; production registers 19 targets.
  - Three targets are Rust-only:
    - `solver::tensegrity_load`, which has no `.ri` caller;
    - `fdm::as_printed_material_r0`, reached only through `rung_compute_key`. Its rung selector `target_fidelity` is owned by the PDROP decision gate **#7087**;
    - `shell-extract::extract`, engine-inserted upstream of `solver::elastic_static`.
  - reify-lsp's `ALL_PRODUCTION_COMPUTE_TARGETS` is a hand copy of 18. It omits shell-extract, and its doc admits an addition is not caught.
  - Seven Rust-minted result types have no `structure def`.
  - A misspelled `@optimized` target is detected only at dispatch.
- **Cancellation (fk-cf610e43d544).**
  - The GUI's `cancel_solve` flips the handle that `EngineSession::with_solve_slot` installs via `Engine::set_active_solve_cancel`. `run_compute_dispatch` copies it into the ambient `solver_progress` context.
  - Every dispatch's `cancellation` argument is a fresh `CancellationHandle::new()` created at three `engine_eval` sites and stored on `ComputeNodeData.running`. Its only production canceller is two copies of an unreachable branch.
  - `OptimizedComputeDispatcher::dispatch` also passes a fresh handle.
  - `elastic_static` is the only ambient reader. Nine trampolines poll the argument (modal 5, shell-extract 5, dynamics 2, trajectory 2, fdm_slice 2, multi_case 1, buckling_multi_case 1), so the GUI cannot cancel them.
- **Shell-extract projection (fk-952285bb817b, fk-750e0c9f3bd6).**
  - One encoder and three decoders, each with a different policy:
    - `value_to_shell_extraction_result`: outer type_name only, rows of exactly 3, u32;
    - `fold_mid_surface_attributes_into_table`: no type_name, warn-and-skip;
    - `engine_admin::parse_shell_extraction_result`: nested type_names, any row length, usize.
  - The encoder drops region voxels and turns diagnostics into Debug strings. A warm hit therefore has `segmentation.regions == []` where the cold one had N > 0, and `shell_extract_persistent_cache_round_trip.rs` pins that divergence.
  - No consumer reads `regions`, `diagnostics` or `solve_time_ms`.
  - `shell_gui_mesh_data`'s `// G-allow` says its GUI consumer is pending, but `gui/src-tauri/src/engine.rs` calls it at two sites.
- **Wire codec copies (fk-a0d2e755b215, weakened).**
  - `reify-shell-extract/src/result.rs` keeps private copies of the f64 slab codec and `MAX_F64_ELEMENTS`, plus u32, [u32;3], [f64;3] and [i32;3] slab codecs.
  - Its production code has 10 `unsafe` blocks, 2 per read fn.
  - Its f64 read path has drifted from the contract's: a different BE decode and different SAFETY text.
  - `severity_to_u8`/`severity_from_u8` exist twice: in `result.rs` and in `diagnostics_wire.rs`.
  - `reify_core::persistent_cache` holds only the trait.
- **Shim (fk-e6650962065a).**
  - `engine_compute.rs` re-exports six contract types, and `graph.rs` re-exports `CancellationHandle`.
  - Thirty `engine_compute::` paths use the shim inside reify-eval: 22 production, 8 test.
  - The contract crate has 0 tests outside `elastic_result.rs`. The six `RealizationReadHandle` accessor tests live in `engine_compute.rs` and use nothing from reify-eval.
- **The persistent cache's version key is reify-eval-scoped.**
  - `ENGINE_VERSION_HASH` is built by `reify-eval/build.rs` using `engine_hash_algo::WORKSPACE_CRATE_COVERAGE` and `compose_engine_version_hash`.
  - It hashes `src/compute_targets` and `src/shell_extract_compute.rs` by path. It pins the Cargo.lock versions of reify-eval's own closure (`engine_hash_closure.txt`, guarded by `tests/infra/test_engine_hash_closure.sh`).
  - Moving the persisted targets without moving this coverage would let a `faer` or analysis-source change serve stale FEA results (C5).
- **Dev-dependency topology matters for the OCCT gate (critic F5).**
  - `scripts/occt-touching-crates.txt` (pinned by `tests/infra/test_occt_gated_scope.sh`, mirrored in `dark-factory-orchestrator.yaml`) counts a crate as OCCT-touching through its direct dev-dependencies.
  - `reify-eval` is on that list. A `reify-analysis` dev-dependency on `reify-eval` would put the new crate in the OCCT gate.
- **#4935** (merge-gate-compile-cost D-B) planned the same extraction in the opposite topology. It was parked `deferred` on 2026-10-13 (§13 R3).

## 3. Sketch of approach

Four layers, in dependency order.

**Layer 1: early, independent fixes.** None of these waits for S15.
- **α:** shared wire primitives move to `reify_core::persistent_cache`.
- **β:** the structured detail becomes contract-owned.
- **γ:** the re-export shim goes, and the handle tests move.
- **Adopted #8525:** the panic guard is shared.
- **δ:** one cancellation channel.

**Layer 2: the descriptor, in place (ε, ζ, η, ι).**
- **ε** lands the descriptor types in the contract, and the registry stores descriptors. Every string-keyed site in reify-eval becomes a read of one typed row while the rows still live in reify-eval's `compute_targets`.
- **ζ** gives the shell projection its typed codec and GUI view.
- **η** lands the identity and schema checks.
- **ι** gives `tensegrity_load` its `.ri` surface.

**Layer 3: the stratum (κ₁, κ₂, λ, μ, ν, ξ, ο).**
- **κ₁** creates `reify-analysis` and its shared foundation (the cross-family helpers, `stdlib_inputs`, `result_topology`). It adds the injection point, the version-hash composition and a **transitional** normal edge `reify-eval → reify-analysis`, so families still in reify-eval reach the shared modules (§13 R8). The edge is legal because `reify-analysis` sits below.
- **κ₂, λ, μ, ν, ξ, ο** each move one family under one move protocol (C7): FDM, dynamics/trajectory, modal, FEA core, tensegrity/form-find, shell-extract. Each move takes the family's descriptor rows, its stdlib cache-key half and its tests.
- **ν** also takes `ElasticResult` out of the contract, so the contract drops the solver.

**Layer 4: retirement and record (π, ρ, σ).**
- **π** deletes reify-eval's legacy table, its `compute_targets` module and the transitional edge. It drops the dead dependencies and lowers the INV-ARCH-1 row.
- **ρ** amends `compute-node-contract.md`, the invariant registry and the overlay.
- **σ** closes the PRD.

## 4. Contract (H)

### C1 — Target identity and the descriptor (`reify_compute_contract::target`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TargetId(&'static str);
impl TargetId { pub const fn new(name: &'static str) -> Self; pub const fn as_str(self) -> &'static str; }

pub struct ComputeTargetDescriptor {
    pub id: TargetId,
    pub compute: ComputeFn,
    pub demand: RealizationDemand,                    // the per-target realization facts (see below)
    pub persistence: Option<&'static dyn ResultCodec>, // C3
    pub significance: Option<SignificanceFn>,          // C4
    pub on_completed: Option<CompletionHook>,          // C4
    pub upstream: Option<UpstreamPlanner>,             // C4
    pub cancellation: CancellationSupport,             // C6
    pub ri_surface: RiSurface,                         // C2 (added by η; ε lands the other fields)
}

/// A table with unique ids, plus the producing crate's version hash (C5). It may be empty (κ₁ lands the
/// injection before any row moves); BT4 asserts the *registered union* is non-empty and duplicate-free.
pub struct ComputeTargets { /* private */ }
impl ComputeTargets {
    pub const fn new(rows: &'static [ComputeTargetDescriptor], producer_version_hash: &'static str) -> Self; // panics (a compile error in const context) on a duplicate id
    pub fn iter(&self) -> impl Iterator<Item = &'static ComputeTargetDescriptor>;
}
```

`RealizationDemand` holds the per-target realization facts. Today those are `None | VolumeMesh | VolumeMeshWithBoundary`. Meshing-service moves the same facts:
- ε #8288 deletes the boundary demand;
- η #8290 adds a per-target volume-mesh options extractor.

Whichever of ε and those two tasks lands second adapts the other's work:
- If #8288 lands first, `RealizationDemand` has no boundary variant.
- If ε lands first, #8288 deletes the variant from the descriptor.
- #8290's extractor is a descriptor field (`volume_mesh_options: Option<MeshOptionsExtractor>`), never a `register_*(target, …)` table. If #8290 lands first, ε converts its table into that field.

The `target-string-routing` ratchet row makes a new string-keyed table red.

Invariants:

1. **A target is one row.**
   - Production code outside a descriptor table names no target string.
   - The compiled IR still carries the `.ri` string (`CompiledFunction.optimized_target`, `ComputeNodeData.target`), and the registry resolves it by name.
   - The ratchet row `target-string-routing` holds reify-eval production at 0.
2. **Registration is injection.**
   - `Engine::register_production_compute_fns(targets: ComputeTargets, morph: MorphRegistration)` is the one bundler (INV-FEA-1).
   - It registers every row of the injected table, plus reify-eval's legacy table until π. A duplicate id panics, as today.
   - `run_tests` takes the table from its caller.
   - reify-eval code never names `reify-analysis` at the end state (π). Until then, the transitional edge is used only to import the shared foundation (κ₁), and the `eval-imports-analysis` ratchet row only falls.
3. **Enumeration.** `Engine::registered_compute_targets() -> impl Iterator<Item = &ComputeTargetDescriptor>` exists. Every identity, register and cancellation test reads descriptors through it, so the tests hold before and after each move.
4. **Snapshots see descriptors.** `OptimizedComputeDispatcher` holds the registry's rows (`&'static` or `Arc`), not a copy of `fns` alone.

### C2 — `.ri` surface and result schema (INV candidate 4, S5's half)

```rust
pub enum RiSurface {
    StdlibOptimized,                      // some stdlib fn carries @optimized("<id>")
    EngineInserted { by: TargetId },      // dispatched only as another row's upstream (C4)
    RustOnly { owner: &'static str },     // reachable only from Rust; `owner` cites a live task "#NNNN: …"
}
```

- **Identity parity.**
  - A reify-eval integration test compiles the stdlib (`reify_compiler::load_stdlib`) and collects every `optimized_target`. It then checks four things against a production-registered engine's descriptors:
    - every collected string is the id of a `StdlibOptimized` row;
    - every `StdlibOptimized` row has a collected string;
    - every `EngineInserted { by }` names a row whose `upstream` can request it;
    - every `RustOnly` owner cites `#NNNN`.
  - A failure names the target. The three Rust-only targets today resolve as follows:
    - `shell-extract::extract` is `EngineInserted { by: solver::elastic_static }`;
    - `fdm::as_printed_material_r0` is `RustOnly { owner: "#7087: …" }`;
    - `solver::tensegrity_load` is `StdlibOptimized` once ι declares its surface.
- **Result-type register.**
  - `ENGINE_INTERNAL_RESULT_TYPES` lists every Rust-minted result `type_name` that no stdlib `structure def` declares. It lives with the shell codec, so in `reify-analysis` after ο. Each entry names its codec's round-trip test, or, for `CarriedTopology`, its owner **#7097** (`ModalResult.topology`).
  - A reify-eval integration test walks the output of one representative dispatch per registered descriptor. The representative-dispatch table must cover every row, and BT7 shares it. The test asserts that every `StructureInstance` `type_name` found is declared in the stdlib registry or listed in the register.
  - S15's C4 builder parity covers the declared types. Nothing user-unreachable enters the stdlib namespace (§13 R5).
- **The LSP list derives.** reify-lsp's posture-lock test builds a throwaway production-registered engine and asserts that the LSP engine registers none of its targets. `ALL_PRODUCTION_COMPUTE_TARGETS` is deleted.

### C3 — Persistence codec

```rust
pub trait ResultCodec: Sync {
    fn format_version(&self) -> u32;
    fn encode(&self, value: &Value, w: &mut dyn Write) -> io::Result<Encoded>; // Encoded::Written { uncompressed_bytes, solve_time_ms } | NotRepresentable
    fn decode(&self, r: &mut dyn Read) -> io::Result<Value>;
}
/// The adapter every persistable row uses: a typed cache record plus its Value bridge pair.
pub struct TypedCodec<C: PersistentlyCacheable + 'static> { pub to_cache: fn(&Value) -> Option<C>, pub to_value: fn(&C) -> Value }
```

- **One generic body.** `persistent_lookup` and `persistent_write` are one generic body each over `descriptor.persistence`. The `WithDiagnostics` envelope (diagnostics and structured-detail replay) stays engine-side and wraps the codec's payload.
- **The persistable set derives.** It is `rows.filter(|r| r.persistence.is_some())`. `PERSISTABLE_TARGETS` is deleted. `engine_hash_tests::PERSISTED_TARGET_SOURCES` and `persistable_target_fixtures` are asserted set-equal to the derived set.
- **Exact by construction.** Every codec ships a round-trip test, `to_value(to_cache(v)?) == v`, on a representative production value. No release-build `_ => None` arm remains.

### C4 — Significance, completion and upstream hooks

```rust
pub enum Significance { Equivalent, Different }
pub type SignificanceFn = fn(prev: &Value, new: &Value, length_tolerance_si: f64) -> Significance;

pub struct CompletionSinks<'a> { pub topology_attributes: &'a mut reify_ir::TopologyAttributeTable }
pub type CompletionHook = fn(result: &Value, sinks: &mut CompletionSinks<'_>) -> Result<(), CompletionError>;

pub struct UpstreamRequest { pub target: TargetId, pub value_inputs: Vec<Value>, pub on_failure: UpstreamFailure }
pub enum UpstreamFailure { Propagate, FallBack { code: reify_core::DiagnosticCode, message: &'static str } }
pub type UpstreamPlanner = fn(downstream_value_inputs: &[Value]) -> Option<UpstreamRequest>;
```

- **Significance.**
  - The generic part of `significance_filter` stays in reify-eval: not-opted-in when `significance` is `None`, the `prev == new` shortcut, and an invalid tolerance treated as `Different`.
  - The per-target comparison is the row's fn. Today's elastic and buckling comparators move to their rows.
- **Completion.**
  - `run_compute_dispatch` calls `on_completed` at exactly one site, which both the persistent-hit path and the Completed path pass through. The mirror NOTE is deleted.
  - A `CompletionError` becomes one coded Warning diagnostic, never `tracing::warn` alone (INV-SF-6).
- **Upstream.**
  - One generic Engine routine replaces `insert_shell_extract_upstream`. It is called from the one `CellHooks` site that S4 η #8864 leaves.
  - The routine runs the downstream row's planner. If the planner returns a request, it allocates the synthetic node and cell and fills the node's `cache_key` with `compute_cache_key` (#3428). It dispatches `request.target` through the same registry, registers the content-hashed synthetic value cell (`shell_extract_feed`), feeds that cell to the downstream node and applies `on_failure`.
  - The elastic row's planner is today's shell classification (`extract_shell_route_params`, `classify_shell`, `build_slab_sdf`), with `resolve_extraction_failure` mapped to `UpstreamFailure`.
  - The fallback Warning gains a `DiagnosticCode`.

### C5 — Version hash (persisted results stay sound across the move)

- **From κ₁, every persistent key covers both crates.** The persistent entry key composes `ENGINE_VERSION_HASH` (reify-eval's dispatch path) with `reify-analysis`'s producer hash, carried on `ComputeTargets`. The composition reuses the existing `compose_engine_version_hash`.
  - This applies to rows still in reify-eval's legacy table too, because they already use shared modules from reify-analysis.
  - It over-invalidates in the safe direction.
- **One algorithm, two build scripts.**
  - `reify-analysis/build.rs` computes its hash over its own sources and Cargo.toml, its path-crate coverage rows and the Cargo.lock pins of its closure.
  - The algorithm (`engine_hash_algo`) moves to `reify-build-utils`, so neither build script `include!`s another crate's file.
  - `reify-analysis` gets a `WORKSPACE_CRATE_COVERAGE` row.
  - A reify-eval row for moved sources is removed only together with the analysis row that now covers them.
- **The closure drift guard covers both crates.** `tests/infra/test_engine_hash_closure.sh` is extended in place to check reify-analysis's closure manifest too. It is an existing member, so no new member and no new manifest row; the Python policy governs new members.
- **The one-byte-flip test is re-derived over the composed key.** `every_persisted_target_source_changes_engine_version_hash_when_one_byte_flips` must show that flipping one byte in any source of a persistable row, on either side, changes the key.

### C6 — One cancellation channel

```rust
impl CancellationHandle { pub fn child(&self) -> CancellationHandle; } // is_cancelled() == self || any ancestor
pub enum CancellationSupport { Cooperative, Uninterruptible { reason: &'static str } }
```

- **One handle per dispatch.**
  - Every dispatch passes a child of `Engine::active_solve_cancel` as the `ComputeFn` `cancellation` argument, or a fresh root when none is installed.
  - That child is also the handle stored on `ComputeNodeData.running`.
  - `OptimizedComputeDispatcher::from_engine` snapshots the active handle, and `dispatch` passes a child of it.
  - A persistent hit runs no trampoline and needs no handle.
- **The context carries no cancellation.**
  - The `solver_progress` dispatch context moves to `reify_compute_contract::dispatch_context` as `DispatchContext { progress: Option<Arc<dyn SolverProgressSink>>, extensions: Extensions }`, keeping its RAII install guard. The GUI's `SolverProgressSink` imports follow it.
  - `Extensions` is a typed map keyed by `TypeId`: `insert::<T: Send + Sync + 'static>(Arc<T>)` and `get::<T>() -> Option<&T>`. It lets engine-owned, family-specific state ride the context while the contract names no solver type (D15). elastic-static-solver-performance's `SolvePolicy` (β #8374) and `FactorLedger` (ι #8382) are installed by reify-eval as extensions and read by the elastic adapter as `ctx.extensions.get::<SolvePolicy>()`.
  - `elastic_static`'s ambient reads (`ctx_cancel`, `ambient_cg_cancel_poll`) poll the argument instead.
  - No production code reads a cancel handle from a thread-local, and no context has a cancel slot.
- **Layers.**
  - A cooperative trampoline returns `ComputeOutcome::Cancelled`. `run_compute_dispatch` maps that to `DispatchError::Cancelled` and leaves the output cells Pending. BT7 observes this at the `run_compute_dispatch`/eval layer.
  - Every `Uninterruptible` row states why ignoring the handle is safe.

### C7 — Move protocol (every Layer-3 family move)

A family move is one diff that does all of the following.

1. **Moves the family.**
   - The family's files, its unit tests and its descriptor rows move into a `reify-analysis` family module, with the stdlib cache-key half used only by that family.
   - **reify-analysis module layout:** shared modules ← family modules ← the `PRODUCTION_TARGETS` table. No family imports another family, so the crate lands and stays with no module SCC (INV-ARCH-1; the S3 tool reds a new SCC). A family move never leaves a module imported from both crates except the κ₁ shared foundation.
   - Unit tests that reach any reify-eval item (an `Engine`, or engine-side helpers such as `dynamics_ops::resolve_body_density`) move to `crates/reify-eval/tests/` (D13).
2. **Repoints every importer:** reify-eval tests, reify-eval-fea-tests, gui and the LSP tests.
3. **Updates every path-keyed registry in the same diff:**
   - `crates/reify-audit/ptodo-baseline.txt` and `pdiag-baseline.txt` fingerprints (regenerate pdiag per diagnostic-severity-policy §3(d));
   - the `reify-audit` G-allow orphan tests that pin moved paths;
   - S15's `tests/infra/test_trampoline_decode_ratchet.py` scan roots, so counts follow the code and do not fall to 0;
   - `scripts/module-scc-baseline.json`, lowering the reify-eval row to the measured residual (S3 rule);
   - `engine_hash_algo` coverage rows (C5);
   - `scripts/check-nan-safe-ordering.sh` `SCOPE_PATHSPECS`, which gain `crates/reify-analysis/*.rs`, with the (hK) pins in `tests/infra/test_nan_safe_ordering_guard_wired.sh` and `docs/prds/compute-fea-hardening.md` decision 9 in the same change;
   - `scripts/check-compute-trampoline-registration.sh`'s definition-file list (§13 R4);
   - S13's `tests/infra/test_selector_resolution_ratchet.py` scan roots and ceilings, when a move relocates code it counts (μ);
   - `.config/nextest.toml` partitions and `scripts/release-sensitive-crates.txt`, if a moved test is heavy or release-only;
   - after S24 ο #8822 lands, `plinecite-baseline-gen --rename <old>=<new>`.
4. **Repoints broken checks.** Runs `reify-audit --pattern PDCHECK` and repoints every live task's `delivered_checks` path the move broke, as three writes: `.yaml`, the `.md` twin, and the task's complete `metadata.delivered_checks`. This step applies to **every** S5 leaf that moves, renames or deletes a file or symbol a live check names, not only family moves. Known cases: δ (`solver_progress.rs`: #6731, #8374, #8383), ε (`compute_targets/mod.rs` demand registration: #7142, #8290), κ₁ and π (`compute_targets/mod.rs`: #6773).
5. **Proves no behaviour change.** The regression floor is every tracked `examples/*.ri` and `tests/prd-gate/fixtures/*.ri` that reaches the family, with identical printed results before and after.
6. **Lowers the transitional-edge ratchet.** Lowers the `eval-imports-analysis` row to its new count.

## 5. Boundary-test sketch (H)

| # | Facing | Scenario | Asserts |
|---|---|---|---|
| BT1 | contract | `cargo tree -p reify-compute-contract -e normal,build` | no `reify-solver-elastic`, `reify-kernel-*`, `reify-eval` or `reify-analysis` in the closure (a new `scripts/assert-crate-dag.sh` row; `reify-meshing`, an OCCT- and gmsh-free facade from #8289, is allowed) |
| BT2 | stratum | the same check for both crates | reify-analysis's normal/build closure excludes `reify-eval`, `reify-kernel-occt` and `reify-compiler`, and it has no reify-eval dev-dependency (#4935's INV-1, OCCT-free; reify-analysis absent from `scripts/occt-touching-crates.txt`). From π: reify-eval's normal/build closure excludes `reify-analysis`, `reify-fdm` and `faer` |
| BT3 | registry → consumers | a scratch descriptor with `persistence`, `significance`, `on_completed` and `demand` set, registered in a test engine | cold dispatch writes and warm dispatch hits through the generic bodies; the hook runs once on each path and the topology table is identical after both; the demand override fires; the significance filter consults the row. No reify-eval source edit is needed to add the row |
| BT4 | registry | the CLI engine and the GUI engine | `registered_compute_targets()` is non-empty and covers the injected table plus (until π) the legacy table, with no duplicates. The LSP engine registers none of them |
| BT5 | `.ri` → registry | identity parity (C2) | holds at main-after-η. Each seeded failure fails naming the target: a stdlib `@optimized("solver::elastic_statc")` typo, a `StdlibOptimized` row with no stdlib caller, a `RustOnly` row whose owner has no `#NNNN` |
| BT6 | producer → `.ri` | result-type register (C2) | the representative-dispatch walk finds only declared or registered type_names; a seeded unregistered type_name fails naming it; every register entry's round-trip test exists |
| BT7 | GUI → trampolines | cancellation (C6) | for every `Cooperative` row, a representative dispatch with a pre-cancelled `set_active_solve_cancel` handle yields `DispatchError::Cancelled` with the output cells Pending. Today this holds for elastic_static only; modal free-vibration is the named red case |
| BT8 | shell codec | cold vs warm (ζ) | `shell_extract_persistent_cache_round_trip.rs`: the cold-dispatch Value equals the warm-hit Value field for field (the `regions` divergence pin is rewritten); the topology table is identical; `decode(encode(r)) == r` over a production extraction |
| BT9 | GUI view | `shell_gui_mesh_data(engine.compute_results())` on `fea_shell_flexure.ri` | the same views as today (`shell_gui_mesh_data_returns_one_entry_with_valid_fields`); a non-shell fixture gives an empty Vec; a malformed nested value gives a coded decode error, not a silent skip |
| BT10 | persistence soundness | one-byte flip (C5) | flipping one byte in any persistable row's source, on either side of the crate boundary, changes the composed key; both closure drift guards hold |
| BT11 | regression floor | every tracked example and prd-gate fixture reaching a moved family | identical `reify eval` output before and after each move. Named: `examples/fdm_bracket.ri`, `examples/fea_shell_flexure.ri`, `examples/tensegrity_pavilion.ri` |
| BT12 | `.ri` surface | `tensegrity_load(...)` on a form-found T-prism under load (ι) | `reify eval` prints `converged: true`, one displacement per node and one force delta per member; the builder's key set equals the declared `TensegrityLoadResult` field layout (S15 C4 shape) |

**G6 note.** No leaf asserts a tuned constant. The regression floor is equality against the same-day pre-change run. Cancellation asserts an outcome. The parity and register tests assert set relations. BT12 asserts structure and convergence, not a magnitude.

## 6. Resolved design decisions

**D1 — Position: below reify-eval, Engine-free, injected (§13 R1).**
- The crate's reify dependencies are the ruling's list (contract, solver-elastic, fdm, shell-extract, stdlib) plus `reify-core`, `reify-ir` and `faer`.
- The analysis code's needs are resolved as follows:
  - registration becomes data (C1);
  - `solver_progress` moves into the contract (C6);
  - `nodes_for_faces` becomes `CarriedTopology::nodes_for_faces` (S13 α #8747);
  - μ relocates the pure carried-mode selector resolver that S13 α leaves in reify-eval's `selector_resolve` into `reify-ir`, beside `SelectorValue`;
  - `resolve_selector_faces` (production-dead) stays engine-side, or is deleted (§12 Q5).
- At the end state the crate compiles in parallel with reify-eval, and an analysis edit never rebuilds reify-eval. That is #4935's compile-unit premise, strengthened.
- Rejected: depending on reify-eval through `fn(&mut Engine)`.

**D2 — Name `reify-analysis` (§13 R2).**
- It names the stratum (heuristic 1: informative names).
- `reify-eval-fea-tests` keeps its name and role.
- κ₁ updates the contract crate's doc mention of `reify-eval-fea`.

**D3 — The descriptor lands in place before any move (ε).** The engine stops naming targets first. Every family move is then mechanical: the rows travel with their files.

**D4 — `ElasticResult` moves with the FEA family (ν).**
- Until `elastic_static` leaves reify-eval, `ElasticResult`'s users are on the reify-eval side. reify-analysis is the right owner: persisting a result is an analysis concern, and the record carries post-processed channels the solver does not produce.
- β cuts the contract's other solver edge early, and BT1 lands in ν.

**D5 — The structured detail is contract-owned and typed.**
- The type is `StructuredComputeDetail::Fea(FeaDetail)`. It mirrors the three `FeaDiagnosticDetail` variants with contract-owned field types: `RigidBodyMode` for the solver's `DofDirection`, and `ElementId` as a `u32` newtype.
- The `From<FeaDiagnosticDetail>` conversion lives at the producer (`fea_diagnostics`).
- The persisted mirror and its on-disk order are unchanged.
- Rejected: a boxed trait object, which loses `Eq`, the typed GUI mapping and the persisted mirror's exhaustiveness.

**D6 — One cancellation channel through a linked child handle (C6).**
- Passing the engine's handle itself would let a per-node cancel cancel every solve.
- Progress stays ambient, relocated to the contract. Making it stateless would change all 19 trampoline signatures for a channel with one producer and one consumer, and is recorded in §11.

**D7 — Shell projection: project only what decode rebuilds (§13 R6).**
- The projected Value keeps `mid_surface`, `segmentation.{vertex_labels, triangle_labels}` and `naming.{face_records, edges}`.
- It drops `segmentation.regions`, the Debug-string `diagnostics` and the always-zero `solve_time_ms`.
- Extraction diagnostics travel on `ComputeOutcome`'s coded channel, which the persistent envelope replays.
- Cold == warm by construction.

**D8 — Wire primitives live in `reify_core::persistent_cache` with no ordering edge (§13 R7).**
- The ruled sequencing rested on the premise that sharing would give shell-extract a contract edge, and here it does not.
- The u32, [u32;3], [f64;3] and [i32;3] codecs and `bincode_options` stay in `result.rs`, as ruled.
- The shared `check_len(field, len, max)` replaces `check_f64_vec_len`, which is hard-coded to "ElasticResult", and `result.rs`'s copy.

**D9 — Engine-internal result types are registered, not declared (§13 R5; C2).**

**D10 — `tensegrity_load` gets a declared `.ri` surface (§13 R4b; ι).**
- The declarations:
  - `structure def TensegrityLoadResult { displacements : List<Vector3<Length>>; member_forces : List<Force>; member_force_deltas : List<Force>; slack : List<Bool>; converged : Bool }`;
  - `@optimized("solver::tensegrity_load") pub fn tensegrity_load(structure : Tensegrity, prestress : List<Force>, youngs_modulus : Pressure, area : Area, loads : List<Vector3<Force>>, supports : List<Int>) -> TensegrityLoadResult`.
- These spell the trampoline's existing input contract and `build_result`'s key set.
- The grammar-gate fixture `tests/prd-gate/fixtures/s5_tensegrity_load_surface.ri` parses (`tree-sitter parse --quiet`, exit 0) and type-checks (`reify check`, exit 0) at main.

**D11 — The registration lint follows the moved symbols (§13 R4).**
- S5 edits only `check-compute-trampoline-registration.sh`'s `EXEMPT_DEFINITION_FILES` and its negative-pass half names, in the diffs that move or delete them.
- `KNOWN_SITES` and the posture rewrite stay S1's.

**D12 — Re-export paths: one inside reify-eval.**
- Inside reify-eval, contract types come from `reify_compute_contract` (the S3 β #8687 precedent).
- The `engine_compute` and `graph` shims are deleted.
- The crate-root re-export stays reify-eval's public surface for front ends, which the finding's proposal allows.

**D13 — Dev-dependency topology (critic F5).**
- `reify-analysis` has **no** dev-dependency on `reify-eval`, which keeps it out of the OCCT gate.
- Its own tests are the moved pure unit tests.
- Every engine- or compiler-driven test lives in `crates/reify-eval/tests/` (or `reify-eval-fea-tests`): BT3–BT7, the representative-dispatch table, and the two moved unit modules that construct an `Engine` (`as_printed_material::iota_tests`, `compute_targets::mod::tests`).
- `reify-eval` reaches reify-analysis through the transitional normal edge until π, and through a dev-dependency after. `reify-eval-fea-tests` and `reify-lsp` dev-depend on it.

**D14 — The transitional edge (§13 R8).**
- κ₁ moves the shared foundation into reify-analysis and gives reify-eval a temporary normal dependency on it, so the family moves can proceed one at a time in any order.
- The `eval-imports-analysis` ratchet row counts `reify_analysis::` paths in reify-eval production. It only falls, and π takes it to 0 with the edge.
- Rejected:
  - the critic's alternative of homing the shared helpers and S15's decoders in the contract, against the ruling that the contract keeps only contract types;
  - a single ~45k-line move leaf.

**D15 — Family state rides the contract's dispatch context as typed extensions (fold-verifier OTHER-1).**
- elastic-static-solver-performance installs engine-owned FEA state (`SolvePolicy`, `FactorLedger`) in the dispatch context. A context in reify-eval is unreachable from reify-analysis, and a context in the contract that names those types re-adds the solver edge that BT1 forbids.
- The contract therefore owns a solver-neutral `DispatchContext` with a `TypeId`-keyed `Extensions` map. reify-eval, which already depends on reify-solver-elastic, installs the state, and the analysis adapter reads it by type.
- This is typed access with no strings and no ad-hoc parser (h12).
- Rejected:
  - passing the state through the `ComputeFn` signature, which would change all 19 trampolines for one family;
  - homing the context in reify-analysis, which would leave reify-eval naming reify-analysis at the end state.

## 7. Disposition table

| Finding | Disposition | Leaf |
|---|---|---|
| fk-a0d2e755b215 wire codec copies (weakened, narrowed as ruled) | f64 slab, length cap and severity pair in reify-core; copies deleted; ratchet row | α |
| fk-8cd376f72fca contract above the solver | contract-owned detail (β); `ElasticResult` and its codec move out with the FEA family; BT1 | β, closed by ν |
| fk-e6650962065a shim and untested contract | shims deleted; 30 paths repointed; handle tests moved | γ |
| fk-d870b2b3bfdf duplicated catch_unwind guard (**#8525**) | adopted as filed; gains its ratchet row | #8525 |
| fk-cf610e43d544 two cancellation channels (**high**) | linked child handle as the one channel; ambient read deleted; BT7 | δ |
| fk-34bddf1fcbce string-keyed per-target capabilities | descriptor registry; 11 routing sites to 0 | ε |
| fk-7d4f2be7b634 persistable allowlist and 6 identical arms | generic codec bodies; derived persistable set | ε |
| fk-952285bb817b three shell decoders, lossy round trip | one typed codec; cold == warm | ζ |
| fk-750e0c9f3bd6 GUI parser duplicate, stale G-allow | GUI view over the codec; parser and G-allow deleted | ζ |
| fk-250d45d9d1f4 identity and schema spelled independently | `RiSurface` parity, result-type register, LSP list derived (η); tensegrity_load surface (ι) | η (with ι) |
| fk-6e78ea52b691 no stratum for compute targets | `reify-analysis`, injected; shared foundation; six family moves; legacy table and transitional edge retired | κ₁ … ο, closed by π |

## 8. Pre-conditions (G3, verified on main 2026-10-13)

**Present at main.** No grammar change is needed, so `grammar_confirmed: true` holds on every leaf.
- `reify_core::PersistentlyCacheable`.
- `reify_ir::{TopologyAttributeTable, TopologyAttribute, SelectorValue}`.
- `CompiledFunction.optimized_target`.
- `Engine::{set_active_solve_cancel, set_solver_progress_sink}`.
- `MorphRegistration`.
- `compose_engine_version_hash`.
- `scripts/assert-crate-dag.sh` B1–B6, run by `crates/reify-build-utils/tests/crate_dag_assertion.rs`.
- `scripts/assert-module-scc.py` and its baseline (S3 α #8673, done).
- `DiagnosticCode`, which is `#[non_exhaustive]`.
- The grammar for D10 (fixture above).

**Hard upstream.**
- **S15** (`trampoline-decode-and-result-build.md`). Its decoders, builders and `into_failed` move with the targets.
  - ε and ι depend on ε #8690 (the `ElasticResult` builder pair the elastic codec wraps).
  - ε and ζ depend on λ #8706 (shell_extract_compute).
  - ε, ζ and ι depend on δ #8686 (`registry_free`).
  - κ₁ depends on every S15 code leaf that edits reify-eval's analysis files (α through λ, and μ for `shell_solve`). This is the program's within-wave order S15 → S5, so no S15 leaf has to chase a moved path, and the family moves inherit the S15 ordering through κ₁.
  - The S15 chain waits on **#6922** and **#6941** (dimension-checked-readers).
- **S4 η #8864** before ε (program cross-reference: the per-target shell-extract hook sits on the one `CellHooks` site η leaves; η deletes the duplicated `elastic_static` branch).
- **S3 β #8687** and **S7 #8551** before γ.
- **S13 α #8747** before κ₁ (it removes `topology_selectors`' `CarriedTopology` import and moves `nodes_for_faces`), and so before μ and ν.
- **S14:** δ #8619 before μ and ν; η #8628 and ζ #8626 before ξ; β #8616 before ν.
- **S6:** κ #8837 before μ (it rewrites `modal_ops`'s record-key reads); τ #8848, φ #8850 and ψ₃ #8854 before λ (they partition `trajectory/trampoline.rs` down to the cache keys and code `trajectory_ops`' TOTS outcome).
- **#5802** before ν.
- **#8525** before ε.
- **S15 π #8720** before ρ (same file).

**Not upstream; file contention or adapt-second (§9).**
- **meshing-service ε #8288** deletes the boundary demand and `build_face_anchors`, and edits `compute_targets/mod.rs`, `bc_resolve.rs`, `elastic_static.rs` and `topology_selectors.rs`.
- **meshing-service η #8290** adds the per-target options extractor (C1).
- **meshing-service ζ #8289** adds the `reify-meshing` facade and `RealizationReadHandle::remesh()` to the contract. BT1 allows the facade.
- **elastic-static-solver-performance** β #8374 (`SolvePolicy` in the dispatch context; the `compute_targets/static_linear_solve.rs` adapter), ι #8382 (`FactorLedger` in the context), κ #8383 (a `phase` field on `SolverProgressUpdate`) and ξ #8375 (`modal_ops`). These are adapt-second against δ through C6's `Extensions`, and ν moves the adapter file if it has landed. Decompose appends a note to each.
- **flexible-assembly-modal A1 #7142** registers a modal VolumeMesh demand (its check: `register_volume_mesh_demand("modal…` in `compute_targets/mod.rs`). This is adapt-second against ε: after ε it is the modal row's `demand`, and ε repoints #7142's check (three writes) if #7142 is still live. Decompose appends a note.
- **S8 λ #8909** edits `multi_case.rs` prose and tests.
- **S8 ι #8907** edits `Value::Field` destructures in `compute_targets/{mod, elastic_static, as_printed_material, as_printed_material_r0}`. It enumerates its sites by `git grep -l FieldSourceKind`, so a move does not strand it.
- Each move leaf's C7 step 4 repoints a live task's broken check.

## 9. Cross-PRD relationship (G4)

| Artefact / PRD | Direction | Mechanism | Owner |
|---|---|---|---|
| `reify-compute-contract` Cargo dependency on `reify-solver-elastic` (program G4) | this PRD **owns** | β, ν; BT1 | **S5** |
| `docs/prds/v0_3/compute-node-contract.md` (registration contract; program G4) | this PRD **owns** | ρ amends §2 (cancellation) and §4 (descriptor registry), and freezes §8 Phases 7–8 as superseded by C3/C4 | **S5** |
| `scripts/check-compute-trampoline-registration.sh` `KNOWN_SITES` (program G4: S1) | **consumes**, with a carve-out | D11 | **S1** (§13 R4) |
| S1 Engine constructor and Registration posture | S1 **consumes** κ₁ | `register_production_compute_fns(ComputeTargets, MorphRegistration)` becomes a field of S1's posture | S1 |
| S15 decoders, builders and C6 ratchet member | S5 **consumes** | κ₁ moves `stdlib_inputs`; family moves carry the builders; C7 step 3 repoints the ratchet's scan roots; ι reuses C4's parity shape | S15 |
| S16 trampoline factoring | S16 **consumes** | the moved `elastic_static` and `modal_ops`; the elastic row's upstream planner is S16's route classification; S16 splits the `sampled_*` helpers that κ₁ moves | S16 |
| S13 selector resolution | **ordered after** S13 α | μ relocates the pure carried-mode resolver to reify-ir without changing its body | S13 |
| S4 cell-eval closure (`engine_eval`) | **ordered after** S4 η #8864 | ε's generic `insert_upstream` is called from η's one `CellHooks` site | S4 |
| S3 INV-ARCH-1 baseline | **consumes** the tool | every move lowers the reify-eval row in its own diff; reify-analysis lands with no SCC row | S3 tool; S5 rows |
| S14 `ShellFrame` doc pointer in `elastic_result.rs` | S14 β #8616 edits first | ν moves the file after it | S14 β, then S5 |
| meshing-service #8288 / #8290 | adapt-second | C1 `RealizationDemand` / `volume_mesh_options` descriptor field; a decompose note goes on both tasks | meshing-service (mechanism), S5 (descriptor) |
| meshing-service #8289 | file contention | BT1 allows `reify-meshing` | meshing-service |
| elastic-static-solver-performance #8374 / #8382 / #8383 / #8375 | adapt-second | C6 `DispatchContext.extensions` carries `SolvePolicy` and `FactorLedger`; the contract names no solver type; ν moves `static_linear_solve.rs` | that PRD (mechanism), S5 (context home) |
| flexible-assembly-modal #7142 | adapt-second | the modal row's `demand` replaces a `register_volume_mesh_demand` call | #7142 (mechanism), S5 (descriptor) |
| S8 λ #8909, ι #8907 | file contention | serialised by lock; both locate sites by grep | S8 |
| S6 κ #8837, τ #8848, φ #8850, ψ₃ #8854 | **ordered after** | μ after κ (modal record keys); λ after τ, φ, ψ₃ (trajectory partition and TOTS codes) | S6 |
| PDROP #7079 (`param_contract` declarations) | no seam | the honored-param declaration stays at S15's decoder; the descriptor carries no parameter facts | PDROP |
| PDROP decision #7087 (`target_fidelity`) | **defers to** | the R0 row is `RustOnly { owner: "#7087: …" }` | #7087 |
| #7097 (`ModalResult.topology`) | **defers to** | `CarriedTopology` register entry | #7097 |
| merge-gate-compile-cost #4935 / #4936 | **subsumes** #4935; #4936 **consumes** π | §13 R3 | S5 |

No new contested-ownership pair is introduced (`phase-3-breadcrumb-map.md` §3).

## 10. Decomposition plan

**New gate-resident tests.** Every leaf adds tests to existing test binaries, with three exceptions.
- **α** creates `tests/infra/test_analysis_stratum_ratchet.py` with its `.sh` wrapper and its `run-all-classification.manifest` row. Every later ratchet-adding leaf depends on α.
- **κ₁** creates the new crate's lib-test binary. Its diff carries that binary's drift-guard registrations (nextest partitions and the release-sensitive list, where applicable). It extends the existing `test_engine_hash_closure.sh` in place rather than adding a member.
- No member asserts a wall-clock bound.

**Trigger-chain gate.** δ is the only high leaf, carrying fk-cf610e43d544. Decompose adds it as a dependency of the human gate #8612 (program FILED convention).

**Ratchet rows (INV candidate 5).**

| row | pattern counted | home |
|---|---|---|
| `wire-codec-copy` | `fn read_f64_slab` / `fn write_f64_slab` / `fn severity_to_u8` / `fn severity_from_u8` / `const MAX_F64_ELEMENTS` definitions outside `reify-core` | α → 0 |
| `catch-unwind-guard` | `catch_unwind(` sites over a `ComputeFn` in reify-eval production outside `call_compute_fn_guarded` | #8525 → 0 |
| `target-string-routing` | quoted `"<family>::<name>"` target literals in reify-eval production outside the legacy descriptor table | ε → 0 |
| `shell-value-decoder` | functions outside the shell codec module that read a `"ShellExtractionResult"` / `"MidSurfaceMesh"` / `"SegmentationResult"` Value by field name | ζ → 0 |
| `eval-imports-analysis` | `reify_analysis::` paths in reify-eval production | κ₁ sets the baseline; each move lowers it; π → 0 |

| Label | Task | Modules | Observable signal | Prereqs | Keys · INV |
|---|---|---|---|---|---|
| α #8984 | Shared wire primitives in `reify_core::persistent_cache`: `write_f64_slab` / `read_f64_slab` (one SAFETY argument, one BE path, its pinned `decode_f64_slab_from_le_bytes` test), `MAX_F64_ELEMENTS`, `check_len(field, len, max)`, `severity_to_u8` / `severity_from_u8`. Adopted by `reify-compute-contract` `elastic_result.rs`, reify-eval `BucklingResultCache` and `diagnostics_wire`, and `reify-shell-extract` `result.rs`; the copies are deleted. Creates the ratchet member, its wrapper and its manifest row | reify-core, reify-compute-contract, reify-eval, reify-shell-extract, tests/infra | every persisted round-trip test is unchanged (ElasticResult, BucklingResultCache, ShellExtractionResult); `wire-codec-copy` = 0; result.rs production `unsafe` blocks fall from 10 to 8 | — | fk-a0d2e755b215 · INV cand 5 |
| β #8986 | Contract-owned `StructuredComputeDetail::Fea(FeaDetail)` (D5); `From<FeaDiagnosticDetail>` at the producer; `diagnostics_wire` and gui `fea_diagnostics_from_structured` map from it; `dispatch.rs` names no solver type | reify-compute-contract, reify-eval, gui/src-tauri | `persistent_round_trip_replays_fea_under_constrained_unanchored` and the gui FEA diagnostics tests pass unchanged; the under-constrained FEA panel lists the same rigid-body modes | — | fk-8cd376f72fca (part; closed by ν) |
| γ #8991 | Deletes the `engine_compute` and `graph` contract re-export shims; repoints the 30 paths to `reify_compute_contract`; moves the six `RealizationReadHandle` tests into the contract crate; trims `compute_contract_reexport_compat.rs` to the crate-root surface | reify-eval, reify-compute-contract | `cargo test -p reify-compute-contract` runs the six handle tests (0 today); `engine_compute.rs` has no `pub use reify_compute_contract` | **#8687**, **#8551**, α | fk-e6650962065a |
| #8525 | (adopted) `call_compute_fn_guarded`, shared by `invoke_compute_trampoline` and `OptimizedComputeDispatcher::dispatch`; adds the `catch-unwind-guard` row | reify-eval | as filed; row = 0 | α | fk-d870b2b3bfdf · INV-FEA-2 |
| δ #8988 | One cancellation channel (C6). `CancellationHandle::child`. Each dispatch's `cancellation` argument and `ComputeNodeData.running` are a child of `active_solve_cancel`; the dispatcher snapshots it. `solver_progress` moves to `reify_compute_contract::dispatch_context` (progress sink plus `Extensions`; any `SolvePolicy`/`FactorLedger` slot already landed by #8374/#8382 becomes an extension). `elastic_static` polls the argument. BT7 runs over today's polling targets in `crates/reify-eval/tests/` (a test-side table, derived from the descriptor by ε). PDCHECK repoint (C7 step 4) for #6731, #8374, #8383 | reify-compute-contract, reify-eval, gui/src-tauri | BT7: a pre-cancelled GUI handle cancels modal free-vibration, transient, inverse dynamics, trajectory simulate and shell-extract (red today) and elastic_static (green today); no thread-local cancel read remains | — | fk-cf610e43d544 (**high**) |
| ε #8994 | Descriptor registry in place (C1, C3, C4). The contract `target` module. `ComputeDispatchRegistry` holds rows, with all 19 in reify-eval's legacy table. Generic persist bodies and a derived persistable set; significance rows; `demand` (C1's adapt-second rule with #8288/#8290); one completion path; generic `insert_upstream` from S4 η's one `CellHooks` site; `registered_compute_targets()`; the dispatcher snapshot holds rows. BT7's table derives from `cancellation`. Registration-lint half names repointed (D11). Adds the `target-string-routing` row. #7142's and #8290's demand registrations become row fields (C1 adapt-second), with check repoints per C7 step 4 | reify-compute-contract, reify-eval, scripts | BT3. The 11 routing sites → 0. `PERSISTABLE_TARGETS`, `is_opted_in` and the `elastic_static` upstream branch are absent. `reify eval examples/fea_shell_flexure.ri` twice on one `REIFY_CACHE_DIR` gives the same output with a warm hit (the existing CLI cache tests) | α, #8525, δ, **#8864**, **#8690**, **#8706**, **#8686** | fk-34bddf1fcbce, fk-7d4f2be7b634 |
| ζ #8996 | One shell projection codec (D7): field-name constants, `encode`, `decode -> Result<_, ShellDecodeError>`. The persistence codec, the completion hook and the GUI view all use it. `shell_gui_mesh_data` becomes a free fn over a generic `Engine::compute_results()` iterator, with `ShellGuiMeshData` beside it. `engine_admin`'s classifier, parser and stale G-allow are deleted, and so is the reify-audit G-allow orphan entry. Adds the `shell-value-decoder` row | reify-eval, gui/src-tauri, crates/reify-audit tests | BT8, BT9; the GUI shell overlay is unchanged on `fea_shell_flexure.ri` | ε, **#8706**, **#8686** | fk-952285bb817b, fk-750e0c9f3bd6 |
| ι #8990 | `tensegrity_load` `.ri` surface (D10): `structure def TensegrityLoadResult` and `@optimized("solver::tensegrity_load") pub fn tensegrity_load` in `tensegrity.ri`; one builder with S15 C4 parity. Docs-truth: a new `## Tensegrity analysis` section in `crates/reify-mcp/src/tools/chunks/stdlib.md` (plus its Module Tree line) documents `form_find`, `form_find_free`, `membrane_load` and `tensegrity_load` in intent terms ("load a strut/cable tensegrity", "load a pavilion with membranes"). Adds a committed `tests/prd-gate/fixtures/` eval fixture | reify-compiler stdlib, reify-eval, reify-mcp chunks | BT12: `reify eval` on the fixture prints `converged: true` with per-node displacements; each chunk signature compiles in a smoke `.ri` | **#8686**, **#8690** | fk-250d45d9d1f4 (part) · INV-PD-2 |
| η #8997 | Identity and schema (C2): `RiSurface` on every row; the identity parity test; `ENGINE_INTERNAL_RESULT_TYPES` and the representative-dispatch walk (whose table covers every row and is shared with BT7); the reify-lsp list deleted, with the posture lock built on a throwaway production engine. All tests read descriptors through `registered_compute_targets()` and live in `crates/reify-eval/tests/` | reify-compute-contract, reify-eval (src rows and tests), reify-lsp tests | BT5, BT6; `const ALL_PRODUCTION_COMPUTE_TARGETS` is absent | ε, ζ, ι | fk-250d45d9d1f4 · INV cand 4 (S5's half) |
| κ₁ #8999 | Creates `reify-analysis` (D1, D2, D13, D14): a workspace member with the module layout of C7 step 1. Moves the shared foundation: the cross-family `compute_targets/mod.rs` value helpers, S15's `stdlib_inputs` (with `into_failed`) and `result_topology`. Adds the transitional reify-eval normal edge and the `eval-imports-analysis` row; `PRODUCTION_TARGETS` with injection into `register_production_compute_fns` and `run_tests` (cli, gui); C5 (algorithm to `reify-build-utils`, composed key, coverage row, `test_engine_hash_closure.sh` extended in place); the reify-analysis half of BT2; every test's registration call repointed to the bundler with the injected table. Updates the contract doc's `reify-eval-fea` mention. C7 steps 3–6 apply to the shared modules it moves (S15's ratchet scan roots follow `stdlib_inputs`) | new crate, reify-eval, reify-cli, gui/src-tauri, reify-lsp tests, reify-eval-fea-tests, reify-compute-contract, reify-build-utils, scripts, tests | BT2 (reify-analysis half); BT10; BT11 on the FEA and modal examples (they now run through moved shared modules); `reify test` still registers every target (the test_runner meaningful-pass guard) | ε, η, **every S15 code leaf that edits reify-eval's analysis files** (#8674 α, #8679 β, #8683 γ, #8686 δ, #8690 ε, #8693 ζ, #8697 η, #8698 θ, #8702 ι, #8706 λ, #8711 μ; program order S15 → S5), **#8747** | fk-6e78ea52b691 (part) · #4935's INV-1 |
| κ₂ #9001 | Moves the FDM family (C7): `as_printed_material`, `as_printed_material_r0`, `fdm_slice`; the R0 row is `RustOnly { owner: "#7087: …" }` | reify-analysis, reify-eval | BT11 on `examples/fdm_bracket.ri`; `eval-imports-analysis` unchanged or lower | κ₁ | fk-6e78ea52b691 (part) |
| λ #9002 | Moves dynamics/trajectory (C7): `dynamics_ops`'s trampoline half, `trajectory_ops`, `InverseDynamicsCacheKey` + `body_solid_hashes`, and the two trajectory cache keys. The mass-props half stays in reify-eval as `body_mass_props.rs`. `dynamics/trampoline.rs` and `trajectory/trampoline.rs` are deleted (after S6 ψ₃ #8854 the latter holds only the cache keys) | reify-analysis, reify-eval, reify-stdlib | BT11 on the dynamics and trajectory examples; no reify-stdlib file is named or documented as a trampoline half | κ₁, **#8848**, **#8850**, **#8854** | fk-6e78ea52b691 (part) |
| μ #9003 | Moves modal (C7): `modal_ops`, `ModalCacheKey` / `TransientCacheKey` and `free_vibration.rs`; `transient.rs` stays. Relocates the carried-mode selector resolver to reify-ir (D1) | reify-analysis, reify-eval, reify-stdlib, reify-ir | BT11 on the modal examples; the BT7 modal rows stay green | κ₁, δ, **#8619**, **#8837** | fk-6e78ea52b691 (part) |
| ν #9004 | Moves the FEA core (C7): `elastic_static`, `buckling`, `buckling_multi_case`, `multi_case`, `shell_solve`, `bc_resolve::boundary_node_set`, `fea_diagnostics` and, if #8374 has landed, its `static_linear_solve.rs` adapter. `bc_resolve`'s engine-side half (`build_face_anchors`, `resolve_selector_faces`) is re-homed to a reify-eval realization-edge module, unless #8288 has deleted it. Moves `ElasticResult` / `ShellChannels` / `AposterioriEstimate` / `max_deflection_magnitude` and `From<PartialElasticResult>` out of the contract, and `BucklingResultCache` out of reify-eval. The contract drops `reify-solver-elastic` (D4), and BT1 lands | reify-analysis, reify-compute-contract, reify-eval | BT1; BT10 for elastic and buckling; BT11 on the FEA examples and `fea_cold_start_heuristic_e2e`; `cargo tree -p reify-compute-contract` lists no solver | κ₁, β, δ, **#5802**, **#8619**, **#8616** | fk-8cd376f72fca |
| ξ #9006 | Moves tensegrity/form-find (C7): `form_find`, `membrane_load`, `tensegrity_load`, `tensegrity_crack` | reify-analysis, reify-eval | BT11 on `examples/tensegrity_pavilion.ri` and the form-find examples; BT12 still green | κ₁, ι, **#8628**, **#8626** | fk-6e78ea52b691 (part) |
| ο #9007 | Moves shell-extract (C7): `shell_extract_compute`, with ζ's codec, the GUI view fn and `ENGINE_INTERNAL_RESULT_TYPES`. gui calls `reify_analysis::shell_gui_mesh_data`. reify-eval drops `rustc-hash` | reify-analysis, reify-eval, gui/src-tauri | BT8, BT9 after the move; BT11 on `fea_shell_flexure.ri` | κ₁, ζ, η | fk-6e78ea52b691 (part) |
| π #9008 | Retires the legacy table and the transitional edge. reify-eval's `compute_targets` module is deleted (`MorphRegistration` is re-homed beside the bundler). reify-eval's normal dependency on reify-analysis becomes a dev-dependency (D13), and reify-eval drops `reify-fdm`, `faer` and the unused `bytemuck`. The reify-eval half of BT2 lands. The INV-ARCH-1 row is lowered to the measured residual. #4936 is re-pointed here | reify-eval, scripts | BT2 complete; `eval-imports-analysis` = 0; `crates/reify-eval/src/compute_targets` is absent; `cargo tree -p reify-eval -e normal` has no `reify-analysis`, `reify-fdm` or `faer` | γ, η, κ₂, λ, μ, ν, ξ, ο | fk-6e78ea52b691 |
| ρ #9009 | Record. `compute-node-contract.md` §2/§4 are amended to C1/C6, and Phases 7–8 are frozen as superseded by C3/C4 (a section-level freeze naming the leaf ids). `design-invariants.md` INV-PD-2's enforcement text names η's parity and register tests beside S15's C4, as a pointer only. The `/prd` overlay's field-population and wired-on-main evidence paths move from `crates/reify-eval/src/compute_targets` to `crates/reify-analysis` | docs, .claude/skills/prd/project.md | the committed rows name the test symbols; the overlay's evidence paths resolve | η, π, **#8720** | INV cand 4 |
| σ #9010 | PRD close: the terminal stamp | this PRD + manifest | a committed `SHIPPED` header with the landed leaf ids, the AS-AUTHORED freeze and the LIVE/AS-AUTHORED map | all above | — |

**Docs-truth gate.**
- ι is the only language-surface leaf. It carries item 1 (the chunk section) and item 4 (intent-level findability) in its own diff.
- Items 2 and 3 are **not applicable**. `tensegrity_load` follows the existing `membrane_load` idiom exactly, so it introduces no new authoring idiom, and there is no corpus file for an index line to point at.

**G7 walk (advisory at author time).**
- INV-SF-6: the upstream fallback Warning, the completion-hook error and the shell decode error are coded.
- INV-SF-5: every `RustOnly` and register exemption names a live owner.
- INV-SF-2: no Error beside a plausible result.
- INV-PD-2 is strengthened (C2).
- INV-ARCH-1: reify-analysis lands with no SCC, and every move lowers its row.
- No waiver is needed.

## 11. Out of scope

- **Honoring currently-ignored params.** PDROP owns them; S15 owns the decoders.
- **The trampoline split into typed request and route enum, and the `modal_ops` / `elastic_static` h14 partitions.** That is S16.
- **The Engine constructor and Registration posture.** That is S1.
- **Stateless progress reporting** (a `ComputeContext` argument). A follow-up candidate, not filed.
- **`target_fidelity` / the R0 rung.** That is #7087.
- **The `ModalResult.topology` declaration.** That is #7097.
- **OCCT-skip scope wiring and the relink measurement.** That is #4936.

## 12. Open questions (tactical)

1. **Representative-dispatch table home.**
   - *Suggested:* `crates/reify-eval/tests/support/`.
   - Constraint (D13): it is never in reify-analysis.
   - Decide in δ, which creates the first rows.
2. **`ComputeTargets` producer hash type.**
   - *Suggested:* whatever `ENGINE_VERSION_HASH` uses today.
   - Decide in κ₁.
3. **Test registration form.**
   - *Suggested:* call the bundler directly, so a test reads like production.
   - Decide in κ₁.
4. **`selector_resolve` relocation shape.**
   - If S13 α lands the carried-mode resolver with an engine type in its signature, μ relocates only the pure core and keeps a thin engine-side wrapper.
   - Decide in μ.
5. **`resolve_selector_faces`.**
   - It is production-dead.
   - *Suggested:* delete it with its tests (h6) if #8288 has not already.
   - Decide in ν.
6. **Which mod.rs helpers are shared.**
   - The helpers κ₁ moves are those used by more than one family at κ₁'s dispatch.
   - A family-local helper moves with its family.
   - κ₁ measures the split.

## 13. Rulings (Leo, 2026-10-13, this session; program §Resolved decisions binds everything else)

1. **R1 — Position: below reify-eval** (D1). It is Engine-free, and reify-eval never depends on it at the end state. Front ends inject the static descriptor table.
2. **R2 — Name: `reify-analysis`** (D2).
3. **R3 — #4935 is subsumed.**
   - #4935 was cancelled at decompose (2026-10-14) with a reason naming κ₁ #8999 and the family moves. It had been parked `deferred` on 2026-10-13 so it could not dispatch meanwhile.
   - Its OCCT-free invariant is carried in BT2.
   - **#4936** is re-pointed from #4935 to π #9008, and a note in its details updates the crate name.
4. **R4 — KNOWN_SITES carve-out** (D11).
5. **R4b — `tensegrity_load`: declare its `.ri` surface** (D10, ι), not delete the target.
6. **R5 — Engine-internal result types: one register with round-trip-tested codecs** (C2, D9), not stdlib declarations. `CarriedTopology` is owned by #7097.
7. **R6 — Shell projection: project only what decode rebuilds** (D7).
8. **R7 — Wire primitives in `reify_core::persistent_cache`, no ordering edge** (D8).
9. **R8 — Shared modules during the migration: a transitional reify-eval → reify-analysis normal edge** (D14).
   - κ₁ moves the shared foundation first.
   - The `eval-imports-analysis` row only falls.
   - π deletes the edge, restoring R1's end state.
