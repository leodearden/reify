//! The registry: α's seed rows (parse + analysis).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{Arity, BindingKind, Family};
    use reify_core::{DimensionVector, Type};

    /// The 7 α seed names, in declaration order.
    const SEED_NAMES: [&str; 7] = [
        "parse_length",
        "parse_length_r",
        "von_mises",
        "max_shear",
        "principal_stresses",
        "safety_factor",
        "stress_invariants",
    ];

    fn scalar(dim: DimensionVector) -> Type {
        Type::Scalar { dimension: dim }
    }

    /// `Tensor{rank:2, n:3, quantity: Scalar<PRESSURE>}` — a stress tensor.
    fn pressure_tensor() -> Type {
        Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(scalar(DimensionVector::PRESSURE)),
        }
    }

    fn dimensionless_tensor() -> Type {
        Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }
    }

    /// `Matrix{m:3, n:3, quantity: Scalar<PRESSURE>}` — legacy `tensor_quantity`
    /// matched `Tensor` and `Matrix` in ONE arm, so both must resolve alike.
    fn pressure_matrix() -> Type {
        Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(scalar(DimensionVector::PRESSURE)),
        }
    }

    fn row_named(name: &str) -> &'static crate::row::BuiltinRow<BuiltinId> {
        rows()
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no seed row named {name:?}"))
    }

    /// Resolve a row's result type for the given args.
    fn resolve(name: &str, args: &[Type]) -> Option<Type> {
        row_named(name).result.resolve(args)
    }

    // ── table shape ──────────────────────────────────────────────────────────

    /// α seeds exactly 7 rows, in declaration order.
    #[test]
    fn the_seed_table_holds_exactly_the_seven_alpha_rows() {
        assert_eq!(
            rows().iter().map(|r| r.name).collect::<Vec<_>>(),
            SEED_NAMES.to_vec(),
            "α seeds the parse family (2) and the analysis family (5)"
        );
    }

    /// Every seed row is an `EvalBuiltin`, in its declared family.
    #[test]
    fn seed_rows_carry_their_family_and_binding_kind() {
        for name in ["parse_length", "parse_length_r"] {
            assert_eq!(row_named(name).family, Family::Parse, "{name}");
        }
        for name in [
            "von_mises",
            "max_shear",
            "principal_stresses",
            "safety_factor",
            "stress_invariants",
        ] {
            assert_eq!(row_named(name).family, Family::Analysis, "{name}");
        }
        for name in SEED_NAMES {
            assert_eq!(
                row_named(name).binding,
                BindingKind::EvalBuiltin,
                "{name} is dispatched by reify_stdlib::eval_builtin"
            );
        }
    }

    /// Arities are derived from the EVAL kernels, not guessed:
    /// `crates/reify-stdlib/src/analysis.rs` builds von_mises /
    /// principal_stresses / max_shear / stress_invariants on `helpers::unary`
    /// and safety_factor on `helpers::binary`; `parse.rs`'s arms take
    /// `single_string_arg`.
    #[test]
    fn seed_arities_match_their_eval_kernels() {
        for name in [
            "parse_length",
            "parse_length_r",
            "von_mises",
            "max_shear",
            "principal_stresses",
            "stress_invariants",
        ] {
            assert_eq!(
                row_named(name).arity,
                Arity::Exact(1),
                "{name} is a 1-arg kernel"
            );
        }
        assert_eq!(
            row_named("safety_factor").arity,
            Arity::Exact(2),
            "safety_factor is helpers::binary — (tensor, yield)"
        );
    }

    // ── result types: every one reproduces today's legacy value verbatim ─────

    /// `parse_length -> Option<Length>` (task #4535).
    #[test]
    fn parse_length_resolves_to_option_length() {
        assert_eq!(
            resolve("parse_length", &[Type::String]),
            Some(Type::Option(Box::new(Type::length())))
        );
    }

    /// `parse_length_r -> Result` — the PRELUDE `Result<T,E>` of task #4035,
    /// registered as `Type::Enum("Result")`.
    #[test]
    fn parse_length_r_resolves_to_enum_result() {
        assert_eq!(
            resolve("parse_length_r", &[Type::String]),
            Some(Type::Enum("Result".to_string()))
        );
    }

    /// `von_mises` / `max_shear` reduce the tensor to a scalar carrying its
    /// quantity.
    #[test]
    fn von_mises_and_max_shear_reduce_to_the_tensors_quantity() {
        for name in ["von_mises", "max_shear"] {
            assert_eq!(
                resolve(name, &[pressure_tensor()]),
                Some(scalar(DimensionVector::PRESSURE)),
                "{name} over a Pressure tensor must yield Scalar<PRESSURE>"
            );
            assert_eq!(
                resolve(name, &[dimensionless_tensor()]),
                Some(Type::dimensionless_scalar()),
                "{name} over a dimensionless tensor must yield \
                 Type::dimensionless_scalar(), NOT Scalar<DIMENSIONLESS> — \
                 the scalar_or_real eval boundary"
            );
        }
    }

    /// `principal_stresses` is the same reduction wrapped in a `List`.
    #[test]
    fn principal_stresses_resolves_to_a_list_of_the_reduced_scalar() {
        assert_eq!(
            resolve("principal_stresses", &[pressure_tensor()]),
            Some(Type::List(Box::new(scalar(DimensionVector::PRESSURE))))
        );
        assert_eq!(
            resolve("principal_stresses", &[dimensionless_tensor()]),
            Some(Type::List(Box::new(Type::dimensionless_scalar())))
        );
    }

    /// `safety_factor` is dimensionless whatever its args — yield/von_mises,
    /// pressure cancels.
    #[test]
    fn safety_factor_is_always_dimensionless() {
        assert_eq!(
            resolve(
                "safety_factor",
                &[pressure_tensor(), scalar(DimensionVector::PRESSURE)]
            ),
            Some(Type::dimensionless_scalar())
        );
    }

    /// `stress_invariants` returns the `StressInvariants` structure declared in
    /// `crates/reify-compiler/stdlib/fea.ri`.
    #[test]
    fn stress_invariants_resolves_to_its_structure_ref() {
        assert_eq!(
            resolve("stress_invariants", &[pressure_tensor()]),
            Some(Type::StructureRef("StressInvariants".to_string()))
        );
    }

    /// Legacy `tensor_quantity` matched `Tensor` and `Matrix` in one arm, so a
    /// `Matrix` arg must resolve identically to the equivalent `Tensor`.
    #[test]
    fn matrix_args_resolve_identically_to_tensor_args() {
        for name in ["von_mises", "max_shear", "principal_stresses"] {
            assert_eq!(
                resolve(name, &[pressure_matrix()]),
                resolve(name, &[pressure_tensor()]),
                "{name}: Matrix and Tensor carry the quantity the same way"
            );
        }
    }

    /// A non-tensor arg0 and an EMPTY arg slice both fall to DIMENSIONLESS —
    /// the legacy default — rather than panicking or returning `None`.
    ///
    /// α's resolvers are deliberately TOTAL: legacy behaviour never rejected,
    /// and wiring `None` to an `E_BuiltinArgShape` diagnostic is τ-numeric's.
    #[test]
    fn mis_shaped_and_absent_args_fall_to_the_legacy_dimensionless_default() {
        for name in ["von_mises", "max_shear"] {
            assert_eq!(
                resolve(name, &[Type::String]),
                Some(Type::dimensionless_scalar()),
                "{name} over a non-tensor must not panic"
            );
            assert_eq!(
                resolve(name, &[]),
                Some(Type::dimensionless_scalar()),
                "{name} with no args must not panic"
            );
        }
        assert_eq!(
            resolve("principal_stresses", &[]),
            Some(Type::List(Box::new(Type::dimensionless_scalar())))
        );
    }

    // ── lookup / name_group over the seeds ───────────────────────────────────

    /// Each seed name resolves at its declared arity and NOWHERE else in
    /// 0..=3.
    #[test]
    fn lookup_resolves_each_seed_at_its_arity_only() {
        for name in SEED_NAMES {
            let declared = match row_named(name).arity {
                Arity::Exact(n) => n,
                other => panic!("α seeds only Exact arities, {name} has {other:?}"),
            };
            for argc in 0..=3 {
                let got = lookup(name, argc);
                if argc == declared {
                    assert_eq!(
                        got,
                        Some(row_named(name).id),
                        "{name}@{argc} must resolve to its own row"
                    );
                } else {
                    assert_eq!(got, None, "{name}@{argc} must not resolve");
                }
            }
        }
        assert_eq!(lookup("not_a_builtin", 1), None);
    }

    /// **α has ZERO same-name overloads.**
    ///
    /// This is the precondition the compiler's arity-insensitive
    /// single-row fallback relies on (`registry_result_type`'s
    /// `group.len() == 1` guard), so it is pinned explicitly rather than left
    /// implicit in the row list.
    #[test]
    fn every_seed_name_group_holds_exactly_one_row() {
        for name in SEED_NAMES {
            assert_eq!(
                name_group(name).len(),
                1,
                "{name} must have exactly one row — α seeds no arity overloads, \
                 and the compiler's single-row arity-parity fallback is only \
                 sound while that holds"
            );
        }
        assert!(name_group("not_a_builtin").is_empty());
    }
}
