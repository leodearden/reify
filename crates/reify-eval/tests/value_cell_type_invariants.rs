//! Regression lock for the value-cell cell_type invariant relied upon by
//! `value_type_kind_matches` (crates/reify-eval/src/lib.rs): post-compilation,
//! no ValueCellDecl.cell_type carries Type::TypeParam, Type::StructureRef, or
//! Type::Geometry — those three variants have no Value counterpart and would
//! fall through the match to the default-reject path.

use reify_compiler::{CompiledModule, TopologyTemplate, ValueCellDecl};
use reify_types::{ModulePath, Severity, Type};

/// Walk every ValueCellDecl in a CompiledModule — primary template cells,
/// guarded-group member/else-member cells, and (via sub_components) any
/// referenced child templates. Assert cell_type is not one of the three
/// unrepresentable variants.
fn assert_module_cells_representable(module: &CompiledModule) {
    for template in &module.templates {
        assert_template_cells_representable(template);
    }
}

fn assert_template_cells_representable(template: &TopologyTemplate) {
    let check = |cell: &ValueCellDecl| {
        assert!(
            !matches!(
                &cell.cell_type,
                Type::TypeParam(_) | Type::StructureRef(_) | Type::Geometry
            ),
            "template `{}` cell `{}` has unrepresentable cell_type {:?}",
            template.name,
            cell.id,
            cell.cell_type,
        );
    };
    for cell in &template.value_cells {
        check(cell);
    }
    for group in &template.guarded_groups {
        for cell in &group.members {
            check(cell);
        }
        for cell in &group.else_members {
            check(cell);
        }
    }
}

#[test]
fn stdlib_value_cells_are_representable() {
    for module in reify_compiler::stdlib_loader::load_stdlib() {
        assert_module_cells_representable(module);
    }
}

#[test]
fn user_fixture_value_cells_are_representable() {
    // Pick a representative .ri example that exercises params + lets +
    // dimensional types across multiple structures. math_linalg.ri is a
    // solid canonical choice already used by m8_stdlib_integration.
    let source = std::fs::read_to_string("../../examples/math_linalg.ri")
        .expect("math_linalg.ri fixture");
    let parsed = reify_syntax::parse(&source, ModulePath::single("math_linalg"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    assert_module_cells_representable(&compiled);
}
