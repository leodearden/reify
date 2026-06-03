//! ISO 286-1 tolerancing builtins: `iso_it_tolerance`, `effective_tolerance_zone`.
//!
//! Task α — the producer. Implements two builtins plus a diagnose classifier;
//! no `.ri` / reify-core / reify-expr changes (those are siblings β/ε or out of
//! α's two-file scope).

use reify_core::{Diagnostic, DimensionVector};
use reify_ir::Value;

use crate::helpers::{sanitize_value, validate_dimensioned_scalar};

/// Evaluate an ISO tolerancing builtin by name.
///
/// Returns `Some(value)` if the name is a recognised tolerancing function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_tolerancing(name: &str, args: &[Value]) -> Option<Value> {
    let _ = (name, args);
    None
}

/// Pure classifier: given the name and args of a stdlib call that returned
/// `Value::Undef`, determine whether this was a recognised tolerancing builtin
/// error and, if so, which `Diagnostic` (with `Severity::Error`) to emit.
///
/// Returns `None` for:
/// - unrecognised function names (non-tolerancing builtins, user functions, etc.)
/// - valid in-envelope calls to `iso_it_tolerance`
/// - any call to `effective_tolerance_zone`
///
/// Returns `Some(Diagnostic)` for out-of-envelope but well-typed calls to
/// `iso_it_tolerance` (grade outside IT5–IT18 or nominal size outside
/// `(0, 500mm]` or inverted/zero range).
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    let _ = (name, args);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
}
