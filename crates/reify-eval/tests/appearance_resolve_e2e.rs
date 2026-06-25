//! End-to-end integration tests for the appearance-substrate β seam:
//! `Material : Visual`, `resolve_color`, and `resolve_appearance`.
//!
//! PRD: docs/prds/v0_6/appearance-substrate.md §4.2/§7.3 (task β #4761).

use reify_core::Severity;
use reify_eval::appearance::{resolve_appearance, resolve_color};
use reify_ir::{PersistentMap, Rgb8, StructureInstanceData, StructureTypeId, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

fn struct_field(val: &Value, key: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
        _ => None,
    }
}

// ── B2: Material back-compat ─────────────────────────────────────────────────

/// B2 — `Material(name:, density:, youngs_modulus:)` constructor with NO
/// `appearance` argument:
///  (a) zero error-severity diagnostics and zero "unresolved" messages;
///  (b) the evaluated Material StructureInstance has a populated `appearance`
///      field that is itself an `Appearance` StructureInstance (not Undef) —
///      the ctor-default `Appearance()` was evaluated and filled in by S2.
///
/// Unit literals must be written without a space before the unit symbol
/// (e.g. `7850kg/m^3`, not `7850 kg/m^3`) — this is a Reify parser invariant.
///
/// Fails until S2 adds `Material : Visual` + `param appearance : Appearance = Appearance()`.
#[test]
fn material_ctor_without_appearance_populates_default_appearance() {
    let source = r#"
structure def TestBody {
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
    let mat_appearance = material.appearance
}
"#;

    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = reify_test_support::make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics and no "unresolved" messages.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );
    let unresolved: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolved type") || d.message.contains("unresolved name"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "expected no 'unresolved' diagnostics, got: {:#?}",
        unresolved
    );

    // (b) Material StructureInstance has a populated `appearance` field.
    let material_cell = reify_core::ValueCellId::new("TestBody", "material");
    let material_val = eval_result
        .values
        .get(&material_cell)
        .unwrap_or_else(|| panic!("cell TestBody.material not found in eval result"));

    let appearance = struct_field(material_val, "appearance")
        .unwrap_or_else(|| panic!("Material must have an `appearance` field (added by task β S2)"));

    match &appearance {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Appearance",
                "appearance field must be an Appearance StructureInstance, got type_name={:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Appearance StructureInstance for material.appearance, got {:?}",
            other
        ),
    }
}

// ── B5: resolve_appearance e2e ────────────────────────────────────────────────

/// Build a synthetic body `Value::StructureInstance` wrapping the given material.
/// This lets `resolve_appearance` navigate `body.material.appearance` against a
/// real stdlib-evaluated Material value.
fn make_body_with_material(material: Value) -> Value {
    let fields: PersistentMap<String, Value> =
        [("material".to_string(), material)].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "SyntheticBody".to_string(),
        version: 1,
        fields,
    }))
}

/// B5 (e2e) — resolve_appearance + resolve_color on real stdlib-evaluated Materials.
///
/// Two assertions:
/// (1) Styled body: Material with explicit `appearance: Appearance(color: Color(r:0.4,g:0.4,b:0.42))`
///     → resolve_appearance navigates `material.appearance`; resolve_color maps to Rgb8{102,102,107}.
/// (2) Anti-drift: plain Material (stdlib default `Appearance()`) and a body with NO
///     material field (hand-minted `neutral_appearance()` path) must resolve to the same
///     neutral grey, tying `materials_appearance.ri` ↔ Rust `neutral_appearance()`.
///
/// Fails until S8 introduces `resolve_appearance` / `neutral_appearance`.
#[test]
fn resolve_appearance_e2e_styled_and_plain_bodies() {
    let source = r#"
structure def StyledBody {
    param material : Material = Material(name: "styled", density: 7850kg/m^3, youngs_modulus: 200GPa, appearance: Appearance(color: Color(r:0.4, g:0.4, b:0.42)))
}
structure def PlainBody {
    param material : Material = Material(name: "plain", density: 7850kg/m^3, youngs_modulus: 200GPa)
}
"#;
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = reify_test_support::make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Zero errors.
    let errors: Vec<_> =
        eval_result.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "expected no errors, got: {errors:#?}");

    let styled_material = eval_result
        .values
        .get(&reify_core::ValueCellId::new("StyledBody", "material"))
        .cloned()
        .expect("StyledBody.material must be in eval result");
    let plain_material = eval_result
        .values
        .get(&reify_core::ValueCellId::new("PlainBody", "material"))
        .cloned()
        .expect("PlainBody.material must be in eval result");

    // (1) Styled body: resolve_appearance traverses body.material.appearance → explicit color.
    let styled_body = make_body_with_material(styled_material);
    let styled_app = resolve_appearance(&styled_body);
    let styled_color =
        struct_field(&styled_app, "color").expect("Appearance must have a `color` field");
    let mut diags: Vec<reify_core::Diagnostic> = Vec::new();
    let rgb = resolve_color(&styled_color, &mut diags);
    assert_eq!(rgb, Rgb8 { r: 102, g: 102, b: 107 }, "r:0.4→102; g:0.4→102; b:0.42→107");
    assert!(diags.is_empty(), "no diags expected for styled color, got: {diags:#?}");

    // (2) Anti-drift: plain body (stdlib Appearance() default, r=g=b=0.7) must resolve
    //     to the same neutral grey as the hand-minted neutral_appearance() fallback path.
    let plain_body = make_body_with_material(plain_material);
    let plain_app = resolve_appearance(&plain_body);
    let plain_color =
        struct_field(&plain_app, "color").expect("plain Appearance must have a `color` field");
    let mut plain_diags: Vec<reify_core::Diagnostic> = Vec::new();
    let plain_rgb = resolve_color(&plain_color, &mut plain_diags);
    assert!(plain_diags.is_empty(), "no diags expected for plain color, got: {plain_diags:#?}");

    // Hand-minted fallback path: a body with NO material field → neutral_appearance().
    let no_material_body = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "NoMaterialBody".to_string(),
        version: 1,
        fields: PersistentMap::new(),
    }));
    let neutral_app = resolve_appearance(&no_material_body);
    let neutral_color =
        struct_field(&neutral_app, "color").expect("neutral Appearance must have a `color` field");
    let mut neutral_diags: Vec<reify_core::Diagnostic> = Vec::new();
    let neutral_rgb = resolve_color(&neutral_color, &mut neutral_diags);
    assert!(
        neutral_diags.is_empty(),
        "no diags expected for neutral fallback, got: {neutral_diags:#?}"
    );

    // Anti-drift: both must agree. If they diverge, materials_appearance.ri and
    // neutral_appearance() have drifted (e.g. the .ri default r:0.7 was changed
    // but the Rust hard-coded fallback was not updated).
    assert_eq!(
        plain_rgb, neutral_rgb,
        "stdlib Appearance() default color must equal hand-minted neutral_appearance() color \
         (anti-drift guard: ties materials_appearance.ri default ↔ Rust neutral_appearance)"
    );
}
