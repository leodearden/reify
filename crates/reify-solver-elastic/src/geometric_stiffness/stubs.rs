//! Element-stiffness stubs for non-P1-tet cells.
//!
//! These entry points exist so the geometric-stiffness module presents the
//! same per-element surface for every supported element class — callers
//! that want to dispatch on cell type don't have to special-case "is the
//! K_g path implemented?". Each stub panics with a citation pointing at
//! the task that will land the real kernel; the user-facing diagnostic
//! payloads (`E_BucklingShellNotImplemented`, `E_BucklingHexWedgeNotImplemented`)
//! are surfaced by the buckling trampoline (task ζ — PRD
//! `docs/prds/v0_5/buckling-eigensolver.md` §13).
//!
//! These functions are **not** the diagnostic path callers should reach
//! at runtime: the buckling trampoline filters non-tet cells *before*
//! calling into the element kernel and emits the named diagnostic. The
//! stubs here exist for code that bypasses the trampoline (test
//! scaffolding, future direct callers) — reaching them means an
//! upstream filter is missing.

use crate::assembly::ElementStiffness;

use super::InitialStress3;

/// Shell-element geometric stiffness — not implemented.
///
/// Cell class: 3-node triangular MITC3+ shell.
/// Tracking task: PRD `docs/prds/v0_5/buckling-eigensolver.md` §13 task ζ
/// (`E_BucklingShellNotImplemented`, citing task 3392 shell-buckling
/// theory). Buckling for shells is deferred behind the diagnostic path.
///
/// # Panics
///
/// Always panics — the buckling trampoline (task ζ) should have filtered
/// shell cells out and emitted the named diagnostic before reaching this
/// kernel. Reaching this function indicates a missing upstream filter.
pub fn geometric_element_stiffness_shell(
    _phys_nodes: &[[f64; 3]; 3],
    _thickness: f64,
    _sigma: &InitialStress3,
) -> ElementStiffness {
    panic!(
        "shell geometric stiffness is not implemented (PRD \
         docs/prds/v0_5/buckling-eigensolver.md §13 task ζ: \
         E_BucklingShellNotImplemented — cite task 3392). The buckling \
         trampoline should have filtered shell cells before reaching the \
         element kernel."
    );
}

/// Hex-P1 geometric stiffness — not implemented.
///
/// Cell class: 8-node hexahedron.
/// Tracking task: PRD `docs/prds/v0_5/buckling-eigensolver.md` §13 task ζ
/// (`E_BucklingHexWedgeNotImplemented`, citing `docs/prds/v0_3/hex-wedge-meshing.md`).
/// Buckling for hex meshes is deferred behind the diagnostic path.
///
/// # Panics
///
/// Always panics — see [`geometric_element_stiffness_shell`] for the
/// diagnostic-routing rationale.
pub fn geometric_element_stiffness_hex_p1(
    _phys_nodes: &[[f64; 3]; 8],
    _sigma: &InitialStress3,
) -> ElementStiffness {
    panic!(
        "hex-P1 geometric stiffness is not implemented (PRD \
         docs/prds/v0_5/buckling-eigensolver.md §13 task ζ: \
         E_BucklingHexWedgeNotImplemented — cite \
         docs/prds/v0_3/hex-wedge-meshing.md). The buckling trampoline \
         should have filtered hex cells before reaching the element kernel."
    );
}

/// Wedge-P1 geometric stiffness — not implemented.
///
/// Cell class: 6-node triangular prism.
/// Tracking task: PRD `docs/prds/v0_5/buckling-eigensolver.md` §13 task ζ
/// (`E_BucklingHexWedgeNotImplemented`, citing `docs/prds/v0_3/hex-wedge-meshing.md`).
///
/// # Panics
///
/// Always panics — see [`geometric_element_stiffness_shell`].
pub fn geometric_element_stiffness_wedge_p1(
    _phys_nodes: &[[f64; 3]; 6],
    _sigma: &InitialStress3,
) -> ElementStiffness {
    panic!(
        "wedge-P1 geometric stiffness is not implemented (PRD \
         docs/prds/v0_5/buckling-eigensolver.md §13 task ζ: \
         E_BucklingHexWedgeNotImplemented — cite \
         docs/prds/v0_3/hex-wedge-meshing.md). The buckling trampoline \
         should have filtered wedge cells before reaching the element \
         kernel."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "shell geometric stiffness is not implemented")]
    fn shell_panics_with_named_diagnostic_citation() {
        let nodes = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let _ = geometric_element_stiffness_shell(&nodes, 0.1, &InitialStress3::zero());
    }

    #[test]
    #[should_panic(expected = "hex-P1 geometric stiffness is not implemented")]
    fn hex_p1_panics_with_named_diagnostic_citation() {
        let nodes = [[0.0; 3]; 8];
        let _ = geometric_element_stiffness_hex_p1(&nodes, &InitialStress3::zero());
    }

    #[test]
    #[should_panic(expected = "wedge-P1 geometric stiffness is not implemented")]
    fn wedge_p1_panics_with_named_diagnostic_citation() {
        let nodes = [[0.0; 3]; 6];
        let _ = geometric_element_stiffness_wedge_p1(&nodes, &InitialStress3::zero());
    }
}
