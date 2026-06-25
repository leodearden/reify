// SPDX-License-Identifier: AGPL-3.0-or-later

//! Structured `Toolpath` value + PrusaSlicer G-code-comment parser (task ζ).
//!
//! See `docs/prds/v0_5/fdm-as-printed-fea.md` §"The Toolpath representation"
//! (task ζ, slice 2). A [`Toolpath`] is an ordered, layer-segmented bead graph:
//! each bead carries its centerline polyline, extrusion width/height, a
//! structural [`BeadRole`], its owning layer index + layer-Z, the nominal
//! extruder temperature, and the active speed; the toolpath additionally
//! records in-layer and inter-layer bead adjacency. The downstream θ
//! `FDMPrint` constitutive mapping consumes this graph (and owns the mm→SI
//! conversion — this module stores native G-code millimetres / mm·min⁻¹
//! exactly as parsed, losslessly).
//!
//! # Why this lives here and not in reify-gcode
//!
//! `reify-gcode` is the low-level command parser; the `Toolpath` abstraction
//! is owned here (PRD design decision #5 — "reify-gcode stays the low-level
//! parser beneath it"). Critically, `reify_gcode::parse_marlin` strips every
//! `;`-to-EOL comment via `strip_comment_and_trim`, so a whole-source call
//! would throw away exactly the `;TYPE:` / `;WIDTH:` / `;HEIGHT:` /
//! `;LAYER_CHANGE` / `;Z:` markers this builder needs — and lose the
//! comment↔move interleaving that tags each bead. Therefore the parser here
//! walks physical lines itself (owning the comment state machine + position
//! sweep) and delegates ONLY G0/G1/G2/G3/G92 move lines to
//! `reify_gcode::parse_marlin(line)` per-line. reify-gcode is reused, not
//! modified.

/// Structural role of a deposited bead, distilled from PrusaSlicer's much
/// larger `ExtrusionRole` (`;TYPE:` comment) vocabulary into the five classes
/// the downstream θ constitutive mapping distinguishes.
///
/// Sacrificial / non-part roles (skirt, brim, wipe tower, …) have **no**
/// variant here — [`role_from_prusaslicer_type`] returns `None` for them and
/// their extrusions are skipped, so they never pollute the bead graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeadRole {
    /// Perimeter shell — PrusaSlicer `Perimeter`, `External perimeter`,
    /// `Overhang perimeter`.
    Perimeter,
    /// Dense solid region — PrusaSlicer `Solid infill`, `Top solid infill`,
    /// `Bottom solid infill`, plus `Gap fill` and `Ironing` (both dense solid
    /// material).
    SolidInfill,
    /// Sparse interior lattice — PrusaSlicer `Internal infill`.
    SparseInfill,
    /// Bridged span — PrusaSlicer `Bridge infill`, `Internal bridge infill`.
    Bridge,
    /// Support structure — PrusaSlicer `Support material`,
    /// `Support material interface`.
    Support,
}

/// Map a PrusaSlicer `;TYPE:` value (the trimmed string after the colon) to a
/// structural [`BeadRole`], or `None` for a sacrificial / non-part /
/// unrecognised type whose extrusions must be skipped.
///
/// Matching is **exact** (case-sensitive) on the canonical PrusaSlicer
/// `ExtrusionRole` display strings. An unknown string yields `None` (skipped),
/// never a hard error — this keeps the parser forward-compatible with future
/// slicer TYPE strings. The groups mirror PrusaSlicer's `ExtrusionRole`
/// enum (`src/libslic3r/ExtrusionEntity.hpp` / GCodeViewer legend):
///
/// - `Perimeter` / `External perimeter` / `Overhang perimeter` → [`BeadRole::Perimeter`]
/// - `Internal infill` → [`BeadRole::SparseInfill`]
/// - `Solid infill` / `Top solid infill` / `Bottom solid infill` / `Gap fill`
///   / `Ironing` → [`BeadRole::SolidInfill`] (all dense solid material)
/// - `Bridge infill` / `Internal bridge infill` → [`BeadRole::Bridge`]
/// - `Support material` / `Support material interface` → [`BeadRole::Support`]
/// - everything else (`Skirt/Brim`, `Wipe tower`, `Custom`, unknown) → `None`
pub fn role_from_prusaslicer_type(type_str: &str) -> Option<BeadRole> {
    match type_str {
        "Perimeter" | "External perimeter" | "Overhang perimeter" => Some(BeadRole::Perimeter),
        "Internal infill" => Some(BeadRole::SparseInfill),
        "Solid infill" | "Top solid infill" | "Bottom solid infill" | "Gap fill" | "Ironing" => {
            Some(BeadRole::SolidInfill)
        }
        "Bridge infill" | "Internal bridge infill" => Some(BeadRole::Bridge),
        "Support material" | "Support material interface" => Some(BeadRole::Support),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── step-1: role mapping ─────────────────────────────────────────────────

    #[test]
    fn perimeter_types_map_to_perimeter() {
        // PrusaSlicer ExtrusionRole strings that are all perimeter shell.
        for s in ["External perimeter", "Perimeter", "Overhang perimeter"] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::Perimeter),
                "{s:?} should map to Perimeter"
            );
        }
    }

    #[test]
    fn internal_infill_maps_to_sparse_infill() {
        assert_eq!(
            role_from_prusaslicer_type("Internal infill"),
            Some(BeadRole::SparseInfill),
            "Internal infill is the sparse interior lattice"
        );
    }

    #[test]
    fn solid_and_dense_types_map_to_solid_infill() {
        // Solid/top/bottom skin + gap fill + ironing are all dense solid material.
        for s in [
            "Solid infill",
            "Top solid infill",
            "Bottom solid infill",
            "Gap fill",
            "Ironing",
        ] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::SolidInfill),
                "{s:?} should map to SolidInfill"
            );
        }
    }

    #[test]
    fn bridge_types_map_to_bridge() {
        for s in ["Bridge infill", "Internal bridge infill"] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::Bridge),
                "{s:?} should map to Bridge"
            );
        }
    }

    #[test]
    fn support_types_map_to_support() {
        for s in ["Support material", "Support material interface"] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::Support),
                "{s:?} should map to Support"
            );
        }
    }

    #[test]
    fn sacrificial_and_unknown_types_map_to_none() {
        // Sacrificial / non-part / unrecognised TYPEs are skipped (None), never
        // a hard error — keeps the parser forward-compatible with new strings.
        for s in [
            "Skirt/Brim",
            "Wipe tower",
            "Custom",
            "Travel",
            "",
            "perimeter", // case-sensitive: lowercase is NOT a known TYPE
            "External Perimeter", // wrong casing of the second word
            "Some future role",
        ] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                None,
                "{s:?} should map to None (skipped)"
            );
        }
    }
}
