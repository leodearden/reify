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

use crate::DimensionVector;

/// Look up a built-in unit symbol's SI conversion factor and dimension.
///
/// Returns `Some((factor, dimension))` such that `si_value = value * factor`,
/// or `None` if `unit` is not one of the hardcoded built-in symbols.
///
/// This is the single source of truth copied verbatim (values and all) from
/// the pre-extraction `reify-compiler::units::unit_to_scalar` match arms;
/// that function now delegates here. Does not resolve user-declared units
/// (e.g. `km`, `ft`, `thou`) — those live only in the compiler's per-module
/// `UnitRegistry`, which has no equivalent at this layer.
pub fn unit_symbol_to_si(unit: &str) -> Option<(f64, DimensionVector)> {
    match unit {
        "mm" => Some((0.001, DimensionVector::LENGTH)),
        "cm" => Some((0.01, DimensionVector::LENGTH)),
        "m" => Some((1.0, DimensionVector::LENGTH)),
        "in" => Some((0.0254, DimensionVector::LENGTH)),
        "deg" => Some((std::f64::consts::PI / 180.0, DimensionVector::ANGLE)),
        "rad" => Some((1.0, DimensionVector::ANGLE)),
        "kg" => Some((1.0, DimensionVector::MASS)),
        "g" => Some((0.001, DimensionVector::MASS)),
        "s" => Some((1.0, DimensionVector::TIME)),
        // Kelvin needs a hardcoded fallback because `std.units` itself uses
        // `1K` in `BOLTZMANN_CONSTANT()`s body — fn bodies in std.units load
        // with no unit_registry seeded, so the K declared at units.ri can't
        // satisfy the same file's own quantity literals. Mirrors the kg/s/m
        // self-bootstrap entries above.
        "K" => Some((1.0, DimensionVector::TEMPERATURE)),
        // Bare SI base units completing the standard set (factor 1.0).
        // A/mol/cd are the SI bases for Current/AmountOfSubstance/LuminousIntensity;
        // they need the same hardcoded fallback as kg/s/K so that stdlib fn bodies
        // and other unseeded-registry scopes can resolve these unit literals
        // (PRD §2.2 / decision D5).
        "A" => Some((1.0, DimensionVector::CURRENT)),
        "mol" => Some((1.0, DimensionVector::AMOUNT_OF_SUBSTANCE)),
        "cd" => Some((1.0, DimensionVector::LUMINOUS_INTENSITY)),
        _ => None,
    }
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

    /// Every bare symbol [`unit_symbol_to_si`] resolves — the ONE edit site a
    /// new arm needs, shared by the forward tests below and by the reverse
    /// coupling guard `every_builtin_symbol_has_an_emission_ladder`.
    ///
    /// Completeness is not a convention here: `builtin_symbol_list_is_complete`
    /// re-derives the arm list from this very file's own source text and fails
    /// if the two disagree, so a symbol cannot be added to (or removed from)
    /// the match without landing here too.
    const BUILTIN_UNIT_SYMBOLS: &[&str] = &[
        "mm", "cm", "m", "in", "deg", "rad", "kg", "g", "s", "K", "A", "mol", "cd",
    ];

    /// The `BUILTIN_UNIT_SYMBOLS` list is exactly [`unit_symbol_to_si`]'s arms.
    ///
    /// Rust cannot iterate a `match`'s arms, and a hand-maintained mirror list
    /// that only *forward*-checks (every listed symbol resolves) would happily
    /// go stale when a NEW arm is added — which is precisely the regression the
    /// reverse-coupling guard exists to catch. So this re-derives the arm set
    /// from the module's source with `include_str!`, which is resolved relative
    /// to this file at compile time and therefore needs no runtime path.
    ///
    /// A reformat of the match (splitting an arm across lines) fails this test
    /// loudly rather than silently weakening the guard — that is the intended
    /// trade, and the panic message says what to do.
    #[test]
    fn builtin_symbol_list_is_complete() {
        let src = include_str!("units.rs");
        let (_, after) = src
            .split_once("pub fn unit_symbol_to_si")
            .expect("this file defines unit_symbol_to_si");
        let (body, _) = after
            .split_once("\npub fn ")
            .expect("another `pub fn` follows unit_symbol_to_si and bounds its body");

        let mut derived: Vec<&str> = body
            .lines()
            .filter_map(|line| {
                let rest = line.trim_start().strip_prefix('"')?;
                let (symbol, tail) = rest.split_once('"')?;
                tail.trim_start().starts_with("=> Some((").then_some(symbol)
            })
            .collect();
        derived.sort_unstable();

        let mut listed: Vec<&str> = BUILTIN_UNIT_SYMBOLS.to_vec();
        listed.sort_unstable();

        assert_eq!(
            derived, listed,
            "BUILTIN_UNIT_SYMBOLS has drifted from unit_symbol_to_si's arms. \
             Derived from source: {derived:?}; listed: {listed:?}. Add (or drop) \
             the symbol in the const, and give any NEW dimension an emission \
             ladder in ri_emittable_units — see \
             every_builtin_symbol_has_an_emission_ladder."
        );
    }

    /// Forward: every listed symbol still resolves. Catches a deleted arm.
    #[test]
    fn every_builtin_symbol_resolves() {
        for sym in BUILTIN_UNIT_SYMBOLS {
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
    /// regression above. `BUILTIN_UNIT_SYMBOLS` is kept honest by
    /// `builtin_symbol_list_is_complete`, so a new arm cannot dodge it.
    ///
    /// Deliberately asserts only NON-EMPTINESS, not membership: `g` and `in` are
    /// resolvable built-ins that are correctly absent from their canonical
    /// ladders (reachable only through a caller-supplied hint). What must hold is
    /// that their *dimension* is writable at all.
    #[test]
    fn every_builtin_symbol_has_an_emission_ladder() {
        for sym in BUILTIN_UNIT_SYMBOLS {
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
}
