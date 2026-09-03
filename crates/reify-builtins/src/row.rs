//! The PRD §7.1 row vocabulary: [`BuiltinRow`] and the column types it is
//! built from.
//!
//! One [`BuiltinRow`] per registered builtin. Same-name arity overloads
//! (`offset`@2/@3, `floor`@1/@2) are DISTINCT rows sharing a name-group;
//! `lookup(name, argc)` disambiguates them.
//!
//! Everything here is pure `reify_core::Type` algebra — no `Value`, no eval fn
//! pointers (PRD decision 3). The [`ResultSpec::ArgAware`] fn pointer is
//! `fn(&[Type]) -> Option<Type>`: compile-time type computation, never eval.

use reify_core::Type;

/// Which builtin family a row belongs to.
///
/// The family column groups rows for reporting and for the per-family
/// migration order the PRD lays out; it is deliberately NOT a dispatch key.
/// Dispatch is keyed on [`BindingKind`] (via the generated per-kind sub-enums),
/// which is what lets an owning crate bind its rows exhaustively without
/// re-deriving membership from a name list.
///
/// α seeds two families; each later τ migration adds its own variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Fallible string→quantity parse builtins (task #4535).
    Parse,
    /// FEA stress-analysis reduction builtins (FEA-5, task #2884).
    Analysis,
}

/// How a builtin is bound to an implementation — PRD §3 decision 4.
///
/// Exhaustiveness is enforced **per kind, in the kind's owning crate**: the
/// `registry!` macro emits one sub-enum per kind present in the invocation, and
/// that crate matches on the sub-enum with no `_` arm (I-REG-2). A flat
/// `BuiltinId` match would instead force every owning crate to name all ~358
/// eventual variants — including ones it has no business knowing — just to stay
/// `_`-free.
///
/// All five variants are declared now even though α seeds only
/// [`BindingKind::EvalBuiltin`], so a later τ adds rows rather than widening
/// the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// Dispatched by `reify_stdlib::eval_builtin`.
    EvalBuiltin,
    /// Intercepted natively in reify-expr (needs an `EvalContext`).
    ExprIntercept,
    /// Geometry queries / selectors / kinematics, via reify-eval's maps.
    EnginePostProcess,
    /// Lowered to a `CompiledGeometryOp` in reify-ir.
    GeometryOp,
    /// Relations and markers — no runtime binding at all.
    CompileOnly,
}

/// How many arguments a row accepts.
///
/// This is the row's own declared shape. It is NOT, in α, a diagnostic gate:
/// the compiler ladder is arity-insensitive today, and preserving that exactly
/// is a hard constraint of the seed migration (see
/// `reify-compiler`'s `builtin_registry::registry_result_type`). Real arity
/// diagnostics arrive with the first genuine overload in τ-numeric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arity {
    /// Exactly `n` arguments.
    Exact(usize),
    /// Between `lo` and `hi` arguments, both bounds inclusive.
    Range(usize, usize),
    /// Any number of arguments, including none.
    Variadic,
}

impl Arity {
    /// Does an `argc`-argument call match this arity?
    pub fn matches(&self, argc: usize) -> bool {
        match self {
            Arity::Exact(n) => argc == *n,
            Arity::Range(lo, hi) => argc >= *lo && argc <= *hi,
            Arity::Variadic => true,
        }
    }
}

/// A per-argument-slot constraint.
///
/// α needs only [`ArgSlot::Any`]: the seed families' arg checking is unchanged
/// from today, and today there is none at the slot level.
///
/// The real vocabulary — the existing `check_builtin_arg_types` per-slot
/// dimension checks, plus the ratified `SameDimensionAs(slot)` constraint that
/// `floor`@2 / `atan2` / `remap` need to say "both args share a free dimension
/// D" — migrates into this column in **τ-numeric** (PRD §3 decision 5). The
/// column is present-but-minimal rather than absent so rows written now do not
/// need reshaping then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgSlot {
    /// No constraint on this slot.
    Any,
}

/// How a row's result type is computed — PRD §3 decision 5.
pub enum ResultSpec {
    /// The result type is the same whatever the args are.
    Const(Type),
    /// The result type is computed from the argument types.
    ///
    /// A `None` return means "these args are mis-shaped". In α that never
    /// happens (the seed resolvers are total, reproducing legacy behaviour that
    /// never rejected); wiring `None` to an `E_BuiltinArgShape` diagnostic
    /// instead of a silent first-arg guess is τ-numeric's work.
    ArgAware(fn(&[Type]) -> Option<Type>),
}

impl ResultSpec {
    /// Compute this row's result type for a call with the given argument types.
    pub fn resolve(&self, args: &[Type]) -> Option<Type> {
        match self {
            ResultSpec::Const(ty) => Some(ty.clone()),
            ResultSpec::ArgAware(f) => f(args),
        }
    }

    /// This row's declared type when it is [`ResultSpec::Const`], `None` for an
    /// [`ResultSpec::ArgAware`] row.
    ///
    /// # Why this exists instead of a `PartialEq` impl
    ///
    /// `ResultSpec` deliberately implements **no** `PartialEq`, and adding one
    /// later would be a mistake. Rust compares `fn` pointers by address, which
    /// is not a meaningful identity — the compiler may merge two
    /// identically-bodied resolvers into one address or duplicate one across
    /// codegen units — so `==` on `ArgAware` would answer a question about
    /// codegen, not about signatures. An impl that papered over that by
    /// answering `false` for `ArgAware` would be **non-reflexive**:
    /// `spec == spec` would be `false` for three of the seven seed rows, so
    /// every later `contains` / `dedup` / `assert_eq!` / derived comparison
    /// reaching a `ResultSpec` would misbehave silently and surface as a
    /// baffling test failure rather than a compile error.
    ///
    /// So the comparable part is exposed explicitly instead: a caller that
    /// wants to compare `Const` payloads asks for them and gets an `Option`
    /// that is honest about the `ArgAware` case. Callers wanting behavioural
    /// equivalence compare [`resolve`](ResultSpec::resolve) outputs over
    /// chosen argument types — which is what the seed tests do, and what
    /// actually pins a signature. Row identity is [`BuiltinRow::id`], never
    /// structural equality.
    pub fn const_type(&self) -> Option<&Type> {
        match self {
            ResultSpec::Const(ty) => Some(ty),
            ResultSpec::ArgAware(_) => None,
        }
    }
}

impl std::fmt::Debug for ResultSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResultSpec::Const(ty) => f.debug_tuple("Const").field(ty).finish(),
            // A fn pointer's address is noise in a failure message; the row's
            // name (which the caller has) is what identifies the resolver.
            ResultSpec::ArgAware(_) => f.write_str("ArgAware(<fn>)"),
        }
    }
}

// NOTE: no `PartialEq for ResultSpec`, deliberately — see
// [`ResultSpec::const_type`] for why an `ArgAware`-aware impl would have to be
// non-reflexive, and what to compare instead.

/// Why a row's signature is what it is — the ratified I-REG-7 closed
/// vocabulary (PRD §7.1, ratified 2026-08-07).
///
/// Every row must carry one; the registry-crate lint test enforces presence and
/// reports the [`Basis::Artifact`] count as a visible ratchet toward zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Basis {
    /// A decision task ruled this signature. Cites ONE task in canonical
    /// `#NNNN` form, however many artifacts echo it.
    Ruling(&'static str),
    /// A physical derivation fixes this signature.
    Physics(&'static str),
    /// A design document DECIDED this signature (not merely recorded it).
    Doc(&'static str),
    /// "Preserves today's behaviour; no independent justification."
    ///
    /// Legal but conspicuous. Per the ratified I-REG-7, an `Artifact` row may
    /// **not** cite implementation behaviour as its justification — the tag
    /// itself already declares exactly that.
    Artifact,
}

impl Basis {
    /// Is this the unjustified [`Basis::Artifact`] tag?
    ///
    /// The count of rows answering `true` is I-REG-7's reviewed ratchet toward
    /// zero.
    pub fn is_artifact(&self) -> bool {
        matches!(self, Basis::Artifact)
    }
}

/// One registered builtin — the PRD §7.1 row.
///
/// Generic over its id type because each `registry!` invocation mints its own
/// `BuiltinId` enum — including the test-local invocations that keep the macro
/// honest. The crate's real registry supplies the default type parameter, so
/// consumers simply write `reify_builtins::BuiltinRow`.
#[derive(Debug)]
pub struct BuiltinRow<Id: Copy + 'static = crate::BuiltinId> {
    /// The name a `.ri` author writes.
    pub name: &'static str,
    /// The generated key this row is dispatched on.
    pub id: Id,
    /// Which builtin family this row belongs to.
    pub family: Family,
    /// Which dispatcher owns this row's implementation.
    pub binding: BindingKind,
    /// How many arguments this row accepts.
    pub arity: Arity,
    /// Per-slot argument constraints, one entry per declared slot.
    pub arg_slots: &'static [ArgSlot],
    /// How this row's result type is computed.
    pub result: ResultSpec,
    /// Why this row's signature is what it is (I-REG-7).
    pub basis: Basis,
}

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

    /// `const_type` is the comparison accessor that stands in for the
    /// `PartialEq` impl this type deliberately does not have: it hands back the
    /// `Const` payload and is honest — rather than silently false — about
    /// `ArgAware`, whose fn-pointer address is a codegen artefact and not an
    /// identity.
    #[test]
    fn result_spec_const_type_exposes_const_payloads_and_declines_for_arg_aware() {
        fn first_arg_or_none(args: &[Type]) -> Option<Type> {
            args.first().cloned()
        }

        let konst = ResultSpec::Const(Type::Option(Box::new(Type::length())));
        assert_eq!(
            konst.const_type(),
            Some(&Type::Option(Box::new(Type::length()))),
            "a Const row must expose its declared type for direct comparison"
        );

        assert_eq!(
            ResultSpec::ArgAware(first_arg_or_none).const_type(),
            None,
            "an ArgAware row has no const payload — callers must compare \
             `resolve` outputs over chosen arg types instead of reaching for `==`"
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
