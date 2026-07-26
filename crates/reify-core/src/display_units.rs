//! Per-cell display-unit ladders — the data table backing the GUI's
//! dimension-aware unit picker (task #5199).
//!
//! Each [`DimensionLadder`] lists the selectable display units for one
//! canonical dimension (as named by
//! `reify_core::DimensionVector::canonical_name`). `display_magnitude =
//! si_value / unit.si_scale`. Exactly one option per ladder is marked
//! `is_default`, matching `DimensionVector::to_display_units`'s existing
//! choice — this keeps the picker's default selection numerically identical
//! to the canonical backend-formatted `value`.
//!
//! Each ladder also carries a curated `derived_unit_name` (PRD
//! display-unit-preference §4) — the single per-dimension label, held
//! invariant-equal to the `is_default` rung's label (§4a) so downstream
//! eval/hover surfaces can unify on one string per dimension.
//!
//! Exposed to the frontend via the `get_unit_ladders` Tauri command
//! (`main.rs`). Doubles as the future substrate for auto-scaling defaults
//! and the DSL `@display` annotation follow-up (task #5200).

/// One selectable display unit within a [`DimensionLadder`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UnitOption {
    /// User-facing unit label (e.g. `"mm"`, `"L"`).
    pub label: String,
    /// `display_magnitude = si_value / si_scale`.
    pub si_scale: f64,
    /// Exactly one option per ladder has `is_default: true` — the unit
    /// `DimensionVector::to_display_units` already chooses for that dimension.
    pub is_default: bool,
}

/// Per-dimension auto-scaling policy (PRD display-unit-preference §5).
///
/// When present on a [`DimensionLadder`] it describes whether the display
/// formatter may hop rungs to keep a magnitude inside a readable target band,
/// plus the band itself. `enabled: false` is the opt-in/default-OFF posture
/// (the policy exists but is not applied until the user turns it on). A
/// `None` `auto_scale` on the ladder means the dimension is structurally
/// *excluded* from auto-scaling (e.g. Angle's discrete deg/rad, or a
/// single-rung ladder with no rung to hop across).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutoScale {
    /// Default posture: `true` = auto-scaling applied by default; `false` =
    /// present but opt-in (off until enabled).
    pub enabled: bool,
    /// Inclusive lower bound of the target mantissa band — `1.0` per §5a
    /// (`1 ≤ |mantissa| < 1000`).
    pub band_lo: f64,
    /// Exclusive upper bound of the target mantissa band — `1000.0` per §5a.
    pub band_hi: f64,
}

/// The ordered set of display-unit options for one canonical dimension.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionLadder {
    /// Canonical dimension name (`DimensionVector::canonical_name()`, e.g. `"Volume"`).
    pub dimension: String,
    /// The single curated derived-unit label for this dimension (PRD
    /// display-unit-preference §4). Invariant: always equals the label of
    /// this ladder's `is_default` rung — the default [`UnitOption`] is the
    /// single source of the curated per-dimension name (§4a). Stored as an
    /// explicit denormalized field (rather than re-derived from the rungs)
    /// so a dimension→name lookup is direct and independent of rung
    /// order/count; the equality is pinned by
    /// `every_ladder_exposes_curated_derived_unit_name`.
    pub derived_unit_name: String,
    /// Selectable units, in picker display order.
    pub units: Vec<UnitOption>,
    /// Auto-scaling policy for this dimension (PRD §5), or `None` when the
    /// dimension is *excluded* from auto-scaling (Angle's discrete deg/rad;
    /// the single-rung Force/Energy/Power ladders have no rung to hop).
    pub auto_scale: Option<AutoScale>,
}

/// Return the full set of per-dimension unit ladders.
///
/// Each dimension's `is_default` entry is numerically identical to the unit
/// `DimensionVector::to_display_units` already chooses for that dimension
/// (Length→mm, Area→mm², Volume→mm³, Angle→deg; Mass/Pressure/Density and the
/// single-rung Force/Energy/Power ladders fall through `to_display_units`'s
/// unscaled fallback branch, so their defaults are the coherent-SI base unit
/// — kg, Pa, kg/m³, N, J, W — at `si_scale: 1.0`).
pub fn unit_ladders() -> Vec<DimensionLadder> {
    vec![
        DimensionLadder {
            dimension: "Length".to_string(),
            derived_unit_name: "mm".to_string(),
            auto_scale: Some(AutoScale {
                enabled: true,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "mm".to_string(),
                    si_scale: 1e-3,
                    is_default: true,
                },
                UnitOption {
                    label: "cm".to_string(),
                    si_scale: 1e-2,
                    is_default: false,
                },
                UnitOption {
                    label: "m".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
                UnitOption {
                    label: "in".to_string(),
                    si_scale: 0.0254,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Area".to_string(),
            derived_unit_name: "mm\u{00B2}".to_string(),
            auto_scale: Some(AutoScale {
                enabled: true,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "mm\u{00B2}".to_string(),
                    si_scale: 1e-6,
                    is_default: true,
                },
                UnitOption {
                    label: "cm\u{00B2}".to_string(),
                    si_scale: 1e-4,
                    is_default: false,
                },
                UnitOption {
                    label: "m\u{00B2}".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Volume".to_string(),
            derived_unit_name: "mm\u{00B3}".to_string(),
            auto_scale: Some(AutoScale {
                enabled: true,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "mm\u{00B3}".to_string(),
                    si_scale: 1e-9,
                    is_default: true,
                },
                UnitOption {
                    label: "cm\u{00B3}".to_string(),
                    si_scale: 1e-6,
                    is_default: false,
                },
                UnitOption {
                    label: "L".to_string(),
                    si_scale: 1e-3,
                    is_default: false,
                },
                UnitOption {
                    label: "m\u{00B3}".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Angle".to_string(),
            derived_unit_name: "deg".to_string(),
            auto_scale: None,
            units: vec![
                UnitOption {
                    label: "deg".to_string(),
                    si_scale: std::f64::consts::PI / 180.0,
                    is_default: true,
                },
                UnitOption {
                    label: "rad".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Mass".to_string(),
            derived_unit_name: "kg".to_string(),
            auto_scale: Some(AutoScale {
                enabled: false,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "g".to_string(),
                    si_scale: 1e-3,
                    is_default: false,
                },
                UnitOption {
                    label: "kg".to_string(),
                    si_scale: 1.0,
                    is_default: true,
                },
            ],
        },
        DimensionLadder {
            dimension: "Pressure".to_string(),
            derived_unit_name: "Pa".to_string(),
            auto_scale: Some(AutoScale {
                enabled: false,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "Pa".to_string(),
                    si_scale: 1.0,
                    is_default: true,
                },
                UnitOption {
                    label: "kPa".to_string(),
                    si_scale: 1e3,
                    is_default: false,
                },
                UnitOption {
                    label: "MPa".to_string(),
                    si_scale: 1e6,
                    is_default: false,
                },
                UnitOption {
                    label: "GPa".to_string(),
                    si_scale: 1e9,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Density".to_string(),
            derived_unit_name: "kg/m\u{00B3}".to_string(),
            auto_scale: Some(AutoScale {
                enabled: false,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "kg/m\u{00B3}".to_string(),
                    si_scale: 1.0,
                    is_default: true,
                },
                UnitOption {
                    label: "g/cm\u{00B3}".to_string(),
                    si_scale: 1000.0,
                    is_default: false,
                },
            ],
        },
        // Single-rung coherent-SI ladders (PRD display-unit-preference §4):
        // seed the curated derived-unit name over the existing
        // DimensionVector::FORCE/ENERGY/POWER consts. One is_default rung
        // satisfies the every_ladder_has_exactly_one_default guard; §5
        // assigns them no auto-scale posture (auto_scale = None).
        DimensionLadder {
            dimension: "Force".to_string(),
            derived_unit_name: "N".to_string(),
            auto_scale: None,
            units: vec![UnitOption {
                label: "N".to_string(),
                si_scale: 1.0,
                is_default: true,
            }],
        },
        DimensionLadder {
            dimension: "Energy".to_string(),
            derived_unit_name: "J".to_string(),
            auto_scale: None,
            units: vec![UnitOption {
                label: "J".to_string(),
                si_scale: 1.0,
                is_default: true,
            }],
        },
        DimensionLadder {
            dimension: "Power".to_string(),
            derived_unit_name: "W".to_string(),
            auto_scale: None,
            units: vec![UnitOption {
                label: "W".to_string(),
                si_scale: 1.0,
                is_default: true,
            }],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Find the ladder for a canonical dimension name, panicking with a
    /// helpful message if it is absent (every assertion below expects the
    /// dimension to be present).
    fn ladder<'a>(ladders: &'a [DimensionLadder], dimension: &str) -> &'a DimensionLadder {
        ladders
            .iter()
            .find(|l| l.dimension == dimension)
            .unwrap_or_else(|| panic!("no ladder for dimension {dimension:?}"))
    }

    /// Find a unit option within a ladder by label, panicking if absent.
    fn unit<'a>(ladder: &'a DimensionLadder, label: &str) -> &'a UnitOption {
        ladder
            .units
            .iter()
            .find(|u| u.label == label)
            .unwrap_or_else(|| panic!("no unit {label:?} in ladder {:?}", ladder.dimension))
    }

    #[test]
    fn ladders_exist_for_all_seven_dimensions() {
        let ladders = unit_ladders();
        for dimension in [
            "Length", "Area", "Volume", "Angle", "Mass", "Pressure", "Density",
        ] {
            ladder(&ladders, dimension);
        }
    }

    #[test]
    fn every_ladder_has_exactly_one_default() {
        let ladders = unit_ladders();
        assert!(!ladders.is_empty(), "unit_ladders() must not be empty");
        for l in &ladders {
            let defaults = l.units.iter().filter(|u| u.is_default).count();
            assert_eq!(
                defaults, 1,
                "ladder {:?} should have exactly one is_default unit, found {defaults}",
                l.dimension
            );
        }
    }

    /// §4a curated-name invariant: every ladder exposes a non-empty
    /// `derived_unit_name`, and it is exactly the label of that ladder's
    /// single `is_default` rung — the default [`UnitOption`] is the single
    /// source of the curated per-dimension label. Pins the denormalized
    /// field to its authoritative source so it cannot drift from the
    /// default rung.
    #[test]
    fn every_ladder_exposes_curated_derived_unit_name() {
        for l in &unit_ladders() {
            assert!(
                !l.derived_unit_name.is_empty(),
                "ladder {:?} has an empty derived_unit_name",
                l.dimension
            );
            let default_rung = l
                .units
                .iter()
                .find(|u| u.is_default)
                .unwrap_or_else(|| panic!("ladder {:?} has no is_default unit", l.dimension));
            assert_eq!(
                l.derived_unit_name, default_rung.label,
                "ladder {:?} derived_unit_name must equal its is_default rung label",
                l.dimension
            );
        }
    }

    /// PRD display-unit-preference §4: Force/Energy/Power are seeded as
    /// single-rung ladders (coherent-SI N/J/W @ `si_scale: 1.0`,
    /// `is_default: true`) supplying the curated derived-unit name. A single
    /// `is_default` rung satisfies `every_ladder_has_exactly_one_default`,
    /// and the names round-trip through `canonical_name` (they are
    /// `NAMED_DIMENSIONS` keys — see the round-trip guard below).
    #[test]
    fn force_energy_power_ladders_seeded() {
        let ladders = unit_ladders();
        for (dimension, label) in [("Force", "N"), ("Energy", "J"), ("Power", "W")] {
            let l = ladder(&ladders, dimension);
            assert_eq!(
                l.units.len(),
                1,
                "ladder {dimension:?} should have exactly one rung"
            );
            let rung = &l.units[0];
            assert_eq!(rung.label, label, "{dimension:?} rung label mismatch");
            assert_eq!(rung.si_scale, 1.0, "{dimension:?} rung si_scale must be 1.0");
            assert!(rung.is_default, "{dimension:?} rung must be is_default");
            assert_eq!(
                l.derived_unit_name, label,
                "{dimension:?} derived_unit_name mismatch"
            );
        }
    }

    /// Collapsed data-driven replacement for five former per-ladder pin
    /// tests (task #5199 amend, reviewer_comprehensive test_coverage
    /// finding): each hand-copied the same `si_scale`/`is_default`
    /// constants the constructor above already contains, and — for the
    /// `is_default` rungs — duplicated what
    /// `default_si_scale_matches_to_display_units_numeric_value` below
    /// already locks against its true source (`to_display_units`). That
    /// test only exercises each ladder's DEFAULT rung though, so this table
    /// keeps ONLY the non-default-rung coverage (cm/m/in, L, g, g/cm³, …)
    /// the five tests used to provide — the default rungs themselves
    /// (Volume mm³, Length mm, Mass kg, Pressure Pa, Density kg/m³) are
    /// deliberately omitted here since pinning them would just re-assert
    /// the constructor's own constants back at itself; their correctness is
    /// covered, against the real source, by the anchored test below (task
    /// #5199 amend, reviewer_comprehensive test_coverage finding — round 2).
    #[test]
    fn ladder_units_pin_expected_scale_and_default() {
        let ladders = unit_ladders();

        // (dimension, label, si_scale, is_default)
        let expected: &[(&str, &str, f64, bool)] = &[
            ("Volume", "L", 1e-3, false),
            ("Length", "cm", 1e-2, false),
            ("Length", "m", 1.0, false),
            ("Length", "in", 0.0254, false),
            ("Mass", "g", 1e-3, false),
            ("Density", "g/cm\u{00B3}", 1000.0, false),
        ];
        for &(dimension, label, si_scale, is_default) in expected {
            let u = unit(ladder(&ladders, dimension), label);
            assert_eq!(u.si_scale, si_scale, "{dimension}/{label} si_scale mismatch");
            assert_eq!(u.is_default, is_default, "{dimension}/{label} is_default mismatch");
        }

        // Pressure lists additional rungs above Pa; scales aren't pinned
        // here — just confirm they exist as selectable rungs on the ladder.
        let pressure = ladder(&ladders, "Pressure");
        for label in ["kPa", "MPa", "GPa"] {
            unit(pressure, label);
        }
    }

    /// Drift guard: every ladder key must be a *real* canonical dimension
    /// name — i.e. round-tripping the name back to its `DimensionVector` (via
    /// the shared `NAMED_DIMENSIONS` table) and calling `canonical_name()` on
    /// it must yield the same name back. This catches alias collisions (e.g.
    /// `TranslationalStiffness`/`Stiffness`) where a name is a valid key into
    /// `NAMED_DIMENSIONS` but is not what `canonical_name()` actually
    /// produces for that dimension — such a ladder would never be reachable
    /// from `build_values`'s `dim.canonical_name()` call.
    #[test]
    fn every_ladder_dimension_round_trips_through_canonical_name() {
        for l in &unit_ladders() {
            let dim = crate::NAMED_DIMENSIONS
                .iter()
                .find(|(_, name)| *name == l.dimension)
                .map(|(dim, _)| *dim)
                .unwrap_or_else(|| {
                    panic!(
                        "ladder dimension {:?} is not a NAMED_DIMENSIONS name",
                        l.dimension
                    )
                });
            assert_eq!(
                dim.canonical_name(),
                Some(l.dimension.as_str()),
                "ladder dimension {:?} does not round-trip through canonical_name()",
                l.dimension
            );
        }
    }

    /// PRD display-unit-preference §5: structural auto-scale posture.
    ///
    /// Auto-scaling has no consumer yet — the `enabled` flag is documented as
    /// applied by no formatter until §5 rung-hopping lands — so this test pins
    /// only the structural invariants that are NOT a tautological echo of the
    /// constructor's own policy literals:
    ///   * the dimensions structurally *excluded* from auto-scaling
    ///     (`auto_scale == None`) are exactly Angle (discrete deg/rad) and the
    ///     single-rung Force/Energy/Power ladders (no rung to hop) — guarded in
    ///     both directions (named set + count) so a dimension can't silently
    ///     drop out of, or into, auto-scaling;
    ///   * every *included* dimension carries the §5a `1 ≤ |mantissa| < 1000`
    ///     target band (`band_lo == 1.0`, `band_hi == 1000.0`).
    ///
    /// The per-dimension default-ON/OFF `enabled` literal is deliberately NOT
    /// re-asserted here: with no auto-scaling consumer to anchor the posture
    /// against, pinning it would only re-state the constructor's own constants
    /// back at itself (same rationale as the sibling
    /// `ladder_units_pin_expected_scale_and_default` amend). Restore that pin
    /// in the follow-up that implements §5 rung-hopping (task #5200), which
    /// gives a real consumer to anchor the `enabled` posture against.
    #[test]
    fn auto_scale_metadata_matches_prd_section5() {
        let ladders = unit_ladders();

        // Structural include/exclude partition: exactly these four dimensions
        // are excluded from auto-scaling (auto_scale == None). Pinning the
        // count alongside the named set closes both directions without
        // coupling to constructor order.
        let excluded_count = ladders.iter().filter(|l| l.auto_scale.is_none()).count();
        assert_eq!(
            excluded_count, 4,
            "exactly four ladders should be excluded from auto-scaling (auto_scale == None)"
        );
        for dimension in ["Angle", "Force", "Energy", "Power"] {
            assert_eq!(
                ladder(&ladders, dimension).auto_scale,
                None,
                "ladder {dimension:?} must be excluded from auto-scaling (auto_scale == None)"
            );
        }

        // §5a: every included (Some) dimension uses the 1 ≤ |mantissa| < 1000 band.
        for l in &ladders {
            if let Some(a) = &l.auto_scale {
                assert_eq!(a.band_lo, 1.0, "ladder {:?} band_lo mismatch", l.dimension);
                assert_eq!(a.band_hi, 1000.0, "ladder {:?} band_hi mismatch", l.dimension);
            }
        }
    }

    /// Drift guard for the module doc's core invariant: each ladder's
    /// `is_default` entry is "numerically identical" to what
    /// `DimensionVector::to_display_units` already chooses. The pin test
    /// above (`ladder_units_pin_expected_scale_and_default`) only checks the
    /// ladder's own hand-copied constants against each other; it would keep
    /// passing even if `to_display_units` itself drifted (e.g. its Length
    /// default silently changed unit). This test instead runs a known SI
    /// magnitude through `to_display_units` directly and checks that
    /// dividing by the ladder's default `si_scale` reproduces its converted
    /// value, locking the invariant to its actual source.
    ///
    /// Only the numeric value is compared, not the unit label:
    /// Mass/Pressure/Density intentionally use a more specific default label
    /// (`"kg"`, `"Pa"`, `"kg/m³"`) than `to_display_units`'s generic `"SI"`
    /// fallback label (see module doc above) — that label divergence is
    /// deliberate, not drift.
    #[test]
    fn default_si_scale_matches_to_display_units_numeric_value() {
        let ladders = unit_ladders();
        let cases: &[(crate::DimensionVector, f64)] = &[
            (crate::DimensionVector::LENGTH, 0.08),
            (crate::DimensionVector::AREA, 0.0045),
            (crate::DimensionVector::VOLUME, 0.00704500224),
            (crate::DimensionVector::ANGLE, 1.2),
            (crate::DimensionVector::MASS, 2.5),
            (crate::DimensionVector::PRESSURE, 101_325.0),
            (crate::DimensionVector::MASS_DENSITY, 7850.0),
            // Single-rung coherent-SI ladders (§4): to_display_units passes
            // them through unscaled, so their default si_scale must be 1.0.
            (crate::DimensionVector::FORCE, 250.0),
            (crate::DimensionVector::ENERGY, 1500.0),
            (crate::DimensionVector::POWER, 750.0),
        ];

        for &(dim, si_value) in cases {
            let name = dim
                .canonical_name()
                .unwrap_or_else(|| panic!("{dim:?} has no canonical_name"));
            let l = ladder(&ladders, name);
            let default_unit = l
                .units
                .iter()
                .find(|u| u.is_default)
                .unwrap_or_else(|| panic!("ladder {name:?} has no is_default unit"));

            let (expected_value, _label) = dim.to_display_units(si_value);
            let actual_value = si_value / default_unit.si_scale;
            let tolerance = 1e-9 * expected_value.abs().max(1.0);

            assert!(
                (actual_value - expected_value).abs() <= tolerance,
                "ladder {name:?} default si_scale ({}) diverges from to_display_units: \
                 si_value/si_scale = {actual_value}, to_display_units = {expected_value}",
                default_unit.si_scale,
            );
        }
    }

    // ── §5 auto-scaling policy: rung selection (task #5236) ──────────────────
    //
    // PRD display-unit-preference §5a/§5b/§5e. [`DimensionLadder::auto_scaled`]
    // is the *policy* half of the leaf: given an SI magnitude it decides which
    // rung (if any) this ladder should render at, or that no rung fits and the
    // §5c engineering-notation fallback applies. The *rendering* half (the
    // magnitude string and §5c's `×10ⁿ` glyphs) lives in reify-ir beside
    // `format_display_number`, so an auto-scaled magnitude stays numerically
    // identical in convention to every other display magnitude.

    /// Magnitudes spanning `10^lo_exp ..= 10^hi_exp`, four samples per decade.
    ///
    /// The exact powers of ten are deliberately included: `log10` on an exact
    /// power of ten can land a hair below the integer, which would floor an
    /// engineering exponent one step low — the §5c normalization invariants
    /// below exist to catch exactly that.
    fn magnitude_sweep(lo_exp: i32, hi_exp: i32) -> Vec<f64> {
        let mut out = Vec::new();
        for e in lo_exp..=hi_exp {
            let decade = 10f64.powi(e);
            for mantissa in [1.0, 2.5, 4.2, 7.5] {
                out.push(mantissa * decade);
            }
        }
        out
    }

    /// The label of whichever rung a choice selected (the anchor rung, for the
    /// engineering variant), or `None` for [`AutoScaleChoice::Static`].
    fn chosen_label<'a>(choice: &AutoScaleChoice<'a>) -> Option<&'a str> {
        match choice {
            AutoScaleChoice::Static => None,
            AutoScaleChoice::Rung(u) => Some(u.label.as_str()),
            AutoScaleChoice::Engineering { rung, .. } => Some(rung.label.as_str()),
        }
    }

    /// §5b/§5e posture gate. A dimension structurally *excluded* from
    /// auto-scaling (`auto_scale: None` — Angle's discrete deg/rad, and the
    /// single-rung Force/Energy/Power ladders with no rung to hop) or
    /// *default-OFF* (`enabled: false` — Mass, Pressure, Density) never hops a
    /// rung, at any magnitude. §5e states this as one rule: a default-OFF
    /// dimension "renders its static default rung's raw magnitude, full stop",
    /// and engineering notation "does **not** apply to default-OFF dimensions
    /// either" — so `Static` is the only admissible answer for both groups.
    #[test]
    fn auto_scaled_posture_gate_keeps_excluded_and_default_off_dims_static() {
        let ladders = unit_ladders();
        for dimension in [
            "Angle", "Force", "Energy", "Power", "Mass", "Pressure", "Density",
        ] {
            let l = ladder(&ladders, dimension);
            for si_value in magnitude_sweep(-12, 12) {
                assert_eq!(
                    l.auto_scaled(si_value),
                    AutoScaleChoice::Static,
                    "ladder {dimension:?} must not auto-scale at si_value {si_value}"
                );
            }
        }

        // The out-of-band magnitudes §5b's rationale names explicitly: a Mass
        // that would otherwise flip 2.5 kg → 2500 g, a Pressure hopping
        // Pa→kPa→MPa→GPa, and a Density well past the band's top.
        for (dimension, si_value) in [
            ("Mass", 2.5e-6),
            ("Pressure", 1.01325e5),
            ("Density", 7850.0),
        ] {
            assert_eq!(
                ladder(&ladders, dimension).auto_scaled(si_value),
                AutoScaleChoice::Static,
                "{dimension:?} is default-OFF/excluded; {si_value} must not trigger a hop"
            );
        }
    }

    /// §5a rung hops. A default-ON dimension picks the rung that keeps
    /// `|mantissa|` inside the `1 ≤ |mantissa| < 1000` band. `0.08 m` needs no
    /// hop at all (mm already reads 80), while larger and smaller magnitudes
    /// walk the ladder. The Volume case is the PRD's own G1 figure
    /// (`0.007 m³ → 7 L`) — reached here at §6(1) rung 3 with no `@display`
    /// pin whatsoever, which is what §5e's unpinned rule buys.
    #[test]
    fn auto_scaled_hops_to_the_rung_that_lands_in_band() {
        let ladders = unit_ladders();

        let length = ladder(&ladders, "Length");
        assert_eq!(
            length.auto_scaled(0.08),
            AutoScaleChoice::Rung(unit(length, "mm")),
            "0.08 m already reads 80 mm — in band, so no hop"
        );
        assert_eq!(
            length.auto_scaled(5.0),
            AutoScaleChoice::Rung(unit(length, "cm")),
            "5.0 m is 5000 mm (out of band) but 500 cm — one hop"
        );
        assert_eq!(
            length.auto_scaled(50.0),
            AutoScaleChoice::Rung(unit(length, "m")),
            "50.0 m is 50000 mm / 5000 cm (both out of band) but 50 m — two hops"
        );

        let area = ladder(&ladders, "Area");
        assert_eq!(
            area.auto_scaled(0.0045),
            AutoScaleChoice::Rung(unit(area, "cm\u{00B2}")),
            "0.0045 m² is 4500 mm² (out of band) but 45 cm²"
        );

        let volume = ladder(&ladders, "Volume");
        assert_eq!(
            volume.auto_scaled(0.007),
            AutoScaleChoice::Rung(unit(volume, "L")),
            "PRD G1: 0.007 m³ is 7000000 mm³ / 7000 cm³ (both out of band) but 7 L"
        );
    }

    /// §5b stability rationale, expressed as the tie-break: when several
    /// eligible rungs are simultaneously in band, the winner is the one
    /// *nearest the ladder's default rung index* — auto-scaling moves as few
    /// rungs as possible off the dimension's familiar default. `0.5 m` is both
    /// `500 mm` and `50 cm`; a mm-default CAD DSL must read it as 500 mm.
    #[test]
    fn auto_scaled_tie_break_prefers_the_rung_nearest_the_default() {
        let ladders = unit_ladders();
        let length = ladder(&ladders, "Length");

        // Both mm (500) and cm (50) are in [1, 1000); mm is the default rung.
        assert_eq!(
            length.auto_scaled(0.5),
            AutoScaleChoice::Rung(unit(length, "mm"))
        );
        // Both cm (500) and m (5) are in band; cm is one hop from the default,
        // m is two — so the nearer rung wins even when neither is the default.
        assert_eq!(
            length.auto_scaled(5.0),
            AutoScaleChoice::Rung(unit(length, "cm"))
        );
    }

    /// §5a's "one SI-prefix step": a candidate rung is eligible only when its
    /// `si_scale` is a power-of-ten multiple of the default rung's. Length's
    /// `in` rung (`si_scale 0.0254`, ratio 25.4 to the mm default) is the one
    /// exclusion in the whole registry — every other rung across all seven
    /// multi-rung ladders is already a decimal sibling of its default. Without
    /// the filter, a magnitude-driven rule could render an unpinned *metric*
    /// length in inches: a unit-*system* flip, strictly worse than the
    /// magnitude flip §5b's rationale is written to avoid.
    ///
    /// The filter is belt-and-braces here rather than the sole guard: under the
    /// minimal-hop tie-break `in` sits last in candidate order, and its in-band
    /// SI span `[0.0254, 25.4)` is fully subsumed by mm ∪ cm ∪ m
    /// (`[1e-3, 1000)`), so it would be unreachable by subsumption too. Pinning
    /// it explicitly keeps the §5a eligibility rule normative in code rather
    /// than an accident of the current rung ordering.
    #[test]
    fn auto_scaled_never_selects_the_non_decimal_inch_rung() {
        let ladders = unit_ladders();
        let length = ladder(&ladders, "Length");

        for si_value in magnitude_sweep(-4, 4) {
            for signed in [si_value, -si_value] {
                let choice = length.auto_scaled(signed);
                assert_ne!(
                    chosen_label(&choice),
                    Some("in"),
                    "auto-scaling must never flip an unpinned metric Length into inches \
                     (si_value {signed}, choice {choice:?})"
                );
            }
        }
    }

    /// Guards that must degrade to [`AutoScaleChoice::Static`] — i.e. to
    /// today's static-default-rung rendering — rather than hop or reach §5c's
    /// engineering notation. Zero is the load-bearing case: it must render as
    /// `0 mm`, never `0×10⁰ mm`.
    #[test]
    fn auto_scaled_zero_and_non_finite_magnitudes_stay_static() {
        let ladders = unit_ladders();
        for dimension in ["Length", "Area", "Volume"] {
            let l = ladder(&ladders, dimension);
            for si_value in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert_eq!(
                    l.auto_scaled(si_value),
                    AutoScaleChoice::Static,
                    "ladder {dimension:?} must stay static at si_value {si_value}"
                );
            }
        }
    }

    /// §5a's band is stated on `|mantissa|`, so a negative magnitude selects
    /// exactly the rung its absolute value would.
    #[test]
    fn auto_scaled_selects_on_absolute_magnitude() {
        let ladders = unit_ladders();
        let area = ladder(&ladders, "Area");
        assert_eq!(
            area.auto_scaled(-0.0045),
            AutoScaleChoice::Rung(unit(area, "cm\u{00B2}")),
            "the band is on |mantissa|, so -0.0045 m² picks the same rung as +0.0045"
        );

        for dimension in ["Length", "Area", "Volume"] {
            let l = ladder(&ladders, dimension);
            for si_value in magnitude_sweep(-6, 6) {
                assert_eq!(
                    chosen_label(&l.auto_scaled(si_value)),
                    chosen_label(&l.auto_scaled(-si_value)),
                    "ladder {dimension:?} picked different rungs for ±{si_value}"
                );
            }
        }
    }
}
