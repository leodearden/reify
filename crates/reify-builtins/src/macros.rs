//! The `registry!` macro: one row list expands into every registry artifact.
//!
//! # Why a macro at all
//!
//! Every artifact below is derived from ONE list of rows, so deleting a row
//! shrinks all of them together. Two independently-authored tables — a row
//! table and a name→id map, say — can drift; one repetition cannot. The
//! in-tree precedent for that property is `corpus_shard_tests!`
//! (`crates/reify-eval/tests/snapshot_cache_divergence_gate.rs:375-396`),
//! whose own doc-comment makes the same argument.
//!
//! PRD open question 1 is resolved here as `macro_rules`, not `build.rs`
//! codegen and not a proc-macro crate: everything needed is token pasting, and
//! a generated-source step would put the registry's single source of truth
//! behind a build artifact.
//!
//! # Invocation shape
//!
//! Rows are GROUPED BY [`BindingKind`](crate::row::BindingKind). The grouping
//! is not cosmetic — it is what makes PRD decision 4 ("exhaustiveness is
//! enforced per kind in the kind's owning crate") mechanically true. Each group
//! mints its own sub-enum, so `reify-stdlib` matches exhaustively on
//! `EvalBuiltinId` and never has to name the geometry or compile-only variants
//! it has no business knowing.
//!
//! ```text
//! registry! {
//!     EvalBuiltin => EvalBuiltinId, as_eval_builtin {
//!         ParseLength {
//!             name: "parse_length",
//!             family: Parse,
//!             arity: Exact(1),
//!             arg_slots: [Any],
//!             result: Const(Type::Option(Box::new(Type::length()))),
//!             basis: Ruling("#4535")
//!         },
//!     }
//! }
//! ```
//!
//! Three things the invocation must spell out that the macro cannot derive,
//! because `macro_rules` has no identifier case-conversion or concatenation:
//! the row's variant ident AND its name literal (S7's lint test pins that
//! pairing), and each group's sub-enum name and accessor name.
//!
//! # Emitted items
//!
//! - `pub enum BuiltinId` — one variant per row, flattened across groups in
//!   source order.
//! - `pub enum <KindId>` per group, plus `BuiltinId::<accessor>()`.
//! - `rows()` / `row(id)` — the row table.
//! - `lookup(name, argc)` / `name_group(name)` — the ONLY string→builtin
//!   resolution in the workspace (I-REG-1).

/// Expand a kind-grouped row list into the full registry surface.
///
/// See the module docs for the invocation shape and the list of emitted items.
macro_rules! registry {
    (
        $(
            $kind:ident => $kind_id:ident, $accessor:ident {
                $(
                    $variant:ident {
                        name: $name:literal,
                        family: $family:ident,
                        arity: $arity_v:ident $(( $($arity_a:expr),* ))?,
                        arg_slots: [ $($slot:ident),* $(,)? ],
                        result: $result_v:ident ( $($result_a:expr),* ),
                        basis: $basis_v:ident $(( $($basis_a:expr),* ))?
                        $(,)?
                    }
                ),* $(,)?
            }
        )*
    ) => {
        /// The generated key every registered builtin is dispatched on.
        ///
        /// One variant per row, flattened across kind groups in declaration
        /// order. Same-name arity overloads are DISTINCT variants.
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash,
            ::strum::EnumIter, ::strum::EnumCount,
        )]
        pub enum BuiltinId {
            $( $( $variant, )* )*
        }

        $(
            /// The subset of [`BuiltinId`] bound by this kind's owning
            /// dispatcher.
            ///
            /// The owning crate matches on THIS enum with no `_` arm, which is
            /// what gives I-REG-2 teeth: adding a row to this group adds a
            /// variant here and stops that crate compiling until it is bound.
            #[derive(
                Debug, Clone, Copy, PartialEq, Eq, Hash,
                ::strum::EnumIter, ::strum::EnumCount,
            )]
            pub enum $kind_id {
                $( $variant, )*
            }

            impl BuiltinId {
                #[doc = concat!(
                    "Narrow to [`", stringify!($kind_id), "`], or `None` if this row \
                     is bound by a different kind."
                )]
                // Unreachable when every row in the registry belongs to this
                // one group — true of α, whose seven seed rows are all
                // EvalBuiltin. The arm is still required the moment a second
                // group appears, so it is allowed rather than conditional.
                #[allow(unreachable_patterns)]
                pub fn $accessor(self) -> Option<$kind_id> {
                    match self {
                        $( BuiltinId::$variant => Some($kind_id::$variant), )*
                        _ => None,
                    }
                }
            }
        )*

        /// Every registered row, in declaration order.
        ///
        /// Lazily built rather than a plain `static`: `ResultSpec::Const(Type)`
        /// holds heap-allocating values (`Type::Option(Box::new(..))`,
        /// `Type::Enum(String)`, `Type::StructureRef(String)`), so the table is
        /// not const-constructible. Re-spelling `Const` as `Const(fn() -> Type)`
        /// would keep a pure `static` at the cost of silently amending the PRD
        /// §7.1 row shape, so lazy init is preferred over redefining the
        /// contract.
        pub fn rows() -> &'static [$crate::row::BuiltinRow<BuiltinId>] {
            static ROWS: ::std::sync::OnceLock<
                ::std::vec::Vec<$crate::row::BuiltinRow<BuiltinId>>
            > = ::std::sync::OnceLock::new();

            ROWS.get_or_init(|| ::std::vec![
                $( $(
                    $crate::row::BuiltinRow {
                        name: $name,
                        id: BuiltinId::$variant,
                        family: $crate::row::Family::$family,
                        binding: $crate::row::BindingKind::$kind,
                        arity: $crate::row::Arity::$arity_v $(( $($arity_a),* ))?,
                        arg_slots: &[ $( $crate::row::ArgSlot::$slot ),* ],
                        result: $crate::row::ResultSpec::$result_v( $($result_a),* ),
                        basis: $crate::row::Basis::$basis_v $(( $($basis_a),* ))?,
                    },
                )* )*
            ])
        }

        /// The row a [`BuiltinId`] was minted for.
        ///
        /// Total by construction — ids and rows come from the same repetition.
        pub fn row(id: BuiltinId) -> &'static $crate::row::BuiltinRow<BuiltinId> {
            rows()
                .iter()
                .find(|r| r.id == id)
                .expect(
                    "every BuiltinId has a row: both are emitted from the same \
                     registry! repetition",
                )
        }

        /// The one authored `name → row` table.
        ///
        /// A true `static` (unlike [`rows`]) because `Arity` is a plain
        /// `usize`-payload enum and `BuiltinId` is fieldless, so the whole
        /// table is const-constructible. Emitted from the SAME repetition as
        /// [`rows`], so a row cannot appear in one and not the other.
        ///
        /// Storing the row's `Arity` rather than a single argc is what lets the
        /// index serve `Range` and `Variadic` rows without a second table.
        static NAME_INDEX: &[(&str, $crate::row::Arity, BuiltinId)] = &[
            $( $(
                (
                    $name,
                    $crate::row::Arity::$arity_v $(( $($arity_a),* ))?,
                    BuiltinId::$variant,
                ),
            )* )*
        ];

        /// Resolve a builtin name and argument count to its [`BuiltinId`].
        ///
        /// **I-REG-1**: this is the only string→builtin resolution in the
        /// workspace. [`name_group`] reads the same [`NAME_INDEX`] — it is a
        /// second ACCESSOR, never a second string map.
        ///
        /// # Where this runs — BOTH paths, including a per-sample loop
        ///
        /// Not compile-time-only. PRD §7.3(3) makes
        /// `reify_stdlib::registry_dispatch::try_dispatch` the FIRST arm of
        /// `reify_stdlib::eval_builtin`, and that arm's whole body is a
        /// `lookup(name, args.len())` — so this scan is on the **eval** path
        /// too. It is not merely on it once per call, either:
        /// `reify_expr::analysis::sample_unary_analysis_at_point` calls
        /// `eval_builtin` POINTWISE for `von_mises` / `max_shear` /
        /// `principal_stresses`, so a field sampled at N points runs this scan
        /// N times.
        ///
        /// A linear scan is still deliberate at α's 7 rows — that is a handful
        /// of `&str` compares per sample, and, sitting at the head of the
        /// chain, it is cheaper than the family dispatchers it displaced.
        /// But the deferral is scoped, not open-ended: a hash/phf index becomes
        /// worth MEASURING once τ grows `NAME_INDEX` past a few dozen rows,
        /// because the per-sample caller above turns the ~358-row end state
        /// into ~358 string comparisons per field sample.
        pub fn lookup(name: &str, argc: usize) -> Option<BuiltinId> {
            NAME_INDEX
                .iter()
                .find(|(n, arity, _)| *n == name && arity.matches(argc))
                .map(|(_, _, id)| *id)
        }

        /// Every id sharing `name`, whatever its arity — empty for an
        /// unregistered name.
        ///
        /// The argc-INDEPENDENT membership accessor. Two consumers need it: the
        /// compiler ladder, whose family arms are name-only today (so an
        /// argc-keyed-only registry would silently change typing for
        /// arity-mismatched calls), and stdlib-namespace κ #5503, which needs a
        /// builtin-MEMBERSHIP authority for its strict-visibility flip.
        ///
        /// Derived from [`NAME_INDEX`] on first call, so PRD open question 2 is
        /// answered "both accessors, one table".
        pub fn name_group(name: &str) -> &'static [BuiltinId] {
            static GROUPS: ::std::sync::OnceLock<
                ::std::vec::Vec<(&'static str, ::std::vec::Vec<BuiltinId>)>
            > = ::std::sync::OnceLock::new();

            let groups = GROUPS.get_or_init(|| {
                let mut out: ::std::vec::Vec<(&'static str, ::std::vec::Vec<BuiltinId>)> =
                    ::std::vec::Vec::new();
                for &(n, _, id) in NAME_INDEX.iter() {
                    match out.iter_mut().find(|entry| entry.0 == n) {
                        Some(entry) => entry.1.push(id),
                        None => out.push((n, ::std::vec![id])),
                    }
                }
                out
            });

            groups
                .iter()
                .find(|entry| entry.0 == name)
                .map(|entry| entry.1.as_slice())
                .unwrap_or(&[])
        }
    };
}

pub(crate) use registry;

#[cfg(test)]
mod tests {
    //! `registry!` is tested over a TEST-LOCAL table, not the real seed rows.
    //!
    //! Two reasons. The macro is proven GENERAL — the fake table exercises a
    //! same-name arity pair, a second binding kind, `Variadic`/`Range`
    //! arities and an `ArgAware` result, none of which α's seven seed rows
    //! happen to have. And the seed rows stay free to change in S5/S6 without
    //! dragging the macro's own contract tests with them.

    use reify_core::Type;
    use strum::{EnumCount, IntoEnumIterator};

    /// A deliberately awkward fake registry.
    mod fake {
        use reify_core::Type;

        /// An `ArgAware` resolver: echoes the first arg's type, declining when
        /// there is no first arg.
        pub(super) fn first_or_none(args: &[Type]) -> Option<Type> {
            args.first().cloned()
        }

        crate::macros::registry! {
            EvalBuiltin => EvalBuiltinId, as_eval_builtin {
                // A same-name arity PAIR — the thing `lookup(name, argc)`
                // exists to disambiguate (`floor`@1/@2, `offset`@2/@3).
                FooOne {
                    name: "foo",
                    family: Parse,
                    arity: Exact(1),
                    arg_slots: [Any],
                    result: Const(Type::String),
                    basis: Ruling("#0001")
                },
                FooTwo {
                    name: "foo",
                    family: Parse,
                    arity: Exact(2),
                    arg_slots: [Any, Any],
                    result: Const(Type::Int),
                    basis: Ruling("#0002")
                },
                Bar {
                    name: "bar",
                    family: Analysis,
                    arity: Variadic,
                    arg_slots: [Any],
                    result: ArgAware(first_or_none),
                    basis: Artifact
                },
            }
            // A SECOND kind group: proves `BuiltinId` flattens across groups
            // and that `as_eval_builtin` declines a row it does not own.
            CompileOnly => CompileOnlyId, as_compile_only {
                Baz {
                    name: "baz",
                    family: Analysis,
                    arity: Range(1, 2),
                    arg_slots: [Any],
                    result: Const(Type::Bool),
                    basis: Doc("docs/prds/v0_6/builtin-signature-registry.md")
                },
            }
        }
    }

    use fake::{BuiltinId, CompileOnlyId, EvalBuiltinId};

    // ── generated enums ──────────────────────────────────────────────────────

    /// `BuiltinId` gets exactly one variant per row, flattened across kind
    /// groups, in source order.
    #[test]
    fn builtin_id_has_one_variant_per_row_flattened_in_source_order() {
        assert_eq!(
            BuiltinId::iter().collect::<Vec<_>>(),
            vec![
                BuiltinId::FooOne,
                BuiltinId::FooTwo,
                BuiltinId::Bar,
                BuiltinId::Baz,
            ],
            "BuiltinId must flatten every kind group in declaration order"
        );
    }

    /// Each kind group gets its own sub-enum holding exactly that group's rows
    /// — the per-kind exhaustiveness surface I-REG-2 leans on.
    #[test]
    fn each_kind_group_gets_a_sub_enum_of_exactly_its_own_rows() {
        assert_eq!(
            EvalBuiltinId::iter().collect::<Vec<_>>(),
            vec![
                EvalBuiltinId::FooOne,
                EvalBuiltinId::FooTwo,
                EvalBuiltinId::Bar
            ],
            "EvalBuiltinId must hold exactly the EvalBuiltin group's rows"
        );
        assert_eq!(
            CompileOnlyId::iter().collect::<Vec<_>>(),
            vec![CompileOnlyId::Baz],
            "CompileOnlyId must hold exactly the CompileOnly group's rows"
        );
    }

    /// `as_<kind>()` round-trips a row of that kind and declines every other.
    #[test]
    fn as_kind_accessors_round_trip_and_decline_foreign_rows() {
        assert_eq!(
            BuiltinId::FooOne.as_eval_builtin(),
            Some(EvalBuiltinId::FooOne)
        );
        assert_eq!(BuiltinId::Bar.as_eval_builtin(), Some(EvalBuiltinId::Bar));
        assert_eq!(
            BuiltinId::Baz.as_eval_builtin(),
            None,
            "a CompileOnly row is not an EvalBuiltin"
        );

        assert_eq!(
            BuiltinId::Baz.as_compile_only(),
            Some(CompileOnlyId::Baz),
            "Baz must round-trip through its OWN kind"
        );
        assert_eq!(BuiltinId::FooOne.as_compile_only(), None);
    }

    // ── the row table ────────────────────────────────────────────────────────

    /// `rows()` yields one row per declaration, in source order, each carrying
    /// its declared id and every declared column verbatim.
    #[test]
    fn rows_are_one_per_declaration_in_source_order_with_all_columns() {
        let rows = fake::rows();
        assert_eq!(rows.len(), 4, "one row per declaration");

        assert_eq!(
            rows.iter().map(|r| r.name).collect::<Vec<_>>(),
            vec!["foo", "foo", "bar", "baz"]
        );
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![
                BuiltinId::FooOne,
                BuiltinId::FooTwo,
                BuiltinId::Bar,
                BuiltinId::Baz
            ],
            "each row must carry the id the macro minted for it"
        );

        // Every column survives the expansion, including the shapes the seed
        // rows do not exercise.
        assert_eq!(rows[0].arity, crate::row::Arity::Exact(1));
        assert_eq!(rows[1].arity, crate::row::Arity::Exact(2));
        assert_eq!(rows[2].arity, crate::row::Arity::Variadic);
        assert_eq!(rows[3].arity, crate::row::Arity::Range(1, 2));

        assert_eq!(rows[0].binding, crate::row::BindingKind::EvalBuiltin);
        assert_eq!(
            rows[3].binding,
            crate::row::BindingKind::CompileOnly,
            "the binding column comes from the group header, not the row"
        );

        assert_eq!(rows[0].family, crate::row::Family::Parse);
        assert_eq!(rows[2].family, crate::row::Family::Analysis);

        assert_eq!(rows[0].basis, crate::row::Basis::Ruling("#0001"));
        assert_eq!(rows[2].basis, crate::row::Basis::Artifact);
        assert_eq!(
            rows[3].basis,
            crate::row::Basis::Doc("docs/prds/v0_6/builtin-signature-registry.md")
        );

        assert_eq!(rows[0].arg_slots, [crate::row::ArgSlot::Any].as_slice());
        assert_eq!(
            rows[1].arg_slots,
            [crate::row::ArgSlot::Any, crate::row::ArgSlot::Any].as_slice()
        );
    }

    /// Both `ResultSpec` shapes survive the macro: `Const` echoes its type,
    /// `ArgAware` is wired to the declared fn.
    #[test]
    fn result_column_carries_both_const_and_arg_aware_specs() {
        let rows = fake::rows();
        assert_eq!(rows[0].result.resolve(&[]), Some(Type::String));
        assert_eq!(rows[1].result.resolve(&[]), Some(Type::Int));
        assert_eq!(
            rows[2].result.resolve(&[Type::Int]),
            Some(Type::Int),
            "ArgAware must be wired to the declared resolver"
        );
        assert_eq!(
            rows[2].result.resolve(&[]),
            None,
            "an ArgAware resolver's None must survive"
        );
    }

    // ── lookup / name_group ──────────────────────────────────────────────────

    /// `lookup(name, argc)` resolves each row at its declared arity and
    /// declines everything else.
    #[test]
    fn lookup_resolves_by_name_and_argc() {
        assert_eq!(fake::lookup("foo", 1), Some(BuiltinId::FooOne));
        assert_eq!(fake::lookup("foo", 2), Some(BuiltinId::FooTwo));
        assert_eq!(fake::lookup("baz", 1), Some(BuiltinId::Baz));
        assert_eq!(fake::lookup("baz", 2), Some(BuiltinId::Baz));

        // Variadic matches any argc, including none.
        assert_eq!(fake::lookup("bar", 0), Some(BuiltinId::Bar));
        assert_eq!(fake::lookup("bar", 9), Some(BuiltinId::Bar));

        // Non-matching arity for a REGISTERED name.
        assert_eq!(fake::lookup("foo", 0), None);
        assert_eq!(fake::lookup("foo", 3), None);
        assert_eq!(fake::lookup("baz", 3), None);

        // Unregistered names.
        assert_eq!(fake::lookup("nope", 1), None);
        assert_eq!(fake::lookup("", 1), None);
    }

    /// The same-name pair resolves to DIFFERENT ids — the whole point of
    /// keying lookup on argc (PRD §7.1).
    #[test]
    fn same_name_arity_pair_resolves_to_different_ids() {
        let one = fake::lookup("foo", 1).expect("foo@1 is registered");
        let two = fake::lookup("foo", 2).expect("foo@2 is registered");
        assert_ne!(
            one, two,
            "an arity overload pair must not collapse to one id"
        );
    }

    /// `name_group` is the argc-INDEPENDENT membership accessor: every id
    /// sharing a name, whatever the arity.
    #[test]
    fn name_group_returns_every_id_sharing_a_name() {
        assert_eq!(
            fake::name_group("foo"),
            [BuiltinId::FooOne, BuiltinId::FooTwo].as_slice(),
            "both arities of an overloaded name belong to one group"
        );
        assert_eq!(fake::name_group("bar"), [BuiltinId::Bar].as_slice());
        assert_eq!(fake::name_group("baz"), [BuiltinId::Baz].as_slice());
        assert!(
            fake::name_group("nope").is_empty(),
            "an unregistered name has an empty group, not a panic"
        );
    }

    // ── the one-repetition property ──────────────────────────────────────────

    /// The row table and the id index are driven by the SAME macro repetition,
    /// so they cannot drift.
    ///
    /// Deleting a row shrinks BOTH — the `corpus_shard_tests!` property
    /// (`snapshot_cache_divergence_gate.rs:375-396`). Deletion itself cannot be
    /// tested from inside, so the property is checked structurally: the two
    /// artifacts agree on count, and each is total over the other.
    #[test]
    fn rows_and_the_id_index_are_driven_by_one_repetition() {
        assert_eq!(
            fake::rows().len(),
            BuiltinId::COUNT,
            "a row without an id variant (or vice versa) means the two \
             artifacts stopped sharing a repetition"
        );

        // Every id resolves to the row that carries it.
        for id in BuiltinId::iter() {
            assert_eq!(
                fake::row(id).id,
                id,
                "row({id:?}) must be the row declaring {id:?}"
            );
        }

        // Every row is reachable through the name index.
        for r in fake::rows() {
            assert!(
                fake::name_group(r.name).contains(&r.id),
                "row {:?} is absent from its own name group",
                r.name
            );
        }
    }
}
