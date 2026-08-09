//! Runtime semantics of the corner-radius constraint the compiler synthesizes
//! for param-driven `rounded_rect`/`rounded_box` args (task #5665).
//!
//! The compiler-layer tests (`reify-compiler/tests/rounded_primitives_tests.rs`)
//! pin that a constraint is emitted and how it is labelled. These pin what it
//! actually *decides* at runtime, so the predicate's semantics are asserted
//! rather than its AST shape.
//!
//! No kernel is needed: the constraint reads only params, so `Engine::check`
//! resolves it definitely — before any geometry is executed, which is exactly
//! the point of synthesizing it (a named violation ahead of the opaque OCCT
//! failure the oversized radius would otherwise produce).
//!
//! Every bound here is an exact metric literal (40mm → 0.04, 30mm → 0.03,
//! 25mm → 0.025, 5mm → 0.005 SI) compared with the same strict `<` / `>` that
//! `eval_cmp` applies at runtime, so no tolerance is involved.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::{Satisfaction, Value};
use reify_test_support::{SpyConstraintSolver, check_source, parse_and_compile};

/// `rounded_rect(width, depth, corner_r)` with `corner_r` a param defaulting to
/// `radius`.
///
/// Width and depth are parameters of the fixture, not constants, so a case can
/// isolate ONE of the predicate's two dimension bounds — see
/// [`corner_r_violating_only_the_depth_bound_violates`].
fn rounded_rect_source_wd(width: &str, depth: &str, radius: &str) -> String {
    format!(
        r#"structure def S {{
    param corner_r: Length = {radius}
    let body = rounded_rect({width}, {depth}, corner_r)
}}"#
    )
}

/// `rounded_rect(40mm, 30mm, corner_r)` with `corner_r` a param defaulting to
/// `radius` — the canonical fixture, whose binding bound is the 30mm depth.
fn rounded_rect_source(radius: &str) -> String {
    rounded_rect_source_wd("40mm", "30mm", radius)
}

/// The single synthesized constraint's satisfaction.
///
/// Located by label rather than by index so the assertion does not silently
/// re-target if the entity ever gains another constraint.
fn corner_constraint(source: &str) -> Satisfaction {
    let result = check_source(source);
    let matching: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|e| e.label.as_deref().is_some_and(|l| l.contains("corner")))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one corner-radius constraint, got: {:?}",
        result.constraint_results
    );
    matching[0].satisfaction
}

/// 2*25mm = 50mm ≥ min(40mm, 30mm) — the radius does not fit, so the
/// synthesized constraint must catch it.
///
/// This is the case the whole task exists for: with a literal `25mm` the
/// compiler errors statically, but behind a param it used to sail through to
/// OCCT.
#[test]
fn oversized_param_corner_r_violates() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("25mm")),
        Satisfaction::Violated,
        "2*0.025 = 0.05 is not < min(0.04, 0.03) — must be Violated"
    );
}

/// A zero radius is degenerate — the positivity conjunct must reject it.
#[test]
fn zero_param_corner_r_violates() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("0mm")),
        Satisfaction::Violated,
        "corner_r = 0 is not > 0 — must be Violated"
    );
}

/// 2*17mm = 34mm — comfortably under the 40mm WIDTH bound, but over the 30mm
/// DEPTH bound. Isolates the SECOND dimension conjunct.
///
/// Every other violating case here trips at least two conjuncts at once
/// (25mm fails both bounds, 0mm fails only positivity), so a predicate that
/// built `Lt(2r, width)` TWICE — the plausible copy-paste in a hand-constructed
/// `CompiledExpr` tree — would leave every other test in this file and in
/// `rounded_primitives_tests.rs` green while an oversized-vs-depth radius sailed
/// straight through to OCCT: exactly the silent skip #5665 exists to remove.
#[test]
fn corner_r_violating_only_the_depth_bound_violates() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("17mm")),
        Satisfaction::Violated,
        "2*0.017 = 0.034 IS < 0.04 (width) but is not < 0.03 (depth) — the depth \
         bound alone must reject it"
    );
}

/// The mirror of the case above, with width and depth swapped: 2*17mm = 34mm
/// clears the 40mm depth bound and violates the 30mm WIDTH one. Isolates the
/// FIRST dimension conjunct.
///
/// Together the two pin both bounds symmetrically, so neither can be dropped,
/// duplicated, or silently aimed at the wrong operand.
#[test]
fn corner_r_violating_only_the_width_bound_violates() {
    assert_eq!(
        corner_constraint(&rounded_rect_source_wd("30mm", "40mm", "17mm")),
        Satisfaction::Violated,
        "2*0.017 = 0.034 IS < 0.04 (depth) but is not < 0.03 (width) — the width \
         bound alone must reject it"
    );
}

/// 2*5mm = 10mm < 30mm ≤ 40mm and 5mm > 0 — a perfectly ordinary radius must
/// NOT be flagged. The no-false-positive case: the constraint has to stay
/// silent on valid designs or it is worse than the silent skip it replaced.
#[test]
fn valid_param_corner_r_is_satisfied() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("5mm")),
        Satisfaction::Satisfied,
        "2*0.005 = 0.01 < 0.03 and 0.005 > 0 — must be Satisfied"
    );
}

/// A `corner_r` that is not a dimensioned scalar at all decides
/// `Indeterminate` — and that is the point of still emitting it.
///
/// `param corner_r: Bool = true` compiles CLEAN (measured: no error, no
/// warning names it), so nothing else in the pipeline mentions the offending
/// call until the geometry fails opaquely inside the kernel. The predicate
/// evaluates to `Undef` over a Bool, which surfaces as an
/// Indeterminate-constraint warning carrying the label — the only designer-
/// visible text naming both the constructor and the argument.
///
/// This is why the compiler-side skip is keyed on `Type::Error` (poison, whose
/// root-cause error is already reported) rather than on "not a Scalar"; see
/// `wrongly_typed_but_error_free_corner_r_keeps_its_constraint` in
/// `reify-compiler/tests/rounded_primitives_tests.rs`.
#[test]
fn non_scalar_param_corner_r_is_indeterminate() {
    let source = r#"structure def S {
    param corner_r: Bool = true
    let body = rounded_rect(40mm, 30mm, corner_r)
}"#;
    assert_eq!(
        corner_constraint(source),
        Satisfaction::Indeterminate,
        "a Bool corner_r makes the predicate Undef — the constraint must report \
         that it cannot decide, not silently vanish"
    );
}

/// A violation must say WHICH constructor was at fault and name the argument.
///
/// The label is the only designer-visible text: `SimpleConstraintChecker`
/// emits no span, and `Engine::labeled_diagnostics` substitutes the label for
/// the raw `ConstraintNodeId` in the message. So the constructor name has to
/// live in the label or it reaches nobody.
///
/// RED before the label lands: it is a fixed placeholder naming neither
/// `rounded_box` nor `corner_r`.
#[test]
fn rounded_box_violation_message_names_the_constructor_and_arg() {
    let source = r#"structure def S {
    param corner_r: Length = 25mm
    let body = rounded_box(40mm, 30mm, 20mm, corner_r)
}"#;
    let result = check_source(source);

    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.satisfaction == Satisfaction::Violated)
        .unwrap_or_else(|| {
            panic!(
                "2*0.025 = 0.05 is not < min(0.04, 0.03) — expected a Violated \
                 constraint, got: {:?}",
                result.constraint_results
            )
        });

    let label = entry
        .label
        .as_deref()
        .expect("a synthesized constraint must carry a label");
    assert!(
        label.contains("rounded_box"),
        "the label must name the constructor at fault, got: {label:?}"
    );
    assert!(
        label.contains("corner_r"),
        "the label must name the offending argument, got: {label:?}"
    );

    // And the label must actually reach the designer-facing message.
    let messages: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .filter(|m| m.contains(label))
        .collect();
    assert!(
        !messages.is_empty(),
        "the label must be substituted into the violation message; \
         diagnostics were: {:#?}",
        result.diagnostics
    );
}

/// One offending call must report ONE violation, however many times its
/// geometry let is reused.
///
/// The compiler re-inlines a geometry let's initializer at every reference of
/// its name in geometry-argument position, so without a guard the same
/// rounded-corner predicate is synthesized once per reuse. The designer then
/// sees several Violated constraints, each carrying a DIFFERENT index, for a
/// single wrong call — so the index, which exists precisely to say which call
/// is at fault, actively misleads.
///
/// RED before the dedup lands: two Violated corner constraints, not one.
#[test]
fn reused_geometry_let_reports_a_single_violation() {
    let source = r#"structure def S {
    param corner_r: Length = 25mm
    let plate = rounded_box(40mm, 30mm, 20mm, corner_r)
    let cut = difference(plate, box(10mm, 10mm, 10mm))
}"#;
    // `corner_constraint` itself asserts there is exactly ONE corner-radius
    // constraint, which is the half under test here; the verdict confirms it is
    // still the violation that 2*0.025 = 0.05 ≥ min(0.04, 0.03) demands.
    assert_eq!(
        corner_constraint(source),
        Satisfaction::Violated,
        "a reused geometry let must report exactly one Violated corner-radius \
         constraint for its one offending call"
    );
}

// ─── the solver half ─────────────────────────────────────────────────────────
//
// Everything above stops at `Engine::check`, which only REPORTS. The design
// rationale for synthesizing an ordinary `CompiledConstraint` (rather than
// re-checking post-evaluation in reify-eval) rests on a second claim: because it
// is indistinguishable from any hand-written constraint, an `auto` corner_r is
// genuinely CONSTRAINED — `filter_constraints_reading_autos` hands the predicate
// to the solver, which then has to find a radius that fits. These two pin that
// claim: the first that the predicate reaches the solver at all, the second that
// a real solver does something correct with it.

/// `rounded_rect(15mm, 12mm, corner_r)` with `corner_r` an `auto` cell.
///
/// 15mm/12mm rather than the file's usual 40mm/30mm because the
/// `DimensionalSolver`'s initial guess for a Length cell is 0.01 SI: here
/// 2*0.01 = 0.02 ≥ 0.012, so the starting point is INFEASIBLE and the solver
/// must move to satisfy the constraint. With 40mm/30mm the initial guess is
/// already feasible and the test would pass even if the constraint never
/// reached the solver.
const AUTO_CORNER_R_SOURCE: &str = r#"structure def S {
    param corner_r: Length = auto
    let body = rounded_rect(15mm, 12mm, corner_r)
}"#;

/// The synthesized predicate must be handed to the SOLVER, not merely to the
/// checker.
///
/// A `SpyConstraintSolver` captures the `ResolutionProblem` the engine builds,
/// so this asserts delivery deterministically — no dependence on any numeric
/// convergence behaviour.
#[test]
fn auto_corner_r_hands_the_synthesized_constraint_to_the_solver() {
    let compiled = parse_and_compile(AUTO_CORNER_R_SOURCE);

    // 5mm is a feasible radius (2*0.005 = 0.01 < 0.012); the value only has to
    // be well-formed — what is under test is the captured problem, not the
    // solve.
    let solver = SpyConstraintSolver::new_solved(
        [(ValueCellId::new("S", "corner_r"), Value::length(0.005))]
            .into_iter()
            .collect(),
    );
    let captured = solver.captured_problem();

    let mut engine =
        Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(solver));
    let _ = engine.eval(&compiled);

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("an auto corner_r must produce a solve — the solver was never called");

    assert!(
        problem
            .auto_params
            .iter()
            .any(|p| p.id == ValueCellId::new("S", "corner_r")),
        "S.corner_r must be in the solve's auto params, got: {:?}",
        problem.auto_params
    );
    assert!(
        problem.constraints.iter().any(|(id, _)| id.entity == "S"),
        "the synthesized corner-radius constraint must survive \
         filter_constraints_reading_autos and reach the solver, got: {:?}",
        problem
            .constraints
            .iter()
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>()
    );
}

/// And a REAL solver must resolve that `auto` into a radius that actually fits.
///
/// This is the half that cannot be inferred from delivery alone: it pins that
/// `reify-constraints`' residual builder bounds the cell from an
/// `And(Gt, And(Lt, Lt))` predicate carrying no `domain` and no
/// `optimized_target`. The feasible set is the open interval (0, 6mm) and the
/// solver starts outside it at 10mm, so a resolved value inside means the
/// constraint did the bounding.
#[test]
fn auto_corner_r_solves_into_the_feasible_region() {
    let compiled = parse_and_compile(AUTO_CORNER_R_SOURCE);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None)
        .with_solver(Box::new(DimensionalSolver));
    let _ = engine.eval(&compiled);

    let snapshot = engine.snapshot().expect("eval must produce a snapshot");
    let id = ValueCellId::new("S", "corner_r");
    let (value, _determinacy) = snapshot.values.get(&id).unwrap_or_else(|| {
        panic!(
            "S.corner_r must be present in the snapshot; keys: {:?}",
            snapshot
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    });

    let Value::Scalar { si_value, .. } = value else {
        panic!("S.corner_r must resolve to a Scalar, got: {value:?}");
    };
    // The bounds are the predicate's own, evaluated at the fixture's dimensions:
    // corner_r > 0 and 2*corner_r < 0.012. Compared with a slack of 1e-12 (the
    // solver's own feasibility threshold) rather than exactly, because a
    // minimizer may legitimately settle ON a strict bound.
    assert!(
        *si_value >= -1e-12 && 2.0 * si_value <= 0.012 + 1e-12,
        "the solver must resolve corner_r into the feasible interval (0, 6mm) \
         it started outside of (initial guess 10mm), got {si_value} SI"
    );
}
