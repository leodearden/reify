//! CLI integration tests for stdlib purpose activation (task-4016 ζ).
//!
//! ## B8 oracle (resolved)
//!
//! PRD §8.1 row B8 said geometry-undef → "indeterminate", but Leo decision A /
//! esc-4016-163 resolved it: geometry-undef → VIOLATED/not-ready (exit non-zero).
//! `determined(p)` returns `Value::Bool(false)` (never Undef) for a geometry-undef
//! cell (reify-expr/src/lib.rs:740-742), so `forall p in geometric_params: determined(p)`
//! is total → Bool(false) → Violated → SomeViolated → exitFAILURE.
//! Indeterminate is unreachable from any determinacy-predicate body.
//!
//! ## Coverage
//!
//! step-3: B8 — simulation_ready stdlib purpose end-to-end CLI (geometry ok/undef).
//! step-5: design_review — stdlib purpose end-to-end CLI (all-auto PASS, determined FAIL).
//! step-7: B3 (let), B4 (guard-active), B5 (guard-undef→indeterminate) — inline-purpose
//!          consumer-facing CLI integration locks.

mod common;

// ── step-3: B8 — simulation_ready stdlib purpose (geometry ok/undef) ──────────

/// B8 PASS: a fully-determined structure satisfies simulation_ready.
///
/// All geometric params have concrete defaults → determined(p)=true for each →
/// `forall p in geometric_params: determined(p)` = true → AllSatisfied → exit 0.
///
/// RED until merge_prelude_purposes propagates simulation_ready into the user module.
#[test]
fn simulation_ready_geometry_ok_passes() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "simulation_ready=Part",
        &common::fixture_path("stdlib_sim_ready_geom_ok.ri"),
    ]);

    assert!(
        status.success(),
        "simulation_ready=Part on a geometry-determined structure should exit 0\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' for geometry-ok; \
         got: {stdout}"
    );
}

/// B8 FAIL (not-ready): a geometry-undetermined structure violates simulation_ready.
///
/// `width` is auto (DeterminacyState::Auto) → determined(width)=false →
/// `forall p in geometric_params: determined(p)` = false → Violated →
/// SomeViolated → exit non-zero.
///
/// This is the VIOLATED/not-ready path — NOT indeterminate (esc-4016-163).
/// Contrast with B5 where a Kleene-undef guard condition yields Indeterminate.
///
/// RED until merge_prelude_purposes propagates simulation_ready into the user module.
#[test]
fn simulation_ready_geometry_undef_violates() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "simulation_ready=Part",
        &common::fixture_path("stdlib_sim_ready_geom_undef.ri"),
    ]);

    assert!(
        !status.success(),
        "simulation_ready=Part on a geometry-undef structure should exit non-zero\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain 'Some constraints violated.' for geometry-undef; \
         got: {stdout}"
    );
}

/// simulation_ready with geometry-determined + determined material param passes.
///
/// The material guard (`where exists p in material_params: constrained(p)`) is
/// INACTIVE because mat is Determined (not auto/constrained), so the guard arm
/// does not fire. The geometry constraint (forall geometric_params: determined)
/// passes → AllSatisfied → exit 0.
///
/// Proves the material guard does not over-block a geometry-complete part.
///
/// RED until merge_prelude_purposes propagates simulation_ready into the user module.
#[test]
fn simulation_ready_determined_material_passes() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "simulation_ready=Part",
        &common::fixture_path("stdlib_sim_ready_material_ok.ri"),
    ]);

    assert!(
        status.success(),
        "simulation_ready=Part with determined material should exit 0 (guard inactive)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' for geometry-ok + determined material; \
         got: {stdout}"
    );
}

/// simulation_ready with auto (constrained) material param violates — guard-active-Violated.
///
/// The material guard (`where exists p in material_params: constrained(p)`) FIRES because
/// `mat` is auto → constrained(mat)=true. The guard body requires
/// `forall p in material_params: determined(p)` — but mat is auto (not determined) →
/// body = false → Violated → SomeViolated → exit non-zero.
///
/// This locks the guard-active branch described in the determinacy_purposes.ri doc comment:
/// "constrained() and determined() are mutually exclusive states, so this arm can only
/// fire Violated". Geometry (width=80mm) is fully determined, so the geometry constraint
/// alone would pass; the violation is exclusively from the guard-active material branch.
#[test]
fn simulation_ready_auto_material_guard_active_violates() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "simulation_ready=Part",
        &common::fixture_path("stdlib_sim_ready_material_active.ri"),
    ]);

    assert!(
        !status.success(),
        "simulation_ready=Part with auto material should exit non-zero \
         (guard active → Violated)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain 'Some constraints violated.' for guard-active branch; \
         got: {stdout}"
    );
}

// ── step-5: design_review — stdlib purpose end-to-end CLI ────────────────────

/// design_review PASS: a structure with all-auto params is in design-review state.
///
/// `forall p in params: constrained(p)` is true when every param is auto
/// (DeterminacyState::Auto → constrained()=true) → AllSatisfied → exit 0.
///
/// RED until merge_prelude_purposes propagates design_review into the user module.
#[test]
fn design_review_all_auto_passes() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "design_review=AllAuto",
        &common::fixture_path("stdlib_design_review.ri"),
    ]);

    assert!(
        status.success(),
        "design_review=AllAuto should exit 0 (all params are auto/constrained)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' for all-auto structure; \
         got: {stdout}"
    );
}

/// design_review FAIL: a structure with a determined param violates design_review.
///
/// `x = 5mm` → DeterminacyState::Determined → constrained()=false →
/// `forall p in params: constrained(p)` = false → Violated → exit non-zero.
///
/// RED until merge_prelude_purposes propagates design_review into the user module.
#[test]
fn design_review_determined_param_violates() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "design_review=HasDet",
        &common::fixture_path("stdlib_design_review.ri"),
    ]);

    assert!(
        !status.success(),
        "design_review=HasDet should exit non-zero (has a determined param)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain 'Some constraints violated.' for structure with determined param; \
         got: {stdout}"
    );
}

// ── step-7: B3/B4/B5 — inline-purpose consumer-facing CLI integration locks ───
//
// These test INLINE purposes (defined in the fixture file itself, not stdlib).
// No merge_prelude_purposes needed — they pass once step-2 (determinacy_purposes.ri
// registered) lands; no new impl step required.

/// B4: guard-active arm fires and is satisfied.
///
/// `vg(s): where s.flag > 0mm { constraint s.val > 10mm }`.
/// ActiveSat: flag=1mm, val=20mm → guard condition 1mm>0mm = true (active) →
/// constraint 20mm>10mm = true → Satisfied → AllSatisfied → exit 0.
#[test]
fn purpose_value_guard_active_satisfied() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "vg=ActiveSat",
        &common::fixture_path("purpose_value_guard.ri"),
    ]);

    assert!(
        status.success(),
        "vg=ActiveSat (guard active, constraint satisfied) should exit 0\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' for B4 active-guard; \
         got: {stdout}"
    );
}

/// B5: Kleene-undef guard condition → guarded constraint is Indeterminate → exit 0.
///
/// `vg(s): where s.flag > 0mm { constraint s.val > 10mm }`.
/// UndefFlag: flag=auto → flag>0mm = Undef (Kleene) → U⇒U = U → Indeterminate.
///
/// This is the INDETERMINATE/exit-0 path — NOT Violated (contrast with B8 where
/// determined() is total and geometry-undef yields Bool(false) → Violated).
/// Kleene-undef is the ONLY path to Indeterminate in determinacy checks.
#[test]
fn purpose_value_guard_undef_flag_indeterminate() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "vg=UndefFlag",
        &common::fixture_path("purpose_value_guard.ri"),
    ]);

    assert!(
        status.success(),
        "vg=UndefFlag (Kleene-undef guard) should exit 0 (indeterminate, not violated)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.to_lowercase().contains("indeterminate"),
        "stdout should contain 'indeterminate' for B5 undef-guard; got: {stdout}"
    );
}

/// B3 PASS: scalar let binding is evaluated and drives a satisfied constraint.
///
/// `sl(s): let m = s.a - s.b; constraint m > 0mm`.
/// Pos: a=10mm, b=3mm → m=7mm → 7mm>0mm = true → Satisfied → exit 0.
#[test]
fn purpose_scalar_let_pos_passes() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "sl=Pos",
        &common::fixture_path("purpose_scalar_let.ri"),
    ]);

    assert!(
        status.success(),
        "sl=Pos (let m=a-b=7mm, constraint m>0mm) should exit 0\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' for B3 let-pos; \
         got: {stdout}"
    );
}

/// B3 FAIL: scalar let binding is evaluated and drives a violated constraint.
///
/// `sl(s): let m = s.a - s.b; constraint m > 0mm`.
/// Neg: a=3mm, b=10mm → m=-7mm → -7mm>0mm = false → Violated → exit non-zero.
#[test]
fn purpose_scalar_let_neg_violates() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "sl=Neg",
        &common::fixture_path("purpose_scalar_let.ri"),
    ]);

    assert!(
        !status.success(),
        "sl=Neg (let m=a-b=-7mm, constraint m>0mm) should exit non-zero\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain 'Some constraints violated.' for B3 let-neg; \
         got: {stdout}"
    );
}
