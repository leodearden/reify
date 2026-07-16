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
//! Exposed to the frontend via the `get_unit_ladders` Tauri command
//! (`main.rs`). Doubles as the future substrate for auto-scaling defaults
//! and the DSL `@display` annotation follow-up (task #5200).

use serde::Serialize;

/// One selectable display unit within a [`DimensionLadder`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnitOption {
    /// User-facing unit label (e.g. `"mm"`, `"L"`).
    pub label: String,
    /// `display_magnitude = si_value / si_scale`.
    pub si_scale: f64,
    /// Exactly one option per ladder has `is_default: true` — the unit
    /// `DimensionVector::to_display_units` already chooses for that dimension.
    pub is_default: bool,
}

/// The ordered set of display-unit options for one canonical dimension.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionLadder {
    /// Canonical dimension name (`DimensionVector::canonical_name()`, e.g. `"Volume"`).
    pub dimension: String,
    /// Selectable units, in picker display order.
    pub units: Vec<UnitOption>,
}

/// Return the full set of per-dimension unit ladders.
///
/// Each dimension's `is_default` entry is numerically identical to the unit
/// `DimensionVector::to_display_units` already chooses for that dimension
/// (Length→mm, Area→mm², Volume→mm³, Angle→deg; Mass/Pressure/Density fall
/// through `to_display_units`'s `"SI"` fallback branch, so their defaults are
/// the raw SI base unit — kg, Pa, kg/m³ — at `si_scale: 1.0`).
pub fn unit_ladders() -> Vec<DimensionLadder> {
    vec![
        DimensionLadder {
            dimension: "Length".to_string(),
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
            let dim = reify_core::NAMED_DIMENSIONS
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
        let cases: &[(reify_core::DimensionVector, f64)] = &[
            (reify_core::DimensionVector::LENGTH, 0.08),
            (reify_core::DimensionVector::AREA, 0.0045),
            (reify_core::DimensionVector::VOLUME, 0.00704500224),
            (reify_core::DimensionVector::ANGLE, 1.2),
            (reify_core::DimensionVector::MASS, 2.5),
            (reify_core::DimensionVector::PRESSURE, 101_325.0),
            (reify_core::DimensionVector::MASS_DENSITY, 7850.0),
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
