//! Engine coexistence / composition gate — PRD §5 BT1–BT9 collection (task-4200 δ).
//!
//! # Purpose
//!
//! This file is the integration gate for the entire Determinacy-intrinsics-completion
//! cluster (PRD `docs/prds/v0_6/determinacy-intrinsics-completion.md`).  It tests
//! that the **two §12 mechanisms delivered by tasks α and γ compose cleanly in ONE
//! compiled module** — the seam no single dependency task crossed.
//!
//! - **α (task-4197)**: compiler-sugar intrinsics `AllParamsDetermined` /
//!   `AllGeometryDetermined` (purpose-body-only, desugar → `forall` Quantifier).
//! - **γ (task-4199)**: `RepresentationWithin` assertion (post-realization,
//!   three-valued Satisfied / Violated / Indeterminate via `achieved_repr_tol` map).
//!
//! Specifically this file verifies:
//!
//! 1. A module containing **both** a purpose using `AllParamsDetermined` and a
//!    structure carrying `RepresentationWithin` compiles without `Severity::Error`
//!    (the two §12 recognizers do not interfere with each other).
//! 2. The intrinsic desugars to `CompiledExprKind::Quantifier`; after
//!    `activate_purpose` + `check_constraints_with_values` the purpose constraint
//!    is `Satisfaction::Satisfied` for a fully-determined structure.
//! 3. `RepresentationWithin` dispatch, driven deterministically via
//!    `set_achieved_repr_tol_for_test` (no OCCT needed):
//!    - injected value **above** bound → `Violated`
//!    - injected value **below** bound → `Satisfied`
//!    - empty map (no entry) → `Indeterminate` (C1 graceful degradation)
//!
//! # PRD §5 BT1–BT9 canonical test homes
//!
//! The table below is the authoritative BT1–BT9 cross-reference.  Each row names
//! the test file that OWNS that boundary test; this file is the *collected view*.
//!
//! | BT   | Description                                          | Owner file / function                           |
//! |------|------------------------------------------------------|-------------------------------------------------|
//! | BT1  | golden-equivalence (A1)                              | `reify-compiler/tests/purpose_compile_tests.rs` fn `all_params_determined_desugars_to_same_compiled_expr_as_hand_written_forall` |
//! | BT2  | AllGeometryDetermined (A2)                           | `reify-compiler/tests/purpose_compile_tests.rs` fn `all_geometry_determined_desugars_to_same_compiled_expr_as_hand_written_forall` |
//! | BT3  | scope / arg diagnostics (A3)                         | `reify-compiler/tests/purpose_compile_tests.rs` fns `all_params_determined_outside_purpose_body_emits_scope_diagnostic`, `all_geometry_determined_outside_purpose_body_emits_scope_diagnostic`, `all_params_determined_zero_args_emits_arg_diagnostic` |
//! | BT4  | intrinsic CLI Satisfied/Violated (A4)                | `reify-cli/tests/cli_determinacy_intrinsics.rs` fns `check_design_review_satisfied_for_determined_bracket`, `check_design_review_violated_for_draft_bracket` |
//! | BT5  | deviation monotonicity (B1/B2)                       | `reify-eval/tests/achieved_repr_tol.rs`         |
//! | BT6  | RW Violated + non-zero exit (C3)                     | `reify-eval/tests/representation_within_assertion.rs` fn `bt6_coarse_sphere_tight_bound_yields_violated` + `reify-cli/tests/cli_representation_within.rs` fn `check_representation_within_violated_under_occt` |
//! | BT7  | RW Satisfied + zero exit (C3) — CLI consumer boundary | `reify-cli/tests/cli_determinacy_gate.rs` fn `check_representation_within_satisfied_exits_zero` |
//! | BT8  | RW Indeterminate (C1)                                | `reify-eval/tests/representation_within_assertion.rs` fn `bt8_no_tessellation_yields_indeterminate` + `reify-cli/tests/cli_representation_within.rs` fn `check_representation_within_violated_under_occt` (stub branch) |
//! | BT9  | budget regression (C2)                               | `reify-eval/tests/representation_within_assertion.rs` fn `c2_extract_output_tolerance_bound_still_returns_declared_bound` + `reify-eval/tests/tolerance_scope.rs` + `reify-eval/tests/tolerance_combine.rs` |
//!
//! This gate (δ) adds the α↔γ COMPOSITION seam (no dep task crossed) and closes the
//! BT7 CLI gap (no CLI Satisfied/zero-exit test existed in γ).  BT1–BT6/BT8/BT9 stay
//! owned by their producing tasks and are exercised daily via those files.

use reify_core::Severity;
use reify_ir::{CompiledExprKind, Satisfaction};
use reify_test_support::{make_simple_engine, parse_and_compile};
use std::collections::BTreeMap;

// ── Shared DSL fixture ────────────────────────────────────────────────────────

/// A module that carries BOTH §12 mechanisms in a single compilation unit.
///
/// - `MyGeom`: fully-determined structure (`param x : Real = 1.0`), used as
///   the subject for both the purpose intrinsic and the RepresentationWithin
///   structural constraint.
/// - `design_review`: purpose using `AllParamsDetermined(subject)` over `Structure`
///   (the α intrinsic, desugars to `forall __p in subject.params: determined(__p)`).
/// - `Checker`: structure with `param subject : MyGeom` and two constraints:
///   - `RepresentationWithin(subject, 1mm)` (γ post-realization assertion, index 0)
///   - `w > 0.0` (ordinary always-Satisfied predicate, index 1)
///
/// The two recognizers (`AllParamsDetermined` desugar in reify-compiler, and
/// `RepresentationWithin` dispatch interception in reify-eval) must NOT interfere —
/// verified by the no-error compilation assertion in `composition_compiles_without_errors`.
const INTEGRATION_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

purpose design_review(subject : Structure) {
    constraint AllParamsDetermined(subject)
}

structure Checker {
    param subject : MyGeom
    param w : Real = 5.0
    constraint RepresentationWithin(subject, 1mm)
    constraint w > 0.0
}
"#;

// ── (a) Compilation gate: no recognizer interference ─────────────────────────

/// Verifies that a module carrying BOTH an `AllParamsDetermined`-using purpose
/// (α) and a `RepresentationWithin`-bearing structure (γ) compiles without any
/// `Severity::Error` diagnostic.
///
/// This is the primary NEW contribution of δ: the α↔γ composition seam.
/// No dependency task compiled both mechanisms in the same module — α tests
/// only intrinsics, γ tests only RepresentationWithin.
///
/// Also asserts that the intrinsic constraint in the purpose desugars to
/// `CompiledExprKind::Quantifier { .. }` (proves the desugar rode the existing
/// reflective path, not a new eval primitive), and that Checker has exactly 2
/// compiled constraints.
#[test]
fn composition_compiles_without_errors() {
    let compiled = parse_and_compile(INTEGRATION_SOURCE);

    // (a1) No Severity::Error diagnostics — the two §12 recognizers do not interfere.
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "α+γ composition module produced unexpected Severity::Error diagnostics: {:?}",
        compiled.diagnostics
    );

    // (a2) Exactly one compiled purpose: design_review.
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "INTEGRATION_SOURCE must produce exactly one compiled purpose (design_review); \
         got {}",
        compiled.compiled_purposes.len()
    );

    // (a3) The intrinsic in design_review desugars to Quantifier (α desugar path).
    let purpose_constraint_expr = &compiled.compiled_purposes[0].constraints[0].expr;
    assert!(
        matches!(
            purpose_constraint_expr.kind,
            CompiledExprKind::Quantifier { .. }
        ),
        "AllParamsDetermined must desugar to a Quantifier; got {:?}",
        purpose_constraint_expr.kind
    );

    // (a4) Checker has exactly 2 compiled constraints (RepresentationWithin + w>0).
    let checker_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Checker")
        .expect("INTEGRATION_SOURCE must produce a 'Checker' template");
    assert_eq!(
        checker_template.constraints.len(),
        2,
        "Checker must have exactly 2 constraints (RepresentationWithin + w>0); \
         got {}",
        checker_template.constraints.len()
    );
}

// ── (b) Intrinsic activation: AllParamsDetermined → Satisfied ────────────────

/// Verifies that `AllParamsDetermined(subject)` activated against `MyGeom`
/// (a fully-determined structure) yields `Satisfaction::Satisfied`.
///
/// Engine path: `eval` → `activate_purpose("design_review", "MyGeom")` →
/// `check_constraints_with_values` → purpose-injected constraint result.
///
/// This is the α-side of the composition gate, proving the intrinsic activation
/// path continues to work in a module that also carries a RepresentationWithin
/// constraint.
#[test]
fn intrinsic_satisfied_for_fully_determined_structure() {
    let compiled = parse_and_compile(INTEGRATION_SOURCE);
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    engine.activate_purpose("design_review", "MyGeom");

    let (constraint_results, _) = engine
        .check_constraints_with_values(&eval_result.values)
        .expect("check_constraints_with_values must not return an error");

    let purpose_result = constraint_results
        .iter()
        .find(|e| e.id.entity.starts_with("purpose:design_review@MyGeom"))
        .unwrap_or_else(|| {
            panic!(
                "expected a purpose-injected constraint with entity prefix \
                 'purpose:design_review@MyGeom'; found ids: {:?}",
                constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        purpose_result.satisfaction,
        Satisfaction::Satisfied,
        "(b) MyGeom.x has a default (= 1.0) so AllParamsDetermined must be Satisfied \
         in a module that also carries RepresentationWithin (α↔γ composition).",
    );
}

// ── (c1) RepresentationWithin: injected > bound → Violated ───────────────────

/// Verifies that `RepresentationWithin(subject, 1mm)` in `Checker` is `Violated`
/// when the `achieved_repr_tol` map is injected with a value above the 1mm bound.
///
/// Uses `set_achieved_repr_tol_for_test` (γ test-instrumentation seam) to drive
/// the assertion deterministically WITHOUT OCCT.  Key is "MyGeom#realization[0]"
/// (the type-name scan resolves `param subject : MyGeom` → this key).
///
/// This is the γ-side of the composition gate, proving the dispatch interception
/// works correctly in a module that also has an AllParamsDetermined purpose.
#[test]
fn representation_within_violated_when_injected_over_bound() {
    let compiled = parse_and_compile(INTEGRATION_SOURCE);
    let mut engine = make_simple_engine();

    // 5e-3 m = 5 mm > 1 mm (1e-3 m) bound → Violated.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 5e-3_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "(c1) achieved 5e-3 m > bound 1mm (1e-3 m) → Violated (α↔γ composition)"
    );

    // The ordinary constraint (w > 0.0, index 1) must still be Satisfied.
    let ord_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 1)
        .expect("must have Checker#constraint[1] (w > 0.0)");
    assert_eq!(
        ord_entry.satisfaction,
        Satisfaction::Satisfied,
        "(c1) w=5.0 > 0.0 must remain Satisfied — RepresentationWithin interception \
         must not disturb the ordinary constraint result"
    );
}

// ── (c2) RepresentationWithin: injected < bound → Satisfied ──────────────────

/// Verifies that `RepresentationWithin(subject, 1mm)` is `Satisfied` when the
/// injected value is below the 1mm bound.
///
/// Mirrors the engine BT7 numeric premise (fine sphere: achieved ≪ 1mm) in the
/// composition context (same module that also has the purpose intrinsic).
#[test]
fn representation_within_satisfied_when_injected_under_bound() {
    let compiled = parse_and_compile(INTEGRATION_SOURCE);
    let mut engine = make_simple_engine();

    // 1e-9 m ≪ 1mm (1e-3 m) bound → Satisfied.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 1e-9_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "(c2) achieved 1e-9 m < bound 1mm (1e-3 m) → Satisfied (α↔γ composition)"
    );
}

// ── (c3) RepresentationWithin: empty map → Indeterminate ─────────────────────

/// Verifies that `RepresentationWithin(subject, 1mm)` is `Indeterminate` when
/// the `achieved_repr_tol` map is empty (no entry for "MyGeom#realization[0]").
///
/// C1 invariant: absent key ⇒ realization not run ⇒ never a false Violated.
/// This is the graceful-degradation guarantee: stub builds (no OCCT) must not
/// produce spurious Violated outcomes.
#[test]
fn representation_within_indeterminate_when_map_empty() {
    let compiled = parse_and_compile(INTEGRATION_SOURCE);
    let mut engine = make_simple_engine();
    // Do NOT inject any map — map stays empty → Indeterminate.

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "(c3) empty achieved_repr_tol map → Indeterminate (C1: absent key ⇒ \
         realization not run ⇒ never a false Violated) (α↔γ composition)"
    );
}
