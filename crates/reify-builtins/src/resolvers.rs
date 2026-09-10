//! `ArgAware` result-type resolvers and the type helpers they are built from.
//!
//! Pure `reify_core::Type` algebra. These are compile-time type computation,
//! never eval — the crate holds no `Value` (PRD decision 3).

use reify_core::{DimensionVector, Type};

/// Route the dimensionless case to `Type::dimensionless_scalar()` (NOT `Scalar{DIMENSIONLESS}`).
///
/// This matches the eval boundary: a dimensionless result produces
/// `Value::Real`, and `value_type_kind_matches(Value::Real,
/// Scalar{DIMENSIONLESS})` is `false` — so a dimensionless arm MUST return
/// `Type::dimensionless_scalar()` to keep the compile-type and eval-value in agreement.
///
/// Moved verbatim from `crates/reify-compiler/src/signatures_common.rs:27`
/// (task #6001 α), whose last user disappeared when the analysis family moved
/// to the registry. `math_signatures.rs` keeps its own bit-identical private
/// copy; deduping that is τ-numeric's, not α's.
pub fn scalar_or_real(dim: DimensionVector) -> Type {
    if dim.is_dimensionless() {
        Type::dimensionless_scalar()
    } else {
        Type::Scalar { dimension: dim }
    }
}

/// The quantity dimension carried by a `Tensor` / `Matrix` arg at position
/// `i`, defaulting to `DIMENSIONLESS` when the arg is absent or not a tensor.
///
/// Ported from `crates/reify-compiler/src/analysis_signatures.rs:96` and
/// re-signatured from `&[CompiledExpr]` to `&[Type]` — reify-builtins cannot
/// see reify-ir. Same `Tensor|Matrix ⇒ Scalar{dimension}` match, same
/// DIMENSIONLESS default.
///
/// Deliberately **non-recursive**, and deliberately blind to [`Type::Field`]:
/// that is [`field_tensor_arg`]'s job. Teaching this helper to look inside a
/// Field codomain would yield a `Scalar` compile-time type for a call eval
/// answers with a `Value::Field` — trading a dimension bug for a KIND LIE that
/// `value_type_kind_matches` (`crates/reify-eval/src/lib.rs:330`) rejects and
/// `engine_admin.rs` surfaces as `EngineError::TypeKindMismatch`.
fn tensor_quantity(args: &[Type], i: usize) -> DimensionVector {
    match args.get(i) {
        Some(Type::Tensor { quantity, .. }) | Some(Type::Matrix { quantity, .. }) => {
            match quantity.as_ref() {
                Type::Scalar { dimension } => *dimension,
                _ => DimensionVector::DIMENSIONLESS,
            }
        }
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// The `(domain, element dimension)` of arg `i` when it is a [`Type::Field`]
/// whose codomain is a 3x3 tensor/matrix of scalars. `None` otherwise.
///
/// Ported from `crates/reify-compiler/src/analysis_signatures.rs:204` (task
/// #6577) and re-signatured `&[CompiledExpr]` → `&[Type]`, the same
/// re-signaturing [`tensor_quantity`] already received.
///
/// Deliberately mirrors eval's gate — `analysis::tensor_element_dimension`
/// (`crates/reify-expr/src/analysis.rs`), reached via `validate_tensor_field` —
/// so the compile-time type and the `Value::Field` eval produces agree under
/// `value_type_kind_matches` (`crates/reify-eval/src/lib.rs:330`). The
/// [`Type::Int`] quantity branch is carried over for the same reason:
/// `tensor_element_dimension` maps it to `DIMENSIONLESS`.
///
/// The mirror covers eval's SHAPE gate only. `validate_tensor_field` also gates
/// on the `(source, lambda)` pair, admitting `(Analytical | Composed, Lambda)`
/// and — since task #7129 landed — `(Sampled, SampledField)`, which is the
/// backing `solve_elastic_static` hands back as `.stress`. A field's source kind
/// is not a type-level concept, so there is deliberately no counterpart to that
/// half here. It errs in the safe direction anyway: when the shape matches but
/// eval declines the pair, eval yields `Value::Undef`, which
/// `value_type_kind_matches` accepts for ANY type, so the `Type::Field` claim
/// still holds.
fn field_tensor_arg(args: &[Type], i: usize) -> Option<(&Type, DimensionVector)> {
    let Some(Type::Field { domain, codomain }) = args.get(i) else {
        return None;
    };
    let dim = match codomain.as_ref() {
        Type::Matrix {
            m: 3,
            n: 3,
            quantity,
        }
        | Type::Tensor {
            rank: 2,
            n: 3,
            quantity,
        } => match quantity.as_ref() {
            Type::Scalar { dimension } => *dimension,
            Type::Int => DimensionVector::DIMENSIONLESS,
            _ => return None,
        },
        _ => return None,
    };
    Some((domain.as_ref(), dim))
}

/// Reduce arg0, lifting the answer over a `Type::Field` argument.
///
/// Every analysis reduction has the same two-form shape and differs only in how
/// it turns arg0's element dimension into a result type, so the two forms live
/// here ONCE and the per-row variation is the `codomain` argument:
///
/// - arg0 is a `Field<D, Tensor<2,3,Q>>` → `Field<D, codomain(Q)>`;
/// - anything else → `codomain(tensor_quantity(arg0))`, the pre-#6577 answer.
///
/// # Why the Field form is a `Field` and not a reduced scalar (task #6577)
///
/// Eval does not reduce a field eagerly — it wraps it LAZILY and hands back a
/// `Value::Field` (`crates/reify-expr/src/analysis.rs`, `wrap_tensor_field`) —
/// and `value_type_kind_matches` (`crates/reify-eval/src/lib.rs:330`) maps a
/// `Value::Field` onto [`Type::Field`] alone. The consumer half needs no
/// counterpart: `max` / `min` already reduce a `Type::Field` codomain via
/// `reduce_field_codomain`, so `max(von_mises(stress))` is still
/// `Scalar<Pressure>`.
///
/// Returns `Some(..)` unconditionally: legacy behaviour never rejected an
/// argument shape, and α preserves it exactly. Wiring the `None` ⇒
/// `E_BuiltinArgShape` path is τ-numeric's work (PRD §3 decision 5).
fn reduce_tensor_arg(args: &[Type], codomain: fn(DimensionVector) -> Type) -> Option<Type> {
    if let Some((domain, dim)) = field_tensor_arg(args, 0) {
        return Some(Type::Field {
            domain: Box::new(domain.clone()),
            codomain: Box::new(codomain(dim)),
        });
    }
    Some(codomain(tensor_quantity(args, 0)))
}

/// `von_mises` / `max_shear`: scalar reduction carrying the tensor's quantity.
///
/// A Pressure tensor → `Scalar<Pressure>`; a dimensionless tensor → `Real`; a
/// Pressure tensor FIELD → `Field<D, Scalar<Pressure>>`. Mirrors `trace` /
/// `magnitude` in `math_fn_result_type`, and `wrap_tensor_field` for the Field
/// form.
pub(crate) fn tensor_scalar_reduction(args: &[Type]) -> Option<Type> {
    reduce_tensor_arg(args, scalar_or_real)
}

/// `principal_stresses`: the same reduction, wrapped in a `List`.
///
/// Mirrors the `eigenvalues` arm in `math_fn_result_type` (matrix → List of
/// eigenvalues). Over a Field the `List` sits INSIDE the `Field` —
/// `Field<D, List(Q)>`, not `List(Field<D, Q>)` — because eval samples the field
/// and each sample is the three eigenvalues (`compute_principal_stresses`,
/// `crates/reify-expr/src/analysis.rs:239-256`).
pub(crate) fn tensor_scalar_reduction_list(args: &[Type]) -> Option<Type> {
    reduce_tensor_arg(args, |dim| Type::List(Box::new(scalar_or_real(dim))))
}

/// `safety_factor`: dimensionless whatever the argument dimensions.
///
/// yield/von_mises cancels, so the codomain discards arg0's dimension entirely —
/// but the row is [`ResultSpec::ArgAware`](crate::row::ResultSpec::ArgAware)
/// rather than `Const` because the argument's SHAPE still reaches the answer: a
/// `Field` argument yields `Field<D, Real>`, since `compute_safety_factor`
/// (`crates/reify-expr/src/analysis.rs:273-302`) hands back a `Value::Field`
/// just as the other reductions do.
pub(crate) fn dimensionless_ratio(args: &[Type]) -> Option<Type> {
    reduce_tensor_arg(args, |_| Type::dimensionless_scalar())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dimensionless input → `Type::dimensionless_scalar()` (NOT `Type::Scalar{DIMENSIONLESS}`).
    #[test]
    fn dimensionless_routes_to_real() {
        assert_eq!(
            scalar_or_real(DimensionVector::DIMENSIONLESS),
            Type::dimensionless_scalar(),
            "scalar_or_real(DIMENSIONLESS) must be Type::dimensionless_scalar()"
        );
    }

    /// Pressure input → `Type::Scalar<PRESSURE>`.
    #[test]
    fn pressure_routes_to_scalar_pressure() {
        assert_eq!(
            scalar_or_real(DimensionVector::PRESSURE),
            Type::Scalar {
                dimension: DimensionVector::PRESSURE
            },
            "scalar_or_real(PRESSURE) must be Type::Scalar<PRESSURE>"
        );
    }

    /// `tensor_quantity` reads a `Tensor`'s and a `Matrix`'s quantity alike,
    /// and defaults to DIMENSIONLESS for absent / non-tensor / non-scalar-
    /// quantity args.
    #[test]
    fn tensor_quantity_reads_tensor_and_matrix_alike() {
        let pressure = Box::new(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        });
        let tensor = Type::Tensor {
            rank: 2,
            n: 3,
            quantity: pressure.clone(),
        };
        let matrix = Type::Matrix {
            m: 3,
            n: 3,
            quantity: pressure,
        };

        assert_eq!(
            tensor_quantity(&[tensor], 0),
            DimensionVector::PRESSURE,
            "Tensor quantity"
        );
        assert_eq!(
            tensor_quantity(&[matrix], 0),
            DimensionVector::PRESSURE,
            "Matrix quantity — legacy matched both in ONE arm"
        );

        assert_eq!(
            tensor_quantity(&[], 0),
            DimensionVector::DIMENSIONLESS,
            "absent arg defaults to DIMENSIONLESS"
        );
        assert_eq!(
            tensor_quantity(&[Type::String], 0),
            DimensionVector::DIMENSIONLESS,
            "non-tensor arg defaults to DIMENSIONLESS"
        );
        assert_eq!(
            tensor_quantity(
                &[Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(Type::String),
                }],
                0
            ),
            DimensionVector::DIMENSIONLESS,
            "a non-Scalar quantity defaults to DIMENSIONLESS"
        );
    }

    // ── #6577's Field-argument contract ──────────────────────────────────────
    //
    // Ported from `crates/reify-compiler/src/analysis_signatures.rs`'s test
    // module (main, blob 780f4cab83) when task #6001 α merged main forward and
    // resolved that file as DELETE. Re-expressed over `&[Type]`, since
    // reify-builtins cannot see `reify_ir::CompiledExpr`.
    //
    // Eval does NOT reduce a field eagerly — it wraps it LAZILY and hands back a
    // `Value::Field` (`compute_von_mises` / `compute_max_shear` →
    // `wrap_tensor_field`, crates/reify-expr/src/analysis.rs). Since
    // `value_type_kind_matches` (crates/reify-eval/src/lib.rs:330) maps a
    // `Value::Field` onto `Type::Field` and nothing else, answering with a
    // reduced `Scalar` here would trade a dimension bug for a KIND LIE that
    // `engine_admin.rs` surfaces as `EngineError::TypeKindMismatch`.

    /// The domain both Field fixtures carry — and the one every Field answer
    /// must hand back verbatim, never re-derived.
    fn field_domain() -> Type {
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        })
    }

    /// `Field<Point3<Length>, Tensor<2,3,quantity>>` — the compile-time type of
    /// `solve_elastic_static(..).stress`.
    fn tensor_field(quantity: Type) -> Type {
        Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::tensor(2, 3, quantity)),
        }
    }

    /// The same Field shape with a `Matrix{m:3,n:3}` codomain. Legacy matched
    /// `Tensor{rank:2,n:3}` and `Matrix{m:3,n:3}` in ONE arm, so both must be
    /// admitted identically.
    fn matrix_field(quantity: Type) -> Type {
        Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(quantity),
            }),
        }
    }

    fn pressure() -> Type {
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }
    }

    /// `Field<D, Tensor<2,3,Scalar<PRESSURE>>>` → `Field<D, Scalar<PRESSURE>>`.
    ///
    /// The resolver behind BOTH `von_mises` and `max_shear` (they share one
    /// row-level `ArgAware` resolver, exactly as they shared one legacy ladder
    /// arm). Mirrors `wrap_tensor_field` (analysis.rs:205-231, :263-265).
    #[test]
    fn a_pressure_tensor_field_reduces_to_a_pressure_field_not_a_scalar() {
        assert_eq!(
            tensor_scalar_reduction(&[tensor_field(pressure())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(pressure()),
            }),
            "a Field argument must yield a Field result carrying the tensor's \
             quantity — eval hands back a Value::Field, which \
             value_type_kind_matches maps onto Type::Field and nothing else"
        );
    }

    /// A `Matrix{m:3,n:3}` codomain is admitted exactly as the `Tensor` one is.
    #[test]
    fn a_matrix_codomain_field_is_admitted_exactly_as_a_tensor_codomain_is() {
        assert_eq!(
            tensor_scalar_reduction(&[matrix_field(pressure())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(pressure()),
            }),
            "legacy matched Tensor{{rank:2,n:3}} and Matrix{{m:3,n:3}} in ONE \
             arm — the Field prelude must not split them"
        );
    }

    /// A dimensionless tensor codomain routes through `scalar_or_real`, so the
    /// Field's codomain is `Type::dimensionless_scalar()` — NOT
    /// `Scalar{DIMENSIONLESS}`, for the reason `scalar_or_real` documents.
    #[test]
    fn a_dimensionless_tensor_field_yields_a_real_codomain_field() {
        assert_eq!(
            tensor_scalar_reduction(&[tensor_field(Type::dimensionless_scalar())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "the Field prelude must wrap the SAME scalar_or_real codomain the \
             concrete path computes, dimensionless case included"
        );
    }

    /// `principal_stresses(Field<D, Tensor<2,3,Scalar<PRESSURE>>>)` →
    /// `Field<D, List(Scalar<PRESSURE>)>`.
    ///
    /// A `List` codomain because sampling the field at a point yields three
    /// eigenvalues. Mirrors `compute_principal_stresses`
    /// (`crates/reify-expr/src/analysis.rs:239-256`).
    #[test]
    fn a_pressure_tensor_field_of_principal_stresses_is_a_field_of_lists() {
        assert_eq!(
            tensor_scalar_reduction_list(&[tensor_field(pressure())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(Type::List(Box::new(pressure()))),
            }),
            "principal_stresses over a Field must yield Field<D, List(Q)> — the \
             List sits INSIDE the Field, because eval samples the field and \
             each sample is the three eigenvalues"
        );
    }

    /// `safety_factor(Field<D, Tensor<2,3,Q>>, yield)` → `Field<D, Real>`.
    ///
    /// Dimensionless whatever the argument dimension — yield/von_mises cancels
    /// pointwise over a field exactly as it does for a scalar — but a `Field`
    /// nonetheless, because `compute_safety_factor`
    /// (`crates/reify-expr/src/analysis.rs:273-302`) hands back a
    /// `Value::Field`. That is why the row cannot stay `ResultSpec::Const`: the
    /// result is no longer arg-INDEPENDENT once a Field argument is admitted.
    #[test]
    fn a_pressure_tensor_field_of_safety_factor_is_a_field_of_reals() {
        assert_eq!(
            dimensionless_ratio(&[tensor_field(pressure()), pressure()]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "safety_factor over a Field must yield Field<D, Real> — the ratio \
             is dimensionless, but eval still wraps it as a Value::Field"
        );
    }

    // ── #6577's boundary guards ─────────────────────────────────────────────
    //
    // Ported from main's commit ce613ea07c. Each pins a fall-through the Field
    // prelude must NOT capture, so the prelude cannot later be "simplified" into
    // a claim eval cannot honour. Fall-through means the concrete-tensor ladder,
    // where `tensor_quantity` reads a `Type::Field` as DIMENSIONLESS — safe,
    // because eval answers those calls with `Value::Undef`, which
    // `value_type_kind_matches` accepts for ANY type.
    //
    // Several pass on arrival. For those, the comment records a MEASURED
    // falsifiability experiment (mutate the gate, observe the failure, revert)
    // rather than asserting the guard's value is self-evident.

    /// A `Field` whose codomain is not a 3x3 tensor/matrix falls through.
    ///
    /// Eval's `tensor_element_dimension` rejects the shape and returns
    /// `Value::Undef`, so claiming a `Field` result here would be a claim eval
    /// cannot honour.
    ///
    /// FALSIFIABILITY (measured): widening `field_tensor_arg`'s codomain match
    /// to a catch-all `_ => DimensionVector::DIMENSIONLESS` makes this test fail
    /// with `left: Some(Field { .. })`, `right: Some(Scalar{DIMENSIONLESS})`.
    /// Reverted.
    #[test]
    fn a_field_whose_codomain_is_not_a_3x3_tensor_falls_through() {
        let vec3_codomain = Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        };
        assert_eq!(
            tensor_scalar_reduction(&[vec3_codomain]),
            Some(Type::dimensionless_scalar()),
            "a Vector3 codomain must NOT take the Field arm — eval's \
             tensor_element_dimension rejects it and yields Value::Undef"
        );

        let wrong_rank = Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::tensor(2, 2, pressure())),
        };
        assert_eq!(
            tensor_scalar_reduction(&[wrong_rank]),
            Some(Type::dimensionless_scalar()),
            "a 2x2 tensor codomain must NOT take the Field arm — eval's gate \
             requires n = 3"
        );

        let non_scalar_quantity = Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::tensor(2, 3, Type::String)),
        };
        assert_eq!(
            tensor_scalar_reduction(&[non_scalar_quantity]),
            Some(Type::dimensionless_scalar()),
            "a non-Scalar, non-Int codomain quantity must NOT take the Field arm"
        );
    }

    /// A `Type::Int` codomain quantity DOES take the Field arm, mapping to
    /// DIMENSIONLESS rather than declining.
    ///
    /// Carried over from eval for the same reason the rest of the gate is:
    /// `tensor_element_dimension` maps `Int` to DIMENSIONLESS, so declining here
    /// would under-claim a call eval really does answer with a `Value::Field`.
    ///
    /// FALSIFIABILITY (measured): deleting the `Type::Int => DIMENSIONLESS` arm
    /// from `field_tensor_arg` makes this test fail with
    /// `left: Some(Scalar{DIMENSIONLESS})`, `right: Some(Field { .. })`.
    /// Reverted.
    #[test]
    fn an_int_codomain_quantity_maps_to_dimensionless_rather_than_declining() {
        let int_quantity = Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::tensor(2, 3, Type::Int)),
        };
        assert_eq!(
            tensor_scalar_reduction(&[int_quantity]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "an Int codomain quantity must still take the Field arm — eval's \
             tensor_element_dimension maps Int to DIMENSIONLESS"
        );
    }

    /// Only arg **0** is read as the shape source.
    ///
    /// Measured at `safety_factor`'s arity so the arity gate cannot confound the
    /// result: at argc 2 the Field arm IS admissible, and the answer is still
    /// the concrete one because slot 0 holds a concrete tensor.
    ///
    /// FALSIFIABILITY (measured): changing `reduce_tensor_arg`'s prelude to scan
    /// every slot (`(0..args.len()).find_map(|i| field_tensor_arg(args, i))`)
    /// makes this test fail with `left: Some(Field { .. })`,
    /// `right: Some(Scalar{DIMENSIONLESS})`. Reverted.
    #[test]
    fn a_field_at_a_non_zero_slot_is_not_read_as_the_shape_source() {
        assert_eq!(
            dimensionless_ratio(&[Type::tensor(2, 3, pressure()), tensor_field(pressure())]),
            Some(Type::dimensionless_scalar()),
            "a Field at slot 1 must not make the result a Field — eval reads the \
             stress argument at slot 0"
        );
    }

    /// The unary reductions admit the Field arm ONLY at argc 1.
    ///
    /// Eval's own dispatch condition is `evaluated_args.len() == 1`
    /// (`crates/reify-expr/src/lib.rs` ladder), so a 2-arg call falls through to
    /// the concrete ladder. Keeping the compiler's claim narrower-or-equal to
    /// what eval can honour is the whole point of mirroring the gate.
    #[test]
    fn the_unary_reductions_admit_the_field_arm_only_at_argc_1() {
        let extra = [tensor_field(pressure()), pressure()];

        assert_eq!(
            tensor_scalar_reduction(&extra),
            Some(Type::dimensionless_scalar()),
            "von_mises / max_shear at argc 2 must NOT take the Field arm — \
             eval's dispatch gate is evaluated_args.len() == 1"
        );
        assert_eq!(
            tensor_scalar_reduction_list(&extra),
            Some(Type::List(Box::new(Type::dimensionless_scalar()))),
            "principal_stresses at argc 2 must NOT take the Field arm, and must \
             still answer with the concrete ladder's List"
        );
    }

    /// Mirror image: `safety_factor` admits the Field arm ONLY at argc 2.
    ///
    /// Eval dispatches `compute_safety_factor` on a Field at
    /// `evaluated_args.len() == 2`; a 1-arg call falls through.
    #[test]
    fn safety_factor_admits_the_field_arm_only_at_argc_2() {
        assert_eq!(
            dimensionless_ratio(&[tensor_field(pressure())]),
            Some(Type::dimensionless_scalar()),
            "safety_factor at argc 1 must NOT take the Field arm — eval's \
             dispatch gate is evaluated_args.len() == 2"
        );
    }
}
