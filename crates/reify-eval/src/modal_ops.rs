//! Compute trampoline for `modal::free_vibration` — the `fn modal_analysis`
//! `@optimized` target (task ζ, docs/prds/v0_3/modal-analysis.md §10).
//!
//! Hosts the modal free-vibration FEA solve (assemble K + M, free-DOF
//! eigensolve via `reify-solver-elastic`) and the `Value`-shaping trampoline.
//! Lives in `reify-eval` — not `reify-stdlib` — because the solve needs
//! `reify-solver-elastic` (which `reify-stdlib` does not depend on); `reify-eval`
//! depends on both. Mirrors `compute_targets/buckling.rs`. Implementations land
//! in steps 4+.
