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

    /// PRD display-unit-preference §5: per-dimension auto-scale posture.
    /// Three postures — default-ON (Length/Area/Volume), opt-in/default-OFF
    /// (Mass/Pressure/Density), and excluded/`None` (Angle is discrete
    /// deg/rad; the single-rung Force/Energy/Power ladders have no rung to
    /// hop). The `1 ≤ |mantissa| < 1000` target band (§5a) is stored
    /// per-entry as `band_lo == 1.0`, `band_hi == 1000.0`.
    #[test]
    fn auto_scale_metadata_matches_prd_section5() {
        let ladders = unit_ladders();

        let on = Some(AutoScale {
            enabled: true,
            band_lo: 1.0,
            band_hi: 1000.0,
        });
        let off = Some(AutoScale {
            enabled: false,
            band_lo: 1.0,
            band_hi: 1000.0,
        });

        // (dimension, expected auto_scale posture)
        let expected: &[(&str, Option<AutoScale>)] = &[
            ("Length", on.clone()),
            ("Area", on.clone()),
            ("Volume", on.clone()),
            ("Mass", off.clone()),
            ("Pressure", off.clone()),
            ("Density", off.clone()),
            ("Angle", None),
            ("Force", None),
            ("Energy", None),
            ("Power", None),
        ];
        for (dimension, want) in expected {
            let l = ladder(&ladders, dimension);
            assert_eq!(
                &l.auto_scale, want,
                "ladder {dimension:?} auto_scale posture mismatch"
            );
        }

        // §5a: every Some(AutoScale) uses the 1 ≤ |mantissa| < 1000 band.
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
}
