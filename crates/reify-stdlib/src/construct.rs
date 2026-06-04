//! Pure list→value **construction** primitives (math-linalg α, task 4179).
//!
//! Exposes [`eval_construct`], the `eval_builtin` dispatch arm for the four
//! N-general constructors that build vectors / matrices from `.ri` source:
//!
//! - `vec(list)`        → [`Value::Vector`] (N = list length)
//! - `matrix(rows)`     → rank-2 nested [`Value::Tensor`] (RANK-2 ONLY)
//! - `diag(list)`       → N×N nested [`Value::Tensor`] (list on the diagonal)
//! - `identity(n: Int)` → N×N dimensionless nested [`Value::Tensor`]
//!
//! These exist so N>3 matrices/vectors are buildable from source — today all
//! four forms parse but evaluate to `undef`. They are pure structural
//! reshaping: NO linear algebra (that is task β), NO grammar work.
//!
//! Cells are built with [`Value::from_real_scalar`] (Real if dimensionless,
//! else Scalar) and sanitized via [`crate::helpers::sanitize_value`]. Any
//! shape / dimension / numeric violation collapses to [`Value::Undef`] with no
//! new diagnostic code.
//!
//! List extraction delegates to the canonical shared cores:
//! - rank-1 (`vec`, `diag`): [`crate::helpers::list_components_f64`], which
//!   delegates to the private `uniform_components_f64` core shared with
//!   `tensor_components_f64`.
//! - rank-2 (`matrix`): [`crate::matrix::list_matrix_components_f64`], which
//!   delegates to the private `rank2_components` core shared with
//!   `matrix_components_f64`.

use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::sanitize_value;

/// Evaluate a construction builtin (`vec` / `matrix` / `diag` / `identity`).
///
/// Returns `Some(value)` when `name` is one of the four constructors (the
/// value is `Value::Undef` on malformed input), or `None` when `name` is not a
/// construction builtin (so `eval_builtin` continues its dispatch chain).
pub(crate) fn eval_construct(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "vec" => eval_vec(args),
        "matrix" => eval_matrix(args),
        "diag" => eval_diag(args),
        "identity" => eval_identity(args),
        _ => return None,
    })
}

/// `vec(list)` → [`Value::Vector`] with one cell per list element.
///
/// Extracts a uniform `(values, dim)` from the single `Value::List` argument
/// and rebuilds each cell via [`Value::from_real_scalar`] (Real if
/// dimensionless, else Scalar) wrapped in [`sanitize_value`]. Wrong arity, a
/// non-`List` arg, or a malformed list (empty / mixed-dimension / non-numeric)
/// collapses to [`Value::Undef`].
fn eval_vec(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let (vals, dim) = match list_components_f64(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    let cells = vals
        .into_iter()
        .map(|x| sanitize_value(Value::from_real_scalar(x, dim)))
        .collect();
    Value::Vector(cells)
}

/// `matrix(rows)` → rank-2 nested [`Value::Tensor`] (RANK-2 ONLY).
///
/// Validates a depth-2 `Value::List` of `Value::List` (non-empty outer, every
/// row a non-empty `List`, uniform column count, uniform dimension, all
/// numeric) and builds the nested `Tensor`. Wrong arity, a non-`List` arg, or
/// any shape / dimension violation (ragged / empty / non-list row / mixed
/// dimension / non-numeric) collapses to [`Value::Undef`].
///
/// Extraction delegates to [`crate::matrix::list_matrix_components_f64`], the
/// canonical List-accepting entry point that shares the `rank2_components` core
/// with `matrix_components_f64`.
fn eval_matrix(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let (nrows, ncols, data, dim) = match crate::matrix::list_matrix_components_f64(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    build_tensor_rank2(nrows, ncols, &data, dim)
}

/// `diag(list)` → N×N nested [`Value::Tensor`] with the list on the diagonal
/// and dimension-sharing zeros off-diagonal.
///
/// Extracts a uniform `(values, dim)` from the single `Value::List` argument,
/// then assembles a dense row-major N×N array with element `i` at `(i, i)` and
/// `0.0` elsewhere. `build_tensor_rank2` turns the off-diagonal `0.0`s into
/// `from_real_scalar(0.0, dim)` cells, so they share the element dimension
/// (Real when dimensionless, dimensioned `Scalar` otherwise). Wrong arity, a
/// non-`List` arg, or a malformed list (empty / mixed-dimension / non-numeric)
/// collapses to [`Value::Undef`].
fn eval_diag(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let (vals, dim) = match list_components_f64(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    let n = vals.len();
    let mut data = vec![0.0_f64; n * n];
    for (i, &v) in vals.iter().enumerate() {
        data[i * n + i] = v;
    }
    build_tensor_rank2(n, n, &data, dim)
}

/// `identity(n: Int)` → N×N **dimensionless** identity [`Value::Tensor`]
/// (`Real(1.0)` on the diagonal, `Real(0.0)` off).
///
/// Accepts exactly one `Value::Int(n)` with `n >= 1`. The slice pattern
/// rejects wrong arity and any non-`Int` argument (Real / String), and the
/// guard rejects `n <= 0` — all collapse to [`Value::Undef`]. The quantity is
/// always `DIMENSIONLESS`, so every cell is a `Real`.
fn eval_identity(args: &[Value]) -> Value {
    let n = match args {
        [Value::Int(n)] if *n >= 1 => *n as usize,
        _ => return Value::Undef,
    };
    let mut data = vec![0.0_f64; n * n];
    for i in 0..n {
        data[i * n + i] = 1.0;
    }
    build_tensor_rank2(n, n, &data, DimensionVector::DIMENSIONLESS)
}

/// Extract uniform numeric components from a `Value::List` into
/// `(values, element_dim)`.
///
/// Returns `None` (→ caller yields `Undef`) when the list is empty, mixes
/// dimensions, or contains a non-numeric element. This is the `Value::List`
/// analogue of `helpers::tensor_components_f64`, which deliberately rejects
/// `Value::List` (it accepts only Vector/Tensor/Point) — the construction
/// builtins receive their argument as an already-evaluated list literal, so a
/// List-accepting extractor is required. Kept local to construct.rs rather than
/// widening the shared `helpers.rs` surface (β/γ own that file).
fn list_components_f64(v: &Value) -> Option<(Vec<f64>, DimensionVector)> {
    let items = match v {
        Value::List(items) if !items.is_empty() => items,
        _ => return None,
    };
    let first_dim = items[0].dimension();
    let mut vals = Vec::with_capacity(items.len());
    for item in items {
        if item.dimension() != first_dim {
            return None; // mixed dimensions
        }
        match item.as_f64() {
            Some(x) => vals.push(x),
            None => return None, // non-numeric component
        }
    }
    Some((vals, first_dim))
}


/// Build a rank-2 nested [`Value::Tensor`] (rows of `Tensor` cells) from flat
/// row-major `data`. Each cell is built via [`Value::from_real_scalar`] (Real
/// if dimensionless, else Scalar) and wrapped in [`sanitize_value`].
///
/// Shared by `matrix` (step-4), `diag` (step-6) and `identity` (step-8): each
/// produces its own row-major `data` (dense for `matrix`; diagonal-plus-zeros
/// for `diag` / `identity`) and delegates assembly here.
fn build_tensor_rank2(nrows: usize, ncols: usize, data: &[f64], dim: DimensionVector) -> Value {
    let rows = (0..nrows)
        .map(|i| {
            let cells = (0..ncols)
                .map(|j| sanitize_value(Value::from_real_scalar(data[i * ncols + j], dim)))
                .collect();
            Value::Tensor(cells)
        })
        .collect();
    Value::Tensor(rows)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    /// Build a `Scalar` with the given dimension (test fixture).
    fn scalar(v: f64, dim: DimensionVector) -> Value {
        Value::Scalar {
            si_value: v,
            dimension: dim,
        }
    }

    // ── `vec(list)` → Value::Vector (step-1 RED / step-2 GREEN) ──────────────

    /// (a) A list of dimensionless `Real`s builds a `Vector` of `Real` cells.
    #[test]
    fn vec_of_reals_builds_vector() {
        let out = eval_builtin(
            "vec",
            &[Value::List(vec![
                Value::Real(1.0),
                Value::Real(2.0),
                Value::Real(3.0),
            ])],
        );
        assert_eq!(
            out,
            Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]),
            "vec([1,2,3]) should build a 3-element Vector of Reals"
        );
    }

    /// (b) A dimensioned list preserves the element dimension as `Scalar` cells.
    #[test]
    fn vec_of_length_scalars_preserves_dimension() {
        let out = eval_builtin(
            "vec",
            &[Value::List(vec![
                scalar(1.0, DimensionVector::LENGTH),
                scalar(2.0, DimensionVector::LENGTH),
            ])],
        );
        assert_eq!(
            out,
            Value::Vector(vec![
                scalar(1.0, DimensionVector::LENGTH),
                scalar(2.0, DimensionVector::LENGTH),
            ]),
            "vec of LENGTH Scalars should build a Vector of LENGTH Scalars"
        );
    }

    /// (c) Integer elements coerce to dimensionless `Real` cells.
    #[test]
    fn vec_of_ints_builds_dimensionless_vector() {
        let out = eval_builtin("vec", &[Value::List(vec![Value::Int(1), Value::Int(2)])]);
        assert_eq!(
            out,
            Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]),
            "vec([Int 1, Int 2]) should build a Vector of dimensionless Reals"
        );
    }

    /// (d) An empty list is malformed → `Undef`.
    #[test]
    fn vec_empty_list_is_undef() {
        assert_eq!(
            eval_builtin("vec", &[Value::List(vec![])]),
            Value::Undef,
            "vec([]) should be Undef"
        );
    }

    /// (e) A mixed-dimension list is malformed → `Undef`.
    #[test]
    fn vec_mixed_dimension_is_undef() {
        let out = eval_builtin(
            "vec",
            &[Value::List(vec![
                Value::Real(1.0),
                scalar(2.0, DimensionVector::LENGTH),
            ])],
        );
        assert_eq!(out, Value::Undef, "vec of mixed dimensions should be Undef");
    }

    /// (f) A non-numeric element (String) is malformed → `Undef`.
    #[test]
    fn vec_non_numeric_element_is_undef() {
        let out = eval_builtin(
            "vec",
            &[Value::List(vec![
                Value::Real(1.0),
                Value::String("x".to_string()),
            ])],
        );
        assert_eq!(
            out,
            Value::Undef,
            "vec containing a String element should be Undef"
        );
    }

    /// (g) Wrong argument count, or a non-`List` argument → `Undef`.
    #[test]
    fn vec_wrong_arity_or_non_list_is_undef() {
        assert_eq!(
            eval_builtin("vec", &[]),
            Value::Undef,
            "vec() with no args should be Undef"
        );
        assert_eq!(
            eval_builtin(
                "vec",
                &[
                    Value::List(vec![Value::Real(1.0)]),
                    Value::List(vec![Value::Real(2.0)]),
                ],
            ),
            Value::Undef,
            "vec(.., ..) with two args should be Undef"
        );
        assert_eq!(
            eval_builtin("vec", &[Value::Real(1.0)]),
            Value::Undef,
            "vec(Real) with a non-List arg should be Undef"
        );
    }

    // ── `matrix(rows)` → rank-2 Value::Tensor (step-3 RED / step-4 GREEN) ─────

    /// Build a `Value::List` row of dimensionless `Real`s (test fixture).
    fn list_row(vals: &[f64]) -> Value {
        Value::List(vals.iter().map(|&v| Value::Real(v)).collect())
    }

    /// Build a `Value::Tensor` row of dimensionless `Real`s (expected cell).
    fn tensor_row(vals: &[f64]) -> Value {
        Value::Tensor(vals.iter().map(|&v| Value::Real(v)).collect())
    }

    /// (a) A 2×2 list-of-lists builds a rank-2 nested `Tensor`.
    #[test]
    fn matrix_square_builds_rank2_tensor() {
        let input = Value::List(vec![list_row(&[1.0, 2.0]), list_row(&[3.0, 4.0])]);
        let out = eval_builtin("matrix", &[input]);
        assert_eq!(
            out,
            Value::Tensor(vec![tensor_row(&[1.0, 2.0]), tensor_row(&[3.0, 4.0])]),
            "matrix([[1,2],[3,4]]) should build a 2×2 nested Tensor"
        );
    }

    /// (b) A non-square 2×3 list-of-lists builds a 2×3 nested `Tensor`.
    #[test]
    fn matrix_non_square_builds_2x3_tensor() {
        let input = Value::List(vec![list_row(&[1.0, 2.0, 3.0]), list_row(&[4.0, 5.0, 6.0])]);
        let out = eval_builtin("matrix", &[input]);
        assert_eq!(
            out,
            Value::Tensor(vec![
                tensor_row(&[1.0, 2.0, 3.0]),
                tensor_row(&[4.0, 5.0, 6.0]),
            ]),
            "matrix of a 2×3 list-of-lists should build a 2×3 nested Tensor"
        );
    }

    /// (c) Dimensioned elements build `Scalar` cells with the shared dimension.
    #[test]
    fn matrix_dimensioned_elements_build_scalar_cells() {
        let m = |v: f64| scalar(v, DimensionVector::LENGTH);
        let input = Value::List(vec![
            Value::List(vec![m(1.0), m(2.0)]),
            Value::List(vec![m(3.0), m(4.0)]),
        ]);
        let out = eval_builtin("matrix", &[input]);
        assert_eq!(
            out,
            Value::Tensor(vec![
                Value::Tensor(vec![m(1.0), m(2.0)]),
                Value::Tensor(vec![m(3.0), m(4.0)]),
            ]),
            "matrix of LENGTH Scalars should build a nested Tensor of LENGTH Scalars"
        );
    }

    /// (d) Ragged rows are malformed → `Undef`.
    #[test]
    fn matrix_ragged_rows_is_undef() {
        let input = Value::List(vec![list_row(&[1.0, 2.0]), list_row(&[3.0])]);
        assert_eq!(
            eval_builtin("matrix", &[input]),
            Value::Undef,
            "matrix([[1,2],[3]]) (ragged) should be Undef"
        );
    }

    /// (e) An empty outer list or an empty row is malformed → `Undef`.
    #[test]
    fn matrix_empty_outer_or_empty_row_is_undef() {
        assert_eq!(
            eval_builtin("matrix", &[Value::List(vec![])]),
            Value::Undef,
            "matrix([]) (empty outer) should be Undef"
        );
        assert_eq!(
            eval_builtin("matrix", &[Value::List(vec![Value::List(vec![])])]),
            Value::Undef,
            "matrix([[]]) (empty row) should be Undef"
        );
    }

    /// (f) A non-list row is malformed → `Undef`.
    #[test]
    fn matrix_non_list_row_is_undef() {
        let input = Value::List(vec![list_row(&[1.0]), Value::Real(2.0)]);
        assert_eq!(
            eval_builtin("matrix", &[input]),
            Value::Undef,
            "matrix([[1],2]) (non-list row) should be Undef"
        );
    }

    /// (g) A mixed dimension across cells is malformed → `Undef`.
    #[test]
    fn matrix_mixed_dimension_is_undef() {
        let input = Value::List(vec![
            Value::List(vec![Value::Real(1.0), scalar(2.0, DimensionVector::LENGTH)]),
            Value::List(vec![Value::Real(3.0), Value::Real(4.0)]),
        ]);
        assert_eq!(
            eval_builtin("matrix", &[input]),
            Value::Undef,
            "matrix mixing dimensionless and LENGTH cells should be Undef"
        );
    }

    // ── `diag(list)` → N×N Value::Tensor (step-5 RED / step-6 GREEN) ──────────

    /// (a) A dimensionless list lands on the diagonal; off-diagonal is `0.0`.
    #[test]
    fn diag_dimensionless_builds_diagonal_tensor() {
        let out = eval_builtin("diag", &[list_row(&[3.0, 5.0, 7.0])]);
        assert_eq!(
            out,
            Value::Tensor(vec![
                tensor_row(&[3.0, 0.0, 0.0]),
                tensor_row(&[0.0, 5.0, 0.0]),
                tensor_row(&[0.0, 0.0, 7.0]),
            ]),
            "diag([3,5,7]) should be a 3×3 Tensor with 3,5,7 on the diagonal"
        );
    }

    /// (b) Off-diagonal zeros share the element dimension (dimensioned zeros).
    #[test]
    fn diag_dimensioned_off_diagonal_zeros_share_dimension() {
        let m = |v: f64| scalar(v, DimensionVector::LENGTH);
        let out = eval_builtin("diag", &[Value::List(vec![m(3.0), m(5.0)])]);
        assert_eq!(
            out,
            Value::Tensor(vec![
                Value::Tensor(vec![m(3.0), m(0.0)]),
                Value::Tensor(vec![m(0.0), m(5.0)]),
            ]),
            "diag of LENGTH Scalars should place LENGTH-dimensioned zeros off-diagonal"
        );
    }

    /// (c) A single-element list builds a 1×1 Tensor.
    #[test]
    fn diag_single_element_builds_1x1() {
        let out = eval_builtin("diag", &[list_row(&[4.0])]);
        assert_eq!(
            out,
            Value::Tensor(vec![tensor_row(&[4.0])]),
            "diag([4]) should be a 1×1 Tensor [[4]]"
        );
    }

    /// (d) An empty list is malformed → `Undef`.
    #[test]
    fn diag_empty_list_is_undef() {
        assert_eq!(
            eval_builtin("diag", &[Value::List(vec![])]),
            Value::Undef,
            "diag([]) should be Undef"
        );
    }

    /// (e) A mixed-dimension or non-numeric list is malformed → `Undef`.
    #[test]
    fn diag_mixed_dimension_or_non_numeric_is_undef() {
        let mixed = Value::List(vec![Value::Real(1.0), scalar(2.0, DimensionVector::LENGTH)]);
        assert_eq!(
            eval_builtin("diag", &[mixed]),
            Value::Undef,
            "diag of mixed dimensions should be Undef"
        );
        let non_numeric = Value::List(vec![Value::Real(1.0), Value::String("x".to_string())]);
        assert_eq!(
            eval_builtin("diag", &[non_numeric]),
            Value::Undef,
            "diag containing a String should be Undef"
        );
    }

    // ── `identity(n: Int)` → N×N dimensionless Tensor (step-7 RED / step-8 GREEN)

    /// Build the expected N×N dimensionless identity `Tensor` (`Real(1.0)` on
    /// the diagonal, `Real(0.0)` elsewhere).
    fn expected_identity(n: usize) -> Value {
        Value::Tensor(
            (0..n)
                .map(|i| {
                    Value::Tensor(
                        (0..n)
                            .map(|j| Value::Real(if i == j { 1.0 } else { 0.0 }))
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    /// (a) `identity(4)` is a 4×4 dimensionless Tensor (diagonal 1.0, else 0.0).
    #[test]
    fn identity_4_builds_dimensionless_identity() {
        let out = eval_builtin("identity", &[Value::Int(4)]);
        assert_eq!(
            out,
            expected_identity(4),
            "identity(4) should be a 4×4 dimensionless identity Tensor"
        );
    }

    /// (b) `identity(1)` is the 1×1 Tensor `[[1.0]]`.
    #[test]
    fn identity_1_builds_1x1() {
        let out = eval_builtin("identity", &[Value::Int(1)]);
        assert_eq!(
            out,
            expected_identity(1),
            "identity(1) should be the 1×1 Tensor [[1.0]]"
        );
    }

    /// (c) `identity(0)` and a negative argument are malformed → `Undef`.
    #[test]
    fn identity_zero_or_negative_is_undef() {
        assert_eq!(
            eval_builtin("identity", &[Value::Int(0)]),
            Value::Undef,
            "identity(0) should be Undef"
        );
        assert_eq!(
            eval_builtin("identity", &[Value::Int(-3)]),
            Value::Undef,
            "identity(-3) should be Undef"
        );
    }

    /// (d) A non-`Int` argument (Real / String) is malformed → `Undef`.
    #[test]
    fn identity_non_int_arg_is_undef() {
        assert_eq!(
            eval_builtin("identity", &[Value::Real(4.0)]),
            Value::Undef,
            "identity(Real 4.0) should be Undef — only Int is accepted"
        );
        assert_eq!(
            eval_builtin("identity", &[Value::String("4".to_string())]),
            Value::Undef,
            "identity(String) should be Undef"
        );
    }

    /// (e) Wrong argument count → `Undef`.
    #[test]
    fn identity_wrong_arity_is_undef() {
        assert_eq!(
            eval_builtin("identity", &[]),
            Value::Undef,
            "identity() with no args should be Undef"
        );
        assert_eq!(
            eval_builtin("identity", &[Value::Int(2), Value::Int(2)]),
            Value::Undef,
            "identity(2, 2) with two args should be Undef"
        );
    }
}
