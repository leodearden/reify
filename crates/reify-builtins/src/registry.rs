//! The registry: α's seed rows (parse + analysis).
//!
//! Every result type below reproduces today's legacy value VERBATIM. α makes
//! zero corrections (PRD §7.3(6)) — the seed migration's job is to move the
//! source of truth, not to change any answer. Each row's `//` comment names
//! where its type came from, with a `file.rs:LINE` anchor and the originating
//! task, in the multi-row ledger dialect of
//! `crates/reify-eval/src/registry_drift_tests.rs:239-274`.

use reify_core::Type;

use crate::resolvers::{tensor_scalar_reduction, tensor_scalar_reduction_list};

crate::macros::registry! {
    EvalBuiltin => EvalBuiltinId, as_eval_builtin {
        // ── Family::Parse (task #4535) ──────────────────────────────────────
        //
        // Both rows are arg-INDEPENDENT: `eval_parse` returns the same VARIANT
        // for a given name whatever the (String) argument's value, so `Const`
        // is exact rather than an approximation.

        // parse_length -> Option<Length>.
        // Legacy: crates/reify-compiler/src/parse_signatures.rs:51.
        // #4535 rules the Option shape; the ladder arm exists because the
        // first-arg fallback would otherwise type this as the arg's own
        // Type::String, breaking both the consumer's match{Some/None} check
        // and the eval-time value_type_kind_matches guard (the eval'd value is
        // a real Value::Option, never a Value::String).
        ParseLength {
            name: "parse_length",
            family: Parse,
            arity: Exact(1),
            arg_slots: [Any],
            result: Const(Type::Option(Box::new(Type::length()))),
            basis: Ruling("#4535")
        },

        // parse_length_r -> the PRELUDE Result<T,E> of dependency task #4035,
        // registered as Type::Enum("Result") so the eval-time
        // value_type_kind_matches guard passes against the
        // Value::Enum{type_name:"Result", ..} that eval_parse constructs.
        // Legacy: crates/reify-compiler/src/parse_signatures.rs:52.
        ParseLengthR {
            name: "parse_length_r",
            family: Parse,
            arity: Exact(1),
            arg_slots: [Any],
            result: Const(Type::Enum("Result".to_string())),
            basis: Ruling("#4535")
        },

        // ── Family::Analysis (FEA-5, task #2884) ────────────────────────────
        //
        // Arities come from the eval kernels, not from the compiler: four of
        // the five are built on `helpers::unary` and safety_factor on
        // `helpers::binary` (crates/reify-stdlib/src/analysis.rs).

        // von_mises -> scalar_or_real(tensor_quantity(arg0)).
        // Legacy: crates/reify-compiler/src/analysis_signatures.rs:73.
        //
        // ARTIFACT (ratified 2026-08-17, Leo, unblock of #6001). #2884 rules
        // BOTH halves of this signature: `von_mises(stress: Tensor<2,3,Pressure>)
        // -> Pressure` — a constrained input slot AND a fixed Pressure result.
        // This row implements NEITHER: `arg_slots: [Any]` accepts any argument,
        // and the ArgAware form generalises the result to the tensor's own
        // quantity. That generalisation exists only to keep accepting inputs
        // #2884 says should be rejected, so it cannot cite #2884 — nor the
        // Q7/#6165 restatement of #2884's output half, which drops the input
        // half and would fabricate a PRESSURE result for a dimensionless
        // argument. Left unresolved here because the end-state needs machinery α
        // does not have: an `ArgSlot` vocabulary richer than `Any` and
        // `ResultSpec::ArgAware`'s `None` wired to an `E_BuiltinArgShape`
        // diagnostic (both τ-numeric, PRD §3 decision 5), plus the corpus
        // migration of examples/fields_analysis.ri owned by leaf θ of
        // docs/prds/v0_6/dimension-checked-readers.md.
        VonMises {
            name: "von_mises",
            family: Analysis,
            arity: Exact(1),
            arg_slots: [Any],
            result: ArgAware(tensor_scalar_reduction),
            basis: Artifact
        },

        // max_shear -> the same reduction as von_mises.
        // Legacy: crates/reify-compiler/src/analysis_signatures.rs:73 (shared arm).
        //
        // ARTIFACT (ratified 2026-08-17, Leo, unblock of #6001). NOT ruled by
        // #2884 — that task's text names von_mises, principal_stresses and
        // stress_invariants only. The former Physics cite ("max shear =
        // (σ₁−σ₃)/2 is a stress, so it carries the tensor's quantity") justifies
        // the ArgAware GENERALISATION, not the signature: max shear is a stress,
        // so under #2884's family-wide input discipline the argument is always a
        // Pressure tensor and the arg-aware form collapses to Const(Pressure).
        // The generalisation therefore earns its keep only from inputs that
        // should be rejected — which is what Artifact declares. Same unresolved
        // dependencies as von_mises above.
        MaxShear {
            name: "max_shear",
            family: Analysis,
            arity: Exact(1),
            arg_slots: [Any],
            result: ArgAware(tensor_scalar_reduction),
            basis: Artifact
        },

        // principal_stresses -> List(scalar_or_real(tensor_quantity(arg0))).
        // Legacy: crates/reify-compiler/src/analysis_signatures.rs:78.
        //
        // ARTIFACT (ratified 2026-08-17, Leo, unblock of #6001). #2884 rules
        // `principal_stresses(stress: Tensor<2,3,Pressure>) -> List<Pressure>`;
        // as with von_mises this row implements neither the input slot nor the
        // fixed element type. Same unresolved dependencies as von_mises above.
        PrincipalStresses {
            name: "principal_stresses",
            family: Analysis,
            arity: Exact(1),
            arg_slots: [Any],
            result: ArgAware(tensor_scalar_reduction_list),
            basis: Artifact
        },

        // safety_factor -> dimensionless, whatever the args.
        // Legacy: crates/reify-compiler/src/analysis_signatures.rs:82, whose
        // own comment records the derivation at :80-81.
        // Like max_shear, NOT ruled by #2884 — the derivation is the basis.
        SafetyFactor {
            name: "safety_factor",
            family: Analysis,
            arity: Exact(2),
            arg_slots: [Any, Any],
            result: Const(Type::dimensionless_scalar()),
            basis: Physics("yield/von_mises — pressure cancels, so the ratio is dimensionless for any arg dimension")
        },

        // stress_invariants -> StructureRef("StressInvariants"), the struct def
        // in crates/reify-compiler/stdlib/fea.ri.
        // Legacy: crates/reify-compiler/src/analysis_signatures.rs:87.
        // #2884 rules the `{I1,I2,I3}` result; mirrors is_dynamics_query ->
        // StructureRef("MassProperties").
        StressInvariants {
            name: "stress_invariants",
            family: Analysis,
            arity: Exact(1),
            arg_slots: [Any],
            result: Const(Type::StructureRef("StressInvariants".to_string())),
            basis: Ruling("#2884")
        },
    }
}

/// The names of every row whose signature has no independent justification
/// — the I-REG-7 ledger.
///
/// This is a **reviewed ratchet toward zero**, not a diagnostic: a
/// [`Basis::Artifact`](crate::row::Basis::Artifact) row is legal, just
/// conspicuous. The lint test below pins the returned set, so adding an
/// unjustified row makes the count move visibly instead of passing silently,
/// and PRD §7.3(7) requires each τ task text to enumerate its Artifact rows
/// with one sentence each on why they were left unresolved.
///
/// α returns a THREE-row ledger — `von_mises`, `max_shear`,
/// `principal_stresses` (ratified 2026-08-17, Leo, unblock of #6001). Each
/// reproduces the legacy arg-aware reduction rather than the constrained-input,
/// Pressure-fixed signature #2884 actually rules; see the per-row comments for
/// why each was left unresolved. The other four seed rows trace to a Ruling or a
/// Physics derivation.
pub fn artifact_basis_rows() -> Vec<&'static str> {
    rows()
        .iter()
        .filter(|r| r.basis.is_artifact())
        .map(|r| r.name)
        .collect()
}

/// How many rows carry [`Basis::Artifact`](crate::row::Basis::Artifact) — the
/// ratchet metric on its own, for callers that want the number without the
/// names.
pub fn artifact_row_count() -> usize {
    rows().iter().filter(|r| r.basis.is_artifact()).count()
}

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

/// The I-REG-7 registry-crate lint: invariants over the row DATA itself.
///
/// Structured-data assertions, not docstring/prose matching — the ledger below
/// is the enforcement, and the `Basis` doc-comment is deliberately NOT pinned
/// by a prose test.
#[cfg(test)]
mod lint {
    use super::*;
    use crate::row::Basis;
    use strum::{EnumCount, IntoEnumIterator};

    /// PascalCase → snake_case, the transform the macro cannot perform itself.
    fn to_snake_case(pascal: &str) -> String {
        let mut out = String::with_capacity(pascal.len() + 4);
        for (i, ch) in pascal.chars().enumerate() {
            if ch.is_uppercase() {
                if i != 0 {
                    out.push('_');
                }
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// Every row carries a basis, and the string-carrying variants carry a
    /// non-empty payload. A `Ruling` payload must be a canonical `#NNNN` task
    /// cite — the same form the PTODO convention requires, checked here
    /// against the rows rather than duplicating that gate.
    #[test]
    fn every_row_carries_a_well_formed_basis() {
        for r in rows() {
            match r.basis {
                Basis::Ruling(cite) => {
                    assert!(
                        cite.len() > 1
                            && cite.starts_with('#')
                            && cite[1..].chars().all(|c| c.is_ascii_digit()),
                        "{}: Ruling basis must be a canonical #NNNN task cite, got {cite:?}",
                        r.name
                    );
                }
                Basis::Physics(why) | Basis::Doc(why) => {
                    assert!(
                        !why.trim().is_empty(),
                        "{}: a Physics/Doc basis must say WHY, not carry an empty string",
                        r.name
                    );
                }
                // Artifact carries no payload by construction — the tag itself
                // is the statement. Counted by the ratchet below.
                Basis::Artifact => {}
            }
        }
    }

    /// **The I-REG-7 ratchet.** The `Artifact` ledger is pinned to its expected
    /// set, and the failure message reports the count AND the names so a future
    /// τ that adds one sees the number move rather than a silent pass.
    #[test]
    fn artifact_basis_ledger_matches_the_reviewed_set() {
        /// α's reviewed ledger, in `rows()` order. The three analysis
        /// reductions reproduce the legacy arg-aware form instead of the
        /// constrained-input, Pressure-fixed signature #2884 rules; ratified as
        /// Artifact 2026-08-17 (Leo, unblock of #6001) so the gap is COUNTED
        /// rather than hidden behind a Ruling cite the rows do not honour.
        /// Zeroing this ledger is the analysis family's end-state migration —
        /// it needs τ-numeric's ArgSlot vocabulary + `E_BuiltinArgShape` wiring
        /// and leaf θ's corpus migration, and must land as one diff.
        /// Per PRD §7.3(7) this set is a claim, not an omission: parse's two
        /// rows and analysis's other two assert an independent basis.
        const EXPECTED_ARTIFACT_ROWS: &[&str] = &["von_mises", "max_shear", "principal_stresses"];

        let actual = artifact_basis_rows();
        assert_eq!(
            actual,
            EXPECTED_ARTIFACT_ROWS.to_vec(),
            "Artifact-basis ledger moved: {} row(s) now carry `basis: Artifact` \
             ({actual:?}), expected {} ({EXPECTED_ARTIFACT_ROWS:?}). I-REG-7 makes \
             this count a reviewed ratchet toward zero — if the new row is \
             genuinely unjustifiable, add it here AND enumerate it in the task \
             text with one sentence on why it was left unresolved.",
            actual.len(),
            EXPECTED_ARTIFACT_ROWS.len()
        );
        assert_eq!(
            artifact_row_count(),
            actual.len(),
            "artifact_row_count must agree with artifact_basis_rows"
        );
    }

    /// The macro takes each row's variant ident and its name literal
    /// INDEPENDENTLY, so nothing but this test prevents
    /// `ParseLength { name: "parse_lenght", .. }`.
    ///
    /// Iterates via `strum::IntoEnumIterator` so a new variant cannot escape by
    /// simply not being listed here.
    #[test]
    fn every_variant_ident_matches_its_row_name() {
        for id in BuiltinId::iter() {
            let variant = format!("{id:?}");
            let r = row(id);
            assert_eq!(
                to_snake_case(&variant),
                r.name,
                "BuiltinId::{variant} declares name {:?}, but its snake_case \
                 form is {:?} — the macro takes the two independently, so a \
                 typo in either shows up only here",
                r.name,
                to_snake_case(&variant)
            );
        }
    }

    /// The id enum and the row table stay total over each other.
    #[test]
    fn every_id_is_row_resolvable_and_the_counts_agree() {
        assert_eq!(
            BuiltinId::COUNT,
            rows().len(),
            "a BuiltinId variant without a row (or vice versa) means the two \
             stopped sharing the registry! repetition"
        );
        for id in BuiltinId::iter() {
            assert_eq!(row(id).id, id, "row({id:?}) must be {id:?}'s own row");
        }
    }

    /// The snake_case transform itself, pinned on the shapes the seed rows use
    /// — including the trailing-capital `ParseLengthR` case, which a naive
    /// implementation gets wrong.
    #[test]
    fn snake_case_transform_handles_the_seed_shapes() {
        assert_eq!(to_snake_case("ParseLength"), "parse_length");
        assert_eq!(to_snake_case("ParseLengthR"), "parse_length_r");
        assert_eq!(to_snake_case("VonMises"), "von_mises");
        assert_eq!(to_snake_case("PrincipalStresses"), "principal_stresses");
    }
}
