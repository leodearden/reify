//! Pure scalar helpers for free-vibration modal analysis (PRD §4 / §7.5).
//!
//! Dependency-free `f64` math (no `reify-solver-elastic` / `reify-ir::Value`
//! deps) so this module stays inside `reify-stdlib`. The `reify-eval` modal
//! trampoline (`modal_ops.rs`) calls these to convert eigen-solver output into
//! the `ModalResult` fields. Implementations land in step-2.
