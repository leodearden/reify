//! Compiler signatures for the math-linalg **construction** builtins
//! (math-linalg α, task 4179) — the frozen §3 contract.
//!
//! Holds the single source of truth for the construction-builtin name family
//! ([`MATH_CONSTRUCTION_NAMES`]), the name-only classification predicate
//! ([`is_math_typed_fn`], mirroring `units::is_geometry_query`), and the
//! shape-dependent result-type resolver ([`math_fn_result_type`]).
//!
//! Unlike the name-only `geometry_query_result_type`, `math_fn_result_type`
//! takes `&[CompiledExpr]` because the construction builtins' return *shape*
//! (the `n` of a `Vector{n}` / `Tensor{rank,n}`) is recovered from the COMPILED
//! ARGUMENT STRUCTURE — list length from a `CompiledExprKind::ListLiteral`, the
//! literal value from `CompiledExprKind::Literal(Value::Int)` — since
//! `Type::List` carries no length.
//!
//! Wired into `expr.rs::resolve_function_overload`'s `NoUserFunctions` ladder
//! (the `is_math_typed_fn` arm) in step-14. The family is pinned disjoint from
//! the geometry / dynamics families by the `units.rs` disjointness tests.

use reify_core::{DimensionVector, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};

/// The complete set of math-linalg **construction** builtin names recognised
/// by the compiler. Single source of truth — imported into the `units.rs` test
/// module to pin disjointness from the geometry / dynamics families.
///
/// Case-sensitive: Reify function names are snake_case. (The §3 operation /
/// function names live in the sibling [`MATH_OPERATION_NAMES`] slice — task
/// 4182 δ — NOT in this construction-only slice.)
pub const MATH_CONSTRUCTION_NAMES: &[&str] = &["vec", "matrix", "diag", "identity"];

/// The complete set of math-linalg **operation / function** builtin names
/// recognised by the compiler (task 4182 δ, the §3 operation family). Sibling
/// to [`MATH_CONSTRUCTION_NAMES`] — kept as a SEPARATE slice so α's
/// construction-only contract (`math_construction_names_are_exactly_the_four`)
/// stays valid; [`is_math_typed_fn`] ORs the two. Single source of truth —
/// imported into the `units.rs` test module to pin disjointness from the five
/// geometry families, the dynamics-query family, AND the construction family.
///
/// Membership is the task-4182 pre-1 frozen set: every §3 operation name that
/// currently DRIFTS to the first-arg default (all are pure eval-builtins with
/// no pub-fn signature). §1.2 trig is deliberately EXCLUDED — it is not in the
/// §3 table this leaf implements; see esc-4182-74 for the (latent) trig
/// compile-time-typing gap surfaced by the probe.
///
/// Case-sensitive snake_case, mirroring [`MATH_CONSTRUCTION_NAMES`].
pub const MATH_OPERATION_NAMES: &[&str] = &[
    // scalar / element-wise
    "sqrt",
    "abs",
    "sign",
    "pow",
    "min",
    "max",
    "clamp",
    "lerp",
    // vector ops
    "dot",
    "cross",
    "normalize",
    "magnitude",
    "outer",
    // matrix ops
    "determinant",
    "inverse",
    "transpose",
    "trace",
    // spectral
    "eigenvalues",
    "complex_eigenvalues",
    // complex
    "complex",
    "real",
    "imag",
    "conjugate",
    "complex_magnitude",
    "phase",
    "arg",
];

/// Is `name` a math-linalg builtin the compiler types via [`math_fn_result_type`]?
/// Name-only classification, mirroring `units::is_geometry_query` (a `.contains`
/// over the single-source-of-truth slices). Recognises BOTH the construction
/// family ([`MATH_CONSTRUCTION_NAMES`]) and the operation family
/// ([`MATH_OPERATION_NAMES`], task 4182 δ). Case-sensitive.
pub(crate) fn is_math_typed_fn(name: &str) -> bool {
    MATH_CONSTRUCTION_NAMES.contains(&name) || MATH_OPERATION_NAMES.contains(&name)
}

/// Result type for a math-linalg construction builtin, derived from the
/// compiled argument structure.
///
/// The return *shape* (`n`) is recovered from the COMPILED ARGUMENT STRUCTURE,
/// not from the arg's `result_type` (which, being a `Type::List`, carries no
/// length): list length from a `CompiledExprKind::ListLiteral`, the literal
/// value from `CompiledExprKind::Literal(Value::Int)`. The quantity slot is the
/// element's `result_type` (or `Type::Real` for the dimensionless `identity`).
///
/// **CRITICAL (D7)**: when the shape is not statically determinable (a
/// non-literal arg — e.g. a `ValueRef`), this degrades to the correct `Vector`
/// / `Tensor` *variant* with a best-effort `n`, recovering the quantity from a
/// `Type::List` element where possible. It NEVER returns the first-arg
/// `Type::List` / `Type::Int`: the eval'd value is a `Value::Vector` /
/// `Value::Tensor`, and `value_type_kind_matches(Value::Vector, Type::List)` is
/// false, so a List fallback would raise a runtime `TypeKindMismatch`.
///
/// Only reached for the four construction names (the caller gates on
/// [`is_math_typed_fn`]); the `_` arm is therefore unreachable in practice and
/// returns a harmless `Type::Real`.
pub(crate) fn math_fn_result_type(name: &str, args: &[CompiledExpr]) -> Type {
    let first = args.first();
    match name {
        // `vec(list)` → Vector{n, quantity}.
        "vec" => {
            let (n, quantity) = first.map_or((0, Type::Real), list_shape);
            Type::Vector {
                n,
                quantity: Box::new(quantity),
            }
        }
        // `diag(list)` → N×N Tensor (same N/quantity recovery as `vec`).
        "diag" => {
            let (n, quantity) = first.map_or((0, Type::Real), list_shape);
            Type::Tensor {
                rank: 2,
                n,
                quantity: Box::new(quantity),
            }
        }
        // `matrix(rows)` → rank-2 Tensor; n = column count from a depth-2 list.
        // D5: an M×N matrix projects to `n = column count`; the row count M is
        // intentionally DISCARDED (`Type::Tensor` carries a single `n`). Future
        // consumers reading `Type::Tensor.n` must NOT assume a square N×N matrix.
        "matrix" => {
            let (n, quantity) = first.map_or((0, Type::Real), matrix_shape);
            Type::Tensor {
                rank: 2,
                n,
                quantity: Box::new(quantity),
            }
        }
        // `identity(n: Int)` → N×N dimensionless Tensor (quantity = Real).
        "identity" => {
            let n = match first.map(|a| &a.kind) {
                Some(CompiledExprKind::Literal(Value::Int(v))) if *v >= 1 => *v as usize,
                // Non-literal / non-Int / non-positive: best-effort n, but STILL
                // a Tensor variant (never the first-arg Int) — D7.
                _ => 0,
            };
            Type::Tensor {
                rank: 2,
                n,
                quantity: Box::new(Type::Real),
            }
        }

        // ── §3 operation family (task 4182 δ) ────────────────────────────────
        // Scalar / element-wise fns.

        // `sqrt` halves the dimension exponents (`Q.root(2)`); a dimensionless
        // result routes back to `Type::Real` (not `Scalar{DIMENSIONLESS}`) so the
        // cell type matches the eval `Value::Real`.
        "sqrt" => {
            let dim = first.map_or(DimensionVector::DIMENSIONLESS, |a| {
                arg_dimension(&a.result_type)
            });
            scalar_or_real(dim.root(2))
        }
        // `abs` is identity over the arg type, EXCEPT it strips a `Complex<Inner>`
        // to its inner type (|z| of a complex is the real magnitude).
        "abs" => match first.map(|a| &a.result_type) {
            Some(Type::Complex(inner)) => (**inner).clone(),
            Some(t) => t.clone(),
            None => Type::Real,
        },
        // `sign` is dimensionless (±1); `pow` is pinned to Real (PRD §3 footnote).
        "sign" | "pow" => Type::Real,
        // `min` / `max` / `clamp` / `lerp` are identity over the first arg's type,
        // PRESERVING its kind (Real stays Real, Scalar stays Scalar) — cloning the
        // type rather than rebuilding a Scalar avoids the Real→Scalar{DIMENSIONLESS}
        // kind drift that would diverge from eval.
        "min" | "max" | "clamp" | "lerp" => {
            first.map(|a| a.result_type.clone()).unwrap_or(Type::Real)
        }

        // Vector ops.
        // `dot` multiplies the operand quantity dimensions → a scalar (Real iff
        // both dimensionless).
        "dot" => scalar_or_real(arg_vector_quantity(args, 0).mul(&arg_vector_quantity(args, 1))),
        // `cross` stays a 3-vector whose quantity is Q1·Q2.
        "cross" => Type::Vector {
            n: 3,
            quantity: Box::new(scalar_or_real(
                arg_vector_quantity(args, 0).mul(&arg_vector_quantity(args, 1)),
            )),
        },
        // `normalize` is dimensionless and preserves N (degrade n→0 if unknown,
        // but STILL a Vector variant — never the first-arg type, D7).
        "normalize" => Type::Vector {
            n: first.map_or(0, |a| vector_n(&a.result_type)),
            quantity: Box::new(Type::Real),
        },
        // `magnitude` collapses a vector to its quantity scalar.
        "magnitude" => scalar_or_real(arg_vector_quantity(args, 0)),
        // `outer` is a rank-2 Tensor; n = column count = second-arg N (degrade
        // →0 if unknown), quantity = Q1·Q2. NEVER the first-arg Vector type (D7).
        "outer" => Type::Tensor {
            rank: 2,
            n: args.get(1).map_or(0, |a| vector_n(&a.result_type)),
            quantity: Box::new(scalar_or_real(
                arg_vector_quantity(args, 0).mul(&arg_vector_quantity(args, 1)),
            )),
        },

        // Matrix ops (read {n, quantity} from a rank-2 `Tensor` or a user-facing
        // `Matrix`). The `determinant(AffineMap)→Real` row is served upstream by
        // `affine_map_algebra_result_type` (runs before this arm), so the args
        // reaching here are only Tensor/Matrix.
        //
        // `determinant` of an N×N matrix scales as Q^N. N is read from the
        // matrix shape (capped into the i8 domain `pow` accepts); a dimensionless
        // result routes back to Real.
        "determinant" => scalar_or_real(
            arg_matrix_quantity(args, 0).pow(clamp_i8(arg_matrix_n(args, 0))),
        ),
        // `inverse` negates the quantity dimension (Q⁻¹) and preserves the shape.
        // ALWAYS a rank-2 Tensor variant — never the first-arg type (D7).
        "inverse" => Type::Tensor {
            rank: 2,
            n: arg_matrix_n(args, 0),
            quantity: Box::new(scalar_or_real(
                DimensionVector::DIMENSIONLESS.div(&arg_matrix_quantity(args, 0)),
            )),
        },
        // `transpose` preserves shape + quantity: identity over a statically
        // shaped matrix; otherwise degrade to a rank-2 Tensor variant (never the
        // first-arg type, D7).
        "transpose" => match first.map(|a| &a.result_type) {
            Some(t @ (Type::Tensor { .. } | Type::Matrix { .. })) => t.clone(),
            _ => Type::Tensor {
                rank: 2,
                n: arg_matrix_n(args, 0),
                quantity: Box::new(scalar_or_real(arg_matrix_quantity(args, 0))),
            },
        },
        // `trace` sums the diagonal → a single quantity scalar.
        "trace" => scalar_or_real(arg_matrix_quantity(args, 0)),

        // Spectral ops. The eigenvalues of a matrix carry the matrix's quantity
        // (a dimensionless matrix → dimensionless eigenvalues, routed to Real).
        // The result KIND is a `List` — NEVER the first-arg Tensor — so the
        // eval'd `Value::List` matches under `value_type_kind_matches` (D7).
        //
        // `eigenvalues` → List(Scalar<Q>) (real spectrum).
        "eigenvalues" => Type::List(Box::new(scalar_or_real(arg_matrix_quantity(args, 0)))),
        // `complex_eigenvalues` → List(Complex<Q>) (general/complex spectrum).
        "complex_eigenvalues" => Type::List(Box::new(Type::Complex(Box::new(scalar_or_real(
            arg_matrix_quantity(args, 0),
        ))))),

        _ => Type::Real,
    }
}

/// The quantity dimension of a `Vector` operand (its `quantity` Scalar's
/// dimension, or `DIMENSIONLESS` for a `Real`/unknown quantity or a non-Vector).
fn vector_quantity_dimension(t: &Type) -> DimensionVector {
    match t {
        Type::Vector { quantity, .. } => arg_dimension(quantity),
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// The vector quantity dimension of `args[i]` (or `DIMENSIONLESS` if absent).
fn arg_vector_quantity(args: &[CompiledExpr], i: usize) -> DimensionVector {
    args.get(i)
        .map_or(DimensionVector::DIMENSIONLESS, |a| {
            vector_quantity_dimension(&a.result_type)
        })
}

/// The element count `n` of a `Vector` operand, or `0` when not statically
/// known (the D7 degrade — the result still uses the correct VARIANT with a
/// best-effort `n`).
fn vector_n(t: &Type) -> usize {
    match t {
        Type::Vector { n, .. } => *n,
        _ => 0,
    }
}

/// The per-dimension element count `n` of a matrix-like operand — a rank-2
/// `Tensor{n}` (what the `matrix`/`diag`/`identity` constructors produce) or a
/// user-facing `Matrix{n}` (column count). `0` when not a statically-shaped
/// matrix (the D7 degrade — the result still uses the correct VARIANT).
fn matrix_n(t: &Type) -> usize {
    match t {
        Type::Tensor { n, .. } | Type::Matrix { n, .. } => *n,
        _ => 0,
    }
}

/// The shape `n` of the matrix-like `args[i]` (or `0` if absent / not a matrix).
fn arg_matrix_n(args: &[CompiledExpr], i: usize) -> usize {
    args.get(i).map_or(0, |a| matrix_n(&a.result_type))
}

/// The quantity dimension of a matrix-like operand (its `quantity` Scalar's
/// dimension, or `DIMENSIONLESS` for a `Real`/unknown quantity or a non-matrix).
fn matrix_quantity_dimension(t: &Type) -> DimensionVector {
    match t {
        Type::Tensor { quantity, .. } | Type::Matrix { quantity, .. } => arg_dimension(quantity),
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// The matrix quantity dimension of `args[i]` (or `DIMENSIONLESS` if absent).
fn arg_matrix_quantity(args: &[CompiledExpr], i: usize) -> DimensionVector {
    args.get(i).map_or(DimensionVector::DIMENSIONLESS, |a| {
        matrix_quantity_dimension(&a.result_type)
    })
}

/// Clamp a matrix dimension `n` (a `usize`) into the `i8` domain
/// [`DimensionVector::pow`] accepts, saturating at `i8::MAX`. Realistic matrices
/// are tiny; this only guards a pathological/overflowing `N` from silently
/// wrapping on the `as i8` cast (the determinant `Q^N` arm).
fn clamp_i8(n: usize) -> i8 {
    n.min(i8::MAX as usize) as i8
}

/// The dimension of an arg's `result_type` for the math return-type algebra:
/// a `Scalar` contributes its dimension; `Real` / `Int` (and any non-Scalar)
/// contribute `DIMENSIONLESS`. Used by the dimension-CHANGING operation arms
/// (sqrt, dot, determinant, …) to extract `Q` before applying the dim algebra.
fn arg_dimension(t: &Type) -> DimensionVector {
    match t {
        Type::Scalar { dimension } => *dimension,
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// Build a dimensioned-scalar result, routing the dimensionless case back to
/// `Type::Real` (NOT `Scalar{DIMENSIONLESS}`).
///
/// This is the load-bearing Scalar-vs-Real boundary: eval yields `Value::Real`
/// for a dimensionless result and `Value::Scalar` for a dimensioned one, and
/// `value_type_kind_matches(Value::Real, Scalar{DIMENSIONLESS})` is false — so a
/// dimensionless arm MUST return `Type::Real` to keep the two-way boundary
/// agreeing (task 4182 δ).
fn scalar_or_real(dim: DimensionVector) -> Type {
    if dim.is_dimensionless() {
        Type::Real
    } else {
        Type::Scalar { dimension: dim }
    }
}

/// Recover `(n, element_quantity)` from a single list argument (`vec` / `diag`).
///
/// - `ListLiteral(elems)` → `(elems.len(), elems[0].result_type)` — exact.
/// - otherwise → `(0, <innermost List element>)` — the DEGRADE path (D7):
///   length unknown, quantity recovered from the arg's `Type::List` where
///   possible, defaulting to `Type::Real`.
fn list_shape(arg: &CompiledExpr) -> (usize, Type) {
    if let CompiledExprKind::ListLiteral(elems) = &arg.kind {
        let quantity = elems
            .first()
            .map(|e| e.result_type.clone())
            .unwrap_or(Type::Real);
        (elems.len(), quantity)
    } else {
        (0, innermost_list_element(&arg.result_type))
    }
}

/// Recover `(ncols, cell_quantity)` from a depth-2 list argument (`matrix`).
///
/// - outer `ListLiteral` whose first row is itself a `ListLiteral(cells)` →
///   `(cells.len(), cells[0].result_type)` — exact column count (an M×N matrix
///   projects to `n = N`, per design decision D5).
/// - otherwise → `(0, <innermost List element>)` — DEGRADE (D7).
fn matrix_shape(arg: &CompiledExpr) -> (usize, Type) {
    if let CompiledExprKind::ListLiteral(rows) = &arg.kind
        && let Some(CompiledExprKind::ListLiteral(cells)) = rows.first().map(|r| &r.kind)
    {
        let quantity = cells
            .first()
            .map(|c| c.result_type.clone())
            .unwrap_or(Type::Real);
        return (cells.len(), quantity);
    }
    (0, innermost_list_element(&arg.result_type))
}

/// Strip all leading `Type::List` wrappers, returning the innermost element
/// type (the scalar quantity). `List(List(Real))` / `List(Real)` / `Real` all
/// → `Real`. Used by the DEGRADE path so a non-literal arg still yields a
/// quantity rather than the bare `List`.
fn innermost_list_element(t: &Type) -> Type {
    let mut cur = t;
    while let Type::List(elem) = cur {
        cur = elem;
    }
    cur.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four construction-builtin names, frozen by the §3 contract. Local
    /// fixture so a drift in `MATH_CONSTRUCTION_NAMES` is caught against an
    /// independent list rather than against itself.
    const EXPECTED_NAMES: [&str; 4] = ["vec", "matrix", "diag", "identity"];

    // ── Name-family contract (step-9 RED / step-10 GREEN) ────────────────────

    /// `is_math_typed_fn` recognises every construction-builtin name.
    #[test]
    fn is_math_typed_fn_recognises_all_construction_names() {
        for name in EXPECTED_NAMES {
            assert!(
                is_math_typed_fn(name),
                "is_math_typed_fn({name:?}) must be true (math-linalg α §3 contract)"
            );
        }
    }

    /// `is_math_typed_fn` rejects names from the other builtin families, the
    /// empty name, an unrelated name, and a sibling math name that α does NOT
    /// register (`determinant` is a β operation, not an α constructor).
    #[test]
    fn is_math_typed_fn_rejects_other_family_and_unknown_names() {
        // Geometry-query family (`units::GEOMETRY_QUERY_NAMES`).
        assert!(
            !is_math_typed_fn("volume"),
            "must reject geometry-query 'volume'"
        );
        // Dynamics-query family (`units::DYNAMICS_QUERY_NAMES`).
        assert!(
            !is_math_typed_fn("body_mass_props"),
            "must reject dynamics-query 'body_mass_props'"
        );
        // `determinant` is now an in-family OPERATION name (task 4182 δ added it
        // to MATH_OPERATION_NAMES), so it must be RECOGNISED — not rejected.
        assert!(
            is_math_typed_fn("determinant"),
            "must recognise 'determinant' — a math-linalg δ operation builtin"
        );
        // Empty / unrelated / a plausible-but-nonexistent math op.
        assert!(!is_math_typed_fn(""), "must reject empty name");
        assert!(
            !is_math_typed_fn("does_not_exist"),
            "must reject unrelated name"
        );
        assert!(
            !is_math_typed_fn("eigenvectors"),
            "must reject 'eigenvectors' — a plausible but unregistered math op"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so the
    /// PascalCase forms must not match (mirrors `is_geometry_query_is_case_sensitive`).
    #[test]
    fn is_math_typed_fn_is_case_sensitive() {
        assert!(!is_math_typed_fn("Vec"));
        assert!(!is_math_typed_fn("Matrix"));
        assert!(!is_math_typed_fn("Diag"));
        assert!(!is_math_typed_fn("Identity"));
    }

    /// `MATH_CONSTRUCTION_NAMES` is exactly the four construction names —
    /// membership both ways plus an exact count (so neither a missing nor an
    /// extra name slips through).
    #[test]
    fn math_construction_names_are_exactly_the_four() {
        assert_eq!(
            MATH_CONSTRUCTION_NAMES.len(),
            EXPECTED_NAMES.len(),
            "MATH_CONSTRUCTION_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            MATH_CONSTRUCTION_NAMES
        );
        for name in EXPECTED_NAMES {
            assert!(
                MATH_CONSTRUCTION_NAMES.contains(&name),
                "MATH_CONSTRUCTION_NAMES must contain {name:?}"
            );
        }
    }

    // ── Operation name-family contract (task 4182 δ, step-1 RED / step-2 GREEN) ──

    /// The math-linalg **operation** names frozen by the task-4182 pre-1 probe:
    /// every §3 operation/function name that currently DRIFTS to the first-arg
    /// default (confirmed empirically — all are pure eval-builtins with no
    /// pub-fn signature, so they reach the `NoUserFunctions` first-arg
    /// fallback). Local fixture so a drift in `MATH_OPERATION_NAMES` is caught
    /// against an independent list rather than against itself (mirrors
    /// `EXPECTED_NAMES` for the construction family). §1.2 trig is deliberately
    /// EXCLUDED — see task-4182 / esc-4182-74.
    const EXPECTED_OPERATION_NAMES: [&str; 26] = [
        // scalar / element-wise
        "sqrt", "abs", "sign", "pow", "min", "max", "clamp", "lerp",
        // vector ops
        "dot", "cross", "normalize", "magnitude", "outer",
        // matrix ops
        "determinant", "inverse", "transpose", "trace",
        // spectral
        "eigenvalues", "complex_eigenvalues",
        // complex
        "complex", "real", "imag", "conjugate", "complex_magnitude", "phase", "arg",
    ];

    /// `is_math_typed_fn` recognises every math-linalg OPERATION name (the
    /// task-4182 δ scope-extension over α's construction-only family).
    #[test]
    fn is_math_typed_fn_recognises_all_operation_names() {
        for name in EXPECTED_OPERATION_NAMES {
            assert!(
                is_math_typed_fn(name),
                "is_math_typed_fn({name:?}) must be true (math-linalg δ §3 operation family)"
            );
        }
    }

    /// `MATH_OPERATION_NAMES` is exactly the pre-1 frozen operation set —
    /// membership both ways plus an exact count (so neither a missing nor an
    /// extra name slips through), mirroring
    /// `math_construction_names_are_exactly_the_four`.
    #[test]
    fn math_operation_names_are_exactly_the_frozen_set() {
        assert_eq!(
            MATH_OPERATION_NAMES.len(),
            EXPECTED_OPERATION_NAMES.len(),
            "MATH_OPERATION_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_OPERATION_NAMES.len(),
            MATH_OPERATION_NAMES
        );
        for name in EXPECTED_OPERATION_NAMES {
            assert!(
                MATH_OPERATION_NAMES.contains(&name),
                "MATH_OPERATION_NAMES must contain {name:?}"
            );
        }
        // Converse: no extra name beyond the frozen fixture.
        for name in MATH_OPERATION_NAMES {
            assert!(
                EXPECTED_OPERATION_NAMES.contains(name),
                "MATH_OPERATION_NAMES has unexpected entry {name:?} not in the frozen set"
            );
        }
    }

    /// Both families are recognised by `is_math_typed_fn`, but they remain
    /// distinct slices — δ ORs them rather than merging (so α's
    /// `math_construction_names_are_exactly_the_four` stays valid).
    #[test]
    fn is_math_typed_fn_recognises_construction_and_operation_alike() {
        for name in EXPECTED_NAMES {
            assert!(
                is_math_typed_fn(name),
                "construction name {name:?} must still resolve"
            );
        }
        for name in EXPECTED_OPERATION_NAMES {
            assert!(is_math_typed_fn(name), "operation name {name:?} must resolve");
        }
    }

    /// Case-sensitivity invariant for the operation family: Reify function
    /// names are snake_case, so PascalCase forms must not match (mirrors
    /// `is_math_typed_fn_is_case_sensitive` for the construction family).
    #[test]
    fn is_math_typed_fn_operation_names_are_case_sensitive() {
        assert!(!is_math_typed_fn("Sqrt"));
        assert!(!is_math_typed_fn("Determinant"));
        assert!(!is_math_typed_fn("Eigenvalues"));
        assert!(!is_math_typed_fn("Complex"));
    }

    // ── Result-type resolution (step-11 RED / step-12 GREEN) ─────────────────

    use reify_core::DimensionVector;
    use reify_core::identity::ValueCellId;
    use reify_ir::Value;

    /// A dimensionless `Real` element expression (`result_type = Type::Real`).
    fn real_elem(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::Real)
    }

    /// A `Scalar<Length>` element expression.
    fn length_elem(v: f64) -> CompiledExpr {
        CompiledExpr::literal(
            Value::Scalar {
                si_value: v,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        )
    }

    /// A `ListLiteral` of `elems` whose own `result_type` is `List(elem_ty)`.
    /// `math_fn_result_type` reads N + quantity from the ELEMENT structure, not
    /// from this outer result_type, so its exact value is immaterial — set
    /// realistically anyway.
    fn list_lit(elems: Vec<CompiledExpr>, elem_ty: Type) -> CompiledExpr {
        CompiledExpr::list_literal(elems, Type::List(Box::new(elem_ty)))
    }

    /// (a) `vec` over a 3-element dimensionless `ListLiteral` →
    /// `Vector{n:3, quantity:Real}`.
    #[test]
    fn vec_result_type_dimensionless_is_vector_n3_real() {
        let arg = list_lit(vec![real_elem(1.0), real_elem(2.0), real_elem(3.0)], Type::Real);
        assert_eq!(
            math_fn_result_type("vec", &[arg]),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (b) `vec` over `Scalar<Length>` elements → `Vector{n:2, quantity:Scalar<Length>}`.
    #[test]
    fn vec_result_type_length_preserves_quantity() {
        let len_ty = Type::Scalar {
            dimension: DimensionVector::LENGTH,
        };
        let arg = list_lit(vec![length_elem(1.0), length_elem(2.0)], len_ty.clone());
        assert_eq!(
            math_fn_result_type("vec", &[arg]),
            Type::Vector {
                n: 2,
                quantity: Box::new(len_ty)
            }
        );
    }

    /// (c) `matrix` over a depth-2 2×2 `ListLiteral` → `Tensor{rank:2, n:2, quantity:Real}`.
    #[test]
    fn matrix_result_type_2x2_is_tensor_rank2_n2_real() {
        let row0 = list_lit(vec![real_elem(1.0), real_elem(2.0)], Type::Real);
        let row1 = list_lit(vec![real_elem(3.0), real_elem(4.0)], Type::Real);
        let arg = list_lit(vec![row0, row1], Type::List(Box::new(Type::Real)));
        assert_eq!(
            math_fn_result_type("matrix", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 2,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (d) `diag` over a 3-element `ListLiteral` → `Tensor{rank:2, n:3, quantity:Real}`.
    #[test]
    fn diag_result_type_is_tensor_rank2_n3_real() {
        let arg = list_lit(vec![real_elem(3.0), real_elem(5.0), real_elem(7.0)], Type::Real);
        assert_eq!(
            math_fn_result_type("diag", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (e) `identity` over `Literal(Value::Int(4))` → `Tensor{rank:2, n:4, quantity:Real}`
    /// (dimensionless).
    #[test]
    fn identity_result_type_is_tensor_rank2_n4_real() {
        let arg = CompiledExpr::literal(Value::Int(4), Type::Int);
        assert_eq!(
            math_fn_result_type("identity", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 4,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (f) DEGRADE (locks D7): a non-literal `vec` arg — a `ValueRef` typed
    /// `List(Real)` whose length is NOT statically known — must STILL resolve to
    /// a `Type::Vector{..}` variant (quantity recovered from the `List` element),
    /// NEVER the first-arg `Type::List`. Falling through to `List` would make the
    /// eval'd `Value::Vector` fail `value_type_kind_matches` at runtime.
    #[test]
    fn vec_result_type_non_literal_arg_degrades_to_vector_not_list() {
        let arg = CompiledExpr::value_ref(
            ValueCellId::new("S", "x"),
            Type::List(Box::new(Type::Real)),
        );
        let result = math_fn_result_type("vec", &[arg]);
        assert!(
            !matches!(result, Type::List(_)),
            "non-literal vec arg must NOT degrade to Type::List (D7), got {result:?}"
        );
        match result {
            Type::Vector { quantity, .. } => assert_eq!(
                *quantity,
                Type::Real,
                "degraded Vector quantity should be recovered from the List element"
            ),
            other => panic!("expected a Type::Vector variant, got {other:?}"),
        }
    }

    // ── D7 degrade-path coverage (amendment: reviewer test_coverage) ─────────
    // The DEGRADE invariant (D7) — a non-statically-determinable arg must still
    // resolve to the correct Vector/Tensor *variant*, never the first-arg
    // List/Int — was originally unit-tested only for `vec` (test f). These pin
    // the same invariant for the matrix/diag/identity branches that protect it.

    /// `matrix`, degrade branch 1: a non-literal arg (a `ValueRef` typed
    /// `List(List(Real))`, length unknown) must STILL resolve to a rank-2
    /// `Type::Tensor` variant, NEVER the first-arg `Type::List`. A `List`
    /// fallback would make the eval'd `Value::Tensor` fail
    /// `value_type_kind_matches` at runtime.
    #[test]
    fn matrix_result_type_non_literal_arg_degrades_to_tensor_not_list() {
        let arg = CompiledExpr::value_ref(
            ValueCellId::new("S", "m"),
            Type::List(Box::new(Type::List(Box::new(Type::Real)))),
        );
        let result = math_fn_result_type("matrix", &[arg]);
        assert!(
            !matches!(result, Type::List(_)),
            "non-literal matrix arg must NOT degrade to Type::List (D7), got {result:?}"
        );
        assert!(
            matches!(result, Type::Tensor { rank: 2, .. }),
            "non-literal matrix arg must degrade to a rank-2 Type::Tensor variant, got {result:?}"
        );
    }

    /// `matrix`, degrade branch 2: an outer `ListLiteral` whose first row is NOT
    /// itself a `ListLiteral` (a malformed depth-1 list, e.g. `matrix([1.0, 2.0])`)
    /// must STILL resolve to a rank-2 `Type::Tensor` variant, never `Type::List`.
    #[test]
    fn matrix_result_type_non_list_first_row_degrades_to_tensor_not_list() {
        let arg = list_lit(vec![real_elem(1.0), real_elem(2.0)], Type::Real);
        let result = math_fn_result_type("matrix", &[arg]);
        assert!(
            !matches!(result, Type::List(_)),
            "matrix with a non-list first row must NOT degrade to Type::List (D7), got {result:?}"
        );
        assert!(
            matches!(result, Type::Tensor { rank: 2, .. }),
            "matrix with a non-list first row must degrade to a rank-2 Type::Tensor variant, got {result:?}"
        );
    }

    /// `diag`: a non-literal arg (a `ValueRef` typed `List(Real)`) must STILL
    /// resolve to a rank-2 `Type::Tensor` variant, never the first-arg `Type::List`.
    #[test]
    fn diag_result_type_non_literal_arg_degrades_to_tensor_not_list() {
        let arg =
            CompiledExpr::value_ref(ValueCellId::new("S", "d"), Type::List(Box::new(Type::Real)));
        let result = math_fn_result_type("diag", &[arg]);
        assert!(
            !matches!(result, Type::List(_)),
            "non-literal diag arg must NOT degrade to Type::List (D7), got {result:?}"
        );
        assert!(
            matches!(result, Type::Tensor { rank: 2, .. }),
            "non-literal diag arg must degrade to a rank-2 Type::Tensor variant, got {result:?}"
        );
    }

    /// `identity`: a non-literal / non-Int / non-positive arg must STILL resolve
    /// to a rank-2 `Type::Tensor` variant, NEVER the first-arg `Type::Int`.
    /// (`identity` always types as a Tensor; only `n` is unknown off the literal
    /// path.)
    #[test]
    fn identity_result_type_non_literal_arg_degrades_to_tensor_not_int() {
        let cases = [
            (
                "non-literal ValueRef<Int>",
                CompiledExpr::value_ref(ValueCellId::new("S", "i"), Type::Int),
            ),
            (
                "non-positive Literal(Int(0))",
                CompiledExpr::literal(Value::Int(0), Type::Int),
            ),
            (
                "non-Int Literal(Real)",
                CompiledExpr::literal(Value::Real(4.0), Type::Real),
            ),
        ];
        for (label, arg) in cases {
            let result = math_fn_result_type("identity", &[arg]);
            assert!(
                !matches!(result, Type::Int),
                "identity degrade ({label}) must NOT yield Type::Int (D7), got {result:?}"
            );
            assert!(
                matches!(result, Type::Tensor { rank: 2, .. }),
                "identity degrade ({label}) must yield a rank-2 Type::Tensor variant, got {result:?}"
            );
        }
    }

    // ── Non-square projection (amendment: reviewer design_coherence) ─────────

    /// NON-SQUARE `matrix` projects to `n = column count` (locks D5). A 2×3
    /// matrix (`matrix([[1,2,3],[4,5,6]])`) types as `Tensor{rank:2, n:3}` — the
    /// row count (M=2) is intentionally discarded. Pins the documented
    /// projection so a future `Type::Tensor.n` consumer can't silently assume a
    /// square N×N. Prior matrix tests only covered the square 2×2 case.
    #[test]
    fn matrix_result_type_non_square_projects_to_column_count() {
        // 2 rows, 3 columns.
        let row0 = list_lit(
            vec![real_elem(1.0), real_elem(2.0), real_elem(3.0)],
            Type::Real,
        );
        let row1 = list_lit(
            vec![real_elem(4.0), real_elem(5.0), real_elem(6.0)],
            Type::Real,
        );
        let arg = list_lit(vec![row0, row1], Type::List(Box::new(Type::Real)));
        assert_eq!(
            math_fn_result_type("matrix", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 3, // column count (3), NOT row count (2) — D5
                quantity: Box::new(Type::Real)
            },
            "non-square 2x3 matrix must project to n = column count = 3 (D5)"
        );
    }

    // ── Operation result-type: scalar / element fns ──────────────────────────
    // (task 4182 δ, step-3 RED / step-4 GREEN)

    /// A value-agnostic typed arg — `math_fn_result_type` reads only
    /// `result_type`, so the carried `Value` is immaterial (use `Undef`).
    fn typed(ty: Type) -> CompiledExpr {
        CompiledExpr::literal(Value::Undef, ty)
    }

    /// `Type::Scalar { dimension }` shorthand for tests.
    fn sca(dim: DimensionVector) -> Type {
        Type::Scalar { dimension: dim }
    }

    /// sqrt halves the dimension exponents: sqrt(Scalar<Length²>) → Scalar<Length>
    /// (`LENGTH.pow(2).root(2)`).
    #[test]
    fn sqrt_of_area_is_length() {
        let area = sca(DimensionVector::LENGTH.pow(2)); // == AREA
        assert_eq!(
            math_fn_result_type("sqrt", &[typed(area)]),
            sca(DimensionVector::LENGTH)
        );
    }

    /// sqrt of a dimensionless arg stays `Type::Real` (NOT Scalar{DIMENSIONLESS})
    /// so the cell type matches the eval `Value::Real` under `value_type_kind_matches`.
    #[test]
    fn sqrt_of_real_is_real() {
        assert_eq!(math_fn_result_type("sqrt", &[real_elem(4.0)]), Type::Real);
    }

    /// abs preserves a Scalar's dimension verbatim.
    #[test]
    fn abs_of_scalar_preserves_dimension() {
        assert_eq!(
            math_fn_result_type("abs", &[length_elem(1.0)]),
            sca(DimensionVector::LENGTH)
        );
    }

    /// abs of a `Complex<Inner>` strips the Complex, returning the inner Scalar.
    #[test]
    fn abs_of_complex_strips_to_inner_scalar() {
        let z = typed(Type::Complex(Box::new(sca(DimensionVector::LENGTH))));
        assert_eq!(
            math_fn_result_type("abs", &[z]),
            sca(DimensionVector::LENGTH)
        );
    }

    /// sign is dimensionless regardless of arg → `Type::Real`.
    #[test]
    fn sign_is_real() {
        assert_eq!(math_fn_result_type("sign", &[length_elem(1.0)]), Type::Real);
    }

    /// pow is pinned to `Type::Real` (PRD §3 footnote — prevents the
    /// dimensioned-arg misread).
    #[test]
    fn pow_is_real() {
        assert_eq!(
            math_fn_result_type("pow", &[length_elem(1.0), real_elem(2.0)]),
            Type::Real
        );
    }

    /// min/max are identity over the first arg's type, PRESERVING its kind:
    /// min(Scalar<Q>,…) → Scalar<Q>, but max(Real,Real) → Real (not
    /// Scalar{DIMENSIONLESS} — the D6/D7 kind-drift hazard).
    #[test]
    fn min_max_are_kind_preserving_identity() {
        assert_eq!(
            math_fn_result_type("min", &[length_elem(1.0), length_elem(2.0)]),
            sca(DimensionVector::LENGTH)
        );
        assert_eq!(
            math_fn_result_type("max", &[length_elem(1.0), length_elem(2.0)]),
            sca(DimensionVector::LENGTH)
        );
        assert_eq!(
            math_fn_result_type("max", &[real_elem(1.0), real_elem(2.0)]),
            Type::Real,
            "max(Real,Real) must stay Real (kind-preserving identity), not Scalar"
        );
    }

    /// clamp/lerp are identity over the first arg's type (kind-preserving).
    #[test]
    fn clamp_lerp_are_identity() {
        assert_eq!(
            math_fn_result_type(
                "clamp",
                &[length_elem(1.0), length_elem(0.0), length_elem(2.0)]
            ),
            sca(DimensionVector::LENGTH)
        );
        assert_eq!(
            math_fn_result_type(
                "lerp",
                &[length_elem(0.0), length_elem(2.0), real_elem(0.5)]
            ),
            sca(DimensionVector::LENGTH)
        );
    }

    // ── Operation result-type: vector ops ────────────────────────────────────
    // (task 4182 δ, step-5 RED / step-6 GREEN)

    /// `Type::Vector { n, quantity: Scalar<dim> }` shorthand for tests.
    fn vecq(n: usize, dim: DimensionVector) -> Type {
        Type::Vector {
            n,
            quantity: Box::new(sca(dim)),
        }
    }

    /// dot multiplies the operand dimensions: dot(Vec<2,L>, Vec<2,L>) → Scalar<Area>.
    #[test]
    fn dot_of_length_vectors_is_area() {
        let v = typed(vecq(2, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("dot", &[v.clone(), v]),
            sca(DimensionVector::LENGTH.mul(&DimensionVector::LENGTH))
        );
    }

    /// cross multiplies dims and stays a 3-vector: cross(Vec<3,L>, Vec<3,F>) →
    /// Vector<3, Scalar<L·F>>.
    #[test]
    fn cross_of_length_force_is_vector3_torque() {
        let a = typed(vecq(3, DimensionVector::LENGTH));
        let b = typed(vecq(3, DimensionVector::FORCE));
        assert_eq!(
            math_fn_result_type("cross", &[a, b]),
            Type::Vector {
                n: 3,
                quantity: Box::new(sca(DimensionVector::LENGTH.mul(&DimensionVector::FORCE)))
            }
        );
    }

    /// normalize is dimensionless and preserves N: normalize(Vec<4,L>) → Vector<4, Real>.
    #[test]
    fn normalize_is_dimensionless_vector_preserving_n() {
        let v = typed(vecq(4, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("normalize", &[v]),
            Type::Vector {
                n: 4,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// magnitude collapses a vector to its quantity scalar: magnitude(Vec<3,L>) → Scalar<L>.
    #[test]
    fn magnitude_is_quantity_scalar() {
        let v = typed(vecq(3, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("magnitude", &[v]),
            sca(DimensionVector::LENGTH)
        );
    }

    /// outer is a rank-2 Tensor whose quantity is Q1·Q2 and whose n is the
    /// column count (second-arg N): outer(Vec<2,L>, Vec<3,F>) →
    /// Tensor{rank:2, n:3, Scalar<L·F>}.
    #[test]
    fn outer_is_rank2_tensor_with_product_quantity() {
        let a = typed(vecq(2, DimensionVector::LENGTH));
        let b = typed(vecq(3, DimensionVector::FORCE));
        assert_eq!(
            math_fn_result_type("outer", &[a, b]),
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(sca(DimensionVector::LENGTH.mul(&DimensionVector::FORCE)))
            }
        );
    }

    // ── Operation result-type: matrix ops ────────────────────────────────────
    // (task 4182 δ, step-7 RED / step-8 GREEN)

    /// `Type::Tensor { rank: 2, n, quantity: Scalar<dim> }` shorthand for tests
    /// (a square N×N matrix with a dimensioned quantity).
    fn tenq(n: usize, dim: DimensionVector) -> Type {
        Type::Tensor {
            rank: 2,
            n,
            quantity: Box::new(sca(dim)),
        }
    }

    /// determinant of an N×N matrix raises the quantity to the Nth power:
    /// determinant(Tensor<2,4,Length>) → Scalar<Length⁴> (`Q.pow(4)`).
    #[test]
    fn determinant_of_4x4_length_is_length_pow4() {
        let m = typed(tenq(4, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("determinant", &[m]),
            sca(DimensionVector::LENGTH.pow(4))
        );
    }

    /// determinant of a 2×2 Length matrix → Scalar<Area> (`Q.pow(2)` == AREA).
    #[test]
    fn determinant_of_2x2_length_is_area() {
        let m = typed(tenq(2, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("determinant", &[m]),
            sca(DimensionVector::AREA),
            "determinant of a 2x2 Length matrix must be Scalar<Area> (Length²)"
        );
    }

    /// N is read from `Type::Tensor{n}`: a 5×5 Length matrix → Scalar<Length⁵>.
    #[test]
    fn determinant_reads_n_from_tensor_shape() {
        let m = typed(tenq(5, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("determinant", &[m]),
            sca(DimensionVector::LENGTH.pow(5)),
            "determinant must read N from Type::Tensor.n (5×5 → Length⁵)"
        );
    }

    /// inverse negates the quantity dimension: inverse(Tensor<2,3,Length>) →
    /// Tensor<2,3,Scalar<Length⁻¹>> (`DIMENSIONLESS.div(Q)`), shape preserved.
    #[test]
    fn inverse_of_length_matrix_is_inverse_length_tensor() {
        let m = typed(tenq(3, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("inverse", &[m]),
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(sca(
                    DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH)
                ))
            }
        );
    }

    /// transpose is identity over the Tensor type (shape + quantity preserved).
    #[test]
    fn transpose_is_identity_over_tensor() {
        let m = tenq(3, DimensionVector::LENGTH);
        assert_eq!(
            math_fn_result_type("transpose", &[typed(m.clone())]),
            m,
            "transpose(Tensor<2,N,Q>) must be the identical Tensor<2,N,Q>"
        );
    }

    /// trace sums the diagonal → a single quantity scalar:
    /// trace(Tensor<2,N,Q>) → Scalar<Q>.
    #[test]
    fn trace_is_quantity_scalar() {
        let m = typed(tenq(3, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("trace", &[m]),
            sca(DimensionVector::LENGTH)
        );
    }

    // ── Operation result-type: spectral ops ──────────────────────────────────
    // (task 4182 δ, step-9 RED / step-10 GREEN)

    /// A `Type::Tensor { rank: 2, n, quantity: Real }` shorthand — a
    /// dimensionless N×N matrix.
    fn ten_real(n: usize) -> Type {
        Type::Tensor {
            rank: 2,
            n,
            quantity: Box::new(Type::Real),
        }
    }

    /// eigenvalues returns a List of the matrix quantity scalar:
    /// eigenvalues(Tensor<2,N,Length>) → List(Scalar<Length>).
    #[test]
    fn eigenvalues_of_length_matrix_is_list_of_scalar() {
        let m = typed(tenq(3, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("eigenvalues", &[m]),
            Type::List(Box::new(sca(DimensionVector::LENGTH)))
        );
    }

    /// eigenvalues of a dimensionless matrix → List(Real) (NOT
    /// List(Scalar{DIMENSIONLESS})), so the eval'd Value::List<Real> matches
    /// under value_type_kind_matches.
    #[test]
    fn eigenvalues_of_dimensionless_matrix_is_list_of_real() {
        assert_eq!(
            math_fn_result_type("eigenvalues", &[typed(ten_real(3))]),
            Type::List(Box::new(Type::Real))
        );
    }

    /// complex_eigenvalues returns a List of Complex<quantity>:
    /// complex_eigenvalues(Tensor<2,N,Length>) → List(Complex(Scalar<Length>)).
    #[test]
    fn complex_eigenvalues_of_length_matrix_is_list_of_complex_scalar() {
        let m = typed(tenq(3, DimensionVector::LENGTH));
        assert_eq!(
            math_fn_result_type("complex_eigenvalues", &[m]),
            Type::List(Box::new(Type::Complex(Box::new(sca(DimensionVector::LENGTH)))))
        );
    }

    /// complex_eigenvalues of a dimensionless matrix → List(Complex(Real)).
    #[test]
    fn complex_eigenvalues_of_dimensionless_matrix_is_list_of_complex_real() {
        assert_eq!(
            math_fn_result_type("complex_eigenvalues", &[typed(ten_real(3))]),
            Type::List(Box::new(Type::Complex(Box::new(Type::Real))))
        );
    }

    /// The result KIND is `Type::List`, never the first-arg Tensor — so the
    /// eval'd `Value::List` passes `value_type_kind_matches` (the D7-style kind
    /// guard).
    #[test]
    fn spectral_results_are_list_kind_not_tensor() {
        let m = typed(tenq(4, DimensionVector::LENGTH));
        assert!(
            matches!(
                math_fn_result_type("eigenvalues", &[m.clone()]),
                Type::List(_)
            ),
            "eigenvalues must be a Type::List, not the first-arg Tensor"
        );
        assert!(
            matches!(
                math_fn_result_type("complex_eigenvalues", &[m]),
                Type::List(_)
            ),
            "complex_eigenvalues must be a Type::List, not the first-arg Tensor"
        );
    }
}
