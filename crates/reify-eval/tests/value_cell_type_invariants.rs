//! Regression lock for the value-cell cell_type invariant relied upon by
//! `value_type_kind_matches` (crates/reify-eval/src/lib.rs): post-compilation,
//! no ValueCellDecl.cell_type carries Type::TypeParam — that variant has no
//! Value counterpart and would fall through the match to the default-reject
//! path. (Type::Geometry is representable as of task 3604 / GHR-β.)
//!
//! `Type::StructureRef` is intentionally NOT checked here (task 1876):
//! user code may declare `param x : SomeStruct = SomeStruct(...)` which
//! compiles to a ValueCellDecl with cell_type = StructureRef. The struct-call
//! default evaluates to Value::Undef (structure constructors are not
//! builtins), and Undef is accepted by the kind-match for any type.

use reify_compiler::{CompiledModule, TopologyTemplate, ValueCellDecl};
use reify_types::{ModulePath, Severity};

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
    // Walker assumption: every compiled ValueCellDecl lives in
    // template.value_cells OR a guarded_group members/else_members list.
    // SubComponentDecl instantiations reference child templates by name;
    // both parent and child templates appear in module.templates, so the
    // top-level loop in assert_module_cells_representable covers them without
    // recursion.  If the compiler ever introduces ValueCellDecls stored off
    // this path (e.g. extra synthetic cells produced during resolution), this
    // walker will need to be extended to reach them.
    let check = |cell: &ValueCellDecl| {
        // Aligned with (a) the module docstring lines 7-11 which explicitly
        // documents that `Type::StructureRef` is intentionally NOT checked
        // (task 1876 — user code may declare `param x : SomeStruct =
        // SomeStruct(...)`; the struct-call default evaluates to `Value::Undef`
        // which passes the kind-match for any type), and (b) the shared
        // predicate `reify_eval::is_representable_cell_type` which is the
        // single source of truth consumed by both this walker and the runtime
        // invariant in crates/reify-eval/src/engine_eval.rs.
        assert!(
            reify_eval::is_representable_cell_type(&cell.cell_type),
            "{}: template `{}` cell `{}` has cell_type {:?}",
            reify_eval::ASSERT_MSG_PREFIX,
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
    // Use CARGO_MANIFEST_DIR so the path is robust to a non-manifest CWD
    // (convention established by task 348; mirrors m10_combined.rs:18 et al.).
    const PATH_MATH_LINALG: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/math_linalg.ri");
    let source = std::fs::read_to_string(PATH_MATH_LINALG).expect("math_linalg.ri fixture");
    let parsed = reify_syntax::parse(&source, ModulePath::single("math_linalg"));
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
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    assert_module_cells_representable(&compiled);
}

#[test]
fn boltflange_value_cells_are_representable() {
    // Exercise the task-1876 case the module docstring calls out explicitly:
    // `examples/m5_geometry_flange.ri` declares
    //     param material : Material = Material(name: "steel", density: 7850.0, ...)
    // The compiled cell for `material` carries cell_type =
    // Type::StructureRef("Material"), which is the exact variant the walker
    // must tolerate per the runtime invariant in
    // crates/reify-eval/src/engine_eval.rs (forbids only TypeParam | Geometry).
    //
    // BoltFlange conforms to `Rigid` and uses the stdlib `Material` struct, so
    // compile with full stdlib prelude (unlike math_linalg.ri which is
    // stdlib-independent).
    const PATH_BOLTFLANGE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/m5_geometry_flange.ri"
    );
    let source = std::fs::read_to_string(PATH_BOLTFLANGE).expect("m5_geometry_flange.ri fixture");
    let parsed = reify_syntax::parse(&source, ModulePath::single("m5_geometry_flange"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    assert_module_cells_representable(&compiled);
}
