# PRD — Realization pipeline (reify-eval): one per-template walk, a typed executor

**Status:** decomposed 2026-10-15 (pass 2): α #4727 (adopted), β₁ #9154, β₂ #9155, γ #9156, δ #9158, ε #9157, ζ #8534 (adopted), η #9159, θ #9160, ι₁ #9161, ι₂ #9162, ι₃ #9163, ω #9164; out-of-plan follow-up #9165 (R6). Subsumed and cancelled: #4691 (→ β₁), #8333 (→ η). Out-of-batch edges: #9036 ← α; #9092 ← δ; gate #8612 ← δ, η. R5 widenings (three writes each): #6745 #6923 #6171 #6168 #6177 #6170 #6175 #8395 #8396 #8400 #6610 #6926. Authored pass 1, 2026-10-15 — review-all program `review-all-reify-20261010`, stream **S2** (wave 3, spawn mode, size L, risk high; `plans/review-all-program-reify-20261012.md`). Leo ruled R1–R6 in this session (§11). An adversarial critic pass (fable/high: 1 blocker, 4 majors, 6 minors, each reproduced in code) is folded, and an opus/high fold-verifier's second pass (7 majors, 10 minors) is folded on top, and a focused opus re-verify of §1, §5.5 and §8 is folded after that (§11 fold records). v0_6. Approach **B + H** (§5 contract, §6 two-way boundary tests; rationale D14).
**Invariants touched:** **INV-ARCH-1** (`module-sccs-only-shrink`): γ takes `geometry_ops` out of the 24-module reify-eval SCC and lowers its baseline row in the same diff; every new module this PRD adds imports nothing in the residual engine SCC. **INV-BUILD-3** (cache-hit parity): δ moves `probe_realization_cache` into the executor and re-points the row's pointer (S24 grammar; §5.10). No registry status cell is flipped by this stream. Every leaf that extracts a construct names its row in `tests/infra/adoption-ratchet.manifest` (program invariant candidate 5, §5.9).
**Finding keys closed (5):** fk-f7e4716af878 (η), fk-53997e96ef7c (δ), fk-17e2aa81f6d6 (θ), fk-98754d80a67e (α, the adopted #4727), fk-9e6d548589d8 (ζ, the adopted #8534). Leaves that only contribute to a key carry it as `x_finding_part` (§9).
**Code anchors** verified against main `d4cd598fee` (2026-10-14) by six scout seats and re-checked by the lead. Since the review anchor `a030ee7528`, engine_build.rs changed by 474 lines (#6308's owner routing among them) and execute_realization_ops shrank from 2,317 to 2,304 lines; no finding's substance changed. Cite-by-symbol; re-locate lines at implementation time. Line numbers appear only in the dated §10.

---

## 1. Consumer and user-observable surface (G1, G2)

**Consumers.**
- **Every surface that realizes geometry.** `reify build` / `check` / `eval` (through `Engine::build`, `realize_for_check`, `build_outputs*`), the CLI combined arm (`realize_for_check → tessellate_realizations → check`), the GUI (`EngineSession` → `tessellate_snapshot` on load, edit and preview; `build` on export), the relate pre-pass's nested build (`relate_solve`), and the test-only public entries `build_snapshot` and `distance_between_placed` (§2.1).
- **Downstream program streams and PRDs.** S16 (mesh-morph observability) consumes the executor's morph input instead of adding a `with_*` knob. instantiation-value-flow consumes γ's override-realizer seam when it generalises `realize_sub_override_handles`. S12's `capture_measurement` consumes ε's realized-terminal record. S23 skips the engine_build prose this stream rewrites.
- **The merge gate.** The adoption-ratchet rows (§5.9), `module_scc_ratchet` (INV-ARCH-1), the PTODO fingerprint ratchet (α resolves every `#4727` citation) and pdiag.

**In-engine seam catalogue (overlay G1).** No new seam. The walk is the §3.2 realization-kind dispatch seam's driver; the executor is the §3.1 op-execute seam; multi-kernel selection (§3.3) stays S10's `dispatcher::select`.

**What users see.** Everything else is behaviour-preserving, observed through the oracles in §6.
1. **The GUI records what tessellation produced (θ).** After a GUI load or slider edit, which runs `tessellate_snapshot` only, a voxel pipeline's operand node reads `Voxel` and its isosurface `Mesh`, with `produced_kernel` OpenVDB. Today both stay at the construction-time `BRep` default (#3437's open obligation, comment-only).
2. **Realization freshness has one home; tessellate does not write it yet (η, θ, R4, R6).** The sink writes the `NodeId::Realization` entry (`Final` on success, `Failed` on failure) on the surfaces that run the Realize post-process: Build, BuildSnapshot and distance. That write moves out of `post_process_geometry_handle_cells`; the entries are the same, from one home.
   - Tessellate surfaces record the node outcome only, so the GUI still shows no failed realization, as today.
   - The reason (R6): geometry-handle value cells derive freshness at eval time from their Realization's entry (the `realization_reads` meet), and in a GUI edit eval runs before `tessellate_snapshot`. A tessellate-written entry would leave geometry cells `Pending` after every edit.
   - Follow-up #9165 (filed at pass 2, ← θ #9160 and S4 γ #8861) owns GUI realization freshness and the GH-cell re-derivation it needs.
3. **`REIFY_BUILD_SCHEDULER` is gone (α).** The one-release kill-switch from 2026-06-23 is deleted; `REIFY_BUILD_SCHEDULER=legacy` changes nothing. No script, CI file or `gui/` code sets it (§10.4).
4. **`build_snapshot` realizes like `build` (η).** It walks the unified schedule with `HydrateCell` steps, so a curated 3-arg fillet resolves (#7270 lands the first half; η makes it structural).
5. **`distance_between_placed` stops accumulating on the engine (η, R2).** It runs the Build walk without export and asks the owning kernel; two calls report independent dispatch attributions.
6. **The repr-aware region-selector gate follows one rule on every surface (η).**
   - **Today, on Build.** Each `build`'s `eval()` rebuilds the graph with every node at the `BRep` default.
     - The in-loop gate (op args and `HydrateCell`) reads a snapshot taken at template start, so it sees reprs realized by earlier templates but never a same-template target; the consuming op reaches the kernel.
     - The post-loop pass re-snapshots, so it does gate a same-template selector cell (`Undef` plus one `QueryNotSupportedOnRepr`).
   - **Today, elsewhere.** Tessellate surfaces pass an empty map to both passes and never gate. `build_snapshot` passes nothing in-loop and a graph view post-loop.
   - **After η.** The in-loop gate reads the walk-local map as of the current template's start, and the post-loop pass reads it current. That is today's Build rule, now stated as the walk's rule rather than an artifact of when a graph snapshot is taken.
     - Build output is byte-identical.
     - Tessellate surfaces and `build_snapshot` gain the rule, so a region selector over an isosurface `Mesh`/`Voxel` target gets the gate's coded diagnostic instead of the generic error path.
   - B-η6 pins both sides.

**Leaf signals (G2, per leaf in §9).** User-visible leaves (α, η, θ) name a CLI or GUI observation. Refactor leaves name (a) an adoption-ratchet row at its target, (b) a behaviour pin on a real surface, and (c) the **regression floor**: `reify eval`, `reify check` and `reify build -o` over `examples/best_practices/*.ri`, the FEA/modal examples and `examples/multi_kernel/voxel_to_mesh.ri` print byte-identical output before and after the leaf.

## 2. Problem (re-verified at `d4cd598fee`)

### 2.1 Four hand-copied drivers (fk-f7e4716af878, high)
- Four functions call `Engine::execute_realization_ops(RealizationOpsInput::new(..)…)`: `build_snapshot`, `build_with_geometry_output` (behind `build`, `realize_for_check`, `build_outputs*`), `tessellate_from_values` (behind `tessellate_realizations` and `tessellate_snapshot`, a static fn with 23 parameters) and `distance_between_placed`.
- Each decides every per-realization axis inline. §10.1 tabulates 16 axes × 4 drivers. The decisions that differ: cross-let seeding (build only), schedule walk (build and tessellate; `build_snapshot` walks template order with no `HydrateCell`), demanded repr source (`compute_demanded_reprs` vs `voxel_pipeline_demand_overrides`), demanded boundary, `prefer_kernel` (None on distance), morph (off on tessellate and distance), `module_binds_selector` and `realized_reprs` (build paths only; the others keep fail-open defaults), the graph-node write-back and `mark_realization_failed` (build and `build_snapshot` only), the per-build reset (absent from distance), the demanded-tolerance lookup (`.get()` on three, a panicking index on tessellate) and the registry test seam (absent from tessellate).
- The 30-line `let build_steps: Vec<BuildStep> = match unified_pass…` block is duplicated token-for-token in `build_with_geometry_output` and `tessellate_from_values` (two renames apart). #7270 (in progress) is adding a third copy to `build_snapshot`.
- `distance_between_placed`'s contract with build is a comment: "any changes to the realization execution or terminal_handles bookkeeping in build() must be mirrored here."
- **Two drivers have no production caller.** `build_snapshot` is called from 22 test lines and `distance_between_placed` from 10, nothing else (§10.2). Production reaches only `build_with_geometry_output` and `tessellate_from_values`.

### 2.2 One 2,304-line executor body (fk-53997e96ef7c, high)
- `Engine::execute_realization_ops` is an associated fn with no receiver: 2,304 lines (1,095 code, 1,199 comment), nested 16 levels, CCN 134 (the file's maximum). Its stage map is §10.3.
- Its only coupling to `Engine` is two constants and two associated fns: `DEFAULT_TESSELLATION_TOLERANCE` (private, engine_build.rs), `DEFAULT_KERNEL_NAME` (engine_admin.rs; S10 ι₃ #8992 deletes it), `probe_realization_cache` and `bump_dispatch`. `mark_realization_failed` is called by the drivers, not the executor; the other `Engine::` tokens in the span are rustdoc.
- Per-op results are not typed. They are folded into six pieces of loop state and four out-parameters (`kernel_error_out`, `produced_repr_out`, the dispatch counters).
- **Rollback is count-based and partial.** The rollback decision is `had_failure || step_handles.len() − handle_start < operations.len()`; only the compile-error arm sets `had_failure`. The rollback restores `step_handles` and removes the intermediate cache entries this realization inserted. `realization_cache.insert_intermediate` runs mid-op, and a later parent of the same op reads the cache, so a conversion that fails at parent k leaves parents 0..k−1 in the cache and in the log. Topology-attribute entries and kernel-side geometry from rolled-back ops stay; they are per-build state.
- The VolumeMesh and Voxel terminal conversions are inline `if demanded_repr == …` blocks sharing a preamble (tessellate the terminal on its owner, then ingest into a fixed kernel).

### 2.3 Tessellate discards what it produced (fk-17e2aa81f6d6, medium)
- `tessellate_from_values` declares `kernel_error` and `produced_repr_out` per realization and never reads them. `distance_between_placed` does the same (`let _ = kernel_error;`).
- The comment calls the discard benign "because the construction-time default (BRep) already matches", and says the symmetric write "MUST close before ζ ships", "tracked by task ζ (#3437)". #3437 is done, and its scope never included the tessellate write. `voxel_pipeline_demand_overrides` (task 5033) has made the premise false: tessellate demands `Voxel`/`Mesh` for isosurface pipelines.
- **The write-back has two copies.** The `build_snapshot` and `build_with_geometry_output` blocks that write `produced_repr`, `produced_kernel` and `input_cone_hash` and call `mark_realization_failed` are identical apart from three local names.
- **A naive fix would split the record (R3).** `realization_handles` (the GHR validity oracle) is written and cleared only on build surfaces (S1's load-bearing asymmetry), and stores a bare `GeometryHandleId`. implicit-solids γ #8391 widens it to keep the owner; ε builds on that widening. `project_realization_read_handle` takes the arm from the node's `produced_repr`, the kernel from the node's `produced_kernel` and the id from `realization_handles`. If tessellate wrote the node while the oracle kept build's handle, the CLI combined arm could pair a gmsh VolumeMesh handle with an OCCT/BRep node, and an FEA compute input would lose its mesh.

### 2.4 The scheduler kill-switch outlived its release (fk-98754d80a67e, medium)
- `BuildScheduler::{LegacyMultiPass, UnifiedDag}` is read once from `REIFY_BUILD_SCHEDULER` at `Engine::new`. It has two production readers: `demand_scoped_unified_pass` returns `(None, None)` for legacy, and the post-geometry re-check compares the enum directly.
- That `(None, None)` also means "no `eval_state`". In practice it is unreachable on every caller: the two `check()`-running surfaces always have state, and `tessellate_snapshot` returns early without it.
- There is no legacy algorithm. The planners in engine_fixpoint.rs never branch on the scheduler, so "legacy" is the `None` arm of three `build_steps` matches, the post-loop hydration and a kernel-less re-check (§10.4).
- 21 test files reference it: 15 only pin `UnifiedDag` defensively, 6 compare or depend on legacy output. Outside `crates/`, `review/briefing.yaml`, `.claude/skills/review-all/project.md` and a comment in `examples/topology_selectors/fillet_top_edges.ri` still describe the variable.
- The default flipped on 2026-06-23 (2a7c699849, #4362). Deletion has been filed since then as **#4727**, whose dependencies (#4362, #4734, #5125) have all landed. Leo's R1 adopts it as this stream's first leaf. This session parked it while re-scoping it.

### 2.5 The projection has three tails and stale prose (fk-9e6d548589d8, low; filed as #8534)
- `project_realization_read_handle`'s Sdf/Voxel arm bypasses `resolve_realization_kernel`, `memoize_projection` and `degrade_projection`. It inlines a memoize and two degrade tails and resolves the kernel by `openvdb_kernel_name()`, not by the node's owner. S10 ι₆ #8998 deletes `openvdb_kernel_name` and keys densify on the owner.
- The shared resolver returns `&dyn GeometryKernel`, but `densify_grid_to_sampled` takes `&mut self`, so the arm cannot route through the helper as it stands.
- A second writer of the projection store exists in engine_build (`redispatch_geometry_consuming_compute_nodes` pre-tessellates BRep bodies and inserts directly). An absent-node arm has a third inline degrade tail.
- Three `#[allow(dead_code)]` sit on live items, and the module header and `degrade_projection`'s doc describe states that have since landed.
- **Four homes of one tolerance.** `Engine::DEFAULT_TESSELLATION_TOLERANCE`, `MESH_PROJECTION_SENTINEL_TOL` (realization_content), `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` (geometry_ops) and `GDT_CONFORMANCE_TESSELLATION_TOLERANCE_M` (engine_constraints) each hold `0.0001` and say they "mirror" the first. #4691 (pending, polish) tracks two of them.

### 2.6 The file and the cycle (h14)
- engine_build.rs is 14,468 lines (14,030 production). The record's seven-partition measurement holds at main (§10.5), with two corrections:
  - The post-process partition's "impl-Engine satellite" premise is false for 20 of its 25 items, which are associated fns with explicit arguments. Their only callers are the entry surfaces.
  - The executor partition's only upward references are the constants above.
- **Child modules lower nothing.** `scripts/assert-module-scc.py` attributes `engine_build/*.rs` to the `engine_build` node, so splitting into child modules changes no SCC row. Only crate-root modules or moved imports do.
- **One import holds geometry_ops in the SCC.** The single production edge `geometry_ops → engine_build` is `crate::engine_build::realize_sub_override_handles`, called inside `walk_placed_realizations`. Removing it takes `geometry_ops` and `realization_content` out of the 24-module SCC (scout model, reproducing today's baseline; §10.5).

## 3. Pre-conditions and substrate (G3, G6)

**Upstream filed work this stream builds on** (each read with `get_task` at authoring; none is done).

| id | stream | what it provides | consumed by |
|---|---|---|---|
| #7270 | none (in progress) | `build_snapshot` walks the unified schedule with `HydrateCell` steps | α, η |
| #8687 | S3 β | `sub_refs_in_op` and `compute_realization_upstream_values_hash_from_ops` move to `realization_inputs.rs`; the reify-eval SCC shrinks | β₁, δ |
| #8747, #8750, #8751 | S13 α, δ, ε | the {compute_targets, …} 5-SCC dissolves; `compile_geometry_op` / `prehydrate_inline_curated_selector_args` / `InlineSelectorMemo` live in `geometry_ops/compile`; geometry_ops.rs is mod declarations + the placed-walk partition, and no geometry_ops test references a placed-walk symbol | γ, δ |
| #9020 | S12 ε2 | `surface_subtree`'s callback takes a typed (occurrence, realization index) key | γ |
| #8992, #8993, #8995, #8998 | S10 ι₃, ι₄, ι₅, ι₆ | `KernelSet` keyed by `KernelId` (the default-kernel sentinel goes); ops dispatch through `select(Op)` with the BRep fallback inside; handle-served requests go to the owner; `openvdb_kernel_name` goes and densify keys on the owner | δ, ε, ζ, η |
| #8872 | S4 τ | `eval_ctx_with_meta` deleted; engine_build's 15 builder sites use `op_arg_eval_ctx` / `probe_eval_ctx` / `cell_eval_ctx` | η, ι₁, ι₃ |
| #9043 | S1 ε | `PerBuildState` in per_build_state.rs with `reset(BuildSurface)`; `BuildSurface` moves there; `realization_handles` becomes a PerBuildState field with named writers | ε, η |
| #9036 | S1 α | `EngineConfig` with `long_chain_threshold` and `mesh_contract` read once | η |
| #9038 | S1 γ₁ | `Engine::new(EngineInputs)`, `EngineInputs::hermetic()`; the `test_registry_override` seam is deleted | η (tests) |
| #9046 | S1 θ | `DispatchAttribution` returned on `BuildResult` / `TessellateResult`; no engine-held tallies | η |
| #9017, #9021 | S12 ε1, ε3 | `MeasurementContext`; `check(module, &MeasurementContext)`; `capture_measurement` copies the handle oracle with upstream hashes | ε, η |
| #8673 | S3 α (done) | `scripts/assert-module-scc.py` and the baseline | γ, every module-adding leaf |

**Substrate verified at main (G3).** No grammar is touched. Every symbol named in §5 exists at `d4cd598fee` under the name used, except the new modules and types this PRD creates. Two premises in the review record were corrected by re-reading the code:
- **P1.** The post-process partition is not an impl-Engine satellite: 20 of its 25 items take explicit arguments (§2.6).
- **P2.** Neither `RealizationOpsInput` nor `RealizationOutputs` has a consumer outside engine_build.rs. Both are private, and "compute-contract consumers" in the program's G4 row names no code reference. The seam is S2-internal; the executor keeps them `pub(crate)` inside its module.

**Premise checks (G6).**
- The schedule can be non-optional without losing a behaviour: no code path exists only for legacy (§2.4, §10.4).
- The rollback refactor preserves today's decision: every `Executed` op pushes exactly one handle, and every abort breaks the loop, so "any outcome other than `Executed`" equals today's count rule (B-δ1 pins it).
- R3's coherence claim rests on readers taking owner and repr from the oracle entry. ε's ratchet row counts node-field reads (§5.9), and B-ε1 is the behavioural cover.

## 4. Resolved design decisions

- **D1 — One walk, entries own their prelude (η).** `Engine::realize_templates(module, &SurfacePolicy, values, demand, dispatch) → WalkOutput` is the only caller of the executor.
  - Each public entry keeps its prelude: the relate pre-pass (Build), the `PerBuildState::reset(surface)` call at today's position (S1 ε's contract; positions differ by surface and are load-bearing, §10.1 axis 13), the value source (`check()` or the snapshot) and its product finalization.
  - The walk owns seeding, the schedule walk, the demand lookup, the executor call, the write-back, the per-step hydration, the post-process set and `snapshot_named_steps`.
  - The relate pre-pass's nested `engine.build` stays legal, because the walk holds no state across calls.
- **D2 — `SurfacePolicy` is a value built from the entry (η).** `SurfacePolicy::for_entry(EntryRequest)` is one exhaustive match, the §5.1 table, so each axis is one cell. Axes that were per-surface accidents become universal:
  - `prefer_kernel` is the module pragma everywhere;
  - `module_binds_selector` is computed once per walk;
  - the selector gate reads a walk-local map of the reprs this walk produced, filled from each `RealizationStatus` as the walk proceeds, on every surface. The in-loop passes (op args, `HydrateCell`) read it as of the current template's start, and the post-loop pass reads it current (§1 item 6). It never reads the oracle (a stale build entry would gate a tessellate result) or the graph node. `realized_reprs_snapshot` is deleted;
  - `long_chain_threshold` and `mesh_contract` come from `EngineConfig` (S1 α);
  - demanded tolerances use `.get()` everywhere (a missing entry is `None`, never a panic).
  Morph enablement is derived (`DemandRule::ForFormat` only), not a field.
- **D3 — R2: `build_snapshot` is a policy row; `distance_between_placed` is a query over the Build walk.** `build_snapshot` keeps its contract (snapshot values, no eval) and gains the schedule from the walk. `distance_between_placed(module, a, b)` runs `EntryRequest::PlacedBodies` (the Build surface, no export product), then serves `GeometryQuery::Distance` through S10 ι₅'s owner rule over the two placed bodies. It therefore resets like Build, records like Build, and accumulates nothing on the engine (S1 θ).
- **D4 — R1: the scheduler is retired first, as the adopted #4727 (α).**
  - `BuildScheduler`, `LegacyMultiPass`, `from_env`/`from_env_value`/`ENV_VAR`, the Engine field, the gated `set_build_scheduler` and the vestigial `unified-dag` feature are deleted.
  - `demand_scoped_unified_pass` returns `UnifiedPassResult` plus the optional demand seed, with no `Option` around the schedule. `tessellate_snapshot` keeps its own early return when there is no `eval_state`.
  - α lands before S1 α #9036, so S1 never moves or migrates a doomed enum: one edge #9036 ← α, and notes on #9036, #9040 and #9047.
  - `detect_let_cycle` is out of scope (§7): it is the only cycle report on eval/check surfaces that run no unified pass.
  - #4727's other items are decided here, not left to the implementer:
    - Delete `resolve_subhandle_list` wherever it lives at α's base (S13 may have moved it), its six tests, and the rustdoc links that name it (in geometry_ops.rs and its tests). It has no production caller and no live task owns wiring one, so its `#[allow(dead_code)]` and `TODO(#4727)` go with it; the commit records why.
    - Retire the `#[ignore]`'d MockGeometryKernel fillet smoke test (`fillet_curated_edges_e2e` covers it).
    - The geometry_ops/tests.rs 4-arg e2e TODO goes with the test file S13 ε #8751 deletes, or is deleted by α if it still exists at α's base.
    - The `process_dfm_e2e` ignores and the `engine_admin.rs` / `Cargo.toml` notes that cite #4727 are deleted with their subjects.
    - `review/briefing.yaml`, `.claude/skills/review-all/project.md` and the `fillet_top_edges.ri` comment stop describing the variable.
- **D5 — Differential corpus fate (α).** The legacy-vs-unified harness was the cutover safety gate. Its value splits three ways:
  - allow-listed divergences (`ConstraintFlips`, `DiagnosticAdded`) become direct assertions on the unified result;
  - empty-allow-list equivalence cases become committed golden projections, generated once from the unified engine at α's base and reviewed in the diff. The comparison basis is stated per cell: a solver-free cell compares its `content_hash`; a solver-derived cell (the harness builds with `DimensionalSolver`) compares through the harness's existing solver-parity tolerance, never bit-exactly, because the gate runs on heterogeneous hosts;
  - the 2× determinism and `residue == ∅` halves are unified-only and stay unchanged.
  The `Divergence` / `CorpusCase.allowed` machinery is deleted. Edit-path loops over both schedulers lose their legacy half, and setter-only tests lose the call.
- **D6 — Typed executor (δ).** It lives in crate-root `realization_executor/`, free functions with no `Engine`. `execute_realization(input, outputs) → ExecutedRealization` carries a typed `RealizationStatus` instead of out-parameters, and each op returns `OpOutcome`. A `#[must_use]` `IntermediateInserts` log is consumed exactly once by `commit` or `roll_back`. The VolumeMesh and Voxel terminals are one function each over a shared `tessellate_terminal`. Contract §5.3–§5.4.
- **D7 — The rollback scope is unchanged and stated (δ).** Rollback restores `step_handles` and removes every intermediate this realization inserted, including those inserted by the failing op before it failed. Topology-attribute entries and kernel-side geometry from rolled-back ops are per-build state and stay, as today. The commit owner's doc says so, and B-δ1 pins it.
- **D8 — R3: the oracle owns kernel and repr (ε).**
  - `realization_handles` becomes `HashMap<RealizationNodeId, RealizedTerminal { handle: KernelHandle, repr: ReprKind }>`. It is still written and cleared only on build surfaces (S1's asymmetry, unchanged).
  - Every reader that needs a realization's geometry takes the owner kernel and the repr from the entry, never from the graph node: the projection, `realize_solid_sdf`, `measure_min_feature`/`measure_min_wall`, `revalidate_geometry_handle`, the redispatch pre-tessellation, and S12's `capture_measurement`, whose context entries carry the `RealizedTerminal`. `engine_constraints`' three direct reads are retired by S12 ε1 #9017 (its row C6), a dependency of ε.
  - The selector gate is not an oracle reader: it reads the walk-local map (D2).
  - The graph node's `produced_repr` / `produced_kernel` become the last surface's outcome: provenance, `realization_kernel_provenance` and the CLI's `--verbose` line.
- **D9 — One write-back sink (η introduces it, θ completes it; R4).**
  - `record_outcome(graph, cache, journal, version, &RealizationOutcome)` is an Engine-free fn over disjoint borrows. It is the only writer of:
    - the node's `produced_repr` and `produced_kernel`, and the walk's only writer of `input_cone_hash`;
    - the realization's `NodeId::Realization` cache entry: `Freshness::Final` with the realization's scalar-read `DependencyTrace` on success, and `Freshness::Failed` through `mark_realization_failed` on failure.
  - R4: the success-path entry write moves out of `post_process_geometry_handle_cells`, which keeps only the oracle insert and the geometry-handle value cells. There is one home for the realization's freshness, separate from the handle oracle.
  - The entry's `CachedResult::GeometryHandle` payload stays today's content-identity token for early cutoff. It is never read as a live handle (cache.rs's existing rule; handle readers use the oracle, D8).
  - η calls the sink where today's drivers write (Build and BuildSnapshot) through a transitional policy field `records_outcome`, so η stays behaviour-preserving: build surfaces write the same entry, from a new home.
  - θ deletes the field (R6): every surface records the node outcome (`produced_repr`, `produced_kernel`, `input_cone_hash`). Because of D8, a tessellate write can no longer mislead a handle reader.
  - The `NodeId::Realization` entry is written iff `policy.post_process == PostProcessSet::Realize`. That is a derived cell, not a field: Build and BuildSnapshot as today, plus distance from η (R2). Tessellate surfaces write no entry, pending the R6 follow-up.
- **D10 — Placed walk moves to crate-root `placed_walk.rs`; the override realizer is a parameter (γ).** The nine-symbol partition moves verbatim. `surface_export_bodies` and `surface_subtree` take `&mut dyn FnMut` for the per-sub override realization, and the callers pass `realize_sub_override_handles`. `placed_walk.rs` imports `geometry_ops::pose` downward and nothing from engine_build. A crate-root home (not `geometry_ops/placed_walk.rs`) because the walk surfaces placed realizations; it does not compile geometry ops (heuristic 6). After γ, geometry_ops.rs holds only module declarations.
- **D11 — New modules sit outside the residual SCC (every module-adding leaf).** A crate-root module this PRD adds imports nothing that is a member of a reify-eval SCC at its landing, and names no `Engine` (its `engine-free-*` row at 0). `assert-module-scc.py check` stays green without `GREW`, and a `STALE` is lowered in the same diff from the tool's own `print`, never from this PRD's prediction.
- **D12 — The h14 partition (β₁, β₂, ι₁–ι₃).** engine_build.rs keeps the entry surfaces, the walk, the schedule helpers, the sink and the five `&mut self` post-process methods that re-enter compute dispatch. The rest moves to crate-root modules by purpose (§5.8). The executor's free dependencies move first (β₁, β₂) so δ's module never imports engine_build. ω measures the remainder against heuristic 14: if engine_build.rs is still above the 2,000-line alarm, the close leaf records the named partition and its symptoms in the commit message, as the heuristic's stays-whole protocol requires.
- **D13 — One tolerance default (β₁; subsumes #4691).** `DEFAULT_TESSELLATION_TOLERANCE_M` lives in `tolerance_budget.rs`. The four former copies (§2.5) are deleted and their users name the single home. The `max_deviation_tessellation_tolerance_pins_engine_default_value` pin is deleted because there is nothing left to pin. Pass 2 cancels #4691 as subsumed by β₁; its own plan, which kept a second const, is superseded. `kernel_id_for_registry_name` and `DEFAULT_KERNEL_NAME` are S10 ι₃'s to delete; δ depends on ι₃ and moves neither.
- **D14 — Why B + H.** The walk touches multi-kernel dispatch (a load-bearing seam in the overlay's G5 list), 13 upstream filed leaves across six streams, and two front ends whose observable behaviour must not move except where §1 says. §5 states the contract an architect implements without further discussion; §6's rows face both producer and consumer sides.

## 5. Contract (H)

### 5.1 `SurfacePolicy` and the one table (η)

```rust
// crates/reify-eval/src/surface_policy.rs (new). Imports crate::per_build_state::BuildSurface (S1 ε) and reify_ir only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryRequest {
    Build(ExportFormat), Realize, PlacedBodies, BuildSnapshot(ExportFormat),
    TessellateRealizations, TessellateSnapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SurfacePolicy {
    pub surface: BuildSurface,
    pub values: ValueSource,              // Evaluated (the entry ran check) | Snapshot
    pub scope: ScheduleScope,             // Full | DemandSeeded
    pub demand: DemandRule,               // ForFormat(ExportFormat) | Tessellation
    pub recheck_after_geometry: bool,     // check_constraints_post_geometry; cross-let seeding iff true
    pub post_process: PostProcessSet,     // Realize | Tessellate
    pub product: WalkProduct,             // Export { emit: bool } | PlacedBodies | Meshes
    pub records_outcome: bool,            // transitional (η only); θ deletes it
}
impl SurfacePolicy {
    pub(crate) const fn for_entry(entry: EntryRequest) -> Self;   // exhaustive match, no wildcard
    pub(crate) const fn morph_enabled(&self) -> bool;              // matches!(self.demand, DemandRule::ForFormat(_))
}
```

| entry (public fn) | surface | values | scope | demand | recheck | post-process | product | records (η) |
|---|---|---|---|---|---|---|---|---|
| `Build(f)`: `build(module, f)` | Build | Evaluated | Full | ForFormat(f) | yes | Realize | Export{emit: true} | yes |
| `Realize`: `realize_for_check`, `build_outputs*` | Build | Evaluated | Full | ForFormat(Step) | yes | Realize | Export{emit: false} | yes |
| `PlacedBodies`: `distance_between_placed` | Build | Evaluated | Full | ForFormat(Step) | yes | Realize | PlacedBodies | yes |
| `BuildSnapshot(f)`: `build_snapshot` | BuildSnapshot | Snapshot | Full | ForFormat(f) | no | Realize | Export{emit: true} | yes |
| `TessellateRealizations` | TessellateRealizations | Evaluated | Full | Tessellation | no | Tessellate | Meshes | no |
| `TessellateSnapshot` | TessellateSnapshot | Snapshot | DemandSeeded | Tessellation | no | Tessellate | Meshes | no |

**Rules.**
- `DemandRule::ForFormat(f)` is today's `compute_demanded_reprs(module, f)` + `compute_boundary_demands` + `compute_demanded_tols`. `Realize` and `PlacedBodies` demand as `Step`, as `realize_for_check` does today. The comment claiming `format` is "irrelevant" there is false (it feeds the demand) and is deleted. `DemandRule::Tessellation` is `voxel_pipeline_demand_overrides` with boundary `false` and the caller's tessellation budgets.
- `BuildSnapshot` keeps today's constraint source (`check_constraints_against_templates` on snapshot values; no post-geometry re-check). Changing it is not ruled (§7).
- `ScheduleScope::DemandSeeded` is reachable only from `TessellateSnapshot`, the one surface that runs no `check()`. The build surfaces' `demanded_rids_build` plumbing is dead and is deleted with the `Option`.
- A new surface is one `EntryRequest` variant plus one table row. A new per-realization knob is one `SurfacePolicy` field or one `RealizationOpsInput` field, never a third place.

### 5.2 The walk (η)

```rust
impl Engine {
    fn realize_templates(&mut self, module: &CompiledModule, policy: &SurfacePolicy,
        values: &ValueMap, demand: &WalkDemand, dispatch: &mut DispatchAttribution) -> WalkOutput;
}
pub(crate) struct WalkDemand { /* per (template, realization): repr, tol, boundary; budgets for Meshes */ }
pub(crate) struct WalkOutput { pub terminal_handles: Vec<Vec<Option<KernelHandle>>>,
    pub module_named_steps: HashMap<String, HashMap<String, KernelHandle>>,
    pub diagnostics: Vec<Diagnostic>, pub product: WalkProductValue }
```

Per template, in order:
1. `seed_cross_sub_named_steps`; `seed_cross_let_named_steps` iff `policy.recheck_after_geometry`.
2. `build_steps` from the schedule: the one copy of today's block, over a non-optional `UnifiedPassResult`.
3. For each `Realize` step:
   - build `RealizationOpsInput` (§5.3) with every field named at this one site;
   - call `execute_realization`;
   - call `record_and_resolve_terminal`;
   - insert the status's repr into the walk-local repr map. `SelectorGate` reads its template-start copy, taken when each template begins (D2);
   - call `record_outcome` (§5.5) when `policy.records_outcome` (θ: always);
   - store the morph source when `policy.morph_enabled()`;
   - under `PostProcessSet::Realize`, call `post_process_geometry_handle_cells` (today's O(R²) per-step call).
4. For each `HydrateCell` step, call `hydrate_value_cell_in_loop` with `realization_read_cells` and the template-start copy of the walk-local repr map.
5. The post-process set (`run_post_processes` takes the current walk-local repr map):
   - `Realize`: `post_process_geometry_handle_cells`, both redispatches, conformance, kinematic, `run_post_processes`, containment samples and the contentless-redispatch warnings, in today's order.
   - `Tessellate`: `hydrate_geometry_handles_into_values`, conformance, kinematic and `run_post_processes`.
6. `snapshot_named_steps`.

The product finalization (export, placed bodies, meshes with `capture_repr_tol` / `achieved_repr_tol`) is a match on `policy.product`, after the template loop.

### 5.3 `RealizationOpsInput` — required fields, one construction site (δ moves it; η fills it)

```rust
// crates/reify-eval/src/realization_executor/mod.rs
pub(crate) struct RealizationOpsInput<'a> {
    pub kernels: &'a mut KernelSet,                      // S10 ι₃/ι₄
    pub ops: &'a [CompiledGeometryOp],
    pub env: OpEnv<'a>,                                  // values, functions, meta (S4 τ's op_arg_eval_ctx inputs)
    pub realization: RealizationRef<'a>,                 // id, name, span, is_terminal
    pub demand: RealizationDemand,                       // repr, tol: Option<f64>, boundary
    pub prefer_kernel: Option<KernelId>,                 // the module pragma
    pub morph: MorphDispatchIo<'a>,                      // disabled() unless policy.morph_enabled()
    pub posture: ExecutorPosture,                        // long_chain_threshold, mesh_contract (EngineConfig)
    pub selector_gate: SelectorGate<'a>,                 // { binds_selector: bool, realized_reprs: &'a HashMap<RealizationNodeId, ReprKind> } — the walk-local map (D2)
    pub realization_cache: &'a mut RealizationCache<KernelHandle>,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}
```
- No `Default`, no `new(..)` with defaulted fields, no `with_*` setters. An omitted field is a compile error. The fail-open defaults (`module_binds_selector: true`, `realized_reprs: None`) are deleted.
- `RealizationOutputs` keeps its six borrows (`step_handles`, `named_steps`, `named_step_reprs`, the two attribute tables, and no `produced_repr_out`: the status carries it).

### 5.4 The executor (δ)

```rust
// crates/reify-eval/src/realization_executor/{mod,op,conversion,terminal,commit,attributes,probe}.rs — no Engine
pub(crate) fn execute_realization(input: RealizationOpsInput<'_>, outputs: RealizationOutputs<'_>) -> ExecutedRealization;
pub(crate) struct ExecutedRealization {
    pub status: RealizationStatus, pub dispatches: usize,
    pub morph_source: Option<MorphSource>, pub volume_mesh: Option<MeshedVolume>,
    // S16 β #9092 adds `morph_outcome: MorphOutcome` here (#9092 ← δ); the walk feeds it to the per-build tally
}
pub(crate) enum RealizationStatus {
    Realized { terminal: KernelHandle, repr: ReprKind },
    CacheHit { terminal: KernelHandle, repr: ReprKind },
    NoOps,
    Failed(ErrorRef),
}
pub(super) enum OpOutcome { Executed { handle: KernelHandle, repr: ReprKind }, CompileFailed, Aborted(ErrorRef) }
pub(super) fn execute_op(op: &CompiledGeometryOp, scratch: &mut RealizationScratch<'_>, input: &mut OpInputs<'_>) -> OpOutcome;
#[must_use]
pub(super) struct IntermediateInserts { /* (ConversionSlot, ReprKind, f64, ContentHash) */ }
impl IntermediateInserts {
    pub(super) fn record(&mut self, slot: ConversionSlot, repr: ReprKind, tol: f64, hash: ContentHash);
    pub(super) fn commit(self);
    pub(super) fn roll_back(self, cache: &mut RealizationCache<KernelHandle>, rid: &RealizationNodeId);
}
pub(super) struct TerminalConversion { pub handle: KernelHandle, pub repr: ReprKind, pub volume_mesh: Option<MeshedVolume> }
pub(super) fn volume_mesh_terminal(..) -> Result<Option<TerminalConversion>, ErrorRef>;   // morph, attributed, plain
pub(super) fn voxel_terminal(..) -> Result<Option<TerminalConversion>, ErrorRef>;
```

**Rules.**
- **Folding.** The fold over ops stops at the first `Aborted` and continues past `CompileFailed`, which pushes the INVALID sentinel exactly as today. The realization rolls back iff any outcome is not `Executed` (equivalent to today's count rule; B-δ1).
- **The log.** Every `insert_intermediate` is followed in the same statement block by `IntermediateInserts::record`, so a failed op's inserts are logged. The log is consumed by value by exactly one of `commit` (success) or `roll_back` (failure). Its `Drop` asserts it was consumed (`debug_assert!`). An intermediate cache hit is not logged, so rollback never removes an earlier realization's entry.
- **Attribute population** (seed, solid attribute, `populate_attribute_history`, the kernel attribute hook) happens inside `execute_op` while the kernel is borrowed. `OpOutcome` carries no attribute history.
- **Scratch.** `RealizationScratch` holds the per-realization accumulators: step ids/reprs, solid-attribute handles, the drop tally, the longest-chain plan, the pragma flag, the inline selector memo, and the intermediate log. The `forward_and_track` closure becomes a scratch method.
- **Terminal conversions** run after the fold only on `Realized`, keyed by `demand.repr` and `is_terminal`. Each shares `tessellate_terminal(owner, handle, tol)`. The selector tie-scan and its centroid helpers (`collect_centroids_with_failure_summary`, `parse_bbox_*`) move into the executor; the tie-scan is their only caller.
- **Cache hits.** `probe_realization_cache` moves into `probe.rs` and returns `RealizationStatus::CacheHit`. INV-BUILD-3's pointer follows it (§5.10).
- **No Engine.** The executor names no `Engine`. `mark_realization_failed` moves to the sink (§5.5), because recording a failure is a write-back. `bump_dispatch` becomes a scratch counter reported as `dispatches`; the walk records it into the call's `DispatchAttribution` (S1 θ).
- **Heuristic 2 and 14 targets, measured at δ.** No function in `realization_executor/` nests more than six levels (the record's leading-indent rule), and no file in it exceeds 1,500 lines.

### 5.5 The sink (η, θ)

```rust
// crates/reify-eval/src/engine_build.rs — Engine-free, over disjoint borrows
pub(crate) struct RealizationOutcome<'a> { pub id: &'a RealizationNodeId, pub status: &'a RealizationStatus,
    pub input_cone_hash: [u8; 32], pub trace: DependencyTrace }
pub(crate) fn record_outcome(graph: &mut EvaluationGraph, cache: &mut CacheStore,
    journal: &mut EventJournal, version: VersionId, outcome: &RealizationOutcome<'_>);
```
- **Every outcome** writes `input_cone_hash`, as today: the build write-back is unconditional on cache hit, miss or failure. A missing prior hash reads as CHANGED (`realization_staleness`), so skipping it would over-evict on the next edit.
  - The sink is the walk's only cone-hash writer. `refresh_and_gate_demanded_realizations`'s pre-gate stamp and `edit_param`'s carry remain separate writers with their own semantics.
- **`Realized`/`CacheHit`** write `produced_repr` and `produced_kernel`, where `terminal` is the realization's last step handle, today's `step_handles.last()`; for a VolumeMesh this is the gmsh handle, not `record_and_resolve_terminal`'s egress body.
  - On Realize post-process surfaces (D9) they also write the `NodeId::Realization` entry, for named realizations only as today: `CachedResult::GeometryHandle(terminal.id)`, `Freshness::Final` and the trace, with today's early-cutoff semantics. Today's call is `record_evaluation_with_freshness`; after S4 γ #8861 it is that leaf's realization write primitive (`record_node`), and η depends on #8861. That is the write today's `post_process_geometry_handle_cells` performs, moved (R4).
- **`Failed`** calls `mark_realization_failed` on Realize post-process surfaces, unchanged. On tessellate surfaces it writes only the cone hash, and the kernel diagnostic is emitted as today (R6).
- **`NoOps`** writes only the cone hash, as today.
- A success writes over a prior `Failed` entry with the same primitive a build uses today.
- The sink writes nothing to the oracle. The oracle is written by `post_process_geometry_handle_cells` under `PostProcessSet::Realize` only (D8).
- The sink stays a pure writer. The walk computes the trace from the realization declaration (`extract_realization_dependencies(&realization.operations)`, today inside `post_process_geometry_handle_cells`) and the cone hash, and passes both in `RealizationOutcome`.
- Sibling in flight: #6422 changes this entry's payload hash. Whichever of #6422 and η lands second rebases (§8).

### 5.6 The realized-terminal oracle (ε, R3)

```rust
// crates/reify-eval/src/per_build_state.rs (S1 ε's module; ε amends the field type)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RealizedTerminal { pub handle: KernelHandle, pub repr: ReprKind }
// PerBuildState.realization_handles: HashMap<RealizationNodeId, RealizedTerminal>
impl PerBuildState {
    pub(crate) fn realized(&self, id: &RealizationNodeId) -> Option<RealizedTerminal>;
}
```
- `post_process_geometry_handle_cells` writes the entry from the terminal `KernelHandle` and the outcome's repr, on build surfaces only. The reset table is S1's, unchanged.
- **Readers** take the owner kernel (`entry.handle.kernel`, routed through S10's `select(OnHandle{owner})`) and the repr (`entry.repr`) from the entry: `project_realization_read_handle` and its resolver, `realize_solid_sdf`, `measure_min_feature`, `measure_min_wall`, `revalidate_geometry_handle`, and the redispatch pre-tessellation.
- **S12's context.** `capture_measurement` copies entries into `MeasurementContext` with their upstream hash. A context entry carries a `RealizedTerminal`, not a bare `GeometryHandleId` (cross-reference to S12, §8).
- **Node fields.** The node's `produced_repr`/`produced_kernel` are read only by `realization_kernel_provenance` (the CLI's verbose print reads it) and `realized_reprs_snapshot` until η deletes it (ratchet row `node-outcome-read`, §5.9).

### 5.7 Placed walk (γ)

```rust
// crates/reify-eval/src/placed_walk.rs (new; moved verbatim from geometry_ops.rs)
pub(crate) type OverrideRealizer<'r> = dyn FnMut(/* today's nine arguments of realize_sub_override_handles */) -> /* its return */ + 'r;
pub(crate) fn surface_export_bodies(/* today's args */, realize_overrides: &mut OverrideRealizer<'_>) -> Vec<ExportBody>;
pub(crate) fn surface_subtree(/* today's args, S12 ε2's typed key */, realize_overrides: &mut OverrideRealizer<'_>);
// also: walk_placed_realizations, PlacedPreFilter, ExportBody, root_template_indices, reachable_template_indices,
//       non_final_realization_indices, realization_is_aux
```
- Rule: `placed_walk.rs` names no `engine_build` path (row `placed-walk-reach-back` at 0). The callers (engine_build's `collect_export_bodies_walk` and the tessellate product; after η, the walk's product finalization) pass `realize_sub_override_handles`.
- In the same diff, γ runs `python3 scripts/assert-module-scc.py check`. It expects `STALE reify-eval` (geometry_ops, and per the scout model realization_content, leave the 24-SCC) and lowers the row from `assert-module-scc.py print`.

### 5.8 Module homes (D11, D12)

| module (crate root) | purpose (one sentence) | creating leaf | moved from |
|---|---|---|---|
| `geometry_op_lowering.rs` | Maps compiled geometry ops to kernel `Operation`s and their repr acceptance. | β₁ | `geometry_op_to_operation`, `compiled_geometry_op_to_operation`, `classify_op_input_reprs`, `op_accepts_repr`, `op_is_voxel_only_input`, `plan_output_repr` |
| `selector_presence.rs` | Decides whether a module or value set binds a topology selector. | β₁ | `module_binds_selector`, `expr_contains_selector_ctor`, `geometry_op_arg_exprs`, `value_contains_selector`, `values_contain_selector` |
| `tolerance_budget.rs` (exists) | gains `DEFAULT_TESSELLATION_TOLERANCE_M` | β₁ | four copies (§2.5) |
| `volume_mesh_assembly.rs` | Builds volume meshes (surface→tet dispatch, mixed shell/tet regions) for VolumeMesh terminals. | β₂ | `VolumeMeshOutcome`, `dispatch_volume_mesh`, `UnifiedElement*`, `MixedRegionMesh`, `MixedRegionError`, `build_mixed_region_mesh`, `dot3`, `dist3_sq`, `nearest_node_index`, `three_nearest_node_indices`, `p2_substitution_diagnostic`; their three test files |
| `placed_walk.rs` | Surfaces placed realizations of a module's template tree. | γ | §5.7 |
| `realization_executor/` | Runs one realization's compiled ops to a terminal handle. | δ | §5.4, plus the P1 helpers (`populate_*`, `TopologyCorrespondenceDropTally`, `forward_solid_attribute_on_ingest` with its lib.rs re-export, `parent_handles_for_op`, `substitute_op_parents`, `conversion_slot`, `ParentHandles`, `SingleParentSweepKind`) and the DispatchTestState scenarios from `engine_build/tests.rs` |
| `surface_policy.rs` | Holds the per-entry realization policy table. | η | new |
| `realization_seeding.rs` | Seeds named steps across subs and lets, and realizes per-sub overrides. | ι₁ | `seed_cross_sub_named_steps`, `realize_overridden_instance_into`, `seed_cross_let_named_steps`, `realize_sub_override_handles`, `snapshot_named_steps` |
| `realization_demand.rs` | Computes per-realization demanded repr, boundary, tolerance and tessellation budget. | ι₂ | the P3 partition as free fns over `(module, functions, format)`; `voxel_pipeline_demand_overrides` |
| `geometry_post_process.rs` | Hydrates geometry-derived value cells after realization. | ι₃ | P4's explicit-argument items, `GeometryListCellAccumulator`, the DFM fold helpers (`is_fits_build_volume_call`, `eval_folded_expr_for_dfm_diagnostics`). `arg_contains_cross_sub_geometry_ref` is S3 β #8687's (to `realization_inputs.rs`) |
| `build_export.rs` | Turns placed bodies into export artifacts and declarative outputs. | ι₃ | `build_outputs_with_result`'s artifact half, `export_product_bodies`, `resolve_export_body_color`, `resolve_instance_color`, `resolve_artifact_path` |

- engine_build.rs keeps the entries, `realize_templates`, `demand_scoped_unified_pass`, `realization_read_cells`, `collect_export_bodies_walk`, the sink, `current_eval_version`, and the five `&mut self` post-process methods (`redispatch_geometry_consuming_compute_nodes`, `redispatch_as_printed_consuming_compute_nodes`, `emit_contentless_redispatch_warnings`, `post_process_containment_samples`, `refresh_and_gate_demanded_realizations`).
- `engine_build/tests.rs` (10,851 lines) is split with its subjects. Each leaf moves the tests of what it moves, as a `#[cfg(test)] mod tests;` child of the new module, and keeps their assertions. A test that reached a private item now reaches the new module's `pub(crate)` surface.
- The remaining engine_build tests are re-homed by ω's measurement only if engine_build/tests.rs stays above the alarm.

### 5.9 Adoption-ratchet rows (`tests/infra/adoption-ratchet.manifest`; grammar in its header)

Row shape as S1 §5.13: kebab-case ids, every ERE starts `^[^/]*`, each leaf adds its rows at the post-change count in its own diff.

| id | ERE (after the prefix) | path | ceiling (today → target) | leaf |
|---|---|---|---|---|
| legacy-scheduler | `(LegacyMultiPass\|BuildScheduler\|set_build_scheduler\|REIFY_BUILD_SCHEDULER)` | `crates/**/*.rs` | 212 → 0 | α |
| optional-unified-pass | `Option<&?(crate::engine_fixpoint::)?UnifiedPassResult>` | `crates/reify-eval/src/**/*.rs` | 2 → 0 | α |
| tessellation-default-copy | `const (DEFAULT_TESSELLATION_TOLERANCE:\|MESH_PROJECTION_SENTINEL_TOL\|MAX_DEVIATION_TESSELLATION_TOLERANCE_M\|GDT_CONFORMANCE_TESSELLATION_TOLERANCE_M)` (the `:` keeps the single home `DEFAULT_TESSELLATION_TOLERANCE_M:` out of the count) | `crates/reify-eval/src/**/*.rs` | 4 → 0 | β₁ |
| placed-walk-reach-back | `engine_build::` | `crates/reify-eval/src/placed_walk.rs` | (new file) → 0 | γ |
| geometry-ops-reach-back | `engine_build::` | `crates/reify-eval/src/geometry_ops.rs` | 1 → 0 | γ |
| engine-free-realization-executor | `(^\|[^A-Za-z_])Engine([^A-Za-z_]\|$)` | `crates/reify-eval/src/realization_executor/**/*.rs` | 0 | δ |
| engine-free-surface-policy / -placed-walk / -geometry-op-lowering / -selector-presence / -volume-mesh-assembly / -realization-seeding / -realization-demand / -geometry-post-process / -build-export | `(^\|[^A-Za-z_])Engine([^A-Za-z_]\|$)` (one row and one glob per module file) | that module's file | 0 | the creating leaf |
| executor-call-site | `execute_realization[(]` | `crates/reify-eval/src/engine_*.rs` | 0 → 4 at δ (the four drivers) → 1 at η (the walk) | δ, η |
| realization-ops-knob | `[.]with_(realization_name\|demanded_tol\|demanded_repr\|demanded_boundary\|prefer_kernel\|is_terminal_realization\|morph_io\|long_chain_threshold\|mesh_contract_mode\|module_binds_selector\|realized_reprs)[(]` | `crates/reify-eval/src/**/*.rs` | 68 → 0 | η |
| schedule-walk-block | `let build_steps: Vec<BuildStep>` | `crates/reify-eval/src/**/*.rs` | 2 (3 if #7270 adds one) → 1 | η |
| node-outcome-write | `[.]produced_(repr\|kernel) = ` | `crates/reify-eval/src/**/*.rs` | 4 → 2 (the sink) | η |
| node-outcome-read | `[.]produced_(repr\|kernel)([^_a-z ]\|$)` | `crates/reify-eval/src/**/*.rs` | 8 → 4 at ε (`realization_kernel_provenance` 2, `realized_reprs_snapshot` 1, a graph.rs test 1) → 3 at η | ε, η |
| projection-store-reach | `realization_projection_store` | `crates/reify-eval/src/engine_build.rs` | 3 (S1 ε #9043 removes the reset destructure's) → 0 | ζ |
| tessellate-outcome-discard | `let _ = kernel_error` | `crates/reify-eval/src/**/*.rs` | 1 → 0 | η |

Measured with `python3 tests/infra/adoption_ratchet.py --manifest <scratch rows>` at `d4cd598fee` (the engine, not `git grep`). No row's subject is a Rust attribute: the engine skips every line whose first non-blank character is `#`. The `\|` escapes the markdown pipe; the ERE in the manifest uses a bare `|`. Ceilings stated as "measured" are written at the measured count in that leaf's diff, and the commit message gives the rule.

### 5.10 Same-diff obligations (every leaf)

- **INV-ARCH-1.** `assert-module-scc.py check` green without `GREW`. A `STALE` is lowered in the same diff.
- **Audit baselines.** Regenerate `crates/reify-audit/pdiag-baseline.txt` when a diagnostic moves file, and re-key moved `ptodo-baseline.txt` fingerprints. After S24 ο #8822 lands, run `plinecite-baseline-gen --rename <old>=<new>` for every moved file under PLINECITE scope. After S24 ξ #8821 lands, update every `docs/invariants.md` Pointer that names a moved symbol. INV-BUILD-3 names `probe_realization_cache` (δ moves it), and INV-BUILD-1 names S1's reset (unchanged by S2).
- **Prose.** The comments that state the old shape go with their code: the `RealizationOpsInput` "8 ORTHOGONAL fields" doc, distance's mirror NOTE, the symmetric-write comment, the stale `BuildSurface` doc, `demand_scoped_unified_pass`'s duplicate doc and the legacy notes. Rationale that must persist goes to this PRD, not inline (Comments stance).
- **Drift guards.** A new reify-eval integration test is a module of an existing harness (`harness_engine`, `harness_cache`, `harness_kernel_realization`), never a new binary (the harness-layout gate lists reify-eval in `_HL_CRATES`). No leaf adds a wall-clock bound. The ratchet rows are manifest rows of an existing infra member, so no `run-all-classification.manifest` row is needed.

## 6. Boundary-test sketch (two-way)

| id | side | scenario | precondition | postcondition | leaf |
|---|---|---|---|---|---|
| B-α1 | producer | no scheduler | α applied | `harness_cache` suites green; allow-listed divergence cases assert the unified verdict directly; each former equivalence case compares against its committed golden; the 2× determinism and `residue == ∅` assertions unchanged | α |
| B-α2 | consumer (CLI) | inert kill-switch | `REIFY_BUILD_SCHEDULER=legacy` in the shell | `reify check` and `reify build -o x.stl` on `examples/topology_selectors/fillet_top_edges.ri` and one FEA example print byte-identical output to the unset run | α |
| B-β1 | producer | one tolerance | β₁ applied | the four former copies resolve to `tolerance_budget::DEFAULT_TESSELLATION_TOLERANCE_M`; `realization_content`'s Mesh-arm tests and the GD&T `max_deviation` tests green | β₁ |
| B-β2 | producer | volume-mesh assembly moved | β₂ applied | `mixed_region_tests`, `dispatch_volume_mesh_tests`, `p2_substitution_diagnostic_tests` green as children of `volume_mesh_assembly`; `sweep_classifier` imports it, not engine_build | β₂ |
| B-γ1 | lint | the cycle is gone | γ applied | `module_scc_ratchet` green with the lowered reify-eval row; geometry_ops is in no SCC; rows `placed-walk-reach-back` and `geometry-ops-reach-back` at 0 | γ |
| B-γ2 | consumer | placed surfacing unchanged | γ applied | `sub_placement_surfacing`, `sub_placement_assembly_gate`, `achieved_repr_tol` and the precision-nominal surface_subtree suites green; `reify build` on a sub-placement example byte-identical | γ |
| B-δ1 | producer | rollback scope | a mock conversion that fails at the second parent of the second op of a realization whose first op inserted intermediates | every intermediate this realization inserted (first op's and the failing op's first parent) is absent from the cache; an intermediate inserted by an earlier realization in the same build survives; the decision equals today's count rule on the existing 36 DispatchTestState scenarios | δ |
| B-δ2 | producer | commit reuse | two realizations sharing a conversion parent | the second reuses the first's committed intermediate (`execute_realization_ops_conversion_intermediates_cache_and_reuse`, re-homed) | δ |
| B-δ3 | producer | terminals | mock kernel set; demanded VolumeMesh (plain, attributed, morph) and Voxel terminals | each terminal fn's handle, repr and `MeshedVolume` equal pre-δ; driven through `execute_realization` only | δ |
| B-δ4 | producer (INV-BUILD-3) | VolumeMesh cache hit | a VolumeMesh-demanded terminal served by `RealizationCache` on the second build | today's behaviour is pinned first (hit returns no `MeshedVolume`); the parity outcome is Open question 1 | δ |
| B-δ5 | consumer | regression floor | δ applied | §1's regression floor byte-identical | δ |
| B-ε1 | consumer (CLI) | coherent record | `realize_for_check → tessellate_realizations → check` on an FEA example whose realization is demanded as VolumeMesh, with a compute target reading its realization content | the compute input carries its VolumeMesh content; no "content projection unavailable" warning; verdicts byte-identical to today. Re-run by θ | ε, θ |
| B-ε2 | producer | oracle wins | a node whose `produced_repr`/`produced_kernel` disagree with its oracle entry (seeded) | the projection and `realize_solid_sdf` follow the entry | ε |
| B-ζ1 | producer | one projection home | Sdf/Voxel realization with and without an OpenVDB-owned entry; an absent node | densify goes through the shared memoize; every degrade uses `degrade_projection`'s text; the store's only insert is the shared memoize | ζ |
| B-ζ2 | consumer | shell extraction | `harness_topology_selector/shell_extract_compute_integration.rs` and `harness_kernel_realization/realization_read_api.rs` | green, assertions unchanged | ζ |
| B-η1 | both | the table | each `EntryRequest` | `SurfacePolicy::for_entry` equals its §5.1 row (one assertion per row; a new variant fails to compile the exhaustive match) | η |
| B-η2 | consumer (CLI) | regression floor | η applied | §1's regression floor byte-identical; `reify check`'s heavy arm verdicts byte-identical | η |
| B-η3 | consumer | distance is a Build query | the 10 `distance_between_placed` tests | green; two consecutive calls return independent `DispatchAttribution`s (S1 θ's B-θ3); a mixed-owner pair yields S10 ι₅'s coded Error | η |
| B-η4 | consumer | build_snapshot schedule | `examples/topology_selectors/fillet_top_edges.ri` via `edit_param` + `build_snapshot` | the curated fillet resolves as under `build`; the geometry equals `build`'s | η |
| B-η5 | producer | nested relate build | the relate suites (`relate_solve` sub-build inside `build`) | green, unchanged | η |
| B-η6 | consumer | one gate rule | a curated region selector over a same-template target demanded non-BRep and one over a cross-template target, run through `reify build -o x.stl`, `tessellate_snapshot` (GUI) and the CLI combined arm | Build: values and diagnostics byte-identical to pre-η (in-loop never gates same-template, post-loop gates once). Tessellate and `build_snapshot`: the gate's coded `QueryNotSupportedOnRepr` appears where pre-η had the generic error, with the per-surface diagnostic count recorded in the commit and no surface emitting more gate diagnostics per cell than Build does | η |
| B-η7 | producer | sink owns the entry | build, then `tessellate_realizations`, then `check` (records off on tessellate under η); and `distance_between_placed` on the sub-placement fixture | every `NodeId::Realization` entry's freshness, trace and payload and every node's `input_cone_hash` equal pre-η on the build path. Distance now writes them as Build does (§1 item 5), asserted once | η |
| B-θ1 | consumer (GUI) | tessellate records | `examples/multi_kernel/voxel_to_mesh.ri` loaded in a GUI `EngineSession` (load runs `tessellate_snapshot` only) | operand node `produced_repr == Voxel`, isosurface `Mesh`, `produced_kernel == OpenVdb` (cfg `has_openvdb`); a MockGeometryKernel variant with mock descriptors runs without native deps | θ |
| B-θ2 | consumer (GUI) | tessellate failure is quiet as today (R6) | a realization whose op fails at the loaded param value (`FailingMockGeometryKernel`) in a GUI session | the kernel diagnostic is emitted as today; no `NodeId::Realization` entry is created; the node keeps its prior `produced_repr`/`produced_kernel`; `input_cone_hash` is written; `get_entity_tree` output is byte-identical to pre-θ | θ |
| B-θ3 | consumer (GUI, engine) | no new cache entries on tessellate | a GUI session (load, three `preview_parameter` edits, reload) on a geometry fixture that never exports | the test asserts the cache holds no `NodeId::Realization` entry after the session; the selective-demand suites (`harness_cache/selective_demand_*`, `selective_eviction_*`) and the GUI edit-path tests stay green with assertions unchanged; hidden bodies still dispatch 0 ops | θ |
| B-ι1 | lint | the partition | each ι leaf applied | `module_scc_ratchet` green; its `engine-free-*` rows at 0; regression floor byte-identical | ι₁–ι₃ |

## 7. Out of scope

- **`detect_let_cycle`'s duplicate cycle report.** It is the only cycle report on eval/check surfaces without a unified pass. Its de-duplication is noted in engine_build's cycle-diagnostics comment and is not this stream's.
- **Realization handles on tessellate surfaces (R3's rejected option B).** The GUI still has no handle oracle, so geometry-consuming compute inputs in a GUI session still degrade, as today.
- **Redispatch of geometry-consuming compute nodes on tessellate surfaces.**
- **`build_snapshot` gaining the post-geometry re-check.**
- **`is_terminal_realization` by schedule position** instead of by template index.
- **Rolling back topology-attribute entries of failed realizations** (D7 records the residual).
- **Declarative `: Output` export provenance and colour matching** (#6366 parts 2–3, an IR design).
- **The sparse Sdf payload** (#8213).
- **Morph observability** (S16).

## 8. Cross-PRD relationship (G4)

| other PRD / stream | direction | mechanism | owner | note |
|---|---|---|---|---|
| S1 engine-state-partition | upstream; amended | `PerBuildState::reset(BuildSurface)`, `BuildSurface` home (ε #9043); `EngineConfig` (α #9036); `Engine::new(EngineInputs)` (γ₁ #9038); `DispatchAttribution` (θ #9046) | S1 produces; S2 consumes | R1: S2 α (#4727) lands before S1 α; edge #9036 ← α; notes on #9036 (no `BuildScheduler` to move; drop `REIFY_BUILD_SCHEDULER` from the knob table and B-α3), #9040, #9047 (no `set_build_scheduler` calls left). R3: S2 ε changes `PerBuildState.realization_handles`'s value type to `RealizedTerminal`. R2: `distance_between_placed` resets as Build (S1 §5.5 said "takes no reset"); note on #9043. S1 ε's own move of the reset destructure flips other PRDs' checks (#6740, #6741, #6898, #6923, #6171), which are S1's to widen (cross-reference lines) |
| S4 cell-eval-transaction-closure | upstream | τ #8872's constructors replace engine_build's 15 `eval_ctx_with_meta` sites; γ #8861 moves the realization cache write onto its sealed `record_node` primitive | S4 | η, ι₁, ι₃ ← #8872; η ← #8861 (the sink calls `record_node`) |
| S10 kernel-interface-typed-seam | upstream | ι₃ `KernelSet`, ι₄ `select(Op)`, ι₅ owner routing (and `distance_between_placed`'s owner rule, #6366 part 1), ι₆ owner-keyed densify | S10 | δ ← ι₃/ι₄/ι₅; ε ← ι₅/ι₆ (ε re-points their owner source to the oracle entry); ζ ← ι₆ |
| S12 check-surface | upstream; amended | ε1 `MeasurementContext` / `check(&ctx)`; ε2 `surface_subtree` typed key; ε3 context hashes | S12 | γ ← ε2; ε, η ← ε1, ε3. R3: context entries carry `RealizedTerminal` (cross-reference line) |
| S13 selector-resolution-factoring | upstream | α dissolves the 5-SCC; δ moves `compile`; ε leaves geometry_ops.rs parent-only with zero placed-walk test refs (its §7 seam) | S13 (the boundary); S2 (the extraction) | γ ← #8751; δ ← #8747, #8750 |
| S3 dependency-invalidation-stratum | upstream | β #8687 moves two engine_build fns to `realization_inputs.rs`; α #8673 the SCC tool | S3 | β₁, δ ← #8687 |
| S24 reify-audit | sibling | PLINECITE `--rename` (#8822), invariants Pointer column (#8821) | S24 | §5.10 same-diff obligations; INV-BUILD-3 pointer moves at δ |
| S16 elastic-modal-trampoline-factoring (#9085–#9122) | downstream | β #9092 returns a typed `MorphOutcome` from the executor into the per-build tally; any morph-observability input lands as a `RealizationOpsInput` / policy-derived field, never a `with_*` knob | S2 owns the executor | edge #9092 ← δ (pass 2); `ExecutedRealization` gains `morph_outcome` in #9092's diff |
| S23 prose rehoming | downstream | S23 skips engine_build / realization_content prose that S2 rewrites | S2 | cross-reference line |
| engine-unified-build-dag / eval-unified-schedule-executor | adopted | #4727 is α; #4958's milestone keeps its edge on #4727 | S2 | — |
| precision-nominal-representation-guarantee, gui-on-demand-measurement, implicit-solids, material-waste | sibling (checks) | their `delivered_check`s grep engine_build.rs / geometry_ops.rs constructs that S2 moves (§10.6) | their PRDs | pass 2 widens the paths (three writes each), per §11 L5 |
| instantiation-value-flow | downstream | generalises `realize_sub_override_handles` over γ's `OverrideRealizer` seam | that PRD | — |
| implicit-solids γ #8391 | upstream | widens `realization_handles` to keep the owner ("KernelHandle or a sibling owner map") | implicit-solids | ε builds on it (transitively upstream through S10 ι₅ #8995); pass-2 note on #8391: widen to `KernelHandle` in the one map, never a sibling owner map (R3's single entry) |
| #6422 (pending), #6032 (pending) | sibling | #6422 changes the Realization entry's payload hash; #6032 asks for an export-free `Engine::realize`, which is `realize_for_check` under η's new name | theirs | whichever of #6422 / η lands second rebases; pass-2 note on #6032 that η's rename delivers its API |
| #8401, #8489, #8460, #8288, #7164, #7300, #8330, #7296, #9024, #5980, #8215 | sibling | edit post-process ordering (#8401 after `post_process_geometry_handle_cells`), add a non-emitting build variant (#8489, which becomes an `EntryRequest` row), stamp `input_cone_hash` (#8460), drop `demanded_boundary` (#8288, whose `RealizationDemand` name also clashes with S5 #8994's) and touch redispatch / demand regions | theirs | file locks serialise; pass 2 posts a cross-reference note on each naming the S2 leaf that moves its region |
| meshing-service, hex-wedge, #6989, #6371 | sibling | edit the VolumeMesh arm or the mixed-region seam | theirs | file locks serialise; δ / β₂ rebase over what lands first |

## 9. Decomposition plan

Labels are Greek; task ids were assigned at decompose (2026-10-15) and are backfilled into each leaf heading. Every leaf is `task_kind="normal"`. Before editing, run `git diff d4cd598fee..main -- <files>` and re-read the symbols.

**Shared done criteria (every leaf).**
- §5.10's same-diff obligations.
- The regression floor (§1).
- Its ratchet rows at the stated ceilings.
- Its boundary-test rows green.
- No new `TODO`/`#[ignore]` without a live `#NNNN` cite.

- **α (#4727, adopted) — Retire `LegacyMultiPass`; the schedule is non-optional.**
  - **Work.**
    - D4 and D5: delete `BuildScheduler` and its parser, env var, field and setter, and the `unified-dag` feature.
    - `demand_scoped_unified_pass` returns a non-optional schedule. The three `build_steps` matches, the per-step `post_process_geometry_handle_cells` gate, the debug DAG assert's cycle gate and the post-geometry re-check compare collapse to their unified arm. The re-check's kernel-less `else` stays as the no-default-kernel branch.
    - `tessellate_from_values` takes `&UnifiedPassResult`.
    - Rewrite the 21 test files per D5. Goldens live beside `harness_cache/unified_dag_differential_corpus.rs` as committed projection files.
    - Apply D4's decisions on #4727's other items: delete `resolve_subhandle_list`, its six tests and its rustdoc links; retire the `#[ignore]`'d fillet smoke test; delete the remaining `#4727` cites with their subjects. No `#4727` cite survives.
    - Update `review/briefing.yaml`, `.claude/skills/review-all/project.md` and the `fillet_top_edges.ri` comment.
    - Re-key the PTODO baseline.
  - **Signal.** B-α1, B-α2; rows `legacy-scheduler` and `optional-unified-pass` at 0.
  - **Keys.** fk-98754d80a67e. Medium (the key's severity; #4727 was high).
  - **Deps.** #7270 (in flight on `build_snapshot`'s walk). Out-of-batch dependent: #9036 ← α.
- **β₁ #9154 — Executor prerequisites below engine_build: op lowering, selector presence, one tessellation default.**
  - **Work.** Create `geometry_op_lowering.rs` and `selector_presence.rs` (§5.8). Move `DEFAULT_TESSELLATION_TOLERANCE_M` into `tolerance_budget.rs` (D13) and point the four copies at it. Subsumes #4691.
  - **Signal.** B-β1; rows `tessellation-default-copy`, `engine-free-geometry-op-lowering` and `engine-free-selector-presence` at 0.
  - **Keys.** none (`x_finding_part` fk-53997e96ef7c). Medium.
  - **Deps.** #8687.
- **β₂ #9155 — `volume_mesh_assembly.rs`.**
  - **Work.** Move the P6 partition and its three test files (§5.8). `sweep_classifier` imports it.
  - **Signal.** B-β2; row `engine-free-volume-mesh-assembly` at 0.
  - **Keys.** none (`x_finding_part` fk-53997e96ef7c). Medium.
  - **Deps.** none.
- **γ #9156 — `placed_walk.rs`; the override realizer is a parameter; geometry_ops leaves the SCC.**
  - **Work.** §5.7 and D10.
  - **Signal.** B-γ1, B-γ2; the reify-eval INV-ARCH-1 row lowered in the diff; rows `placed-walk-reach-back`, `geometry-ops-reach-back` and `engine-free-placed-walk` at 0.
  - **Keys.** none. Medium.
  - **Deps.** #8751, #9020.
- **δ #9158 — `realization_executor/`: typed per-op outcome, terminal functions, one commit/rollback owner.**
  - **Work.**
    - §5.3–§5.4 and D6/D7.
    - Delete `execute_realization_ops`; the four drivers call `execute_realization`, and their `with_*` chains stay until η.
    - Move the 36 DispatchTestState scenarios and the 3 long-chain tests onto `execute_realization` in `realization_executor/tests.rs`.
    - Move the INV-BUILD-3 pointer.
  - **Signal.** B-δ1–B-δ5; rows `engine-free-realization-executor` at 0 and `executor-call-site` at 4; the heuristic 2/14 targets of §5.4.
  - **Keys.** fk-53997e96ef7c. **High.**
  - **Deps.** β₁, β₂, γ, #8687, #8747, #8750, #8992, #8993, #8995. Out-of-batch dependent: S16 β #9092 ← δ.
- **ε #9157 — The realized-terminal oracle (R3).**
  - **Work.**
    - §5.6 and D8.
    - Re-point S10 ι₅/ι₆'s owner source and S12's context entries to `RealizedTerminal`.
  - **Signal.** B-ε1, B-ε2; row `node-outcome-read` at 4.
  - **Keys.** none (`x_finding_part` fk-17e2aa81f6d6). Medium.
  - **Deps.** #9043, #9017, #9021, #8391, #8995, #8998.
- **ζ (#8534, adopted) — `project_realization_read_handle` has one home.**
  - **Work.**
    - The Sdf/Voxel arm and the absent-node arm go through `memoize_projection` / `degrade_projection`. They resolve the owner from the oracle entry (ε) with a `&mut` resolver, because densify takes `&mut self`.
    - engine_build's redispatch pre-tessellation goes through realization_content's API for both its `.get(..).is_some()` skip check and its insert, so engine_build names the store nowhere.
    - Delete the three stale `#[allow(dead_code)]` and the stale header and doc lines.
  - **Signal.** B-ζ1, B-ζ2; row `projection-store-reach` at 0.
  - **Keys.** fk-9e6d548589d8. Low.
  - **Deps.** ε, β₁, #8998.
- **η #9159 — `SurfacePolicy` and the one walk; four drivers become table rows.**
  - **Work.**
    - §5.1–§5.2, §5.5 (the sink with `records_outcome`), D1–D3 and D9.
    - `tessellate_from_values` and the driver loops are deleted.
    - `distance_between_placed` becomes the R2 query.
    - `RealizationOpsInput` is built at the one site with every field named.
    - The selector gate's walk-local map replaces `realized_reprs_snapshot`, which is deleted, for the op gate, `hydrate_value_cell_in_loop` and `run_post_processes` (D2, §1 item 6).
    - The success-path `NodeId::Realization` entry write moves from `post_process_geometry_handle_cells` into the sink (R4).
    - Rename `realize_for_check` to `realize` and update its callers: reify-cli `main.rs`, `relate_solve`, the tests #8333 lists and the reify-lsp doc. This subsumes #8333.
  - **Signal.** B-η1–B-η7; rows `realization-ops-knob` and `tessellate-outcome-discard` at 0, `schedule-walk-block` at 1, `executor-call-site` at 1, `node-outcome-write` at 2, `node-outcome-read` at 3, and `engine-free-surface-policy` at 0.
  - **Keys.** fk-f7e4716af878. Subsumes #8333. **High.**
  - **Deps.** α, δ, ε, #9043, #9036, #9038, #9046, #8872, #8861, #9017, #7270, #8995.
- **θ #9160 — One sink on every surface: tessellate records the node outcome.**
  - **Work.**
    - Delete `records_outcome` (D9). Every surface records its node outcome through the sink. The `NodeId::Realization` entry stays a Realize post-process write (R6).
    - Delete the #3437 symmetric-write comment.
  - **Signal.** B-θ1, B-θ2, B-θ3; B-ε1 re-run green.
  - **Keys.** fk-17e2aa81f6d6. Medium.
  - **Deps.** η, ε.
- **ι₁ #9161 — `realization_seeding.rs`.**
  - **Work.** Move the P2 partition (§5.8). γ's closure points at the moved `realize_sub_override_handles`.
  - **Signal.** B-ι1; row `engine-free-realization-seeding` at 0.
  - **Keys.** none (`x_finding_part` fk-53997e96ef7c). Medium.
  - **Deps.** η, #8872.
- **ι₂ #9162 — `realization_demand.rs`.**
  - **Work.** The P3 partition as free functions over explicit arguments (§5.8). The walk's `WalkDemand` is built from it.
  - **Signal.** B-ι1; row `engine-free-realization-demand` at 0.
  - **Keys.** none (`x_finding_part` fk-53997e96ef7c). Medium.
  - **Deps.** η.
- **ι₃ #9163 — `geometry_post_process.rs` and `build_export.rs`.**
  - **Work.** Move P4's explicit-argument items and the export legs (§5.8). Re-point `engine_constraints`' DFM imports.
  - **Signal.** B-ι1; rows `engine-free-geometry-post-process` and `engine-free-build-export` at 0.
  - **Keys.** none (`x_finding_part` fk-53997e96ef7c). Medium.
  - **Deps.** θ, ι₁, ι₂, #8872.
- **ω #9164 — PRD close.**
  - **Work.**
    - Measure engine_build.rs and engine_build/tests.rs against heuristic 14 (D12). If either is still above the alarm, record the named partition and its symptoms in the commit.
    - Set this PRD's Status to `SHIPPED` with the landed leaf ids, the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and apply the same header to the capability manifest.
  - **Signal.** The committed header.
  - **Keys.** none. Medium.
  - **Deps.** every other leaf.

**Gate.** δ and η are high, so the trigger-chain gate #8612 depends on both.

## 10. Measurement record (dated: main `d4cd598fee`, 2026-10-14)

Rules are inline. Six scout seats measured; the lead re-ran the decisive caller counts.

**10.1 Driver axes** (scout A; read from the four call sites).
- `build_snapshot` @3535 (530 lines), `build_with_geometry_output` @4160 (1,060), `distance_between_placed` @5695 (217), `tessellate_from_values` @6595 (520).
- `with_*` calls per driver: 10, 11, 9, 9.
- Cross-let seeding only in build_with_geometry_output @4526. Reset positions: build_snapshot @3548 (first statement); Build @4221 (after the relate pre-pass @4202, before `check` @4241); TessellateRealizations @6233 (after `check` @6189); TessellateSnapshot @12333 (top); distance none.
- Both `build_steps` blocks (@4548–4588, @6767–6811) are token-identical after renaming `demanded_rids_build`→`demanded_rids` and `unified_pass.as_ref()`→`unified_pass` (python strip + difflib, 2 residual hunks).

**10.2 Callers** (rule: non-comment `git grep` over `crates/*/src` and `gui/src-tauri/src`, excluding `tests.rs`, `*_tests.rs` and `/tests/`; re-run by the lead).
- `.build_snapshot(`: 0 production, 22 test lines.
- `distance_between_placed(`: 0 production, 10 tests (cross_kernel_handoff 4, sub_placement_assembly_gate 3, sub_placement_surfacing 3).
- `tessellate_realizations`: reify-cli main.rs:977 and reify-test-support helpers.rs:1908.
- `tessellate_snapshot`: gui engine.rs:3965.

**10.3 Executor anatomy** (scout B; python brace match).
- Span 7445–9748: 2,304 lines, 1,199 `//` lines, max indent 16 levels.
- Op loop 7676–8943; `Ok(geom_op)` arm 7715–8917; conversion recipe 7940–8569 (630 lines); labelled `'convert` loop 8220–8543.
- Rollback 8983–9003; success branch 9004–9742; VolumeMesh terminal 9312–9554; Voxel terminal 9555–9624.
- One `return` (7485, cache hit), five op-loop `break`s, one inner `continue`, no `?`.
- `intermediate_cache_inserts`: one push (8520), one consumer (8995).
- Tests: 36 of 89 `#[test]` fns in engine_build/tests.rs use DispatchTestState (18 `run`, 14 `run_demand`, 4 as a bare container), and 3 long-chain tests hand-build the structs.

**10.4 Scheduler** (scout C).
- Definition: engine_fixpoint.rs 28–70. Field: lib.rs:527. Set: engine_admin.rs:279.
- Production reads: engine_build.rs 6047 and 5131.
- Pins: 317 lines in 33 files outside docs/prds, matching `BuildScheduler|set_build_scheduler|REIFY_BUILD_SCHEDULER|LegacyMultiPass`. Of those, 21 are test files: 15 pin-only, 6 compare or depend.
- 0 hits in scripts/, gui/, tests/infra or CI.
- First commit a1c5099a0c (2026-06-11); default flip 2a7c699849 (2026-06-23).

**10.5 Partition and cycle** (scout E; partition-level model reproducing the 24/5 baseline).
- Production lines: P1 executor 4,433; P2 seeding 605; P3 demand 757; P4 post-process 2,719 (20 of 25 items explicit-argument); P5 entries 3,922; P6 volume mesh 711; P7 IR helpers 838.
- 28 `engine_build → geometry_ops` sites and 1 `geometry_ops → engine_build` site (geometry_ops.rs:12907).
- Removing the latter splits the 24-SCC into [engine_admin, engine_build, engine_edit, engine_eval, realization_staleness, resolve_order] (6), [compute_persist, engine_compute, shell_extract_compute] (3) and [cache, deps] (2), plus the unchanged 5.
- `assert-module-scc.py check` is OK at d4cd598fee.

**10.6 Collisions** (scout F; 2,286 non-terminal tasks paged; 251 name an S2 file; re-measured by the pass-2 prep seat, whose full per-task check lists are the source for the R5 writes).
- **Present-checks S2's moves flip; R5 widens them at pass 2** (old paths kept, the destination appended):
  - #6745, #6923, #6171, #6168: placed-walk constructs in geometry_ops.rs → `placed_walk.rs` (γ).
  - #6168, #6923: `achieved_repr_tol: &mut std::collections::BTreeMap<String, f64>` matches only a `tessellate_from_values` parameter, which η deletes. The pattern widens to `(std::collections::)?BTreeMap` with paths + `placed_walk.rs`.
  - #6171, #6923: `fn compute_tessellation_budgets` → `realization_demand.rs` (ι₂).
  - #6175: `Self::effective_tessellation_tolerance` becomes a free fn, so the pattern widens to `([A-Za-z_]+::)?effective_tessellation_tolerance` with paths + `realization_demand.rs`.
  - #6177, #6923, #6170: `fn build_outputs_with_result` / export constructs → `build_export.rs` (ι₃).
  - #6170: `engine\.realize_for_check\(&compiled\)` in reify-cli → `engine\.(realize_for_check|realize)\(&compiled\)` (η's rename).
  - #8395: the `ImplicitSolid` classifier arm → `geometry_op_lowering.rs` (β₁).
  - #8396: `ImplicitUnsupportedOp` → `realization_executor/` (δ).
- **Absent-checks that would pass vacuously when their subject moves; R5 widens them too:** #8400 (`_diagnostics:` in `post_process_derived_lets` → `geometry_post_process.rs`) and #6610/#6926 (`one level of override depth only` → `realization_seeding.rs`).
- **Unaffected:** #6740 and #6898's `pub fn tessellate_snapshot\(` (an entry surface that stays), checks whose paths are already the `crates/reify-eval/src` directory (#8391, #8395's and #8396's other rows, #8401), #8992, #8687 and #8872's absent-checks.
- **Not S2's:** the reset-destructure checks (`capture_repr_tol: _`, `achieved_repr_tol\.clear\(\)`) on #6740, #6741, #6898, #6923 and #6171 flip when S1 ε #9043 moves `reset_per_build_state` (cross-reference line). #6171/#6923's `kernel\.measure_mesh_deviation\(placed_id` and `kernel\.tessellate\(placed_id, budget\)` are already stale today (the code says `placed.id`). R5 widens them verdict-preserving, and the staleness goes to the lead (cross-reference line).

## 11. Rulings (Leo, 2026-10-15, this session) and open questions

**Rulings.**
- **R1 — #4727.** **Adopt it as S2's first leaf, re-scoped to this PRD, landing before S1 α #9036**, so S1 never moves or migrates the doomed enum (D4, D5; leaf α).
- **R2 — production-orphaned drivers.** **`build_snapshot` is a policy row; `distance_between_placed` runs the Build-policy walk without export and asks the owning kernel** (D3; leaf η).
- **R3 — write-back coherence.** **One sink writes the node outcome on every surface; the handle oracle stays build-only, and each entry carries `{KernelHandle, repr}` that every handle reader uses** (D8, D9; leaves ε, θ).
- **R4 — realization freshness (critic F1).** **The sink owns the `NodeId::Realization` cache entry: `Final` with its trace on success, `Failed` on failure. The write moves out of `post_process_geometry_handle_cells`, which keeps only the oracle and the geometry-handle cells** (D9; leaf η). Its "every surface" half is narrowed by R6.
- **R6 — tessellate freshness deferred (focused re-verify, N7).** Geometry-handle value cells derive freshness at eval time from their Realization's entry, and the GUI runs eval before tessellation, so a tessellate-written entry would leave geometry cells `Pending` after every edit. **θ records only the node outcome on tessellate surfaces; the entry stays a Realize post-process write; follow-up #9165 (← θ #9160 and S4 γ #8861) owns GUI realization freshness and the GH-cell re-derivation it needs, and first measures whether GUI sessions after an export already show stuck-`Pending` cells** (D9; leaf θ; B-θ2, B-θ3).
- **R5 — other PRDs' delivered checks.** **Pass 2 widens the §10.6 checks to S2's new module paths, with three writes each (`.yaml`, `.md` twin, task metadata),** as S13's pass 2 did.

**Lead decisions (recorded, not relitigated).**
- **L1.** Entries keep their prelude and reset position (D1). This preserves S1 ε's load-bearing reset asymmetry and the relate re-entrancy.
- **L2.** Crate-root modules outside the residual SCC rather than `engine_build/` children (D11). Children lower no INV-ARCH-1 row (§2.6).
- **L3.** η's declared behaviour changes are §1 items 4, 5 and 6. Distance gains Build's reset and records (B-η3, B-η7), and the gate rule reaches tessellate and `build_snapshot` (B-η6). Build itself is byte-identical. Everything else in η is behaviour-preserving. The tessellate write-back is θ's separate, user-visible change.
- **L6.** η subsumes #8333 (rename `realize_for_check` to `realize`). Other sidecars mention `realize_for_check` only in binding prose, never in a mechanical check.
- **L4.** No `REIFY_BUILD_SCHEDULER` deprecation diagnostic: no in-repo setter exists, and the variable leaves S1 α's knob table.
- **L5.** Superseded by R5.

**Critic fold record** (fable/high, 2026-10-15; verdict "ship-after-fold"). Each finding was reproduced in code before folding.
- **Blocker F1.** The success-path freshness write was fused with the build-only oracle insert → R4 (D9, §5.5).
- **Majors.**
  - F2: the gate's oracle view would gate tessellate results with stale build reprs → a walk-local map (D2, §1 item 6, B-η6).
  - F3: the tolerance row matched its own single home → the `:`-anchored alternation.
  - F4: the optional-schedule row counted 0 today → the module-path spelling (2 today).
  - F5: the projection-store row was already at its ceiling (the second writer spans two lines) → `projection-store-reach` in engine_build.rs (3 → 0).
- **Minors.**
  - F6: #4727's orphan items decided in D4.
  - F7: three non-crate scheduler mentions added to α.
  - F8: golden comparison basis stated in D5.
  - F9: §2.2's coupling list corrected.
  - F10: resolved by F2 (the gate keeps today's map type; ε no longer touches it).
  - F11: #8333 folded into η (L6).

**Fold-verifier record** (opus/high, 2026-10-15; verdict "needs-another-pass", folded here). The critic's F3, F4, F5 and F7–F10 were confirmed closed. Partial folds and new defects:
- **N1 (F2).** §1 item 6 misstated today: eval rebuilds the graph with `BRep` defaults on every build. Rewritten as the cold-build and tessellate deltas, with B-η6 comparing against pre-η.
- **N2 (F1).** The cone-hash write is outcome-independent today. §5.5 now writes it on every outcome, with the type `[u8; 32]`; "only writer" is scoped; B-η7 compares it.
- **N3.** S4 γ #8861 moves the same realization write onto `record_node`. η ← #8861; §5.5 names the early-cutoff primitive; #6422 is a sibling.
- **N4.** implicit-solids γ #8391 widens `realization_handles` first. ε ← #8391, ε's check re-keyed to a present-check on `RealizedTerminal`, and a pass-2 note on #8391 asks for `KernelHandle` in one map.
- **N5.** #6168/#6923's `tessellate_from_values` parameter check added to §10.6.
- **N6.** S16 β #9092 edits the executor: #9092 ← δ, and `ExecutedRealization` gets its `morph_outcome` slot.
- **N7.** The GUI freshness cascade was declared and B-θ3 extended; the focused re-verify then showed the cascade has no production walk, so R6 supersedes both.
- **Minors.**
  - M1: six tests and rustdoc links.
  - M2: D13 wording; #4691 and #8333 cancelled at pass 2.
  - M3: ζ names the get.
  - M4: S1 note on distance's reset.
  - M5: L3.
  - M6: `arg_contains_cross_sub_geometry_ref` is S3's.
  - M7: §8 sibling rows.
  - M8: `terminal` = the last step handle.
  - M9: η/θ checks added; `build_snapshot` named.
  - M10: the trace's source named.

**Focused re-verify record** (opus/medium, 2026-10-15; verdict "fix-then-commit", folded here). N2, N3, N4, N6, M8, M10 and every ε/η check were confirmed.
- **V1 (N1).** The post-loop pass already gates today. §1 item 6 now states today's in-loop/post-loop split, η keeps it as the walk's rule, and B-η6 asserts Build byte-identical.
- **V2 (N7).** The cascade has no production walk, and dirty marking is dirty ∩ demand → R6.
- **V3.** The η binding was reworded.
- **V4.** The entry is written for named realizations only, as today (§5.5).
- **#9092.** Its text is amended at pass 2 to name `execute_realization` / `ExecutedRealization.morph_outcome`.

**Open questions (tactical; decide in the named leaf).**
1. **VolumeMesh cache-hit parity** (δ, INV-BUILD-3). Today a hit returns no `MeshedVolume`. Suggested: δ pins today's behaviour (B-δ4). If `record_and_resolve_terminal` then observably differs from the miss path, δ makes `CacheHit` carry the terminal's `MeshedVolume` from the gmsh handle.
2. **Golden format** (α). Suggested: one JSON projection per corpus case under `crates/reify-eval/tests/harness_cache/goldens/`, generated by a `--bless`-style env-gated test and reviewed in the diff.
3. **`OverrideRealizer` signature** (γ). Suggested: today's nine arguments, unchanged; instantiation-value-flow may widen it later.
4. **Module names** (any leaf). An implementing architect may rename a §5.8 module when heuristic 1 suggests better, and records the reason in the commit. The purpose sentence is binding; the name is not.
