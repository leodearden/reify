use std::collections::{BTreeMap, BTreeSet, HashMap};

use reify_core::{ConstraintNodeId, Type, ValueCellId};
use reify_core::dimension::{DimensionVector, FORCE};
use reify_ir::{DeterminacyState, Value};

// --- Range value constructors ---

/// Create a Range with both bounds inclusive: [lower, upper].
pub fn range_inclusive(lower: Value, upper: Value) -> Value {
    Value::range(Some(lower), Some(upper), true, true)
}

/// Create a Range with both bounds exclusive: (lower, upper).
pub fn range_exclusive(lower: Value, upper: Value) -> Value {
    Value::range(Some(lower), Some(upper), false, false)
}

/// Create a Range with a lower bound only (inclusive): [lower, +∞).
pub fn range_at_least(lower: Value) -> Value {
    Value::range(Some(lower), None, true, false)
}

/// Create a Range with an upper bound only (inclusive): (-∞, upper].
pub fn range_at_most(upper: Value) -> Value {
    Value::range(None, Some(upper), false, true)
}

/// Create a half-open Range: [lower, upper).
pub fn range_half_open(lower: Value, upper: Value) -> Value {
    Value::range(Some(lower), Some(upper), true, false)
}

// --- Temperature value constructors ---

/// Create a Scalar with TEMPERATURE dimension in kelvin (SI base unit).
pub fn kelvin(v: f64) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: DimensionVector::TEMPERATURE,
    }
}

/// Create a Scalar with TEMPERATURE dimension from degrees Celsius (converts to kelvin).
pub fn celsius(v: f64) -> Value {
    kelvin(v + 273.15)
}

/// Create a Scalar with TEMPERATURE dimension from degrees Fahrenheit (converts to kelvin).
pub fn fahrenheit(v: f64) -> Value {
    kelvin((v - 32.0) * 5.0 / 9.0 + 273.15)
}

// --- Value constructors ---

/// Create a Scalar with LENGTH dimension from millimeters.
pub fn mm(v: f64) -> Value {
    Value::Scalar {
        si_value: v * 0.001,
        dimension: DimensionVector::LENGTH,
    }
}

/// Create a Scalar with ANGLE dimension from degrees.
pub fn deg(v: f64) -> Value {
    Value::Scalar {
        si_value: v * std::f64::consts::PI / 180.0,
        dimension: DimensionVector::ANGLE,
    }
}

/// Create a Scalar with FORCE dimension from newtons.
pub fn newton(v: f64) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: FORCE,
    }
}

/// Create a Scalar with MASS dimension from kilograms (SI base unit).
pub fn kg(v: f64) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: DimensionVector::MASS,
    }
}

/// Create a Scalar with LENGTH dimension from meters.
pub fn meters(v: f64) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: DimensionVector::LENGTH,
    }
}

/// Create a Scalar with AREA dimension from mm².
pub fn mm2(v: f64) -> Value {
    Value::Scalar {
        si_value: v * 1e-6,
        dimension: DimensionVector::AREA,
    }
}

/// Create a Scalar with VOLUME dimension from mm³.
pub fn mm3(v: f64) -> Value {
    Value::Scalar {
        si_value: v * 1e-9,
        dimension: DimensionVector::VOLUME,
    }
}

// --- ID constructors ---

/// Create a ValueCellId from entity and member names.
pub fn vcid(entity: &str, member: &str) -> ValueCellId {
    ValueCellId::new(entity, member)
}

/// Create a ConstraintNodeId from entity name and index.
pub fn cnid(entity: &str, index: u32) -> ConstraintNodeId {
    ConstraintNodeId::new(entity, index)
}

// --- Determinacy state helpers ---

/// Return a (Value, DeterminacyState::Determined) tuple.
pub fn determined(value: Value) -> (Value, DeterminacyState) {
    (value, DeterminacyState::Determined)
}

/// Return a (Value::Undef, DeterminacyState::Undetermined) tuple.
pub fn undetermined() -> (Value, DeterminacyState) {
    (Value::Undef, DeterminacyState::Undetermined)
}

/// Return a (Value, DeterminacyState::Provisional) tuple.
pub fn provisional(value: Value) -> (Value, DeterminacyState) {
    (value, DeterminacyState::Provisional)
}

/// Return a (Value::Undef, DeterminacyState::Auto) tuple.
pub fn auto_state() -> (Value, DeterminacyState) {
    (Value::Undef, DeterminacyState::Auto)
}

// --- SnapshotValuesBuilder ---

/// Fluent builder for `HashMap<ValueCellId, (Value, DeterminacyState)>`.
///
/// Constructs the snapshot-values map used by the evaluation engine's predicate
/// tests without manually constructing each (Value, DeterminacyState) tuple.
///
/// # Example
/// ```ignore
/// let snapshot = SnapshotValuesBuilder::new()
///     .determined(vcid("Bolt", "mass"), mm(1000.0))
///     .undetermined(vcid("Bolt", "volume"))
///     .build();
/// ```
pub struct SnapshotValuesBuilder {
    entries: HashMap<ValueCellId, (Value, DeterminacyState)>,
}

impl SnapshotValuesBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Insert an entry with `DeterminacyState::Determined`.
    pub fn determined(mut self, id: ValueCellId, value: Value) -> Self {
        self.entries.insert(id, determined(value));
        self
    }

    /// Insert an entry with `Value::Undef` and `DeterminacyState::Undetermined`.
    pub fn undetermined(mut self, id: ValueCellId) -> Self {
        self.entries.insert(id, undetermined());
        self
    }

    /// Insert an entry with `DeterminacyState::Provisional`.
    pub fn provisional(mut self, id: ValueCellId, value: Value) -> Self {
        self.entries.insert(id, provisional(value));
        self
    }

    /// Insert an entry with `Value::Undef` and `DeterminacyState::Auto`.
    pub fn auto_val(mut self, id: ValueCellId) -> Self {
        self.entries.insert(id, auto_state());
        self
    }

    /// Consume the builder and return the completed snapshot map.
    pub fn build(self) -> HashMap<ValueCellId, (Value, DeterminacyState)> {
        self.entries
    }
}

impl Default for SnapshotValuesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// --- Approximate value comparison macro ---

/// Compare two `Value` instances with floating-point tolerance.
///
/// Handles `Scalar` (compares `si_value`), `Real` (compares directly), `Int`/`Bool`/`String`
/// (exact match), `Point`/`Vector` (component-wise recursive). Panics with a descriptive
/// message on mismatch.
///
/// # Usage
/// ```ignore
/// assert_value_approx!(actual, expected);          // default tolerance 1e-9
/// assert_value_approx!(actual, expected, 1e-6);    // custom tolerance
/// ```
#[macro_export]
macro_rules! assert_value_approx {
    ($left:expr, $right:expr) => {
        $crate::assert_value_approx!($left, $right, 1e-9)
    };
    ($left:expr, $right:expr, $tol:expr) => {
        $crate::values::assert_value_approx_impl(&$left, &$right, $tol, file!(), line!())
    };
}

/// Implementation function for `assert_value_approx!`. Not intended for direct use.
#[doc(hidden)]
pub fn assert_value_approx_impl(left: &Value, right: &Value, tol: f64, file: &str, line: u32) {
    fn check(left: &Value, right: &Value, tol: f64, path: &str) -> Result<(), String> {
        /// Guard: fail immediately if either f64 is NaN (NaN comparisons are always false,
        /// which would silently pass tolerance checks).
        fn guard_nan(a: f64, b: f64, path: &str) -> Result<(), String> {
            if a.is_nan() || b.is_nan() {
                return Err(format!(
                    "{path}: NaN detected in comparison: left={a}, right={b}"
                ));
            }
            Ok(())
        }

        match (left, right) {
            (
                Value::Scalar {
                    si_value: a,
                    dimension: da,
                },
                Value::Scalar {
                    si_value: b,
                    dimension: db,
                },
            ) => {
                if da != db {
                    return Err(format!("{path}: dimension mismatch: {:?} vs {:?}", da, db));
                }
                guard_nan(*a, *b, path)?;
                if (a - b).abs() > tol {
                    return Err(format!(
                        "{path}: values differ: {a} vs {b} (diff={}, tol={tol})",
                        (a - b).abs()
                    ));
                }
                Ok(())
            }
            (Value::Real(a), Value::Real(b)) => {
                guard_nan(*a, *b, path)?;
                if (a - b).abs() > tol {
                    return Err(format!(
                        "{path}: values differ: {a} vs {b} (diff={}, tol={tol})",
                        (a - b).abs()
                    ));
                }
                Ok(())
            }
            (Value::Int(a), Value::Int(b)) => {
                if a != b {
                    return Err(format!("{path}: values differ: Int({a}) vs Int({b})"));
                }
                Ok(())
            }
            (Value::Bool(a), Value::Bool(b)) => {
                if a != b {
                    return Err(format!("{path}: values differ: Bool({a}) vs Bool({b})"));
                }
                Ok(())
            }
            (Value::String(a), Value::String(b)) => {
                if a != b {
                    return Err(format!(
                        "{path}: values differ: String({a:?}) vs String({b:?})"
                    ));
                }
                Ok(())
            }
            (Value::Point(a), Value::Point(b)) | (Value::Vector(a), Value::Vector(b)) => {
                if a.len() != b.len() {
                    return Err(format!(
                        "{path}: length mismatch: {} vs {}",
                        a.len(),
                        b.len()
                    ));
                }
                for (i, (ai, bi)) in a.iter().zip(b.iter()).enumerate() {
                    check(ai, bi, tol, &format!("{path}[{i}]"))?;
                }
                Ok(())
            }
            (
                Value::Complex {
                    re: ra,
                    im: ia,
                    dimension: da,
                },
                Value::Complex {
                    re: rb,
                    im: ib,
                    dimension: db,
                },
            ) => {
                if da != db {
                    return Err(format!("{path}: dimension mismatch: {:?} vs {:?}", da, db));
                }
                guard_nan(*ra, *rb, &format!("{path}.re"))?;
                guard_nan(*ia, *ib, &format!("{path}.im"))?;
                if (ra - rb).abs() > tol {
                    return Err(format!(
                        "{path}.re: values differ: {ra} vs {rb} (diff={}, tol={tol})",
                        (ra - rb).abs()
                    ));
                }
                if (ia - ib).abs() > tol {
                    return Err(format!(
                        "{path}.im: values differ: {ia} vs {ib} (diff={}, tol={tol})",
                        (ia - ib).abs()
                    ));
                }
                Ok(())
            }
            _ => {
                if left != right {
                    return Err(format!("{path}: values differ: {left:?} vs {right:?}"));
                }
                Ok(())
            }
        }
    }
    if let Err(msg) = check(left, right, tol, "root") {
        panic!("assert_value_approx failed at {file}:{line}: {msg}");
    }
}

// --- Point/Vector/Matrix constructors ---

/// Create a `Value::Point` with three length-dimensioned components (in meters).
///
/// Useful for constructing position values in geometry tests.
pub fn point3(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![Value::length(x), Value::length(y), Value::length(z)])
}

/// Create a `Value::Vector` with three length-dimensioned components (in meters).
///
/// Useful for constructing displacement/translation vectors in geometry tests.
pub fn vec3(x: f64, y: f64, z: f64) -> Value {
    Value::Vector(vec![Value::length(x), Value::length(y), Value::length(z)])
}

/// Create a `Value::Vector` with three dimensionless `Real` components.
///
/// Useful for unit normals, directions, and other dimensionless vectors in geometry tests.
pub fn vec3_dimensionless(x: f64, y: f64, z: f64) -> Value {
    Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
}

/// Create a 3×3 `Value::Matrix` with `Real` entries.
///
/// Arguments are given row-major: `matrix3x3(r0c0, r0c1, r0c2, r1c0, ...)`.
#[allow(clippy::too_many_arguments)]
pub fn matrix3x3(
    r0c0: f64,
    r0c1: f64,
    r0c2: f64,
    r1c0: f64,
    r1c1: f64,
    r1c2: f64,
    r2c0: f64,
    r2c1: f64,
    r2c2: f64,
) -> Value {
    Value::Matrix(vec![
        vec![Value::Real(r0c0), Value::Real(r0c1), Value::Real(r0c2)],
        vec![Value::Real(r1c0), Value::Real(r1c1), Value::Real(r1c2)],
        vec![Value::Real(r2c0), Value::Real(r2c1), Value::Real(r2c2)],
    ])
}

// --- Complex value constructors ---

/// Create a Value::Int.
pub fn int_val(n: i64) -> Value {
    Value::Int(n)
}

/// Create a Value::Real.
pub fn real_val(f: f64) -> Value {
    Value::Real(f)
}

/// Create a Value::Bool.
pub fn bool_val(b: bool) -> Value {
    Value::Bool(b)
}

/// Create a Value::String.
pub fn string_val(s: &str) -> Value {
    Value::String(s.to_string())
}

/// Create a Value::Enum with the given type name and variant.
pub fn enum_val(type_name: &str, variant: &str) -> Value {
    Value::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
    }
}

/// Create a Value::List from a Vec of values.
pub fn list_val(items: Vec<Value>) -> Value {
    Value::List(items)
}

/// Create a Value::Set from a Vec of values (duplicates are removed).
pub fn set_val(items: Vec<Value>) -> Value {
    Value::Set(items.into_iter().collect::<BTreeSet<_>>())
}

/// Create a Value::Map from a Vec of (key, value) pairs.
pub fn map_val(entries: Vec<(Value, Value)>) -> Value {
    Value::Map(entries.into_iter().collect::<BTreeMap<_, _>>())
}

/// Create a Value::Option(Some(v)).
pub fn some_val(v: Value) -> Value {
    Value::Option(Some(Box::new(v)))
}

/// Create a Value::Option(None).
pub fn none_val() -> Value {
    Value::Option(None)
}

/// Create a Value::Undef.
pub fn undef() -> Value {
    Value::Undef
}

// --- Complex value constructors ---

/// Create a `Value::Complex` with the given real and imaginary parts (dimensionless).
pub fn complex_val(re: f64, im: f64) -> Value {
    Value::Complex {
        re,
        im,
        dimension: DimensionVector::DIMENSIONLESS,
    }
}

/// Create a `Value::Complex` with the given real/imaginary parts and explicit dimension.
pub fn complex_val_dimensioned(re: f64, im: f64, dimension: DimensionVector) -> Value {
    Value::Complex { re, im, dimension }
}

/// Create a `Value::Point` with two length-dimensioned components (in meters).
pub fn point2(x: f64, y: f64) -> Value {
    Value::Point(vec![Value::length(x), Value::length(y)])
}

/// Create a `Value::Vector` with two length-dimensioned components (in meters).
pub fn vec2(x: f64, y: f64) -> Value {
    Value::Vector(vec![Value::length(x), Value::length(y)])
}

/// Create a `Value::Vector` with two dimensionless `Real` components.
///
/// Useful for unit normals, directions, and other dimensionless 2D vectors in geometry tests.
pub fn vec2_dimensionless(x: f64, y: f64) -> Value {
    Value::Vector(vec![Value::Real(x), Value::Real(y)])
}

/// Create a `Value::Point` with two dimensionless `Real` components.
///
/// Useful for dimensionless 2D coordinate values in geometry tests.
pub fn point2_dimensionless(x: f64, y: f64) -> Value {
    Value::Point(vec![Value::Real(x), Value::Real(y)])
}

/// Create a `Value::Orientation` (unit quaternion: w + xi + yj + zk).
pub fn orientation_val(w: f64, x: f64, y: f64, z: f64) -> Value {
    Value::Orientation { w, x, y, z }
}

/// Create a `Value::Frame` with the given origin (Point) and basis (Orientation).
pub fn frame_val(origin: Value, basis: Value) -> Value {
    Value::Frame {
        origin: Box::new(origin),
        basis: Box::new(basis),
    }
}

/// Create a `Value::Transform` with the given rotation (Orientation) and translation (Vector).
pub fn transform_val(rotation: Value, translation: Value) -> Value {
    Value::Transform {
        rotation: Box::new(rotation),
        translation: Box::new(translation),
    }
}

/// Create a `Value::Plane` with the given origin (Point) and normal (Vector).
pub fn plane_val(origin: Value, normal: Value) -> Value {
    Value::Plane {
        origin: Box::new(origin),
        normal: Box::new(normal),
    }
}

/// Create a `Value::Axis` with the given origin (Point) and direction (Vector).
pub fn axis_val(origin: Value, direction: Value) -> Value {
    Value::Axis {
        origin: Box::new(origin),
        direction: Box::new(direction),
    }
}

/// Create a `Value::BoundingBox` with the given min and max corner points.
pub fn bbox_val(min: Value, max: Value) -> Value {
    Value::BoundingBox {
        min: Box::new(min),
        max: Box::new(max),
    }
}

/// Create a `Value::Tensor` wrapping the given items.
pub fn tensor_val(items: Vec<Value>) -> Value {
    Value::Tensor(items)
}

// --- TypeAliasMap builder ---

/// Fluent builder for `HashMap<String, Type>`, used to describe type alias mappings
/// in tests without a full compiler IR type alias implementation.
///
/// # Example
/// ```ignore
/// let aliases = TypeAliasMap::new()
///     .alias("Pressure", pressure_type)
///     .alias("Velocity", velocity_type)
///     .build();
/// ```
pub struct TypeAliasMap {
    entries: HashMap<String, Type>,
}

impl TypeAliasMap {
    /// Create an empty TypeAliasMap builder.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Add a type alias entry.
    pub fn alias(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.entries.insert(name.into(), ty);
        self
    }

    /// Consume the builder and return the completed alias map.
    pub fn build(self) -> HashMap<String, Type> {
        self.entries
    }
}

impl Default for TypeAliasMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Return a map of common engineering type aliases with correct dimension vectors.
///
/// Includes:
/// - `Pressure`: FORCE / AREA (Pa = kg·m⁻¹·s⁻²)
/// - `Velocity`: LENGTH / TIME (m/s)
/// - `Acceleration`: LENGTH / TIME² (m/s²)
pub fn common_type_aliases() -> HashMap<String, Type> {
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);

    TypeAliasMap::new()
        .alias(
            "Pressure",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        )
        .alias(
            "Velocity",
            Type::Scalar {
                dimension: velocity_dim,
            },
        )
        .alias(
            "Acceleration",
            Type::Scalar {
                dimension: DimensionVector::ACCELERATION,
            },
        )
        .build()
}

/// Build a `MultiCaseResult`-shaped `Value` from a slice of `(case_name, Value)` pairs.
///
/// The runtime shape is an outer `Value::Map` with a single key `"cases"` whose
/// value is an inner `Value::Map<Value::String, Value>` of per-case entries:
///
/// ```text
/// Value::Map {
///   Value::String("cases") => Value::Map {
///     Value::String("operating") => <case_value>,
///     Value::String("overload")  => <case_value>,
///     ...
///   }
/// }
/// ```
///
/// This helper is the single construction site for the `MultiCaseResult` shape.
/// Producers (`solve_load_cases` — PRD task 3005) emit this shape at runtime;
/// consumers (`envelope_*`, `linear_combine`, GUI dropdown) read it — all should
/// call this function to construct test fixtures rather than re-deriving the shape.
pub fn multi_case_result_value(cases: &[(&str, Value)]) -> Value {
    let mut inner = BTreeMap::new();
    for (name, val) in cases {
        inner.insert(Value::String((*name).to_string()), val.clone());
    }
    let mut outer = BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(inner));
    Value::Map(outer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{DeterminacyState, Value};
    use std::collections::HashMap;

    // step-1: failing tests for range value constructors
    #[test]
    fn range_inclusive_creates_both_bounds_inclusive() {
        let v = range_inclusive(mm(10.0), mm(100.0));
        match v {
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_some(), "lower should be Some");
                assert!(upper.is_some(), "upper should be Some");
                assert!(lower_inclusive, "lower_inclusive should be true");
                assert!(upper_inclusive, "upper_inclusive should be true");
                assert_eq!(*lower.unwrap(), mm(10.0));
                assert_eq!(*upper.unwrap(), mm(100.0));
            }
            _ => panic!("expected Value::Range"),
        }
    }

    #[test]
    fn range_exclusive_creates_both_bounds_exclusive() {
        let v = range_exclusive(mm(10.0), mm(100.0));
        match v {
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_some(), "lower should be Some");
                assert!(upper.is_some(), "upper should be Some");
                assert!(!lower_inclusive, "lower_inclusive should be false");
                assert!(!upper_inclusive, "upper_inclusive should be false");
            }
            _ => panic!("expected Value::Range"),
        }
    }

    #[test]
    fn range_at_least_creates_lower_bound_only() {
        let v = range_at_least(mm(5.0));
        match v {
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_some(), "lower should be Some");
                assert!(upper.is_none(), "upper should be None");
                assert!(lower_inclusive, "lower_inclusive should be true");
                assert!(
                    !upper_inclusive,
                    "upper_inclusive should be false (no upper bound)"
                );
            }
            _ => panic!("expected Value::Range"),
        }
    }

    #[test]
    fn range_at_most_creates_upper_bound_only() {
        let v = range_at_most(mm(50.0));
        match v {
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_none(), "lower should be None");
                assert!(upper.is_some(), "upper should be Some");
                assert!(
                    !lower_inclusive,
                    "lower_inclusive should be false (no lower bound)"
                );
                assert!(upper_inclusive, "upper_inclusive should be true");
            }
            _ => panic!("expected Value::Range"),
        }
    }

    #[test]
    fn range_half_open_creates_lower_inclusive_upper_exclusive() {
        let v = range_half_open(mm(0.0), mm(100.0));
        match v {
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_some(), "lower should be Some");
                assert!(upper.is_some(), "upper should be Some");
                assert!(lower_inclusive, "lower_inclusive should be true");
                assert!(!upper_inclusive, "upper_inclusive should be false");
            }
            _ => panic!("expected Value::Range"),
        }
    }

    // step-3: failing tests for temperature value constructors
    #[test]
    fn kelvin_creates_temperature_scalar() {
        let v = kelvin(300.0);
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 300.0).abs() < 1e-9, "si_value should be 300.0");
                assert_eq!(dimension, DimensionVector::TEMPERATURE);
            }
            _ => panic!("expected Value::Scalar"),
        }
    }

    #[test]
    fn celsius_converts_to_kelvin() {
        let v0 = celsius(0.0);
        let v100 = celsius(100.0);
        match v0 {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 273.15).abs() < 1e-9, "0°C should be 273.15 K");
                assert_eq!(dimension, DimensionVector::TEMPERATURE);
            }
            _ => panic!("expected Value::Scalar"),
        }
        match v100 {
            Value::Scalar { si_value, .. } => {
                assert!((si_value - 373.15).abs() < 1e-9, "100°C should be 373.15 K");
            }
            _ => panic!("expected Value::Scalar"),
        }
    }

    #[test]
    fn fahrenheit_converts_to_kelvin() {
        let v_freeze = fahrenheit(32.0);
        let v_boil = fahrenheit(212.0);
        match v_freeze {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 273.15).abs() < 1e-9,
                    "32°F should be 273.15 K, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::TEMPERATURE);
            }
            _ => panic!("expected Value::Scalar"),
        }
        match v_boil {
            Value::Scalar { si_value, .. } => {
                assert!(
                    (si_value - 373.15).abs() < 1e-9,
                    "212°F should be 373.15 K, got {}",
                    si_value
                );
            }
            _ => panic!("expected Value::Scalar"),
        }
    }

    // step-1: failing test for kg() constructor (task 1718)
    #[test]
    fn kg_creates_mass_scalar() {
        let v = kg(2.5);
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 2.5).abs() < 1e-9, "si_value should be 2.5");
                assert_eq!(dimension, DimensionVector::MASS);
            }
            _ => panic!("expected Value::Scalar"),
        }
    }

    // step-19: failing tests for SnapshotValuesBuilder
    #[test]
    fn snapshot_values_builder_determined() {
        let id = vcid("Entity", "param");
        let snapshot: HashMap<ValueCellId, (Value, DeterminacyState)> =
            SnapshotValuesBuilder::new()
                .determined(id.clone(), Value::Int(10))
                .build();
        assert_eq!(snapshot.len(), 1);
        let (val, state) = &snapshot[&id];
        assert_eq!(*val, Value::Int(10));
        assert_eq!(*state, DeterminacyState::Determined);
    }

    #[test]
    fn snapshot_values_builder_undetermined() {
        let id = vcid("Entity", "param");
        let snapshot: HashMap<ValueCellId, (Value, DeterminacyState)> =
            SnapshotValuesBuilder::new()
                .undetermined(id.clone())
                .build();
        assert_eq!(snapshot.len(), 1);
        let (val, state) = &snapshot[&id];
        assert_eq!(*val, Value::Undef);
        assert_eq!(*state, DeterminacyState::Undetermined);
    }

    #[test]
    fn snapshot_values_builder_provisional() {
        let id = vcid("Entity", "param");
        let snapshot: HashMap<ValueCellId, (Value, DeterminacyState)> =
            SnapshotValuesBuilder::new()
                .provisional(id.clone(), Value::Real(2.5))
                .build();
        assert_eq!(snapshot.len(), 1);
        let (val, state) = &snapshot[&id];
        assert_eq!(*val, Value::Real(2.5));
        assert_eq!(*state, DeterminacyState::Provisional);
    }

    #[test]
    fn snapshot_values_builder_auto_val() {
        let id = vcid("Entity", "param");
        let snapshot: HashMap<ValueCellId, (Value, DeterminacyState)> =
            SnapshotValuesBuilder::new().auto_val(id.clone()).build();
        assert_eq!(snapshot.len(), 1);
        let (val, state) = &snapshot[&id];
        assert_eq!(*val, Value::Undef);
        assert_eq!(*state, DeterminacyState::Auto);
    }

    #[test]
    fn snapshot_values_builder_mixed() {
        let id1 = vcid("Entity", "p1");
        let id2 = vcid("Entity", "p2");
        let id3 = vcid("Entity", "p3");
        let id4 = vcid("Entity", "p4");
        let snapshot: HashMap<ValueCellId, (Value, DeterminacyState)> =
            SnapshotValuesBuilder::new()
                .determined(id1.clone(), Value::Int(1))
                .undetermined(id2.clone())
                .provisional(id3.clone(), Value::Int(3))
                .auto_val(id4.clone())
                .build();
        assert_eq!(snapshot.len(), 4);
        assert_eq!(snapshot[&id1].1, DeterminacyState::Determined);
        assert_eq!(snapshot[&id2].1, DeterminacyState::Undetermined);
        assert_eq!(snapshot[&id3].1, DeterminacyState::Provisional);
        assert_eq!(snapshot[&id4].1, DeterminacyState::Auto);
    }

    // step-17: failing tests for determinacy state helpers
    #[test]
    fn determined_returns_correct_state() {
        let (val, state) = determined(Value::Int(42));
        assert_eq!(val, Value::Int(42));
        assert_eq!(state, DeterminacyState::Determined);
    }

    #[test]
    fn undetermined_returns_undef_and_undetermined() {
        let (val, state) = undetermined();
        assert_eq!(val, Value::Undef);
        assert_eq!(state, DeterminacyState::Undetermined);
    }

    #[test]
    fn provisional_returns_correct_state() {
        let (val, state) = provisional(Value::Real(2.78));
        assert_eq!(val, Value::Real(2.78));
        assert_eq!(state, DeterminacyState::Provisional);
    }

    #[test]
    fn auto_state_returns_undef_and_auto() {
        let (val, state) = auto_state();
        assert_eq!(val, Value::Undef);
        assert_eq!(state, DeterminacyState::Auto);
    }

    #[test]
    fn int_val_produces_int() {
        let v = int_val(42);
        assert!(matches!(v, Value::Int(42)));
    }

    #[test]
    fn real_val_produces_real() {
        let v = real_val(2.78);
        assert!(matches!(v, Value::Real(_)));
        if let Value::Real(f) = v {
            assert!((f - 2.78).abs() < 1e-10);
        }
    }

    #[test]
    fn bool_val_produces_bool() {
        assert!(matches!(bool_val(true), Value::Bool(true)));
        assert!(matches!(bool_val(false), Value::Bool(false)));
    }

    #[test]
    fn string_val_produces_string() {
        let v = string_val("hi");
        assert!(matches!(v, Value::String(ref s) if s == "hi"));
    }

    #[test]
    fn enum_val_produces_enum() {
        let v = enum_val("Color", "Red");
        assert!(
            matches!(v, Value::Enum { ref type_name, ref variant } if type_name == "Color" && variant == "Red")
        );
    }

    #[test]
    fn list_val_produces_list() {
        let v = list_val(vec![Value::Int(1), Value::Int(2)]);
        if let Value::List(items) = v {
            assert_eq!(items.len(), 2);
            assert!(matches!(items[0], Value::Int(1)));
            assert!(matches!(items[1], Value::Int(2)));
        } else {
            panic!("expected Value::List");
        }
    }

    #[test]
    fn set_val_produces_set() {
        let v = set_val(vec![Value::Int(1), Value::Int(2)]);
        if let Value::Set(s) = v {
            assert_eq!(s.len(), 2);
        } else {
            panic!("expected Value::Set");
        }
    }

    #[test]
    fn map_val_produces_map() {
        let v = map_val(vec![(string_val("key"), Value::Int(99))]);
        if let Value::Map(m) = v {
            assert_eq!(m.len(), 1);
        } else {
            panic!("expected Value::Map");
        }
    }

    #[test]
    fn some_val_produces_some_option() {
        let v = some_val(Value::Int(1));
        if let Value::Option(Some(inner)) = v {
            assert!(matches!(*inner, Value::Int(1)));
        } else {
            panic!("expected Value::Option(Some(_))");
        }
    }

    #[test]
    fn none_val_produces_none_option() {
        let v = none_val();
        assert!(matches!(v, Value::Option(None)));
    }

    #[test]
    fn undef_produces_undef() {
        let v = undef();
        assert!(matches!(v, Value::Undef));
    }

    // step-9: failing tests for TypeAliasMap builder
    #[test]
    fn type_alias_map_builder_stores_entries() {
        use reify_core::dimension::FORCE;
        use reify_core::{DimensionVector, Type};
        let pressure_type = Type::Scalar {
            dimension: FORCE.div(&DimensionVector::AREA),
        };
        let velocity_type = Type::Scalar {
            dimension: DimensionVector::LENGTH.div(&DimensionVector::TIME),
        };
        let map = TypeAliasMap::new()
            .alias("Pressure", pressure_type.clone())
            .alias("Velocity", velocity_type.clone())
            .build();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("Pressure"), Some(&pressure_type));
        assert_eq!(map.get("Velocity"), Some(&velocity_type));
    }

    // step-1: failing tests for point3/vec3/vec3_dimensionless/matrix3x3 constructors
    #[test]
    fn point3_creates_value_point_with_length_components() {
        let p = point3(1.0, 2.0, 3.0);
        match p {
            Value::Point(items) => {
                assert_eq!(items.len(), 3);
                // Each component should be a Value::Scalar with LENGTH dimension
                for (i, expected_m) in [1.0, 2.0, 3.0].iter().enumerate() {
                    match &items[i] {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert!(
                                (si_value - expected_m).abs() < 1e-12,
                                "component {} si_value: expected {}, got {}",
                                i,
                                expected_m,
                                si_value
                            );
                            assert_eq!(
                                *dimension,
                                reify_core::dimension::DimensionVector::LENGTH,
                                "component {} should have LENGTH dimension",
                                i
                            );
                        }
                        other => panic!("component {} should be Scalar, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Value::Point, got {:?}", other),
        }
    }

    #[test]
    fn vec3_creates_value_vector_with_length_components() {
        let v = vec3(0.5, -1.0, 0.0);
        match v {
            Value::Vector(items) => {
                assert_eq!(items.len(), 3);
                for (i, expected_m) in [0.5, -1.0, 0.0].iter().enumerate() {
                    match &items[i] {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert!(
                                (si_value - expected_m).abs() < 1e-12,
                                "component {} si_value: expected {}, got {}",
                                i,
                                expected_m,
                                si_value
                            );
                            assert_eq!(
                                *dimension,
                                reify_core::dimension::DimensionVector::LENGTH,
                                "component {} should have LENGTH dimension",
                                i
                            );
                        }
                        other => panic!("component {} should be Scalar, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }

    #[test]
    fn vec3_dimensionless_creates_value_vector_with_real_components() {
        let v = vec3_dimensionless(1.0, 0.0, 0.0);
        match v {
            Value::Vector(items) => {
                assert_eq!(items.len(), 3);
                for (i, expected) in [1.0, 0.0, 0.0].iter().enumerate() {
                    match &items[i] {
                        Value::Real(f) => {
                            assert!(
                                (f - expected).abs() < 1e-12,
                                "component {} Real: expected {}, got {}",
                                i,
                                expected,
                                f
                            );
                        }
                        other => panic!("component {} should be Real, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }

    #[test]
    fn matrix3x3_creates_value_matrix_with_real_entries() {
        let m = matrix3x3(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        match m {
            Value::Matrix(rows) => {
                assert_eq!(rows.len(), 3, "should have 3 rows");
                let expected = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
                for (r, row) in rows.iter().enumerate() {
                    assert_eq!(row.len(), 3, "row {} should have 3 columns", r);
                    for (c, val) in row.iter().enumerate() {
                        match val {
                            Value::Real(f) => {
                                assert!(
                                    (f - expected[r][c]).abs() < 1e-12,
                                    "matrix[{}][{}]: expected {}, got {}",
                                    r,
                                    c,
                                    expected[r][c],
                                    f
                                );
                            }
                            other => panic!("matrix[{}][{}] should be Real, got {:?}", r, c, other),
                        }
                    }
                }
            }
            other => panic!("expected Value::Matrix, got {:?}", other),
        }
    }

    // step-3: failing tests for assert_value_approx macro
    #[test]
    fn assert_value_approx_equal_scalars_pass() {
        let a = mm(10.0);
        let b = mm(10.0);
        assert_value_approx!(a, b);
    }

    #[test]
    fn assert_value_approx_scalars_within_tolerance_pass() {
        let a = meters(1.0);
        let b = Value::Scalar {
            si_value: 1.0 + 1e-10,
            dimension: DimensionVector::LENGTH,
        };
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    #[should_panic(expected = "values differ")]
    fn assert_value_approx_scalars_beyond_tolerance_panics() {
        let a = meters(1.0);
        let b = meters(2.0);
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    fn assert_value_approx_reals_pass() {
        assert_value_approx!(Value::Real(2.78), Value::Real(2.78));
    }

    #[test]
    fn assert_value_approx_ints_pass() {
        assert_value_approx!(Value::Int(42), Value::Int(42));
    }

    #[test]
    #[should_panic(expected = "values differ")]
    fn assert_value_approx_ints_mismatch_panics() {
        assert_value_approx!(Value::Int(1), Value::Int(2));
    }

    #[test]
    fn assert_value_approx_bools_pass() {
        assert_value_approx!(Value::Bool(true), Value::Bool(true));
    }

    #[test]
    fn assert_value_approx_point_components_pass() {
        let a = point3(1.0, 2.0, 3.0);
        let b = point3(1.0, 2.0, 3.0);
        assert_value_approx!(a, b);
    }

    #[test]
    fn assert_value_approx_vector_components_pass() {
        let a = vec3(1.0, 0.0, 0.0);
        let b = vec3(1.0 + 1e-11, 0.0, 0.0);
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    #[should_panic(expected = "values differ")]
    fn assert_value_approx_point_mismatch_panics() {
        let a = point3(1.0, 2.0, 3.0);
        let b = point3(1.0, 2.0, 4.0);
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    #[should_panic(expected = "dimension mismatch")]
    fn assert_value_approx_dimension_mismatch_panics() {
        let a = meters(1.0);
        let b = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert_value_approx!(a, b);
    }

    #[test]
    #[should_panic(expected = "length mismatch")]
    fn assert_value_approx_component_count_mismatch_panics() {
        let a = point3(1.0, 2.0, 3.0);
        let b = Value::Point(vec![meters(1.0), meters(2.0)]);
        assert_value_approx!(a, b);
    }

    #[test]
    #[should_panic(expected = "values differ")]
    fn assert_value_approx_type_variant_mismatch_panics() {
        let a = Value::Int(1);
        let b = Value::Real(1.0);
        assert_value_approx!(a, b);
    }

    #[test]
    #[should_panic(expected = "NaN detected")]
    fn assert_value_approx_nan_scalar_panics() {
        let a = meters(1.0);
        let b = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        assert_value_approx!(a, b);
    }

    #[test]
    #[should_panic(expected = "NaN detected")]
    fn assert_value_approx_nan_real_panics() {
        assert_value_approx!(Value::Real(f64::NAN), Value::Real(1.0));
    }

    // Complex value tests for assert_value_approx
    #[test]
    fn assert_value_approx_complex_within_tolerance_pass() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0 + 1e-11,
            im: 2.0 - 1e-11,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    #[should_panic(expected = "values differ")]
    fn assert_value_approx_complex_re_out_of_tolerance_panics() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    #[should_panic(expected = "values differ")]
    fn assert_value_approx_complex_im_out_of_tolerance_panics() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 5.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_value_approx!(a, b, 1e-9);
    }

    #[test]
    #[should_panic(expected = "dimension mismatch")]
    fn assert_value_approx_complex_dimension_mismatch_panics() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_value_approx!(a, b);
    }

    #[test]
    #[should_panic(expected = "NaN detected")]
    fn assert_value_approx_complex_nan_re_panics() {
        let a = Value::Complex {
            re: f64::NAN,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_value_approx!(a, b);
    }

    #[test]
    #[should_panic(expected = "NaN detected")]
    fn assert_value_approx_complex_nan_im_panics() {
        let a = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_value_approx!(a, b);
    }

    #[test]
    fn common_type_aliases_has_standard_engineering_types() {
        let aliases = common_type_aliases();
        assert!(aliases.contains_key("Pressure"), "should have Pressure");
        assert!(aliases.contains_key("Velocity"), "should have Velocity");
        assert!(
            aliases.contains_key("Acceleration"),
            "should have Acceleration"
        );
        // Pressure must be a Scalar with the canonical PRESSURE dimension (kg·m⁻¹·s⁻²)
        match aliases["Pressure"] {
            Type::Scalar { dimension } => {
                assert_eq!(
                    dimension,
                    DimensionVector::PRESSURE,
                    "Pressure alias dimension should equal DimensionVector::PRESSURE"
                );
            }
            _ => panic!("Pressure should be a Scalar type"),
        }
        // Velocity must be a Scalar with LENGTH/TIME dimension (m/s)
        match aliases["Velocity"] {
            Type::Scalar { dimension } => {
                assert_eq!(
                    dimension,
                    DimensionVector::LENGTH.div(&DimensionVector::TIME),
                    "Velocity alias dimension should equal LENGTH/TIME"
                );
            }
            _ => panic!("Velocity should be a Scalar type"),
        }
        // Acceleration must be a Scalar with the canonical ACCELERATION dimension (m·s⁻²)
        match aliases["Acceleration"] {
            Type::Scalar { dimension } => {
                assert_eq!(
                    dimension,
                    DimensionVector::ACCELERATION,
                    "Acceleration alias dimension should equal DimensionVector::ACCELERATION"
                );
            }
            _ => panic!("Acceleration should be a Scalar type"),
        }
    }

    // --- complex_val tests (step 19) ---

    #[test]
    fn complex_val_produces_dimensionless_complex() {
        let v = complex_val(3.0, 4.0);
        match v {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12);
                assert!((im - 4.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            _ => panic!("expected Value::Complex"),
        }
    }

    #[test]
    fn complex_val_dimensioned_produces_complex_with_dimension() {
        let v = complex_val_dimensioned(1.0, 2.0, DimensionVector::LENGTH);
        match v {
            Value::Complex { re, im, dimension } => {
                assert!((re - 1.0).abs() < 1e-12);
                assert!((im - 2.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            _ => panic!("expected Value::Complex"),
        }
    }

    // --- point2/vec2 tests (step 21) ---

    #[test]
    fn point2_creates_value_point_with_length_components() {
        let p = point2(1.0, 2.0);
        match p {
            Value::Point(items) => {
                assert_eq!(items.len(), 2);
                for (i, expected) in [1.0, 2.0].iter().enumerate() {
                    match &items[i] {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert!((si_value - expected).abs() < 1e-12);
                            assert_eq!(*dimension, DimensionVector::LENGTH);
                        }
                        _ => panic!("component {} should be Scalar", i),
                    }
                }
            }
            _ => panic!("expected Value::Point"),
        }
    }

    #[test]
    fn vec2_creates_value_vector_with_length_components() {
        let v = vec2(5.0, -3.0);
        match v {
            Value::Vector(items) => {
                assert_eq!(items.len(), 2);
                for (i, expected) in [5.0, -3.0].iter().enumerate() {
                    match &items[i] {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert!((si_value - expected).abs() < 1e-12);
                            assert_eq!(*dimension, DimensionVector::LENGTH);
                        }
                        _ => panic!("component {} should be Scalar", i),
                    }
                }
            }
            _ => panic!("expected Value::Vector"),
        }
    }

    #[test]
    fn vec2_dimensionless_creates_vector_with_two_real_components() {
        let v = vec2_dimensionless(1.0, 2.0);
        match v {
            Value::Vector(items) => {
                assert_eq!(items.len(), 2);
                for (i, expected) in [1.0, 2.0].iter().enumerate() {
                    match &items[i] {
                        Value::Real(f) => {
                            assert!(
                                (f - expected).abs() < 1e-12,
                                "component {} Real: expected {}, got {}",
                                i,
                                expected,
                                f
                            );
                        }
                        other => panic!("component {} should be Real, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }

    #[test]
    fn point2_dimensionless_creates_point_with_two_real_components() {
        let p = point2_dimensionless(3.0, 4.0);
        match p {
            Value::Point(items) => {
                assert_eq!(items.len(), 2);
                for (i, expected) in [3.0, 4.0].iter().enumerate() {
                    match &items[i] {
                        Value::Real(f) => {
                            assert!(
                                (f - expected).abs() < 1e-12,
                                "component {} Real: expected {}, got {}",
                                i,
                                expected,
                                f
                            );
                        }
                        other => panic!("component {} should be Real, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Value::Point, got {:?}", other),
        }
    }

    // --- orientation_val tests (step 23) ---

    #[test]
    fn orientation_val_produces_quaternion() {
        let v = orientation_val(1.0, 0.0, 0.0, 0.0);
        match v {
            Value::Orientation { w, x, y, z } => {
                assert!((w - 1.0).abs() < 1e-12);
                assert!((x - 0.0).abs() < 1e-12);
                assert!((y - 0.0).abs() < 1e-12);
                assert!((z - 0.0).abs() < 1e-12);
            }
            _ => panic!("expected Value::Orientation"),
        }
    }

    // --- frame_val / transform_val tests (step 25) ---

    #[test]
    fn frame_val_produces_frame() {
        let origin = point3(0.0, 0.0, 0.0);
        let basis = orientation_val(1.0, 0.0, 0.0, 0.0);
        let v = frame_val(origin.clone(), basis.clone());
        match v {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert!(matches!(*o, Value::Point(_)));
                assert!(matches!(*b, Value::Orientation { .. }));
            }
            _ => panic!("expected Value::Frame"),
        }
    }

    #[test]
    fn transform_val_produces_transform() {
        let rotation = orientation_val(1.0, 0.0, 0.0, 0.0);
        let translation = vec3(1.0, 2.0, 3.0);
        let v = transform_val(rotation, translation);
        match v {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert!(matches!(*r, Value::Orientation { .. }));
                assert!(matches!(*t, Value::Vector(_)));
            }
            _ => panic!("expected Value::Transform"),
        }
    }

    // --- plane_val / axis_val / bbox_val tests (step 27) ---

    #[test]
    fn plane_val_produces_plane() {
        let origin = point3(0.0, 0.0, 0.0);
        let normal = vec3_dimensionless(0.0, 0.0, 1.0);
        let v = plane_val(origin, normal);
        assert!(matches!(v, Value::Plane { .. }));
    }

    #[test]
    fn axis_val_produces_axis() {
        let origin = point3(0.0, 0.0, 0.0);
        let direction = vec3_dimensionless(1.0, 0.0, 0.0);
        let v = axis_val(origin, direction);
        assert!(matches!(v, Value::Axis { .. }));
    }

    #[test]
    fn bbox_val_produces_bounding_box() {
        let min = point3(0.0, 0.0, 0.0);
        let max = point3(1.0, 1.0, 1.0);
        let v = bbox_val(min, max);
        assert!(matches!(v, Value::BoundingBox { .. }));
    }

    // --- tensor_val tests (step 29) ---

    #[test]
    fn tensor_val_wraps_items() {
        let items = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        let v = tensor_val(items.clone());
        match v {
            Value::Tensor(vals) => {
                assert_eq!(vals.len(), 3);
                assert!(matches!(vals[0], Value::Real(1.0)));
            }
            _ => panic!("expected Value::Tensor"),
        }
    }

    // step-N: tests for multi_case_result_value

    #[test]
    fn multi_case_result_value_empty_yields_outer_map_with_empty_cases_inner() {
        let v = multi_case_result_value(&[]);
        match v {
            Value::Map(outer) => {
                assert_eq!(outer.len(), 1, "outer map should have exactly one key");
                let cases_key = Value::String("cases".to_string());
                match outer.get(&cases_key) {
                    Some(Value::Map(inner)) => {
                        assert_eq!(inner.len(), 0, "inner cases map should be empty");
                    }
                    Some(other) => panic!("expected Value::Map for 'cases', got {:?}", other),
                    None => panic!("outer map missing 'cases' key"),
                }
            }
            _ => panic!("expected Value::Map"),
        }
    }

    #[test]
    fn multi_case_result_value_inner_keyed_by_value_string_with_per_case_values() {
        let v =
            multi_case_result_value(&[("operating", Value::Int(42)), ("overload", Value::Int(99))]);
        match v {
            Value::Map(outer) => {
                assert_eq!(outer.len(), 1, "outer map should have exactly one key");
                let cases_key = Value::String("cases".to_string());
                match outer.get(&cases_key) {
                    Some(Value::Map(inner)) => {
                        assert_eq!(inner.len(), 2, "inner map should have 2 entries");
                        let op_key = Value::String("operating".to_string());
                        assert_eq!(
                            inner.get(&op_key),
                            Some(&Value::Int(42)),
                            "operating case should be Int(42)"
                        );
                        let ov_key = Value::String("overload".to_string());
                        assert_eq!(
                            inner.get(&ov_key),
                            Some(&Value::Int(99)),
                            "overload case should be Int(99)"
                        );
                    }
                    Some(other) => panic!("expected Value::Map for 'cases', got {:?}", other),
                    None => panic!("outer map missing 'cases' key"),
                }
            }
            _ => panic!("expected Value::Map"),
        }
    }
}
