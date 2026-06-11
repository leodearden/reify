//! Compiler integration tests for **task 4119 δ**, selector composition algebra
//! (`union`/`intersect`/`difference`) — the E_SELECTOR_KIND_MISMATCH diagnostic
//! (BT1) and same-kind composition result type (Type::Selector(k)).
//!
//! RED until step-4 wires the `selector_composition_result_type` arm in
//! `crates/reify-compiler/src/expr.rs`.
//!
//! Two test groups:
//!   (a) Mixed-kind composition: `union`/`intersect`/`difference` over Face and
//!       Edge selectors each produce EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`
//!       diagnostic (Error severity) naming both kinds.  BT1.
//!   (b) Same-kind composition: `union`/`intersect`/`difference` over two Face
//!       selectors compile with no errors and the binding's inferred type is
//!       `Type::Selector(Face)`.

use reify_core::{DiagnosticCode, Severity, ty::SelectorKind};
use reify_core::Type;
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Test sources ─────────────────────────────────────────────────────────────

/// Mixed-kind: `union(faces(b), edges(b))` — Face ∪ Edge → E_SELECTOR_KIND_MISMATCH.
const SOURCE_UNION_MIXED: &str = r#"
structure def UnionMixed {
    let b = box(10mm, 10mm, 10mm)
    let sel = union(faces(b), edges(b))
}
"#;

/// Mixed-kind: `intersect(faces(b), edges(b))` — Face ∩ Edge → E_SELECTOR_KIND_MISMATCH.
const SOURCE_INTERSECT_MIXED: &str = r#"
structure def IntersectMixed {
    let b = box(10mm, 10mm, 10mm)
    let sel = intersect(faces(b), edges(b))
}
"#;

/// Mixed-kind: `difference(faces(b), edges(b))` — Face ∖ Edge → E_SELECTOR_KIND_MISMATCH.
const SOURCE_DIFFERENCE_MIXED: &str = r#"
structure def DifferenceMixed {
    let b = box(10mm, 10mm, 10mm)
    let sel = difference(faces(b), edges(b))
}
"#;

/// Same-kind: `union(faces(b), faces(c))` — Face ∪ Face → Type::Selector(Face).
const SOURCE_UNION_SAME_KIND: &str = r#"
structure def UnionSameKind {
    let b = box(10mm, 10mm, 10mm)
    let c = box(20mm, 20mm, 20mm)
    let sel = union(faces(b), faces(c))
}
"#;

/// Same-kind: `intersect(faces(b), faces(c))` — Face ∩ Face → Type::Selector(Face).
const SOURCE_INTERSECT_SAME_KIND: &str = r#"
structure def IntersectSameKind {
    let b = box(10mm, 10mm, 10mm)
    let c = box(20mm, 20mm, 20mm)
    let sel = intersect(faces(b), faces(c))
}
"#;

/// Same-kind: `difference(faces(b), faces_by_normal(b, ...))` — Face ∖ Face → Selector(Face).
const SOURCE_DIFFERENCE_SAME_KIND: &str = r#"
structure def DifferenceSameKind {
    let b = box(10mm, 10mm, 10mm)
    let sel = difference(faces(b), faces_by_normal(b, [0, 0, 1], 1deg))
}
"#;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Locate the `default_expr` of the named value cell in the first template.
fn cell_default_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("expected '{member}' value cell"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("expected '{member}' cell to have a default_expr"))
}

// ── (a) Mixed-kind: exactly one E_SELECTOR_KIND_MISMATCH per composition ─────

/// `union(faces(b), edges(b))` must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`
/// Error diagnostic whose message names BOTH kinds (Face and Edge). BT1.
#[test]
fn union_mixed_kind_emits_exactly_one_kind_mismatch_error() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_MIXED);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "union(faces, edges): expected exactly 1 E_SELECTOR_KIND_MISMATCH, got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(
        d.severity,
        Severity::Error,
        "E_SELECTOR_KIND_MISMATCH must be Error severity"
    );
    // Message must name both kinds (case-insensitive).
    let msg = d.message.to_lowercase();
    assert!(
        msg.contains("face"),
        "message must name the Face kind, got: {:?}",
        d.message
    );
    assert!(
        msg.contains("edge"),
        "message must name the Edge kind, got: {:?}",
        d.message
    );
    // Must have at least one label (the call-site span).
    assert!(
        !d.labels.is_empty(),
        "E_SELECTOR_KIND_MISMATCH must carry a call-site label, got: {:#?}",
        d
    );
}

/// `intersect(faces(b), edges(b))` must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`.
#[test]
fn intersect_mixed_kind_emits_exactly_one_kind_mismatch_error() {
    let compiled = compile_source_with_stdlib(SOURCE_INTERSECT_MIXED);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "intersect(faces, edges): expected exactly 1 E_SELECTOR_KIND_MISMATCH, got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(d.severity, Severity::Error, "must be Error severity");
    let msg = d.message.to_lowercase();
    assert!(msg.contains("face"), "message must name Face kind");
    assert!(msg.contains("edge"), "message must name Edge kind");
    assert!(!d.labels.is_empty(), "must carry a call-site label");
}

/// `difference(faces(b), edges(b))` must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`.
#[test]
fn difference_mixed_kind_emits_exactly_one_kind_mismatch_error() {
    let compiled = compile_source_with_stdlib(SOURCE_DIFFERENCE_MIXED);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "difference(faces, edges): expected exactly 1 E_SELECTOR_KIND_MISMATCH, got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(d.severity, Severity::Error, "must be Error severity");
    let msg = d.message.to_lowercase();
    assert!(msg.contains("face"), "message must name Face kind");
    assert!(msg.contains("edge"), "message must name Edge kind");
    assert!(!d.labels.is_empty(), "must carry a call-site label");
}

// ── (b) Same-kind: no error, result type is Type::Selector(Face) ──────────────

/// `union(faces(b), faces(c))` must compile without any error-severity diagnostic
/// and the `sel` binding's inferred type must be `Type::Selector(Face)`.
#[test]
fn union_same_kind_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_SAME_KIND);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "union(faces, faces): must compile without errors; got: {errors:#?}"
    );

    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "union(faces(b), faces(c)) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

/// `intersect(faces(b), faces(c))` must compile without errors and the result
/// type must be `Type::Selector(Face)`.
#[test]
fn intersect_same_kind_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_INTERSECT_SAME_KIND);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "intersect(faces, faces): must compile without errors; got: {errors:#?}"
    );

    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "intersect(faces(b), faces(c)) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

/// `difference(faces(b), faces_by_normal(b,...))` must compile without errors
/// and the result type must be `Type::Selector(Face)`.
#[test]
fn difference_same_kind_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_DIFFERENCE_SAME_KIND);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "difference(faces, faces_by_normal): must compile without errors; got: {errors:#?}"
    );

    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "difference(faces(b), faces_by_normal(b,...)) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

// ── (c) Named-leaf constructors: face/edge/solid_body ────────────────────────
// RED until step-8 adds face/edge/solid_body to GEOMETRY_TOPOLOGY_SELECTOR_NAMES
// and topology_selector_result_type in crates/reify-compiler/src/units.rs.
//
// Design note: the BodySelector ctor is `solid_body(g, name)`, NOT `body(g, name)`.
// `body` is the RBD mechanism constructor (joint_signatures.rs → StructureRef("Mechanism")).
// The existing family-disjointness tests in units.rs guard against `body` entering
// GEOMETRY_TOPOLOGY_SELECTOR_NAMES.  The step-8 impl unit test additionally asserts
// `!GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&"body")` directly.

/// `face(b, "top")` source — compiles to `Type::Selector(Face)` after step-8.
const SOURCE_FACE_NAMED: &str = r#"
structure def FaceNamed {
    let b = box(10mm, 10mm, 10mm)
    let sel = face(b, "top")
}
"#;

/// `edge(b, "rim")` source — compiles to `Type::Selector(Edge)` after step-8.
const SOURCE_EDGE_NAMED: &str = r#"
structure def EdgeNamed {
    let b = box(10mm, 10mm, 10mm)
    let sel = edge(b, "rim")
}
"#;

/// `solid_body(b, "core")` source — compiles to `Type::Selector(Body)` after step-8.
const SOURCE_SOLID_BODY_NAMED: &str = r#"
structure def SolidBodyNamed {
    let b = box(10mm, 10mm, 10mm)
    let sel = solid_body(b, "core")
}
"#;

/// `face(b, "top")` must compile without errors and infer `Type::Selector(Face)`.
/// RED until step-8 adds `face` to GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
#[test]
fn face_named_ctor_types_as_face_selector() {
    let compiled = compile_source_with_stdlib(SOURCE_FACE_NAMED);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "face(b, \"top\"): must compile without errors; got: {errors:#?}"
    );
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "face(b, \"top\") must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

/// `edge(b, "rim")` must compile without errors and infer `Type::Selector(Edge)`.
/// RED until step-8 adds `edge` to GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
#[test]
fn edge_named_ctor_types_as_edge_selector() {
    let compiled = compile_source_with_stdlib(SOURCE_EDGE_NAMED);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "edge(b, \"rim\"): must compile without errors; got: {errors:#?}"
    );
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Edge),
        "edge(b, \"rim\") must infer Type::Selector(Edge), got {:?}",
        sel_expr.result_type
    );
}

/// `solid_body(b, "core")` must compile without errors and infer `Type::Selector(Body)`.
/// RED until step-8 adds `solid_body` to GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
/// Closes the `solid_body` vs `body` naming decision (PRD §11.1):
/// `body` stays as the RBD mechanism constructor → `StructureRef("Mechanism")`.
#[test]
fn solid_body_named_ctor_types_as_body_selector() {
    let compiled = compile_source_with_stdlib(SOURCE_SOLID_BODY_NAMED);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "solid_body(b, \"core\"): must compile without errors; got: {errors:#?}"
    );
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Body),
        "solid_body(b, \"core\") must infer Type::Selector(Body) (not StructureRef(\"Mechanism\") \
        which is the `body(...)` RBD ctor type); got {:?}",
        sel_expr.result_type
    );
}

// ── (d) Anti-cascade contract on mismatch ────────────────────────────────────

/// After a mixed-kind mismatch the `sel` binding must still infer
/// `Type::Selector(first_kind)` — the anti-cascade contract prevents a cascade
/// of downstream type errors on an already-diagnosed mismatch.
#[test]
fn union_mixed_kind_sel_still_types_as_selector_after_mismatch() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_MIXED);
    // Confirm the mismatch was emitted (guard against the diagnostic going missing).
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();
    assert_eq!(mismatches.len(), 1, "prerequisite: expected 1 mismatch diagnostic");
    // The cell must still have type Selector(Face) — the first-kind anti-cascade.
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "anti-cascade: sel must infer Selector(Face) even after mismatch, got {:?}",
        sel_expr.result_type
    );
}

// ── (e) Variadic (3-operand) union ───────────────────────────────────────────

const SOURCE_UNION_THREE: &str = r#"
structure def UnionThree {
    let a = box(10mm, 10mm, 10mm)
    let b = box(20mm, 20mm, 20mm)
    let c = box(30mm, 30mm, 30mm)
    let sel = union(faces(a), faces(b), faces(c))
}
"#;

/// `union(faces(a), faces(b), faces(c))` — 3 operands, all Face — must compile
/// without errors and type as `Type::Selector(Face)`.  Locks the variadic path.
#[test]
fn union_three_operands_same_kind_compiles_clean() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_THREE);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "union(3 face selectors): must compile without errors; got: {errors:#?}"
    );
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "union(3 face selectors) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

// ── (f) Nested composition ────────────────────────────────────────────────────

const SOURCE_NESTED_COMPOSITION: &str = r#"
structure def NestedComp {
    let a = box(10mm, 10mm, 10mm)
    let b = box(20mm, 20mm, 20mm)
    let c = box(30mm, 30mm, 30mm)
    let sel = union(union(faces(a), faces(b)), faces(c))
}
"#;

/// `union(union(faces(a), faces(b)), faces(c))` — nested composition — must
/// compile without errors and type as `Type::Selector(Face)`.  Locks the
/// recursive selector-expr path in `is_selector_expr` and the type-ladder arm.
#[test]
fn nested_union_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_NESTED_COMPOSITION);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "union(union(faces,faces), faces): must compile without errors; got: {errors:#?}"
    );
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "nested union must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

// ── (g) Arity enforcement ─────────────────────────────────────────────────────

const SOURCE_UNION_SINGLE_ARG: &str = r#"
structure def UnionSingle {
    let b = box(10mm, 10mm, 10mm)
    let sel = union(faces(b))
}
"#;

/// `union(faces(b))` — one operand, below the ≥2 arity floor — locks the
/// compile-time behavior: no compile-time arity diagnostic (arity is enforced
/// at eval by the `args.len() < 2` gate), so the binding types as
/// `Type::Selector(Face)` and fails silently to Undef at eval.
/// Documents current behavior so a future compile-time arity check is visible
/// as a deliberate breaking change.
#[test]
fn union_single_arg_no_compile_time_arity_error() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_SINGLE_ARG);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "union(single arg): no compile-time arity error expected (arity is eval-side); \
         got: {errors:#?}"
    );
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "union(faces(b)) must still infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

/// `difference` is binary (exactly 2 operands).  Passing 3 same-kind selector
/// operands must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH` error at compile
/// time with a message referencing the arity constraint, so the user is not left
/// with a silent `Value::Undef` at eval.
///
/// The binding must still type as `Type::Selector(Face)` (anti-cascade).
const SOURCE_DIFFERENCE_THREE_ARGS: &str = r#"
structure def DiffThree {
    let a = box(10mm, 10mm, 10mm)
    let b = box(20mm, 20mm, 20mm)
    let c = box(30mm, 30mm, 30mm)
    let sel = difference(faces(a), faces(b), faces(c))
}
"#;

#[test]
fn difference_three_args_emits_compile_time_arity_error() {
    let compiled = compile_source_with_stdlib(SOURCE_DIFFERENCE_THREE_ARGS);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "difference(3 args): expected exactly 1 E_SELECTOR_KIND_MISMATCH arity error, \
         got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(d.severity, Severity::Error, "arity error must be Error severity");
    // Message must mention the count / arity constraint.
    let msg = d.message.to_lowercase();
    assert!(
        msg.contains("2") || msg.contains("exactly"),
        "arity error message must reference the expected count (2) or 'exactly'; got: {:?}",
        d.message
    );
    assert!(!d.labels.is_empty(), "arity error must carry a call-site label");

    // Anti-cascade: the binding must still infer Selector(Face).
    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "anti-cascade: sel must still infer Selector(Face) even after arity error; got {:?}",
        sel_expr.result_type
    );
}
