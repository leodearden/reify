//! The PRD §7.1 row vocabulary: [`BuiltinRow`] and the column types it is
//! built from.

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DimensionVector, Type};

    /// Stand-in for a macro-generated `BuiltinId`. [`BuiltinRow`] is generic
    /// over its id type precisely so a `registry!` invocation can mint its own
    /// enum (see `macros.rs`); this local stub exercises that generality
    /// without depending on the real seed table.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum StubId {
        Alpha,
    }

    // ── Family ───────────────────────────────────────────────────────────────

    /// α seeds exactly two families. Both variants must exist and be
    /// distinguishable.
    #[test]
    fn family_has_the_two_seed_variants() {
        assert_ne!(
            Family::Parse,
            Family::Analysis,
            "Parse and Analysis must be distinct Family variants"
        );
    }

    // ── BindingKind ──────────────────────────────────────────────────────────

    /// All five PRD decision-4 binding kinds are declared now, even though α
    /// seeds only `EvalBuiltin`. The kind column must cover the full ~358-name
    /// surface — a later τ adding `GeometryOp` rows must not also have to widen
    /// the vocabulary.
    #[test]
    fn binding_kind_declares_all_five_decision_4_variants() {
        let all = [
            BindingKind::EvalBuiltin,
            BindingKind::ExprIntercept,
            BindingKind::EnginePostProcess,
            BindingKind::GeometryOp,
            BindingKind::CompileOnly,
        ];
        // Pairwise-distinct: catches a copy-paste that aliases two variants.
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "BindingKind variants must be pairwise distinct");
            }
        }
    }

    // ── Arity ────────────────────────────────────────────────────────────────

    /// `Exact(n)` matches exactly `n` args.
    #[test]
    fn arity_exact_matches_only_its_own_argc() {
        assert!(Arity::Exact(1).matches(1), "Exact(1) must match argc 1");
        assert!(!Arity::Exact(1).matches(0), "Exact(1) must reject argc 0");
        assert!(!Arity::Exact(1).matches(2), "Exact(1) must reject argc 2");
        assert!(Arity::Exact(0).matches(0), "Exact(0) must match argc 0");
    }

    /// `Range(lo, hi)` matches inclusively on both ends.
    #[test]
    fn arity_range_matches_inclusively() {
        let r = Arity::Range(1, 2);
        assert!(!r.matches(0), "Range(1,2) must reject argc 0");
        assert!(r.matches(1), "Range(1,2) must match its lower bound");
        assert!(r.matches(2), "Range(1,2) must match its upper bound");
        assert!(!r.matches(3), "Range(1,2) must reject argc 3");
    }

    /// `Variadic` matches any argc, including zero.
    #[test]
    fn arity_variadic_matches_any_argc() {
        assert!(Arity::Variadic.matches(0), "Variadic must match argc 0");
        assert!(Arity::Variadic.matches(7), "Variadic must match argc 7");
    }

    // ── ArgSlot ──────────────────────────────────────────────────────────────

    /// α needs only `Any`. The per-slot dimension vocabulary arrives in
    /// τ-numeric; this pins that the column exists and is constructible.
    #[test]
    fn arg_slot_any_exists_and_is_comparable() {
        assert_eq!(ArgSlot::Any, ArgSlot::Any);
    }

    // ── ResultSpec ───────────────────────────────────────────────────────────

    /// `Const` resolves to a clone of its declared type, whatever the args.
    #[test]
    fn result_spec_const_resolves_to_its_type_regardless_of_args() {
        let spec = ResultSpec::Const(Type::Option(Box::new(Type::length())));
        let expected = Type::Option(Box::new(Type::length()));

        assert_eq!(spec.resolve(&[]), Some(expected.clone()));
        assert_eq!(spec.resolve(&[Type::String]), Some(expected));
    }

    /// `ArgAware` delegates to its fn pointer, args and all.
    #[test]
    fn result_spec_arg_aware_calls_its_resolver() {
        fn first_arg_or_none(args: &[Type]) -> Option<Type> {
            args.first().cloned()
        }
        let spec = ResultSpec::ArgAware(first_arg_or_none);

        assert_eq!(
            spec.resolve(&[Type::Scalar {
                dimension: DimensionVector::PRESSURE
            }]),
            Some(Type::Scalar {
                dimension: DimensionVector::PRESSURE
            }),
            "ArgAware must forward the args to its resolver"
        );
        assert_eq!(
            spec.resolve(&[]),
            None,
            "an ArgAware resolver returning None must surface as None"
        );
    }

    // ── Basis (PRD §7.1 / I-REG-7 ratified closed vocabulary) ────────────────

    /// The four ratified `Basis` variants exist, and only `Artifact` reports
    /// itself as an artifact.
    #[test]
    fn basis_declares_the_four_ratified_variants() {
        assert!(
            !Basis::Ruling("#4535").is_artifact(),
            "Ruling is a justified basis"
        );
        assert!(
            !Basis::Physics("pressure cancels").is_artifact(),
            "Physics is a justified basis"
        );
        assert!(
            !Basis::Doc("docs/prds/v0_6/builtin-signature-registry.md").is_artifact(),
            "Doc is a justified basis"
        );
        assert!(
            Basis::Artifact.is_artifact(),
            "Artifact must report itself — it is the ratchet metric I-REG-7 counts"
        );
    }

    // ── BuiltinRow ───────────────────────────────────────────────────────────

    /// The row carries all eight §7.1 columns and is constructible.
    #[test]
    fn builtin_row_carries_all_eight_prd_columns() {
        let row: BuiltinRow<StubId> = BuiltinRow {
            name: "parse_length",
            id: StubId::Alpha,
            family: Family::Parse,
            binding: BindingKind::EvalBuiltin,
            arity: Arity::Exact(1),
            arg_slots: &[ArgSlot::Any],
            result: ResultSpec::Const(Type::Option(Box::new(Type::length()))),
            basis: Basis::Ruling("#4535"),
        };

        assert_eq!(row.name, "parse_length");
        assert_eq!(row.id, StubId::Alpha);
        assert_eq!(row.family, Family::Parse);
        assert_eq!(row.binding, BindingKind::EvalBuiltin);
        assert_eq!(row.arity, Arity::Exact(1));
        assert_eq!(row.arg_slots, [ArgSlot::Any].as_slice());
        assert_eq!(
            row.result.resolve(&[Type::String]),
            Some(Type::Option(Box::new(Type::length())))
        );
        assert_eq!(row.basis, Basis::Ruling("#4535"));
    }

    /// The row's arity and result columns compose the way the dispatchers use
    /// them: gate on `matches(argc)`, then `resolve(args)`.
    #[test]
    fn builtin_row_arity_gates_before_result_resolution() {
        let row: BuiltinRow<StubId> = BuiltinRow {
            name: "safety_factor",
            id: StubId::Alpha,
            family: Family::Analysis,
            binding: BindingKind::EvalBuiltin,
            arity: Arity::Exact(2),
            arg_slots: &[ArgSlot::Any, ArgSlot::Any],
            result: ResultSpec::Const(Type::dimensionless_scalar()),
            basis: Basis::Physics("yield/von_mises — pressure cancels"),
        };

        assert!(row.arity.matches(2), "safety_factor is a 2-arg builtin");
        assert!(!row.arity.matches(1), "1 arg must not match Exact(2)");
        assert_eq!(
            row.result
                .resolve(&[Type::dimensionless_scalar(), Type::dimensionless_scalar()]),
            Some(Type::dimensionless_scalar())
        );
    }
}
