//! Trampoline for `solver::membrane_load` — combined membrane + bar/cable load
//! analysis with a tension-only active set (PRD
//! `docs/prds/v0_6/tensegrity-membrane.md` §5 / §10 / §11, task η, layer M2).
//!
//! # Contract
//!
//! Receives the ten `value_inputs` matching the future `membrane_load` signature
//! (the surface analogue of T3b's six-input `tensegrity_load`, broadcasting a
//! single shared membrane section because `Tensegrity.surfaces` is a bare
//! `List<List<Int>>` with no per-triangle `Membrane` binding):
//!
//! ```text
//! [0] structure         : Tensegrity              (Value::StructureInstance)
//! [1] prestress         : List<Force>             — one per line member,
//!                                                   struts-then-cables
//! [2] youngs_modulus    : Pressure                (broadcast line-member E)
//! [3] area              : Area                    (broadcast line-member A)
//! [4] loads             : List<Vector3<Force>>    (per-node external force)
//! [5] supports          : List<Int>              (fixed node indices)
//! [6] surface_prestress : List<Pressure>          — one σ₀ per triangle,
//!                                                   surfaces order
//! [7] membrane_thickness: Length                  (broadcast patch thickness)
//! [8] membrane_youngs   : Pressure                (broadcast patch E)
//! [9] membrane_poisson  : Real                    (broadcast patch ν)
//! ```
//!
//! It cracks the Tensegrity into node coordinates + line connectivity (struts
//! then cables) + surface triangle triples, broadcasts the shared line section
//! `(E, A)` and the shared membrane section `(t, E, ν)` across patches, calls the
//! pure kernel [`reify_solver_elastic::membrane_load_analysis`], and rebuilds a
//! `MembraneLoadResult` `Value::StructureInstance`.
//!
//! # Failure → diagnostic
//!
//! Infeasible input returns [`ComputeOutcome::Failed`] carrying a single
//! `E_MembraneLoadInfeasible` `Diagnostic::error` (the mnemonic lives in the
//! message text, mirroring the `tensegrity_load` trampoline). The trampoline
//! never panics and never returns a silently-wrong (`converged: false`) result.

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::membrane_load`. See the module doc for the
/// input/output contract.
///
/// pre-2 scaffold: returns a placeholder [`ComputeOutcome::Failed`]. The real
/// input-cracking, kernel call, result building, located guards, and
/// registration in `register_compute_fns` land in steps 14 / 16.
pub fn solve_membrane_load_trampoline(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Failed {
        diagnostics: vec![Diagnostic::error(
            "E_MembraneLoadInfeasible: solver::membrane_load trampoline is not yet \
             implemented (scaffold placeholder, #4418 pre-2)"
                .to_string(),
        )],
    }
}
