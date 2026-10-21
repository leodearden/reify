//! `Value::Enum` rendering on the three formatter surfaces: `Display`,
//! `Value::format_hover` and `Value::format_display`.
//!
//! A data-carrying variant prints its named payload fields after the head,
//! in stored (declaration) order; a unit variant prints exactly its head.

use crate::Value;
use std::f64::consts::{FRAC_PI_4, FRAC_PI_6};

fn payload_enum(type_name: &str, variant: &str, payload: Vec<(&str, Value)>) -> Value {
    Value::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        payload: payload
            .into_iter()
            .map(|(field, value)| (field.to_string(), value))
            .collect(),
    }
}

fn assert_renders(value: &Value, display: &str, hover: &str, gui: &str) {
    assert_eq!(value.to_string(), display, "Display of {value:?}");
    assert_eq!(value.format_hover(), hover, "format_hover of {value:?}");
    assert_eq!(value.format_display(), gui, "format_display of {value:?}");
}

#[test]
fn single_field_variant_prints_its_payload() {
    let revolute = payload_enum(
        "JointPosition",
        "RevolutePosition",
        vec![("angle", Value::angle(FRAC_PI_4))],
    );
    assert_renders(
        &revolute,
        "JointPosition::RevolutePosition { angle: 0.7853981633974483 rad }",
        "JointPosition::RevolutePosition { angle: 0.7853981633974483 rad }",
        "RevolutePosition { angle: 45 }",
    );
}

#[test]
fn multi_field_variant_prints_fields_in_stored_order() {
    let planar = payload_enum(
        "JointPosition",
        "PlanarPosition",
        vec![
            ("x", Value::length(0.001)),
            ("y", Value::length(0.002)),
            ("angle", Value::angle(FRAC_PI_6)),
        ],
    );
    assert_renders(
        &planar,
        "JointPosition::PlanarPosition { x: 0.001 m, y: 0.002 m, angle: 0.5235987755982989 rad }",
        "JointPosition::PlanarPosition { x: 0.001 m, y: 0.002 m, angle: 0.5235987755982989 rad }",
        "PlanarPosition { x: 1, y: 2, angle: 30 }",
    );
}

#[test]
fn nested_payload_enum_renders_recursively() {
    let leaf = |meters: f64| payload_enum("Tree", "Leaf", vec![("value", Value::length(meters))]);
    let node = payload_enum(
        "Tree",
        "Node",
        vec![("left", leaf(0.001)), ("right", leaf(0.002))],
    );
    assert_renders(
        &node,
        "Tree::Node { left: Tree::Leaf { value: 0.001 m }, right: Tree::Leaf { value: 0.002 m } }",
        "Tree::Node { left: Tree::Leaf { value: 0.001 m }, right: Tree::Leaf { value: 0.002 m } }",
        "Node { left: Leaf { value: 1 }, right: Leaf { value: 2 } }",
    );
}

#[test]
fn undef_payload_field_renders_with_each_formatters_undef_spelling() {
    let circle = payload_enum("Shape", "Circle", vec![("radius", Value::Undef)]);
    assert_renders(
        &circle,
        "Shape::Circle { radius: undef }",
        "Shape::Circle { radius: (undefined) }",
        "Circle { radius: undefined }",
    );
}

#[test]
fn unit_variant_prints_exactly_its_head() {
    assert_renders(
        &Value::enum_unit("Color", "Red"),
        "Color::Red",
        "Color::Red",
        "Red",
    );
}
