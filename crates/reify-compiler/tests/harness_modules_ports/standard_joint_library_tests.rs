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
//! (b′) Non-vacuity companions (task 6384) — (a) and (b) both assert a
//!     diagnostic is ABSENT, an oracle a SKIPPED verdict satisfies as readily as
//!     a clean one. These use a positive/mutation oracle instead, over an inline
//!     body and over the shipped joints.ri text respectively, so the silence
//!     above is known to be a computed verdict. See the (b′) header below.
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
/// companion — the `coincident_body` row of
/// `orientation_dof_is_classified_not_skipped` (section (b′) below) — closes
/// that hole: it over-declares this same body and asserts the mismatch
/// actually fires naming "declared 4 rotational free DOF", a phrase only
/// reachable once `dof_kind_of` has classified `Type::Orientation(3)`.
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
/// a skipped verdict are both silent. The SAME companion closes it: the
/// `coincident_body` row of `orientation_dof_is_classified_not_skipped`
/// (section (b′) below) over-declares this exact body, and one row suffices
/// for both joints because the joint NAME is not an input to DOF
/// classification — this definition and `spherical`'s are byte-identical apart
/// from it.
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
// an unresolvable DOF type name degrades to `Type::Error` and the §7.1 verdict
// gate then sets `skip_verdict` and emits NOTHING (anti-cascade), so a joint
// written `with orientation: Orientation` produced BYTE-IDENTICAL silence to one
// written `with orientation: Blorp`. The full chain, and why the resolver arm
// closing it is the fix, is stated once in the "Orientation type-name
// resolution" header of `type_resolution.rs`'s `mod tests`.
//
// ORACLE CHOICE — the consequence for THIS file. "Zero diagnostics of any code"
// is not a usable non-vacuity oracle here: it is satisfied *by* the defect.
// These companions use a positive/mutation oracle instead — declare an
// orientation-bearing DOF the body cannot satisfy and require the mismatch to
// fire naming `declared N rotational free DOF` with N ≥ 3. That phrase is
// produced only by `describe_declared` inside `check_joint_dof`, which the skip
// path never reaches, and N ≥ 3 requires `dof_kind_of` to have classified
// `Type::Orientation(3)` as (3 rot, 0 trans). It is therefore unsatisfiable
// unless the surface name `Orientation` really resolves and really flows into
// the classifier.

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
/// `check_joint_dof` is a one-place fix rather than one per case.
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
/// proven over two rows, one per body shape:
///
///  1. `orient_probe` — a bare `with orientation: Orientation` over
///     `concentric(a, b)`, whose residual (3,3) − (2,2) = (1 rot, 1 trans)
///     cannot match (3, 0). The narrowest possible probe of the arm: it pins
///     the classifier independently of any joint definition.
///  2. `coincident_body` — the mutation companion for BOTH (b) clean tests
///     above: the same `coincident(c, d)` body they use, but over-declaring
///     `{ orientation: Orientation, extra: Angle }` = (3,0) + (1,0)
///     = (4 rot, 0 trans) against the residual (3 rot, 0 trans). It is
///     declared under the unused name `spherical_probe`, deliberately NOT
///     `spherical`: the prelude already defines that joint, and whether a user
///     may redefine a prelude joint name is a variable this row does not mean
///     to test — if that ever starts being diagnosed (or silently skipped),
///     this row must fail for an Orientation reason or not at all. Only the
///     BODY has to stay byte-identical to `stdlib/joints.ri`'s
///     `spherical`/`ball`, and it does.
///
/// Row 2 is what makes both `spherical_joint_definition_is_self_check_clean`
/// and `ball_joint_definition_is_self_check_clean` non-vacuous, and one row
/// covers both: `spherical` and `ball` are kinematic synonyms with
/// byte-identical bodies, and the joint NAME is not an input to DOF
/// classification — it reaches only `describe_declared`'s message prefix. A
/// second row differing only in that name would add cost, not coverage.
///
/// Scope note: like the (b) tests, these rows compile INLINE joint definitions
/// rather than reading `stdlib/joints.ri`, so they pin the classifier and the
/// verdict machinery, NOT the stdlib bodies. The shipped file's own bytes are
/// covered by `stdlib_spherical_over_declared_dof_is_diagnosed` below, which is
/// also what makes section (a)'s diagnostic-absence oracle mean anything.
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
            "coincident_body (the shared spherical/ball shape): over-declared \
             {orientation: Orientation, extra: Angle} (4rot,0trans) \
             vs coincident residual (3rot,0trans)",
            "joint spherical_probe(c: Point3<Length>, d: Point3<Length>) \
             with { orientation: Orientation, extra: Angle } = coincident(c, d)",
            "declared 4 rotational free DOF",
            "3 rot + 0 trans",
        ),
    ] {
        assert_single_dof_mismatch(label, source, declared_phrase, residual_phrase);
    }
}

/// The SHIPPED `stdlib/joints.ri` text must itself reach the verdict — proven
/// by mutating that text and requiring the mismatch to fire.
///
/// `standard_joint_library_compiles_clean` (section (a)) is the only test in
/// this file that reads the shipped file, and its oracle is diagnostic-ABSENCE
/// — the one oracle a SKIPPED verdict satisfies exactly as well as a clean one
/// (ORACLE CHOICE above). The rows in `orientation_dof_is_classified_not_skipped`
/// fix that for the classifier, but they compile INLINE text. So without this
/// test, editing joints.ri's `with orientation: Orientation` to any
/// unresolvable name — or reverting the resolver arm — would leave every test
/// here green: (a) because the verdict silently skips, (b)/(b′) because they
/// never read the file.
///
/// The mutation over-declares `spherical`'s DOF record in place:
/// `orientation: Orientation` → `{ orientation: Orientation, extra: Angle }` =
/// (4 rot, 0 trans), against the untouched `coincident(c, d)` residual
/// (3 rot, 0 trans). Everything else in the file — including `ball`, which
/// keeps the other of the two `with orientation: Orientation` occurrences and
/// stays clean — is byte-for-byte the shipped text, which is why exactly one
/// mismatch is expected. Section (a)'s silence is thereby attributable to a
/// COMPUTED verdict over the real bytes rather than to a skip.
///
/// The pre-assert on the anchor is load-bearing, not defensive noise: the
/// anchor spans the `joint spherical(…)` line precisely so it selects one of
/// those two occurrences, and if joints.ri is ever reformatted a
/// silently-zero-substitution mutation would compile the UNMODIFIED file and
/// this test would go vacuous in exactly the way it exists to prevent.
#[test]
fn stdlib_spherical_over_declared_dof_is_diagnosed() {
    // Split so the mutation is visibly a swap of the `with` line alone, with
    // the preceding `joint` line serving only to disambiguate spherical from
    // ball. Kept byte-exact against stdlib/joints.ri (4-space continuation
    // indent included) — the anchor assert below is what enforces that.
    const SPHERICAL_DECL: &str = "joint spherical(c: Point3<Length>, d: Point3<Length>)\n";
    const DECLARED_DOF: &str = "    with orientation: Orientation";
    const OVER_DECLARED_DOF: &str = "    with { orientation: Orientation, extra: Angle }";

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib/joints.ri");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read stdlib/joints.ri at `{path}`: {e}"));

    let anchor = format!("{SPHERICAL_DECL}{DECLARED_DOF}");
    assert_eq!(
        source.matches(anchor.as_str()).count(),
        1,
        "the `spherical` DOF-record anchor must occur EXACTLY once in \
         stdlib/joints.ri, or the mutation below is not the mutation this test \
         claims to make. Zero occurrences means the file was reformatted and \
         this test silently stopped mutating anything; more than one means the \
         anchor no longer discriminates spherical from ball. Re-derive the \
         anchor from the file rather than relaxing this assert.\nanchor:\n{anchor}"
    );
    let mutated = source.replace(
        anchor.as_str(),
        &format!("{SPHERICAL_DECL}{OVER_DECLARED_DOF}"),
    );

    assert_single_dof_mismatch(
        "stdlib/joints.ri with `spherical` over-declared to (4rot,0trans) \
         vs its own unmodified coincident residual (3rot,0trans)",
        &mutated,
        "declared 4 rotational free DOF",
        "3 rot + 0 trans",
    );
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
