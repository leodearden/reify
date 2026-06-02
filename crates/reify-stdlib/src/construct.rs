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
//! new diagnostic code, mirroring `matrix_components_f64`'s shape-guards.
//!
//! Built inline (a construct.rs-local `Value::List` extractor, local Tensor
//! assembly) rather than editing `matrix.rs` / `helpers.rs`, which are owned by
//! sibling tasks β/γ — avoids narrow-file-lock contention and merge
//! serialization (PRD §7).

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
}
