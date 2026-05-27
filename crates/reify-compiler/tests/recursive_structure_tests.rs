//! Recursive structure detection tests (task 203).
//!
//! These tests verify that the compiler detects self-referential structure
//! definitions and tags them with `is_recursive = true`, emitting a warning
//! diagnostic with the cycle path.

use reify_compiler::find_template;
use reify_test_support::parse_and_compile;
use reify_core::Severity;

// ─── step-1: direct self-reference ───

#[test]
fn direct_self_reference_tagged_recursive() {
    // Structure A has a self-referencing sub (with termination guard per task 204).
    // Expect: A.is_recursive == true, warning diagnostic with 'recursive structure cycle'.
    let source = r#"
        structure A {
            param n : Int = 3
            sub x = A(n: n - 1) where n > 0
        }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("should have template A");

    assert!(
        a_template.is_recursive,
        "structure A with sub x = A() should be tagged is_recursive=true"
    );

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
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
    // A references B, B references A (with termination guards per task 204).
    // Expect: both A and B are tagged is_recursive=true.
    let source = r#"
        structure A {
            param n : Int = 3
            sub b = B(n: n - 1) where n > 0
        }
        structure B {
            param n : Int = 3
            sub a = A(n: n - 1) where n > 0
        }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("should have template A");
    let b_template = find_template(&compiled.templates, "B").expect("should have template B");

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
    // A -> B -> C -> A — indirect cycle through 3 structures (with termination guards).
    // Expect: all three tagged is_recursive=true, diagnostic contains full cycle path.
    let source = r#"
        structure A {
            param n : Int = 3
            sub b = B(n: n - 1) where n > 0
        }
        structure B {
            param n : Int = 3
            sub c = C(n: n - 1) where n > 0
        }
        structure C {
            param n : Int = 3
            sub a = A(n: n - 1) where n > 0
        }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("should have template A");
    let b_template = find_template(&compiled.templates, "B").expect("should have template B");
    let c_template = find_template(&compiled.templates, "C").expect("should have template C");

    assert!(
        a_template.is_recursive,
        "A should be recursive in A->B->C->A cycle"
    );
    assert!(
        b_template.is_recursive,
        "B should be recursive in A->B->C->A cycle"
    );
    assert!(
        c_template.is_recursive,
        "C should be recursive in A->B->C->A cycle"
    );

    // The diagnostic should contain the cycle path
    let cycle_warning = compiled.diagnostics.iter().find(|d| {
        d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
    });
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
    let compiled = parse_and_compile(source);

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
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
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
    // A<->B cycle (with termination guards) plus standalone C->D — only A and B should be tagged.
    let source = r#"
        structure A {
            param n : Int = 3
            sub b = B(n: n - 1) where n > 0
        }
        structure B {
            param n : Int = 3
            sub a = A(n: n - 1) where n > 0
        }
        structure D { param z : Scalar = 1mm }
        structure C { sub d = D() }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("template A");
    let b_template = find_template(&compiled.templates, "B").expect("template B");
    let c_template = find_template(&compiled.templates, "C").expect("template C");
    let d_template = find_template(&compiled.templates, "D").expect("template D");

    assert!(
        a_template.is_recursive,
        "A should be recursive (in A<->B cycle)"
    );
    assert!(
        b_template.is_recursive,
        "B should be recursive (in A<->B cycle)"
    );
    assert!(
        !c_template.is_recursive,
        "C should NOT be recursive (only C->D, no cycle)"
    );
    assert!(
        !d_template.is_recursive,
        "D should NOT be recursive (no subs at all)"
    );
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_recursive"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);

    // Should not panic — verify it compiled
    let a_template = find_template(&compiled.templates, "A").expect("should have template A");

    assert!(
        !a_template.is_recursive,
        "A should NOT be tagged recursive just because it references an unknown structure"
    );

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
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
    // A has sub b = B() (recursive, guarded) and sub c = C() (non-recursive — C is a leaf).
    // B has sub a = A() (recursive, guarded) — creates A<->B cycle.
    // Expect: A and B are recursive, C is not. Only the A<->B sub needs a guard;
    // the A -> C edge is not in the A<->B SCC so no guard is required there.
    let source = r#"
        structure C { param w : Scalar = 1mm }
        structure B {
            param n : Int = 3
            sub a = A(n: n - 1) where n > 0
        }
        structure A {
            param n : Int = 3
            sub b = B(n: n - 1) where n > 0
            sub c = C()
        }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("template A");
    let b_template = find_template(&compiled.templates, "B").expect("template B");
    let c_template = find_template(&compiled.templates, "C").expect("template C");

    assert!(
        a_template.is_recursive,
        "A should be recursive (in A<->B cycle)"
    );
    assert!(
        b_template.is_recursive,
        "B should be recursive (in A<->B cycle)"
    );
    assert!(
        !c_template.is_recursive,
        "C should NOT be recursive (not in any cycle)"
    );
}

// ─── step-17: multi-path cycle convergence (Tarjan bug) ───

#[test]
fn multi_path_convergence_all_cycle_participants_tagged() {
    // Graph: A→B, A→D, B→C, D→C, C→A (with termination guards per task 204).
    // Two cycles exist: A→B→C→A and A→D→C→A.
    // All four nodes A, B, C, D participate in a cycle.
    //
    // The old DFS gray/black approach misses D: DFS finds A→B→C→A and marks A,B,C in_cycle.
    // Then A visits D; D visits C (already black/fully-explored), takes no action — D is
    // never tagged, even though D→C→A→D is a valid cycle.
    //
    // Tarjan's SCC correctly identifies the single SCC {A, B, C, D} where all four are
    // mutually reachable through the cycles.
    let source = r#"
        structure A {
            param n : Int = 3
            sub b = B(n: n - 1) where n > 0
            sub d = D(n: n - 1) where n > 0
        }
        structure B {
            param n : Int = 3
            sub c = C(n: n - 1) where n > 0
        }
        structure D {
            param n : Int = 3
            sub cc = C(n: n - 1) where n > 0
        }
        structure C {
            param n : Int = 3
            sub a = A(n: n - 1) where n > 0
        }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("template A");
    let b_template = find_template(&compiled.templates, "B").expect("template B");
    let c_template = find_template(&compiled.templates, "C").expect("template C");
    let d_template = find_template(&compiled.templates, "D").expect("template D");

    assert!(
        a_template.is_recursive,
        "A should be recursive (participates in A→B→C→A cycle)"
    );
    assert!(
        b_template.is_recursive,
        "B should be recursive (participates in A→B→C→A cycle)"
    );
    assert!(
        c_template.is_recursive,
        "C should be recursive (participates in both cycles)"
    );
    assert!(
        d_template.is_recursive,
        "D should be recursive (participates in A→D→C→A cycle) — \
         the old DFS misses D because C is fully explored before D is checked"
    );

    // Should have at least one recursive cycle warning
    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();
    assert!(
        !recursion_warnings.is_empty(),
        "expected recursive structure cycle warning(s), got: {:?}",
        compiled.diagnostics
    );
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
    let compiled = parse_and_compile(source);

    let leaf_template =
        find_template(&compiled.templates, "Leaf").expect("should have template Leaf");

    assert!(
        !leaf_template.is_recursive,
        "Leaf with no subs should NOT be tagged is_recursive"
    );

    let recursion_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();
    assert!(
        recursion_warnings.is_empty(),
        "no recursion warnings expected for leaf structure, got: {:?}",
        recursion_warnings
    );
}

// ─── step-19: false-positive resistance — pointing into a cycle without being in it ───

#[test]
fn pointer_into_cycle_not_tagged_recursive() {
    // A<->B cycle (with termination guards). Z has sub a = A() — no path from A back to Z.
    // Z points INTO the A<->B cycle but is not a member of it.
    // Tarjan's SCC correctly excludes Z: mutual reachability is required.
    // Z can reach A, but A cannot reach Z, so Z is in its own singleton SCC with no self-edge.
    // Z's sub needs no guard because Z is not in a cyclic SCC (the termination check
    // only inspects subs whose target is in the same cyclic SCC as the template).
    let source = r#"
        structure A {
            param n : Int = 3
            sub b = B(n: n - 1) where n > 0
        }
        structure B {
            param n : Int = 3
            sub a = A(n: n - 1) where n > 0
        }
        structure Z {
            sub a = A()
        }
    "#;
    let compiled = parse_and_compile(source);

    let a_template = find_template(&compiled.templates, "A").expect("template A");
    let b_template = find_template(&compiled.templates, "B").expect("template B");
    let z_template = find_template(&compiled.templates, "Z").expect("template Z");

    assert!(
        a_template.is_recursive,
        "A should be recursive (in A<->B cycle)"
    );
    assert!(
        b_template.is_recursive,
        "B should be recursive (in A<->B cycle)"
    );
    assert!(
        !z_template.is_recursive,
        "Z should NOT be recursive — it points into the A<->B cycle but is not a member of it; \
         naive in_cycle propagation would wrongly tag Z because its neighbor A is in_cycle"
    );
}
