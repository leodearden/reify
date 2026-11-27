use std::sync::Arc;

use crate::value::{Value, ValueMap};

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

    /// This value with every value `rewrite` maps replaced by its image, at any
    /// depth: the rebuilding sibling of [`Value::any_deep`], over the same
    /// children. A value `rewrite` maps is replaced whole, not descended into.
    pub fn map_deep(&self, rewrite: &dyn Fn(&Value) -> Option<Value>) -> Value {
        if let Some(image) = rewrite(self) {
            return image;
        }
        let deep = |value: &Value| value.map_deep(rewrite);
        let boxed = |value: &Value| Box::new(deep(value));
        let all = |items: &[Value]| items.iter().map(deep).collect::<Vec<_>>();
        match self {
            Value::List(items) => Value::List(all(items)),
            Value::Tensor(items) => Value::Tensor(all(items)),
            Value::Point(items) => Value::Point(all(items)),
            Value::Vector(items) => Value::Vector(all(items)),
            Value::Set(items) => Value::Set(items.iter().map(deep).collect()),
            Value::Matrix(rows) => Value::Matrix(rows.iter().map(|row| all(row)).collect()),
            Value::Map(entries) => Value::Map(
                entries
                    .iter()
                    .map(|(key, value)| (deep(key), deep(value)))
                    .collect(),
            ),
            Value::Option(inner) => Value::Option(inner.as_deref().map(boxed)),
            Value::Enum {
                type_name,
                variant,
                payload,
            } => Value::Enum {
                type_name: type_name.clone(),
                variant: variant.clone(),
                payload: payload
                    .iter()
                    .map(|(name, value)| (name.clone(), deep(value)))
                    .collect(),
            },
            Value::Frame { origin, basis } => Value::Frame {
                origin: boxed(origin),
                basis: boxed(basis),
            },
            Value::Transform {
                rotation,
                translation,
            } => Value::Transform {
                rotation: boxed(rotation),
                translation: boxed(translation),
            },
            Value::Plane { origin, normal } => Value::Plane {
                origin: boxed(origin),
                normal: boxed(normal),
            },
            Value::Axis { origin, direction } => Value::Axis {
                origin: boxed(origin),
                direction: boxed(direction),
            },
            Value::BoundingBox { min, max } => Value::BoundingBox {
                min: boxed(min),
                max: boxed(max),
            },
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => Value::Range {
                lower: lower.as_deref().map(boxed),
                upper: upper.as_deref().map(boxed),
                lower_inclusive: *lower_inclusive,
                upper_inclusive: *upper_inclusive,
            },
            Value::StructureInstance(data) => {
                let fields = data
                    .fields
                    .iter()
                    .map(|(name, value)| (name.clone(), deep(value)))
                    .collect();
                let mut data = data.clone();
                data.fields = fields;
                Value::StructureInstance(data)
            }
            Value::Field {
                domain_type,
                codomain_type,
                source,
                lambda,
            } => Value::Field {
                domain_type: domain_type.clone(),
                codomain_type: codomain_type.clone(),
                source: source.clone(),
                lambda: Arc::new(deep(lambda)),
            },
            Value::Lambda {
                params,
                body,
                captures,
            } => {
                let mut mapped = ValueMap::new();
                for (id, value) in captures.iter() {
                    mapped.insert(id.clone(), deep(value));
                }
                Value::Lambda {
                    params: params.clone(),
                    body: body.clone(),
                    captures: mapped,
                }
            }
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
            | Value::SampledField(_) => self.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use crate::expr::CompiledExpr;
    use crate::persistent::PersistentMap;
    use crate::structure_type_id::StructureTypeId;
    use crate::value::{FieldSourceKind, StructureInstanceData, Value, ValueMap};
    use reify_core::identity::RealizationNodeId;
    use reify_core::{Type, ValueCellId};

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

    fn handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Part", 0),
            upstream_values_hash: [7u8; 32],
            kernel_handle: None,
        }
    }

    /// One value per child slot `any_deep` descends, each holding `target` there.
    fn containers_holding(target: &Value) -> Vec<(&'static str, Value)> {
        let t = || target.clone();
        let boxed = || Box::new(t());
        let mut captures = ValueMap::new();
        captures.insert(ValueCellId::new("Part", "body"), t());
        vec![
            ("list", Value::List(vec![Value::Int(1), t()])),
            ("tensor", Value::Tensor(vec![t()])),
            ("point", Value::Point(vec![t()])),
            ("vector", Value::Vector(vec![t()])),
            ("set", Value::Set(BTreeSet::from([t()]))),
            (
                "matrix",
                Value::Matrix(vec![vec![Value::Int(1)], vec![t()]]),
            ),
            (
                "map key",
                Value::Map(BTreeMap::from([(t(), Value::Int(1))])),
            ),
            (
                "map value",
                Value::Map(BTreeMap::from([(Value::Int(1), t())])),
            ),
            ("option", Value::Option(Some(boxed()))),
            (
                "enum payload",
                Value::Enum {
                    type_name: "Support".to_string(),
                    variant: "Pinned".to_string(),
                    payload: vec![("at".to_string(), t())],
                },
            ),
            (
                "frame",
                Value::Frame {
                    origin: boxed(),
                    basis: Box::new(identity_orientation()),
                },
            ),
            (
                "transform",
                Value::Transform {
                    rotation: Box::new(identity_orientation()),
                    translation: boxed(),
                },
            ),
            (
                "plane",
                Value::Plane {
                    origin: Box::new(Value::Int(0)),
                    normal: boxed(),
                },
            ),
            (
                "axis",
                Value::Axis {
                    origin: boxed(),
                    direction: Box::new(Value::Int(0)),
                },
            ),
            (
                "bounding box",
                Value::BoundingBox {
                    min: Box::new(Value::Int(0)),
                    max: boxed(),
                },
            ),
            ("range lower", Value::range(Some(t()), None, true, true)),
            ("range upper", Value::range(None, Some(t()), true, true)),
            ("structure field", instance(&[("target", t())])),
            (
                "field lambda",
                Value::Field {
                    domain_type: Type::length(),
                    codomain_type: Type::length(),
                    source: FieldSourceKind::Analytical,
                    lambda: Arc::new(t()),
                },
            ),
            (
                "lambda capture",
                Value::Lambda {
                    params: vec![],
                    body: Box::new(CompiledExpr::literal(Value::Int(0), Type::Int)),
                    captures,
                },
            ),
        ]
    }

    #[test]
    fn map_deep_rewrites_every_child_any_deep_reaches() {
        let target = handle();
        for (slot, container) in containers_holding(&target) {
            assert!(
                container.any_deep(&|v| *v == target),
                "{slot}: any_deep must reach the target"
            );
            let rewritten = container.map_deep(&|v| (*v == target).then_some(Value::Undef));
            assert!(
                !rewritten.any_deep(&|v| *v == target),
                "{slot}: map_deep must rewrite every child any_deep reaches: {rewritten:?}"
            );
            assert!(
                rewritten.contains_undef(),
                "{slot}: the target's image takes its place: {rewritten:?}"
            );
        }
    }

    #[test]
    fn map_deep_keeps_the_siblings_of_a_rewritten_value() {
        let target = handle();
        let holder = instance(&[("target", target.clone()), ("n", Value::Int(1))]);
        let expected = instance(&[("target", Value::Undef), ("n", Value::Int(1))]);
        assert_eq!(
            holder.map_deep(&|v| (*v == target).then_some(Value::Undef)),
            expected
        );
    }

    #[test]
    fn map_deep_with_no_match_returns_an_equal_value() {
        let nested = Value::List(vec![
            instance(&[("len", Value::length(1.0))]),
            Value::Option(Some(Box::new(Value::Int(2)))),
        ]);
        assert_eq!(nested.map_deep(&|_| None), nested);
    }
}
