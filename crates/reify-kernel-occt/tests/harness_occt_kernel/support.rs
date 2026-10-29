//! Helpers shared by two or more `harness_occt_kernel` members. A leaf: it imports no
//! member.

#![cfg(has_occt)]

use reify_ir::{GeometryError, GeometryHandle, GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

/// Create the canonical 20×10×5 axis-aligned box used by the InertiaTensor
/// density/analytic tests and store it in `kernel`. Returns the handle ID.
///
/// Twin: `src/lib.rs`'s in-file `mod tests` holds a byte-identical copy
/// for the private-reach tests that stayed. Edit both until #8659 moves
/// those tests and deletes that copy.
pub(crate) fn make_box_20_10_5(kernel: &mut OcctKernel) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(20.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(5.0),
        })
        .expect("Box creation must succeed")
        .id
}

/// Assert that a `Result` is an `Err(GeometryError::OperationFailed(msg))` where
/// `msg` contains the given `expected_substring`. Panics on `Ok` or wrong error variant.
pub(crate) fn assert_operation_fails_with(
    result: Result<GeometryHandle, GeometryError>,
    expected_substring: &str,
) {
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.to_lowercase()
                    .contains(&expected_substring.to_lowercase()),
                "expected error containing '{expected_substring}', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed containing '{expected_substring}', got Ok"),
        Err(other) => panic!(
            "expected OperationFailed containing '{expected_substring}', got {:?}",
            other
        ),
    }
}

/// Parse a `{"x":_,"y":_,"z":_}` JSON string into `(x, y, z)`.
/// Used by `CenterOfMass` query tests to decode the `Value::String` encoding returned
/// by `query_centroid`.
///
/// Twin: `src/lib.rs`'s in-file `mod tests` holds a byte-identical copy
/// for the private-reach tests that stayed. Edit both until #8659 moves
/// those tests and deletes that copy.
pub(crate) fn parse_centroid_json(s: &str) -> (f64, f64, f64) {
    let parse_field = |field: &str| -> f64 {
        let needle = format!("\"{field}\":");
        let start = s
            .find(needle.as_str())
            .unwrap_or_else(|| panic!("field {field} not found in centroid JSON: {s:?}"))
            + needle.len();
        let rest = &s[start..];
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        rest[..end]
            .trim()
            .parse::<f64>()
            .unwrap_or_else(|e| panic!("failed to parse {field} in centroid JSON: {s:?}: {e}"))
    };
    (parse_field("x"), parse_field("y"), parse_field("z"))
}

/// Positive precondition backing `occt_non_length_fields_stay_ungated`:
/// asserts that each of `op`'s deliberately-ungated numeric fields
/// (`HalfSpace.nx`/`ny`/`nz`, `CircularPattern.angle`, `Draft.angle`) is
/// a bare, undimensioned `Value::Real`/`Value::Int` — never a
/// `Value::Scalar` of any dimension.
///
/// `occt_non_length_fields_stay_ungated` proves a field stayed on the
/// context-free `extract_f64` by observing that no LENGTH-tripwire WARN
/// names it, which is informative only because a bare `Value::Real`/
/// `Int` is the one shape `reify_ir::check_length_field` can never wave
/// through: its early return is gated on `Value::Scalar { dimension,
/// .. } if dimension == LENGTH`, not on the `Scalar` variant alone, so
/// an ANGLE-dimensioned fixture would still warn on a rewire to
/// `extract_length_f64` *today* — ANGLE is not LENGTH. Keeping the
/// fixture bare is what preserves that reach even if the predicate is
/// later loosened to wave through any dimensioned `Scalar`, not just
/// `LENGTH`. This guard reads the value out of the SAME `op` the
/// control executes, so there is no second copy to drift out of sync.
/// See `docs/notes/angle-literal-migration-ledger.md` §1.2.1; task 5780
/// (δ) owns migrating `Draft.angle`, task 5781 (ε) owns migrating
/// `CircularPattern.angle`.
///
/// Panics naming `{op.kind_name()}.{field}` if a field is dimensioned,
/// or naming the op kind if it carries no deliberately-ungated field at
/// all — an unrecognised variant must fail loudly rather than silently
/// check nothing.
pub(crate) fn assert_ungated_fields_are_bare(op: &GeometryOp) {
    let fields: Vec<(&str, &Value)> = match op {
        GeometryOp::HalfSpace { nx, ny, nz, .. } => vec![("nx", nx), ("ny", ny), ("nz", nz)],
        GeometryOp::CircularPattern { angle, .. } => vec![("angle", angle)],
        GeometryOp::Draft { angle, .. } => vec![("angle", angle)],
        other => panic!(
            "{} carries no deliberately-ungated field — teach \
             assert_ungated_fields_are_bare this variant or stop \
             calling it on this op kind",
            other.kind_name()
        ),
    };
    for (field, value) in fields {
        assert!(
            matches!(value, Value::Real(_) | Value::Int(_)),
            "{}.{field} is {value:?}, not a bare Value::Real/Int — a \
             bare value is the one shape check_length_field can never \
             wave through, so only it stays reachable if \
             check_length_field's Scalar+LENGTH predicate is ever \
             loosened to accept other dimensioned Scalars; retyping \
             this fixture gives that reach up for nothing",
            op.kind_name(),
        );
    }
}
