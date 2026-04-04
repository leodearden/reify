//! Recursive structure detection tests (Task 203).
//!
//! Tests the `detect_recursive_structures()` post-pass behavior:
//! 1. Self-referencing structures are tagged `is_recursive = true`.
//! 2. Non-recursive structures remain `is_recursive = false`.
//! 3. Indirect cycles (2-node, 3-node) tag all participants.
//! 4. Non-cyclic graph patterns (linear chains, diamonds) are correctly excluded.
//! 5. Multiple independent cycles are each detected and diagnosed separately.
//! 6. Warning diagnostics include correct cycle path strings.
//! 7. Collection sub form (`sub items : List<S>`) creates reference edges.
//! 8. References to unknown structures do not crash.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning all templates and diagnostics.
fn compile_all(source: &str) -> (Vec<TopologyTemplate>, Vec<Diagnostic>) {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    (compiled.templates, compiled.diagnostics)
}

/// Helper: find a template by name in a list of templates.
fn find_template<'a>(templates: &'a [TopologyTemplate], name: &str) -> &'a TopologyTemplate {
    templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("expected template named '{}'", name))
}

// ─── Step 1: direct self-reference and non-recursive baseline ─────────────────

/// A structure that references itself via a sub should have `is_recursive == true`.
#[test]
fn direct_self_reference_sets_is_recursive() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        s.is_recursive,
        "structure S with self-referencing sub should have is_recursive == true"
    );
}

/// A structure with only params and lets (no subs) should NOT be recursive.
#[test]
fn no_subs_not_recursive() {
    let source = r#"
structure S {
    param n : Int = 5
    let doubled : Int = n * 2
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        !s.is_recursive,
        "structure S with no subs should have is_recursive == false"
    );
}

// ─── Step 3: indirect cycles (2-node and 3-node) ─────────────────────────────

/// Mutual recursion: A references B, B references A — both should be tagged recursive.
#[test]
fn indirect_two_node_cycle_both_recursive() {
    let source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let a = find_template(&templates, "A");
    let b = find_template(&templates, "B");

    assert!(
        a.is_recursive,
        "A in A<->B cycle should have is_recursive == true"
    );
    assert!(
        b.is_recursive,
        "B in A<->B cycle should have is_recursive == true"
    );
}

/// Three-node cycle: A -> B -> C -> A — all three should be tagged recursive.
#[test]
fn indirect_three_node_cycle_all_recursive() {
    let source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub c = C(n: n - 1) where n > 0
}
structure C {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let a = find_template(&templates, "A");
    let b = find_template(&templates, "B");
    let c = find_template(&templates, "C");

    assert!(
        a.is_recursive,
        "A in A->B->C->A cycle should have is_recursive == true"
    );
    assert!(
        b.is_recursive,
        "B in A->B->C->A cycle should have is_recursive == true"
    );
    assert!(
        c.is_recursive,
        "C in A->B->C->A cycle should have is_recursive == true"
    );
}

// ─── Step 5: non-cyclic graph patterns ───────────────────────────────────────

/// Linear chain: A -> B -> C with no back-edges — none should be recursive.
#[test]
fn linear_chain_no_cycle() {
    let source = r#"
structure C { param x : Int = 1 }
structure B { sub c = C() }
structure A { sub b = B() }
"#;

    let (templates, _diagnostics) = compile_all(source);
    let a = find_template(&templates, "A");
    let b = find_template(&templates, "B");
    let c = find_template(&templates, "C");

    assert!(
        !a.is_recursive,
        "A in linear chain A->B->C should not be recursive"
    );
    assert!(
        !b.is_recursive,
        "B in linear chain A->B->C should not be recursive"
    );
    assert!(
        !c.is_recursive,
        "C in linear chain A->B->C should not be recursive"
    );
}

/// Diamond: A->B, A->C, B->D, C->D (shared leaf, no back-edge) — none recursive.
#[test]
fn diamond_no_cycle() {
    let source = r#"
structure D { param x : Int = 1 }
structure B { sub d = D() }
structure C { sub d = D() }
structure A {
    sub b = B()
    sub c = C()
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let a = find_template(&templates, "A");
    let b = find_template(&templates, "B");
    let c = find_template(&templates, "C");
    let d = find_template(&templates, "D");

    assert!(!a.is_recursive, "A in diamond should not be recursive");
    assert!(!b.is_recursive, "B in diamond should not be recursive");
    assert!(!c.is_recursive, "C in diamond should not be recursive");
    assert!(!d.is_recursive, "D in diamond should not be recursive");
}

// ─── Step 7: multiple independent cycles and mixed topologies ────────────────

/// Two separate cycles: {A<->B} and {C<->D} — all four should be recursive,
/// and two separate warning diagnostics should be emitted.
#[test]
fn multiple_independent_cycles() {
    let source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
structure C {
    param n : Int = 5
    sub d = D(n: n - 1) where n > 0
}
structure D {
    param n : Int = 5
    sub c = C(n: n - 1) where n > 0
}
"#;

    let (templates, diagnostics) = compile_all(source);
    let a = find_template(&templates, "A");
    let b = find_template(&templates, "B");
    let c = find_template(&templates, "C");
    let d = find_template(&templates, "D");

    assert!(a.is_recursive, "A in A<->B cycle should be recursive");
    assert!(b.is_recursive, "B in A<->B cycle should be recursive");
    assert!(c.is_recursive, "C in C<->D cycle should be recursive");
    assert!(d.is_recursive, "D in C<->D cycle should be recursive");

    // Expect exactly 2 warning diagnostics (one per SCC)
    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();
    assert_eq!(
        cycle_warnings.len(),
        2,
        "expected 2 cycle warnings (one per SCC), got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Mixed: cycle A<->B plus non-cycling structure C that references A.
/// A and B should be recursive, C should not.
#[test]
fn mixed_recursive_and_non_recursive() {
    let source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
structure C {
    sub a = A()
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let a = find_template(&templates, "A");
    let b = find_template(&templates, "B");
    let c = find_template(&templates, "C");

    assert!(a.is_recursive, "A in A<->B cycle should be recursive");
    assert!(b.is_recursive, "B in A<->B cycle should be recursive");
    assert!(
        !c.is_recursive,
        "C (references A but not in cycle) should NOT be recursive"
    );
}

// ─── Step 9: warning diagnostic content and count ────────────────────────────

/// A self-referencing cycle should produce exactly one warning containing 'recursive structure
/// cycle detected' and the cycle path 'S -> S'.
#[test]
fn warning_diagnostic_emitted_for_cycle() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("recursive structure cycle detected")
        })
        .collect();

    assert_eq!(
        cycle_warnings.len(),
        1,
        "expected exactly 1 cycle warning for self-referencing S, got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // The warning should contain the cycle path "S -> S"
    assert!(
        cycle_warnings[0].message.contains("S -> S"),
        "cycle warning should contain path 'S -> S', got: {}",
        cycle_warnings[0].message
    );
}

/// Two independent cycles should produce exactly two warning diagnostics.
#[test]
fn warning_diagnostic_count_matches_scc_count() {
    let source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
structure C {
    param n : Int = 5
    sub d = D(n: n - 1) where n > 0
}
structure D {
    param n : Int = 5
    sub c = C(n: n - 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("recursive structure cycle detected")
        })
        .collect();

    assert_eq!(
        cycle_warnings.len(),
        2,
        "expected 2 cycle warnings for 2 independent SCCs, got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ─── Step 11: collection sub and unknown structure ───────────────────────────

/// A structure using collection form `sub items : List<S>` should create a reference
/// edge and be tagged recursive (self-referential through collection).
#[test]
fn collection_sub_creates_reference_edge() {
    let source = r#"
structure S {
    param n : Int = 5
    sub items : List<S>
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        s.is_recursive,
        "structure S with collection sub `sub items : List<S>` should have is_recursive == true"
    );
}

/// A structure with a sub referencing an undefined structure should not crash
/// and should not be tagged recursive (the reference produces no edge).
#[test]
fn sub_referencing_unknown_structure_not_recursive() {
    let source = r#"
structure S {
    sub x = Unknown()
}
"#;

    let (templates, diagnostics) = compile_all(source);

    // S might not even compile successfully, but the detection phase should not panic.
    // If S exists in the templates, it should not be recursive.
    if let Some(s) = templates.iter().find(|t| t.name == "S") {
        assert!(
            !s.is_recursive,
            "S referencing unknown structure should not be tagged recursive"
        );
    }

    // The compilation might produce error diagnostics about 'Unknown', that's fine.
    // What matters is no panic occurred.
    let _ = diagnostics;
}

// ─── Task 362: source labels on cycle warnings ────────────────────────────────

/// Cycle warnings should include source labels so users can see exactly which
/// sub-component declarations form the cycle.
#[test]
fn cycle_warning_has_source_labels() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();

    assert_eq!(cycle_warnings.len(), 1, "expected exactly 1 cycle warning");

    let warning = cycle_warnings[0];
    assert!(
        !warning.labels.is_empty(),
        "cycle warning should have at least one source label, got none"
    );
    assert!(
        warning.labels.iter().any(|l| l.message.contains("references")),
        "at least one label should mention 'references', got: {:?}",
        warning.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
}
