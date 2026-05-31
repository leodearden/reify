//! Modal-analysis stdlib helpers (`std.modal`).
//!
//! Pure scalar helpers for the v0.3 free-vibration modal solver. The FEA work
//! (assemble K + M, eigensolve) lives in the `reify-eval` trampoline
//! (`modal_ops.rs`) because `reify-stdlib` does not depend on
//! `reify-solver-elastic`; this module holds only the dependency-free scalar
//! math the trampoline calls. See task ζ / docs/prds/v0_3/modal-analysis.md §10.

pub mod free_vibration;
pub mod trampoline;
pub mod transient;
