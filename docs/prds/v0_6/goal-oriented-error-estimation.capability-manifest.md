# Capability manifest — `goal-oriented-error-estimation`

**PRD:** `docs/prds/v0_6/goal-oriented-error-estimation.md` · **Machine twin:** `goal-oriented-error-estimation.capability-manifest.yaml`

**Measured against main `191d6c85ba` (2026-09-11).** Every binding below was verified first-hand in the
authoring session and re-verified by an adversarial review pass; none is inherited from the chartering
record. Three bindings **correct** the draft the review saw — the point-functional regularization
(β/`ball-mean-functional-…`), the fallible estimator seam (β/`estimator-seam-is-infallible-…`) and the
CLI-reachability of the two overloads (γ/`dims-overload-reaches-…`).

**Decompose-time verification (`scripts/prd-decompose-verify.mjs`, run 2026-09-11 on leaves α/β/γ/δ/ζ).**
Two adversary findings changed the design and are bound below: the §5.6 read-back originally used
`match … { some(e) => … }`, which does not parse (γ/`readback-form-…`), and BT5's `final_indicator`
clause was unconditional although the committed budget ends `NotConverged` (γ/`committed-budget-…`).
Two more became obligations: `target_quantity_of_interest` must enter the persistent FEA cache key
(γ/`qoi-enters-the-fea-cache-key`), and eval-stderr assertions need a fresh `REIFY_CACHE_DIR`. The
remaining FAIL records were harness artifacts (the α→γ edge it could not see — wired; the fixtures being
untracked until this commit; `probe_kind=ir` polarity) and are recorded in the hand-back, not here.

Mechanizes G3 (substrate) + G6 (premise validity) per leaf. **28 bindings, all PASS — no binding blocks
the batch.** Mechanical (`grep`) checks are copied into producer `metadata.delivered_checks` by
`commit_planning`; `manual` checks stay sidecar-only and are excluded from the dispatch gate.

Every mechanical pattern below was confirmed **currently absent** on main (measured counts: `VariantCtor`
0, `compute_dual_weighted_indicator` 0, `LocalDisplacement` 0, `QoiRequiresAdaptive` 0,
`goal_oriented_refinement` 0, `dwr-effectivity` 0), so each is a genuine landing signal rather than a
vacuous match.

Committed evidence fixtures (all parse with zero ERROR nodes at the anchor SHA; `reify check` results
recorded in each header): `tests/prd-gate/fixtures/dce_runtime_payload.ri`,
`tests/prd-gate/fixtures/dwr_cantilever_energy.ri`, `tests/prd-gate/fixtures/dwr_cantilever_qoi.ri`,
`tests/prd-gate/fixtures/dwr_qoi_without_adaptive.ri`, `tests/prd-gate/fixtures/dwr_qoi_readback.ri`
(the one that PASSES today — it pins the read-back substrate).

---

## α — runtime payload construction for data-carrying enums

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `literal-only-payload-rule-is-real-and-task-less` | G6 rejection premise, **observed to fire**: `reify check tests/prd-gate/fixtures/dce_runtime_payload.ri` exits 1 with `non-constant payload value for field 'width' of variant 'Rect' is not yet supported`; the arm is `variant_construct.rs`'s `CompiledExprKind::Literal`-only payload match, whose deferral comment cites no task id; `enums.md` "Payload limits" documents it. No live task owns lifting it (task search 2026-09-11). | PASS | grep `non-constant payload value` **absent** in `crates/reify-compiler/src/variant_construct.rs` |
| `structure-ctor-node-is-the-template` | substrate, wired-on-main. `CompiledExprKind::StructureInstanceCtor { ordered_args, defaults, lets, … }` (`reify-ir/src/expr.rs`) with its `reify-expr` eval arm delegating to `eval_structure_instance_ctor`; `Value::Enum.payload: Vec<(String, Value)>` is unconstrained and already carries nested enums on the production path (`convergence_status_to_value`). A `VariantCtor` mirrors a live shape. | PASS | grep `VariantCtor` present in `crates/reify-ir/src/expr.rs` |
| `payload-field-types-already-resolve` | G3 substrate. Declaring `LocalDisplacement { at: Point3<Length>, radius: Length, direction: Vector3<Dimensionless> }` with `param q : Option<QoIDescriptor> = none` passes `reify check` today; only construction is blocked. | PASS | manual — a positive property of today's compiler; the fixture-level check is the α signal. |
| `docs-truth-chunk-and-spec-move-in-the-same-diff` | Docs-truth gate. `enums.md` "Payload limits" first bullet states the literal-only rule verbatim; the spec (§3.8/§4.5) describes named-field payloads without the restriction. Both are language surface this leaf changes. | PASS | grep `must be compile-time literals` **absent** in `crates/reify-mcp/src/tools/chunks/enums.md` |

## β — dual-load assembler, dual-weighted indicator, fallible estimator seam

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `ball-mean-functional-is-bounded-so-the-identity-holds` | **CORRECTION to the draft**, G6 branch 2. A point value `d·u(x)` in 3-D is not bounded on `H¹` (Dirac load); `analytical_validation.rs`'s module doc records the single-node-force divergence (P1 error 2.3 % → 10.3 % from 12³ to 24³) and reads deflection as a face mean for that reason. The shipped functional is a **ball mean** over `E = {K : centroid ∈ ball} ∪ fallback {K ∋ at}` — an `L²` functional, so `J(u)−J(u_h) = a(e_z, e_u)` holds and effectivity has a limit. `E` is never empty and `g` never zero for a non-zero direction. | PASS | manual — a mathematical property; the behavioural cover is BT1 (reciprocity) + BT3 (patch exactness with a non-trivial dual). |
| `recovery-substrate-is-all-present` | substrate, wired-on-main. `element_stress_p1`, `recover_nodal_stress_p1`, `compute_zz_indicator`'s `η_e² = V_e·Δσᵀ S Δσ` via `energy_density_voigt`/`compliance_matrix` (positive definite on symmetric tensors), `barycentric_p1`, `locate_element_p1` (lowest index wins, scale-invariant barycentric slack), `apply_point_load` — all `pub` in `reify-solver-elastic` and all on the production path. | PASS | grep `compute_dual_weighted_indicator` present in `crates/reify-solver-elastic/src/error_estimator.rs` |
| `dual-is-a-second-cg-solve-on-the-borrowed-k` | substrate. `solve_cg(k: &SparseRowMat, f: &[f64], …)` borrows `K`; `apply_dirichlet_row_elimination` zeroes row + column, unit diagonal, pins `f[i] = value` — symmetric — and every `DirichletBc.value` on the cantilever path is `0.0`. A second solve on `(&k, &g)` with `g` zeroed at constrained DOFs needs no solver-crate API change; no factorization exists to reuse. | PASS | manual — an existing API property; BT1 is the behavioural cover. |
| `estimator-seam-is-infallible-and-must-become-fallible` | **CORRECTION to the draft**, G3. `AdaptiveProblem::solve_and_estimate(&mut self) -> AdaptiveEstimate` cannot fail; `run_adaptive_refinement` propagates only `refine`'s error. A QoI unresolvable on a later iteration would have no exit. §5.5 authorizes exactly two `adaptive.rs` changes: the `Result` return and the `global_indicator → relative_error` rename (+ `qoi` channel). | PASS | grep `fn solve_and_estimate\(&mut self\) -> Result<` present in `crates/reify-solver-elastic/src/adaptive.rs` |
| `self-dual-reduction-fixture-is-constructible` | G6 branch 2 / C3. The cantilever's tip load is **distributed** over the tip face, so `g ∝ f` is impossible on that pencil; C3 is pinned instead on a purpose-built fixture whose primal load **is** the QoI's own `dual_load` (`f = g`, `F = 1`) under `SolverMode::Deterministic` cold start — then `z_h` is bit-identical to `u_h` and `η_K = η_e²` bit-for-bit. Any other configuration agrees only to `10·cg_tolerance / (relative Δσ)`. | PASS | manual — the fixture lands in this leaf; a grep would pin the test's shape. |
| `dorfler-weight-homogeneity-changes-by-design` | G6, disclosed. Z-Z marks on `η_e` (a square root); DWR marks on `\|η_K\|` (an energy). `mark_dorfler`'s prefix rule is not invariant under squaring, so at `θ = 0.5` the DWR set is smaller and more concentrated. Deliberate; no test may compare marked-set sizes across modes. | PASS | manual — a documented convention, asserted by no check. |

## γ — DSL surface, eval threading, result fields, diagnostics

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `qoi-descriptor-is-an-empty-stub-with-four-pins` | G6 premise, measured. `enum QoIDescriptor {}` has zero variants; `qoi_descriptor_enum_is_an_empty_stub`, the `Option<Enum("QoIDescriptor")>` member-type pin, the `OptionNone` default pin, and the **`ElasticResult` exactly-13-param-cell** assertion all move in this diff (13 → 15). | PASS | grep `LocalDisplacement` present in `crates/reify-compiler/stdlib/solver_elastic.ri` |
| `eval-never-reads-the-param` | G6 premise, measured. Zero hits for `target_quantity_of_interest` / `QoIDescriptor` in `crates/reify-eval/` and `crates/reify-solver-elastic/`; `extract_adaptive_params` reads exactly four knobs. This leaf adds the read. | PASS | grep `target_quantity_of_interest` present in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| `dims-overload-reaches-the-adaptive-lane-from-the-cli` | **CORRECTION to the draft**, G6 branch 3. Probed on the committed `dwr_cantilever_energy.ri` with a cold `REIFY_CACHE_DIR` (release and debug binaries agree): the dims overload with `adaptive: true` runs the uniform lane — `adaptive refinement finished at 90375 DOFs on a 240×4×24 grid`, `convergence_status: NotConverged`, `global_relative_energy_error: Some(0.10577012532863311)`; the body overload does **not** (no gmsh kernel registered in the CLI; the trampoline never solves). The fixture pair uses the dims overload; the localized lane is asserted from the Rust e2e (BT6). | PASS | manual — the signal is a comparison across two files through `reify eval`; the fixture headers record the cold-cache baseline. |
| `committed-budget-does-not-guarantee-convergence` | G6, from the decompose gate. `NotConverged { reason }` carries no `final_indicator`, and the energy twin ends `NotConverged` at 0.10577 against a 0.02 target after 2 iterations. BT5's equality clause is therefore **conditional** on `Converged`; `qoi_relative_error` is asserted populated in both arms. | PASS | manual — a conditional assertion on a runtime outcome; the fixture header states it. |
| `qoi-enters-the-fea-cache-key` | G6 branch 3, from the decompose gate. `reify eval` is served by a persistent FEA cache (`compute_persist.rs`; `REIFY_CACHE_DIR`); a warm hit returns byte-identical stdout and **no** stderr. `target_accuracy` demonstrably enters the key; `target_quantity_of_interest` is unconstructible today so its membership is unverified. γ asserts, against one fresh cache dir, that the QoI twin is a miss after the energy twin and that `E_QoiRequiresAdaptive` re-fires on a second run. | PASS | manual — a runtime cache behaviour; the twin-run assertion is the cover (§11 Q7). |
| `readback-form-is-a-plain-enum-match-not-an-option-match` | **CORRECTION to the draft**, G3, from the decompose gate. `match o { some(e) => …, none => … }` does not parse (`match_pattern` has no call form; `Option` is built-in). A `NoQoi`-carrying enum matched with a full-binding arm, a bare-variant arm and the unit arm, plus `unwrap_or` over `Option<Real>`, passes `reify check` and `reify eval` today — `tests/prd-gate/fixtures/dwr_qoi_readback.ri`. `ElasticResult.qoi` is therefore `QoIEstimate` (with `NoQoi`), not `Option<QoIEstimate>`. | PASS | grep `NoQoi` present in `crates/reify-compiler/stdlib/solver_elastic.ri` |
| `variant-name-uniqueness-is-unenforced` | G3 substrate + obligation. Bare-variant construction resolves by silent first match across all in-scope `enum_defs` (pinned by `local_result_enum_coexists_with_prelude_result_and_prelude_wins_first_match`); `Cubic` and `Triangular` already duplicate across enums; the stdlib collision gate indexes enum names, not variants. `LocalDisplacement`, `LocalNormalStress`, `DisplacementEstimate`, `NormalStressEstimate` have zero hits today; this leaf lands the exactly-once test. | PASS | grep `NormalStressEstimate` present in `crates/reify-compiler/stdlib/solver_elastic.ri` |
| `six-diagnostic-variants-are-additive-substrate` | substrate. `DiagnosticCode` is a plain additive enum with rustdoc mnemonics; none of `QoiRequiresAdaptive`, `QoiUnresolvable`, `QoiDualNotConverged`, `QoiDegenerate`, `ElasticAdaptiveMaterialUnsupported`, `ElasticAdaptiveKnobInert` exists (count 0). The two existing adaptive warnings are `code: None` today. | PASS | grep `QoiRequiresAdaptive` present in `crates/reify-core/src/diagnostics.rs` |
| `three-producer-arms-write-the-new-fields` | INV-PD-2 / C7. The engine builds `ElasticResult` as an explicit `StructureInstance`; the a-posteriori triple is written by `aposteriori_adaptive_fields`, `aposteriori_nonadaptive_default_fields`, and the non-isotropic fallback arm. All three must write `qoi` / `qoi_relative_error`, and `Value::Scalar { si_value, dimension }` (the `max_von_mises` form) carries the payload dimension. | PASS | grep `qoi_relative_error` present in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| `rejection-is-a-deliverable-not-a-property-of-main` | G6 branch 4, honestly scoped. `dwr_qoi_without_adaptive.ri` asserts `E_QoiRequiresAdaptive`; today `reify check` rejects it earlier (`unknown variant 'LocalDisplacement'`), so the asserted rejection is unobservable until α and γ land. Bound as this leaf's deliverable; the vehicle is `reify eval` (check exits 0 on non-constraint errors until #5403/#5748). | PASS | manual — a rejection that does not exist yet cannot be grepped; the fixture header records today's observed behaviour. |

## δ — effectivity study and validation gate

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `no-effectivity-band-is-asserted` | G6 branch 1. Recovery-form DWR effectivity on P1 tets has no achievability basis in the repo; the gate asserts only relational properties (sign agreement, decreasing `qoi_error_bound`, `\|J_ref − J\|` not larger than seed plus the cited model-offset floor). The band is filed from measured numbers (§11 Q5), the `esc-3453-5/6` lesson. | PASS | grep `dwr-effectivity` present in `docs/notes/` |
| `reference-fixtures-exist-and-are-heavy-filtered` | substrate. `analytical_validation.rs` (Timoshenko cantilever; records the P1 lock floor and the 3-D-vs-beam model offset) and `aposteriori_validation.rs` (L-shaped domain, gmsh-gated with a printed skip) exist; `analytical_validation` is in `REIFY_HEAVY_NEXTEST_FILTER`, `aposteriori_validation` is not. The leaf's gate test must be placed with that in mind and skip loudly without gmsh. | PASS | manual — placement decision inside the leaf; the merge gate reds on a mis-registered slow binary. |
| `no-monotonicity-under-remeshing` | G6 branch 1, inverted (asserts what may NOT be claimed). Refinement is a full gmsh remesh from surface — non-nested spaces — and the repo already carries a `#[ignore = "flaky: …after a full gmsh remesh-from-surface…"]` on this fixture family. Strict decrease of `\|J_ref − J\|` is therefore not asserted. | PASS | manual — a negative obligation on the test's shape. |

## ε — PDROP C1 flip and cite retirement

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `the-c1-declaration-surface-is-owned-elsewhere-so-check-do-not-assume` | G4 seam. The C1 mechanism is #7079's, the `ElasticOptions` declaration #7080's, the detector #7085's — none landed (zero hits for `ParamNotHonored`/`PDROP` in the Rust tree). This leaf flips whatever exists and otherwise records the honored state; no fourth C1 disposition. | PASS | manual — the declaration syntax belongs to #7079 and is not fixed at this PRD's authoring time. |
| `the-cite-retirement-is-not-vacuously-satisfiable` | PTODO/PDROP contract. #7177 appears in tracked source only in two PDROP PRD rows, which this landing repoints to ε's id; #7080 writes the cite when it lands. An unconditional `expect: absent` would pass vacuously today. Real cover is the PTODO fingerprint ratchet, which reds on a cite to a terminal task. | PASS | manual — expect:absent would pass vacuously against today's main. |

## ζ — exemplar corpus, discoverability, docs-truth

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `fea-has-zero-doc-chunk-presence-so-the-fea-chunk-leaf-is-waived` | Docs-truth waiver evidence, re-measured 2026-09-11. None of the 17 chunks under `crates/reify-mcp/src/tools/chunks/` mentions `solve_elastic_static`, `ElasticOptions`, `adaptive` or `target_accuracy`. The enums chunk **is** updated (in α). The exemplar, index line and discoverability obligations are delivered; this leaf files the FEA chunk-coverage follow-up (shared obligation with #7264). | PASS | grep `goal_oriented_refinement` present in `examples/best_practices/INDEX.md` |
| `examples-corpus-is-compile-and-constraint-gated` | substrate, wired-on-main. `examples_smoke.rs` walks `examples/` and `best_practices_index_matches_corpus_directory` pins file↔row; `best_practices_constraint_gate.rs` requires every constraint Satisfied or pinned Indeterminate. `examples/*.ri` runs the full gate. | PASS | manual — an existing harness property; the INDEX check above is the landing signal. |

## η — PRD close

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `terminal-vocabulary-is-closed-and-this-leaf-is-the-only-stamper` | Overlay "PRD terminal status" — exactly `{SHIPPED, SUPERSEDED, WITHDRAWN}`, first token after the `Status` label, case-insensitive. Depends by real edges on every sibling; a `cancelled` sibling counts as satisfied. | PASS | grep `Status:\*\* \*\*SHIPPED` present in the PRD `.md` |
