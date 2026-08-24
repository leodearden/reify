//! Built-in unit-symbol → SI conversion table.
//!
//! Extracted from `reify-compiler::units::unit_to_scalar` (task #4535) so that
//! reify-stdlib's runtime `parse_length`/`parse_length_r` and reify-compiler's
//! quantity-literal handling share exactly one physical table instead of
//! risking two independently-diverging copies. Returns pure core types
//! (`f64` SI scale factor + [`crate::DimensionVector`]) — no `Value`
//! coupling — so this stays a leaf module that both compiler and stdlib can
//! depend on without inverting the crate DAG (see the B1 invariant note in
//! `lib.rs`).
//!
//! # The units-gate MIGRATION HINTS live here for the same reason (task 5750)
//!
//! [`LENGTH_MIGRATION_HINT`] and [`DENSITY_MIGRATION_HINT`] are the exact
//! clauses appended to a rejection at a LENGTH / Density argument slot. Both
//! the EVAL layer (`reify-eval::arg_acceptance`) and the COMPILE layer
//! (`reify-compiler::builtin_signatures`) must render the same wording for the
//! same authoring mistake — PRD `docs/prds/v0_6/units-length-gate-completion.md`
//! decision D9 — so they live here, as one table with one edit site, exactly as
//! [`unit_symbol_to_si`] does rather than as two independently-diverging copies.
//!
//! ## Why a HOIST and not a copy
//!
//! The neighbouring precedent says the opposite, and a reviewer will ask.
//! `reify-compiler::conformance`'s `dimensioned_scalar_migration_hint` records
//! that its own clause is "Copied and never imported: `reify-eval` depends on
//! `reify-compiler`, so the reverse edge would be a dependency cycle (D9)."
//! That constraint is about the reify-eval → reify-compiler EDGE. `reify-core`
//! sits BELOW both, so hoisting here never creates that cycle.
//!
//! The copy route was reconsidered and rejected on a measurement:
//! `reify-eval/src/lib.rs` declares `pub(crate) mod arg_acceptance;`, so even
//! though `reify-compiler` dev-depends on `reify-eval`, a drift-pin test cannot
//! import `length_spec()` to compare against. Short of widening reify-eval's
//! public API, sharing one const is the only structural pin available.
//!
//! And unlike the conformance hint — COMPUTED from the `NAMED_DIMENSIONS`
//! registry, and so drift-proof by construction — these two are IRREGULAR
//! hard-coded literals that do not follow that template. They genuinely can
//! drift, which is what earns them the shared const.
//!
//! These two are NOT interchangeable with the conformance hint and must not be
//! unified with it: for LENGTH that one renders "pass a dimensioned Length
//! literal such as `1m`". Pinned by
//! `builtin_slot_and_ctor_conformance_length_hints_are_deliberately_different`
//! in `reify-compiler/tests/builtin_arg_signature_tests.rs`.

use crate::DimensionVector;

/// The migration hint appended to a rejection at a LENGTH argument slot.
///
/// Read by BOTH layers — `reify-eval::arg_acceptance::length_spec` and the
/// LENGTH slots of `reify-compiler::builtin_signatures::builtin_arg_slots` — so
/// the compile-time and runtime diagnostics for one authoring mistake read
/// identically (PRD decision D9). See the module doc for why this is a hoist
/// rather than a copy, and why it must not be unified with
/// `reify-compiler::conformance`'s computed ctor-slot hint.
pub const LENGTH_MIGRATION_HINT: &str = "pass a dimensioned length such as `5mm`";

/// The migration hint appended to a rejection at a Density argument slot.
///
/// Mirrors [`LENGTH_MIGRATION_HINT`]; read by
/// `reify-eval::arg_acceptance::density_spec` and by the `center_of_mass` /
/// `moment_of_inertia` density slots in `reify-compiler::builtin_signatures`.
pub const DENSITY_MIGRATION_HINT: &str =
    "pass a dimensioned Density literal such as `7850kg/m^3`";

/// The built-in unit symbols, as DATA rather than control flow — one physical
/// table, one edit site.
///
/// Each entry is `(symbol, si_factor, dimension)` such that
/// `si_value = magnitude * si_factor`. [`unit_symbol_to_si`] is a lookup over
/// this slice and nothing else; the values are those copied verbatim from the
/// pre-extraction `reify-compiler::units::unit_to_scalar` match arms (task
/// #4535), which now delegates here.
///
/// It is `pub` because two guards outside this crate iterate it: `reify-core`'s
/// own `every_builtin_symbol_has_an_emission_ladder` (the reverse-coupling
/// guard for [`ri_emittable_units`]) and, more importantly,
/// `reify-compiler`'s `stdlib_unit_declarations_agree_bit_for_bit_with_the_
/// builtin_table`, which compares every entry against the per-module
/// `UnitRegistry` that actually WINS at resolution time. Both previously kept
/// hand-mirrored symbol lists; driving them from this slice is what removes the
/// possibility of a new symbol silently dodging either check (task #5095).
///
/// Two properties are pinned by tests, both load-bearing rather than tidiness:
/// symbols are UNIQUE (the linear lookup silently shadows a duplicate, where
/// the old `match` gave `unreachable_patterns`), and the table is non-vacuous
/// (every guard that iterates it passes trivially over an empty one).
pub const BUILTIN_UNITS: &[(&str, f64, DimensionVector)] = &[
    ("mm", 0.001, DimensionVector::LENGTH),
    ("cm", 0.01, DimensionVector::LENGTH),
    ("m", 1.0, DimensionVector::LENGTH),
    ("in", 0.0254, DimensionVector::LENGTH),
    ("deg", std::f64::consts::PI / 180.0, DimensionVector::ANGLE),
    ("rad", 1.0, DimensionVector::ANGLE),
    ("kg", 1.0, DimensionVector::MASS),
    ("g", 0.001, DimensionVector::MASS),
    ("s", 1.0, DimensionVector::TIME),
    // Kelvin needs a hardcoded fallback because `std.units` itself uses
    // `1K` in `BOLTZMANN_CONSTANT()`s body — fn bodies in std.units load
    // with no unit_registry seeded, so the K declared at units.ri can't
    // satisfy the same file's own quantity literals. Mirrors the kg/s/m
    // self-bootstrap entries above.
    ("K", 1.0, DimensionVector::TEMPERATURE),
    // Bare SI base units completing the standard set (factor 1.0).
    // A/mol/cd are the SI bases for Current/AmountOfSubstance/LuminousIntensity;
    // they need the same hardcoded fallback as kg/s/K so that stdlib fn bodies
    // and other unseeded-registry scopes can resolve these unit literals
    // (PRD §2.2 / decision D5).
    ("A", 1.0, DimensionVector::CURRENT),
    ("mol", 1.0, DimensionVector::AMOUNT_OF_SUBSTANCE),
    ("cd", 1.0, DimensionVector::LUMINOUS_INTENSITY),
];

/// Look up a built-in unit symbol's SI conversion factor and dimension.
///
/// Returns `Some((factor, dimension))` such that `si_value = value * factor`,
/// or `None` if `unit` is not one of the hardcoded built-in symbols.
///
/// A faithful view of [`BUILTIN_UNITS`] and nothing more — the single source of
/// truth for the built-in symbols, carrying the values copied verbatim from the
/// pre-extraction `reify-compiler::units::unit_to_scalar` match arms; that
/// function now delegates here. Does not resolve user-declared units
/// (e.g. `km`, `ft`, `thou`) — those live only in the compiler's per-module
/// `UnitRegistry`, which has no equivalent at this layer.
pub fn unit_symbol_to_si(unit: &str) -> Option<(f64, DimensionVector)> {
    BUILTIN_UNITS
        .iter()
        .find(|(symbol, ..)| *symbol == unit)
        .map(|&(_, factor, dimension)| (factor, dimension))
}

/// Ordered `.ri`-**emittable** unit symbols for a dimension, most-preferred
/// first — the reverse of [`unit_symbol_to_si`] (task #5095).
///
/// Used by `reify_ir::ri_literal` to write a [`crate::DimensionVector`]-carrying
/// value back out as `.ri` source text (`80mm`, `90deg`). The caller walks the
/// returned slice front-to-back and takes the first symbol whose magnitude is
/// **bit-exactly** recoverable (`magnitude * factor == si_value`), so the order
/// here directly decides which literal a user's file ends up carrying.
///
/// Two contracts hold for every returned slice, both pinned by
/// `ri_emittable_ladders_resolve_to_their_dimension_and_end_at_factor_one`:
///
/// 1. Every symbol is a **bare built-in** that [`unit_symbol_to_si`] resolves
///    to this same dimension. That matters because the compiler resolves a
///    bare unit as `registry.lookup(..).or_else(|| unit_to_scalar(..))` — the
///    built-in table is an unconditional fallback, so these symbols parse in
///    any module with no `std.units` import and no seeded `UnitRegistry`.
/// 2. The **last** symbol has SI factor exactly `1.0`. `mag * 1.0 == si_value`
///    then holds by IEEE identity, so the caller's exactness ladder can never
///    exhaust for a finite `si_value` and never has to fall back to a lossy
///    emission.
///
/// Returns `&[]` — meaning "not emittable as a bare unit literal" — for
/// dimensionless values and for every dimension that would need a compound
/// `UnitExpr` (Area `m^2`, Volume `m^3`, Pressure, Force, …). Compound unit
/// expressions go through the compiler's `resolve_unit_expr`, which is
/// registry-ONLY with no built-in fallback, so emitting one would make the
/// round-trip contingent on the target module's imports. `sr` (SolidAngle)
/// and `USD` (Money) are simply absent from [`unit_symbol_to_si`].
///
/// # Intentionally NOT [`crate::display_units::unit_ladders`]
///
/// That table is also an ordered per-dimension unit ladder, and unifying the
/// two would be a mistake. It answers a different question — what a human
/// picks in the GUI unit picker — and is unusable here on three independent
/// grounds: its labels include forms `unit_symbol_to_si` cannot resolve and
/// the lexer cannot tokenise (`mm²`, `cm³`, `L`, `MPa`, `kg/m³`, `N`, `J`,
/// `W`); its Length rungs end at `in` (0.0254) rather than a factor-1.0
/// terminator; and its coverage differs in both directions. Emission order is
/// load-bearing for round-trip exactness, display order is picker ergonomics
/// and is free to change. The two are cross-guarded for *physical* agreement
/// only, by `ri_emittable_units_agrees_physically_with_display_ladders`.
pub fn ri_emittable_units(dim: &DimensionVector) -> &'static [&'static str] {
    // Ladders are ordered smallest-unit-first so an edit stays in the unit a
    // human would write (`80mm`, not `0.08m`), with the SI base last as the
    // always-exact terminator. `in` is deliberately absent: it is emittable
    // only via an explicit caller hint, never chosen canonically.
    match *dim {
        DimensionVector::LENGTH => &["mm", "cm", "m"],
        DimensionVector::ANGLE => &["deg", "rad"],
        // `g` (0.001) exists in the built-in table but is not a canonical
        // choice — `kg` is the SI base and the exact terminator.
        DimensionVector::MASS => &["kg"],
        DimensionVector::TIME => &["s"],
        DimensionVector::TEMPERATURE => &["K"],
        DimensionVector::CURRENT => &["A"],
        DimensionVector::AMOUNT_OF_SUBSTANCE => &["mol"],
        DimensionVector::LUMINOUS_INTENSITY => &["cd"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The index → SI base symbol table `ri_compound_unit_expr` renders
    // from; the structural guard below sweeps emitted atoms against it.
    use crate::dimension::BASE_UNIT_SYMBOLS;

    /// Anti-vacuity floor for every guard in this module that ITERATES
    /// [`BUILTIN_UNITS`].
    ///
    /// `builtin_units_is_the_table_unit_symbol_to_si_reads`,
    /// `every_builtin_symbol_resolves`, `builtin_unit_symbols_are_unique` and
    /// `every_builtin_symbol_has_an_emission_ladder` all pass trivially over an
    /// emptied or gutted table. This is the one assertion that cannot.
    ///
    /// 13 is the count when the floor was set. Deleting a genuine unit is a
    /// deliberate act, so this is `>=`, not `==`: a floor against accidental
    /// erosion, not a second mirror of the table's length.
    #[test]
    fn builtin_units_is_not_vacuous() {
        assert!(
            BUILTIN_UNITS.len() >= 13,
            "BUILTIN_UNITS has {} entries, fewer than the 13 built-in symbols \
             present when this floor was set — every table-iterating guard in \
             this module weakens silently as entries disappear",
            BUILTIN_UNITS.len()
        );
    }

    /// [`BUILTIN_UNITS`] is exactly the table [`unit_symbol_to_si`] reads.
    ///
    /// The lookup is a `.find()` over the table, so this holds by construction
    /// today — the point is to keep it holding if the lookup ever grows a
    /// special case, an alias arm, or a normalisation step in front of the
    /// table. Bit-equality on the factor, not a tolerance: the serializer's
    /// `exact_magnitude` reproduces the compiler's single multiply against
    /// exactly these bits, so a one-ULP shift here voids its proof.
    ///
    /// This replaces the source-scanning `builtin_symbol_list_is_complete`,
    /// deleted along with the hand-mirrored `BUILTIN_UNIT_SYMBOLS` const it
    /// existed to police. That guard `include_str!`d this very file and re-derived
    /// the arm set by looking for lines shaped `"sym" => Some((` — and silently
    /// DROPPED any arm it could not parse. An or-pattern arm (`"ft" | "foot" =>
    /// …`), or an arm with `=>` broken onto its own line, was simply invisible to
    /// it, so it passed VACUOUSLY in precisely the add-an-arm direction it claimed
    /// to protect, while the reverse-coupling guard never saw the new symbol
    /// either. Iterating entries cannot skip one.
    #[test]
    fn builtin_units_is_the_table_unit_symbol_to_si_reads() {
        for &(sym, factor, dim) in BUILTIN_UNITS {
            let (found_factor, found_dim) = unit_symbol_to_si(sym)
                .unwrap_or_else(|| panic!("BUILTIN_UNITS entry {sym:?} must resolve"));
            assert_eq!(
                found_factor.to_bits(),
                factor.to_bits(),
                "unit_symbol_to_si({sym:?}) returned factor {found_factor:?} \
                 (bits {:#018x}) but BUILTIN_UNITS holds {factor:?} (bits \
                 {:#018x}) — the lookup is no longer a faithful view of the table",
                found_factor.to_bits(),
                factor.to_bits()
            );
            assert_eq!(
                found_dim,
                dim,
                "unit_symbol_to_si({sym:?}) returned dimension {:?} but \
                 BUILTIN_UNITS holds {:?}",
                found_dim.canonical_name(),
                dim.canonical_name()
            );
        }
    }

    /// No symbol appears twice in [`BUILTIN_UNITS`] — a NEW obligation created
    /// by making the table data rather than control flow, and load-bearing.
    ///
    /// A duplicated `match` arm was a compiler diagnostic
    /// (`unreachable_patterns`). A duplicated table row is not: the linear
    /// `.find()` returns the FIRST and silently shadows the rest, so a stray
    /// `("mm", 0.01, LENGTH)` row would be a silent PHYSICAL change — every `mm`
    /// literal off by 10× — with no diagnostic anywhere and every other guard in
    /// this module still green.
    #[test]
    fn builtin_unit_symbols_are_unique() {
        for (i, entry) in BUILTIN_UNITS.iter().enumerate() {
            for (j, other) in BUILTIN_UNITS.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    entry.0, other.0,
                    "symbol {:?} appears at both index {i} and index {j} in \
                     BUILTIN_UNITS. unit_symbol_to_si's `.find()` returns the \
                     first (factor {:?}) and silently shadows the second (factor \
                     {:?}) — a duplicate row is a silent physical change, not a \
                     lint.",
                    entry.0, entry.1, other.1
                );
            }
        }
    }

    /// Forward: every table symbol still resolves. Catches a deleted entry.
    #[test]
    fn every_builtin_symbol_resolves() {
        for &(sym, _, _) in BUILTIN_UNITS {
            assert!(
                unit_symbol_to_si(sym).is_some(),
                "built-in symbol {sym:?} no longer resolves"
            );
        }
    }

    #[test]
    fn mm_converts_to_length_with_milli_factor() {
        let (factor, dim) = unit_symbol_to_si("mm").expect("mm should be recognized");
        assert!((factor - 0.001).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn cm_converts_to_length_with_centi_factor() {
        let (factor, dim) = unit_symbol_to_si("cm").expect("cm should be recognized");
        assert!((factor - 0.01).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn m_converts_to_length_with_unit_factor() {
        let (factor, dim) = unit_symbol_to_si("m").expect("m should be recognized");
        assert!((factor - 1.0).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn in_converts_to_length_with_inch_factor() {
        let (factor, dim) = unit_symbol_to_si("in").expect("in should be recognized");
        assert!((factor - 0.0254).abs() < 1e-9);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn kg_converts_to_mass_with_unit_factor() {
        let (factor, dim) = unit_symbol_to_si("kg").expect("kg should be recognized");
        assert!((factor - 1.0).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::MASS);
    }

    #[test]
    fn deg_converts_to_angle_with_pi_over_180_factor() {
        let (factor, dim) = unit_symbol_to_si("deg").expect("deg should be recognized");
        assert!((factor - std::f64::consts::PI / 180.0).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::ANGLE);
    }

    #[test]
    fn bogus_unit_is_unrecognized() {
        assert_eq!(unit_symbol_to_si("bogus"), None);
    }

    // ─── `ri_emittable_units` — the reverse (emission) table ─────────────────
    //
    // Task #5095 (ai-native-editing β). These four tests are the contract the
    // `.ri` source-literal serializer in `reify-ir::ri_literal` leans on.

    /// (a) Every dimension with a bare built-in symbol has its expected
    /// ordered ladder. Order is load-bearing: the serializer walks it
    /// front-to-back and takes the first rung that is bit-exact.
    #[test]
    fn ri_emittable_units_lists_the_expected_ordered_ladders() {
        assert_eq!(
            ri_emittable_units(&DimensionVector::LENGTH),
            &["mm", "cm", "m"]
        );
        assert_eq!(ri_emittable_units(&DimensionVector::ANGLE), &["deg", "rad"]);
        assert_eq!(ri_emittable_units(&DimensionVector::MASS), &["kg"]);
        assert_eq!(ri_emittable_units(&DimensionVector::TIME), &["s"]);
        assert_eq!(ri_emittable_units(&DimensionVector::TEMPERATURE), &["K"]);
        assert_eq!(ri_emittable_units(&DimensionVector::CURRENT), &["A"]);
        assert_eq!(
            ri_emittable_units(&DimensionVector::AMOUNT_OF_SUBSTANCE),
            &["mol"]
        );
        assert_eq!(
            ri_emittable_units(&DimensionVector::LUMINOUS_INTENSITY),
            &["cd"]
        );
    }

    /// (b) Dimensionless and every dimension that would need a compound unit
    /// expression emit nothing. `sr`/`USD` are absent from `unit_symbol_to_si`
    /// entirely, and compound `UnitExpr`s resolve from the compiler's
    /// per-module registry only — so neither is round-trip-safe as a bare
    /// symbol.
    #[test]
    fn ri_emittable_units_is_empty_for_dimensionless_and_unsupported_dimensions() {
        assert!(ri_emittable_units(&DimensionVector::DIMENSIONLESS).is_empty());
        for dim in [
            DimensionVector::PRESSURE,
            DimensionVector::AREA,
            DimensionVector::VOLUME,
            DimensionVector::FORCE,
            DimensionVector::SOLID_ANGLE,
            DimensionVector::MONEY,
        ] {
            assert!(
                ri_emittable_units(&dim).is_empty(),
                "dimension {:?} must have no bare-symbol emission ladder",
                dim.canonical_name()
            );
        }
    }

    /// (b2) REVERSE COUPLING — every built-in symbol's dimension is emittable.
    ///
    /// Tests (a)–(c) all walk FORWARD: from a ladder to the symbols on it. That
    /// direction cannot see the hole `ri_emittable_units`'s `_ => &[]` catch-all
    /// leaves — add a dimension to [`unit_symbol_to_si`] and forget its ladder,
    /// and the serializer silently starts REFUSING a value it could write
    /// perfectly well, with every forward test still green. (The failure mode is
    /// a refusal, not a corruption, which is why this is a coverage guard rather
    /// than a correctness one — but a refusal γ cannot explain is still a bug.)
    ///
    /// So this walks BACKWARD, from the symbol table to the ladders. It is green
    /// today for all 13 symbols by construction; it exists to fire on exactly the
    /// regression above. It iterates [`BUILTIN_UNITS`] directly — the one physical
    /// table `unit_symbol_to_si` reads — so there is no mirror list in between and
    /// a new entry cannot dodge it.
    ///
    /// Deliberately asserts only NON-EMPTINESS, not membership: `g` and `in` are
    /// resolvable built-ins that are correctly absent from their canonical
    /// ladders (reachable only through a caller-supplied hint). What must hold is
    /// that their *dimension* is writable at all.
    #[test]
    fn every_builtin_symbol_has_an_emission_ladder() {
        for &(sym, _, _) in BUILTIN_UNITS {
            let (_factor, dim) =
                unit_symbol_to_si(sym).unwrap_or_else(|| panic!("{sym:?} must resolve"));
            assert!(
                !ri_emittable_units(&dim).is_empty(),
                "built-in symbol {sym:?} has dimension {:?}, which ri_emittable_units \
                 gives no ladder for — the `_ => &[]` catch-all swallowed it, so \
                 value_to_ri_literal now refuses every value of that dimension even \
                 though a bare symbol for it exists. Add the ladder.",
                dim.canonical_name()
            );
        }
    }

    /// (c) SELF DRIFT-GUARD — load-bearing.
    ///
    /// Every symbol on a ladder must resolve through `unit_symbol_to_si` to
    /// that very dimension, and the LAST rung's factor must be **exactly**
    /// `1.0`. The serializer accepts a rung only when
    /// `magnitude * factor == si_value` bit-identically; the factor-1.0
    /// terminator is what makes that acceptance unconditional (`mag * 1.0 ==
    /// si_value` by IEEE identity), so the ladder can never exhaust for a
    /// finite input. Drop the terminator and lossy emission silently returns.
    #[test]
    fn ri_emittable_ladders_resolve_to_their_dimension_and_end_at_factor_one() {
        let dims = [
            DimensionVector::LENGTH,
            DimensionVector::ANGLE,
            DimensionVector::MASS,
            DimensionVector::TIME,
            DimensionVector::TEMPERATURE,
            DimensionVector::CURRENT,
            DimensionVector::AMOUNT_OF_SUBSTANCE,
            DimensionVector::LUMINOUS_INTENSITY,
        ];
        for dim in dims {
            let ladder = ri_emittable_units(&dim);
            assert!(
                !ladder.is_empty(),
                "{:?} must have a non-empty emission ladder",
                dim.canonical_name()
            );
            for sym in ladder {
                let (_factor, sym_dim) = unit_symbol_to_si(sym).unwrap_or_else(|| {
                    panic!("emitted symbol {sym:?} must be a bare built-in unit")
                });
                assert_eq!(
                    sym_dim,
                    dim,
                    "symbol {sym:?} resolves to {:?}, not {:?}",
                    sym_dim.canonical_name(),
                    dim.canonical_name()
                );
            }
            let last = ladder[ladder.len() - 1];
            let (last_factor, _) = unit_symbol_to_si(last).expect("terminator resolves");
            assert_eq!(
                last_factor,
                1.0,
                "ladder for {:?} must terminate at an exactly-1.0-factor symbol \
                 (got {last:?} with factor {last_factor}); the serializer's \
                 exactness fallback is unconditional only because of this",
                dim.canonical_name()
            );
        }
    }

    /// (d) CROSS-TABLE DRIFT-GUARD vs the GUI display ladders.
    ///
    /// `display_units::unit_ladders()` is a *display* table and is
    /// deliberately NOT reused as the emission table (its labels include
    /// unparseable forms like `mm²`/`L`/`MPa`, its Length rungs end at `in`
    /// rather than a factor-1.0 terminator, and its dimension coverage
    /// differs in both directions). This guard therefore asserts neither
    /// subset nor order — only that where the two tables name the same
    /// symbol they agree on the physics, exactly.
    #[test]
    fn ri_emittable_units_agrees_physically_with_display_ladders() {
        let ladders = crate::display_units::unit_ladders();
        let dims = [
            DimensionVector::LENGTH,
            DimensionVector::ANGLE,
            DimensionVector::MASS,
            DimensionVector::TIME,
            DimensionVector::TEMPERATURE,
            DimensionVector::CURRENT,
            DimensionVector::AMOUNT_OF_SUBSTANCE,
            DimensionVector::LUMINOUS_INTENSITY,
            DimensionVector::AREA,
            DimensionVector::VOLUME,
            DimensionVector::PRESSURE,
            DimensionVector::FORCE,
        ];
        let mut compared = 0usize;
        for dim in dims {
            let Some(name) = dim.canonical_name() else {
                continue;
            };
            let Some(ladder) = ladders.iter().find(|l| l.dimension == name) else {
                continue;
            };
            for sym in ri_emittable_units(&dim) {
                let Some(rung) = ladder.units.iter().find(|u| u.label == *sym) else {
                    continue;
                };
                let (factor, _) = unit_symbol_to_si(sym).expect("emitted symbol resolves");
                assert_eq!(
                    factor, rung.si_scale,
                    "conversion factor for {sym:?} ({name}) drifted: \
                     unit_symbol_to_si says {factor}, display ladder says {}",
                    rung.si_scale
                );
                compared += 1;
            }
        }
        assert!(
            compared >= 6,
            "expected the two tables to overlap on at least mm/cm/m/deg/rad/kg, \
             compared only {compared} symbols — did a table lose entries?"
        );
    }

    // ─── `ri_compound_unit_expr` — the COMPOUND emission builder ─────────────
    //
    // Task #6400. Sibling of `ri_emittable_units`, deliberately NOT a widening
    // of it: that table answers "which BARE built-in symbols may be written"
    // and its four guards above (including
    // `ri_emittable_units_is_empty_for_dimensionless_and_unsupported_dimensions`,
    // which asserts AREA/VOLUME/PRESSURE/FORCE/SOLID_ANGLE/MONEY have empty
    // ladders) must stay green and unmodified. This one answers "which
    // `UnitExpr`-shaped string spells this dimension out of SI base symbols",
    // and it carries a STRICTLY STRONGER precondition: the compiler's
    // `resolve_unit_expr` is registry-ONLY with no `unit_to_scalar` fallback,
    // so the caller must assert a seeded registry. The function itself is pure
    // — the opt-in lives at `reify_ir::ri_literal::UnitScope`.

    /// A `DimensionVector` with a single slot set — the escape hatch for
    /// exponents the named constants cannot express.
    ///
    /// Built through the public tuple field rather than `basis_n`, which is
    /// private to `dimension.rs`.
    fn one_slot(index: usize, exponent: crate::Rational) -> DimensionVector {
        let mut exps = [crate::Rational::ZERO; 10];
        exps[index] = exponent;
        DimensionVector(exps)
    }

    /// (a) SHAPES — the exact string emitted, in `BASE_UNIT_SYMBOLS` index
    /// order (0:m 1:kg 2:s 3:A 4:K 5:mol 6:cd 7:rad 8:sr 9:USD).
    ///
    /// Positive-exponent factors come first joined by `*`, then each
    /// negative-exponent factor is introduced by its own `/` carrying the
    /// ABSOLUTE exponent; `^n` is omitted when `|n| == 1`. That is the same
    /// partition and ordering `impl Display for DimensionVector` already uses
    /// (dimension.rs), so the two renderings agree modulo separator and
    /// denominator sign — one convention across both renderers rather than two.
    ///
    /// These strings are load-bearing, not cosmetic: they land in a user's
    /// source file, and `ri_literal_roundtrip.rs` re-parses every one of them
    /// through the real parser and `resolve_unit_expr`.
    #[test]
    fn ri_compound_unit_expr_emits_the_expected_shapes() {
        let cases: &[(DimensionVector, &str)] = &[
            (DimensionVector::AREA, "m^2"),
            (DimensionVector::VOLUME, "m^3"),
            (DimensionVector::FORCE, "m*kg/s^2"),
            (DimensionVector::PRESSURE, "kg/m/s^2"),
            (DimensionVector::MASS_DENSITY, "kg/m^3"),
            (DimensionVector::ENERGY, "m^2*kg/s^2"),
            (DimensionVector::SOLID_ANGLE, "sr"),
            (DimensionVector::MONEY, "USD"),
            // A single atom. The bare ladder still wins at the `reify-ir`
            // policy layer (`80mm`, never `0.08m`) — this function is pure and
            // has no opinion about that; containment lives at the call site.
            (DimensionVector::LENGTH, "m"),
        ];
        for &(dim, expected) in cases {
            assert_eq!(
                ri_compound_unit_expr(&dim).as_deref(),
                Some(expected),
                "compound emission for {:?} drifted",
                dim.canonical_name()
            );
        }
    }

    /// (a2) EMPTY-NUMERATOR fallback. A leading `/` is not a valid `unit_expr`
    /// — the rule must start with an atom — so a dimension whose every
    /// non-zero exponent is negative is written as a product of SIGNED powers
    /// instead. `s^-1` is also simply what a human writes for a frequency.
    #[test]
    fn ri_compound_unit_expr_uses_signed_powers_when_the_numerator_is_empty() {
        assert_eq!(
            ri_compound_unit_expr(&DimensionVector::FREQUENCY).as_deref(),
            Some("s^-1")
        );
    }

    /// (b) REJECTIONS, each for its own distinct reason. Every one of these is
    /// a case where emitting SOMETHING would produce a literal that does not
    /// re-parse — the exact failure this module exists to prevent.
    #[test]
    fn ri_compound_unit_expr_refuses_what_it_cannot_write_exactly() {
        // Nothing to emit.
        assert_eq!(ri_compound_unit_expr(&DimensionVector::DIMENSIONLESS), None);

        // A fractional exponent: `UnitExpr::Pow` is integral, so `m^(1/2)` has
        // no spelling at all.
        assert_eq!(
            ri_compound_unit_expr(&DimensionVector::LENGTH.root(2)),
            None,
            "a fractional exponent has no integral `UnitExpr::Pow` spelling"
        );

        // An exponent outside `i8` — exactly the bound `resolve_unit_expr`
        // enforces via `i8::try_from`, so accepting one here would emit a
        // literal the compiler answers with `ExponentOutOfRange`.
        assert_eq!(
            ri_compound_unit_expr(&one_slot(0, crate::Rational::new(200, 1))),
            None,
            "an exponent outside i8 is what `resolve_unit_expr` rejects"
        );

        // Current / AmountOfSubstance / LuminousIntensity: bare `A`/`mol`/`cd`
        // are in BUILTIN_UNITS but are NEVER seeded into the compiler's
        // `UnitRegistry` (stdlib/units.ri does not declare them, and
        // si_units.rs registers them only as prefix bases `mA`/`kA`/…), so a
        // compound naming one would fail to resolve. VOLTAGE and CHARGE are
        // the witnesses; the raw slots pin the reason rather than the pair.
        for dim in [DimensionVector::VOLTAGE, DimensionVector::CHARGE] {
            assert_eq!(
                ri_compound_unit_expr(&dim),
                None,
                "{:?} names a base symbol the registry never carries",
                dim.canonical_name()
            );
        }
        for slot in [3usize, 5, 6] {
            assert_eq!(
                ri_compound_unit_expr(&one_slot(slot, crate::Rational::ONE)),
                None,
                "slot {slot} ({}) has no registry-resolvable bare symbol",
                BASE_UNIT_SYMBOLS[slot]
            );
        }
    }

    /// (c) STRUCTURAL guard — every alphabetic atom in an emitted string is a
    /// member of [`BASE_UNIT_SYMBOLS`].
    ///
    /// Fires if the builder ever reaches for a symbol outside the base table
    /// (a prefixed `mm`, a named derived `Pa`), which would abandon the
    /// factor-1.0 identity the whole exactness argument rests on.
    #[test]
    fn ri_compound_unit_expr_only_ever_names_base_unit_symbols() {
        let dims = [
            DimensionVector::AREA,
            DimensionVector::VOLUME,
            DimensionVector::FORCE,
            DimensionVector::PRESSURE,
            DimensionVector::MASS_DENSITY,
            DimensionVector::ENERGY,
            DimensionVector::SOLID_ANGLE,
            DimensionVector::MONEY,
            DimensionVector::LENGTH,
            DimensionVector::FREQUENCY,
        ];
        let mut checked = 0usize;
        for dim in dims {
            let expr = ri_compound_unit_expr(&dim)
                .unwrap_or_else(|| panic!("{:?} must be emittable", dim.canonical_name()));
            for atom in expr.split(|c: char| !c.is_alphabetic()) {
                if atom.is_empty() {
                    continue;
                }
                assert!(
                    BASE_UNIT_SYMBOLS.contains(&atom),
                    "emitted expression {expr:?} for {:?} names {atom:?}, which is \
                     not an SI base symbol — only base symbols resolve to factor \
                     exactly 1.0, and that identity is what makes the emitted \
                     magnitude the SI value verbatim",
                    dim.canonical_name()
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 10,
            "expected to sweep at least 10 atoms, saw {checked} — did the shape \
             table lose entries?"
        );
    }

}
