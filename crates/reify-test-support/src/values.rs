use std::collections::HashMap;

use reify_types::{
    dimension::{DimensionVector, FORCE},
    ConstraintNodeId, DeterminacyState, Value, ValueCellId,
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use reify_types::DeterminacyState;

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
            SnapshotValuesBuilder::new()
                .auto_val(id.clone())
                .build();
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
        let (val, state) = provisional(Value::Real(3.14));
        assert_eq!(val, Value::Real(3.14));
        assert_eq!(state, DeterminacyState::Provisional);
    }

    #[test]
    fn auto_state_returns_undef_and_auto() {
        let (val, state) = auto_state();
        assert_eq!(val, Value::Undef);
        assert_eq!(state, DeterminacyState::Auto);
    }
}
