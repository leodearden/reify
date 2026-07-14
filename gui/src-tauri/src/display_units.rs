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

    #[test]
    fn volume_ladder_pins_mm3_default_and_litre() {
        let ladders = unit_ladders();
        let volume = ladder(&ladders, "Volume");

        let mm3 = unit(volume, "mm\u{00B3}");
        assert_eq!(mm3.si_scale, 1e-9);
        assert!(mm3.is_default);

        let l = unit(volume, "L");
        assert_eq!(l.si_scale, 1e-3);
        assert!(!l.is_default);
    }

    #[test]
    fn length_ladder_pins_mm_default_cm_m_in() {
        let ladders = unit_ladders();
        let length = ladder(&ladders, "Length");

        let mm = unit(length, "mm");
        assert_eq!(mm.si_scale, 1e-3);
        assert!(mm.is_default);

        let cm = unit(length, "cm");
        assert_eq!(cm.si_scale, 1e-2);
        assert!(!cm.is_default);

        let m = unit(length, "m");
        assert_eq!(m.si_scale, 1.0);
        assert!(!m.is_default);

        let inch = unit(length, "in");
        assert_eq!(inch.si_scale, 0.0254);
        assert!(!inch.is_default);
    }

    #[test]
    fn mass_ladder_pins_g_and_kg_default() {
        let ladders = unit_ladders();
        let mass = ladder(&ladders, "Mass");

        let g = unit(mass, "g");
        assert_eq!(g.si_scale, 1e-3);
        assert!(!g.is_default);

        let kg = unit(mass, "kg");
        assert_eq!(kg.si_scale, 1.0);
        assert!(kg.is_default);
    }

    #[test]
    fn pressure_ladder_pins_pa_default_and_lists_kpa_mpa_gpa() {
        let ladders = unit_ladders();
        let pressure = ladder(&ladders, "Pressure");

        let pa = unit(pressure, "Pa");
        assert_eq!(pa.si_scale, 1.0);
        assert!(pa.is_default);

        // kPa/MPa/GPa scales aren't pinned here — just confirm they exist as
        // selectable rungs on the ladder.
        unit(pressure, "kPa");
        unit(pressure, "MPa");
        unit(pressure, "GPa");
    }

    #[test]
    fn density_ladder_pins_si_default_and_g_per_cm3() {
        let ladders = unit_ladders();
        let density = ladder(&ladders, "Density");

        let si = unit(density, "kg/m\u{00B3}");
        assert_eq!(si.si_scale, 1.0);
        assert!(si.is_default);

        let g_cm3 = unit(density, "g/cm\u{00B3}");
        assert_eq!(g_cm3.si_scale, 1000.0);
        assert!(!g_cm3.is_default);
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
}
