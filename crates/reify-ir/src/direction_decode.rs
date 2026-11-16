//! The shared `&Value` readers for directions and frame bases
//! (docs/prds/v0_6/direction-typing.md §5 C3/C8).

use std::fmt;

use reify_core::direction::DIRECTION_MIGRATION_HINT;

use crate::direction::UnitVec3;
use crate::value::Value;

/// Why a value could not be read as a direction or a frame basis. `got` is the
/// offending value's `kind_name()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeDirectionError {
    NotADirection { got: &'static str },
    NotAFrame { got: &'static str },
    MalformedFrameBasis { got: &'static str },
}

impl fmt::Display for DecodeDirectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeDirectionError::NotADirection { got } => {
                write!(
                    f,
                    "expected a Direction, got {got}; {DIRECTION_MIGRATION_HINT}"
                )
            }
            DecodeDirectionError::NotAFrame { got } => write!(f, "expected a Frame, got {got}"),
            DecodeDirectionError::MalformedFrameBasis { got } => write!(
                f,
                "Frame basis must be a finite, non-zero Orientation quaternion, got {got}"
            ),
        }
    }
}

impl std::error::Error for DecodeDirectionError {}

/// Accepts only `Value::Direction`: a raw `Vector` is not a direction.
pub fn decode_direction(v: &Value) -> Result<UnitVec3, DecodeDirectionError> {
    match v {
        Value::Direction(u) => Ok(*u),
        other => Err(DecodeDirectionError::NotADirection {
            got: other.kind_name(),
        }),
    }
}

/// The frame's x, y and z axes in world coordinates: row `i` is column `i` of
/// the rotation matrix of the normalised basis quaternion (active `q·v·q*`).
pub fn frame_basis(frame: &Value) -> Result<[[f64; 3]; 3], DecodeDirectionError> {
    let Value::Frame { basis, .. } = frame else {
        return Err(DecodeDirectionError::NotAFrame {
            got: frame.kind_name(),
        });
    };
    let malformed = DecodeDirectionError::MalformedFrameBasis {
        got: basis.kind_name(),
    };
    let Value::Orientation { w, x, y, z } = **basis else {
        return Err(malformed);
    };
    let n = (w * w + x * x + y * y + z * z).sqrt();
    if !n.is_finite() || n < 1e-12 {
        return Err(malformed);
    }
    let (w, x, y, z) = (w / n, x / n, y / n, z / n);
    Ok([
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
        ],
        [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
        ],
        [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rng::SplitMix64;
    use reify_core::direction::DIRECTION_MIGRATION_HINT;
    use std::f64::consts::FRAC_1_SQRT_2;

    fn origin() -> Value {
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])
    }

    fn frame(w: f64, x: f64, y: f64, z: f64) -> Value {
        Value::Frame {
            origin: Box::new(origin()),
            basis: Box::new(Value::Orientation { w, x, y, z }),
        }
    }

    fn unit(x: f64, y: f64, z: f64) -> UnitVec3 {
        UnitVec3::from_vector([x, y, z]).unwrap()
    }

    fn row_bits(rows: [[f64; 3]; 3]) -> [[u64; 3]; 3] {
        rows.map(|row| row.map(f64::to_bits))
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn assert_rows_close(got: [[f64; 3]; 3], want: [[f64; 3]; 3]) {
        for (r, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            for c in 0..3 {
                assert!(
                    (g[c] - w[c]).abs() <= 1e-12,
                    "row {r} col {c}: got {got:?}, want {want:?}"
                );
            }
        }
    }

    #[test]
    fn decode_direction_returns_the_held_unit_vector() {
        let u = unit(0.6, 0.8, 0.0);
        let decoded = decode_direction(&Value::Direction(u)).expect("a Direction decodes");
        assert_eq!(
            decoded.components().map(f64::to_bits),
            u.components().map(f64::to_bits)
        );
    }

    #[test]
    fn decode_direction_rejects_every_non_direction() {
        let vector = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let axis = Value::Axis {
            origin: Box::new(origin()),
            direction: Box::new(Value::Direction(unit(1.0, 0.0, 0.0))),
        };
        for (v, kind) in [
            (vector, "Vector"),
            (Value::Real(1.0), "Real"),
            (Value::Undef, "Undef"),
            (axis, "Axis"),
        ] {
            assert_eq!(v.kind_name(), kind);
            assert_eq!(
                decode_direction(&v),
                Err(DecodeDirectionError::NotADirection { got: kind })
            );
        }
    }

    #[test]
    fn not_a_direction_message_names_the_kind_and_ends_with_the_hint() {
        let vector = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let message = decode_direction(&vector).unwrap_err().to_string();
        assert!(message.contains("Vector"), "{message}");
        assert!(message.ends_with(DIRECTION_MIGRATION_HINT), "{message}");
    }

    #[test]
    fn frame_basis_of_identity_is_exactly_identity() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for f in [frame(1.0, 0.0, 0.0, 0.0), frame(2.0, 0.0, 0.0, 0.0)] {
            let rows = frame_basis(&f).expect("an identity frame has a basis");
            assert_eq!(row_bits(rows), row_bits(identity), "{f:?} gave {rows:?}");
        }
    }

    #[test]
    fn frame_basis_rows_are_the_rotated_world_axes() {
        let about_111 = frame_basis(&frame(0.5, 0.5, 0.5, 0.5)).unwrap();
        assert_rows_close(
            about_111,
            [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        );

        let about_z = frame_basis(&frame(FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2)).unwrap();
        assert_rows_close(
            about_z,
            [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        );
    }

    #[test]
    fn frame_basis_rows_are_orthonormal() {
        let mut rng = SplitMix64(0x5EED_F2A3_E0BA_5150);
        let mut checked = 0usize;
        while checked < 1000 {
            let q = [
                rng.symmetric(),
                rng.symmetric(),
                rng.symmetric(),
                rng.symmetric(),
            ];
            if q.iter().map(|c| c * c).sum::<f64>().sqrt() < 1e-6 {
                continue;
            }
            checked += 1;
            let rows = frame_basis(&frame(q[0], q[1], q[2], q[3]))
                .unwrap_or_else(|e| panic!("quaternion {q:?}: {e}"));
            for i in 0..3 {
                let norm = dot(rows[i], rows[i]).sqrt();
                assert!((norm - 1.0).abs() <= 1e-12, "{q:?} row {i}: {rows:?}");
                for j in (i + 1)..3 {
                    let d = dot(rows[i], rows[j]);
                    assert!(d.abs() <= 1e-12, "{q:?} rows {i}·{j} = {d:e}");
                }
            }
        }
    }

    #[test]
    fn frame_basis_rejects_malformed_input() {
        let vector = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        assert_eq!(
            frame_basis(&vector),
            Err(DecodeDirectionError::NotAFrame { got: "Vector" })
        );

        for f in [frame(0.0, 0.0, 0.0, 0.0), frame(f64::NAN, 0.0, 0.0, 1.0)] {
            assert_eq!(
                frame_basis(&f),
                Err(DecodeDirectionError::MalformedFrameBasis { got: "Orientation" }),
                "{f:?}"
            );
        }

        let real_basis = Value::Frame {
            origin: Box::new(origin()),
            basis: Box::new(Value::Real(1.0)),
        };
        assert_eq!(
            frame_basis(&real_basis),
            Err(DecodeDirectionError::MalformedFrameBasis { got: "Real" })
        );
    }
}
