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
use reify_core::{DiagnosticCode, Severity};
use reify_ir::VariantPayload;

/// The shared `Shape` enum used by the construction-check tests: one
/// single-field variant (`Circle`), one two-field variant (`Rect`), and one
/// bare variant (`Point`).
const SHAPE_ENUM: &str = "\
enum Shape {
    Circle { radius: Length },
    Rect { width: Length, height: Length },
    Point,
}
";

/// Compile `source` and collect the codes of its Error-severity diagnostics
/// (used to render a helpful message when a `has_error_code` assertion fails).
fn error_codes(source: &str) -> Vec<Option<DiagnosticCode>> {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

/// True if compiling `source` yields at least one Error-severity diagnostic
/// carrying `code`.
fn has_error_code(source: &str, code: DiagnosticCode) -> bool {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

/// Build a `structure def` source whose single param `outline : Shape` defaults
/// to the given construction expression, prepended with [`SHAPE_ENUM`].
fn shape_param_source(construction: &str) -> String {
    format!("{SHAPE_ENUM}\nstructure def Widget {{\n    param outline : Shape = {construction}\n}}\n")
}

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

/// step-3 (RED): a construction that omits a declared field must emit
/// `DiagnosticCode::VariantMissingField`. `Rect` declares `width` + `height`;
/// `Rect { width: 20mm }` omits `height`.
///
/// Currently FAILS: the VariantConstruct compile arm still emits the
/// "not yet supported (task δ)" poison (no typed code).
#[test]
fn missing_field_emits_variant_missing_field() {
    let source = shape_param_source("Rect { width: 20mm }");
    assert!(
        has_error_code(&source, DiagnosticCode::VariantMissingField),
        "Rect {{ width: 20mm }} omits declared field 'height' -> expected \
         VariantMissingField; got error codes {:?}",
        error_codes(&source)
    );
}

/// step-5 (RED) case (a): a supplied field that the named-field variant does
/// not declare must emit `DiagnosticCode::VariantUnknownField`. `Circle`
/// declares `radius`; `diameter` is unknown. (`radius` is also missing, so a
/// VariantMissingField co-occurs — we specifically require the UnknownField
/// code to be present, not merely *some* error.)
///
/// Currently FAILS: only the missing-field check is implemented.
#[test]
fn unknown_field_on_named_variant_emits_variant_unknown_field() {
    let source = shape_param_source("Circle { diameter: 5mm }");
    assert!(
        has_error_code(&source, DiagnosticCode::VariantUnknownField),
        "Circle {{ diameter: 5mm }} supplies undeclared field 'diameter' -> expected \
         VariantUnknownField; got error codes {:?}",
        error_codes(&source)
    );
}

/// step-5 (RED) case (b): supplying any field to a bare/`Unit` variant must
/// emit `DiagnosticCode::VariantUnknownField` (its declared field set is empty).
///
/// Currently FAILS: a Unit variant has no declared fields, so the missing-field
/// check finds nothing and no error is raised at all.
#[test]
fn field_on_bare_variant_emits_variant_unknown_field() {
    let source = shape_param_source("Point { x: 1mm }");
    assert!(
        has_error_code(&source, DiagnosticCode::VariantUnknownField),
        "Point {{ x: 1mm }} supplies a field to a bare variant -> expected \
         VariantUnknownField; got error codes {:?}",
        error_codes(&source)
    );
}

/// step-7 (RED): a supplied field whose value type is incompatible with the
/// declared field type must emit `DiagnosticCode::VariantPayloadType`. `radius`
/// is declared `Length`; the String `"x"` mismatches. The field-set is correct
/// (radius is declared and supplied), so this isolates the payload-type check.
///
/// Currently FAILS: no payload-type check exists, so the construction raises no
/// error at all.
#[test]
fn payload_type_mismatch_emits_variant_payload_type() {
    let source = shape_param_source("Circle { radius: \"x\" }");
    assert!(
        has_error_code(&source, DiagnosticCode::VariantPayloadType),
        "Circle {{ radius: \"x\" }} supplies a String for Length field 'radius' -> \
         expected VariantPayloadType; got error codes {:?}",
        error_codes(&source)
    );
}
