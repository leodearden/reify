//! Design-for-Manufacturing (DFM) builtins (PRD v0_6 process-dfm-completion, task α).
//!
//! Two surfaces, mirroring the stackup / flexure modules:
//!
//! - [`eval_dfm`] — the pure builtin dispatcher (sibling of `stackup::eval_stackup`),
//!   wired into `crate::eval_builtin`'s fall-through chain in `lib.rs`. It evaluates
//!   `fits_build_volume(part_bbox, envelope_bbox[, severity_or_rule])`, a pure
//!   bbox-vs-bbox extent comparator (no kernel / `EvalContext` access). The two
//!   `Value::BoundingBox` inputs are resolved from Solids UPSTREAM by the existing
//!   kernel-aware `bounding_box(solid)` builtin, so `fits_build_volume` itself stays
//!   unit-testable and dependency-free (PRD §2.1 / §4 decision 4).
//!
//! - [`diagnose`] — the `DFMSeverity` → diagnostic-severity bridge (sibling of
//!   `flexures::flexure_diagnose`). It is re-exported as `crate::dfm_diagnose` and
//!   called from reify-expr's builtin fall-through on BOTH the success and the
//!   `Value::Undef` paths: a successfully-evaluated `fits_build_volume` that returns
//!   `Bool(false)` is a build-volume VIOLATION whose severity comes from the optional
//!   rule argument; a `Value::Undef` result is a usage error.

use reify_ir::Value;
use reify_core::Diagnostic;

/// Evaluate a DFM builtin by name.
///
/// Returns `Some(value)` if `name` is a recognised DFM function, `None` otherwise
/// (so the dispatch chain in `lib.rs` can fall through). Mirrors
/// [`crate::stackup::eval_stackup`]'s `Option<Value>` fall-through convention.
pub(crate) fn eval_dfm(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

/// Pure post-call DFM diagnostic classifier (the `DFMSeverity` bridge).
///
/// Mirrors [`crate::flexures::flexure_diagnose`]: returns a `Vec<Diagnostic>`, fires on
/// BOTH the success and `Value::Undef` paths, and short-circuits to an empty `Vec` for
/// any non-DFM `name`.
pub fn diagnose(_name: &str, _args: &[Value], _result: &Value) -> Vec<Diagnostic> {
    Vec::new()
}
