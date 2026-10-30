# Capability manifest: `kinematics-typed-kinds-and-mechanism-record`

This manifest mechanizes G3 and G6 per leaf, per `.claude/skills/prd/project.md`. It covers the PRD `docs/prds/v0_6/kinematics-typed-kinds-and-mechanism-record.md` (program stream S6, pass 1, author time, after the critic pass).

All evidence was gathered against main `e217c0f30b` and re-verified at `b1a7be2982` on 2026-10-13. Every S6 path is byte-identical to the review's pinned tree `a030ee7528`. Bindings cite by symbol and file, never by `path:line`.

This file is the human-readable twin of `docs/prds/v0_6/kinematics-typed-kinds-and-mechanism-record.capability-manifest.yaml`, which carries the same bindings plus their `delivered_check` descriptors; the binding rows below are generated from it. **Keep the two in sync by hand.** A later correction to a check takes three writes: the `.yaml` descriptor, this `.md` row, and the producer task's complete `metadata.delivered_checks` list.

## Verdict at author time

All 39 `kind: grep` checks were executed with `git grep -E` against `HEAD` and confirmed **RED**:
- An `expect: present` check finds nothing today.
- An `expect: absent` check finds its target today.

Further properties:
- **Paths:** every check path exists. Checks for new modules anchor on the `mod` declaration in an existing `lib.rs` / `mod.rs`, so `reify-audit --pattern PDCHECK` sees no dead pathspec.
- **Anchoring:** absent checks anchor on the definition construct (`fn name\(`), so a test function whose name begins with the same prefix cannot keep a check red after delivery. No absent check matches comment text: overlay rule 1, esc-6739-1.
- **Manual rows:** these are excluded from the dispatch gate. Each one names the behavioural test, fixture or upstream edge that enforces it.

**Upstream-producer bindings.** These are recorded as `producer:#NNNN` and wired as real `add_dependency` edges at decompose:

| Producer | Leaves |
|---|---|
| **#8545** | β, κ |
| **#7497** | κ |
| **#6922** | κ, ρ |
| **#8560** | κ, ξ |
| **#6941** | ι, μ, τ |
| **#7456** | λ₀, λ |
| **#8745** | δ, λ |

χ is held on S8 (`x_seam_wait`; verdict OPEN until released).

**G6.** No leaf asserts a tuned numeric constant.
- **T1 and T4 (1e-9 m):** these hold because Gauss-Newton is one-step exact on linear prismatic residuals. The solver's own tolerance (`NewtonConfig::default().tol_pos_m`) is 1e-6. A leaf whose residual is nonlinear asserts within that tolerance instead.
- **T14:** a measured control value: 45.24 N = 4 kg × 11.31 m/s². It does not depend on the GAP-3 rank policy, because every joint in its loop carries the same q̈ + g, so λ = 0.
- **T15** *(amended 2026-10-30, #9566)*: derived from measured inputs, not tuned. The inputs are the open control's τ_open = [−14.31375, 2.4525] N, M = diag(0.5, 0.25) kg, and the one live row A = [1, −1] that #7312's rank-preserving `reduce_constraint_rank` keeps. They give λ = −6.40625 N and τ = [−20.72, 8.85875] N, confirmed by a scratch build of ρ's compliance wiring. The FD Jacobian of this linear prismatic residual is exact to rounding. ~~T15's closed answer equals the open one because this loop's constraint is projected out by `reduce_constraint_rank` (§11 records the cut-joint note).~~
- **Rejection premises** (T3 self-loop, T9 bind forms, T11 world-parented closure, T16 coupling refusal, T19 infeasible TOTS) are asserted as coded diagnostics. Each one's before-image fixture shows today's silent result.

## Fixture evidence (`target/debug/reify`, built from main 2026-10-12; no S6 path changed since)

| fixture (`tests/prd-gate/fixtures/`) | command | exit | observation at main |
|---|---|---|---|
| `s6_shared_carriage_tie.ri` | eval | 0 | `fv = [[0.5, 0.501]]`; armA at 0.8 m, armB at 1.001 m; `warning: kinematic singularity detected` |
| `s6_brace_through_snapshot.ri` | eval | 0 | `fv = [[0.5, 0.5005]]`; A and C at 0.5 m, B at 1.0005 m (the brace demands 0.5 m); singularity warning |
| `s6_ring_through_snapshot.ri` | eval | 0 | `s_a_bound`: solidX at x ≈ 0 (j_c = −0.3 m, outside 0..2 m, no warning); `s_unbound`: singularity warning |
| `s6_corexy_tie_orientation.ri` | eval | 0 | orientation A: carriage at (0.21213, 0.07071) m, the analytic CoreXY map for u = 200 mm, v = 100 mm; orientation B: carriage at (0.0005, 0) m, `free_values [[]]`, no warning |
| `s6_closed_planar_sweep_warmstart.ri` | eval | 0 | `sweep_1` defined; `sweep_2 = undef (OpContractViolation)` |
| `s6_open_planar_midpoint.ri` | eval | 0 | `s_planar_unbound = undef`; `s_planar_bound` at (0.5 m, 0.5 m), rotation π/2 |
| `s6_mechanism_errors.ri` | eval | 1 | `error: duplicate solid …`; `world_parented_closure` → only `note: … undef (OpContractViolation)` |
| `s6_loopclosure_declared_fields.ri` | eval | 0 | `lc_joint = undef`; `sing = undef`; `fv = [[0.7499…]]` |
| `s6_loopclosure_closing_joint_member.ri` | check | 1 | `error: structure 'LoopClosure' has no member 'closing_joint'` |
| `s6_idyn_open_after_closing.ri` / `…_open_before_closing_control.ri` | eval | 0 / 0 | ids 0, 1, 2 = 22.62, 33.93, **11.31** N / ids 0, 1, 2 = 22.62, 33.93, **45.24** N · after #7312 (re-measured at `ec9e47f729`, 2026-10-30): ids 0, 1, 2 = **16.45, 40.10, 17.48** N / unchanged |
| `s6_idyn_closed_flexure.ri` / `…_open_flexure_control.ri` | eval | 0 / 0 | sample 1 τ = **[4.905, 2.4525]** N / **[−14.31375, 2.4525]** N · unchanged after #7312 (re-measured at `ec9e47f729`, 2026-10-30) |
| `s6_idyn_coupling.ri` | eval | 0 | `forces = undef (OpContractViolation)` |
| `s6_tots_infeasible.ri` | eval / check | 0 / 0 | eval: `optimal = undef`, `d = undef`, no diagnostic; check: `OK constraint[0]`, `All constraints satisfied.` |
| `s6_tots_nonconvergence.ri` | eval | 0 | normal-looking `PiecewisePolynomialProfile`, no diagnostic |
| `s6_joint_position_enum_substrate.ri` | eval | 0 | literal payloads only: `ang = 0.7853981633974483 rad`; `got = JointPosition::RevolutePosition` (payload not printed). Non-literal payloads fail today ("non-constant payload value … not yet supported"), so #7456 is upstream |

## Bindings

| leaf | capability | binding | verdict | delivered_check |
|---|---|---|---|---|
| α | ratchet-member-registered | producer-self — tests/infra/test_kinematics_typed_dispatch_ratchet.{py,sh}; manifest row lands in the same diff (esc-4914-162) | PASS | grep present `test_kinematics_typed_dispatch_ratchet` in `tests/infra/run-all-classification.manifest` |
| α | baselines-measured | measured at e217c0f30b by the critic/fold-check: joint-kind-string-dispatch ≈20 (C10 rule), internal-eval-builtin ≈26 (multi-line calls joined), record-key-literal 16, strip-world-sentinel 2; α re-measures every row at its own HEAD | PASS | manual (baseline numbers are α's own measurement; the member enforces them) |
| β | joint-kind-in-ir | producer-self — reify-ir has no JointKind at main; reify-ir deps are reify-core + reify-ast and every consumer crate depends on reify-ir | PASS | grep present `pub enum JointKind` in `crates/reify-ir/src` |
| β | upstream-8545-flat-width | producer:#8545 (in progress; branch task/8545 adds loop_closure_value::flat_width) — same file | PASS | manual (dependency edge is the enforcement) |
| γ | relation-kind-is-the-registry-enum | **[Amended 2026-10-20, retro-critic R6]** `RelationKind` is `reify_builtins::RelationKind` (S8-ψ #9418); red at main (the sub-enum does not exist yet), green when γ binds it; replaces `relation-kind-in-ir`. The no-`from_name` rule (I-REG-1) is prose, not a check | PASS | grep present `reify_builtins::RelationKind` in `crates/reify-constraints/src` |
| γ | instance-carries-kind | grep: RelationInstance { name: String, operands, nominal_delta_dof } in reify-constraints relate_solve.rs; one construction site reify-eval relate_solve::relation_instance | PASS | grep present `kind: RelationKind` in `crates/reify-constraints/src/relate_solve.rs` |
| δ | ctor-table-derives | grep: joint_signatures::joint_ctor_result_type is a 10-name string match at main | PASS | grep present `JointKind::from_ctor_name` in `crates/reify-compiler/src/joint_signatures.rs` |
| δ | relate-mount-list-derives | grep: relate_solve::is_motion_joint_cell_type hard-codes six nominal names at main | PASS | grep present `JointKind::from_nominal_type_name` in `crates/reify-eval/src/relate_solve.rs` |
| δ | driving-cross-check | behavioural: a compiler test pins kinematic.ri DrivingJoint conformance == JointKind::is_driving for ALL | PASS | manual (behavioural test; the test's existence is reviewed, not grepped) |
| ε | typed-inverse | grep: transform_inverse/transform_log/transform3_identity exist only as builtin arms at main (seat J §6) | PASS | grep present `fn transform_inverse\(` in `crates/reify-stdlib/src/geometry.rs` |
| ζ | fn-gone | grep: 0 production callers; 10 dedicated tests + 2 oracle uses | PASS | grep absent `fn per_joint_jacobian_local\(` in `crates/reify-stdlib/src/loop_closure.rs` |
| η | enum-declared | grammar-fixture: tests/prd-gate/fixtures/s6_joint_position_enum_substrate.ri — named-field variants, bare heads, Map keyed by joint, match: reify eval exit 0, ang = 0.785 rad | PASS | grep present `enum JointPosition` in `crates/reify-compiler/stdlib/kinematic.ri` |
| η | payload-display | measured: the same fixture prints got = JointPosition::RevolutePosition (no payload) at main | PASS | manual (printed-output shape is asserted by the leaf's test, not a source grep) |
| θ | error-enum | grep: engine_eval::detect_mechanism_errors hard-codes duplicate_solid; fixture tests/prd-gate/fixtures/s6_mechanism_errors.ri shows world_parented_closure as an undef note only | PASS | grep present `enum MechanismError` in `crates/reify-stdlib/src/mechanism.rs` |
| ι | joint-enum | producer-self; depends on β (JointKind), ε (typed primitives), ζ (no per_joint_jacobian_local signal contract) | PASS | grep present `enum Joint \{` in `crates/reify-stdlib/src` |
| κ | record-module | producer-self — the three parsers and two strip_world_sentinel copies exist at main | PASS | grep present `mod loop_closure_record` in `crates/reify-stdlib/src/lib.rs` |
| κ | extract-parser-gone | grep: loop_closure::extract_loop_closure_chains at main | PASS | grep absent `fn extract_loop_closure_chains\(` in `crates/reify-stdlib/src` |
| κ | classifier-parser-gone | grep: loop_closure_solver::mechanism_loop_closure_chains at main (zero production consumers) | PASS | grep absent `fn mechanism_loop_closure_chains\(` in `crates/reify-stdlib/src` |
| κ | shared-prefix-defect | fixtures tests/prd-gate/fixtures/s6_shared_carriage_tie.ri (fv [[0.5, 0.501]], armA 0.8 m vs armB 1.001 m, false singular), s6_brace_through_snapshot.ri (fv [[0.5, 0.5005]], B at 1.0005 m, singular), s6_ring_through_snapshot.ri, s6_corexy_tie_orientation.ri (orientation A carriage (0.21213, 0.07071) m; orientation B (0.0005, 0) m silently) | PASS | manual (behavioural (T1-T5, T21); the e2e tests are the cover) |
| κ | upstreams | producer:#7497 (pending, branch task/7497), #8545 (in progress), #8560 (shim removal), #6922 (dynamics/eval.rs); θ for MechanismError | PASS | manual (dependency edges are the enforcement) |
| κ | modal-ops-on-reader | grep: reify-eval modal_ops.rs reads "loop_closures" by string at 2 production sites | PASS | grep present `is_closed_chain` in `crates/reify-eval/src/modal_ops.rs` |
| λ₀ | gui-descriptor-on-kind | grep: gui/src-tauri/src/engine.rs::extract_joint_descriptor string match with _ => FixedNoMotion at main | PASS | grep present `JointKind::` in `gui/src-tauri/src/engine.rs` |
| λ₀ | slider-walker-enum-form | grep: collect_snapshot_bind_pairs matches bind(Ident, Ident\|QuantityLiteral\|NumberLiteral) only at main | PASS | manual (behavioural (T22): GUI engine test over an enum-form bind) |
| λ₀ | upstream-7456 | producer:#7456 (pending, high) — non-literal enum payloads (variant_construct.rs rejects `RevolutePosition { angle: a }` today) | PASS | manual (dependency edge is the enforcement) |
| λ | bindings-module | grep: make_binding lives in snapshot.rs (pub(crate) for dynamics) at main; λ also extracts typed snapshot::snapshot_of and converts sweep's eval_builtin("bind")/("snapshot") calls | PASS | grep present `mod bindings` in `crates/reify-stdlib/src/lib.rs` |
| λ | dynamics-no-snapshot-path | grep: dynamics/eval.rs::snapshot_for_sample calls crate::snapshot::make_binding at main | PASS | grep absent `crate::snapshot::make_binding\(` in `crates/reify-stdlib/src/dynamics` |
| λ | corpus-migration-extent | measured: 34 bind( lines in 11 tracked .ri files (prj/printer_v01/printer.ri x21), 0 list/orientation-valued; 81 Rust eval_builtin("bind") sites; 13 embedded .ri calls in 6 Rust test files; gui engine_tests x23 + commands_tests x1; docs/reify-stdlib-reference.md §13 | PASS | manual (migration completeness is the examples/e2e suite staying green) |
| λ | upstreams | producer:#7456 (non-literal payloads; 21 of 34 .ri bind lines are non-literal), #8745 (conformance joint-bound partition) | PASS | manual (dependency edges are the enforcement) |
| μ | snapshot-copy-gone | grep: snapshot::wrap_jointvalue_for_joint (identical body to loop_closure::value_for_joint) at main | PASS | grep absent `fn wrap_jointvalue_for_joint\(` in `crates/reify-stdlib/src` |
| μ | raw-decode-gone | grep: loop_closure::jointvalue_from_bound_value (dimension-blind as_f64) at main | PASS | grep absent `fn jointvalue_from_bound_value\(` in `crates/reify-stdlib/src` |
| μ | midpoint-defect | fixture tests/prd-gate/fixtures/s6_open_planar_midpoint.ri: s_planar_unbound = undef; s_planar_bound at (0.5 m, 0.5 m, rot pi/2) | PASS | manual (behavioural (T6)) |
| ν | input-error-enum | grep: LoopClosureSolverInputs is a positional 5-tuple alias at main; 3 ad hoc length checks | PASS | grep present `enum LoopClosureInputError` in `crates/reify-stdlib/src` |
| ν | error-map-channel | behavioural (T12): Rust boundary test, malformed record -> named LoopClosureInputError -> snapshot error Map -> coded diagnostic via θ's engine scan, which ν makes recurse into List/Map values (detect_error_map_diagnostics reads top-level cells only at main) | PASS | manual (behavioural boundary test below the compiler) |
| ξ | wrapper-gone | grep: solve_loop_closure_with_diagnostics, single production caller snapshot.rs, behaviourally identical to the plain solve | PASS | grep absent `fn solve_loop_closure_with_diagnostics\(` in `crates/reify-stdlib/src` |
| ξ | one-producer | producer-self — no diagnostics_of at main; engine_eval duplicates the singularity message | PASS | grep present `fn diagnostics_of` in `crates/reify-stdlib/src` |
| ξ | is-singular-undef | fixture tests/prd-gate/fixtures/s6_loopclosure_declared_fields.ri: sing = undef for a non-singular snapshot; ξ declares and populates converged : Bool; T21 ξ half: the #7207 shape reports converged = false | PASS | manual (behavioural (T13)) |
| ο | sweep-kind-match-gone | grep: sweep::driving_joint_kind + wrap_value_for_driving_joint with dead planar arms at main | PASS | grep absent `fn driving_joint_kind\(` in `crates/reify-stdlib/src/sweep.rs` |
| π | declaration | grep: kinematic.ri declares free_values : List<JointValue> (= Real) at main; runtime is per-loop flat rows | PASS | grep present `free_values[[:space:]]*:[[:space:]]*Map<Joint,[[:space:]]*JointPosition>` in `crates/reify-compiler/stdlib/kinematic.ri` |
| π | loopclosure-member | fixture tests/prd-gate/fixtures/s6_loopclosure_closing_joint_member.ri: reify check exit 1 'structure LoopClosure has no member closing_joint' | PASS | manual (behavioural (C7); a check fixture flips from exit 1 to exit 0) |
| π | warm-start-defect | fixture tests/prd-gate/fixtures/s6_closed_planar_sweep_warmstart.ri: sweep_2 = undef (width fix is #8545, upstream of μ/ξ) | PASS | manual (behavioural (T8)) |
| ρ | tree-module | producer-self; fixtures tests/prd-gate/fixtures/s6_idyn_open_after_closing.ri (ids 0,1,2 with 11.31 N; 16.45, 40.10, 17.48 N after #7312) and s6_idyn_closed_flexure.ri (sample 1 tau_a 4.905 N vs open control -14.31 N; T15 target after #7312: tau = [-20.72, 8.85875] N) | PASS | grep present `struct MechanismTree` in `crates/reify-stdlib/src/dynamics` |
| ρ | upstream-6922 | producer:#6922 (pending; branch task/6922 active) rewrites dynamics/eval.rs readers | PASS | manual (dependency edge is the enforcement) |
| σ₀ | spatial-at-crate-root | grep: joints.rs imports crate::dynamics::spatial::SpatialVector6 (joints->dynamics edge of the reify-stdlib SCC {dynamics, joints, mechanism, snapshot}); dynamics/spatial.rs has no crate imports | PASS | grep present `mod spatial` in `crates/reify-stdlib/src/lib.rs` |
| σ₀ | com-moves-to-dynamics | grep: snapshot.rs imports crate::dynamics::eval::mass_properties_from_value for the center_of_mass group at main (SCC edge snapshot->dynamics) | PASS | grep present `mod com\b` in `crates/reify-stdlib/src/dynamics/mod.rs` |
| σ₀ | scc-row-lowered | scripts/module-scc-baseline.json reify-stdlib row = [dynamics, joints, mechanism, snapshot] at main; scripts/assert-module-scc.py | PASS | manual (the INV-ARCH-1 tool enforces the baseline) |
| σ | force-string-match-gone | grep: dynamics/eval.rs::joint_force_value 6-string match + _ => None at main; fixture tests/prd-gate/fixtures/s6_idyn_coupling.ri forces = undef | PASS | grep absent `fn joint_force_value\(` in `crates/reify-stdlib/src/dynamics` |
| τ | marshal-module | h14: trampoline.rs marshalling group (read_scalar_si..track_data_to_value) | PASS | grep present `mod marshal` in `crates/reify-stdlib/src/trajectory/mod.rs` |
| υ | scanner-gone | grep: tots.rs mod shaper_family_guard (~278-line text scanner) at main | PASS | grep absent `mod shaper_family_guard` in `crates/reify-stdlib/src/trajectory` |
| υ | stand-in-solve-gone | grep: input_shape.rs::run_tots (canonical stand-in model) at main; fixture tests/prd-gate/fixtures/s6_tots_infeasible.ri: eval optimal = undef, check 'All constraints satisfied' (exit 0) | PASS | grep absent `fn run_tots\(` in `crates/reify-stdlib/src/trajectory` |
| φ | codes-exist | grep: no TrajectoryConstraintInfeasible in reify-core DiagnosticCode at main; infeasible becomes ComputeOutcome::Failed with the coded Error | PASS | grep present `TrajectoryConstraintInfeasible` in `crates/reify-core/src/diagnostics.rs` |
| φ | code-str-gone | grep: TotsOutcome::code_str, 0 production callers | PASS | grep absent `fn code_str\(` in `crates/reify-stdlib/src/trajectory/tots.rs` |
| φ₂ | gcode-parse-error-code | grep: no GcodeParseError in reify-core DiagnosticCode at main; the parse error is dropped by eval_gcode_import (silent Undef) | PASS | grep present `GcodeParseError` in `crates/reify-core/src/diagnostics.rs` |
| φ₂ | gcode-dead-code-allow-gone | grep: module-wide `#![allow(dead_code)]` in gcode_import.rs (warnings never read outside tests) | PASS | grep absent `#!\[allow\(dead_code\)\]` in `crates/reify-stdlib/src/trajectory/gcode_import.rs` |
| χ | s8-typed-diagnose-column | producer: S8 typed diagnose-hook column (wave 2, not yet authored); held via x_seam_wait | OPEN | manual (held leaf; released by S8's session) |
| ψ₁ | joint-kinds-const-gone | grep: joints.rs pub(crate) const JOINT_KINDS at main | PASS | grep absent `const JOINT_KINDS` in `crates/reify-stdlib/src` |
| ψ₁ | driving-kinds-const-gone | grep: joints.rs pub(crate) const DRIVING_JOINT_KINDS at main | PASS | grep absent `const DRIVING_JOINT_KINDS` in `crates/reify-stdlib/src` |
| ψ₁ | multi-dof-kinds-gone | grep: loop_closure.rs test const MULTI_DOF_KINDS at main | PASS | grep absent `const MULTI_DOF_KINDS` in `crates/reify-stdlib/src` |
| ψ₁ | ratchet-rows-zero | α's member: joint-kind-string-dispatch, joint-kinds-const, internal-eval-builtin rows at 0 | PASS | manual (the ratchet member enforces the counts) |
| ψ₂ | newton-module | h14: loop_closure_solver.rs numerics group (NewtonConfig..DIVERGENCE_LIMIT) | PASS | grep present `mod newton` in `crates/reify-stdlib/src` |
| ψ₃ | track-access-module | h14: trampoline.rs location/deviation accessor group | PASS | grep present `mod track_access` in `crates/reify-stdlib/src/trajectory` |
| ω₁ | docs-truth-four-items | overlay docs-truth gate: chunk, exemplar + INDEX line, reify-design index line, discoverability | PASS | manual (docs-truth acceptance is a smoke compile plus findability review) |
| ω₂ | terminal-stamp | overlay decompose-close obligation | PASS | manual (committed header; prose is not grepped) |

## Gap-fill amendments (2026-10-16)

From `plans/gap-fill-deliberation-reify-20261016.md` §K6-D5: new leaf φ₂ #9281 with the two `φ₂` rows above, confirmed red at main `c7bbfe2519` with `git grep -E` (the author-time count of 39 grep checks is now 41). The §K6-D6 honesty task #9283 is standalone; its check lives on the task, not here.

## Retro-critic sitting amendments (2026-10-20)

From `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R6`: γ #8830 binds `reify_builtins::RelationKind` (S8-ψ #9418, new edge #8830 ← #9418). Its check `relation-kind-in-ir` (`pub enum RelationKind` present in `crates/reify-ir/src`) is replaced by `relation-kind-is-the-registry-enum` (`reify_builtins::RelationKind` present in `crates/reify-constraints/src`; red at main, green when γ binds the S8-ψ sub-enum); `instance-carries-kind` is unchanged. γ's files: `crates/reify-constraints/src/{relate_solve,solvespace,classifier}.rs`, `crates/reify-eval/src/relate_solve.rs`, `crates/reify-constraints/Cargo.toml`, `crates/reify-eval/Cargo.toml`; reify-ir `relation_kind.rs` and `lib.rs` dropped (three writes: this row, the `.yaml`, #8830's `metadata.delivered_checks` / `metadata.files`).

## T15 re-derivation amendment (2026-10-30)

This follows the PRD's "T15 re-derivation after #7312" (task #9566). #7312 (`10eb8ca95b`) made `reduce_constraint_rank` rank-preserving. T15's target therefore became the KKT value τ = [−20.72, 8.85875] N (G6 bullet above). T14's before-image moved, but its target did not. Both fixture rows were re-measured at `ec9e47f729`, and ρ's `tree-module` binding is amended in both twins. No delivered check changed: it is still `struct MechanismTree` present. So #8845's `metadata.delivered_checks` is untouched, and its `user_observable_signal` carries the new values.
