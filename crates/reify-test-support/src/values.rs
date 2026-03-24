use reify_types::{
    dimension::{DimensionVector, FORCE},
    ConstraintNodeId, Value, ValueCellId,
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
pub fn determined(value: Value) -> (Value, reify_types::DeterminacyState) {
    (value, reify_types::DeterminacyState::Determined)
}

/// Return a (Value::Undef, DeterminacyState::Undetermined) tuple.
pub fn undetermined() -> (Value, reify_types::DeterminacyState) {
    (Value::Undef, reify_types::DeterminacyState::Undetermined)
}

/// Return a (Value, DeterminacyState::Provisional) tuple.
pub fn provisional(value: Value) -> (Value, reify_types::DeterminacyState) {
    (value, reify_types::DeterminacyState::Provisional)
}

/// Return a (Value::Undef, DeterminacyState::Auto) tuple.
pub fn auto_state() -> (Value, reify_types::DeterminacyState) {
    (Value::Undef, reify_types::DeterminacyState::Auto)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DeterminacyState;

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
