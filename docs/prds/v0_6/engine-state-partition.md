# PRD — Engine state partition (reify-eval): one constructor, sub-structs by lifetime class

**Status:** authored pass 1, 2026-10-14 — review-all program `review-all-reify-20261010`, stream **S1** (wave 3, spawn mode, size L, risk high; `plans/review-all-program-reify-20261012.md`). Leo ruled R1–R4 in-session (§11). An adversarial critic pass (fable/high: 2 blockers, 6 majors, 14 minors, each verified in code or in the task record) is folded, and an opus/high fold-verifier's second pass (5 partial resolutions, 14 new defects, 2 of them blockers) is folded on top (§11 fold record). Not yet decomposed. v0_6. Approach **B + H** (§5 contract, §6 two-way boundary tests; rationale D15).
**Invariants owned:** **INV-FEA-1** — `proposed` → `enforced(type+lint+test)` (leaf γ₂). Its registry row names `Engine::new_production`, which does not exist. `register_production_compute_fns` (reshaped by S5 κ₁ #8999) and this PRD's one constructor implement it. **INV-BUILD-1** — stays enforced; its mechanism is corrected from the 68-field `reset_per_build_state` destructure to `PerBuildState::reset(BuildSurface)` over PerBuildState's own fields plus a surface-sequence test (leaf ε). This stream is the single owner of both status cells (program G4). Each flip writes resolving pointers in S24's grammar (`reify-audit-typed-findings-and-registry-lanes.md` §5.7): inline in the Enforcement cell before S24 ξ #8821 lands, in the Pointer column after it (§5.11). Every leaf that extracts a construct names its row in `tests/infra/adoption-ratchet.manifest` (program invariant candidate 5). Every new module stays outside the engine module SCC (INV-ARCH-1).
**Finding keys closed (13):** fk-800d61acd5d0, fk-ba15d44904cb, fk-d8fd9ed9be89, fk-cb6c853cb648, fk-e9536a3edbe7, fk-8250a05a9a10, fk-fb8cf3e787b9, fk-d2c64e9d3868, fk-4fa7bda0085b, fk-4db43199eda2, fk-06103e5f06eb, the prior-instrument canonical `language-pipeline|crates/reify-eval/src/lib.rs::engine|h11`, and fk-3aeb721eaaa6, which is filed as #8564 and is referenced, not re-filed. §9 assigns each key to exactly one closing leaf, or to the filed task #8564. A leaf that only contributes to a key carries it as `x_finding_part`.
**Code anchors** verified against main `988f4522b9` (2026-10-14). Since the review anchor `a030ee7528`, #6308 (export and terminal-owner routing, `kernel_owner.rs`), S3 γ #8675 (dead `cache.rs`/`deps.rs` peers) and #8753 have landed. No S1 symbol changed in substance, and every count below was re-measured at `988f4522b9` by this session's scout seats (§10, rules inline). Cite-by-symbol; re-locate lines at implementation time. Line numbers appear only in the dated §10.

---

## 1. Consumer and user-observable surface (G1, G2)

**Consumers.**
- **Every front end that builds an Engine.** The CLI (`reify check | build | eval | report | explain`, `reify test`), the GUI (`EngineSession`), the LSP (analysis and diagnostics), and the CLI MCP context until #6665 deletes it. Today there are 18 production construction sites, 6 constructors and a posture assembled by setter calls that differ from site to site (§2.2, §10.3).
- **Downstream program streams.**
  - S2 (realization pipeline, wave 3) builds its `SurfacePolicy` from `BuildSurface` and consumes `PerBuildState` (ε) and `EngineConfig` (α). It retires `BuildScheduler::LegacyMultiPass`, and with it `EngineConfig.build_scheduler`.
  - S27 φ #8776 is held on γ₁ and γ₂ and moves the CLI's preset into `reify-cli/src/engine_config.rs`.
  - S12's `Engine::measure` and `capture_measurement` read PerBuildState's producer stores. Its `MeasurementContext::measured_for_test` replaces `set_achieved_repr_tol_for_test`.
  - S5 κ₁'s injected `ComputeTargets` becomes a field of the Registration posture.
  - S24 ξ #8821 records this stream's owner leaves for INV-FEA-1 and INV-BUILD-1.
- **The merge gate.** The new adoption-ratchet rows (§5.13), `module_scc_ratchet` (INV-ARCH-1), the rewritten compute-trampoline registration lint (§5.12), `version_id_discipline_gate` (INV-BUILD-2, kept green) and PREGISTRY over the two status cells.

**In-engine seam catalogue (overlay G1).** No new seam. ComputeNode dispatch (§3.4) registration moves into the constructor. Multi-kernel dispatch (§3.3) membership becomes a constructor input (S10's `KernelSet`); selection stays S10's `dispatcher::select`.

**What users see.** These are the declared behaviour changes; everything else is behaviour-preserving and observed through the oracles in §6.
1. **The GUI uses the persistent cache (R3).** GUI FEA, modal and other compute results hit and populate the same on-disk cache that `reify eval` writes. One layered resolver (env, `~/.config/reify/config.toml`, project `.reify/config.toml`) picks the directory for both front ends, and the GUI's startup sweep sweeps that directory.
2. **The GUI's "undef because …" hover is right after a slider tick (R4).** The cause map is re-classified on the edit and cached paths. A failed solve on those paths reports `SolveFailed`, not `AwaitingSolve` (#7038, subsumed).
3. **No stale per-call outputs in the GUI.**
   - After a slider edit followed by a whole-file reload, the GUI no longer shows the previous edit's demand-prune record.
   - A realization that was dispatched but whose ops failed no longer replays a stale value as `Final` in the derived-cell overlay. The proxy's own comment records this known defect.
4. **`reify eval`'s geometry arm declares the OpenVDB kernel, as `reify build` and `reify check` do.** Today the eval geometry arm never calls `ensure_openvdb_kernel`. B-γ3 pins the eval output on the isosurface fixture. The leaf records today's output before changing anything, and claims a user-visible fix only if that pre-change run shows a missing-kernel diagnostic.
5. **The no-trampoline warning names its posture.** On a deliberately trampoline-free engine, `NoRegisteredComputeTrampoline` is a Warning that names why the engine registers nothing (for example, "`reify check` runs without compute trampolines"). On a registered engine it is an Error. The severity reads the declared posture, never registry emptiness.
6. **Test suites stop reading the developer's shell environment (R1).** `REIFY_BUILD_SCHEDULER`, `REIFY_MESH_CONTRACT` and `REIFY_WARM_STATE_BUDGET_BYTES` no longer leak into test engines.
7. **An unrecognized engine knob is reported, not silently defaulted (INV-SF-3).** An env value that does not parse keeps today's fallback and adds a coded Warning.

**Leaf signals (G2, detailed per leaf in §9).** The user-visible leaves (α, γ₁, ζ, θ) name a CLI or GUI-path observation. Refactor leaves name three things:
- (a) an adoption-ratchet row at its target;
- (b) a behaviour pin on a real surface;
- (c) the regression floor: `reify eval` and `reify check` over `examples/best_practices/*.ri` and the FEA/modal examples print byte-identical output before and after the leaf.

## 2. Problem (re-verified at `988f4522b9`)

1. **A 68-field god struct whose lifetime classes are recorded twice** (fk-800d61acd5d0, fk-ba15d44904cb, the h11 canonical; heuristics 5, 6, 9, 11, 13).
   - **Layout.** `pub struct Engine` sits at the crate root with 68 fields, 2 of them cfg-gated. 701 of its 775 lines are comments. There are 24 `impl Engine` blocks in 17 files, and satellites read private fields directly (engine_admin 50 fields, engine_eval 35, engine_build 24, engine_edit 23).
   - **The double record.** Each field's class appears in its doc comment and again in `reset_per_build_state`'s exhaustive destructure, which names all 68. Only 8 are ever reset: 5 on every surface, 1 on the build surfaces, 1 on the tessellate surfaces, 1 on `Build` only. The other 60 are bound `_`. The destructure turns a forgotten classification into a compile error, but a wrong one still compiles (#5415).
   - **The two records already disagree.** `relate_static_facts`' doc and its constructor comment say "reset on every surface"; the code resets it on `Build` only. `distance_between_placed` bumps the dispatch tallies without calling the reset.
2. **No single configuration value** (fk-4fa7bda0085b; heuristics 3, 7, 11).
   - **Env reads at different moments.** `REIFY_BUILD_SCHEDULER`, `REIFY_MESH_CONTRACT` (through reify-ir `MeshContractMode::from_env`) and `REIFY_WARM_STATE_BUDGET_BYTES` are read in the shared constructor body. `REIFY_LONG_CHAIN_THRESHOLD_MS` is re-read at 4 build entry points.
   - **A dead manifest route.** `with_registered_kernels_and_manifest(.., Some(..))`, kernel pins and `set_warm_state_budget` have no production caller.
   - **Setter-assembled posture.** Each site opts into the persistent cache, compute trampolines, repr-tol capture, OpenVDB, the solver and undef-cause capture by setter calls, and the mix differs (the posture matrix in §10.3).
   - **GUI/CLI cache-dir divergence.** The GUI never sets a persistent cache dir. Its startup sweep resolves a directory with `user_config: None, project_config: None`, while the CLI's `resolve_cache_root_layered` loads both config files, so the two can disagree.
3. **The kernel set is decided by link-time side effects** (fk-4db43199eda2; heuristics 7, 9, 11).
   - **Selection.** `with_registered_kernel` instantiates the lexicographically least B-rep-capable `inventory` registration.
   - **Anchors.** `extern crate … as _` anchors: CLI 3, GUI 1, LSP 0. reify-eval tests carry 17 code anchors in 12 files, plus the reify-eval `Cargo.toml` "dead-strip invariant" prose that forbids further references.
   - **Test override.** The gated `test_registry_override` field is read at 3 build entry points, but not at the 2 tessellate ones.
   - S10 owns the `KernelSet` type (ι₃ #8992) and the build-routing registry reads (ι₄ #8993). This stream owns the construction half.
4. **The compute-registration posture is inferred** (fk-06103e5f06eb; heuristics 10, 11).
   - **Severity rule.** `Engine::soft_no_trampoline_diagnostic` picks Warning or Error by `compute_registry.fns.is_empty()`, so an engine nobody registered is indistinguishable from a deliberate opt-out.
   - **Lint coverage.** `scripts/check-compute-trampoline-registration.sh` `KNOWN_SITES` pins 3 of the 18 production sites, and checks "registers the bundle", not "declares a posture".
   - **MCP context.** `mcp_context.rs::ensure_engine` registers nothing, yet sets a persistent cache dir for compute dispatch that a trampoline-free engine never reaches.
   - INV-FEA-1 names a nonexistent `Engine::new_production`.
5. **Per-call outputs live at instance lifetime** (fk-e9536a3edbe7; heuristics 5, 7, 12).
   - **The fields.** `last_eval_set`, `last_demand_prune_measurement`, `last_dispatch_count`, `last_dispatch_count_by_realization` and `last_changed_realizations` hold per-call results.
   - **GUI reads.** The GUI reads three ungated getters at 5 production sites (commands.rs `engine_state_json`, `demand_dispatch_json`; engine.rs `set_active_fea_case`, `build_gui_state`).
   - **The stale prune record.** `last_demand_prune_measurement` is written only by `edit_param` and is never cleared.
   - **The `_post_refresh` suffix.** It is "load-bearing" only because `build_gui_state` re-ran tessellate and overwrote the tally.
   - **The mesh-key proxy.** `surface_geometry_derived_cells` reconstructs "which realizations ran this pass" by parsing delta mesh `entity_path` keys (`dispatched_entities`).
6. **Undef causes are joined against a different snapshot** (fk-8250a05a9a10; heuristics 5, 7).
   - Only `eval()` writes `last_undef_causes`. `edit_param`, `edit_check` and `eval_cached` never do, so the GUI preview path (`preview_parameter` → `edit_check` → `build_values` → `trace_undef_causes`) traces new values against pre-edit origins.
   - #7038: the `SolveFailed` capture is wired on 2 of 6 solve-result consumers.
7. **Five parallel per-purpose maps kept in lockstep by hand** (fk-fb8cf3e787b9; heuristics 7, 8, 10). All are confirmed at main:
   - the 4 activate→rebuild caller pairs;
   - the rebuild re-inlined in `deactivate_purpose`;
   - `active_purpose_let_cells` cleared at 0 sites;
   - `deactivate_purpose`'s early return before the let-cell removal;
   - `eval_cached` clearing `active_purposes` only when bindings are non-empty;
   - the stale "Until task γ lands" doc.
   No test inspects the maps' agreement.
8. **Test seams that fix internal shapes in place** (fk-d8fd9ed9be89, fk-cb6c853cb648; heuristic 9, the internals-reaching-tests stance).
   - **Gated accessors.** Engine carries 28 `pub fn` items gated `#[cfg(any(test, feature = "test-instrumentation"))]`. The feature is enabled only as a dev-dependency (reify-eval itself, reify-lsp, reify-eval-fea-tests, gui/src-tauri).
   - **Call counts.** `cache_store` 224 calls, `cache_store_mut` 119 (59 seed the cache through `put`/`record_*`, 54 mutate it another way, 0 are read-only), `set_build_scheduler` 45, `realization_cache` 36, `last_dispatch_count` 33, `warm_pool` 27, `warm_pool_mut` 25.
   - **Counters with no production reader.** The always-present observation counters (`last_guard_phase_group_evals`, `geometry_revalidation_slow_path`, `last_dispatch_count`, and the five S4 deletes) have no production reader.
   - **`morph_source`.** Its `#[allow(dead_code)]` comments are stale: engine_build calls `store_morph_source` and reads the field directly, so only the getter is test-only.
9. **Snapshot doc wording** (fk-d2c64e9d3868, weakened; heuristic 1, comments).
   - The type says "immutable". The `values` field claims READ-boundary revalidation, while `read_value_revalidated`'s own doc says it has no production caller.
   - The skeptic note stands: the in-place mutation is disclosed in the field doc. This is a doc correction at weakened weight.

## 3. Pre-conditions and substrate (G3, G6)

**Upstream filed leaves** (all pending at `988f4522b9` unless noted; `get_task` each at dispatch, since several are in flight):
- **S4** (`cell-eval-transaction-closure.md`):
  - δ #8862 deletes `edit_source`, `last_role_flip_probes` and `last_diff_value_cells`.
  - λ #8869 deletes the three override/unknown-structure counters.
  - τ #8872 adds the crate-root `eval_env`, makes the base builder private and deletes `Engine::cell_eval_ctx`.
  - η #8864 adds `param_override.rs`.
  - Every leaf here that edits the Engine struct depends on δ, λ and τ (program cross-reference).
- **S5** (`analysis-stratum.md`):
  - ε #8994 defines `ComputeTargets` (in `reify_compute_contract::target`).
  - κ₁ #8999 adds the `targets` parameter, `register_production_compute_fns(targets: ComputeTargets, morph)`, `run_tests(targets, …)` and `reify_analysis::PRODUCTION_TARGETS`.
  - ε #8994: the lint's `EXEMPT_DEFINITION_FILES` and negative-pass half names (S5's carve-out, Leo S5 R4).
  - ζ #8996 and ο #9007 move `shell_gui_mesh_data` out of engine_admin.rs.
- **S10** (`kernel-interface-typed-seam.md`, decomposed 2026-10-14):
  - ι₃ #8992 adds `KernelSet` keyed by `KernelId`, deletes `repr_capable_kernel` and re-keys `with_test_kernels_and_registry`.
  - ι₄ #8993 replaces the six build-routing `collect_registry()`/`registry()` reads with the set's own descriptors.
  - β₃ #8957 drops `cfg(has_openvdb)` from the anchor lines.
- **S3** (`dependency-invalidation-stratum.md`):
  - β #8687 moves the predicates below the engine SCC. Before it lands, reify-eval's baselined SCC is the 24-module one that contains `cache`, `deps`, `demand`, `journal`, `warm_pool`, `engine_compute` and `engine_fixpoint`, which are the home modules of the payload types every new sub-struct names. So α depends on #8687, and every later leaf inherits the edge (D5).
  - δ #8691 repoints `trace_undef_causes` onto `DependencyGraph`.
  - ζ #8699 deletes `propagate_freshness_only` and the freshness-walk tests, which removes 29 `cache_store_mut` sites.
  - κ #8680 adds `set_observed_demand` (in progress).
- **S12** (`check-surface-subject-and-measurement-context.md`, decomposed in `891e35ae6e`):
  - ε1 #9017: check-time passes read only `MeasurementContext` (its row C6 → 0); D5's `measured_for_test` replaces `set_achieved_repr_tol_for_test`; D8's `measure` / `capture_measurement`.
  - ε3 #9021: per-entry hashes, plus an engine identity and a kernel generation (new Engine state, homed in §5.3); `reset_geometry_for_reload` also clears `realization_handles`.
  - ζ1 #9022 and ζ2 #9023: one `CheckEnginePlan` for `cmd_check`, so its construction sites fall from 4 to 1.
- **S25** (`gui-backend-engine-modules-events-tool-contract.md`):
  - α #8937 splits `engine.rs` into modules (the persistent-cache sweep moves to `engine/persistent_cache.rs`).
  - θ #8944 rewrites `surface_geometry_derived_cells` to key on typed `ValueCellId`s.
- **#6665** deletes `mcp_context.rs` and `cmd_mcp_server`. γ₁ handles both landing orders, so there is no edge (§9 γ₁).

**Substrate verified at main.**
- **Config resolution.**
  - `reify_config::cache::{CacheResolverInputs, resolve_cache}` exist.
  - The layered loader (`resolve_cache_root_layered`, `load_config_or_none`) lives in `reify-cli/src/cache.rs` (α moves it down).
  - The knob parsers are already pure: `BuildScheduler::from_env_value`, `MeshContractMode::from_env_value`, `WarmStatePool::from_env_value`. Only their `from_env` wrappers read the process.
- **R4's premise.** The snapshot graph's `ValueCellNode` carries `default_expr`, so the edit paths can re-classify undef origins without the module.
- **The manifest route.** `with_registered_kernels_and_manifest` returns kernel-pin diagnostics through `kernel_pin_diagnostics`, a pure function.
- **Test plumbing.**
  - The adoption-ratchet engine is live (`tests/infra/adoption-ratchet.manifest`, `test_infra_adoption_ratchet.sh`); rows go there, with no new infra member.
  - `test-instrumentation` is enabled only through dev-dependencies (reify-eval itself, reify-lsp, reify-eval-fea-tests, gui/src-tauri), so a gated item is unreachable from every non-test build. ι's compile-time size check relies on that.
  - Before γ₁ gates the legacy constructors, four crates need the feature reachable from their tests:
    - reify-compiler, reify-cli and reify-mesh-morph construct engines in their tests but enable no reify-eval feature. γ₁ adds the feature to reify-compiler's existing reify-eval dev-dependency, and adds `[dev-dependencies] reify-eval = { workspace = true, features = ["test-instrumentation"] }` to reify-cli and reify-mesh-morph, which have no such entry today.
    - reify-test-support constructs engines in library code (`helpers.rs`, `fixtures.rs`). γ₁ adds `reify-eval/test-instrumentation` to its `eval-helpers` feature; reify-test-support is a dev-dependency only.
    - Under the workspace's `resolver = "2"`, dev-dependency features do not reach a non-test build.
- **Existing names this PRD builds on.**
  - `MorphRegistration::{Enabled, Unavailable}` (in `compute_targets/mod.rs`).
  - `version_id_discipline_gate.rs` pins INV-BUILD-2's single allocator.
  - The INV-FEA-1 and INV-BUILD-1 rows are quoted in §10.6.

**Premise corrections (G6).**
- `Engine::new_production` does not exist.
- `set_morph_producer` and `set_warm_pool_budget_from_manifest` do not exist; the real names are `register_morph_producer` and `set_warm_state_budget`.
- `sweep_persistent_cache_at_startup` is a free function, not an Engine method.
- **Repoint owner.** The `trace_undef_causes` repoint is S3 δ #8691, not ζ #8699.
- **No `preview_*`.** reify-eval has no `preview_*` method; the GUI's `preview_parameter` calls `edit_check`.
- **`last_changed_realizations`.** It has production readers inside the same edit call, so it is a per-call output, not instrumentation.
- **The journal cannot replace the dispatch counters.** It records no dispatch event, so the counts become result fields, not journal folds.
- **S12's measurement stores stay on the engine.** S12 §8 writes "S1 moves the producer stores onto `BuildResult`/`TessellateResult`". This PRD keeps `realization_handles`, `achieved_repr_tol` and `relate_static_facts` in `PerBuildState`. The *next* call consumes them, through S12's `capture_measurement`, whose contract S12 keeps (D12; cross-reference to S12).
- **The ruled classes do not cover every field.** The six ruled classes leave 11 fields without a home: session identity, host intent, the prelude and the host hooks. D3 adds three classes from the census evidence; this is an elaboration, not a new design.
- **"Keeps the link anchors."** S10 R1 says "S1 keeps the link anchors", and β₃ #8957 "keeps the anchor lines". This PRD reads that as "owns": the production anchors are deleted once each binary names its kernels directly (D6). A note on #8957 says so, so that β₃ does not treat the lines as load-bearing.
- **No public `KernelSet` constructor.** S10 ι₃ #8992 defines none ("constructor signatures are unchanged"), and its `KernelSet::ensure` bodies keep today's `kernel_registry::registry().get(name)` read. γ₁ therefore owns two S1 additions to S10's type: `KernelSet::from_registrations(&[&KernelRegistration], primary)`, and an `ensure` that instantiates only a member the set declared (D6; S10 cross-reference).
- **Sibling PRD text stays as committed.** S12's D8 and §8, and S10's R1 and §12, keep their wording (one editor per document). The two corrections above reach their implementers through task-record notes on #8992, #8957, #9017 and #9021 written at decompose, and through cross-reference lines for the lead.

## 4. Resolved design decisions

- **D1 — One constructor for front ends and tests; tests are hermetic (R1).** `Engine::new(EngineInputs) -> Engine` is the only public constructor.
  - **Tests.** Tests start from `EngineInputs::hermetic()` — no env or file reads, no kernels, no compute registration, `SimpleConstraintChecker`, no solver, the stdlib prelude — and override what they need.
  - **No gated test constructor survives.** The legacy two-argument names exist only between γ₁ and δ₃:
    - γ₁ renames and gates them, after enabling the feature on the three crates that lacked it (§3);
    - δ₁–δ₃ migrate every test, crate by crate;
    - δ₃ deletes them.
  - **Why** (Leo, on "which gives the best long-run tests"):
    - one construction path, so tests run production's constructor with explicit values;
    - hermeticity, because 1,124 test engines today read the developer's shell env;
    - one discoverable way to write a test engine;
    - a production-posture test is one input value away.
- **D2 — `EngineInputs` names every input; none defaults silently.** The fields are `config`, `kernels`, `registration`, `constraints` (checker plus fallback solver) and `prelude`.
  - **Why five, not three.** The ruling lists config, kernel set and posture. The checker, solver and prelude are the three other inputs tests inject today (279 `MockConstraintChecker` sites, `with_solver`, 38 explicit-prelude sites), and production fixes each one.
  - **Presets live in the front ends.** Front-end presets live in their front-end crates, one function per front end (§5.1 table), never in reify-eval: at the end state reify-eval cannot name `reify_analysis::PRODUCTION_TARGETS` (S5 D2/π).
- **D3 — Ten owned sub-structs by lifetime class.**
  - The ten: `EngineConfig`, `Registrations`, `KernelSet` (S10), `EngineLineage`, `HostInputs`, `EvalArtifacts`, `PurposeState`, `PerBuildState`, `EngineCaches`, `Instrumentation`.
  - The five per-call outputs leave the Engine for the result structs.
  - Every one of today's 68 fields has exactly one home (§5.3); 5 are already deleted by S4.
  - A new field's class is chosen by where it is declared.
- **D4 — A lifecycle rule lives on the sub-struct that owns the fields it governs.**
  - The rules: `PerBuildState::reset`; `EvalArtifacts::install` replaces snapshot and causes atomically; `PurposeState::{activate, deactivate, reload}` include the infrastructure rebuild; `EngineCaches::flush_all` keeps the kernel-local caches coherent with `reset_geometry_for_reload`; `EngineLineage::allocate` is INV-BUILD-2's allocator.
  - **Privacy.** Sub-struct fields are private to their module, and satellites use named methods. A read-only borrow is allowed where a satellite only reads. No `pub(crate)` field is reached across modules for a mutation that a lifecycle rule governs.
- **D5 — Every new module sits below the engine SCC (INV-ARCH-1; S4 D13 precedent).**
  - **The rule.** A new module imports nothing in the engine SCC as baselined at the leaf's landing, and it never names `Engine`.
  - **The expected baseline.** After S3 β #8687, the baseline is the residual `{engine_admin, engine_build, engine_edit, engine_eval, geometry_ops}` plus the separate `{cache, deps}` 2-cycle (S3 OQ-5). A new module may import `cache`/`deps` without joining that cycle, because neither imports it back.
  - **The gate.** `module_scc_ratchet` stays green with no `GREW`, and a shrink lowers `scripts/module-scc-baseline.json` in the same diff.
  - **Ordering.** α depends on #8687, so no module-adding leaf lands against the 24-module SCC.
  - **`BuildScheduler`.** α moves it into `engine_config.rs`, so the config module never imports `engine_fixpoint` (S2 later deletes its `LegacyMultiPass` arm).
  - **The lint.** Because the SCC tool is blind to `Engine::` paths, each new module gets an `engine-free-<module>` ratchet row at 0 (§5.13).
  - **Where Engine-side code goes.** It stays in `impl Engine` in the existing engine files. No new `impl Engine` file is added.
- **D6 — Each front end names its kernels; production never reads the process-global kernel registry.**
  - **Membership.** Each kernel crate exports its registration as a public item. A front end builds `KernelSet::from_registrations(&[&KernelRegistration], primary)` from the items of the crates it depends on.
  - **`KernelSet::ensure`.** It instantiates only a member the set declared, and it never consults the global registry.
  - **Ownership.** S10 ι₃ #8992 defines neither of these, so both are γ₁'s additions to S10's type (S10 cross-reference).
  - **No global reads.** After S10 ι₄ (routing) and γ₁ (construction), no production path reads `kernel_registry::{registry, collect_registry}`. The production anchors and `test_registry_override` go. The `inventory` submissions remain for the conformance enumeration of linked kernels.
  - **Per-arm sets.** A front end's sets reproduce today's instantiation per command arm. There is one declared addition: OpenVDB on the CLI eval geometry arm.
  - **Manifold is not added.** Adding it would change which kernel `select` picks (S10).
- **D7 — The posture is an enum the engine reads and prints.**
  - `Registration::{Targets{targets, morph}, None(NoRegistration)}`.
  - The no-trampoline severity reads it.
  - `Engine::posture() -> EnginePosture` is printable data.
  - The registration lint matches every discovered production construction site against a `KNOWN_SITES` row carrying its posture token (§5.12).
- **D8 — `EngineConfig` is resolved once.**
  - **Sources.** It is resolved from `ConfigSources` (an env snapshot, the user config, the project config and an optional manifest) plus front-end overrides.
  - **reify-eval reads no process environment.** `ConfigSources::from_process` and the layered cache resolver live in reify-config. The `from_env` wrappers go; the pure `from_env_value` parsers stay.
  - **The manifest input stays.** No front end passes a manifest today (S27 OQ4-A keeps `reify.toml` inert); kernel-pin diagnostics come back on `ResolvedConfig`.
  - **The GUI posture enables the persistent cache (R3).**
- **D9 — Undef causes sit beside the snapshot they were classified from (R4).**
  - The map is a field of `EvaluationState`, tagged with its `SnapshotId`.
  - `eval_cached` and `edit_param` re-classify the cells they re-evaluated and carry the rest. Cells outside the re-evaluated cone are unchanged by construction, so their causes still hold.
  - **The declared-cell set is captured at eval.** The classifier skips synthetic guard, list and sub cells, which have no template declaration. The snapshot graph cannot tell these apart: guard cells are minted as `Let` nodes with a `default_expr`. And `edit_param` has no module. So `eval` and `eval_cached` store the declared cells with their declaration spans beside the map (the spans feed `UserUndef` and `Unbound` causes), and `edit_param` filters by the set it carries.
  - `trace_undef_causes` answers only for a matching snapshot.
  - The capture flag is `EngineConfig.capture_undef_causes`.
  - #7038's `SolveFailed` capture is wired on the cached and edit solve paths (subsumed).
- **D10 — Per-call outputs return on the call's result.**
  - `EvalPass` goes on `EvalResult`/`CheckResult`; `DispatchAttribution` goes on `BuildResult`/`TessellateResult`.
  - The GUI records the values from the calls it made, on `EngineSession`, and clears them on reload.
- **D11 — One read-only probe; raw mutators burn down to zero (R2).**
  - `Engine::probe() -> EngineProbe<'_>` (gated) carries read-only borrows and by-value counters. `Engine::fault_injection()` (gated) carries the panic-on-eval hook.
  - `cache_store_mut`, `warm_pool_mut` and `snapshot_mut` go to 0 across two burn-down leaves, under the migration policy of §5.10.
  - In production `Instrumentation` is a zero-sized type, checked at compile time.
- **D12 — Per-build state is what a surface resets and a later call reads.** `PerBuildState` keeps the five reset tables and the `capture_repr_tol` request flag, which S12's `measure` sets and restores. The dispatch tallies are per-call outputs (D10), and `geometry_revalidation_slow_path` is instrumentation (D11).
- **D13 — Partition diffs are moves with their same-diff obligations.**
  - **Shims and baselines.** No re-export shim at an old path. A pure move regenerates `crates/reify-audit/pdiag-baseline.txt` (`docs/notes/diagnostic-severity-policy.md` §3(d)) and re-keys moved `ptodo-baseline.txt` fingerprints.
  - **Program obligations.** After S24 ο #8822 lands, a moved file under PLINECITE scope runs `plinecite-baseline-gen --rename <old>=<new>`. After S24 ξ #8821 lands, a moved symbol named by a `docs/invariants.md` Pointer gets its pointer updated.
  - **INV-BUILD-2.** `version_id_discipline_gate.rs` stays green when the counters move (λ).
- **D14 — The two h14 partitions follow the lifetime classes (μ).** Items scheduled by another stream are not moved here:
  - `validate_param_override` (S4 η);
  - `shell_gui_mesh_data` and its parsers (S5 ζ/ο);
  - `trace_undef_causes`' graph source (S3 δ);
  - `eval_ctx_with_meta` (S4 τ).
- **D15 — B + H.** The bar is met several ways:
  - Blast radius: ≥ 5 crates (reify-eval, reify-config, reify-cli, reify-lsp, gui/src-tauri, the kernel crates' registration items).
  - About 12 mechanisms.
  - Two load-bearing seams (ComputeNode registration; multi-kernel membership).
  - ≥ 3 consuming streams (S2, S27, S12).
  - §5 fixes the signatures and invariants; §6 holds the two-way boundary tests.
- **D16 — Priorities.**
  - λ is high: it closes the one high finding.
  - ε is high: it carries INV-BUILD-1 and is S2's seam.
  - γ₁ and γ₂ are high: γ₁ is the constructor that S27 φ and every later leaf build on, and γ₂ carries INV-FEA-1.
  - These four are dependencies of the trigger-chain gate #8612. The rest carry their keys' severity.
- **D17 — Size splits (critic minor 13).**
  - γ splits into γ₁, the constructor, presets and kernels (task gate), and γ₂, the lint rewrite, infra test, verify-pipeline map and INV-FEA-1 flip (full gate).
  - δ splits by crate: δ₁ reify-eval, δ₂ gui/src-tauri, δ₃ the remaining crates plus deletion of the legacy constructors. This bounds each δ's rebase conflict with the in-flight wave-3 tasks.

## 5. Contract (H)

### 5.1 `EngineInputs` and the one constructor (γ₁, δ₁–δ₃)

```rust
// crates/reify-eval/src/engine_inputs.rs (new; below the engine SCC)
pub struct EngineInputs {
    pub config: EngineConfig,
    pub kernels: KernelSet,                 // S10 ι₃
    pub registration: Registration,
    pub constraints: ConstraintStack,
    pub prelude: Prelude,                   // Stdlib | Explicit(&'static [CompiledModule])
}
impl EngineInputs {
    pub fn hermetic() -> Self;              // EngineConfig::hermetic(), KernelSet empty, Registration::None(Hermetic),
                                            // ConstraintStack::simple(), Prelude::Stdlib; reads no env, no file
    pub fn with_checker(self, c: Box<dyn ConstraintChecker>) -> Self;
    pub fn with_kernel(self, k: Box<dyn GeometryKernel>) -> Self;   // S10's injected-kernel entry (L8)
    pub fn with_kernels(self, k: KernelSet) -> Self;
    pub fn with_solver(self, s: Box<dyn ConstraintSolver>) -> Self;
    pub fn with_prelude(self, p: &'static [CompiledModule]) -> Self;
    pub fn with_registration(self, r: Registration) -> Self;
    pub fn with_config(self, f: impl FnOnce(&mut EngineConfig)) -> Self;
}
pub struct ConstraintStack { pub checker: Box<dyn ConstraintChecker>, pub solver: Option<Box<dyn ConstraintSolver>> }
impl ConstraintStack { pub fn simple() -> Self; pub fn production() -> Self; } // production = Simple + SolverRegistry::production()

impl Engine {
    pub fn new(inputs: EngineInputs) -> Engine;   // the only constructor; registers per `inputs.registration`
    pub fn posture(&self) -> EnginePosture;       // Debug + Display + Eq: config, kernel ids + primary,
                                                  // registration kind / reason / target count, solver present
}
```

**Invariants (after δ₃).**
- `Engine::new` is the only `pub fn` returning `Engine`, and it reads no process environment.
- `register_production_compute_fns` becomes `pub(crate)` and is called only from `Engine::new`.
- `with_registered_kernel`, `with_registered_kernels`, `with_registered_kernels_and_manifest`, `with_test_kernels_and_registry`, `with_prelude`, `Engine::with_solver` (and their `legacy_test_*` renames), `pick_lexmin_brep_kernel(_in)` and `test_registry_override` do not exist.

**Front-end presets.** One function per front end, in its own crate. The table reproduces each site's posture at `988f4522b9` except the marked changes.

| site (today) | registration | kernels | constraints | config overrides |
|---|---|---|---|---|
| CLI `cmd_check` (one site after S12 ζ1 #9022 / ζ2 #9023) | `None(CheckWithoutTrampolines)` | from `CheckEnginePlan.kernel`: empty, or occt primary + openvdb on demand | simple | — (repr-tol capture and OpenVDB are S12's `MeasureRequest`) |
| CLI `cmd_build` | `Targets(PRODUCTION_TARGETS, Enabled(register_morph_producer))` | occt primary, openvdb on demand | simple (solver-free posture kept) | persistent cache off, as today |
| CLI `cmd_eval` geometry arm | as build | occt primary, openvdb on demand (**declared change**) | production | persistent cache from sources (+ `--cache-dir`); `capture_undef_causes` |
| CLI `cmd_eval` plain arm, `cmd_report`, `cmd_explain` | as build | empty | production | persistent cache from sources; `capture_undef_causes` on eval |
| `reify test` (`test_runner::build_test_engine`) | `Targets(<caller's table>, Unavailable{..})` | empty | simple | — |
| CLI MCP `ensure_engine` (only if #6665 has not landed) | `None(McpContext)` | empty | simple | persistent cache off (its comment's claim is false; corrected) |
| LSP ×4 (`AnalysisContext::from_parsed`, `EvalState::new`, `compute_diagnostics_with_state` cold rebuild, `compute_diagnostics`) | `None(LspAnalysis)` | empty | simple | `capture_undef_causes` (analysis) |
| GUI session (`main.rs` → `EngineSession`) | `Targets(PRODUCTION_TARGETS, Enabled)` under feature `gui`, `Unavailable` otherwise | occt primary, openvdb | production | persistent cache from sources (**R3**); `capture_undef_causes` |
| GUI `get_def_preview` | `None(DefinitionPreview)` | empty | simple | — |

**Notes on the table.**
- **"On demand."** The member-only `KernelSet::ensure` (D6) instantiates a declared kernel on first request, as `ensure_openvdb_kernel` does today.
- **`cmd_check` ordering.** γ₁ depends on #9022 and #9023, so it migrates the single plan-driven check site rather than today's four.
- **`EngineSession`.** It gains the same shape: `EngineSession::new(EngineInputs)` is its one constructor, and its posture setters in `from_engine` move into the GUI preset.
- **Where the GUI preset lives.** It lives in the src-tauri library (`engine.rs` or its S25 successor module, not `main.rs`), so B-α1 and B-γ2 can test it.

### 5.2 `EngineConfig` (α)

```rust
// crates/reify-eval/src/engine_config.rs (new)
#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    pub build_scheduler: BuildScheduler,          // S2 deletes it with LegacyMultiPass
    pub mesh_contract: MeshContractMode,          // INV-GEO-1 posture (today `mesh_contract_mode`)
    pub unfold: UnfoldLimits,                     // { depth, nodes }; today max_unfold_depth/_nodes
    pub long_chain_threshold: Duration,           // read once, not per build entry point
    pub warm_state_budget_bytes: Option<u64>,     // None = unlimited, as today
    pub persistent_cache: Option<PersistentCacheConfig>,  // { dir, max_bytes }
    pub capture_undef_causes: bool,
}
pub struct EngineOverrides { /* one Option<_> per field: front-end flags such as --cache-dir */ }
pub struct ResolvedConfig { pub config: EngineConfig, pub diagnostics: Vec<Diagnostic> }
impl EngineConfig {
    pub fn hermetic() -> Self;    // defaults; no env, no files
    pub fn resolve(sources: &reify_config::ConfigSources, overrides: &EngineOverrides)
        -> Result<ResolvedConfig, ConfigError>;
}
// crates/reify-config (moved down from reify-cli cache.rs; semantics unchanged)
pub struct ConfigSources { /* env snapshot, user config, project config, Option<Manifest> */ }
impl ConfigSources {
    pub fn from_process(project_dir: Option<&Path>) -> Result<Self, ConfigError>;
    pub fn empty() -> Self;
}
pub fn resolve_cache_layered(sources: &ConfigSources, cli_dir: Option<&Path>)
    -> Result<CacheResolution, CacheError>;
```

**Knobs.** `REIFY_BUILD_SCHEDULER`, `REIFY_MESH_CONTRACT`, `REIFY_WARM_STATE_BUDGET_BYTES`, `REIFY_LONG_CHAIN_THRESHOLD_MS`, `REIFY_CACHE_DIR`, `REIFY_CACHE_MAX_BYTES`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, `HOME`. `PATH` (the `fdm_slice` tool lookup) is tool discovery, not engine config: it stays where it is, and S5 moves `fdm_slice` into reify-analysis.

**Rules.**
- Resolution happens once per engine.
- No production knob setter survives γ₁.
  - `set_persistent_cache_dir`, `set_max_unfold_*` and `set_warm_state_budget` lose every production caller at γ₁ and become `cfg(any(test, feature = "test-instrumentation"))`. δ₁–δ₃ move their test callers (22, 10 and 3 sites) to `.with_config`, and δ₃ deletes them.
  - `set_capture_undef_causes` is deleted by ζ.
  - The gated test setters `set_build_scheduler` and `set_mesh_contract_mode` become `with_config` overrides in δ₁–δ₃, and ι deletes them.
- `BuildScheduler` moves into `engine_config.rs` (D5).
- An unrecognized knob value keeps today's fallback and adds one coded Warning (`DiagnosticCode::EngineKnobIgnored`) to `ResolvedConfig.diagnostics`. Front ends print these diagnostics as they print others (INV-SF-3, INV-SF-6).
- A `REIFY_CACHE_MAX_BYTES` resolver error keeps today's behaviour: persistent cache off, logged at DEBUG.
- `kernel_pin_diagnostics` moves to `engine_config.rs` and runs when `ConfigSources` carries a manifest.
- CLI and GUI call the same `ConfigSources::from_process` with the same project-directory rule (§11 Q2), so they resolve one cache directory. The GUI's startup sweep and its engine use that resolution.

### 5.3 Field homes (the whole partition)

| home (module) | fields today | landed by |
|---|---|---|
| `EngineConfig` (`engine_config.rs`) | `build_scheduler`, `mesh_contract_mode`, `max_unfold_depth`, `max_unfold_nodes`, `capture_undef_causes`, `persistent_cache_dir` (+ the new `long_chain_threshold`, `warm_state_budget_bytes`) | α |
| `Registrations` (`registrations.rs`) | `constraint_checker`, `solver`, `solvers`, `optimization_registry`, `compute_registry`, `morph_producer`, `solver_progress_sink`, `prelude`, `prelude_functions` (+ the posture) | β |
| `KernelSet` (S10 `kernel_set.rs`) | `geometry_kernels`, `default_kernel_name`; `repr_capable_kernel` deleted by ι₃; `test_registry_override` deleted by γ₁ | S10 ι₃, γ₁ |
| `PerBuildState` (`per_build_state.rs`) | `topology_attribute_table`, `swept_kind_table`, `realization_handles`, `achieved_repr_tol`, `relate_static_facts`, `capture_repr_tol` | ε |
| `EvalArtifacts` (`eval_artifacts.rs`) | `eval_state` (+ `last_undef_causes` folded into `EvaluationState`), `functions`, `meta_map`, `objectives`, `centrality_synthesized_scopes`, `compiled_fields` | ζ |
| `PurposeState` (`purpose_state.rs`) | `compiled_purposes`, `active_purposes`, `active_purpose_bindings`, `active_objective_map`, `active_purpose_let_cells`; `active_tolerance_scope` becomes derived | η |
| per-call outputs (fields deleted) | `last_eval_set`, `last_demand_prune_measurement`, `last_changed_realizations`, `last_dispatch_count`, `last_dispatch_count_by_realization` | θ |
| `Instrumentation` (`instrumentation.rs`, gated; ZST in production) | `last_guard_phase_group_evals`, `geometry_revalidation_slow_path`, `panic_on_eval_cells` | ι |
| `EngineCaches` (`engine_caches.rs`) | `cache`, `warm_pool`, `realization_cache`, `volume_mesh_sources`, `realization_projection_store`, `morph_source`, `cumulative_eval_cache_totals`, `persistent_hit_count`, `persistent_miss_count` | λ |
| `EngineLineage` (`lineage.rs`) | `next_snapshot_id`, `next_version_id`, `structure_registry`, `journal` | λ |
| `HostInputs` (`host_inputs.rs`) | `param_overrides`, `demand`, `observed_demand`, `active_solve_cancel` | λ |
| deleted upstream | `last_role_flip_probes`, `last_diff_value_cells` (S4 δ #8862); `last_param_override_type_kind_rejections`, `last_param_override_dimension_rejections`, `last_sub_component_unknown_structure_errors` (S4 λ #8869) | S4 |

Counted: 6 + 9 + 4 + 6 + 7 + 6 + 5 + 3 + 9 + 4 + 4 + 5 = 68 (§10.1).

**Fields added after `988f4522b9` by other streams take homes by the same rule.**
- S12 ε3 #9021 adds the engine's identity and its kernel generation. Identity → `EngineLineage`. Kernel generation → `EngineCaches`, beside the kernel-local caches whose flush advances it.
- ε and λ depend on #9021, so they place whatever it declared.
- Any other new field takes the home whose lifetime rule it obeys, and the leaf that adds it says which.

After λ:

```rust
pub struct Engine {
    config: EngineConfig, kernels: KernelSet, registrations: Registrations, lineage: EngineLineage,
    inputs: HostInputs, eval: EvalArtifacts, purposes: PurposeState, build: PerBuildState,
    caches: EngineCaches, instrumentation: Instrumentation,
}
```

Each sub-struct's doc states its lifetime rule once. The Engine doc is one line per sub-struct, and the 701-line field doc block goes.

### 5.4 Registration posture (β, γ₁)

```rust
// crates/reify-eval/src/registrations.rs (new)
pub enum Registration {
    Targets { targets: ComputeTargets, morph: MorphRegistration },   // S5 κ₁'s table
    None(NoRegistration),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoRegistration { CheckWithoutTrampolines, LspAnalysis, DefinitionPreview, McpContext, Hermetic }
pub(crate) struct Registrations { posture: RegistrationPosture, checker, solver, solvers,
    optimization_registry, compute_registry, morph_producer, solver_progress_sink, prelude: PreludeTables }
```

**Severity rule.** A missing `@optimized` target is an Error under `Targets` and a Warning under `None(r)`; the Warning's message names `r` (one display string per variant). `soft_no_trampoline_diagnostic` reads the posture. `compute_registry.fns.is_empty()` is not a posture input anywhere.

**Variants.** Adding a variant is a design change to this section. `McpContext` is deleted in #6665's diff, or never added if #6665 lands first.

**Transition (β → γ₁).** Before γ₁, the legacy constructors record `None(Hermetic)`, and a `register_production_compute_fns` call records `Targets`. That call is an explicit declaration, not an inference. γ₁ makes the posture a required constructor input.

### 5.5 `PerBuildState` (ε)

```rust
// crates/reify-eval/src/per_build_state.rs (new); BuildSurface moves here from engine_build.rs
pub(crate) enum BuildSurface { Build, BuildSnapshot, TessellateRealizations, TessellateSnapshot }
pub(crate) struct PerBuildState { topology_attribute_table, swept_kind_table, realization_handles,
    achieved_repr_tol, relate_static_facts, capture_repr_tol }
impl PerBuildState {
    pub(crate) fn reset(&mut self, surface: BuildSurface);   // exhaustive destructure of Self, no `..`
    // named writers for the realization walk; read-only views for S12's capture_measurement
}
```

**The reset table** (semantics unchanged):

| surface | resets |
|---|---|
| every surface | `topology_attribute_table`, `swept_kind_table` |
| `Build`, `BuildSnapshot` | `realization_handles` |
| `TessellateRealizations`, `TessellateSnapshot` | `achieved_repr_tol` |
| `Build` only | `relate_static_facts` |
| never | `capture_repr_tol` (S12's `measure` sets and restores it) |

The build↔tessellate asymmetry stays load-bearing (the CLI combined arm `build → tessellate_realizations → check`).

**Rules.**
- A field declared on PerBuildState that is not classified in `reset` does not compile. A field declared anywhere else is never touched by a surface reset.
- Each of the four entry points calls `self.build.reset(surface)` exactly once. `distance_between_placed` takes no reset; S2's `SurfacePolicy` decides that driver, and after θ it owns no engine-held accumulator.
- `reset_per_build_state` and its 68-field destructure are deleted.
- Every other clear of a PerBuildState field goes through a named PerBuildState method. For example, S12 ε3 #9021's `reset_geometry_for_reload` clear of `realization_handles` becomes `self.build.clear_handles()`. The `per-build-reset-site` row (§5.13) counts any other site.
- Until θ and ι land, the dispatch tallies and the slow-path counter are reset by plain statements beside the `reset` call, and those statements go with their fields.
- The docs say `relate_static_facts` resets on `Build` only (#5415).

### 5.6 `EvalArtifacts` and undef causes (ζ)

```rust
// crates/reify-eval/src/eval_artifacts.rs (new); EvaluationState moves here from lib.rs
pub struct EvaluationState { pub snapshot: Snapshot, pub reverse_index: ReverseDependencyIndex,
    pub trace_map: HashMap<NodeId, DependencyTrace>, undef_causes: UndefCauseMap }
pub(crate) struct UndefCauseMap { classified_for: SnapshotId, causes: HashMap<ValueCellId, UndefCause>,
    declared: Arc<HashMap<ValueCellId, Span>> }   // template-declared cells and their decl spans, from the module
pub(crate) struct EvalArtifacts { state: Option<EvaluationState>, functions, meta_map, objectives,
    centrality_synthesized_scopes, compiled_fields }
impl EvalArtifacts { pub(crate) fn install(&mut self, state: EvaluationState); /* … */ }
```

**Rules** (when `config.capture_undef_causes`):
- **`eval` classifies every cell**, as today.
- **`eval_cached` and `edit_param` re-classify only the cells they re-evaluated.**
  - The classifier reads the snapshot graph's `ValueCellNode.default_expr`, filtered by `declared`, plus op-contract failures as today.
  - `declared` maps each template-declared cell to its declaration span. The span is needed because `UndefCause::UserUndef { span }` and `Unbound { span }` carry it and `ValueCellNode` has none.
  - `eval` and `eval_cached` capture `declared` from the module they were given. `eval_cached` refreshes the spans of carried entries from its module. Only `edit_param`, which has no module, carries `declared` over unchanged.
  - Every other entry is carried.
  - Synthetic guard, list and sub cells are never classified, on any path.
- **The tag always matches.** The map's `classified_for` equals its snapshot's id at every install.
- **`trace_undef_causes` never answers across snapshots.** For a mismatched id it returns "no recorded cause", never a cause from another snapshot.
- **#7038.** `record_failed_autos` is called on `eval_cached`, `dispatch_merged_cluster_solve_cached` and `edit_param`. (`edit_source` is gone after S4 δ.)
- **The setter goes.** `set_capture_undef_causes` is deleted; front ends set the flag in their preset.
- **Snapshot docs are corrected.** The type and field docs describe a persistent-map value mutated on the working clone, with revalidation opt-in through `read_value_revalidated`.

### 5.7 `PurposeState` (η)

```rust
// crates/reify-eval/src/purpose_state.rs (new)
pub(crate) struct PurposeName(String);
pub(crate) struct ActivePurpose { bindings: Vec<(String, String)>, constraints: Vec<ConstraintNodeId>,
    objective: Option<ObjectiveSet>, let_cells: Vec<ValueCellId> }
pub(crate) struct PurposeState { compiled: Vec<CompiledPurpose>, active: BTreeMap<PurposeName, ActivePurpose> }
pub(crate) struct PurposeRebuild<'a> { state: &'a mut EvaluationState, demand: &'a mut DemandRegistry,
    compiled_fields: &'a [CompiledField] }
impl PurposeState {
    pub(crate) fn activate(&mut self, plan: ActivationPlan, rebuild: PurposeRebuild<'_>);
    pub(crate) fn deactivate(&mut self, name: &PurposeName, rebuild: PurposeRebuild<'_>) -> bool;
    pub(crate) fn reload(&mut self, compiled: Vec<CompiledPurpose>) -> Vec<(PurposeName, Vec<(String, String)>)>;
    pub(crate) fn tolerance_scope(&self) -> &HashMap<String, f64>;   // derived from the active bindings
}
```

**Invariants.**
- **One entry per active purpose.** An active purpose is one map entry holding all its parts, so no part can outlive the others.
- **Every mutation rebuilds.** `activate` and `deactivate` rebuild the reverse index, the trace map, the demand cone and the tolerance scope themselves, so no caller pairs them by hand.
- **`reload` handles vanished purposes.** It drops entries whose purpose vanished from `compiled` (the orphan fix) and returns the bindings to re-inject.
- **The Engine side stays in engine_purposes.rs.** The `impl Engine` side in `engine_purposes.rs` builds each `ActivationPlan` (the graph injection) and calls `activate`. Both re-injection blocks in engine_eval become `reload` plus one `activate` per returned binding set, with no conditional clear.
- **Tolerance scope.** The tolerance scope may be cached inside PurposeState and recomputed on mutation, but it is never a sibling field.

### 5.8 `EngineCaches`, `EngineLineage`, `HostInputs` (λ)

- **`EngineCaches`.** It holds the node cache, warm pool, realization cache, volume-mesh sources, projection store, morph sources, lifetime totals and persistent hit/miss stats. `flush_all()` is the full flush that `reset_geometry_for_reload` performs; it keeps the kernel-local `realization_cache` and `volume_mesh_sources` coherent with the kernels. `morph_source`'s stale `allow(dead_code)` and its test-only getter go.
- **`EngineLineage`.** It holds `VersionCounter { next_snapshot_id, next_version_id }` and `allocate()`, the single writer (INV-BUILD-2). `eval_cached`'s snapshot-only re-stamp keeps its `version-id-gate: allow` as a method of `VersionCounter`. `crates/reify-eval/tests/version_id_discipline_gate.rs` allowlists by file name (`("engine_admin.rs", "allocate_snapshot_version")`, `("engine_admin.rs", "last_allocated_version")`), so λ re-keys those entries to `lineage.rs` in the same diff. S12 ε3's engine identity lives here too. It also holds the structure interner, whose ids every live snapshot holds, so it only grows (μ moves `populate_structure_registry` beside it). And it holds the append-only journal.
- **`HostInputs`.** It holds `param_overrides`, `demand` (with its derived cone), `observed_demand` and `active_solve_cancel`: what the host supplied, which survives eval.

### 5.9 Per-call outputs (θ)

```rust
pub enum EvalPass { Cold, Cached, Edit(EditReport) }
pub struct EditReport { pub eval_set: Vec<NodeId>, pub demand_prune: Option<DemandPruneMeasurement>,
    pub changed_realizations: BTreeSet<RealizationNodeId> }
pub struct DispatchAttribution { /* private */ by_realization: BTreeMap<RealizationNodeId, usize> }
impl DispatchAttribution { pub fn dispatched(&self) -> impl Iterator<Item = &RealizationNodeId>; pub fn total(&self) -> usize; }
// EvalResult.pass: EvalPass and CheckResult.pass: EvalPass; BuildResult.dispatch and TessellateResult.dispatch
```

**Rules.**
- The drivers accumulate the attribution in a local passed to `bump_dispatch` and return it, so `distance_between_placed` no longer accumulates on the engine.
- The five fields and their getters are deleted. `last_substantive_value` stays, because it is a cache lookup, not a per-call output.
- **The GUI records what its own calls returned.** `EngineSession` keeps the `DispatchAttribution` of the tessellation that produced the displayed meshes and the `EditReport` of the last preview, both cleared by reload. The debug JSON keys (`last_dispatch_count_post_refresh` and the rest) keep their names and meanings.
- `surface_geometry_derived_cells` takes the dispatched set from the result instead of parsing delta mesh keys.

### 5.10 Instrumentation, `EngineProbe` and the test-seam policy (ι, κ₁, κ₂)

```rust
// crates/reify-eval/src/instrumentation.rs (new)
#[cfg(any(test, feature = "test-instrumentation"))]
pub(crate) struct Instrumentation { guard_phase_group_evals: usize,
    geometry_revalidation_slow_path: AtomicUsize, faults: FaultInjection }
#[cfg(not(any(test, feature = "test-instrumentation")))]
pub(crate) struct Instrumentation;                                  // const-asserted size 0
// recording methods are #[inline] no-ops in production; on_surface(BuildSurface) resets per-surface counters
#[cfg(any(test, feature = "test-instrumentation"))]
pub struct EngineProbe<'a> { /* read-only: cache(), warm_pool(), realization_cache(), structure_registry(),
    realization_handles_len(), imported_file_content_hash(..), imported_field_provenance(..), terminal_handle(..);
    by value: guard_phase_group_evals(), geometry_revalidation_slow_path() */ }
// in impl Engine (engine_admin.rs), gated: pub fn probe(&self) -> EngineProbe<'_>;
//                                          pub fn fault_injection(&mut self) -> &mut FaultInjection;
```

**End state (after κ₂).**
- **What remains gated on Engine.** Only `probe()` and `fault_injection()`.
- **What is deleted.**
  - the other 26 gated accessors;
  - `CacheStore::insert_synthetic_realization_entry`;
  - S4's `ValueCellSeal::for_test_seeding()`, once no test seeds a value cell.
- **What moves to the inputs.** `set_build_scheduler` and `set_mesh_contract_mode` become `EngineInputs::with_config` overrides (S2 later deletes the scheduler field).
- **What S12 replaces.** `set_achieved_repr_tol_for_test` is replaced by S12's `MeasurementContext::measured_for_test`.

**Migration policy, per seeding or mutating test site** (recorded per site in the commit). Pick one:
- (a) Drive the state through public `eval` / `edit_param` / `build` / `tessellate` calls, using a fixture helper in reify-test-support (for example `engine_after(source, edits)`).
- (b) If the test exercises `CacheStore`, `WarmStatePool`, `RealizationCache` or `ComputeDispatchRegistry` logic, move it to that type's own unit tests, which construct the type directly without an Engine.
- (c) Delete it with the reason (duplicate coverage).

No test reaches a sub-struct's private state.

### 5.11 Status cells (S1 owns INV-FEA-1 and INV-BUILD-1)

| row | Invariant text after the flip | Enforcement | Status | Pointer (S24 §5.7) | leaf |
|---|---|---|---|---|---|
| INV-FEA-1 | Every production engine declares its compute-registration posture; registration has one bundler and the engine one constructor | type+lint+test: `Engine::new(EngineInputs)` is the only constructor and `Registration` a required input; `NoRegisteredComputeTrampoline` severity reads the posture; `check-compute-trampoline-registration.sh` declares every production construction site with its posture; per-front-end posture tests | `enforced(type+lint+test)` | `item:crates/reify-eval/src/registrations.rs::Registration; test:tests/infra/test_compute_trampoline_registration_wired.sh` | γ₂ |
| INV-BUILD-1 | Per-build engine state resets exactly once per build/tessellate entry point; a field is per-build iff it is declared on `PerBuildState` | type+test: `PerBuildState::reset(BuildSurface)` destructures its own fields exhaustively; no other Engine state is reset by a surface; the surface-sequence matrix | `enforced(type+test)` | `item:crates/reify-eval/src/per_build_state.rs::PerBuildState; test:crates/reify-eval/tests/harness_engine/per_build_surface_sequence.rs` | ε |

**Same-diff edits.**
- ε rewords INV-BUILD-3's parenthetical (which cites "INV-BUILD-1, now `enforced(type)` as of task ι/#5069") to name the new mechanism.
- The Owner column names this PRD for both rows.

**Before and after #8821.** Before #8821 lands, the pointers sit inline in the Enforcement cell. After it lands, they move to the Pointer column and the `pending-owner:` tag is deleted in the flipping diff.

**#8821's owner map.** FEA-1 → γ₂, BUILD-1 → ε.

### 5.12 The registration lint (`KNOWN_SITES`; S1 owns, S5 carve-out)

**The positive pass changes.** Today it checks that 3 literal sites carry a `MorphRegistration` token. After γ₂:
- **Discovery.** It discovers every file outside reify-eval's engine modules whose lexed production view contains `Engine::new(` or `EngineSession::new(`. "Engine modules" means `crates/reify-eval/src/engine_*.rs` plus everything under `crates/reify-eval/src/engine_*/`. It uses the existing scope pathspecs and `scripts/lib_rust_production_view.sh`.
- **Matching.** Each discovered file must match at least one `KNOWN_SITES` row of the form `path|posture-token`. The token is either `Registration::Targets` or `NoRegistration::<Variant>`, and it must appear in that file's production view.
- **Test-support files.** Some files compile only under a `#[cfg(test)] mod` declaration or through a test-support crate, but the lexed view reads them as production: `crates/reify-test-support/src/{helpers,fixtures}.rs`, `realization_read_test_support.rs`, `geometry_ops/tests.rs`. `EngineInputs::hermetic` is therefore an accepted posture token. The leaf that converts such a file adds its `path|EngineInputs::hermetic` row, and γ₂ adds an infra case where a hermetic-only file needs no other token.
- **Several postures in one file.** A path may carry several rows, one per posture it constructs, and every row's token is required. For example, gui `engine.rs` constructs the session's `Targets` and `get_def_preview`'s `None(DefinitionPreview)`; CLI `main.rs` holds several until S27 υ moves the commands.
- **The MCP context row.** The `McpContext` row exists only while `mcp_context.rs` does. A note on #6665 tells it to delete that row in its own diff.
- **The `Targets` constraint.** A `Targets` row's file must also name `PRODUCTION_TARGETS`. The one exception, `test_runner.rs`, passes its caller's table.
- **Two failure modes.** A discovered file without a row reds: "production Engine construction without a declared posture". A row whose file no longer constructs an engine reds: stale.

**Unchanged.** The negative pass (half names) and `EXEMPT_DEFINITION_FILES` are S5 ε's (Leo S5 R4).

**What else γ₂ changes.**
- **The infra test.** γ₂ adds cases to `tests/infra/test_compute_trampoline_registration_wired.sh`: an undeclared site reds; a `Targets` row without `PRODUCTION_TARGETS` reds.
- **The infra-test map.** γ₂ adds `scripts/verify-pipeline-infra-tests.txt` rows mapping every declared site file to that test.
- **φ #8776.** It moves the CLI rows to `crates/reify-cli/src/engine_config.rs` in its own diff.

**The gate.** The script and the map are verify-pipeline files, so γ₂ and φ land through the full gate.

### 5.13 Adoption-ratchet rows (`tests/infra/adoption-ratchet.manifest`; grammar in its header)

**Row shape.**
- Columns are `pattern-id | ERE | path | ceiling | owner leaf`.
- Pattern-ids are kebab-case (the engine's `_PATTERN_ID`).
- Every ERE starts `^[^/]*` so `//` comment lines do not count (the engine itself skips only `#` lines; S10 precedent).
- Paths resolve through `Path.glob`, so `engine_*.rs` matches direct children only.
- Each leaf adds its rows at the post-change count in its own diff, and lowers a row's ceiling in the diff that lowers its count.
- The engine scans `#[cfg(test)]` modules too, so a leaf whose row targets 0 also rewrites the in-crate tests that hit it.

| id | ERE (after the prefix) | path | ceiling | leaf |
|---|---|---|---|---|
| engine-env-read | `env::var(_os)?[(]` | `crates/reify-eval/src/**/*.rs` | 1, the `fdm_slice` `PATH` lookup (S5 later takes it to 0). α rewrites the in-crate tests that read env today (`engine_fixpoint.rs`, `lib.rs`, `warm_pool.rs`) onto the pure `from_env_value` parsers | α |
| compute-posture-inferred | `compute_registry[.]fns[.]is_empty` | `crates/reify-eval/src/**/*.rs` | 0 | β |
| engine-legacy-ctor | `fn (with_registered_kernels?\|with_registered_kernels_and_manifest\|with_test_kernels_and_registry)[(]` | `crates/reify-eval/src/engine_admin.rs` | 0 | γ₁ |
| engine-global-registry-read | `kernel_registry::(registry\|collect_registry\|pick_lexmin_brep_kernel)[(]` | `crates/reify-eval/src/**/*.rs` | the measured remainder at γ₁, production 0. `kernel_registry.rs`'s own items are unprefixed and do not count | γ₁ |
| kernel-link-anchor-cli / kernel-link-anchor-gui / kernel-link-anchor-eval-tests | `extern crate reify_kernel_` | `crates/reify-cli/src/**/*.rs`; `gui/src-tauri/src/**/*.rs`; `crates/reify-eval/tests/**/*.rs` | 0 | γ₁; γ₁; δ₁ |
| engine-legacy-test-ctor-eval / -gui / -rest | `(::\|[.])legacy_test_(new\|with_prelude\|with_solver)[(]` (call sites only; the definitions do not match) | `crates/reify-eval/**/*.rs`; `gui/src-tauri/src/**/*.rs`; `crates/**/*.rs` | 0, after δ₁; δ₂; δ₃ | δ₁; δ₂; δ₃ |
| per-build-reset-site | `((topology_attribute_table\|swept_kind_table) = [A-Za-z_:]*(TopologyAttributeTable\|SweptKindTable)::default\|(realization_handles\|achieved_repr_tol\|relate_static_facts)[.](clear\|drain)[(])` | `crates/reify-eval/src/engine_*.rs` | 0 (5 today, all in `reset_per_build_state`) | ε |
| reset-per-build-state | `reset_per_build_state` | `crates/reify-eval/src/**/*.rs` | 0 | ε |
| undef-cause-field | `last_undef_causes` | `crates/reify-eval/src/**/*.rs` | 0 | ζ |
| undef-capture-setter / undef-capture-setter-gui | `set_capture_undef_causes[(]` | `crates/**/*.rs`; `gui/src-tauri/src/**/*.rs` | 0 | ζ |
| parallel-purpose-map | `active_(purposes\|purpose_bindings\|tolerance_scope\|objective_map\|purpose_let_cells)([^_a-z]\|$)` | `crates/reify-eval/src/**/*.rs` | 0 | η |
| purpose-rebuild-pair | `rebuild_purpose_infrastructure[(]` | `crates/reify-eval/src/**/*.rs` | 0 | η |
| last-call-getter | `fn last_(eval_set\|demand_prune_measurement\|dispatch_count\|dispatch_count_by_realization\|changed_realizations)[(]` | `crates/reify-eval/src/**/*.rs` | 0 | θ |
| dispatched-entity-proxy | `dispatched_entities` | `gui/src-tauri/src/**/*.rs` | 0 | θ |
| engine-gated-accessor | `pub fn (cache_store\|realization_cache\|warm_pool\|last_guard_phase_group_evals\|realization_handles_len\|geometry_revalidation_slow_path_count\|structure_registry\|test_terminal_handle\|imported_file_content_hash\|imported_field_provenance\|set_build_scheduler\|set_mesh_contract_mode\|set_panic_on_eval\|remove_panic_on_eval\|clear_panic_on_eval)[(]` | `crates/reify-eval/src/engine_admin.rs` | 0 | ι |
| raw-state-mutator / raw-state-mutator-gui | `[.](cache_store_mut\|warm_pool_mut\|snapshot_mut)[(]` | `crates/**/*.rs`; `gui/src-tauri/src/**/*.rs` | the measured remainder at κ₁; 0 at κ₂ | κ₁, κ₂ |
| engine-free-engine-config, engine-free-engine-inputs, engine-free-registrations, engine-free-per-build-state, engine-free-eval-artifacts, engine-free-purpose-state, engine-free-instrumentation, engine-free-engine-caches, engine-free-lineage, engine-free-host-inputs | `(^\|[^A-Za-z_])Engine([^A-Za-z_]\|$)` | that module's file under `crates/reify-eval/src/` | 0 | the creating leaf |

The `\|` in this table escapes the markdown pipe; the ERE in the manifest uses a bare `|`.

## 6. Boundary-test sketch (two-way)

| id | side | scenario | precondition | postcondition | leaf |
|---|---|---|---|---|---|
| B-α1 | consumer (GUI) | GUI and CLI resolve one cache dir | a user config at `$XDG_CONFIG_HOME/reify/config.toml` sets `[cache] dir`; same project dir | the GUI production session's `config().persistent_cache` equals what `reify eval` resolves; the startup sweep sweeps that dir | α |
| B-α2 | consumer (GUI) | GUI FEA result persists | two GUI sessions in sequence on one cache dir load an FEA example | the second session reports a persistent hit (`persistent_hit_count > 0`) and identical values | α |
| B-α3 | producer | hermetic config ignores the process | `REIFY_BUILD_SCHEDULER=legacy`, `REIFY_MESH_CONTRACT=warn`, `REIFY_WARM_STATE_BUDGET_BYTES=1` set | `EngineConfig::hermetic()` equals its defaults; `resolve(from_process)` reflects all three; an unrecognized value yields one `EngineKnobIgnored` Warning and today's fallback | α |
| B-β1 | consumer (CLI) | deliberate opt-out stays a Warning | `reify check` on an FEA-bearing fixture | the existing `check_downgrades_unregistered_trampoline_fallback_to_warning_while_eval_and_build_keep_erroring` stays green; the Warning names `CheckWithoutTrampolines` | β |
| B-β2 | producer | registered engine missing a target errors | a `Targets` engine whose table lacks one stdlib `@optimized` target | `NoRegisteredComputeTrampoline` at Error severity, whatever else the table holds | β |
| B-γ1 | lint | undeclared production site | a scratch production file in reify-cli that calls `Engine::new(` without a `KNOWN_SITES` row | the lint exits 1 naming the file; with the row it exits 0 | γ₂ |
| B-γ2 | consumers (each front end) | posture table | the presets of §5.1 | one test per front-end crate asserts its `EnginePosture` row (registration kind/reason, kernel ids, solver present, persistent cache on/off) | γ₁ |
| B-γ3 | consumer (CLI) | eval declares OpenVDB | the isosurface fixture `build_voxel_to_mesh_writes_the_whole_isosurface` uses; γ₁ first records today's `reify eval` output on it | the eval geometry preset's posture lists openvdb; `reify eval` on the fixture exits 0 with no missing-kernel diagnostic. If the pre-γ₁ run already showed none (the eval arm never reached the isosurface), this row is a regression pin only and §1's claim is dropped in γ₁'s commit | γ₁ |
| B-δ1 | producer (tests) | hermetic suites | `REIFY_BUILD_SCHEDULER=legacy REIFY_MESH_CONTRACT=warn REIFY_WARM_STATE_BUDGET_BYTES=1` in the shell | `cargo nextest run -p reify-eval` gives the same pass/fail set as without them | δ₁ |
| B-ε1 | both | surface-sequence matrix (`crates/reify-eval/tests/harness_engine/per_build_surface_sequence.rs`) | one fixture with geometry, a `RepresentationWithin` bound, a DFM rule and a relate block | for every ordered pair of surfaces A→B followed by `check`: Conforms/DFM/RepresentationWithin verdicts and the relate-consumption record equal the single-surface baselines; the CLI combined arm `build → tessellate → check` is byte-identical to today; `realize_for_check → tessellate → check` still sees the relate facts (#5415) | ε |
| B-ζ1 | consumer (GUI) | hover after a slider tick | a param whose preview value makes a downstream cell Undef | after `preview_parameter`, `build_values`' cause for that cell names the edited state's origin; before ζ it names the pre-edit cause or none | ζ |
| B-ζ2 | producer | edit-path classification equivalence | ≥ 5 fixtures × edit sequences, capture on | after each edit the cause map equals a cold `eval` of the edited values' map | ζ |
| B-ζ3 | producer (#7038) | failed solve on cached/edit paths | an auto whose solve fails | `SolveFailed { detail }` on `eval_cached` and `edit_param`, never `AwaitingSolve` | ζ |
| B-η1 | both | vanished purpose | activate a let-bearing purpose; re-eval a source without it; deactivate | values and diagnostics equal a never-activated engine's; no orphan entry | η |
| B-η2 | consumer (CLI) | purpose regression floor | the `reify check --purpose` CLI tests | unchanged output | η |
| B-θ1 | consumer (GUI) | no stale prune record | slider edit, then whole-file reload | `build_gui_state` and the debug engine state carry no demand-prune record | θ |
| B-θ2 | consumer (GUI) | dispatched-but-failed realization | a realization dispatched whose ops fail | the derived-cell overlay drops the stale value instead of replaying it as `Final` | θ |
| B-θ3 | producer | dispatch attribution | build, tessellate, `distance_between_placed` | each result's `dispatch.total()` equals that call's dispatches; consecutive calls do not accumulate | θ |
| B-ι1 | producer | zero-cost instrumentation | a non-test build | `const _: () = assert!(size_of::<Instrumentation>() == 0)` compiles under `cfg(not(any(test, feature = "test-instrumentation")))` | ι |
| B-λ1 | both | regression floor | `examples/best_practices/*.ri` and the FEA/modal examples | `reify eval` / `reify check` output byte-identical to pre-λ; `version_id_discipline_gate` green; `module_scc_ratchet` green | λ |

## 7. Out of scope

- The `KernelSet` type, kernel selection and descriptor rows (S10).
- `check()`'s measurement context and the `set_achieved_repr_tol_for_test` replacement (S12).
- `BuildScheduler::LegacyMultiPass` retirement, `SurfacePolicy` and a surface for `distance_between_placed` (S2).
- Typed `ValueCellId` / `InstancePath` (S3 OQ-2 b, about 2,066 sites): a recorded post-S1 follow-up candidate, not filed here.
- The Snapshot module hash and the LSP content-hash mirror deletion (#8564, filed; it depends on ζ so it builds on `EvalArtifacts`).
- A production `reify.toml` loader (S27 OQ4-A: inert).
- The h14 partitions of engine_eval / engine_build / engine_edit (S4 ν, S2).
- GUI `EngineSession` internals beyond its constructor and the per-call reads (S25).
- Whether production reads should route through `read_value_revalidated` (fk-d2c64e9d3868's "decide separately" half): a recorded follow-up candidate.
- Removing the `inventory` kernel submissions (they stay for the conformance enumeration).

## 8. Cross-PRD relationship (G4)

| other PRD / task | direction | seam mechanism | owner | status |
|---|---|---|---|---|
| S4 `cell-eval-transaction-closure.md` δ #8862, λ #8869, τ #8872 | upstream | fields deleted; crate-root `eval_env`; `Engine::cell_eval_ctx` gone | S4 | every Engine-struct leaf here depends on all three |
| S4 η #8864 | upstream | `param_override.rs` | S4 | μ depends |
| S4 γ #8861 `ValueCellSeal::for_test_seeding()` | consumes → deletes | test cache seeding | S1 κ₂ deletes it once unused | cross-reference |
| S5 ε #8994 (type), κ₁ #8999 (injection) | upstream | `ComputeTargets` (ε); `PRODUCTION_TARGETS`, `register_production_compute_fns(targets, morph)`, `run_tests(targets, …)` (κ₁) | S5 | β depends on κ₁, which depends on ε |
| S5 ε #8994 | sibling | lint `EXEMPT_DEFINITION_FILES` + half names | S5 (carve-out) | file contention only; γ₂ rewrites the positive pass after ε |
| S5 ζ #8996, ο #9007 | upstream | `shell_gui_mesh_data` leaves engine_admin.rs | S5 | μ depends on #9007 |
| S10 ι₃ #8992, ι₄ #8993, β₃ #8957 | upstream | `KernelSet`; routing reads; anchor cfg | S10 owns the type; S1 γ₁ adds `KernelSet::from_registrations` and the member-only `ensure` (D6) and deletes the anchor lines β₃ keeps | γ₁ depends on all three; S10 cross-reference + a note on #8957 |
| S3 β #8687, δ #8691, ζ #8699, κ #8680 | upstream | predicates below the SCC (so new modules do not grow it); `trace_undef_causes` repoint; 29 seam sites deleted; `set_observed_demand` | S3 | α ← #8687 (inherited by every leaf); ζ ← #8691; κ₁ ← #8699; λ ← #8680 |
| S12 ε1 #9017, ε3 #9021 | upstream | `measure` / `capture_measurement` read PerBuildState; `measured_for_test` replaces `set_achieved_repr_tol_for_test`; check-time reads → 0; ε3 adds engine identity + kernel generation and clears `realization_handles` on reload | S12 (measurement), S1 (PerBuildState, field homes §5.3) | ε ← #9017, #9021; S12 §8's "S1 moves the stores onto results" corrected (they stay in PerBuildState); notes on #9017, #9021 |
| S12 ζ1 #9022, ζ2 #9023 | upstream | one `CheckEnginePlan` for `cmd_check` (4 → 1 construction sites) | S12 | γ₁ ← #9022, #9023; the check preset takes `CheckEnginePlan.kernel` |
| S25 α #8937, θ #8944 | upstream | GUI module split; `surface_geometry_derived_cells` | S25 | α, γ₁, θ ← #8937; θ ← #8944 |
| S27 φ #8776 | downstream (held on S1) | CLI preset moves to `reify-cli/src/engine_config.rs`; its `KNOWN_SITES` rows and infra-test map row move with it | S27 | released by this stream: φ ← γ₁, γ₂ |
| S27 τ #8758, υ #8775 | sibling | `main.rs` partitions | S27 | file lock, symbol-cited; no edge |
| S2 (wave 3, unfiled) | downstream | `PerBuildState::reset(BuildSurface)`, `BuildSurface` home, `EngineConfig` | S1 produces; S2 consumes | cross-reference: S2 leaves depend on ε and α |
| S24 ξ #8821, ο #8822 | consumes | owner map; pointer grammar; PLINECITE rename | S24 | owner map recorded at pass 2 |
| #6665 | sibling | deletes `mcp_context.rs` | #6665 | γ₁ handles both orders; a note on #6665 asks it to delete the `McpContext` `KNOWN_SITES` row |
| S4 ρ #8876 | upstream | h14 partition of `persistent_cache.rs` | S4 | μ ← #8876 (μ moves `sweep_persistent_cache_at_startup` into the post-ρ module) |
| #6671 (solver-unification δ) | downstream | typed-cause upgrade of the `SolveFailed` channel | #6671 | builds on ζ's wiring; a note on #6671 |
| #8564 (fk-3aeb721eaaa6) | downstream | Snapshot module hash | #8564 | #8564 ← ζ |
| #7038 | subsumed | `SolveFailed` capture on cached/edit paths | S1 ζ | cancelled at decompose with a reason naming ζ |
| S19 adoption-ratchet engine | consumes | manifest rows | S19 (engine) | rows only |
| S23 | downstream | the Engine field doc block that λ moves | S1 | S23 skips what λ rewrites |

## 9. Decomposition plan

Labels are Greek; task ids are assigned at decompose. Each leaf names its keys, invariant, ratchet rows, dependencies and signal.

**Shared done criteria (every leaf).**
- **INV-ARCH-1 (D5).** A new module imports nothing in the residual engine SCC, names no `Engine` (its `engine-free-*` row at 0), and `module_scc_ratchet` stays green with no `GREW`; a shrink lowers the baseline.
- **D13 same-diff obligations.** Regenerate pdiag and re-key ptodo; after #8822 run `plinecite-baseline-gen --rename`; after #8821 update Pointers.
- **The regression floor.**
- **Drift-guard registrations.** A new gate-resident test carries its nextest and wall-clock registrations in its own diff. A new reify-eval integration test is a module of an existing harness (for example `tests/harness_engine/`), never a new standalone binary, because reify-eval is in the harness-layout gate's `_HL_CRATES`.
- **The INV-id each leaf cites**, with its enforcement mechanism.

- **α — EngineConfig resolved once; one layered cache resolver; the GUI uses the persistent cache.**
  - **Work.**
    - Add `engine_config.rs` per §5.2, with `BuildScheduler` moved into it (D5).
    - Add `reify_config::ConfigSources` and `resolve_cache_layered`, moved down from `reify-cli/src/cache.rs`; the CLI calls them.
    - Delete the `from_env` wrappers (`BuildScheduler`, reify-ir `MeshContractMode`, `WarmStatePool`, `long_chain_threshold_from_env`). Rewrite the in-crate tests that read env onto the pure parsers.
    - The long-chain threshold is read once.
    - The engine holds `config`, exposed read-only as `Engine::config(&self) -> &EngineConfig`, and the transitional constructors resolve from `ConfigSources::from_process(None)`.
    - The GUI (post-#8937 `engine/persistent_cache.rs`; the preset in the src-tauri library) resolves through the same call, sets its engine's persistent cache (R3) and sweeps that directory.
    - Add `EngineKnobIgnored`.
  - **Signal.** B-α1, B-α2, B-α3; the existing `configured_eval_engine_wires_cache_dir` and `cache_dir_cli_override_replaces_configured_dir` green; row `engine-env-read` at 1; `engine-free-engine-config` at 0.
  - **Keys.** fk-4fa7bda0085b. Medium.
  - **Deps.** #8687, #8862, #8869, #8872, #8937.
- **β — Registration posture; `Registrations`; severity reads the posture.**
  - **Work.** Add `registrations.rs` per §5.4; the collaborators, registries and hooks move into `Registrations`; `soft_no_trampoline_diagnostic` reads the posture; `Engine::posture()`.
  - **Signal.** B-β1, B-β2; rows `compute-posture-inferred` and `engine-free-registrations` at 0.
  - **Keys.** none (`x_finding_part` fk-06103e5f06eb, closed by γ₂). Medium.
  - **Deps.** α, #8999.
- **γ₁ — The one constructor; every production front end; explicit kernels.**
  - **The constructor.** Add `engine_inputs.rs`, `Engine::new(EngineInputs)` and `Engine::config(&self) -> &EngineConfig` per §5.1 (α already adds `config()`). Every production site moves to its preset: the CLI (with `cmd_check`'s single plan-driven site after #9022/#9023), the LSP, the GUI and `test_runner`. `EngineSession::new(EngineInputs)` replaces the GUI's constructor pair.
  - **Kernels.**
    - Each kernel crate exports its registration item.
    - Add `KernelSet::from_registrations` and the member-only `ensure` (D6); the front ends build `KernelSet` from those items.
    - Delete: the CLI and GUI anchors; the registry constructors; `pick_lexmin_brep_kernel(_in)`; `with_test_kernels_and_registry`; `test_registry_override` (and its cfg pairs in engine_build).
  - **Legacy test constructors.**
    - Add `features = ["test-instrumentation"]` to the reify-eval dev-dependency of reify-cli, reify-compiler and reify-mesh-morph.
    - Then rename three Engine items with a scripted token rename, and gate them `cfg(any(test, feature = "test-instrumentation"))`: the old `Engine::new(checker, kernel)` → `Engine::legacy_test_new`; `with_prelude` → `legacy_test_with_prelude`; the `Engine::with_solver` builder (about 165 chained test calls) → `legacy_test_with_solver`.
    - The rename covers test files and `crates/reify-test-support/src`.
    - The GUI's test constructor `EngineSession::new(checker, kernel)` (304 test calls) is renamed the same way to a `cfg(test)` `EngineSession::legacy_test_new`, so the workspace test build stays green until δ₂.
    - The registry-constructor test sites (≈ 47) and the `with_test_kernels_and_registry` sites move to explicit `KernelSet`s here, since their constructors are deleted.
  - **The MCP context.** If `mcp_context.rs` still exists, give it `None(McpContext)` and delete its false persistent-cache comment.
  - **Signal.** B-γ2, B-γ3; B-β1 still green; rows `engine-legacy-ctor`, `engine-global-registry-read`, `kernel-link-anchor-cli`/`-gui` and `engine-free-engine-inputs` at target; the workspace test build is green.
  - **Keys.** fk-4db43199eda2. **High.**
  - **Deps.** α, β, #8992, #8993, #8957, #8937, #9022, #9023.
- **γ₂ — The registration lint declares every production site; INV-FEA-1.**
  - **Work.** Rewrite the lint's positive pass and its infra test per §5.12, add the infra-test map rows, and flip INV-FEA-1 (§5.11).
  - **Signal.** B-γ1; `test_compute_trampoline_registration_wired.sh` green with its new red cases; INV-FEA-1 reads `enforced(type+lint+test)` with resolving pointers.
  - **Keys.** fk-06103e5f06eb. INV: INV-FEA-1. **High.**
  - **Deps.** γ₁. Full gate (verify-pipeline files).
- **δ₁ — reify-eval tests build through `EngineInputs::hermetic()` (R1).**
  - **Work.** A script quoted in the commit message, and re-run on any rebase, rewrites reify-eval's `Engine::legacy_test_new(c, k)` / `legacy_test_with_prelude(c, k, p)` calls (≈ 1,095 sites, src test modules and `tests/`) into `Engine::new(EngineInputs::hermetic()…)` with `.with_checker` / `.with_kernel` / `.with_prelude` / `.with_solver`.
  - **Hand-migrated.** Construction-adjacent `set_build_scheduler` / `set_mesh_contract_mode` / `set_max_unfold_*` / `set_persistent_cache_dir` / `set_warm_state_budget` calls become `.with_config` overrides. Files converted outside the engine modules (`realization_read_test_support.rs`, `geometry_ops/tests.rs`) get their `KNOWN_SITES` hermetic rows.
  - **Deleted.** The 17 reify-eval test anchors and the reify-eval `Cargo.toml` dead-strip prose.
  - **Signal.** B-δ1; reify-eval suites green; rows `engine-legacy-test-ctor-eval` and `kernel-link-anchor-eval-tests` at 0.
  - **Keys.** none (ruling R1). Medium.
  - **Deps.** γ₁.
- **δ₂ — GUI tests build through the hermetic preset.**
  - **Work.** The same script migrates gui/src-tauri's ≈ 304 `EngineSession::legacy_test_new` test calls and the GUI's `legacy_test_*` engine calls to `EngineSession::new(EngineInputs::hermetic()…)`, then deletes `EngineSession::legacy_test_new`.
  - **Signal.** GUI suites green; row `engine-legacy-test-ctor-gui` at 0.
  - **Keys.** none. Medium.
  - **Deps.** γ₁.
- **δ₃ — The remaining crates; the legacy constructors are deleted.**
  - **Work.**
    - Migrate reify-lsp, reify-eval-fea-tests, reify-compiler, reify-cli, reify-mesh-morph and reify-test-support (library helpers in `helpers.rs` and `fixtures.rs`).
    - Delete `legacy_test_new`, `legacy_test_with_prelude`, `legacy_test_with_solver` and the gated `set_persistent_cache_dir` / `set_max_unfold_*` / `set_warm_state_budget`.
    - Delete the dev-dependency feature lines that γ₁ added and that become unused.
    - Each file converted to `EngineInputs::hermetic()` outside reify-eval's engine modules gets its `KNOWN_SITES` `path|EngineInputs::hermetic` row (§5.12).
  - **Signal.** workspace suites green; row `engine-legacy-test-ctor-rest` at 0; `git grep -n 'fn legacy_test_' -- crates` empty.
  - **Keys.** none. Medium.
  - **Deps.** δ₁, δ₂.
- **ε — `PerBuildState` with `reset(surface)`; INV-BUILD-1.**
  - **Work.**
    - Add `per_build_state.rs` per §5.5, with `BuildSurface` moved there.
    - Delete `reset_per_build_state` and its destructure.
    - Route #9021's reload clear through `clear_handles()`.
    - Fix the relate-facts docs.
    - Update INV-BUILD-1 and INV-BUILD-3 (§5.11).
  - **Signal.** B-ε1; rows `per-build-reset-site`, `reset-per-build-state` and `engine-free-per-build-state` at 0; INV-BUILD-1 reads `enforced(type+test)` with resolving pointers.
  - **Keys.** none (`x_finding_part` fk-800d61acd5d0, closed by λ). INV: INV-BUILD-1. **High.**
  - **Deps.** γ₁, #8862, #8869, #8872, #9017, #9021.
- **ζ — Undef causes beside the snapshot; `EvalArtifacts`; Snapshot docs (R4).**
  - **Work.** Add `eval_artifacts.rs` per §5.6, with the declared-cell set captured at eval. Re-classify on `eval_cached` and `edit_param`. Wire #7038's capture. Delete `set_capture_undef_causes`. Correct the Snapshot docs.
  - **Signal.** B-ζ1, B-ζ2, B-ζ3; rows `undef-cause-field`, `undef-capture-setter`(-gui) and `engine-free-eval-artifacts` at 0.
  - **Keys.** fk-8250a05a9a10, fk-d2c64e9d3868. Subsumes #7038. INV: INV-SF-1 (correct provenance, not a status flip). Medium.
  - **Deps.** γ₁, #8691, #8862.
- **η — `PurposeState`: one map; activation includes the rebuild.**
  - **Work.** Add `purpose_state.rs` per §5.7. The engine_eval re-injection becomes `reload` + `activate`. The stale γ doc is deleted.
  - **Signal.** B-η1, B-η2; rows `parallel-purpose-map`, `purpose-rebuild-pair` and `engine-free-purpose-state` at 0.
  - **Keys.** fk-fb8cf3e787b9. Medium.
  - **Deps.** ζ, #8862.
- **θ — Per-call outputs return on the result.**
  - **Work.** Implement §5.9:
    - add `EvalPass` and `DispatchAttribution`;
    - delete the five fields and their getters;
    - the GUI reads its own calls' results and clears them on reload;
    - `surface_geometry_derived_cells` uses the dispatched set;
    - update every struct-literal construction of `EvalResult` / `CheckResult` / `BuildResult` / `TessellateResult` the compiler names. At least: reify-lsp `analysis.rs` (a production `CheckResult {`), reify-cli `main.rs` tests, `crates/reify-test-support/src/helpers.rs`, `crates/reify-eval/tests/common/differential.rs`, and the gui `tests/{engine_tests,commands_tests,types_tests}.rs`.
  - **Signal.** B-θ1, B-θ2, B-θ3; rows `last-call-getter` and `dispatched-entity-proxy` at 0.
  - **Keys.** fk-e9536a3edbe7. Medium.
  - **Deps.** γ₁, #8944, #8937.
- **ι — `Instrumentation` and `EngineProbe` (R2, the read half).**
  - **Work.** Implement §5.10: delete the always-present counters from Engine; `probe()` and `fault_injection()` replace the read and fault accessors; the remaining test `set_build_scheduler` / `set_mesh_contract_mode` calls become config overrides.
  - **Signal.** B-ι1; rows `engine-gated-accessor` and `engine-free-instrumentation` at 0; suites green; the regression floor (B-λ1's example set) is byte-identical.
  - **Keys.** fk-d8fd9ed9be89. Medium.
  - **Deps.** θ, δ₃.
- **κ₁ — Raw-mutator burn-down I: compute-dispatch tests.**
  - **Scope.** The `cache_store_mut` / `warm_pool_mut` sites in the compute-dispatch tests:
    - in-crate: `engine_compute.rs` (42) and `compute_persist.rs` (8);
    - integration: `cancellation_compute_dispatch.rs`, `material_field_cancellation.rs`, `input_shape_tots_compute_node.rs`, `zv_shaped_ramp_db_reduction.rs`, `harness_fea_solver_e2e/tensegrity_pavilion_e2e.rs`;
    - reify-eval-fea-tests: `modal_compute_node.rs`, `simulate_trajectory_compute_node.rs`.
  - **Work.** Migrate each site under §5.10's policy.
  - **Signal.** row `raw-state-mutator` lowered to the measured remainder; suites green; the regression floor is byte-identical on the FEA/modal examples; per-site dispositions in the commit.
  - **Keys.** none (`x_finding_part` fk-cb6c853cb648, closed by κ₂). Medium.
  - **Deps.** ι, #8699.
- **κ₂ — Raw-mutator burn-down II; the mutators are deleted.**
  - **Scope.** The rest:
    - freshness, geometry-handle, topology-selector, forall, commit-migration, opaque-state and warm-state-donation tests;
    - the in-crate `engine_admin` / `engine_eval` tests;
    - GUI `mark_value_cell_failed_for_test`;
    - every `warm_pool_mut` and `snapshot_mut` site.
  - **Then delete** `cache_store_mut`, `warm_pool_mut`, `snapshot_mut`, `CacheStore::insert_synthetic_realization_entry` and, if unused, `ValueCellSeal::for_test_seeding()`.
  - **Signal.** `raw-state-mutator`(-gui) at 0; the accessors are gone; suites green; the regression floor is byte-identical.
  - **Keys.** fk-cb6c853cb648. Medium.
  - **Deps.** κ₁.
- **λ — `EngineCaches`, `EngineLineage`, `HostInputs`; Engine holds only sub-structs.**
  - **Work.** Implement §5.8 and the struct of §5.3, including the homes for #9021's fields. Re-key `version_id_discipline_gate.rs`'s allowlist to `lineage.rs`. The 701-line Engine field doc becomes one doc per sub-struct.
  - **Signal.** B-λ1; `pub struct Engine` has exactly the ten sub-struct fields of §5.3; `version_id_discipline_gate` green; rows `engine-free-engine-caches`, `engine-free-lineage` and `engine-free-host-inputs` at 0.
  - **Keys.** fk-800d61acd5d0, `language-pipeline|crates/reify-eval/src/lib.rs::engine|h11`. **High.**
  - **Deps.** ε, ζ, η, θ, ι, κ₂, #8680, #9021.
- **μ — h14 partitions of engine_admin.rs and lib.rs.**
  - **engine_admin.rs.** `kernel_pin_diagnostics` → `engine_config.rs`; `sweep_persistent_cache_at_startup` → the post-#8876 persistent-cache module; `populate_structure_registry` / `module_has_auto_cells` → `lineage.rs`; undef tracing → `eval_artifacts.rs`; tests move with their unit.
  - **lib.rs.** `EngineError`, `value_type_kind_matches` and its tests → `value_kind.rs`; the result structs and their codes → `results.rs`. `Engine` stays at the crate root.
  - **Signal.**
    - each file's production half is below the h14 alarm (`docs/code-quality.md` heuristic 14 measurement);
    - `module_scc_ratchet` green;
    - B-λ1 still byte-identical.
  - **Keys.** fk-ba15d44904cb. Medium.
  - **Deps.** λ, #9007, #8864, #8691, #8699, #8876.
- **ω — PRD close.** Set the terminal Status stamp, the AS-AUTHORED freeze and the LIVE map, plus the matching manifest header (overlay "PRD terminal status").
  - **Signal.** The committed header.
  - **Deps.** every leaf.

**Out-of-batch edges.**
- S27 φ #8776 ← γ₁, γ₂ (released by this stream).
- #8564 ← ζ.
- #7038 is cancelled as subsumed by ζ.
- The gate #8612 depends on γ₁, γ₂, ε and λ.

**DAG summary.** α → β → γ₁ → {γ₂, δ₁, δ₂, ε, ζ, θ}; {δ₁, δ₂} → δ₃; ζ → η; {θ, δ₃} → ι → κ₁ → κ₂; {ε, ζ, η, θ, ι, κ₂} → λ → μ; every leaf → ω.

## 10. Measurement record (dated: main `988f4522b9`, 2026-10-14)

1. **Engine fields.** 68 (2 cfg-gated). Rule: fields at brace depth 1 of `pub struct Engine` (lib.rs 462–1236); the struct literal in engine_admin.rs and the `reset_per_build_state` destructure (engine_build.rs 3360) name the same 68. 63 remain after #8862 and #8869.
2. **Struct and impl layout.**
   - Struct body: 775 lines, 701 of them comments (rule: lines whose first non-space characters are `//`).
   - `impl Engine` blocks: 24 in 17 files (23 inherent plus one `ContainmentQuery` impl in engine_eval.rs).
   - Reset arms: 8 of 68 fields are reset (5 every surface, 1 build, 1 tessellate, 1 Build-only); 60 are bound `_`.
3. **Production construction sites: 18** (rule: `Engine::(new|with_prelude|with_registered_kernels?(_and_manifest)?|with_test_kernels_and_registry)(` outside `tests/` and `cfg(test)` modules).
   - **Per crate.** reify-cli 10 (main.rs 9, mcp_context.rs 1), reify-lsp 4, gui/src-tauri 3, reify-eval `test_runner.rs` 1.
   - **Posture matrix.**

     | front end | persistent cache | compute registration | solver | kernels | undef capture |
     |---|---|---|---|---|---|
     | CLI check | no | none | no | occt (geometry arm) / none | no |
     | CLI build | no | bundle | no | occt (+openvdb if isosurface) | no |
     | CLI eval / report / explain | yes | bundle | production | occt (geometry arm, no openvdb) / none | eval only |
     | CLI MCP | yes | none | no | none | no |
     | LSP ×4 | no | none | no | none | analysis.rs only |
     | GUI session | never set (sweep only) | bundle | production | occt + openvdb | always |
     | GUI `get_def_preview` | no | none | no | none | no |
     | `test_runner` | no | bundle (morph unavailable) | no | none | no |
   - **The lint.** `KNOWN_SITES` has 3 rows.
4. **Test constructions: 1,124 in 305 files.**
   - By constructor: `new` 1,039, `with_prelude` 38, `with_registered_kernel` 14, `with_registered_kernels` 12, `with_test_kernels_and_registry` 12, `with_registered_kernels_and_manifest` 9.
   - By crate: reify-eval 1,095, reify-lsp 11, reify-eval-fea-tests 7, reify-compiler 6, reify-cli 4, reify-mesh-morph 1.
   - The GUI tests add 307 `EngineSession` constructions.
   - Checker arguments: `MockConstraintChecker` 279, a `checker` variable 551, `SimpleConstraintChecker` 103.
5. **Env reads in reify-eval production: 6 runtime knobs.**
   - Read in the constructor: `REIFY_BUILD_SCHEDULER`, `REIFY_MESH_CONTRACT` (in reify-ir) and `REIFY_WARM_STATE_BUDGET_BYTES`.
   - `REIFY_WARM_STATE_BUDGET_BYTES` plus the manifest is read only by `set_warm_state_budget` (no production caller).
   - `REIFY_LONG_CHAIN_THRESHOLD_MS` is read per call at 4 build entry points.
   - `PATH` is read per `fdm_slice` dispatch.
6. **Registry rows (verbatim at `988f4522b9`).**
   - INV-FEA-1: `| INV-FEA-1 | Every production engine declares its registration posture; full registration has one constructor | type+test (`Engine::new_production`; grep architecture test; locking tests for deliberate opt-outs incl. LSP) | proposed | compute-fea-hardening |`.
   - INV-BUILD-1: `| INV-BUILD-1 | Per-build engine state resets exactly once per build/tessellate entry point; per-build vs must-survive classification is explicit | type (`reset_per_build_state` exhaustive destructure; later `BuildSession` ownership) | enforced(type) | engine-build-hardening |`.
7. **Test seams.**
   - **Gated `pub fn`s:** 28 on Engine, 1 on `CacheStore` (`insert_synthetic_realization_entry`) and 1 free fn (`compile_geometry_op_probe`).
   - **Calls** (rule: `NAME(` on tracked `.rs` lines, comments and the definition excluded): `cache_store` 224, `cache_store_mut` 119, `set_build_scheduler` 45, `realization_cache` 36, `last_dispatch_count` 33, `warm_pool` 27, `warm_pool_mut` 25, `last_changed_realizations` 23, `set_panic_on_eval` 17, `with_test_kernels_and_registry` 13, `set_achieved_repr_tol_for_test` 13.
   - **`cache_store_mut` sites.** 59 seed (57 `.put`, 2 `record_*`), 54 mutate another way, 0 are read-only. All are test code. 42 of them are in `engine_compute.rs`'s in-crate tests. #8699 deletes 29.
8. **Per-call outputs.**
   - 5 GUI production reads of the three ungated `last_*` getters, plus `last_substantive_value` (a cache lookup, out of scope).
   - `last_demand_prune_measurement`: 1 write (`edit_param`), 0 clears.
9. **Purpose maps** (production sites, field declaration excluded): `compiled_purposes` 8, `active_purposes` 9, `active_purpose_bindings` 8, `active_tolerance_scope` 7, `active_objective_map` 7, `active_purpose_let_cells` 6. There are 4 activate→rebuild pairs and 1 inlined rebuild. Only reify-cli (main.rs) calls the purpose API outside reify-eval.
10. **Kernel registry and anchors.**
    - Production `extern crate reify_kernel_*` anchors: CLI 3, GUI 1. reify-eval tests: 17 code anchors in 12 files, plus 7 doc-comment mentions.
    - Production global-registry reads: `collect_registry()` 5 in engine_build (S10 ι₄); `registry()` 4 in engine_admin plus 1 in engine_constraints.

## 11. Rulings (Leo, 2026-10-14, this session) and open questions

**Rulings.**
- **R1 — test construction.** Leo asked which option gives the best test architecture and the most reliably accurate, easily written and maintained tests in the long run. The answer given: one constructor for front ends and tests, with tests starting from a hermetic preset. Confirmed: **one `Engine::new(EngineInputs)`, tests start from `EngineInputs::hermetic()`, a scripted migration leaf pinned by a ratchet row, and no gated test constructor survives** (D1; leaves γ₁, δ₁–δ₃).
- **R2 — test seams.** **`EngineProbe` (read) plus raw mutators to 0** in this PRD (D11; leaves ι, κ₁, κ₂).
- **R3 — GUI persistent cache.** **The GUI uses it, through the CLI's layered resolver moved into reify-config** (D8; leaf α).
- **R4 — undef causes after an edit.** **Re-classify the re-evaluated cone, carry the rest, tag the map with its snapshot; subsumes #7038** (D9; leaf ζ).

**Critic fold record** (fable/high critic, 2026-10-14; verdict "ship-after-fold"):
- **Blockers folded.**
  - B1: gating the legacy constructors would red reify-cli, reify-compiler and reify-mesh-morph tests. γ₁ enables the feature on their dev-dependency first (§3, D1).
  - B2: new modules would GROW the 24-module SCC before #8687. α ← #8687; D5 restated "as baselined at landing"; `BuildScheduler` moves into `engine_config.rs`.
- **Majors folded.**
  - M1: the declared-cell set is captured at eval (§5.6).
  - M2: `per-build-reset-site` re-scoped; it was vacuous, and now counts 5 today and 0 at ε.
  - M3: S12 ε3 #9021's fields are homed (§5.3); ε and λ ← #9021.
  - M4: γ₁ ← S12 ζ1 #9022 and ζ2 #9023; the `cmd_check` rows are collapsed.
  - M5: γ₁ owns `KernelSet::from_registrations` and the member-only `ensure`; the registry row is widened.
  - M6: the sibling-PRD contradictions are carried by task notes and cross-reference lines (one editor per document).
- **Minors folded.** Kebab-case row ids; in-crate env-reading tests; regression-floor pins on ι, κ₁, κ₂; the version-gate allowlist re-key; knob-setter timing; several `KNOWN_SITES` rows per path; the narrowed κ₁ check; θ's struct-literal sites; the GUI preset location; S12 / S5 ids; the #6665 row note; the γ and δ splits (D17); B-γ3 made pre-measured.
- **Fold-verifier pass (opus/high), folded.**
  - `declared` carries decl spans and is captured by `eval_cached` too.
  - The knob setters are gated, not deleted, at γ₁.
  - reify-test-support library sites are covered, with the exact dev-dependency additions.
  - `with_solver` and `EngineSession::new(checker, kernel)` get legacy renames instead of deletion at γ₁.
  - The δ₁, δ₃ and α checks are re-aimed so that their own leaf turns them green.
  - The legacy-test rows count call sites only.
  - Discovery accepts `EngineInputs::hermetic`, and engine modules include subdirectories.
  - `Engine::config()` is added in α.
  - The surface-sequence test is a `harness_engine` module.
  - Stale labels, the anchor count (17) and the key-mapping wording are fixed.
  - #8992 is added to the note list.
- **Flagged, not changed.** γ₁ sits behind S5 κ₁ #8999's dependency chain and S10 ι₃/ι₄, so the stream's high leaves are far downstream. That is accepted for a wave-3 stream.

**Open questions (tactical; decide in the named leaf).**
1. **`KernelSet::from_registrations` signature details** (γ₁). Examples: whether `primary` is an `Option<KernelId>`, and whether `ensure` on a non-member returns an error or `None`. Suggested: mirror S10's `KernelSet { entries, primary: Option<KernelId> }`. A non-member `ensure` returns the `SelectionError::OwnerAbsent`-style coded error, never a silent instantiation.
2. **Project-directory rule** for `ConfigSources::from_process` (α). The CLI uses the process cwd. The GUI could use the cwd or the opened file's directory. Suggested: the process cwd for both, so B-α1 compares like with like. A per-file project config is a follow-up.
3. **`EngineKnobIgnored` wording and severity** (α). Suggested: Warning, naming the variable and the value it fell back to.
4. **`DispatchAttribution` ordering** (θ). Suggested: `BTreeMap` for deterministic debug JSON; key names unchanged.
