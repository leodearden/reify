//! End-to-end integration tests for the appearance-substrate β seam:
//! `Material : Visual`, `resolve_color`, and `resolve_appearance`.
//!
//! PRD: docs/prds/v0_6/appearance-substrate.md §4.2/§7.3 (task β #4761).

use reify_core::Severity;
use reify_ir::Value;

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
