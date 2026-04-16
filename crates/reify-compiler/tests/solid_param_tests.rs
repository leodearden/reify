//! Tests for `param x : Solid = <geometry_call>` compilation.
//!
//! A `Solid`-typed param with a geometry-call default should be lowered as a
//! realization (like a geometry let) rather than a scalar ValueCellDecl.

use reify_compiler::TopologyTemplate;
use reify_types::Severity;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_solid_param"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:#?}",
        errors
    );
    compiled
}

// ─── step-3: Solid-typed param should lower to a realization ─────────────────

/// `param g : Solid = cylinder(10mm, 20mm)` must:
/// (a) compile without errors,
/// (b) produce NO ValueCellDecl named `g`,
/// (c) produce exactly 1 RealizationDecl,
/// (d) register `g` as Type::Geometry (verified indirectly: the cell_type of
///     any value cell named `g` must not exist, since the param is a realization).
#[test]
fn solid_param_compiles_as_realization() {
    let source = r#"structure def Widget {
    param g : Solid = cylinder(10mm, 20mm)
}"#;
    let compiled = compile_no_errors(source);

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template not found");

    // (b) No value_cell named "g" — it should be a realization, not a scalar cell.
    let has_g_cell = template
        .value_cells
        .iter()
        .any(|c| c.id.member == "g");
    assert!(
        !has_g_cell,
        "expected no ValueCellDecl for 'g', but one was found (param should lower as realization)"
    );

    // (c) Exactly 1 RealizationDecl for the single geometry param.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 RealizationDecl for `param g : Solid = cylinder(...)`, got {}",
        template.realizations.len()
    );
}
