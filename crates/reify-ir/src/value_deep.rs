use crate::value::Value;

impl Value {
    /// True when an `Undef` appears anywhere in the value, not only at the top
    /// level: the deep sibling of [`Value::is_undef`].
    pub fn contains_undef(&self) -> bool {
        self.any_deep(&Value::is_undef)
    }

    /// True when `pred` holds for this value or for any value nested in it, at
    /// any depth: collection elements, map keys and values, enum payloads,
    /// composite components, range bounds, structure-instance fields, a
    /// field's lambda and a lambda's captures.
    pub fn any_deep(&self, pred: &dyn Fn(&Value) -> bool) -> bool {
        if pred(self) {
            return true;
        }
        let deep = |value: &Value| value.any_deep(pred);
        match self {
            Value::List(items)
            | Value::Tensor(items)
            | Value::Point(items)
            | Value::Vector(items) => items.iter().any(deep),
            Value::Set(items) => items.iter().any(deep),
            Value::Matrix(rows) => rows.iter().flatten().any(deep),
            Value::Map(entries) => entries.iter().any(|(key, value)| deep(key) || deep(value)),
            Value::Option(inner) => inner.as_deref().is_some_and(deep),
            Value::Enum { payload, .. } => payload.iter().any(|(_, value)| deep(value)),
            Value::Frame {
                origin: a,
                basis: b,
            }
            | Value::Transform {
                rotation: a,
                translation: b,
            }
            | Value::Plane {
                origin: a,
                normal: b,
            }
            | Value::Axis {
                origin: a,
                direction: b,
            }
            | Value::BoundingBox { min: a, max: b } => deep(a) || deep(b),
            Value::Range { lower, upper, .. } => [lower, upper]
                .into_iter()
                .any(|bound| bound.as_deref().is_some_and(deep)),
            Value::StructureInstance(data) => data.fields.iter().any(|(_, value)| deep(value)),
            Value::Field { lambda, .. } => deep(lambda),
            Value::Lambda { captures, .. } => captures.iter().any(|(_, value)| deep(value)),
            Value::Undef
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Real(_)
            | Value::String(_)
            | Value::Scalar { .. }
            | Value::Complex { .. }
            | Value::Orientation { .. }
            | Value::Direction(_)
            | Value::AffineMap { .. }
            | Value::GeometryHandle { .. }
            | Value::Selector(_)
            | Value::Feature(_)
            | Value::SampledField(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::persistent::PersistentMap;
    use crate::structure_type_id::StructureTypeId;
    use crate::value::{StructureInstanceData, Value};
    use reify_core::identity::RealizationNodeId;

    fn instance(pairs: &[(&str, Value)]) -> Value {
        let fields: PersistentMap<String, Value> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Beam".to_string(),
            version: 1,
            fields,
        }))
    }

    fn identity_orientation() -> Value {
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    #[test]
    fn undef_itself_contains_undef() {
        assert!(Value::Undef.contains_undef());
    }

    #[test]
    fn leaf_values_do_not_contain_undef() {
        let leaves = [
            Value::length(1.0),
            Value::Int(3),
            Value::String("tip".to_string()),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Beam", 0),
                upstream_values_hash: [7u8; 32],
                kernel_handle: None,
            },
        ];
        for leaf in leaves {
            assert!(!leaf.contains_undef(), "{leaf:?} holds no Undef");
        }
    }

    #[test]
    fn list_with_an_undef_element_contains_undef() {
        assert!(Value::List(vec![Value::Int(1), Value::Undef]).contains_undef());
    }

    #[test]
    fn nested_list_reaches_an_inner_undef() {
        let nested = Value::List(vec![Value::List(vec![Value::Undef])]);
        assert!(nested.contains_undef());
    }

    #[test]
    fn fully_defined_list_does_not_contain_undef() {
        assert!(!Value::List(vec![Value::Int(1), Value::Int(2)]).contains_undef());
    }

    #[test]
    fn option_holding_undef_contains_undef() {
        assert!(Value::Option(Some(Box::new(Value::Undef))).contains_undef());
    }

    #[test]
    fn point_with_an_undef_coordinate_contains_undef() {
        assert!(Value::Point(vec![Value::length(1.0), Value::Undef]).contains_undef());
    }

    #[test]
    fn range_with_an_undef_lower_bound_contains_undef() {
        let range = Value::range(Some(Value::Undef), Some(Value::length(2.0)), true, true);
        assert!(range.contains_undef());
    }

    #[test]
    fn enum_payload_holding_undef_contains_undef() {
        let variant = Value::Enum {
            type_name: "Support".to_string(),
            variant: "Pinned".to_string(),
            payload: vec![("offset".to_string(), Value::Undef)],
        };
        assert!(variant.contains_undef());
    }

    #[test]
    fn structure_instance_with_an_undef_field_contains_undef() {
        let partial = instance(&[("len", Value::length(1.0)), ("tip", Value::Undef)]);
        assert!(partial.contains_undef());
    }

    #[test]
    fn fully_defined_structure_instance_does_not_contain_undef() {
        let complete = instance(&[("len", Value::length(1.0)), ("count", Value::Int(2))]);
        assert!(!complete.contains_undef());
    }

    #[test]
    fn frame_with_an_undef_origin_contains_undef() {
        let frame = Value::Frame {
            origin: Box::new(Value::Undef),
            basis: Box::new(identity_orientation()),
        };
        assert!(frame.contains_undef());
    }

    #[test]
    fn any_deep_finds_a_geometry_handle_nested_in_an_instance_field() {
        let handle = Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Part", 0),
            upstream_values_hash: [7u8; 32],
            kernel_handle: None,
        };
        let holder = instance(&[("target", handle.clone()), ("n", Value::Int(1))]);
        let wrapped = Value::List(vec![holder]);
        assert!(wrapped.any_deep(&|v| *v == handle));
        assert!(!wrapped.any_deep(&|v| matches!(v, Value::Selector(_))));
    }
}
