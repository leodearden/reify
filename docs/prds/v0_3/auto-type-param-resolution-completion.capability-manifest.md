# Capability manifest — `auto:` Type-Parameter Resolution v0.2 Completion

Mechanizes G3 (substrate-exists) + G6 (premise-valid) for
`docs/prds/v0_3/auto-type-param-resolution-completion.md`. Built at decompose
(2026-06-09), verified against `main` @ `1d192cdae4`. Each **leaf** signal's
asserted capabilities are bound to on-`main` evidence; any binding resolving to
`declared-only | test-only | producer-downstream | producer-absent |
fixture-ERROR | bound≤floor` blocks the batch. **All bindings PASS.**

## Substrate corrections discovered at decompose (thread into task descriptions)

These do **not** block — the substrate exists — but the PRD cites the wrong
home/line for two of them. Implementers should use the corrected coordinates:

| PRD claim | Reality on `main` | Affected tasks |
|---|---|---|
| §6.4 / §12-γ: new diagnostic registered in `crates/reify-ir/src/diagnostics.rs` ("home of the existing `AutoTypeParam*` codes") | The existing `AutoTypeParam*` variants live in **`crates/reify-core/src/diagnostics.rs`** (alongside `Type`; the `ConstraintChecker` *trait* is the thing in reify-ir, at `constraint.rs:198`). `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` (γ) and `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE` (δ) go where their siblings live = **reify-core**. | γ, δ |
| §5.2 / §6.3: `build_constraints_template` 2nd call-site "≈:1377" | Actual 2nd call-site is **`auto_type_param.rs:1392`** (1st at `:786` ✓). Hoist/`NOTE(substitution-pass-trigger)` markers confirmed at `:777,1338,1383,2040`; helper def `:2045`; `build_constraint_blame_map` `:1931` (PRD said ≈1902-1941). | β, γ |
| §10 / memory: `Value`-side homes | `reify-types` crate is **deleted** (renamed). `Value`/`Value::StructureInstance` → `reify-ir/src/value.rs:955`; `Type` → reify-core. Task **3751**'s record still cites `crates/reify-types/...` — ε's repoint is the moment to update those paths to reify-ir/reify-core (impl-time note, not a batch dep). | ε (re: 3751) |

## Foundation tasks (intermediate — substrate assertions pinned)

### α — Monomorphize + apply `TypeParam→StructureRef` (keystone, L1)
Signal is a `reify-compiler` integration test (intermediate; user-observable proof
deferred to β/δ/ζ). Substrate it builds on:

| Capability | Evidence | Verdict |
|---|---|---|
| Recursive type-param walker to reuse | `substitute_type_params` @ `reify-compiler/src/type_resolution.rs:1161` | `grep wired` PASS |
| Pass-2 site w/ `Σ` + `&mut ctx.templates` in scope | `auto_type_param_phase.rs:180-187` | `grep wired` PASS |
| Lookup-by-name forces synthesized-name rewrite | `EvaluationGraph::from_templates` `graph.rs:278`; `find_template` by `t.name` ignoring type_args `reify-compiler/src/types.rs:789`, called `graph.rs:365` | `grep wired` PASS |
| Representability invariant α must satisfy (net-positive) | `assert_value_cell_types_representable` `engine_eval.rs:144`; `Type::TypeParam`→panic, `Type::StructureRef`→permitted (`is_representable_cell_type`) | `grep wired` PASS |
| Hard prereq (call-site + slot rewrite) | task **3558** done (`8d1cf09598`) | `producer:3558 done` PASS |

### β-inject — Thread `&dyn ConstraintChecker` through `compile_*` (intermediate, L2 plumbing)

| Capability | Evidence | Verdict |
|---|---|---|
| Real checker exists to inject | `SimpleConstraintChecker` @ `reify-constraints/src/lib.rs:47` | `grep wired` PASS |
| Trait object to thread | `ConstraintChecker` trait @ `reify-ir/src/constraint.rs:198` | `grep wired` PASS |
| Crate-DAG layering (why injection, not dep) | `reify-compiler/Cargo.toml` has **no** `reify-constraints`; `reify-eval/Cargo.toml:64` has it **dev-only** | `grep wired` PASS |
| Existing injection point in a binary | `gui/src-tauri/src/main.rs:651` constructs `SimpleConstraintChecker` | `grep wired` PASS |
| no-op-on-stub invariant | default-stub `CompileTimeIndeterminateChecker` `auto_type_param_phase.rs:52-66` (kept) | `grep wired` PASS |

## Leaf tasks (user-observable signals — the load-bearing bindings)

### β — Per-candidate feasibility → `examples/auto/bearing_constraint_select.ri` selects the unique survivor (today `Ambiguous`)

| Asserted capability | Evidence | Verdict |
|---|---|---|
| Selection *changes* under the real checker | producer: α (monomorph) + β-inject (CLI supplies `SimpleConstraintChecker`); both are β's deps → signal producible from β's dep set | `producer:α,β-inject upstream` PASS (G6 branch-3) |
| Per-candidate `ValueMap` seeds **real** field values (not Undef) | candidate defaults are declared literals (e.g. `GasketSeal.thickness=2mm`, `bearing_auto_seal.ri:55`) — real `Value`, not `Value::Undef` | `field-population: real-default` PASS |
| Constraint-blame fires in-loop | `build_constraint_blame_map` `auto_type_param.rs:1931` | `grep wired` PASS |
| Fixture parses | `auto-completion-2.ri` (strict `auto: Seal`, 2 candidates, member constraint) `tree-sitter parse --quiet` exit 0 | `grammar-fixture parses` PASS |
| Numeric floor | discrete type selection, no approximation | floor N/A |

### γ — Joint-recheck + `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` → `examples/auto/bounded_fallback_unsound.ri` emits the hard error; `auto_fallback_soundness` proves the invariant

| Asserted capability | Evidence | Verdict |
|---|---|---|
| New diagnostic is genuinely new | no `BOUNDED_INFEASIBLE`/`BoundedInfeasible` variant on `main`; siblings in `reify-core/src/diagnostics.rs` (`AutoTypeParam{NoCandidate,Ambiguous,NonUnique,DepthBoundExceeded,CrossProductSizeExceeded,PoolOverflow}`) | `grep wired` PASS — **register in reify-core, not reify-ir** |
| Joint-recheck has a real per-A feasibility to call | producer: β (candidate-dependent feasibility) — γ's dep | `producer:β upstream` PASS |
| Hoist-revert targets exist | `NOTE(substitution-pass-trigger)` `:777,1338,1383,2040`; `build_constraints_template` `:2045` | `grep wired` PASS |
| Graceful-degradation retained (Warning when jointly feasible) | existing `AutoTypeParamDepthBoundExceeded`/`...CrossProductSizeExceeded` Warnings preserved | `grep wired` PASS |
| Fixture parses | combinatorial `structure def` over existing grammar (>max_depth=6 auto-params); no novel syntax | `grammar-fixture parses` PASS |

### δ — Auto-construct resolved param → `examples/auto/bearing_resolved_value.ri`: `b.seal.thickness == 2 mm` (non-`Undef`); non-constructible candidate → `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE`

| Asserted capability | Evidence | Verdict |
|---|---|---|
| SIR value ctor to synthesize over | `eval_structure_instance_ctor` `reify-expr/src/lib.rs:910`; `Value::StructureInstance` `reify-ir/src/value.rs:955` — task **3540** done (`3faa8373de`) | `producer:3540 done` PASS |
| Default-branch + Undef fallthrough to replace | `elaborate_child_params_only` `unfold.rs:291`, default branch `:336-342`, `Value::Undef` fallthrough `:344` | `grep wired` PASS |
| `2 mm` is producible & exact | GasketSeal's own declared default `thickness = 2mm` (`bearing_auto_seal.ri:55`); δ copies the candidate's real default into the synthesized ctor — value-copy round-trip, **not** a computed approximation | `field-population: real-default` PASS; numeric floor **N/A — exact by construction** (G6) |
| Monomorph clone to carry the ctor | producer: α — δ's dep | `producer:α upstream` PASS |
| New diagnostic home | `reify-core/src/diagnostics.rs` (sibling of the AutoTypeParam* codes) | PASS — **reify-core, not reify-ir** |
| Fixture parses | `auto-completion-1.ri` (`param seal : T` + `b.seal.thickness` chain) exit 0 | `grammar-fixture parses` PASS |

### ε — M-007 real-source exercise + task-graph/audit reconciliation → `auto_backjumping_real_source` passes; grep shows M-013 WIRED; 3751 `depends_on` α

| Asserted capability | Evidence | Verdict |
|---|---|---|
| Backjump path to exercise from real source | `build_constraint_blame_map` `:1931`, `DfsControl::BackjumpTo` (`auto_type_param.rs:2174-2268`); task **2660** done but only `MockConstraintChecker`-exercised | `grep wired` + `producer:α,β upstream` PASS (real-source exercise needs β's in-loop substitution) |
| 3522 is closable-superseded | 3522 **pending**, deps `[]`; its (a)call-site + (b)populate landed in 3558 (`8d1cf09598`) | `producer:3558 done` PASS |
| 3751 is repointable | 3751 **pending**, currently `depends_on [3522]` | `grep wired` PASS (real edge mutation owned by ε) |
| Audit rows exist to flip | `findings/auto-resolution-backtracking.md:9` state breakdown + per-mechanism rows (M-002/014 FICTION, M-013 TODO, M-005/006/007 PARTIAL) | `grep wired` PASS |
| Numeric floor | none | N/A |

### ζ — Integration gate (B+H acceptance) → four `examples/auto/*.ri` pass `cargo test -p reify-eval --test auto_type_param_completion_e2e`; workspace green; real binary injects the checker

| Asserted capability | Evidence | Verdict |
|---|---|---|
| All four producers landed | producer: α,β,γ,δ,ε (ζ's deps) | `producer:α,β,γ,δ,ε upstream` PASS |
| Real binary actually selects under `SimpleConstraintChecker` | `gui/src-tauri/src/main.rs:651` + β-inject wires `reify-cli/src/main.rs` | `grep wired` + `producer:β-inject upstream` PASS |
| §11 two-way boundary tables realizable | each row maps to a landed seam (producer rows = reify-compiler/eval; consumer rows = stub no-op invariant, fn-generics/DCE unchanged, LSP/MCP/CLI diagnostic flow) | PASS |
| Aggregate test target | ζ owns the new `auto_type_param_completion_e2e` harness binding β/γ/δ's four fixtures | (deliverable; no substrate gap) |

### θ — Supersede v0.1 parent → `grep -E '^Status:.*[Ss]uperseded' docs/prds/auto-type-param-resolution.md` returns the marker

| Asserted capability | Evidence | Verdict |
|---|---|---|
| v0.1 parent exists to edit | `docs/prds/auto-type-param-resolution.md` present (no current `Status:` line → θ prepends `## §0 — Superseded`) | `grep wired` PASS |
| v0.2 parent exists for completion note | `docs/prds/v0_2/auto-resolution-backtracking.md` present | `grep wired` PASS |
| Residuals resolved before retiring | producer: α,β,γ,δ (θ's deps) | `producer:α,β,γ,δ upstream` PASS |

## Summary

8 tasks, 0 FAIL bindings. Every leaf's user-observable signal is producible from
its own dependency set (no G6 branch-3 misattribution); the single asserted number
(`2 mm`) is exact-by-construction (candidate-default value-copy), so no numeric
floor applies. Grammar: no novel syntax — `auto-completion-{1,2}.ri` parse 0-ERROR;
`grammar_confirmed=true` for the whole batch. Two PRD path/line citations are
corrected above (diagnostic home reify-core≠reify-ir; `build_constraints_template`
2nd call `:1392`) and are threaded into the γ/δ/β task descriptions.
