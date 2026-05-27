//! Collection sub-structure tests (task 64).

use reify_test_support::{compile_source, parse_and_compile};
use reify_core::{DiagnosticCode, Severity};
use reify_ir::CompiledExprKind;

/// Helper: compile source and assert no error-severity diagnostics.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

// ─── step-1: parse collection sub form ───

#[test]
fn parse_collection_sub_form() {
    let source = "structure S { sub bolts : List<Bolt> }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let structure = match &parsed.declarations[0] {
        reify_ast::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_ast::MemberDecl::Sub(s) => s,
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let structure = match &parsed.declarations[0] {
        reify_ast::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_ast::MemberDecl::Sub(s) => s,
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
    let compiled = parse_and_compile(source);
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
    let compiled = parse_and_compile(source);
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
    let compiled = parse_and_compile(source);
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
    assert_eq!(count_cell.cell_type, reify_core::Type::Int);

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
        reify_ir::CompiledExprKind::ValueRef(id) => {
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
    let compiled = parse_and_compile(source);
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
        reify_core::Type::length(),
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
    let compiled = parse_and_compile(source);
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
        reify_core::Type::Int,
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
    let compiled = parse_and_compile(source);
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
    let compiled = parse_and_compile(source);
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
        reify_core::Type::Int,
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
    let compiled = parse_and_compile(source);
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
    fn find_list_ref(expr: &reify_ir::CompiledExpr) -> bool {
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
    fn contains_undef_literal(expr: &reify_ir::CompiledExpr) -> bool {
        match &expr.kind {
            CompiledExprKind::Literal(reify_ir::Value::Undef) => true,
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

// ─── task-1441 regression: multi-member child resolution ───

#[test]
fn compile_indexed_member_access_multi_member_child() {
    // Bolt has two members: diameter (Scalar) and grade (Scalar).
    // Accessing BOTH members via bolts[0].diameter and bolts[0].grade must
    // each compile to a ValueRef with the correct member name and type —
    // pinning that member-type resolution works for all members of collection
    // subs, not just the first.
    let source = r#"
        structure Bolt {
            param diameter : Scalar = 10mm
            param grade : Scalar = 8.8
        }
        structure S {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[0].diameter
            let g = bolts[0].grade
        }
    "#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    let g_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "g")
        .expect("should have let binding 'g'");

    let expr = g_cell
        .default_expr
        .as_ref()
        .expect("g should have an expression");

    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S.bolts[0]", "entity should be S.bolts[0]");
            assert_eq!(
                id.member, "grade",
                "member should be grade (the second member)"
            );
        }
        other => panic!("expected ValueRef(S.bolts[0].grade), got {:?}", other),
    }
    // Note: `Scalar` keyword always maps to Type::length() regardless of physical meaning —
    // this tests the type-system mapping, not dimensional analysis.
    // (grade 8.8 is dimensionless in the physical world but Scalar → length() in Reify's type system.)
    assert_eq!(
        expr.result_type,
        reify_core::Type::length(),
        "grade member should have Scalar/length type matching its 'Scalar' declaration"
    );

    // Also verify the first member (diameter) resolves correctly in the same
    // multi-member scenario — both members must compile to ValueRef with the
    // right entity, member name, and type.
    let d_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have let binding 'd'");

    let d_expr = d_cell
        .default_expr
        .as_ref()
        .expect("d should have an expression");

    match &d_expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S.bolts[0]", "entity should be S.bolts[0]");
            assert_eq!(
                id.member, "diameter",
                "member should be diameter (the first member)"
            );
        }
        other => panic!("expected ValueRef(S.bolts[0].diameter), got {:?}", other),
    }
    assert_eq!(
        d_expr.result_type,
        reify_core::Type::length(),
        "diameter member should have Scalar/length type matching its 'Scalar' declaration"
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
    let compiled = parse_and_compile(source);
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
        reify_core::Type::List(_) => {}
        other => panic!("expected List type, got {:?}", other),
    }
}

// ─── task-1454: combined collection + non-collection sub InstanceQualifiedAccess ───

/// Proof that `sub_member_types` is the single authoritative source for ALL sub types.
///
/// A structure with BOTH a non-collection sub (`sub part = Inner()`) and a collection
/// sub (`sub parts : List<Inner>`) — each accessed via `InstanceQualifiedAccess` —
/// must compile without errors and without an ICE diagnostic.
///
/// The existing tests `sub_member_type_resolves_without_ice` (collection) and
/// `non_collection_sub_member_type_resolves_without_ice` (non-collection) cover each
/// form in isolation.  This test locks in the superset property: both subs coexist in
/// the same entity and both resolve correctly from `sub_member_types`.
#[test]
fn mixed_sub_types_instance_qualified_access() {
    let source = r#"
        trait MechTrait {
            param diameter : Length
        }
        structure Inner : MechTrait {
            param diameter : Length = 5mm
        }
        structure Outer {
            sub part = Inner()
            sub parts : List<Inner>
            let d1 = part.(MechTrait::diameter)
            let d2 = parts.(MechTrait::diameter)
        }
    "#;
    let compiled = compile_no_errors(source);

    // Confirm no ICE diagnostics (compile_no_errors already rejects Error-severity
    // diagnostics, but an explicit ICE check makes the intent of this test clear).
    let has_ice = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on mixed sub-type instance qualified access, got: {:?}",
        compiled.diagnostics
    );

    // Positive correctness check: verify that d1 and d2 resolved to the expected
    // ValueRef IDs and types — not the ICE fallback (Type::Real) or an error result.
    //
    // For non-collection subs, InstanceQualifiedAccess produces a ValueRef scoped to
    // "Outer.part" with the element type from Inner (Length).
    // For collection subs it produces a ValueRef scoped to "Outer.parts" with the same
    // element type — the evaluator handles list-expansion semantics at runtime.
    let outer_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("should have template Outer");

    // --- d1: non-collection sub member access ---
    let d1_cell = outer_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d1")
        .expect("should have let binding 'd1'");
    let d1_expr = d1_cell
        .default_expr
        .as_ref()
        .expect("d1 should have an expression");
    match &d1_expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "Outer.part",
                "d1 should reference sub-scope Outer.part"
            );
            assert_eq!(
                id.member, "diameter",
                "d1 should reference member 'diameter'"
            );
        }
        other => panic!("expected ValueRef for d1, got {:?}", other),
    }
    assert_eq!(
        d1_expr.result_type,
        reify_core::Type::length(),
        "d1 (non-collection sub InstanceQualifiedAccess) should resolve to Length"
    );

    // --- d2: collection sub member access ---
    let d2_cell = outer_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d2")
        .expect("should have let binding 'd2'");
    let d2_expr = d2_cell
        .default_expr
        .as_ref()
        .expect("d2 should have an expression");
    match &d2_expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "Outer.parts",
                "d2 should reference sub-scope Outer.parts"
            );
            assert_eq!(
                id.member, "diameter",
                "d2 should reference member 'diameter'"
            );
        }
        other => panic!("expected ValueRef for d2, got {:?}", other),
    }
    assert_eq!(
        d2_expr.result_type,
        reify_core::Type::length(),
        "d2 (collection sub InstanceQualifiedAccess) should resolve to Length (element type; \
         list-expansion is handled by the evaluator at runtime)"
    );
}

// ─── task-1441 regression: collection/scalar coexistence + bare collection identifier ───

#[test]
fn compile_collection_identifier_after_noncollection_sub() {
    // Structure has BOTH a non-collection sub (rib = Rib()) AND a collection sub
    // (bolts : List<Bolt>). Bare-identifier resolution of `bolts` must resolve to
    // ValueRef(__list_bolts__...), NOT confuse bolts with rib or error.
    // This locks in that collection_sub_names (not map presence) is what gates
    // collection-specific resolution — after the refactor sub_member_types covers
    // BOTH subs, so only the gate distinguishes them.
    let source = r#"
        structure Rib { param width : Scalar = 5mm }
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            sub rib = Rib()
            sub bolts : List<Bolt>
            constraint bolts.count == 3
            let gs = bolts
        }
    "#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");

    // Find the 'gs' let binding
    let gs_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "gs")
        .expect("should have let binding 'gs'");

    let expr = gs_cell
        .default_expr
        .as_ref()
        .expect("gs should have an expression");

    // Should resolve to a ValueRef to the lexicographically-first member's per-member list.
    // Bolt has one member (diameter), so the result is __list_bolts__diameter.
    // sub_member_types uses BTreeMap internally, so this is deterministic.
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S", "entity should be S");
            assert_eq!(
                id.member, "__list_bolts__diameter",
                "member should be __list_bolts__diameter (lex-first member of Bolt)"
            );
        }
        other => panic!(
            "expected ValueRef(S.__list_bolts__diameter) for bare collection sub 'bolts', got {:?}",
            other
        ),
    }

    // Result type should be List
    match &expr.result_type {
        reify_core::Type::List(_) => {}
        other => panic!(
            "expected List type for bare collection identifier, got {:?}",
            other
        ),
    }
}

// ─── task-1729: negative test for mixed-sub wrong-trait InstanceQualifiedAccess ───

/// Negative counterpart to `mixed_sub_types_instance_qualified_access`.
///
/// When a non-collection sub (`sub part = Inner()`) and a collection sub
/// (`sub parts : List<Inner>`) are both accessed via `InstanceQualifiedAccess`
/// with a trait that `Inner` does NOT implement, the compiler must emit an
/// Error-severity diagnostic for EACH access — not silently succeed or ICE.
///
/// The diagnostic path lives at expr.rs:1631-1639 and is identical for both
/// sub kinds (both go through `sub_structure_traits` lookup).
#[test]
fn mixed_sub_types_wrong_trait_diagnostic() {
    let source = r#"
        trait MechTrait {
            param diameter : Length
        }
        trait UnrelatedTrait {
            param weight : Scalar
        }
        structure Inner : MechTrait {
            param diameter : Length = 5mm
        }
        structure Outer {
            sub part = Inner()
            sub parts : List<Inner>
            let d1 = part.(UnrelatedTrait::weight)
            let d2 = parts.(UnrelatedTrait::weight)
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // Both sub accesses (non-collection 'part' and collection 'parts') must each
    // produce an error diagnostic.
    assert!(
        errors.len() >= 2,
        "expected at least 2 error diagnostics (one per sub), got {}: {:?}",
        errors.len(),
        errors
    );

    // Every error must carry the typed `TraitNotImplemented` diagnostic code
    // (introduced in task 2205 — decouples the assertion from message wording).
    for err in &errors {
        assert_eq!(
            err.code,
            Some(DiagnosticCode::TraitNotImplemented),
            "error code should be DiagnosticCode::TraitNotImplemented: {:?}",
            err
        );
    }

    // Non-collection sub: error mentions "'part'" (quoted, to exclude the "parts" match)
    // and "UnrelatedTrait". The diagnostic format is: sub-component 'part' (type ...).
    let part_err = errors
        .iter()
        .find(|e| e.message.contains("'part'") && e.message.contains("UnrelatedTrait"));
    assert!(
        part_err.is_some(),
        "expected an error mentioning 'part' and 'UnrelatedTrait', got: {:?}",
        errors
    );

    // Collection sub: error mentions "parts" and "UnrelatedTrait".
    let parts_err = errors
        .iter()
        .find(|e| e.message.contains("parts") && e.message.contains("UnrelatedTrait"));
    assert!(
        parts_err.is_some(),
        "expected an error mentioning 'parts' and 'UnrelatedTrait', got: {:?}",
        errors
    );
}
