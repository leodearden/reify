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

// ── B6: FEA library material editorial appearances ────────────────────────────

/// B6 — Each of the four FEA library materials carries an editorial
/// `appearance : Appearance` member (task γ, #4762).
///
/// Asserts for each material:
///   - The `appearance` field is a non-Undef `Value::StructureInstance` with
///     `type_name == "Appearance"`.
///   - Its `color` field is a non-Undef `Value::StructureInstance` with
///     `type_name == "Color"`.
///   - `color.named` is `Value::String("")` (γ uses explicit r/g/b only;
///     `named` defaults to `""`).
///   - `color.r / .g / .b` round-trip to the characteristic editorial values
///     within 1e-9 (reading the declared Real literals directly, not via
///     `resolve_color`, which avoids float→u8 rounding ambiguity; egress is δ).
///   - `appearance.finish` is the expected `Finish` enum variant.
///   - `appearance.metalness` and `.roughness` round-trip within 1e-9.
///
/// Editorial values (PRD §4.3 / decision 5 — NOT physically derived):
///   Steel   (0.50, 0.50, 0.52) Finish.Satin  metalness 0.90 roughness 0.40
///   Al      (0.66, 0.67, 0.69) Finish.Satin  metalness 0.90 roughness 0.45
///   Ti      (0.55, 0.54, 0.53) Finish.Satin  metalness 0.85 roughness 0.45
///   ABS     (0.92, 0.91, 0.88) Finish.Matte  metalness 0.0  roughness 0.85
///
/// Fails until step-4 (impl) adds `: ElasticMaterial + Visual` and the
/// editorial `appearance` param to each material in `materials_fea.ri`.
#[test]
fn fea_library_materials_carry_editorial_appearance() {
    let source = r#"
structure def LibAppearance {
    let steel = Steel_AISI_1045()
    let al    = Aluminium_6061_T6()
    let ti    = Titanium_Ti6Al4V()
    let abs   = ABS_Plastic()
}
"#;
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = reify_test_support::make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Zero Error-severity diagnostics.
    let errors: Vec<_> =
        eval_result.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "expected no Error diagnostics, got: {errors:#?}");

    // Helper: extract an f64 from Real / Scalar / Int; panics on mismatch.
    let extract_real = |val: Option<Value>, field: &str| -> f64 {
        match val {
            Some(Value::Real(x)) => x,
            Some(Value::Scalar { si_value, .. }) => si_value,
            Some(Value::Int(n)) => n as f64,
            None => panic!("field `{field}` not found"),
            Some(other) => panic!("expected numeric for `{field}`, got {other:?}"),
        }
    };

    // All four materials are fields of the same top-level structure.
    const STRUCT: &str = "LibAppearance";

    // Characteristic editorial values per material:
    //   (cell_name, r, g, b, finish_variant, metalness, roughness)
    let cases: &[(&str, f64, f64, f64, &str, f64, f64)] = &[
        ("steel", 0.50, 0.50, 0.52, "Satin", 0.90, 0.40),
        ("al",    0.66, 0.67, 0.69, "Satin", 0.90, 0.45),
        ("ti",    0.55, 0.54, 0.53, "Satin", 0.85, 0.45),
        ("abs",   0.92, 0.91, 0.88, "Matte", 0.0,  0.85),
    ];

    for &(cell_name, exp_r, exp_g, exp_b, exp_finish, exp_metalness, exp_roughness) in cases {
        let cell_id = reify_core::ValueCellId::new(STRUCT, cell_name);
        let material_val = eval_result
            .values
            .get(&cell_id)
            .unwrap_or_else(|| panic!("{STRUCT}.{cell_name} not found in eval result"));

        // material.appearance must be a non-Undef StructureInstance of type "Appearance".
        let appearance = struct_field(material_val, "appearance").unwrap_or_else(|| {
            panic!("{STRUCT}.{cell_name} must have an `appearance` field (task γ #4762)")
        });
        let app_data = match &appearance {
            Value::StructureInstance(data) => data,
            other => panic!(
                "{STRUCT}.{cell_name}.appearance should be StructureInstance, got {other:?}"
            ),
        };
        assert_eq!(
            app_data.type_name, "Appearance",
            "{STRUCT}.{cell_name}.appearance type_name should be \"Appearance\", got {:?}",
            app_data.type_name
        );

        // appearance.color must be a non-Undef StructureInstance of type "Color".
        let color = struct_field(&appearance, "color").unwrap_or_else(|| {
            panic!("{STRUCT}.{cell_name}.appearance must have a `color` field")
        });
        let color_data = match &color {
            Value::StructureInstance(data) => data,
            other => panic!(
                "{STRUCT}.{cell_name}.appearance.color should be StructureInstance, got {other:?}"
            ),
        };
        assert_eq!(
            color_data.type_name, "Color",
            "{STRUCT}.{cell_name}.appearance.color type_name should be \"Color\", got {:?}",
            color_data.type_name
        );

        // color.named must be String("") — γ uses explicit r/g/b, no named color.
        let named = struct_field(&color, "named").unwrap_or_else(|| {
            panic!("{STRUCT}.{cell_name}.appearance.color must have a `named` field")
        });
        assert_eq!(
            named,
            Value::String("".to_string()),
            "{STRUCT}.{cell_name}.appearance.color.named should be \"\", got {named:?}"
        );

        // color.r / .g / .b must round-trip to the editorial values within 1e-9.
        let r = extract_real(struct_field(&color, "r"), "r");
        let g = extract_real(struct_field(&color, "g"), "g");
        let b = extract_real(struct_field(&color, "b"), "b");
        assert!(
            (r - exp_r).abs() < 1e-9,
            "{STRUCT}.{cell_name}.appearance.color.r: expected {exp_r}, got {r}"
        );
        assert!(
            (g - exp_g).abs() < 1e-9,
            "{STRUCT}.{cell_name}.appearance.color.g: expected {exp_g}, got {g}"
        );
        assert!(
            (b - exp_b).abs() < 1e-9,
            "{STRUCT}.{cell_name}.appearance.color.b: expected {exp_b}, got {b}"
        );

        // appearance.finish must be the expected Finish enum variant.
        let finish = struct_field(&appearance, "finish").unwrap_or_else(|| {
            panic!("{STRUCT}.{cell_name}.appearance must have a `finish` field")
        });
        match &finish {
            Value::Enum { variant, .. } => {
                assert_eq!(
                    variant, exp_finish,
                    "{STRUCT}.{cell_name}.appearance.finish: expected {exp_finish:?}, got {variant:?}"
                );
            }
            other => panic!(
                "{STRUCT}.{cell_name}.appearance.finish should be an Enum, got {other:?}"
            ),
        }

        // appearance.metalness / .roughness must round-trip within 1e-9.
        let metalness = extract_real(struct_field(&appearance, "metalness"), "metalness");
        let roughness = extract_real(struct_field(&appearance, "roughness"), "roughness");
        assert!(
            (metalness - exp_metalness).abs() < 1e-9,
            "{STRUCT}.{cell_name}.appearance.metalness: expected {exp_metalness}, got {metalness}"
        );
        assert!(
            (roughness - exp_roughness).abs() < 1e-9,
            "{STRUCT}.{cell_name}.appearance.roughness: expected {exp_roughness}, got {roughness}"
        );
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

    // Widen the anti-drift guard to finish/metalness/roughness, not just color.
    // neutral_appearance() sets these to match the .ri defaults (finish=Satin,
    // metalness=0.0, roughness=0.5); assert the stdlib-evaluated Appearance() agrees.
    // If the .ri defaults change, neutral_appearance() must be updated in tandem.

    // finish — enum variant must agree.
    let plain_finish = struct_field(&plain_app, "finish")
        .expect("plain Appearance must have a `finish` field");
    let neutral_finish = struct_field(&neutral_app, "finish")
        .expect("neutral Appearance must have a `finish` field");
    match (plain_finish, neutral_finish) {
        (Value::Enum { variant: pv, .. }, Value::Enum { variant: nv, .. }) => {
            assert_eq!(
                pv, nv,
                "stdlib Appearance() finish must equal neutral_appearance() finish (anti-drift)"
            );
        }
        (pf, nf) => {
            panic!("expected Enum for finish: plain={pf:?} neutral={nf:?}")
        }
    }

    // Helper: extract an f64 from Real / Scalar / Int; panics on mismatch.
    let extract_real = |val: Option<Value>, field: &str| -> f64 {
        match val {
            Some(Value::Real(x)) => x,
            Some(Value::Scalar { si_value, .. }) => si_value,
            Some(Value::Int(n)) => n as f64,
            None => panic!("field `{field}` not found in Appearance"),
            Some(other) => panic!("expected numeric for `{field}`, got {other:?}"),
        }
    };

    // metalness — dimensionless Real; must agree to machine precision.
    let plain_metalness = extract_real(struct_field(&plain_app, "metalness"), "metalness");
    let neutral_metalness = extract_real(struct_field(&neutral_app, "metalness"), "metalness");
    assert!(
        (plain_metalness - neutral_metalness).abs() < f64::EPSILON,
        "stdlib metalness {plain_metalness} ≠ neutral metalness {neutral_metalness} (anti-drift)"
    );

    // roughness — dimensionless Real; must agree to machine precision.
    let plain_roughness = extract_real(struct_field(&plain_app, "roughness"), "roughness");
    let neutral_roughness = extract_real(struct_field(&neutral_app, "roughness"), "roughness");
    assert!(
        (plain_roughness - neutral_roughness).abs() < f64::EPSILON,
        "stdlib roughness {plain_roughness} ≠ neutral roughness {neutral_roughness} (anti-drift)"
    );
}
