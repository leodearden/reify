//! Numeric value-cell reader for the `reify-eval` integration-test binaries.
//!
//! Consumers include it from their test-crate root with
//! `#[path = "common/numeric_cell.rs"] mod numeric_cell;`, following the
//! `common/angle_expr.rs` precedent.

use reify_ir::Value;

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar` as its SI magnitude), panicking on anything else so a shape
/// regression fails loudly.
pub fn as_f64(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}
