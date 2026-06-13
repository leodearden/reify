//! Per-argument type signatures for geometry topology-selector builtins
//! (task 4493, type-hygiene ζ).
//!
//! Hosts the checkable argument-slot table ([`builtin_arg_slots`]) and the
//! call-site checker ([`check_builtin_arg_types`]) for the geometry
//! topology-selector family.  The mechanism is generic (name-keyed), but only
//! geometry-selector dimensioned-scalar slots are populated here; math args
//! (polymorphic, no fixed dimension) and geometry-handle arg0 (ε=4358's
//! territory, PRD §4 out-of-scope) are intentionally absent.
//!
//! # Design: sibling of `math_signatures.rs`
//!
//! Placed beside `math_signatures.rs` per PRD open-question-3 (implementer's
//! choice): the arg-slot table covers the geometry family, not the math-linalg
//! §3 family, so folding it into the frozen `math_signatures` contract would be
//! a misnomer.  The module structure mirrors `math_signatures.rs`: a public-to-
//! crate name-keyed match function + a small set of supporting types.
//!
//! # Checked vs. unchecked slots (decision-4 gradualism)
//!
//! CHECKED (definite dimension mismatch, zero false positives):
//! - `center_of_mass` / `moment_of_inertia` arg1 `density` → MASS_DENSITY ("Density")
//! - `faces_by_normal` / `edges_parallel_to` arg2 `tol` → ANGLE ("Angle")
//! - `edges_at_height` arg1 `h` → LENGTH ("Length"), arg2 `tol` → LENGTH ("Length")
//!
//! UNCHECKED (would false-positive on valid call sites or is out-of-scope):
//! - arg0 (geometry handle) — ε=4358's territory
//! - `dir` Vec3 slot — accepts list literals `[0,0,1]` that coerce
//! - Range slots (`edges_by_length` / `faces_by_area`)
//! - Names without dimensioned-scalar args (`split`, `face`, `edge`, `solid_body`, …)

use reify_core::{
    Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, SourceSpan, Type,
};
use reify_ir::CompiledExpr;

/// A single checkable argument slot: the zero-based index, human-readable
/// parameter name, and expected type for a builtin argument.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CheckableArg {
    /// Zero-based index of this argument in the call argument list.
    pub index: usize,
    /// Human-readable parameter name used in diagnostic messages
    /// (e.g., `"density"`, `"tol"`, `"h"`).
    pub name: &'static str,
    /// Expected type for this slot.
    pub expected: ExpectedArg,
}

/// The expected type for a checkable builtin argument slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExpectedArg {
    /// A dimensioned scalar with a specific dimension.
    Scalar {
        /// The required physical dimension (e.g., `DimensionVector::MASS_DENSITY`).
        dimension: DimensionVector,
        /// Human-readable type name for diagnostic messages
        /// (e.g., `"Density"`, `"Angle"`, `"Length"`).
        type_name: &'static str,
    },
}

/// Return the checkable dimensioned-scalar argument slots for a named builtin.
///
/// Returns an empty `Vec` for:
/// - Unrecognized names.
/// - Names with no checked dimensioned-scalar arg (e.g. `split`, `face`, `edge`,
///   `solid_body`, `volume`, `edges`, `faces`, …).
///
/// The returned slots correspond exactly to the CHECKED arg positions listed
/// in the module-level docs.  Mirrors the name-keyed structure of
/// `math_fn_result_type` (task 4182 result-type precedent).
pub(crate) fn builtin_arg_slots(name: &str) -> Vec<CheckableArg> {
    match name {
        // ── Mass-properties topology selectors ───────────────────────────────
        // arg0: geometry handle (unchecked — ε=4358's territory)
        // arg1: density → MASS_DENSITY ("Density")
        "center_of_mass" | "moment_of_inertia" => vec![CheckableArg {
            index: 1,
            name: "density",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
            },
        }],

        // ── Directional topology selectors ───────────────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: dir Vec3 (unchecked — accepts list literals like [0,0,1])
        // arg2: tol → ANGLE ("Angle")
        "faces_by_normal" | "edges_parallel_to" => vec![CheckableArg {
            index: 2,
            name: "tol",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::ANGLE,
                type_name: "Angle",
            },
        }],

        // ── Height-based topology selectors ──────────────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: h → LENGTH ("Length")
        // arg2: tol → LENGTH ("Length")
        "edges_at_height" => vec![
            CheckableArg {
                index: 1,
                name: "h",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                },
            },
            CheckableArg {
                index: 2,
                name: "tol",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                },
            },
        ],

        // All other names: empty (no dimensioned-scalar arg to check).
        _ => vec![],
    }
}

/// Check the compiled arguments of a builtin call against its known type
/// signatures, pushing [`DiagnosticCode::ArgTypeMismatch`] errors for
/// DEFINITE static mismatches only.
///
/// No-op stub until step-4 implements the logic.
pub(crate) fn check_builtin_arg_types(
    _name: &str,
    _compiled_args: &[CompiledExpr],
    _call_span: SourceSpan,
    _diagnostics: &mut Vec<Diagnostic>,
) {
    // Stub: no-op. Implemented in step-4.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::GEOMETRY_TOPOLOGY_SELECTOR_NAMES;
    use reify_core::DimensionVector;

    // ── builtin_arg_slots table contract (step-1) ────────────────────────────

    fn mass_density_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
            },
        }
    }

    fn angle_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::ANGLE,
                type_name: "Angle",
            },
        }
    }

    fn length_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::LENGTH,
                type_name: "Length",
            },
        }
    }

    /// moment_of_inertia → arg1 density (MASS_DENSITY).
    #[test]
    fn moment_of_inertia_has_density_slot() {
        let slots = builtin_arg_slots("moment_of_inertia");
        assert_eq!(slots.len(), 1, "moment_of_inertia should have 1 slot, got: {:?}", slots);
        assert_eq!(slots[0], mass_density_slot(1, "density"));
    }

    /// center_of_mass → arg1 density (MASS_DENSITY).
    #[test]
    fn center_of_mass_has_density_slot() {
        let slots = builtin_arg_slots("center_of_mass");
        assert_eq!(slots.len(), 1, "center_of_mass should have 1 slot, got: {:?}", slots);
        assert_eq!(slots[0], mass_density_slot(1, "density"));
    }

    /// faces_by_normal → arg2 tol (ANGLE).
    #[test]
    fn faces_by_normal_has_angle_slot() {
        let slots = builtin_arg_slots("faces_by_normal");
        assert_eq!(slots.len(), 1, "faces_by_normal should have 1 slot, got: {:?}", slots);
        assert_eq!(slots[0], angle_slot(2, "tol"));
    }

    /// edges_parallel_to → arg2 tol (ANGLE).
    #[test]
    fn edges_parallel_to_has_angle_slot() {
        let slots = builtin_arg_slots("edges_parallel_to");
        assert_eq!(slots.len(), 1, "edges_parallel_to should have 1 slot, got: {:?}", slots);
        assert_eq!(slots[0], angle_slot(2, "tol"));
    }

    /// edges_at_height → arg1 h (LENGTH) AND arg2 tol (LENGTH).
    #[test]
    fn edges_at_height_has_h_and_tol_slots() {
        let slots = builtin_arg_slots("edges_at_height");
        assert_eq!(slots.len(), 2, "edges_at_height should have 2 slots, got: {:?}", slots);
        assert_eq!(slots[0], length_slot(1, "h"));
        assert_eq!(slots[1], length_slot(2, "tol"));
    }

    /// Names with no dimensioned-scalar arg or unrecognized names return empty.
    #[test]
    fn empty_for_unchecked_names() {
        let unchecked = [
            "edges",
            "faces",
            "adjacent_faces",
            "shared_edges",
            "split",
            "face",
            "edge",
            "solid_body",
            "volume",
            "box",
            "",
            "closest_point",
            "is_on",
            "angle_between_surfaces",
            "edges_by_length",
            "faces_by_area",
        ];
        for name in unchecked {
            let slots = builtin_arg_slots(name);
            assert!(
                slots.is_empty(),
                "builtin_arg_slots({:?}) should be empty, got {:?}",
                name,
                slots
            );
        }
    }

    /// Coverage invariant: every key in the table's domain is a member of
    /// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` — catching typos and keeping the
    /// arg-slot table consistent with the recognized family even as new
    /// selector names land.
    #[test]
    fn arg_slot_keys_are_subset_of_topology_selector_names() {
        let checked_names = [
            "center_of_mass",
            "moment_of_inertia",
            "faces_by_normal",
            "edges_parallel_to",
            "edges_at_height",
        ];
        for name in &checked_names {
            assert!(
                GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "arg-slot key {:?} is not in GEOMETRY_TOPOLOGY_SELECTOR_NAMES; \
                 either fix the name or add it to the selector slice",
                name
            );
        }
    }
}
