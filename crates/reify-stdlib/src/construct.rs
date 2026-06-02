//! Pure list→value **construction** primitives (math-linalg α, task 4179).
//!
//! Exposes [`eval_construct`], the `eval_builtin` dispatch arm for the four
//! N-general constructors that build vectors / matrices from `.ri` source:
//!
//! - `vec(list)`        → [`Value::Vector`] (N = list length)
//! - `matrix(rows)`     → rank-2 nested [`Value::Tensor`] (RANK-2 ONLY)
//! - `diag(list)`       → N×N nested [`Value::Tensor`] (list on the diagonal)
//! - `identity(n: Int)` → N×N dimensionless nested [`Value::Tensor`]
//!
//! These exist so N>3 matrices/vectors are buildable from source — today all
//! four forms parse but evaluate to `undef`. They are pure structural
//! reshaping: NO linear algebra (that is task β), NO grammar work.
//!
//! Cells are built with [`Value::from_real_scalar`] (Real if dimensionless,
//! else Scalar) and sanitized via [`crate::helpers::sanitize_value`]. Any
//! shape / dimension / numeric violation collapses to [`Value::Undef`] with no
//! new diagnostic code, mirroring `matrix_components_f64`'s shape-guards.
//!
//! Built inline (a construct.rs-local `Value::List` extractor, local Tensor
//! assembly) rather than editing `matrix.rs` / `helpers.rs`, which are owned by
//! sibling tasks β/γ — avoids narrow-file-lock contention and merge
//! serialization (PRD §7).

use reify_ir::Value;

/// Evaluate a construction builtin (`vec` / `matrix` / `diag` / `identity`).
///
/// Returns `Some(value)` when `name` is one of the four constructors (the
/// value is `Value::Undef` on malformed input), or `None` when `name` is not a
/// construction builtin (so `eval_builtin` continues its dispatch chain).
//
// STUB (pre-1): always `None` until the per-builtin arms land in steps 2/4/6/8.
// `#[allow(dead_code)]` because this is not yet wired into `eval_builtin`'s
// dispatch chain — that wiring arrives in step-2. The allow is removed then.
#[allow(dead_code)]
pub(crate) fn eval_construct(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {
    // Inline RED/GREEN eval tests for the four constructors land in steps
    // 1/3/5/7 (tests) and go GREEN in steps 2/4/6/8 (impl). Placeholder module
    // so the crate compiles with the construct.rs scaffold in place.
}
