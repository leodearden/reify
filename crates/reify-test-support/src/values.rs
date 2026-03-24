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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn int_val_produces_int() {
        let v = int_val(42);
        assert!(matches!(v, Value::Int(42)));
    }

    #[test]
    fn real_val_produces_real() {
        let v = real_val(3.14);
        assert!(matches!(v, Value::Real(_)));
        if let Value::Real(f) = v {
            assert!((f - 3.14).abs() < 1e-10);
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
        assert!(matches!(v, Value::Enum { ref type_name, ref variant } if type_name == "Color" && variant == "Red"));
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
}
