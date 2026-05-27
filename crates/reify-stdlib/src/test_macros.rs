use reify_core::DimensionVector;
use reify_ir::Value;

/// Assert that an expression evaluates to `Value::Real(v)` where `|v - expected| < 1e-12`.
macro_rules! assert_real_approx {
    ($expr:expr, $expected:expr) => {
        match $expr {
            Value::Real(v) => assert!(
                (v - $expected).abs() < 1e-12,
                "expected Real({}) got Real({})",
                $expected,
                v
            ),
            other => panic!("expected Real({}), got {:?}", $expected, other),
        }
    };
}

/// Assert that an expression evaluates to `Value::Scalar { si_value, dimension }` where
/// `|si_value - expected_si| < 1e-12` and `dimension == expected_dim`.
macro_rules! assert_scalar_approx {
    ($expr:expr, $expected_si:expr, $expected_dim:expr) => {
        match $expr {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - $expected_si).abs() < 1e-12,
                    "expected si_value={}, got {}",
                    $expected_si,
                    si_value
                );
                assert_eq!(dimension, $expected_dim);
            }
            other => panic!(
                "expected Scalar{{si={}, dim={:?}}}, got {:?}",
                $expected_si, $expected_dim, other
            ),
        }
    };
}

/// Assert that an expression evaluates to a 3-component wrapper variant
/// (`Value::Tensor`, `Value::Vector`, or `Value::Point`) where each component
/// is approximately equal to the expected `[x, y, z]` values within 1e-12.
macro_rules! assert_vector3_approx {
    ($variant:ident, $expr:expr, [$ex:expr, $ey:expr, $ez:expr]) => {
        match $expr {
            Value::$variant(items) => {
                assert_eq!(
                    items.len(),
                    3,
                    "expected 3-component {}",
                    stringify!($variant)
                );
                let vals: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                assert!(
                    (vals[0] - $ex).abs() < 1e-12,
                    "x: expected {}, got {}",
                    $ex,
                    vals[0]
                );
                assert!(
                    (vals[1] - $ey).abs() < 1e-12,
                    "y: expected {}, got {}",
                    $ey,
                    vals[1]
                );
                assert!(
                    (vals[2] - $ez).abs() < 1e-12,
                    "z: expected {}, got {}",
                    $ez,
                    vals[2]
                );
            }
            other => panic!(
                "expected {}([{}, {}, {}]), got {:?}",
                stringify!($variant),
                $ex,
                $ey,
                $ez,
                other
            ),
        }
    };
}

/// Assert that an expression evaluates to `Value::Orientation { w, x, y, z }`.
///
/// Three calling forms:
/// - `assert_orientation_approx!(expr, w, x, y, z)` — sign-sensitive, tolerance 1e-12,
///   emits per-component labeled diagnostics.
/// - `assert_orientation_approx!(expr, w, x, y, z, tol = T)` — sign-sensitive with
///   explicit tolerance, same per-component diagnostics.
/// - `assert_orientation_approx!(expr, w, x, y, z, sign_insensitive = T)` — accepts
///   ±quaternion within explicit tolerance, single combined diagnostic.
macro_rules! assert_orientation_approx {
    // Default tolerance (1e-12), sign-sensitive, per-component diagnostics.
    ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr) => {
        assert_orientation_approx!($expr, $ew, $ex, $ey, $ez, tol = 1e-12)
    };
    // Explicit tolerance, sign-sensitive, per-component diagnostics.
    ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr, tol = $tol:expr) => {
        match $expr {
            Value::Orientation { w, x, y, z } => {
                assert!((w - $ew).abs() < $tol, "w: expected {}, got {}", $ew, w);
                assert!((x - $ex).abs() < $tol, "x: expected {}, got {}", $ex, x);
                assert!((y - $ey).abs() < $tol, "y: expected {}, got {}", $ey, y);
                assert!((z - $ez).abs() < $tol, "z: expected {}, got {}", $ez, z);
            }
            other => panic!(
                "expected Orientation({}, {}, {}, {}), got {:?}",
                $ew, $ex, $ey, $ez, other
            ),
        }
    };
    // Sign-insensitive: accepts ±quaternion within explicit tolerance.
    ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr, sign_insensitive = $tol:expr) => {
        match $expr {
            Value::Orientation { w, x, y, z } => {
                let pos_ok = (w - $ew).abs() < $tol
                    && (x - $ex).abs() < $tol
                    && (y - $ey).abs() < $tol
                    && (z - $ez).abs() < $tol;
                let neg_ok = (w + $ew).abs() < $tol
                    && (x + $ex).abs() < $tol
                    && (y + $ey).abs() < $tol
                    && (z + $ez).abs() < $tol;
                assert!(
                    pos_ok || neg_ok,
                    "expected Orientation(\u{b1}{}, \u{b1}{}, \u{b1}{}, \u{b1}{}) within {}, got ({}, {}, {}, {})",
                    $ew, $ex, $ey, $ez, $tol, w, x, y, z
                );
            }
            other => panic!(
                "expected Orientation(\u{b1}{}, \u{b1}{}, \u{b1}{}, \u{b1}{}), got {:?}",
                $ew, $ex, $ey, $ez, other
            ),
        }
    };
}

/// Build a `Value::Vector` of 3 `Value::Scalar` components from `[f64; 3]` and a dimension.
pub(crate) fn make_scalar_vec3(vals: [f64; 3], dim: DimensionVector) -> Value {
    Value::Vector(
        vals.iter()
            .map(|&v| Value::Scalar {
                si_value: v,
                dimension: dim,
            })
            .collect(),
    )
}
