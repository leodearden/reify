//! Lowering (CST→AST) tests for geometric-joints α (task 4395).
//!
//! Covers the lowering of `joint NAME(datums) with <DOF> = <body>` definitions
//! into `Declaration::Joint(JointDef)` — BOTH the single-form (`with angle: Angle
//! in 0deg..120deg`) and the record-form (`with { angle: Angle, travel: Length }`).
//!
//! TDD RED: this file fails to COMPILE until step-6 adds `JointDef`,
//! `JointDofField`, and `Declaration::Joint` to reify-ast plus the lowering arm
//! in ts_parser.rs. This is the established compile-failure RED convention for
//! API-surface additions in this codebase (cf. default_decl_tests.rs, task 4496).
//!
//! Scope (α = parse + lower ONLY): tests assert structural shape — name, param
//! count, dof length, body length, range presence/absence. No type-checking, no
//! DOF self-check, no validate_range — those are β's task.

use reify_ast::*;
use reify_core::ModulePath;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse `source` and return the first `Declaration::Joint(JointDef)`.
/// Panics if there are parse errors or the first declaration is not a joint.
fn first_joint(source: &str) -> JointDef {
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    match module.declarations.into_iter().next().expect("no declarations") {
        Declaration::Joint(j) => j,
        other => panic!("expected Declaration::Joint, got {:?}", other),
    }
}

// ── Single-form tests ─────────────────────────────────────────────────────────

/// `joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in 0deg..120deg = { coaxial(a, b)  on(a.point, stop) }`
/// lowers to Declaration::Joint(JointDef) with:
///   - name = "revolute"
///   - params: 3 FnParam (a/Axis, b/Axis, stop/Plane)
///   - dof: Vec<JointDofField> of len 1, name="angle", type=Angle, range=Some(_)
///   - body: Vec<Expr> of len 2 (coaxial + on)
///
/// RED until step-6 (JointDef / Declaration::Joint absent).
#[test]
fn joint_single_form_lowers_to_jointdef() {
    let source = "joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in 0deg..120deg = { coaxial(a, b)  on(a.point, stop) }";
    let j = first_joint(source);

    assert_eq!(j.name, "revolute", "joint name must be 'revolute'");
    assert!(!j.is_pub, "revolute is not pub");

    // 3 datum params: a: Axis, b: Axis, stop: Plane
    assert_eq!(
        j.params.len(),
        3,
        "expected 3 params, got {}",
        j.params.len()
    );
    assert_eq!(j.params[0].name, "a");
    assert_eq!(j.params[1].name, "b");
    assert_eq!(j.params[2].name, "stop");

    // dof: 1 field, name=angle, type=Angle, range=Some(_)
    assert_eq!(
        j.dof.len(),
        1,
        "single-form must have exactly 1 dof field, got {}",
        j.dof.len()
    );
    assert_eq!(j.dof[0].name, "angle", "dof[0].name must be 'angle'");
    match &j.dof[0].type_expr.kind {
        TypeExprKind::Named { name, .. } => {
            assert_eq!(name, "Angle", "dof[0] type must be 'Angle', got '{name}'")
        }
        other => panic!("expected TypeExprKind::Named for dof[0].type_expr, got {:?}", other),
    }
    assert!(
        j.dof[0].range.is_some(),
        "single-form dof with `in 0deg..120deg` must have range=Some"
    );

    // body: 2 expressions (coaxial + on)
    assert_eq!(
        j.body.len(),
        2,
        "block body must lower to 2 expressions, got {}",
        j.body.len()
    );
}

/// `joint ball(c: Point, d: Point) with orientation: Orientation = coincident(c, d)`
/// lowers with dof[0].range == None (no `in` clause).
///
/// RED until step-6.
#[test]
fn joint_no_range_dof_lowers_range_none() {
    let source = "joint ball(c: Point, d: Point) with orientation: Orientation = coincident(c, d)";
    let j = first_joint(source);

    assert_eq!(j.name, "ball");
    assert_eq!(j.dof.len(), 1);
    assert_eq!(j.dof[0].name, "orientation");
    assert!(
        j.dof[0].range.is_none(),
        "dof field without `in …` must have range=None"
    );

    // body: single-expression form lowers to a 1-element Vec<Expr>
    assert_eq!(
        j.body.len(),
        1,
        "single-expr body must lower to exactly 1 expression, got {}",
        j.body.len()
    );
}

// ── Record-form tests ─────────────────────────────────────────────────────────

/// `joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } = coaxial(a, b)`
/// lowers with dof of len 2 (angle: Angle, travel: Length) and body of len 1.
///
/// RED until step-6.
#[test]
fn joint_record_form_lowers_dof_fields() {
    let source = "joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } = coaxial(a, b)";
    let j = first_joint(source);

    assert_eq!(j.name, "cylindrical");
    assert_eq!(
        j.params.len(),
        2,
        "cylindrical must have 2 datum params, got {}",
        j.params.len()
    );

    // record-form dof: 2 fields in source order
    assert_eq!(
        j.dof.len(),
        2,
        "record-form must have exactly 2 dof fields, got {}",
        j.dof.len()
    );
    assert_eq!(j.dof[0].name, "angle", "dof[0].name must be 'angle'");
    assert_eq!(j.dof[1].name, "travel", "dof[1].name must be 'travel'");
    match &j.dof[0].type_expr.kind {
        TypeExprKind::Named { name, .. } => {
            assert_eq!(name, "Angle", "dof[0] type must be 'Angle'")
        }
        other => panic!("expected Named for dof[0].type_expr, got {:?}", other),
    }
    match &j.dof[1].type_expr.kind {
        TypeExprKind::Named { name, .. } => {
            assert_eq!(name, "Length", "dof[1] type must be 'Length'")
        }
        other => panic!("expected Named for dof[1].type_expr, got {:?}", other),
    }

    // body: 1 expression (coaxial)
    assert_eq!(
        j.body.len(),
        1,
        "single-expr body must lower to 1 expression, got {}",
        j.body.len()
    );
}

// ── Block-body ordering test ──────────────────────────────────────────────────

/// A 2-relation block body lowers to body.len()==2 in source order.
/// Reuses the relation_member lowering (same as relate_block).
///
/// RED until step-6.
#[test]
fn joint_block_body_lowers_conjunction_in_order() {
    let source = "joint pin(a: Axis, b: Axis, c: Point) with rotation: Angle = { coaxial(a, b)  on(c, a.point) }";
    let j = first_joint(source);

    assert_eq!(j.body.len(), 2, "block body must produce 2 body exprs");

    // First expr: coaxial call
    match &j.body[0].kind {
        ExprKind::FunctionCall { name, .. } => {
            assert_eq!(name, "coaxial", "body[0] must be 'coaxial' call")
        }
        other => panic!("body[0] must be ExprKind::FunctionCall, got {:?}", other),
    }
    // Second expr: on call
    match &j.body[1].kind {
        ExprKind::FunctionCall { name, .. } => {
            assert_eq!(name, "on", "body[1] must be 'on' call")
        }
        other => panic!("body[1] must be ExprKind::FunctionCall, got {:?}", other),
    }
}

// ── Amendment tests (amend pass) ─────────────────────────────────────────────

/// `/// A revolute joint.\npub joint revolve<T>(a: Axis) with x: Angle = coaxial(a, a)`
/// lowers with:
///   - is_pub == true
///   - type_params.len() == 1 (the `T` parameter)
///   - doc == Some("A revolute joint.")   (extracted from the preceding `///` line)
///
/// Covers the three non-default paths in lower_joint that were exercised by
/// lower_function's analogous paths but had no joint-specific test (suggestion 1).
#[test]
fn joint_pub_type_param_and_doc_lower_correctly() {
    let source = "/// A revolute joint.\npub joint revolve<T>(a: Axis) with x: Angle = coaxial(a, a)";
    let j = first_joint(source);

    assert!(j.is_pub, "pub joint must have is_pub == true");
    assert_eq!(
        j.type_params.len(),
        1,
        "joint<T> must have exactly 1 type param, got {}",
        j.type_params.len()
    );
    assert_eq!(
        j.type_params[0].name, "T",
        "type_params[0].name must be 'T', got '{}'",
        j.type_params[0].name
    );
    assert_eq!(
        j.doc,
        Some("A revolute joint.".to_string()),
        "doc comment must be extracted from the preceding `///` line"
    );
}

/// `@kinematic\njoint foo(a: Axis) with x: Angle = coaxial(a, a)` — an `@kinematic`
/// annotation preceding the joint definition must land in `j.annotations`.
///
/// The annotation is drained from `pending_annotations` in `lower_source_file` and
/// set on the JointDef before pushing — same as every other Declaration type.
/// A regression in that wiring would pass undetected without this test (suggestion 1).
#[test]
fn joint_annotation_propagates_to_jointdef() {
    let source = "@kinematic\njoint foo(a: Axis) with x: Angle = coaxial(a, a)";
    let j = first_joint(source);

    assert_eq!(
        j.annotations.len(),
        1,
        "expected 1 annotation on joint, got {:?}",
        j.annotations
    );
    assert_eq!(
        j.annotations[0].name, "kinematic",
        "annotation name must be 'kinematic', got '{}'",
        j.annotations[0].name
    );
    assert!(
        j.annotations[0].args.is_empty(),
        "bare @kinematic must have no args, got {:?}",
        j.annotations[0].args
    );
}

/// `joint x(a: Axis) with t: Angle = { }` — an EMPTY block body lowers to
/// `body.len() == 0` silently (no diagnostic in α).
///
/// This pins the α lowering contract so β knows what it must validate: an
/// empty joint body is structurally valid at the CST/AST level and must be
/// caught by β's DOF-body self-check (E_JOINT_DOF_MISMATCH or similar) rather
/// than by the lowerer (suggestion 2 — document rather than enforce in α).
#[test]
fn joint_empty_block_body_lowers_to_empty_vec() {
    let source = "joint x(a: Axis) with t: Angle = { }";
    let j = first_joint(source);

    // α: no errors — the empty body passes through the lowerer silently.
    // β TODO: emit a diagnostic here (empty joint body is semantically invalid). // ptodo:allow test note - no live owner
    assert_eq!(
        j.body.len(),
        0,
        "empty block body `= {{ }}` must lower to body.len()==0 in α; \
         β must emit a diagnostic for this case"
    );
}
