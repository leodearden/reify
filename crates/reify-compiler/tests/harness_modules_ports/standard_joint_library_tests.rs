//! Tests for the standard joint library — geometric-joints γ (task 4397).
//!
//! The standard joint set (revolute / prismatic / cylindrical / planar /
//! spherical / ball) is defined as `joint … with` declarations over the landed
//! relation vocabulary. Each joint's body residual must exactly match its
//! declared DOF by COUNT and KIND (the β self-checking law). These tests:
//!
//! (a) `standard_joint_library_compiles_clean` — reads `stdlib/joints.ri` and
//!     asserts zero Error-severity diagnostics and zero `JointDofMismatch`
//!     (RED until step-2 creates the file).
//!
//! (b) Per-joint inline tests — compile exactly one `joint … with` definition
//!     per standard joint and assert zero `JointDofMismatch`. These characterise
//!     the landed ΔDOF kind-split tables (relation_signatures.rs) and lock the
//!     joint bodies against regression. GREEN from the moment the β self-check
//!     machinery is wired (pre-landed).
//!
//! DOF derivation — nominal rigid-body freedom = (3 rot, 3 trans):
//!   revolute:    concentric(Axis,Axis)(2,2) + on(Point,Plane)(0,1) → Σ=(2,3) → residual(1,0) ✓
//!   prismatic:   concentric(Axis,Axis)(2,2) + perpendicular(Axis,Axis)(1,0) → Σ=(3,2) → residual(0,1) ✓
//!   cylindrical: concentric(Axis,Axis)(2,2) → Σ=(2,2) → residual(1,1) ✓
//!   planar:      flush(Plane,Plane)(2,1) → Σ=(2,1) → residual(1,2) ✓
//!   spherical:   coincident(Point,Point)(0,3) → Σ=(0,3) → residual(3,0) ✓
//!   ball:        coincident(Point,Point)(0,3) → Σ=(0,3) → residual(3,0) ✓

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// The error-severity `JointDofMismatch` diagnostics emitted while compiling
/// `module` — the β joint-DOF self-check signal (mirrors β's `joint_dof_errors`
/// helper in `joint_dof_self_check_tests.rs`).
fn joint_dof_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::JointDofMismatch) && d.severity == Severity::Error
        })
        .collect()
}

// ── (a) Library-compiles-clean ────────────────────────────────────────────────

/// The standard joint library `stdlib/joints.ri` compiles with zero
/// Error-severity diagnostics and zero `JointDofMismatch` — all 6 standard
/// joints are self-check-clean.
///
/// RED: `stdlib/joints.ri` does not exist yet → the file read fails with
/// `std::io::Error`. Step-2 (impl) creates the file and makes this green.
#[test]
fn standard_joint_library_compiles_clean() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib/joints.ri");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read stdlib/joints.ri at `{path}`: {e}"));
    let module = compile_source_with_stdlib(&source);

    let errors: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "stdlib/joints.ri must compile with zero Error-severity diagnostics, got: {errors:#?}",
    );
    assert!(
        joint_dof_errors(&module).is_empty(),
        "stdlib/joints.ri must emit zero E_JOINT_DOF_MISMATCH (all 6 joints must be \
         self-check-clean): {:#?}",
        joint_dof_errors(&module)
    );
}

// ── (b) Per-joint inline self-check-clean tests ──────────────────────────────
//
// Each test compiles exactly one `joint … with` definition inline (without the
// full library file) and asserts zero `JointDofMismatch` diagnostics. This
// pins:
//   - the relation ΔDOF kind-split tables (relation_delta_dof_kinds in
//     relation_signatures.rs) against drift;
//   - the canonical joint body for each standard kind against regression.
//
// These tests are GREEN from the moment the β self-check machinery is wired
// (pre-landed, task 4396). Creating joints.ri (step-2) does not affect them.
//
// NOTE — intentional overlap with joint_dof_self_check_tests.rs (task 4396):
// `revolute_joint_definition_is_self_check_clean` overlaps B1 there
// (`b1_revolute_concentric_plus_on_is_clean`) and
// `cylindrical_joint_definition_is_self_check_clean` overlaps B4
// (`b4_cylindrical_record_is_clean`). The distinction is the `in <range>`
// clause added here: the β tests pin count/kind without a range bound;
// these pin that count/kind passes unchanged when a dimensionally-typed
// `in <range>` is present, which IS the form shipped in joints.ri. They
// act as regression-guards that the range annotation does not perturb the
// self-check signal (the β `range_dimension_match_angle_dof_angle_range_is_clean`
// control pins range-dimension acceptance; these pin the full combined form).

/// revolute: concentric(a,b)=(2rot,2trans) + on(p,stop)=(0rot,1trans)
/// → Σ=(2,3) → residual(1rot,0trans) = angle:Angle ✓
///
/// Overlaps `b1_revolute_concentric_plus_on_is_clean` in
/// `joint_dof_self_check_tests.rs` (β, task 4396). The extra coverage here is
/// the `in 0deg..120deg` range clause, which is present in the shipped
/// joints.ri and absent from the β control.
#[test]
fn revolute_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint revolute(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) \
         with angle: Angle in 0deg..120deg = { concentric(a, b)  on(p, stop) }",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "revolute: residual(1rot,0trans) must match declared `angle: Angle` = (1,0) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// prismatic: concentric(a,b)=(2rot,2trans) + perpendicular(key_a,key_b)=(1rot,0trans)
/// → Σ=(3,2) → residual(0rot,1trans) = travel:Length ✓
/// (perpendicular lifts Axis→Direction via .dir; ΔDOF=(1,0) unconditional)
#[test]
fn prismatic_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint prismatic(a: Axis, b: Axis, key_a: Axis, key_b: Axis) \
         with travel: Length in 0mm..50mm = { concentric(a, b)  perpendicular(key_a, key_b) }",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "prismatic: residual(0rot,1trans) must match declared `travel: Length` = (0,1) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// cylindrical: concentric(a,b)=(2rot,2trans) → Σ=(2,2) → residual(1rot,1trans)
/// = { angle:Angle, travel:Length } = (1,1) ✓
///
/// Overlaps `b4_cylindrical_record_is_clean` in `joint_dof_self_check_tests.rs`
/// (β, task 4396). The extra coverage here is the `in 0deg..360deg` /
/// `in 0mm..50mm` range bounds present in the shipped joints.ri.
#[test]
fn cylindrical_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint cylindrical(a: Axis, b: Axis) \
         with { angle: Angle in 0deg..360deg, travel: Length in 0mm..50mm } = concentric(a, b)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "cylindrical: residual(1rot,1trans) must match declared `{{ angle:Angle, travel:Length }}` \
         = (1,1) → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// planar: flush(face_a,face_b)=(2rot,1trans) → Σ=(2,1) → residual(1rot,2trans)
/// = { x:Length, y:Length, spin:Angle } = (1,2) ✓
#[test]
fn planar_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint planar(face_a: Plane, face_b: Plane) \
         with { x: Length, y: Length, spin: Angle } = flush(face_a, face_b)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "planar: residual(1rot,2trans) must match declared `{{ x:Length, y:Length, spin:Angle }}` \
         = (1,2) → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// spherical: coincident(c,d) where c,d:Point3<Length> → (0rot,3trans)
/// → Σ=(0,3) → residual(3rot,0trans) = orientation:Orientation ✓
///
/// NON-VACUITY: a *clean* verdict here is indistinguishable, from diagnostics
/// alone, from a *skipped* verdict — if `Orientation` failed to resolve, the
/// DOF type would become `Type::Error`, which suppresses the §7.1 count/kind
/// verdict entirely (anti-cascade, `compile_builder/entities_phase.rs`) and
/// emits nothing at all. This test would then pass for the wrong reason. Its
/// companion — the `spherical` row of `orientation_dof_is_classified_not_skipped`
/// (section (b′) below) — closes that hole: it over-declares the same body and
/// asserts the mismatch actually fires naming "declared 4 rotational free DOF",
/// a phrase only reachable once `dof_kind_of` has classified
/// `Type::Orientation(3)`.
#[test]
fn spherical_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint spherical(c: Point3<Length>, d: Point3<Length>) \
         with orientation: Orientation = coincident(c, d)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "spherical: residual(3rot,0trans) must match declared `orientation: Orientation` = (3,0) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// ball: coincident(c,d) where c,d:Point3<Length> → (0rot,3trans)
/// → Σ=(0,3) → residual(3rot,0trans) = orientation:Orientation ✓
/// (design §7 canonical name; kinematic synonym of spherical — both defined to
/// preserve both vocabularies)
///
/// NON-VACUITY: same masking hazard as
/// `spherical_joint_definition_is_self_check_clean` above — a clean verdict and
/// a skipped verdict are both silent. Its companion — the `ball` row of
/// `orientation_dof_is_classified_not_skipped` (section (b′) below) — proves
/// the verdict is genuinely computed, not skipped.
#[test]
fn ball_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint ball(c: Point3<Length>, d: Point3<Length>) \
         with orientation: Orientation = coincident(c, d)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "ball: residual(3rot,0trans) must match declared `orientation: Orientation` = (3,0) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

// ── (b′) Orientation-DOF non-vacuity companions (task 6384) ─────────────────
//
// The (b) `spherical` / `ball` tests above assert the ABSENCE of a diagnostic.
// That oracle is satisfiable two ways: the verdict was computed and matched, or
// the verdict was never computed at all. The second way is a live failure mode —
// `resolve_type_expr_with_aliases` returns `None` with NO diagnostic for an
// unknown bare type name, `compile_joint_self_check` maps that to `Type::Error`,
// and the verdict gate treats `Type::Error` as "already diagnosed elsewhere" and
// sets `skip_verdict` without emitting anything (anti-cascade). A joint written
// `with orientation: Orientation` therefore produced BYTE-IDENTICAL silence to
// one written `with orientation: Blorp`.
//
// So "zero diagnostics of any code" is NOT a usable non-vacuity oracle here: it
// is satisfied *by* the defect. These companions use a positive/mutation oracle
// instead — declare an orientation-bearing DOF the body cannot satisfy and
// require the mismatch to fire naming `declared N rotational free DOF` with
// N ≥ 3. That phrase is produced only by `describe_declared` inside
// `check_joint_dof`, which the skip path never reaches, and N ≥ 3 requires
// `dof_kind_of` to have classified `Type::Orientation(3)` as (3 rot, 0 trans).
// It is therefore unsatisfiable unless the surface name `Orientation` really
// resolves and really flows into the classifier.

/// Assert that `source` draws EXACTLY ONE `E_JOINT_DOF_MISMATCH` whose message
/// names both `declared_phrase` (from `describe_declared`) and
/// `residual_phrase` (the body's computed residual).
///
/// Both halves matter and neither is redundant:
///   * exactly-one — zero would mean the DOF type never resolved and the
///     verdict was SKIPPED (the `Type::Error` → `skip_verdict` path), which is
///     byte-identically silent to a clean verdict;
///   * `declared_phrase` — produced only by `describe_declared` inside
///     `check_joint_dof`, code the skip path never reaches, and its rotational
///     count is unreachable at N ≥ 3 unless `dof_kind_of` really classified
///     `Type::Orientation(3)` as (3 rot, 0 trans);
///   * `residual_phrase` — pins the value the (b) clean tests above claim
///     `orientation: Orientation` matches.
///
/// `label` names the row so a failure in the table below is attributable
/// without re-running each case by hand. The exact diagnostic prose lives HERE
/// and in the table rows only, so a rewording of `describe_declared` /
/// `check_joint_dof` is a one-place fix rather than three.
fn assert_single_dof_mismatch(
    label: &str,
    source: &str,
    declared_phrase: &str,
    residual_phrase: &str,
) {
    let module = compile_source_with_stdlib(source);
    let errs = joint_dof_errors(&module);
    assert_eq!(
        errs.len(),
        1,
        "{label}: exactly one E_JOINT_DOF_MISMATCH must fire. Zero here means the \
         declared DOF type never resolved and the §7.1 verdict was SKIPPED, not \
         clean — which would make the matching (b) test above vacuous.\n\
         source: {source}\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
    let msg = &errs[0].message;
    assert!(
        msg.contains(declared_phrase),
        "{label}: the mismatch must report the declared side as {declared_phrase:?} \
         (i.e. `dof_kind_of` actually classified the Orientation DOF as \
         (3 rot, 0 trans)), got: {msg}",
    );
    assert!(
        msg.contains(residual_phrase),
        "{label}: the mismatch must report the body residual as {residual_phrase:?}, \
         got: {msg}",
    );
}

/// The `Orientation` DOF type must be CLASSIFIED, never silently skipped —
/// proven over three rows, one per hazard:
///
///  1. `orient_probe` — a bare `with orientation: Orientation` over
///     `concentric(a, b)`, whose residual (3,3) − (2,2) = (1 rot, 1 trans)
///     cannot match (3, 0). The narrowest possible probe of the arm.
///  2. `spherical` — the mutation companion for
///     `spherical_joint_definition_is_self_check_clean`: the same
///     `coincident(c, d)` body, but over-declaring
///     `{ orientation: Orientation, extra: Angle }` = (3,0) + (1,0)
///     = (4 rot, 0 trans) against the residual (3 rot, 0 trans).
///  3. `ball` — the identical over-declaration on the kinematic synonym, so a
///     future change touching only one of the two joint definitions cannot
///     leave the other vacuously green.
///
/// Rows 2 and 3 are what make the (b) clean tests non-vacuous; row 1 pins the
/// classifier itself independently of either joint definition.
#[test]
fn orientation_dof_is_classified_not_skipped() {
    for (label, source, declared_phrase, residual_phrase) in [
        (
            "orient_probe: declared Orientation (3rot,0trans) vs concentric residual (1rot,1trans)",
            "joint orient_probe(a: Axis, b: Axis) \
             with orientation: Orientation = concentric(a, b)",
            "declared 3 rotational free DOF",
            "1 rot + 1 trans",
        ),
        (
            "spherical: over-declared {orientation: Orientation, extra: Angle} (4rot,0trans) \
             vs coincident residual (3rot,0trans)",
            "joint spherical(c: Point3<Length>, d: Point3<Length>) \
             with { orientation: Orientation, extra: Angle } = coincident(c, d)",
            "declared 4 rotational free DOF",
            "3 rot + 0 trans",
        ),
        (
            "ball: over-declared {orientation: Orientation, extra: Angle} (4rot,0trans) \
             vs coincident residual (3rot,0trans)",
            "joint ball(c: Point3<Length>, d: Point3<Length>) \
             with { orientation: Orientation, extra: Angle } = coincident(c, d)",
            "declared 4 rotational free DOF",
            "3 rot + 0 trans",
        ),
    ] {
        assert_single_dof_mismatch(label, source, declared_phrase, residual_phrase);
    }
}

// ── (c) B8 boundary tests ─────────────────────────────────────────────────────
//
// Couplings (couple / gear / screw / rack_and_pinion) type to
// `Type::StructureRef("Coupling")`, NOT `Type::Relation`. Core-δ's
// `check_relate_relations` (entity.rs) rejects any relate-block member whose
// type ≠ `Type::Relation` with `DiagnosticCode::RelateExpectsRelation`.
//
// These tests characterise and LOCK the pre-landed B8 boundary (geometric-joints
// γ owns enforcing + documenting this boundary — task 4397). No new code is
// needed; the enforcement is by composition (core-δ + joint_signatures.rs).

/// Filter error-severity `RelateExpectsRelation` diagnostics — the δ
/// relate-block enforcement signal (mirrors `relate_block_check_tests.rs`).
fn relate_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::RelateExpectsRelation) && d.severity == Severity::Error
        })
        .collect()
}

/// B8 — `couple(a, b)` in a `relate { }` body draws `E_RELATE_EXPECTS_RELATION`:
/// `couple` types to `Type::StructureRef("Coupling")` (not `Type::Relation`), so
/// core-δ rejects it at relate-block enforcement time.
#[test]
fn couple_in_relate_block_draws_relate_expects_relation() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a : Axis\n    param b : Axis\n    \
         relate { couple(a, b) }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        !errs.is_empty(),
        "couple() types to StructureRef(\"Coupling\") (not Type::Relation); \
         a `relate {{ }}` block containing it must emit E_RELATE_EXPECTS_RELATION.\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
}

/// B8 variant — `gear(a, b)` in a `relate { }` body draws
/// `E_RELATE_EXPECTS_RELATION` for the same reason: `gear` also types to
/// `Type::StructureRef("Coupling")`.
#[test]
fn gear_in_relate_block_draws_relate_expects_relation() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a : Axis\n    param b : Axis\n    \
         relate { gear(a, b) }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        !errs.is_empty(),
        "gear() types to StructureRef(\"Coupling\") (not Type::Relation); \
         a `relate {{ }}` block containing it must emit E_RELATE_EXPECTS_RELATION.\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
}

/// B8 constructor-health companion — `couple(a, b)` as a bare `let` binding
/// (outside any `relate { }` body) must compile without Error-severity
/// diagnostics. This confirms the coupling constructor path is healthy and that
/// the `RelateExpectsRelation` errors drawn above are specifically because
/// `couple()` types to `StructureRef("Coupling")` (not `Type::Relation`) — NOT
/// because the constructor fails to resolve for some unrelated reason.
///
/// Joint constructors return a fixed `StructureRef` regardless of argument
/// types (no arg-type enforcement for joint builtins at compile time, §13). So
/// `couple(a, b)` with `a: Axis, b: Axis` is a valid expression that the
/// compiler accepts, typing it to `Coupling` without error.
///
/// Without this companion, a future arity/scope regression that breaks
/// `couple` resolution could cause a *different* error whose type is still not
/// `Type::Relation`, firing `RelateExpectsRelation` for the wrong reason and
/// masking the regression in the B8 tests above.
#[test]
fn couple_constructor_outside_relate_is_healthy() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a : Axis\n    param b : Axis\n    \
         let c = couple(a, b)\n}",
    );
    let errors: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "couple(a, b) as a bare `let` binding must compile without Error diagnostics \
         (the constructor is healthy and returns Coupling outside relate); got: {errors:#?}",
    );
}

/// B8 positive control — `concentric(a, b)` in a `relate { }` body is accepted:
/// a genuine drive relation types to `Type::Relation` and is NOT rejected.
/// This confirms that the B8 rejection is coupling-specific, not a blanket error.
#[test]
fn concentric_in_relate_block_is_accepted() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a : Axis\n    param b : Axis\n    \
         relate { concentric(a, b) }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        errs.is_empty(),
        "concentric(a, b) types to Type::Relation and must NOT emit \
         E_RELATE_EXPECTS_RELATION in a `relate {{ }}` block, got: {errs:#?}",
    );
}

// ── (d) stdlib registration check ────────────────────────────────────────────

/// `std.joints` must be registered as a prelude stdlib module — `load_stdlib()`
/// returns a compiled module whose `path` display is `std/joints`.
///
/// RED: `std.joints` is not registered in `stdlib_sources()` yet (step-3 test).
/// Step-4 (impl) adds the `include_str!` entry and makes this green.
#[test]
fn std_joints_registered_in_stdlib_prelude() {
    let modules = reify_compiler::stdlib_loader::load_stdlib();
    let found = modules
        .iter()
        .any(|m| format!("{}", m.path) == "std/joints");
    assert!(
        found,
        "std.joints must be registered in the stdlib prelude (stdlib_loader.rs::stdlib_sources);\n\
         currently loaded module paths: {:?}",
        modules
            .iter()
            .map(|m| format!("{}", m.path))
            .collect::<Vec<_>>()
    );
}
