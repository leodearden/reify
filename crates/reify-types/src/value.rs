use crate::dimension::DimensionVector;
use crate::hash::ContentHash;
use crate::identity::ValueCellId;
use crate::persistent::PersistentMap;

/// Runtime values in Reify (M1 subset).
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    /// Dimensioned scalar: value in SI base units, with dimension.
    Scalar { si_value: f64, dimension: DimensionVector },
    /// Undefined — not yet determined or computation failed.
    Undef,
}

impl Value {
    /// Create a scalar with LENGTH dimension from a value in meters.
    pub fn length(meters: f64) -> Self {
        Value::Scalar {
            si_value: meters,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// Create a scalar with ANGLE dimension from a value in radians.
    pub fn angle(radians: f64) -> Self {
        Value::Scalar {
            si_value: radians,
            dimension: DimensionVector::ANGLE,
        }
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Value::Undef)
    }

    /// Get the f64 value if this is a numeric type.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Scalar { si_value, .. } => Some(*si_value),
            _ => None,
        }
    }

    /// Get the dimension of this value (DIMENSIONLESS for non-scalar types).
    pub fn dimension(&self) -> DimensionVector {
        match self {
            Value::Scalar { dimension, .. } => *dimension,
            _ => DimensionVector::DIMENSIONLESS,
        }
    }

    /// Compute a content hash for incremental caching.
    pub fn content_hash(&self) -> ContentHash {
        match self {
            Value::Bool(b) => ContentHash::of(&[0, *b as u8]),
            Value::Int(i) => {
                let mut buf = [0u8; 9];
                buf[0] = 1;
                buf[1..].copy_from_slice(&i.to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::Real(r) => {
                let mut buf = [0u8; 9];
                buf[0] = 2;
                // Normalize -0.0 to 0.0 and NaN to a canonical form
                let normalized = if *r == 0.0 { 0.0f64 } else { *r };
                let bits = if normalized.is_nan() {
                    f64::NAN.to_bits() // canonical NaN
                } else {
                    normalized.to_bits()
                };
                buf[1..].copy_from_slice(&bits.to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::String(s) => ContentHash::of(&[3]).combine(ContentHash::of_str(s)),
            Value::Scalar { si_value, dimension } => {
                let normalized = if *si_value == 0.0 { 0.0f64 } else { *si_value };
                let bits = if normalized.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    normalized.to_bits()
                };
                let mut buf = [0u8; 9];
                buf[0] = 4;
                buf[1..].copy_from_slice(&bits.to_le_bytes());
                ContentHash::of(&buf).combine(dimension.content_hash())
            }
            Value::Undef => ContentHash::of(&[5]),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a.to_bits() == b.to_bits(),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Scalar { si_value: a, dimension: ad }, Value::Scalar { si_value: b, dimension: bd }) => {
                a.to_bits() == b.to_bits() && ad == bd
            }
            (Value::Undef, Value::Undef) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Type-tag discriminant for cross-type ordering:
        // Undef=0, Bool=1, Int=2, Real=3, Scalar=4, String=5
        fn type_tag(v: &Value) -> u8 {
            match v {
                Value::Undef => 0,
                Value::Bool(_) => 1,
                Value::Int(_) => 2,
                Value::Real(_) => 3,
                Value::Scalar { .. } => 4,
                Value::String(_) => 5,
            }
        }

        let tag_a = type_tag(self);
        let tag_b = type_tag(other);

        if tag_a != tag_b {
            return tag_a.cmp(&tag_b);
        }

        // Same type — compare within type
        match (self, other) {
            (Value::Undef, Value::Undef) => Ordering::Equal,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Real(a), Value::Real(b)) => {
                // Total order via to_bits(), consistent with PartialEq
                a.to_bits().cmp(&b.to_bits())
            }
            (Value::Scalar { si_value: a, dimension: ad }, Value::Scalar { si_value: b, dimension: bd }) => {
                // Compare by dimension first, then by value bits
                ad.cmp(bd).then_with(|| a.to_bits().cmp(&b.to_bits()))
            }
            (Value::String(a), Value::String(b)) => a.cmp(b),
            _ => unreachable!("same type tag but different variants"),
        }
    }
}

/// The determinacy state of a value cell in the evaluation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeterminacyState {
    /// Value is fully determined.
    Determined,
    /// Value cannot be determined (missing input, cycle, error).
    Undetermined,
    /// Value is provisionally determined (may change during solving).
    Provisional,
    /// Value is marked auto — to be resolved by the constraint solver.
    Auto,
}

/// The satisfaction state of a constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Satisfaction {
    /// All constraint conditions are met.
    Satisfied,
    /// At least one constraint condition is violated.
    Violated,
    /// Satisfaction cannot be determined (undef inputs).
    Indeterminate,
}

impl Satisfaction {
    /// Compute a content hash for incremental caching.
    /// Domain-separated with tag byte [10] to avoid collisions with Value hashes.
    pub fn content_hash(&self) -> ContentHash {
        match self {
            Satisfaction::Satisfied => ContentHash::of(&[10, 0]),
            Satisfaction::Violated => ContentHash::of(&[10, 1]),
            Satisfaction::Indeterminate => ContentHash::of(&[10, 2]),
        }
    }
}

/// An error produced during value evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError(pub String);

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EvalError {}

/// Freshness of a cached value (for incremental evaluation).
///
/// M2 model: tracks evaluation lifecycle with richer state than M1's
/// simple Fresh/Stale/Uncomputed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Value is fully computed and up-to-date.
    Final,
    /// Value is a provisional result from an in-progress evaluation pass.
    Intermediate { generation: u64 },
    /// Value has been requested but not yet computed.
    /// `last_substantive` holds the content hash of the last known-good value, if any.
    Pending { last_substantive: Option<ContentHash> },
    /// Evaluation failed with an error.
    Failed { error: EvalError },
}

/// Map from ValueCellId to Value. Uses PersistentMap (im::HashMap) for
/// O(1) structural-sharing clones.
#[derive(Debug, Clone, Default)]
pub struct ValueMap {
    inner: PersistentMap<ValueCellId, Value>,
}

impl ValueMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &ValueCellId) -> Option<&Value> {
        self.inner.get(id)
    }

    pub fn insert(&mut self, id: ValueCellId, value: Value) {
        self.inner.insert(id, value);
    }

    pub fn contains(&self, id: &ValueCellId) -> bool {
        self.inner.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ValueCellId, &Value)> {
        self.inner.iter()
    }

    /// Get a value, returning Undef if not present.
    pub fn get_or_undef(&self, id: &ValueCellId) -> Value {
        self.inner.get(id).cloned().unwrap_or(Value::Undef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_content_hash_determinism() {
        let v1 = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        let v2 = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(v1.content_hash(), v2.content_hash());
    }

    #[test]
    fn neg_zero_normalized() {
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        assert_eq!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn nan_normalized() {
        let nan1 = Value::Real(f64::NAN);
        let nan2 = Value::Real(f64::NAN);
        assert_eq!(nan1.content_hash(), nan2.content_hash());
    }

    #[test]
    fn different_values_different_hashes() {
        let a = Value::length(0.08);
        let b = Value::length(0.10);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn different_dimensions_different_hashes() {
        let len = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let mass = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert_ne!(len.content_hash(), mass.content_hash());
    }

    #[test]
    fn test_freshness_final() {
        let f = Freshness::Final;
        let f2 = f.clone();
        assert_eq!(f, f2);
        assert_eq!(format!("{:?}", f), "Final");
    }

    #[test]
    fn test_freshness_intermediate() {
        let f = Freshness::Intermediate { generation: 42 };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Intermediate { generation } => assert_eq!(*generation, 42),
            _ => panic!("expected Intermediate"),
        }
    }

    #[test]
    fn test_freshness_pending_none() {
        let f = Freshness::Pending { last_substantive: None };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Pending { last_substantive } => assert!(last_substantive.is_none()),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn test_freshness_pending_some() {
        let hash = ContentHash::of(b"test");
        let f = Freshness::Pending { last_substantive: Some(hash) };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Pending { last_substantive } => assert_eq!(*last_substantive, Some(hash)),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn test_freshness_failed() {
        let err = EvalError("type mismatch".to_string());
        let f = Freshness::Failed { error: err.clone() };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Failed { error } => assert_eq!(error.0, "type mismatch"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn test_eval_error_display() {
        let err = EvalError("division by zero".to_string());
        assert_eq!(format!("{}", err), "division by zero");
        assert_eq!(err.0, "division by zero");

        // Verify Clone and PartialEq
        let err2 = err.clone();
        assert_eq!(err, err2);
    }

    #[test]
    fn value_map_get_or_undef() {
        let mut map = ValueMap::new();
        let id = ValueCellId::new("Bracket", "width");
        map.insert(id.clone(), Value::length(0.08));
        assert!(!map.get_or_undef(&id).is_undef());
        assert!(map.get_or_undef(&ValueCellId::new("Bracket", "missing")).is_undef());
    }

    #[test]
    fn value_map_clone_structural_sharing() {
        let mut original = ValueMap::new();
        let id_width = ValueCellId::new("Bracket", "width");
        let id_height = ValueCellId::new("Bracket", "height");
        let id_depth = ValueCellId::new("Bracket", "depth");

        original.insert(id_width.clone(), Value::length(0.08));
        original.insert(id_height.clone(), Value::length(0.10));

        // Clone the map (O(1) structural sharing via im::HashMap)
        let mut cloned = original.clone();

        // Insert into the clone
        cloned.insert(id_depth.clone(), Value::length(0.05));

        // Original is unmodified
        assert_eq!(original.len(), 2);
        assert!(!original.contains(&id_depth));
        assert!(original.contains(&id_width));

        // Clone has all three entries
        assert_eq!(cloned.len(), 3);
        assert!(cloned.contains(&id_depth));
        assert!(cloned.contains(&id_width));

        // Original values are still correct
        match original.get(&id_width) {
            Some(Value::Scalar { si_value, .. }) => assert!((si_value - 0.08).abs() < 1e-10),
            other => panic!("Expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn satisfaction_content_hash_deterministic() {
        // Same variant produces same hash on repeated calls
        let h1 = Satisfaction::Satisfied.content_hash();
        let h2 = Satisfaction::Satisfied.content_hash();
        assert_eq!(h1, h2);

        let h3 = Satisfaction::Violated.content_hash();
        let h4 = Satisfaction::Violated.content_hash();
        assert_eq!(h3, h4);

        let h5 = Satisfaction::Indeterminate.content_hash();
        let h6 = Satisfaction::Indeterminate.content_hash();
        assert_eq!(h5, h6);
    }

    #[test]
    fn determinacy_state_auto_exists_and_is_distinct() {
        // Auto variant should exist and be distinct from other variants
        let auto = DeterminacyState::Auto;
        let determined = DeterminacyState::Determined;
        let undetermined = DeterminacyState::Undetermined;
        let provisional = DeterminacyState::Provisional;

        assert_ne!(auto, determined);
        assert_ne!(auto, undetermined);
        assert_ne!(auto, provisional);
    }

    #[test]
    fn determinacy_state_auto_is_copy_clone_eq_hash() {
        let auto = DeterminacyState::Auto;
        let auto2 = auto; // Copy
        assert_eq!(auto, auto2); // PartialEq + Eq

        let auto3 = auto.clone(); // Clone
        assert_eq!(auto, auto3);

        // Hash: usable as HashMap key
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(auto, "auto");
        assert_eq!(map.get(&DeterminacyState::Auto), Some(&"auto"));
    }

    #[test]
    fn determinacy_state_auto_discriminant_is_3() {
        // Determined=0, Undetermined=1, Provisional=2, Auto=3
        assert_eq!(DeterminacyState::Determined as u8, 0);
        assert_eq!(DeterminacyState::Undetermined as u8, 1);
        assert_eq!(DeterminacyState::Provisional as u8, 2);
        assert_eq!(DeterminacyState::Auto as u8, 3);
    }

    // --- Ord tests (step-1) ---

    #[test]
    fn value_ord_cross_type_ordering() {
        // Undef < Bool < Int < Real < Scalar < String
        let undef = Value::Undef;
        let bool_f = Value::Bool(false);
        let bool_t = Value::Bool(true);
        let int0 = Value::Int(0);
        let real0 = Value::Real(0.0);
        let scalar = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let string = Value::String("z".into());

        assert!(undef < bool_f);
        assert!(bool_f < bool_t);
        assert!(bool_t < int0);
        assert!(int0 < real0);
        assert!(real0 < scalar);
        assert!(scalar < string);
    }

    #[test]
    fn value_ord_within_int() {
        assert!(Value::Int(1) < Value::Int(2));
        assert!(Value::Int(-10) < Value::Int(0));
        assert_eq!(Value::Int(5).cmp(&Value::Int(5)), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_ord_within_string() {
        assert!(Value::String("a".into()) < Value::String("b".into()));
        assert!(Value::String("abc".into()) < Value::String("abd".into()));
    }

    #[test]
    fn value_ord_within_real_and_nan() {
        // Normal ordering
        assert!(Value::Real(1.0) < Value::Real(2.0));
        // NaN consistency: NaN should have a defined position (via to_bits)
        let nan = Value::Real(f64::NAN);
        let inf = Value::Real(f64::INFINITY);
        // Just verify it doesn't panic and gives consistent results
        let _ = nan.cmp(&inf);
        assert_eq!(nan.cmp(&nan), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_ord_neg_zero() {
        // -0.0 and +0.0 have different bits, so they may have different ordering
        // (consistent with PartialEq which uses to_bits)
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        // They should have a defined comparison (not panic)
        let _ = pos.cmp(&neg);
    }

    // --- Enum tests (step-3) ---

    #[test]
    fn value_enum_debug() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let dbg = format!("{:?}", v);
        assert!(dbg.contains("Color"));
        assert!(dbg.contains("Red"));
    }

    #[test]
    fn value_enum_equality() {
        let a = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let b = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let c = Value::Enum { type_name: "Color".into(), variant: "Blue".into() };
        let d = Value::Enum { type_name: "Shape".into(), variant: "Red".into() };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn value_enum_ordering() {
        let enum_val = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let string_val = Value::String("zzz".into());
        // Enum sorts after String
        assert!(enum_val > string_val);

        // Within Enum: sort by type_name then variant
        let a = Value::Enum { type_name: "Color".into(), variant: "Blue".into() };
        let b = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let c = Value::Enum { type_name: "Shape".into(), variant: "A".into() };
        assert!(a < b); // same type_name, Blue < Red
        assert!(b < c); // Color < Shape
    }

    #[test]
    fn value_enum_content_hash() {
        let a = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let b = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let c = Value::Enum { type_name: "Color".into(), variant: "Blue".into() };
        assert_eq!(a.content_hash(), b.content_hash()); // deterministic
        assert_ne!(a.content_hash(), c.content_hash()); // distinct
    }

    #[test]
    fn satisfaction_content_hash_distinct_variants() {
        let satisfied = Satisfaction::Satisfied.content_hash();
        let violated = Satisfaction::Violated.content_hash();
        let indeterminate = Satisfaction::Indeterminate.content_hash();

        assert_ne!(satisfied, violated);
        assert_ne!(satisfied, indeterminate);
        assert_ne!(violated, indeterminate);
    }
}
