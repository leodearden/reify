//! Compiler-side checks for named-field enum-variant construction (task δ #3942).
//!
//! Drives the producer/compiler side of data-carrying enums:
//!   1. IR payload resolution — `module.enum_defs` carries resolved
//!      `VariantPayload::Named` field types (steps 1-2).
//!   2. Field-set + type checking of `Variant { ... }` construction
//!      expressions (steps 3-10): VariantMissingField / VariantUnknownField /
//!      VariantPayloadType.
//!
//! Diagnostic assertions match on `Diagnostic.code` (typed `DiagnosticCode`)
//! rather than message substrings, per the codebase convention
//! (reify-core/src/diagnostics.rs).

mod common;

use common::compile_with_stdlib_helper;
use reify_ir::VariantPayload;

/// Field names (in declaration order) of a `Named` payload.
///
/// Panics with a descriptive message if the payload is `Unit` — used by the
/// payload-shape assertions so a regression that drops the named-field payload
/// reports which variant lost its fields.
fn named_field_names<'a>(payload: &'a VariantPayload, variant: &str) -> Vec<&'a str> {
    match payload {
        VariantPayload::Named(fields) => fields.iter().map(|(n, _)| n.as_str()).collect(),
        VariantPayload::Unit => {
            panic!("variant '{}' expected a Named payload, got Unit", variant)
        }
    }
}

/// step-1 (RED): the resolved IR `module.enum_defs` must carry each variant's
/// named-field payload (field names, in declaration order) — not collapse every
/// variant to `VariantPayload::Unit`.
///
/// Currently FAILS: `compile_builder/pre_pass.rs` maps every AST variant to
/// `EnumVariantDef::unit`, dropping the named-field payload.
#[test]
fn enum_defs_carry_resolved_named_field_payloads() {
    let source = "\
enum Shape {
    Circle { radius: Length },
    Rect { width: Length, height: Length },
    Point,
}
";
    let module = compile_with_stdlib_helper(source);
    let shape = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Shape")
        .expect("Shape enum should be present in module.enum_defs");

    // Look up each variant by name (do not assume ordering within enum_defs).
    let variant = |name: &str| {
        shape
            .variants
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("variant '{}' not found on Shape", name))
    };

    // Circle { radius: Length } -> Named(["radius"])
    assert_eq!(
        named_field_names(&variant("Circle").payload, "Circle"),
        ["radius"],
        "Circle must carry a single named field 'radius'"
    );

    // Rect { width, height } -> Named(["width", "height"]) in DECLARATION order
    assert_eq!(
        named_field_names(&variant("Rect").payload, "Rect"),
        ["width", "height"],
        "Rect must carry named fields [width, height] in declaration order"
    );

    // Point -> Unit (bare)
    assert_eq!(
        variant("Point").payload,
        VariantPayload::Unit,
        "Point must carry a Unit (bare) payload"
    );
}
