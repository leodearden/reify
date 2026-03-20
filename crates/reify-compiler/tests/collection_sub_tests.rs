//! Collection sub-structure tests (task 64).

use reify_types::{CompiledExprKind, Severity};

/// Helper: parse + compile source, assert no errors, return compiled output.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_coll"));
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
        "expected no error diagnostics, got: {:?}",
        errors
    );
    compiled
}

// ─── step-1: parse collection sub form ───

#[test]
fn parse_collection_sub_form() {
    let source = "structure S { sub bolts : List<Bolt> }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let structure = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_syntax::MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "bolts");
    assert_eq!(sub.structure_name, "Bolt");
    assert!(sub.is_collection, "expected is_collection=true for List<Bolt>");
    assert!(sub.args.is_empty(), "collection sub should have no args");
}

#[test]
fn parse_instantiation_sub_form() {
    let source = "structure S { sub rib = Rib() }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let structure = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_syntax::MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "rib");
    assert_eq!(sub.structure_name, "Rib");
    assert!(!sub.is_collection, "expected is_collection=false for = form");
}

// ─── step-3: compile collection sub ───

#[test]
fn compile_collection_sub() {
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S { sub bolts : List<Bolt> }
    "#;
    let compiled = compile_no_errors(source);
    // Find the S template
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");
    let sub = &s_template.sub_components[0];
    assert_eq!(sub.name, "bolts");
    assert_eq!(sub.structure_name, "Bolt");
    assert!(sub.is_collection, "compiled SubComponentDecl should have is_collection=true");
}

#[test]
fn compile_instantiation_sub() {
    let source = r#"
        structure Rib { param width : Scalar = 5mm }
        structure S { sub rib = Rib() }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");
    let sub = &s_template.sub_components[0];
    assert_eq!(sub.name, "rib");
    assert_eq!(sub.structure_name, "Rib");
    assert!(!sub.is_collection, "compiled SubComponentDecl should have is_collection=false");
}

// ─── step-5: count constraint recognition ───

#[test]
fn compile_count_constraint() {
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            param n : Int = 4
            sub bolts : List<Bolt>
            constraint bolts.count == n
        }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    // Verify synthetic count cell exists
    let count_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "__count_bolts")
        .expect("should have __count_bolts value cell");
    assert_eq!(count_cell.kind, reify_compiler::ValueCellKind::Let);
    assert_eq!(count_cell.cell_type, reify_types::Type::Int);

    // Verify count cell is in structure_controlling
    assert!(
        s_template.structure_controlling.contains(&count_cell.id),
        "count cell should be in structure_controlling"
    );

    // Verify count cell's default_expr is a ValueRef to 'n'
    let expr = count_cell.default_expr.as_ref().expect("should have default_expr");
    match &expr.kind {
        reify_types::CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.member, "n", "count expression should reference param n");
        }
        other => panic!("expected ValueRef, got {:?}", other),
    }

    // Verify SubComponentDecl has count_cell set
    let sub = &s_template.sub_components[0];
    assert!(sub.count_cell.is_some(), "sub should have count_cell set");
    assert_eq!(sub.count_cell.as_ref().unwrap().member, "__count_bolts");
}

// ─── step-15: bolts[0].diameter access via compiled expression ───

#[test]
fn compile_indexed_collection_member_access() {
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[0].diameter
        }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    // Find the 'd' let binding
    let d_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have let binding 'd'");

    // The expression should compile to a ValueRef with scoped ID S.bolts[0].diameter
    let expr = d_cell.default_expr.as_ref().expect("d should have an expression");
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S.bolts[0]", "entity should be S.bolts[0]");
            assert_eq!(id.member, "diameter", "member should be diameter");
        }
        other => panic!(
            "expected ValueRef(S.bolts[0].diameter), got {:?}",
            other
        ),
    }
}
