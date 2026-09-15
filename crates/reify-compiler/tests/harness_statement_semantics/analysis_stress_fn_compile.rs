//! Compile-time type-pinning tests for the FEA stress-analysis reduction
//! builtins: `von_mises`, `principal_stresses`, and `stress_invariants`.
//!
//! Pins that calling these builtins on a `matrix([[..Pa..]])` stress tensor
//! in `.ri` source produces the CORRECT compile-time cell types:
//!
//!   `von_mises(stress)`        → `Scalar<PRESSURE>`  (NOT Tensor<PRESSURE>)
//!   `principal_stresses(stress)` → `List<Scalar<PRESSURE>>` (NOT Tensor)
//!   `stress_invariants(stress)` → `StructureRef("StressInvariants")` (NOT Tensor)
//!
//! Without a name-keyed ladder arm, all three would drift to the first-arg
//! `Tensor<Pressure>` type — the `NoUserFunctions` fallback.
//!
//! Typing source, since registry α (task #6001): `expr.rs`'s `NoUserFunctions`
//! ladder consults `crates/reify-compiler/src/builtin_registry.rs`
//! `registry_result_type`, which answers off the `Family::Analysis` rows in
//! `crates/reify-builtins/src/registry.rs` (`VonMises`, `MaxShear`,
//! `PrincipalStresses`, `SafetyFactor`, `StressInvariants`). These tests were
//! first written RED for task #2884 step-4, against the pre-registry state;
//! that step's `is_analysis_typed_fn` arm is what α deleted and replaced with
//! the single registry arm, and they pass GREEN through it today.

use crate::common::compile_with_stdlib_helper;
use reify_compiler::{RequirementKind, stdlib_loader};
use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};

/// `.ri` fixture: a 3×3 uniaxial Pressure tensor via `matrix([[..Pa..]])`.
/// Uses SI `MPa` (6e6 Pa) literals — these are available via the prelude.
///
/// Also pins `max_shear` (→ Scalar<PRESSURE>) and `safety_factor`
/// (→ Real) — the two analysis builtins whose newly-wired compile typing
/// (task 2884 step-4) previously drifted to the first-arg Tensor type.
const ANALYSIS_TYPE_FIXTURE: &str = r#"
structure def AnalysisTypePins {
    let stress = matrix([[1.0e6Pa, 0.0Pa, 0.0Pa],
                         [0.0Pa, 0.0Pa, 0.0Pa],
                         [0.0Pa, 0.0Pa, 0.0Pa]])

    let vm   = von_mises(stress)
    let ps   = principal_stresses(stress)
    let inv  = stress_invariants(stress)
    let ms   = max_shear(stress)
    let sf   = safety_factor(stress, 250.0e6Pa)
}
"#;

/// Helper: compile the fixture and return the compiled module, asserting zero
/// Error-severity diagnostics.
fn compile_fixture() -> reify_compiler::CompiledModule {
    let module = compile_with_stdlib_helper(ANALYSIS_TYPE_FIXTURE);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "analysis_stress_fn_compile fixture must produce no Error diagnostics; got: {errs:?}"
    );
    module
}

/// Look up the `cell_type` of `member` on the `AnalysisTypePins` template.
fn cell_type(module: &reify_compiler::CompiledModule, member: &str) -> Type {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "AnalysisTypePins")
        .unwrap_or_else(|| panic!("AnalysisTypePins template not found"));
    template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| {
            panic!(
                "cell '{}' not found on AnalysisTypePins; available: {:?}",
                member,
                template
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        })
        .cell_type
        .clone()
}

/// `von_mises(stress)` on a `Tensor<PRESSURE>` must compile-type as
/// `Scalar<PRESSURE>` — NOT the first-arg `Tensor<PRESSURE>` drift.
///
/// Written RED for task #2884 step-4; GREEN today through the registry
/// arm described in the module header.
#[test]
fn von_mises_cell_type_is_scalar_pressure() {
    let module = compile_fixture();
    let ty = cell_type(&module, "vm");
    assert_eq!(
        ty,
        Type::Scalar {
            dimension: DimensionVector::PRESSURE
        },
        "von_mises(Tensor<Pressure>) must compile as Scalar<PRESSURE>, got {ty:?}"
    );
}

/// `principal_stresses(stress)` on a `Tensor<PRESSURE>` must compile-type as
/// `List(Scalar<PRESSURE>)` — NOT the first-arg `Tensor<PRESSURE>` drift.
///
/// Written RED for task #2884 step-4; GREEN today through the registry
/// arm described in the module header.
#[test]
fn principal_stresses_cell_type_is_list_scalar_pressure() {
    let module = compile_fixture();
    let ty = cell_type(&module, "ps");
    assert_eq!(
        ty,
        Type::List(Box::new(Type::Scalar {
            dimension: DimensionVector::PRESSURE
        })),
        "principal_stresses(Tensor<Pressure>) must compile as List(Scalar<PRESSURE>), got {ty:?}"
    );
}

/// `stress_invariants(stress)` on a `Tensor<PRESSURE>` must compile-type as
/// `StructureRef("StressInvariants")` — NOT the first-arg `Tensor<PRESSURE>` drift.
///
/// Written RED for task #2884 step-4; GREEN today through the registry
/// arm described in the module header.
#[test]
fn stress_invariants_cell_type_is_structure_ref() {
    let module = compile_fixture();
    let ty = cell_type(&module, "inv");
    assert_eq!(
        ty,
        Type::StructureRef("StressInvariants".to_string()),
        "stress_invariants(Tensor<Pressure>) must compile as StructureRef(\"StressInvariants\"), got {ty:?}"
    );
}

/// `max_shear(stress)` on a `Tensor<PRESSURE>` must compile-type as
/// `Scalar<PRESSURE>` — NOT the first-arg `Tensor<PRESSURE>` drift.
///
/// Pins that the registry's `MaxShear` row fixes `max_shear`'s compile type
/// (it previously drifted to `Tensor<PRESSURE>` via the `NoUserFunctions`
/// fallback, mirroring the `von_mises` bug). Task #2884 step-4 first fixed it
/// with an `is_analysis_typed_fn` arm; registry α replaced that arm.
#[test]
fn max_shear_cell_type_is_scalar_pressure() {
    let module = compile_fixture();
    let ty = cell_type(&module, "ms");
    assert_eq!(
        ty,
        Type::Scalar {
            dimension: DimensionVector::PRESSURE
        },
        "max_shear(Tensor<Pressure>) must compile as Scalar<PRESSURE>, got {ty:?}"
    );
}

/// `safety_factor(stress, yield)` must compile-type as `Type::dimensionless_scalar()`
/// (dimensionless yield/von_mises ratio) — NOT the first-arg `Tensor<PRESSURE>` drift.
///
/// The yield argument (`250.0e6Pa`) has `Scalar<PRESSURE>` type; the result
/// is dimensionless because pressure cancels.
#[test]
fn safety_factor_cell_type_is_real() {
    let module = compile_fixture();
    let ty = cell_type(&module, "sf");
    assert_eq!(
        ty,
        Type::dimensionless_scalar(),
        "safety_factor(Tensor<Pressure>, Scalar<Pressure>) must compile as Type::dimensionless_scalar(), got {ty:?}"
    );
}

// ─── AnalysisResult trait param-dimension pins (task 6165, RULING Q7 posture 2) ───

/// Return the `std/analysis` CompiledModule from the production stdlib loader.
/// Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, `OnceLock` caching.
fn analysis_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/analysis")
        .expect("stdlib should contain std/analysis module")
}

/// Return the declared `Type` of `AnalysisResult`'s required param `member`.
/// Panics if the trait, or the named `Param` member, is not found.
fn analysis_result_param_type(member: &str) -> Type {
    let module = analysis_module();
    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "AnalysisResult")
        .unwrap_or_else(|| panic!("expected 'AnalysisResult' trait in std/analysis module"));
    let req = trait_def
        .required_members
        .iter()
        .find(|r| r.name == member)
        .unwrap_or_else(|| panic!("AnalysisResult should have '{member}' member"));
    match &req.kind {
        RequirementKind::Param(ty) => ty.clone(),
        other => panic!("{member} should be Param, got {other:?}"),
    }
}

/// Assert that `AnalysisResult`'s `member` param is `Type::Scalar { dimension: expected }`.
fn assert_result_param_dimension(member: &str, expected: DimensionVector) {
    let ty = analysis_result_param_type(member);
    assert_eq!(
        ty,
        Type::Scalar {
            dimension: expected
        },
        "{member} should be Scalar{{{expected:?}}}, got {ty:?}"
    );
}

/// The five stress-bearing params on `AnalysisResult`, whose declared type
/// RULING Q7 posture 2 (task 6165) tightened from `Real` to `Stress`.
const STRESS_PARAMS: [&str; 5] = [
    "von_mises_stress",
    "principal_stress_1",
    "principal_stress_2",
    "principal_stress_3",
    "max_shear_stress",
];

/// RULING Q7 posture 2 (task 6165): pins that all five stress-bearing params
/// on `AnalysisResult` are `Scalar<PRESSURE>` (via the `Stress` alias) — NOT
/// the dimension-agnostic `Real` placeholder.
#[test]
fn analysis_result_stress_params_are_scalar_pressure() {
    for member in STRESS_PARAMS {
        assert_result_param_dimension(member, DimensionVector::PRESSURE);
    }
}

/// `safety_factor_value` STAYS `Real` (dimensionless) — the regression fence
/// on the "stays Real" half of RULING Q7 posture 2 (task 6165), so a future
/// agent does not retype it to `Stress` by symmetry with its five siblings.
#[test]
fn analysis_result_safety_factor_value_stays_dimensionless() {
    assert_result_param_dimension("safety_factor_value", DimensionVector::DIMENSIONLESS);
}

/// Access-path coherence (the same-value-two-types wart RULING Q7 closes):
/// `AnalysisResult.von_mises_stress`'s declared type must equal the
/// compile-time cell type of `von_mises(stress)` called directly — the
/// trait-side and builtin-side types must agree, since both describe the
/// same physical quantity.
#[test]
fn analysis_result_von_mises_stress_matches_builtin_cell_type() {
    let builtin_ty = cell_type(&compile_fixture(), "vm");
    let trait_ty = analysis_result_param_type("von_mises_stress");
    assert_eq!(
        trait_ty, builtin_ty,
        "AnalysisResult.von_mises_stress ({trait_ty:?}) must equal von_mises(stress)'s cell type ({builtin_ty:?})"
    );
}

/// See `analysis_result_von_mises_stress_matches_builtin_cell_type`; same
/// coherence claim for `max_shear_stress` / `max_shear(stress)`.
#[test]
fn analysis_result_max_shear_stress_matches_builtin_cell_type() {
    let builtin_ty = cell_type(&compile_fixture(), "ms");
    let trait_ty = analysis_result_param_type("max_shear_stress");
    assert_eq!(
        trait_ty, builtin_ty,
        "AnalysisResult.max_shear_stress ({trait_ty:?}) must equal max_shear(stress)'s cell type ({builtin_ty:?})"
    );
}

/// Compile `source` against the stdlib and assert it is rejected with a
/// `TypeMismatchForTraitMember` diagnostic naming `member`.
fn assert_trait_member_type_mismatch(source: &str, member: &str) {
    let module = compile_with_stdlib_helper(source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let matched = errs.iter().any(|d| {
        d.code == Some(DiagnosticCode::TypeMismatchForTraitMember)
            && d.message.contains("type mismatch for trait member")
            && d.message.contains(member)
    });
    assert!(
        matched,
        "expected a TypeMismatchForTraitMember diagnostic for '{member}' \
         against AnalysisResult's declared type; got: {errs:?}"
    );
}

/// The accept half of the user-visible accept/reject contract RULING Q7
/// posture 2 creates: a structure conforming to `AnalysisResult` with all
/// six params typed as the trait now requires — `Stress` for the five
/// stress-bearing params, `Real` for `safety_factor_value` — must compile
/// with zero Error diagnostics. (This also exercises the retype's constraint
/// half: `von_mises_stress >= 0` / `max_shear_stress >= 0`'s bare `0`
/// literal must still dimension-check now that its sibling operand is
/// `Scalar<PRESSURE>`.)
#[test]
fn analysis_result_conforming_structure_compiles_clean() {
    let source = r#"
structure def ConformingAnalysis : AnalysisResult {
    param von_mises_stress : Stress = 1.0Pa
    param principal_stress_1 : Stress = 1.0Pa
    param principal_stress_2 : Stress = 1.0Pa
    param principal_stress_3 : Stress = 1.0Pa
    param max_shear_stress : Stress = 1.0Pa
    param safety_factor_value : Real = 1.0
}
"#;
    let module = compile_with_stdlib_helper(source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "a structure conforming to AnalysisResult with Stress-typed stress \
         params should compile with no Error diagnostics; got: {errs:?}"
    );
}

/// The reject half: a structure whose five stress params are `Real` instead
/// of `Stress` must be rejected by trait conformance. This is the
/// user-visible contract the retype creates — without this test, a future
/// change that made a dimensionless `Real` implicitly convertible to a
/// dimensioned scalar would silently re-open the same-value-two-types wart
/// RULING Q7 closes while every other test in this file stayed green.
#[test]
fn analysis_result_real_typed_stress_param_is_rejected() {
    let source = r#"
structure def MisTypedAnalysis : AnalysisResult {
    param von_mises_stress : Real = 1.0
    param principal_stress_1 : Real = 1.0
    param principal_stress_2 : Real = 1.0
    param principal_stress_3 : Real = 1.0
    param max_shear_stress : Real = 1.0
    param safety_factor_value : Real = 1.0
}
"#;
    for member in STRESS_PARAMS {
        assert_trait_member_type_mismatch(source, member);
    }
}

/// The other reject half, fencing the "stays Real" clause behaviourally
/// rather than only through the declared-type pin above: an otherwise
/// conforming structure that declares `safety_factor_value : Stress` must be
/// rejected too. Without it, a compiler change that started accepting a
/// dimensioned scalar where the trait requires a dimensionless one would
/// leave every other test in this file green.
#[test]
fn analysis_result_dimensioned_safety_factor_value_is_rejected() {
    let source = r#"
structure def DimensionedSafetyFactor : AnalysisResult {
    param von_mises_stress : Stress = 1.0Pa
    param principal_stress_1 : Stress = 1.0Pa
    param principal_stress_2 : Stress = 1.0Pa
    param principal_stress_3 : Stress = 1.0Pa
    param max_shear_stress : Stress = 1.0Pa
    param safety_factor_value : Stress = 1.0Pa
}
"#;
    assert_trait_member_type_mismatch(source, "safety_factor_value");
}
