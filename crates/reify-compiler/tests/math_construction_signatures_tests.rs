//! End-to-end `reify check` cell-typing acceptance test for the math-linalg
//! **construction** builtins (task 4179, math-linalg α).
//!
//! Compiles a `structure def` whose `let` members are `vec` / `matrix` / `diag`
//! / `identity` calls and asserts (a) no Error-severity diagnostics and (b) each
//! cell types as the expected `Vector` / `Tensor` variant. This exercises the
//! `is_math_typed_fn` arm wired into `expr.rs::resolve_function_overload`'s
//! `NoUserFunctions` ladder (step-14): until that arm lands, each cell falls
//! through to the first-arg `List` / `Int` fallback (and would `TypeKindMismatch`
//! at eval against the real `Value::Vector` / `Value::Tensor`), so these tests
//! are RED.
//!
//! Modeled on `parametric_vector_point_resolution_tests.rs` — the
//! `compile_with_stdlib_helper` + `template.value_cells[..].cell_type` scaffold.

mod common;

use common::compile_with_stdlib_helper;
use reify_core::{Severity, Type};

/// A structure whose four `let` members are the construction-builtin calls.
/// Bare numeric literals type as `Type::dimensionless_scalar()` (dimensionless), so every inferred
/// quantity slot is `Type::dimensionless_scalar()`.
const CONSTRUCT_SOURCE: &str = r#"
structure def Constructed {
    let v = vec([1.0, 2.0, 3.0, 4.0])
    let m = matrix([[1.0, 2.0], [3.0, 4.0]])
    let d = diag([3.0, 5.0, 7.0])
    let i = identity(4)
}
"#;

/// Compile `CONSTRUCT_SOURCE`, assert no Error-severity diagnostics, then return
/// the resolved `cell_type` of member `member` on template `Constructed`.
fn construct_cell_type(member: &str) -> Type {
    let module = compile_with_stdlib_helper(CONSTRUCT_SOURCE);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "construction-builtin source must produce no Error-severity diagnostics; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Constructed")
        .expect("template `Constructed` not found in compiled module");

    template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("cell `{member}` not found on `Constructed`"))
        .cell_type
        .clone()
}

/// `vec([1.0, 2.0, 3.0, 4.0])` types as a 4-element dimensionless `Vector`.
#[test]
fn vec_cell_types_as_vector_n4_real() {
    assert_eq!(
        construct_cell_type("v"),
        Type::Vector {
            n: 4,
            quantity: Box::new(Type::dimensionless_scalar())
        },
        "vec([1,2,3,4]) cell must type as Vector{{n:4, quantity:Real}}"
    );
}

/// `matrix([[1.0, 2.0], [3.0, 4.0]])` types as a rank-2 `Tensor`
/// (n = column count = 2).
#[test]
fn matrix_cell_types_as_tensor_rank2_n2_real() {
    assert_eq!(
        construct_cell_type("m"),
        Type::Tensor {
            rank: 2,
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar())
        },
        "matrix([[1,2],[3,4]]) cell must type as Tensor{{rank:2, n:2, quantity:Real}}"
    );
}

/// `diag([3.0, 5.0, 7.0])` types as a rank-2 `Tensor` (n = 3).
#[test]
fn diag_cell_types_as_tensor_rank2_n3_real() {
    assert_eq!(
        construct_cell_type("d"),
        Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar())
        },
        "diag([3,5,7]) cell must type as Tensor{{rank:2, n:3, quantity:Real}}"
    );
}

/// `identity(4)` types as a 4×4 **dimensionless** rank-2 `Tensor`.
#[test]
fn identity_cell_types_as_dimensionless_tensor_rank2_n4() {
    assert_eq!(
        construct_cell_type("i"),
        Type::Tensor {
            rank: 2,
            n: 4,
            quantity: Box::new(Type::dimensionless_scalar())
        },
        "identity(4) cell must type as dimensionless Tensor{{rank:2, n:4, quantity:Real}}"
    );
}
