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
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

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
    assert!(
        sub.is_collection,
        "expected is_collection=true for List<Bolt>"
    );
    assert!(sub.args.is_empty(), "collection sub should have no args");
}

#[test]
fn parse_instantiation_sub_form() {
    let source = "structure S { sub rib = Rib() }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

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
    assert!(
        !sub.is_collection,
        "expected is_collection=false for = form"
    );
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
    assert!(
        sub.is_collection,
        "compiled SubComponentDecl should have is_collection=true"
    );
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
    assert!(
        !sub.is_collection,
        "compiled SubComponentDecl should have is_collection=false"
    );
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
    let expr = count_cell
        .default_expr
        .as_ref()
        .expect("should have default_expr");
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
    let expr = d_cell
        .default_expr
        .as_ref()
        .expect("d should have an expression");
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S.bolts[0]", "entity should be S.bolts[0]");
            assert_eq!(id.member, "diameter", "member should be diameter");
        }
        other => panic!("expected ValueRef(S.bolts[0].diameter), got {:?}", other),
    }
    // Result type should be Scalar (length) — resolved from child template's member type
    assert_eq!(
        expr.result_type,
        reify_types::Type::length(),
        "indexed collection member access should preserve the member's actual type (Scalar/length)"
    );
}

// ─── step-23: type annotation tests for indexed collection member access ───

#[test]
fn compile_indexed_collection_member_access_preserves_type() {
    let source = r#"
        structure Bolt { param count_per_row : Int = 4 }
        structure S {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let c = bolts[0].count_per_row
        }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    let c_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("should have let binding 'c'");

    let expr = c_cell
        .default_expr
        .as_ref()
        .expect("c should have an expression");
    // The result_type should be Int (matching Bolt.count_per_row's type), not Real
    assert_eq!(
        expr.result_type,
        reify_types::Type::Int,
        "indexed collection member access should preserve the member's actual type"
    );
}

// ─── step-25: declaration order dependency ───

#[test]
fn compile_count_constraint_before_sub_declaration() {
    // The constraint `bolts.count == n` appears BEFORE the `sub bolts : List<Bolt>` declaration.
    // This tests that the compiler handles forward-referenced sub declarations.
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            param n : Int = 4
            constraint bolts.count == n
            sub bolts : List<Bolt>
        }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    // (1) SubComponentDecl for 'bolts' should have count_cell set
    let sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == "bolts")
        .expect("should have sub 'bolts'");
    assert!(
        sub.count_cell.is_some(),
        "sub 'bolts' should have count_cell set even when constraint appears before sub declaration"
    );
    assert_eq!(
        sub.count_cell.as_ref().unwrap().member,
        "__count_bolts",
        "count_cell should point to __count_bolts"
    );

    // (2) The __count_bolts cell should exist in structure_controlling
    let count_id = sub.count_cell.as_ref().unwrap();
    assert!(
        s_template.structure_controlling.contains(count_id),
        "__count_bolts should be in structure_controlling"
    );
}

// ─── step-17: bolts.count compiles to ValueRef(__count_bolts) ───

#[test]
fn compile_bolts_count_expression() {
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let n = bolts.count
        }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    // Find the 'n' let binding
    let n_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "n")
        .expect("should have let binding 'n'");

    // The expression should compile to a ValueRef to the __count_bolts cell
    let expr = n_cell
        .default_expr
        .as_ref()
        .expect("n should have an expression");
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S", "entity should be S");
            assert_eq!(id.member, "__count_bolts", "member should be __count_bolts");
        }
        other => panic!("expected ValueRef(S.__count_bolts), got {:?}", other),
    }
    // Result type should be Int
    assert_eq!(
        expr.result_type,
        reify_types::Type::Int,
        "bolts.count type should be Int"
    );
}

// ─── step-27: dynamic index collection member access ───

#[test]
fn compile_dynamic_index_collection_member_access() {
    // When index is non-literal (a param), the compiler takes the dynamic-index path.
    // The collection base should be a ValueRef to __list_bolts, NOT a Literal(Undef).
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            param idx : Int = 0
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[idx].diameter
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

    let expr = d_cell
        .default_expr
        .as_ref()
        .expect("d should have an expression");

    // The expression should be IndexAccess(ValueRef(__list_bolts__diameter), idx)
    // where __list_bolts__diameter is a per-member synthetic list
    fn find_list_ref(expr: &reify_types::CompiledExpr) -> bool {
        match &expr.kind {
            CompiledExprKind::ValueRef(id) => {
                id.entity == "S" && id.member == "__list_bolts__diameter"
            }
            CompiledExprKind::MethodCall { object, .. } => find_list_ref(object),
            CompiledExprKind::IndexAccess { object, .. } => find_list_ref(object),
            _ => false,
        }
    }

    assert!(
        find_list_ref(expr),
        "dynamic index collection access should contain ValueRef to S.__list_bolts__diameter, got: {:?}",
        expr.kind
    );

    // Also verify it does NOT contain a Literal(Undef) as the collection base
    fn contains_undef_literal(expr: &reify_types::CompiledExpr) -> bool {
        match &expr.kind {
            CompiledExprKind::Literal(reify_types::Value::Undef) => true,
            CompiledExprKind::MethodCall { object, .. } => contains_undef_literal(object),
            CompiledExprKind::IndexAccess { object, .. } => contains_undef_literal(object),
            _ => false,
        }
    }

    assert!(
        !contains_undef_literal(expr),
        "dynamic index collection access should NOT contain Literal(Undef) as base"
    );
}

// ─── step-29: collection sub as standalone identifier ───

#[test]
fn compile_collection_sub_as_standalone_identifier() {
    // A bare collection sub name (`bolts`) should resolve to ValueRef(__list_bolts),
    // not produce an 'unresolved name' error.
    let source = r#"
        structure Bolt { param grade : Scalar = 8.8 }
        structure S {
            sub bolts : List<Bolt>
            constraint bolts.count == 3
            let grades = bolts
        }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    // Find the 'grades' let binding
    let grades_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "grades")
        .expect("should have let binding 'grades'");

    let expr = grades_cell
        .default_expr
        .as_ref()
        .expect("grades should have an expression");

    // Should be a ValueRef to __list_bolts__grade (first member's per-member list), not Literal(Undef)
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S", "entity should be S");
            assert_eq!(
                id.member, "__list_bolts__grade",
                "member should be __list_bolts__grade"
            );
        }
        other => panic!("expected ValueRef(S.__list_bolts__grade), got {:?}", other),
    }

    // Result type should be List
    match &expr.result_type {
        reify_types::Type::List(_) => {}
        other => panic!("expected List type, got {:?}", other),
    }
}
