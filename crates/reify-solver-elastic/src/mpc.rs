//! Multi-point constraint (MPC) types for the structural-analysis solver.
//!
//! **Stub for Task 3020 (Shells T10) to extend.** This file ships the
//! `MpcRow` placeholder type so the file the orchestrator's file-list
//! expects exists, but does not (yet) ship constraint-equation
//! construction or row-elimination wiring — those are Task 3020's
//! deliverable.
//!
//! See PRD `docs/prds/v0_4/structural-analysis-shells.md` task T10 / T11.

#[cfg(test)]
mod tests {
    use super::*;

    /// `MpcRow` round-trips its public fields without losing data.
    ///
    /// The struct is a **typed handoff** to Task 3020. Locking the
    /// public-field shape here means T10's later edit can populate
    /// construction methods (`MpcRow::shell_tet_tying`, ...) without
    /// having to negotiate the field shape.
    #[test]
    fn mpc_row_round_trips_dofs_coeffs_and_rhs_via_public_constructor() {
        let row = MpcRow {
            dofs: vec![3, 7, 11],
            coeffs: vec![1.0, -0.5, 0.5],
            rhs: 0.0,
        };
        assert_eq!(row.dofs, vec![3, 7, 11]);
        assert_eq!(row.coeffs, vec![1.0, -0.5, 0.5]);
        assert_eq!(row.rhs, 0.0);
        assert_eq!(
            row.dofs.len(),
            row.coeffs.len(),
            "MpcRow contract: dofs and coeffs must agree in length",
        );
    }
}
