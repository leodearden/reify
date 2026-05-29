//! Compiler typing tests for structural-query accessors: `self.children`, `self.members`,
//! `self.descendants` (task 3982, PRD §8 Phase 1 — compiler-only, no eval).
//!
//! Observable signals:
//!   - Each accessor resolves to `Type::List(Box::new(Type::StructureRef("Structure")))`.
//!   - `let n = count(self.children)` compiles with no Error diagnostics.
//!   - A bogus accessor (`self.grandchildren`) still emits an "unknown member" error.
//!   - A user-declared `param children` shadows the built-in accessor.

use reify_core::{Severity, Type};
use reify_test_support::{compile_source, parse_and_compile};

/// Helper: assert `ty` is `List(StructureRef("Structure"))`.
fn assert_list_of_entity_ref(ty: &Type, label: &str) {
    let prefix = if label.is_empty() {
        String::new()
    } else {
        format!("{} ", label)
    };
    match ty {
        Type::List(inner) => {
            assert_eq!(
                inner.as_ref(),
                &Type::StructureRef("Structure".to_string()),
                "{}expected List(StructureRef(\"Structure\")), got List({:?})",
                prefix,
                inner,
            );
        }
        other => panic!("{}expected List type, got: {:?}", prefix, other),
    }
}

// ─── step-1 core RED test ───

/// `self.children`, `self.members`, `self.descendants` each resolve to
/// `Type::List(Box::new(Type::StructureRef("Structure")))` with zero Error diagnostics.
///
/// RED today: each accessor falls through to the unknown-member poison path
/// and emits an Error diagnostic, so assertion (a) fails immediately.
#[test]
fn self_structural_accessors_resolve_to_list_of_entity_ref() {
    let source = r#"
        structure Leaf {}
        structure Asm {
            sub a = Leaf()
            let cs = self.children
            let ms = self.members
            let ds = self.descendants
        }
    "#;

    let compiled = compile_source(source);

    // (a) Zero Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) For each of cs/ms/ds, Asm template's value cell's default_expr.result_type
    //     equals `Type::List(Box::new(Type::StructureRef("Structure")))`.
    let asm_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Asm")
        .expect("Asm template");

    for member_name in &["cs", "ms", "ds"] {
        let cell = asm_template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == *member_name)
            .unwrap_or_else(|| panic!("value cell '{}' not found in Asm", member_name));
        let default_expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("cell '{}' has no default_expr", member_name));
        assert_list_of_entity_ref(&default_expr.result_type, member_name);
    }
}

// ─── step-3 guardrail tests ───

/// A bogus accessor (`self.grandchildren`) must still emit an Error diagnostic
/// whose message contains "unknown member".  The dispatch is not greedy.
#[test]
fn bogus_accessor_still_errors() {
    let source = r#"
        structure S {
            let x = self.grandchildren
        }
    "#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for self.grandchildren"
    );
    let has_unknown_member = errors.iter().any(|d| d.message.contains("unknown member"));
    assert!(
        has_unknown_member,
        "expected 'unknown member' in error message, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A user-declared `param children` shadows the built-in accessor.
/// `self.children` must resolve to the param cell (type Int, not List<StructureRef>).
#[test]
fn user_declared_member_shadows_accessor() {
    let source = r#"
        structure S {
            param children : Int = 3
            let c = self.children
        }
    "#;

    // parse_and_compile panics on any Error diagnostic — confirms zero errors.
    let compiled = parse_and_compile(source);

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let c_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("c value cell");

    let default_expr = c_cell.default_expr.as_ref().expect("c has default_expr");

    // The default_expr should be a ValueRef pointing to the param cell, not a MethodCall.
    // We verify by checking it references ValueCellId("S", "children") and has type Int.
    let refs = default_expr.collect_value_refs();
    let expected_id = reify_core::ValueCellId::new("S", "children");
    assert!(
        refs.contains(&expected_id),
        "c's default_expr should reference S.children (the param), got refs: {:?}",
        refs
    );

    // The result type must be Int, not List<StructureRef("Structure")>.
    assert_eq!(
        default_expr.result_type,
        Type::Int,
        "c should have type Int (from the param), got: {:?}",
        default_expr.result_type
    );
}

/// `let n = count(self.children)` over a multi-sub structure compiles with no panic.
/// Exercises the free-function `count` applied to a structural-query accessor.
#[test]
fn count_of_self_children_compiles_clean() {
    let source = r#"
        structure Leaf {}
        structure Asm {
            sub a = Leaf()
            sub b = Leaf()
            let n = count(self.children)
        }
    "#;
    // parse_and_compile panics on any Error diagnostic.
    let _compiled = parse_and_compile(source);
}
