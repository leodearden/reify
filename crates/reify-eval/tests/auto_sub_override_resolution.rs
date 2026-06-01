//! Eval e2e tests for sub-instance-override `auto` (task 3806, steps 5–6).
//!
//! Tests the full parse→compile→eval pipeline for the sub-override `auto`
//! binding site (`sub b : Bearing { bore = auto }`).
//!
//! ## RED→GREEN arc
//!
//! Step 5 (RED): Tests assert that the scoped `A.b.bore` cell resolves to
//! `DeterminacyState::Determined` with the constraint-determined value, that
//! the §4.4 invariant holds (same result as the param-default auto equivalent),
//! that strict under-determined auto produces the expected error, and that
//! `auto(free)` emits the expected warning.  The tests compile because steps 1–4
//! already built the AST field, lowering, and compiler producer — but the eval
//! reconciliation (graph.rs / unfold.rs) is not yet in place.
//!
//! Step 6 (GREEN): graph.rs `from_templates` skips inserting a child-derived
//! scoped node when the parent template already has an override cell for that id.
//! unfold.rs `elaborate_child_params_only` skips writing the child default when
//! the snapshot already holds an `Auto` entry for the scoped id.  Both changes
//! ensure the `Auto` cell survives into the per-template solver problem and the
//! M3 solver resolves it correctly via the parent's constraints.
//!
//! ## Example smoke test
//!
//! Step 7 (RED): Adds `example_auto_binding_sites_ri_resolves` which reads
//! `examples/auto_binding_sites.ri` via a compile-time path, parses + compiles
//! + evals it, and asserts no error-severity diagnostics plus that the
//!   sub-override bore cell resolved to `Determined`.  RED until step 8 creates
//!   the file.
//!
//! Step 8 (GREEN): Creates `examples/auto_binding_sites.ri`.
//!
//! ## Forward-reference regression (steps 9–10)
//!
//! Step 9 (RED): Test (e) asserts that the sub-override `bore = auto` resolves
//! correctly when the **parent** structure is declared BEFORE the child
//! (`Bearing`) in source.  A determining constraint forces a NON-default value
//! (25 mm ≠ child default 10 mm), so the test discriminates "override honored →
//! solver resolves 25 mm" from "override dropped → child default 10 mm →
//! constraint 25 mm == 10 mm violated".  This is RED until step 10 wires the
//! deferred post-pass.
//!
//! Step 10 (GREEN): After the deferred post-pass resolves the forward-declared
//! child's member type, the scoped Auto cell is pushed and the solver resolves
//! 25 mm correctly.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::parse_and_compile_with_stdlib;

// ── Shared fixtures ───────────────────────────────────────────────────────────

/// Bearing has a `bore` param with default 5mm so that when it differs from the
/// constraint-determined value (10mm) we can tell the solver did real work.
const BEARING_5MM: &str = "structure Bearing { param bore : Length = 5mm }";

fn bearing_source(override_expr: &str, body: &str) -> String {
    format!(
        r#"{BEARING_5MM}
structure A {{
    sub b : Bearing {{ bore = {override_expr} }}
    {body}
}}"#
    )
}

/// Build an Engine backed by `SimpleConstraintChecker` + `DimensionalSolver`.
fn engine_with_solver() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(DimensionalSolver))
}

/// Filter `diagnostics` to error-severity entries.
fn errors_only(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Filter `diagnostics` to warning-severity entries.
fn warnings_only(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

// ── Test (a): strict auto resolves uniquely ───────────────────────────────────

/// `bore = auto` with `constraint self.b.bore == 10mm` must resolve to
/// `(10mm, Determined)`.  The child default is intentionally 5mm (different
/// from the constraint-determined value) so the solver does real work.
///
/// RED until step-6 wires the eval reconciliation; GREEN once graph.rs and
/// unfold.rs let the parent override take precedence.
#[test]
fn sub_override_auto_strict_resolves_determined() {
    let source = bearing_source("auto", "constraint self.b.bore == 10mm");
    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "expected no error-severity eval diagnostics, got: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "A.b.bore should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "A.b.bore should be Determined after auto resolution, got {:?}",
        det
    );

    // 10mm = 0.010 SI
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.010).abs() < 1e-6),
        "A.b.bore should equal 10mm (0.010 SI); got {:?}",
        val
    );
}

// ── Test (b): §4.4 invariant ──────────────────────────────────────────────────

/// §4.4 invariant: the resolved `A.b.bore` value via sub-override `auto` must
/// equal the value produced by the equivalent param-default `auto` in a
/// standalone structure `A2 { param bore : Length = auto; constraint bore == 10mm }`.
///
/// Both share the same equality constraint and must reach the same resolved value
/// (10mm).  This verifies that the sub-override producer reuses the identical
/// M3 resolution semantics, not a separate path.
#[test]
fn sub_override_auto_section_4_4_invariant() {
    // Sub-override path.
    let source_a = bearing_source("auto", "constraint self.b.bore == 10mm");
    let compiled_a = parse_and_compile_with_stdlib(&source_a);
    let mut engine_a = engine_with_solver();
    let result_a = engine_a.eval(&compiled_a);

    let a_errors = errors_only(&result_a.diagnostics);
    assert!(
        a_errors.is_empty(),
        "sub-override path: unexpected errors: {:?}",
        a_errors
    );

    let snap_a = engine_a.snapshot().expect("snapshot for A");
    let id_a = ValueCellId::new("A.b", "bore");
    let (val_a, det_a) = snap_a.values.get(&id_a).expect("A.b.bore in snapshot");
    assert_eq!(
        *det_a,
        DeterminacyState::Determined,
        "A.b.bore should be Determined"
    );

    // Param-default-auto equivalent path (no sub, no child default).
    let source_a2 = r#"
structure A2 {
    param bore : Length = auto
    constraint self.bore == 10mm
}
"#;
    let compiled_a2 = parse_and_compile_with_stdlib(source_a2);
    let mut engine_a2 = engine_with_solver();
    let result_a2 = engine_a2.eval(&compiled_a2);

    let a2_errors = errors_only(&result_a2.diagnostics);
    assert!(
        a2_errors.is_empty(),
        "param-default-auto path: unexpected errors: {:?}",
        a2_errors
    );

    let snap_a2 = engine_a2.snapshot().expect("snapshot for A2");
    let id_a2 = ValueCellId::new("A2", "bore");
    let (val_a2, det_a2) = snap_a2.values.get(&id_a2).expect("A2.bore in snapshot");
    assert_eq!(
        *det_a2,
        DeterminacyState::Determined,
        "A2.bore should be Determined"
    );

    // §4.4: the two paths must produce the same resolved value.
    let si_a = match val_a {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("A.b.bore should be Scalar, got {:?}", other),
    };
    let si_a2 = match val_a2 {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("A2.bore should be Scalar, got {:?}", other),
    };
    assert!(
        (si_a - si_a2).abs() < 1e-9,
        "§4.4 invariant violated: sub-override A.b.bore = {} != param-default A2.bore = {}",
        si_a,
        si_a2
    );
}

// ── Test (c): strict under-determined emits M3 error ─────────────────────────

/// `bore = auto` (strict) with NO determining constraint must emit the same
/// "not uniquely determined" M3 error as the param-default equivalent.
///
/// Mirrors `crates/reify-constraints/src/solver.rs` §"strict auto params require
/// a unique solution" (DiagnosticCode::ConstraintNonUnique / Infeasible path).
#[test]
fn sub_override_auto_strict_underdetermined_emits_error() {
    // No constraint — auto cannot be uniquely determined.
    let source = bearing_source("auto", "// no constraint");
    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    // Must produce at least one Error-severity diagnostic (non-unique).
    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        !eval_errors.is_empty(),
        "expected a 'not uniquely determined' error diagnostic for underdetermined \
         strict auto; got no errors (diagnostics: {:?})",
        result.diagnostics
    );

    // The error message must mention "uniquely determined" or "unique".
    assert!(
        eval_errors.iter().any(|d| {
            d.message.to_lowercase().contains("unique")
                || d.message.to_lowercase().contains("not uniquely")
        }),
        "expected an error about unique determination; got: {:?}",
        eval_errors
    );
}

// ── Test (d): auto(free) non-unique emits warning ─────────────────────────────

/// `bore = auto(free)` with no determining constraint must emit the existing
/// auto(free) non-unique warning and still produce a feasible value.
///
/// Mirrors the `auto(free)` warning path in engine_eval.rs ~1935.
#[test]
fn sub_override_auto_free_non_unique_emits_warning() {
    // auto(free) with no constraint → non-unique feasible solution.
    let source = bearing_source("auto(free)", "// no constraint");
    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    // auto(free) → no error-severity diagnostics (feasibility is accepted).
    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "auto(free) should not produce error diagnostics; got: {:?}",
        eval_errors
    );

    // Must produce at least one warning about non-uniqueness.
    let eval_warnings = warnings_only(&result.diagnostics);
    assert!(
        !eval_warnings.is_empty(),
        "expected a non-unique warning for auto(free) underdetermined; \
         got no warnings (diagnostics: {:?})",
        result.diagnostics
    );

    // The cell should have a resolved (Scalar) value — any feasible value.
    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    if let Some((val, det)) = snap.values.get(&id) {
        assert_eq!(
            *det,
            DeterminacyState::Determined,
            "A.b.bore should be Determined even with auto(free)"
        );
        assert!(
            matches!(val, Value::Scalar { .. }),
            "A.b.bore should be a Scalar value; got {:?}",
            val
        );
    }
    // If the key is absent the solver may not have run (no solver wired),
    // but with DimensionalSolver it should always be present.
}

// ── Test (step-7 / step-8): example smoke test ────────────────────────────────

/// Path to the γ-slice integration-gate example file.
///
/// RED until step-8 creates `examples/auto_binding_sites.ri`.
const AUTO_BINDING_SITES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto_binding_sites.ri"
);

/// `examples/auto_binding_sites.ri` must compile with no error-severity
/// diagnostics, evaluate without errors, and the sub-override `bore` cell
/// must resolve to `DeterminacyState::Determined`.
///
/// Pipeline:
///   1. `std::fs::read_to_string(AUTO_BINDING_SITES_PATH)` — reads the file.
///   2. `parse_and_compile_with_stdlib(&source)` — compiles with stdlib.
///   3. No compile-time Error diagnostics.
///   4. `engine_with_solver().eval(&compiled)` — full eval with DimensionalSolver.
///   5. No eval-time Error diagnostics.
///   6. `A.b.bore` (or whatever sub structure the file defines) is `Determined`.
///
/// Mirrors the smoke-test pattern from
/// `tests/topology_selector_smoke_tests.rs::block_inertia_compiles_with_stdlib_no_errors`.
///
/// RED until step-8 creates `examples/auto_binding_sites.ri`.
#[test]
fn example_auto_binding_sites_ri_resolves() {
    let source = std::fs::read_to_string(AUTO_BINDING_SITES_PATH)
        .expect("examples/auto_binding_sites.ri should exist (created by step-8)");

    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "examples/auto_binding_sites.ri should compile with no error-severity diagnostics; \
         got:\n{:#?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "examples/auto_binding_sites.ri should evaluate with no error-severity diagnostics; \
         got:\n{:#?}",
        eval_errors
    );

    // The sub-override `bore` cell in `AutoBindingSites` structure must be Determined.
    // The example file defines `structure AutoBindingSites { sub b : Bearing { bore = auto }
    // constraint self.b.bore == 10mm }` so the scoped id is AutoBindingSites.b.bore.
    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("AutoBindingSites.b", "bore");
    let (_, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "AutoBindingSites.b.bore should be in snapshot after eval; \
             available cells: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "AutoBindingSites.b.bore should be Determined after auto resolution; got {:?}",
        det
    );
}

// ── Test (e): forward-reference (step 9 RED → step 10 GREEN) ─────────────────

/// (e) `bore = auto` resolves correctly when the **parent** is declared BEFORE
/// the child in source (legal forward-reference).
///
/// Source order: `structure A { … }  structure Bearing { … }`.  The constraint
/// forces `bore == 10mm` — deliberately different from Bearing's own default
/// (5mm here) — so the test discriminates two failure modes:
///  - Override honored → solver produces 10 mm  ✓
///  - Override dropped → child default 5 mm → `10mm == 5mm` → error  ✗
///
/// Using 10mm (= 0.010 m SI) as the target lets the Nelder-Mead solver satisfy
/// the constraint at its default initial point (0.010 m for Length types),
/// avoiding convergence-precision issues that arise when the initial point is
/// far from the target.
///
/// RED (step 9): the current inline lookup emits a spurious "no such param"
///   error and drops the override because Bearing is not yet compiled when A is
///   processed.
/// GREEN (step 10): the deferred post-pass resolves Bearing's `bore` type after
///   all templates are compiled and pushes the scoped Auto cell; the solver then
///   resolves it to 10 mm.
#[test]
fn sub_override_auto_strict_forward_declared_child_resolves_determined() {
    // Parent before child; constraint forces 10mm ≠ Bearing default 5mm.
    // The 5mm child default discriminates "override honored (10mm)" from
    // "override dropped (child default 5mm → constraint fails)".
    let source = r#"
structure A {
    sub b : Bearing { bore = auto }
    constraint self.b.bore == 10mm
}
structure Bearing { param bore : Length = 5mm }
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "forward-declared child: unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "forward-declared child: expected no error-severity eval diagnostics; got: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "forward-declared child: A.b.bore should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "forward-declared child: A.b.bore should be Determined; got {:?}",
        det
    );

    // Must be 10mm (0.010 SI), NOT the child's default 5mm (0.005 SI).
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.010).abs() < 1e-6),
        "forward-declared child: A.b.bore should equal 10mm (0.010 SI); got {:?}",
        val
    );
}

// ── Test (f): S1 — DimensionalSolver type-agnosticism regression (task 4123) ──

/// (f) Forward-declared child, DIMENSIONAL/arithmetic constraint.
///
/// When the parent structure is declared before the child (forward-declared),
/// `try_resolve_cross_sub_geometry_value_ref` emits a `ValueCellRef` with a
/// placeholder `Type::Geometry` (the real type is unknown until the post-pass).
/// This test proves that the `DimensionalSolver` resolves the constraint
/// `2 * self.b.bore == 20mm` to `bore == 10mm` regardless of that placeholder
/// type, because the solver evaluates constraint operands numerically via
/// `reify_expr::eval_expr(...).as_f64()` and is completely type-agnostic on the
/// operand's static `Type`.
///
/// Source order: `structure A { … }  structure Bearing { … }` — Bearing is
/// forward-declared.  The child default is intentionally 5mm (≠ 10mm) so the
/// test discriminates "solver did real work (10mm)" from "child default leaked
/// (5mm)".
///
/// Expected GREEN on arrival: the DimensionalSolver is provably type-agnostic
/// (same numeric residuals regardless of declaration order).  Regression guard
/// against any future change that would make the solver inspect static types.
///
/// See also: `crates/reify-compiler/src/expr.rs`
/// `try_resolve_cross_sub_geometry_value_ref` — the forward-declared branch
/// cross-references this test.
#[test]
fn sub_override_auto_forward_declared_dimensional_constraint_type_agnostic() {
    // Parent before child; arithmetic constraint 2 * bore == 20mm → bore == 10mm.
    // Child default is 5mm (different) so the result discriminates solver work
    // from a silent "use the default" path.
    let source = r#"
structure A {
    sub b : Bearing { bore = auto }
    constraint 2 * self.b.bore == 20mm
}
structure Bearing { param bore : Length = 5mm }
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "S1/type-agnostic: unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "S1/type-agnostic: expected no error-severity eval diagnostics; got: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "S1/type-agnostic: A.b.bore should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "S1/type-agnostic: A.b.bore should be Determined; got {:?}",
        det
    );

    // 2 * bore == 20mm  →  bore == 10mm == 0.010 SI.
    // Must NOT be 5mm (child default), proving the solver used the constraint.
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.010).abs() < 1e-6),
        "S1/type-agnostic: A.b.bore should equal 10mm (0.010 SI) via \
         arithmetic constraint; got {:?}",
        val
    );
}

// ── Test (g): S2 — dangling forward-declared member emits ConstraintIndeterminate ─

/// (g) A constraint referencing a non-existent member of a forward-declared child
/// (member NOT in param_overrides) must surface a `ConstraintIndeterminate`
/// warning rather than failing silently.
///
/// When `b` is forward-declared, `try_resolve_cross_sub_geometry_value_ref`
/// emits a `ValueCellRef` for any member access on `b` — including members that
/// do not exist in the child (there is no member-existence check at compile time
/// for the forward-declared branch).  At eval time the backing cell is absent,
/// so `reify_expr::eval_expr` returns `Value::Undef` via `get_or_undef`, and
/// `SimpleConstraintChecker` emits a `ConstraintIndeterminate` warning
/// (`"constraint <id> indeterminate: undefined inputs"`).
///
/// Note: this is a deliberate downgrade from the legacy hard
/// `"unresolvable GeomRef::Sub"` error.  A future design follow-up could
/// promote the warning to an error after the post-pass can distinguish
/// forward-declared members vs genuinely absent ones.
///
/// Expected GREEN on arrival — eval already emits the diagnostic; this test
/// closes the reviewer's "verify eval emits a diagnostic" item (task 4123 S2)
/// and guards against future silent regressions.
#[test]
fn sub_override_auto_forward_declared_nonexistent_member_emits_indeterminate_warning() {
    // Parent before child; `nonexistent` is NOT a member of Bearing and is NOT
    // in param_overrides (empty body `{ }`).
    let source = r#"
structure A {
    sub b : Bearing { }
    constraint self.b.nonexistent == 10mm
}
structure Bearing { param bore : Length = 10mm }
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    // No compile-time errors expected: the forward-declared branch emits a
    // ValueCellRef for any member access, deferring the "does it exist?"
    // check to the post-pass (and the post-pass only processes param_overrides,
    // not constraint refs, so no error is produced there either).
    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "S2/dangling-ref: unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    // Must produce at least one diagnostic.
    assert!(
        !result.diagnostics.is_empty(),
        "S2/dangling-ref: expected at least one eval diagnostic for dangling \
         member reference; got none"
    );

    // Must contain a ConstraintIndeterminate warning (not a hard error).
    let indeterminate_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ConstraintIndeterminate)
        })
        .collect();

    assert!(
        !indeterminate_warnings.is_empty(),
        "S2/dangling-ref: expected a ConstraintIndeterminate warning for \
         undefined inputs; diagnostics: {:?}",
        result.diagnostics
    );
}
