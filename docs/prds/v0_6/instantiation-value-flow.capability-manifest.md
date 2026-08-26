# Capability manifest — instantiation-value-flow

PRD: `docs/prds/v0_6/instantiation-value-flow.md`. Evidence bound 2026-08-26
against main `041554eae524`; symbol evidence re-verified by grep in the
authoring session (cite-by-symbol; the PRD header carries the dated SHA).
Probe evidence: the D3 decompose-verify workflow (`wf_e306782c-e1a`) ran first
and BLOCKED — a module-path defect in the probe fixture (fixed), plus two
adversary findings folded into the PRD (B5 ownership → ζ; B8 verdict-pin).
The corrected fixture set was then re-verified through the same deterministic
α harness the workflow drives (`scripts/prd-capability-check.py --json`,
probe-set in the session record, `REIFY_BIN=target/debug/reify` rebuilt
2026-08-26 against main `041554eae524`): **five probes, all PASS** — grammar +
resolve on the probe fixture; silent-accept baselines observed verbatim
(B7: exit 0, "All constraints satisfied."; B8: "INDETERMINATE
Top#constraint[0]" + "No constraints violated (1 indeterminate)", exit 0;
B9: "OK Bar#constraint[0]", exit 0). Machine-readable twin:
`instantiation-value-flow.capability-manifest.yaml`.

## α — adopt #6586 (ordering + undef-ctor-arg diagnostic)

- `phase-1.5-walk-substrate` — capability→producer: the dependency-ordered
  instance-scope walk to port exists and is wired on the #5360 production path
  (`elaborate_child_instance_nested`, `crates/reify-eval/src/unfold.rs`; 7
  references in file). PASS.
- `undef-cause-substrate` — `pub enum UndefCause` exists
  (`crates/reify-ir/src/value.rs`) with tracer (`undef_tracer.rs`). PASS.
- `undef-ctor-arg-rejection` — G6 branch 4: silent-accept OBSERVED today
  (D3 probe: `reify check` on the α-baseline fixture exits 0, no diagnostic,
  child unrealized) ⇒ rejection mechanism absent; producer = this leaf α
  (it delivers the diagnostic). Bound as producer-self, not rejection-absent
  FAIL — the observed absence is the leaf's premise, not its assumption. PASS.

## β — adopt #6592 (one valuation → placements, descendant args, geometry)

- `instance-value-cells-correct` — capability→producer: instance-scope cells
  already correct for the whole plain-sub subtree (producer: task #5360, done;
  `unfold.rs` phase-1.5). PASS.
- `one-level-override-machinery` — the machinery to generalize is wired on
  main: `realize_sub_override_handles` (`crates/reify-eval/src/engine_build.rs`)
  invoked from the surfacing walk; documented scope cut "one level of override
  depth only" (`engine_build.rs` rustdoc, banner at `seed_cross_sub_named_steps`).
  PASS.
- `pose-eval-site` — the single pose evaluation site to overlay:
  `eval_sub_pose` (`crates/reify-eval/src/geometry_ops.rs`), called from
  `walk_placed_realizations`. PASS.
- `probe-fixture-parses` — grammar-fixture:
  `tests/prd-gate/fixtures/instantiation_value_flow_probe.ri`, D3: parses,
  0 ERROR nodes; `reify check` exits 0 (valid input, geometry plane silently
  wrong per #6592's STEP/GUI evidence). PASS.

## γ — loud-failure convergence

- `diagnostic-code-registry` — `DiagnosticCode` registry
  (`crates/reify-core/src/diagnostics.rs`) with typed-code test precedent. PASS.
- `severity-exit-house-pattern` — producer: task 4458 (done) — `cmd_eval` /
  `cmd_build` Severity::Error exit gates; γ converges `reify check` on the same
  rule (INV-SF-2 house pattern). PASS.
- `check-fails-on-unrealized-rejection` — G6 branch 4: silent-accept OBSERVED
  today (D3: `reify check` on the probe fixture exits 0, "All constraints
  satisfied.", while the geometry plane is default-shaped); producer = this
  leaf γ. PASS (producer-self).

## δ — instance-scoped constraint checking

- `rescoping-precedent` — `map_value_refs`
  (`crates/reify-ir/src/expr.rs`) used for entity rescoping in
  `post_process_cross_sub_value_cells` (`engine_build.rs`). PASS.
- `runtime-constraint-node-precedent` — the forall per-element emission ledger
  (`forall_templates`, `crates/reify-eval/src/graph.rs`) materializes
  constraint nodes at runtime with drain/resize discipline (engine_edit
  collection-count phase). PASS.
- `dispatch-reusable` — `dispatch_constraints`
  (`crates/reify-eval/src/engine_constraints.rs`) is the shared dispatcher δ
  feeds per-instance clones through. PASS.
- `override-violation-rejection` — G6 branch 4: silent-accept OBSERVED today
  (D3 probe: `Bar(length: -5mm)` against `constraint length > 0mm` — check
  exits 0, no violation reported); producer = this leaf δ. PASS (producer-self).

## ε — test-plane completion + story correction

- `pinned-boundary-test-exists` —
  `cross_sub_nested_sub_in_override_path_produces_compile_error`
  (`crates/reify-eval/tests/cross_sub_geometry_e2e.rs`), whose doc mandates
  re-baselining when nested overrides land. PASS.
- `values-plane-suite-exists` —
  `nested_constructor_arg_threads_through_two_levels`
  (`crates/reify-eval/tests/harness_engine/nested_sub_derived_let_e2e.rs`),
  kernel-less; the geometry-plane siblings ε adds attach here. PASS.

## ζ — integration gate

- `step-grep-channel` — the canonical CLI STEP observation channel exists:
  `crates/reify-cli/tests/harness_cli/cli_sub_placement_assembly.rs` (runs
  `reify build`, greps `MANIFOLD_SOLID_BREP(`). PASS.
- `mesh-stats-channel` — GUI debug MCP `mesh_stats`
  (`gui/src-tauri/src/debug_server.rs`, parity-locked by
  `debugParity.test.ts`). PASS.
- `fixture-registration-mechanism` — `_RUST_COUPLED_RI_FIXTURES`
  (`scripts/verify.sh`) + PG-DRIFT (`tests/infra/test_verify_scope.sh`)
  re-derives the coupled set; ζ registers both fixtures in the same diff. PASS.
- `oracle-harness-premise` — the Shape-A oracle (instance ≡ hand-specialized
  twin) is exactly the mechanism the dogfood duplicates prove works end-to-end
  (`FairleadPairMirrored`, dogfood branch commit 804236401a; #6592 record). PASS.

## θ — docs-truth bundle

- `chunks-surface` — `crates/reify-mcp/src/tools/chunks/*.md` exists; overlay
  docs-truth gate defines the four deliverables; signature verification against
  the compiler registries per overlay. PASS.
- `exemplar-gate` — `examples/best_practices/` auto-compile-gated by
  `examples_smoke.rs`. PASS. NOTE: the exemplar itself is only writable AFTER β
  lands (per-instance sizing driving nested geometry is newly possible) — θ
  depends on β, DAG-direction correct.

## ι — warm-trace follow-up

- `gap-evidence` — instance param cells commit with `DependencyTrace::default()`
  (`unfold.rs`, phase-1 commit site); the warm reverse-index cone
  (`engine_edit.rs`) therefore has no edge from a parent cell into a ctor-arg
  instance cell. Verified by the fix-shape groundwork read. PASS (gap named,
  producer = this leaf).

## κ — solved-value-auto consumption in the per-instance overlay

Added 2026-08-26 (approved by Leo, solver-integration session; evidence bound
by the IVF-sequencing study against main `181d1ec24c`/`62a47674fd` — memo in
#6631/#6592 details).

- `scoped-auto-cell-substrate` — auto ctor args are filtered out of
  `compiled_args` (`extract_auto_free` filter arm) and minted as parent-scoped
  Auto cells (bare-args + spec-body arms, `crates/reify-compiler/src/entity.rs`).
  Trace-verified. PASS.
- `solver-scoped-write-back` — the dimensional solver writes solved autos back
  under the scoped instance keys (Solved-arm write-back,
  `crates/reify-eval/src/engine_eval.rs`). Trace-verified. PASS.
- `own-scope-post-solve-rail` — own-scope let/param autos already realize
  post-solve under eval (solved template let-cone re-run); probe-verified
  (auto_let.ri realizes at the solved 10mm). PASS.
- `silent-default-baseline` — G6 rejection baseline: a solved ctor-arg auto
  realizes the child DEFAULT today (auto_ctl.ri: eval solves 0.01 m, `reify
  build` STEP z-extent −5, exit 0). Probe-verified; κ + the #6631 posture
  flip retire it. PASS.

## η — PRD close

- No capabilities: docs stamp per overlay decompose-close obligations
  (terminal vocabulary; AS-AUTHORED freeze; ID backfill).
