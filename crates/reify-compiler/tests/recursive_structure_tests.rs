//! Recursive structure detection tests (task 203).
//!
//! These tests verify that the compiler detects self-referential structure
//! definitions and tags them with `is_recursive = true`, emitting a warning
//! diagnostic with the cycle path.

use reify_types::Severity;

/// Helper: parse + compile source, allowing warnings but not errors.
/// Returns the compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_recursive"));
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

// ─── step-1: direct self-reference ───

#[test]
fn direct_self_reference_tagged_recursive() {
    // Structure A has `sub x = A()` — directly references itself.
    // Expect: A.is_recursive == true, warning diagnostic with 'recursive structure cycle'.
    let source = r#"
        structure A {
            sub x = A()
        }
    "#;
    let compiled = compile_module(source);

    let a_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "A")
        .expect("should have template A");

    assert!(
        a_template.is_recursive,
        "structure A with sub x = A() should be tagged is_recursive=true"
    );

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("recursive structure cycle"))
        .collect();
    assert!(
        !recursion_warnings.is_empty(),
        "expected a warning diagnostic containing 'recursive structure cycle', got: {:?}",
        compiled.diagnostics
    );
}

// ─── step-3: mutual recursion ───

#[test]
fn mutual_recursion_both_tagged() {
    // A has sub b = B(), B has sub a = A() — mutually recursive.
    // Expect: both A and B are tagged is_recursive=true.
    let source = r#"
        structure A {
            sub b = B()
        }
        structure B {
            sub a = A()
        }
    "#;
    let compiled = compile_module(source);

    let a_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "A")
        .expect("should have template A");
    let b_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "B")
        .expect("should have template B");

    assert!(
        a_template.is_recursive,
        "structure A should be tagged is_recursive=true in A<->B mutual cycle"
    );
    assert!(
        b_template.is_recursive,
        "structure B should be tagged is_recursive=true in A<->B mutual cycle"
    );
}

// ─── step-5: indirect cycle through 3 structures ───

#[test]
fn indirect_cycle_three_structures_all_tagged() {
    // A -> B -> C -> A — indirect cycle through 3 structures.
    // Expect: all three tagged is_recursive=true, diagnostic contains full cycle path.
    let source = r#"
        structure A {
            sub b = B()
        }
        structure B {
            sub c = C()
        }
        structure C {
            sub a = A()
        }
    "#;
    let compiled = compile_module(source);

    let a_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "A")
        .expect("should have template A");
    let b_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "B")
        .expect("should have template B");
    let c_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "C")
        .expect("should have template C");

    assert!(a_template.is_recursive, "A should be recursive in A->B->C->A cycle");
    assert!(b_template.is_recursive, "B should be recursive in A->B->C->A cycle");
    assert!(c_template.is_recursive, "C should be recursive in A->B->C->A cycle");

    // The diagnostic should contain the cycle path
    let cycle_warning = compiled
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Warning && d.message.contains("recursive structure cycle"));
    assert!(
        cycle_warning.is_some(),
        "expected a warning diagnostic about recursive structure cycle, got: {:?}",
        compiled.diagnostics
    );
    let msg = &cycle_warning.unwrap().message;
    // Cycle path should mention A, B, C
    assert!(
        msg.contains('A') && msg.contains('B') && msg.contains('C'),
        "cycle warning should contain A, B, C in path, got: {}",
        msg
    );
}

// ─── step-7: non-recursive structures produce no false positives ───

#[test]
fn non_recursive_dag_no_false_positives() {
    // A -> B, C -> D — no cycles. None should be tagged recursive.
    let source = r#"
        structure B { param x : Scalar = 1mm }
        structure A { sub b = B() }
        structure D { param y : Scalar = 2mm }
        structure C { sub d = D() }
    "#;
    let compiled = compile_module(source);

    for template in &compiled.templates {
        assert!(
            !template.is_recursive,
            "structure {} should NOT be recursive in a DAG, but is_recursive=true",
            template.name
        );
    }

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("recursive structure cycle"))
        .collect();
    assert!(
        recursion_warnings.is_empty(),
        "expected no recursive structure cycle warnings for a DAG, got: {:?}",
        recursion_warnings
    );
}

// ─── step-9: mixed recursive and non-recursive ───

#[test]
fn mixed_recursive_and_non_recursive_only_cycle_tagged() {
    // A->B->A cycle plus standalone C->D — only A and B should be tagged.
    let source = r#"
        structure A {
            sub b = B()
        }
        structure B {
            sub a = A()
        }
        structure D { param z : Scalar = 1mm }
        structure C { sub d = D() }
    "#;
    let compiled = compile_module(source);

    let a_template = compiled.templates.iter().find(|t| t.name == "A").expect("template A");
    let b_template = compiled.templates.iter().find(|t| t.name == "B").expect("template B");
    let c_template = compiled.templates.iter().find(|t| t.name == "C").expect("template C");
    let d_template = compiled.templates.iter().find(|t| t.name == "D").expect("template D");

    assert!(a_template.is_recursive, "A should be recursive (in A<->B cycle)");
    assert!(b_template.is_recursive, "B should be recursive (in A<->B cycle)");
    assert!(!c_template.is_recursive, "C should NOT be recursive (only C->D, no cycle)");
    assert!(!d_template.is_recursive, "D should NOT be recursive (no subs at all)");
}

// ─── step-11: sub referencing unknown/external structure ───

#[test]
fn unknown_structure_reference_no_panic_no_false_positive() {
    // A has sub x = External() where External doesn't exist in this module.
    // Should compile without panic, without false recursion diagnostic.
    let source = r#"
        structure A {
            sub x = External()
        }
    "#;
    // Note: this may emit a warning about unresolved structure, but should NOT
    // emit a recursive structure cycle warning.
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_recursive"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);

    // Should not panic — verify it compiled
    let a_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "A")
        .expect("should have template A");

    assert!(
        !a_template.is_recursive,
        "A should NOT be tagged recursive just because it references an unknown structure"
    );

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("recursive structure cycle"))
        .collect();
    assert!(
        recursion_warnings.is_empty(),
        "should produce no recursive structure cycle warnings for unknown reference, got: {:?}",
        recursion_warnings
    );
}

// ─── step-13: multiple subs, one creating a cycle ───

#[test]
fn multiple_subs_one_cycle_correct_tagging() {
    // A has sub b = B() and sub c = C().
    // B has sub a = A() — creates A<->B cycle.
    // C has no subs — not in any cycle.
    // Expect: A and B are recursive, C is not.
    let source = r#"
        structure C { param w : Scalar = 1mm }
        structure B {
            sub a = A()
        }
        structure A {
            sub b = B()
            sub c = C()
        }
    "#;
    let compiled = compile_module(source);

    let a_template = compiled.templates.iter().find(|t| t.name == "A").expect("template A");
    let b_template = compiled.templates.iter().find(|t| t.name == "B").expect("template B");
    let c_template = compiled.templates.iter().find(|t| t.name == "C").expect("template C");

    assert!(a_template.is_recursive, "A should be recursive (in A<->B cycle)");
    assert!(b_template.is_recursive, "B should be recursive (in A<->B cycle)");
    assert!(!c_template.is_recursive, "C should NOT be recursive (not in any cycle)");
}

// ─── step-15: structure with no subs ───

#[test]
fn no_subs_not_recursive() {
    // Structure with no sub declarations should not be tagged recursive.
    let source = r#"
        structure Leaf {
            param width : Scalar = 10mm
            param height : Scalar = 5mm
        }
    "#;
    let compiled = compile_module(source);

    let leaf_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Leaf")
        .expect("should have template Leaf");

    assert!(
        !leaf_template.is_recursive,
        "Leaf with no subs should NOT be tagged is_recursive"
    );

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("recursive structure cycle"))
        .collect();
    assert!(
        recursion_warnings.is_empty(),
        "no recursion warnings expected for leaf structure, got: {:?}",
        recursion_warnings
    );
}
