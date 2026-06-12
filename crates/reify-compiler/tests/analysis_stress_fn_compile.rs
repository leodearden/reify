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
//! Without the `is_analysis_typed_fn` arm in `expr.rs`, all three would drift
//! to the first-arg `Tensor<Pressure>` type — the `NoUserFunctions` fallback.
//!
//! Tests are RED until step-4 wires the arm into the `NoUserFunctions` ladder.

mod common;
use common::compile_with_stdlib_helper;
use reify_core::{DimensionVector, Severity, Type};

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
/// RED until step-4 wires `is_analysis_typed_fn` into `expr.rs`.
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
/// RED until step-4 wires `is_analysis_typed_fn` into `expr.rs`.
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
/// RED until step-4 wires `is_analysis_typed_fn` into `expr.rs`.
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
/// Pins that the newly-wired `is_analysis_typed_fn` arm in `expr.rs` fixes
/// `max_shear`'s compile type (it previously drifted to `Tensor<PRESSURE>`
/// via the `NoUserFunctions` fallback, mirroring the `von_mises` bug).
#[test]
fn max_shear_cell_type_is_scalar_pressure() {
    let module = compile_fixture();
    let ty = cell_type(&module, "ms");
    assert_eq!(
        ty,
        Type::Scalar { dimension: DimensionVector::PRESSURE },
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
