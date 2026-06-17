# Capability manifest — type-hygiene.md (2026-06-10)

Per-leaf capability→evidence bindings (G3+G6 mechanized). All bindings verified against working tree 2026-06-10 (survey agents + probes `/tmp/moi-probe{1..8}.ri` against fresh `target/debug/reify`). **No FAIL bindings.**

| Leaf | Capability asserted by signal | Evidence | Verdict |
|---|---|---|---|
| β | binop compile site has both operand `result_type`s for literal rewrite | grep: `expr.rs:952-976` (compiled operands), guards `:1088-1191` wired production path | PASS wired |
| β | boilerplate corpus exists to delete | grep: `0.0 * 1kg` family in stdlib/examples (4226-era convention) | PASS wired |
| β | dep 4373 exists/pending (canonical dimensionless type) | task 4373 pending (priority-override) | PASS producer-upstream |
| α | guard-arm precedent + diagnostic machinery at same site | grep: Implies arm `expr.rs:1160-1191` (task 3921), `format_dimension_mismatch_diagnostic` Add/Sub `:1110-1149` | PASS wired |
| α | fixit reductions exist, dimension-preserving, DSL-exposed | grep: `math_signatures.rs:265` (trace), `:273` (eigenvalues); sorted ascending `reify-stdlib/src/matrix.rs:223`; probe 6 (PD constraint OK+VIOLATED both observed) | PASS wired |
| α | Kleene runtime to preserve | grep: `reify-expr/src/lib.rs:2507-2545` `eval_and`/`eval_or` + pinning tests `:6193+` | PASS wired |
| γ | `MASS_DENSITY` constant | grep: `reify-core/src/dimension.rs:198` | PASS wired |
| γ | `material.density` → `Value::Scalar{MASS_DENSITY}`, si kg/m³ | probe 5 (warning + Undef today proves the dimensioned value arrives); `materials_mechanical.ri:73-77` (task 3111) | PASS wired |
| γ | kernel InertiaTensor seam produces real tensors | `dynamics_ops.rs:228-258` (task 4237 merged); probe 5 analytic box values match m(a²+b²)/12 | PASS wired |
| γ | compound-unit density literal parses (`7850kg/m^3`) | grep: `examples/structural_traits_dimensioned.ri:19` (in-repo, compiling) | PASS grammar-fixture |
| γ | migration surface enumerated | PRD §8.2 (9 files + 3 doc sites, agent-verified) | PASS wired |
| δ | B-side holes reproducible | grep: `cell_f64` `dynamics_ops.rs:46-53` (any-Scalar verbatim), `resolve_arg_value` `:197-206` (None→ladder) | PASS wired |
| δ | loud-guard precedent | grep: `E_DynamicsBodyMassPropsArity` `dynamics_ops.rs:312-321` | PASS wired |
| ε | evaluate-args-against-ValueMap pattern works in production | grep: `eval_named_arg` call site `geometry_ops.rs:535` (compile_geometry_op, production path) | PASS wired |
| ε | owned resolver inventory complete | PRD §8.1 (12 fns, agent sweep of `CompiledExprKind::` matches); geometry-handle pair excluded → owner 4358 | PASS wired |
| ζ | result-type signature table consumed in production | grep: `math_signatures.rs` wired into `resolve_function_overload` (expr.rs ladder); `args: &[CompiledExpr]` already threaded | PASS wired |
| η | phase-5 site + converter available | grep: `conformance/checker.rs:1464-1492` (required-only check at `:1482`), `implicitly_converts_to` in scope; probe 7 reproduces the gap | PASS wired |
| θ | CLI summary/exit sites | grep: `reify-cli/src/main.rs:539,656,831` | PASS wired |
| ι | checker has expr+values for definedness inspection | grep: `reify-constraints/src/lib.rs:75-86` (SimpleConstraintChecker::check) | PASS wired |
| κ | member + populate + extraction sites; dimensioned-matrix type form | grep: `dynamics.ri:71-83`, `dynamics_ops.rs:284-392`, `dynamics/eval.rs:314-330`; Tensor↔Matrix cross-variant accepted (`reify-eval/src/lib.rs:222` family tests `:1253+`); `Tensor<2,3,MomentOfInertia>` param verified end-to-end (probe 3) | PASS wired |
| κ | "identical numeric output" claim | type-layer-only change; si_values untouched → exact, not a tuned bound | PASS (no floor) |
| λ | all capabilities | producer-upstream within batch (α–κ) | PASS producer-upstream |

Numeric-floor branch: N/A — no leaf asserts a tuned numeric bound (κ's identity claim is structural).
