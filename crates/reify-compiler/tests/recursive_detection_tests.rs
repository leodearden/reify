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
use reify_test_support::compile_source;
use reify_core::*;
use reify_ir::*;

/// Helper: compile and destructure into templates + diagnostics.
fn compile_all(source: &str) -> (Vec<TopologyTemplate>, Vec<Diagnostic>) {
    let compiled = compile_source(source);
    (compiled.templates, compiled.diagnostics)
}

/// Helper: find a template by name in a list of templates.
///
/// Delegates to `reify_compiler::find_template` and panics if the template is absent —
/// preserving the clear panic messages at the 31 call sites without changing their spelling.
/// The fully-qualified path avoids shadowing the local name with the glob import.
fn find_template<'a>(templates: &'a [TopologyTemplate], name: &str) -> &'a TopologyTemplate {
    reify_compiler::find_template(templates, name)
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

    let (templates, diagnostics) = compile_all(source);
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

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();
    assert_eq!(
        cycle_warnings.len(),
        1,
        "expected exactly 1 cycle warning for A<->B, got: {:?}",
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        cycle_warnings[0].message.contains(" -> "),
        "cycle warning message should contain ' -> ' arrow separator, got: {}",
        cycle_warnings[0].message
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

/// A 2-node mutual cycle (A<->B) exercises the `find_cycle_back_to` code path
/// for multi-node SCCs. The diagnostic message should contain either "A -> B -> A"
/// or "B -> A -> B" depending on Tarjan's stack pop order.
#[test]
fn multi_node_cycle_path_in_diagnostic() {
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
        "expected exactly 1 cycle warning for A<->B mutual cycle, got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Tarjan pops nodes in reverse discovery order, so either start is valid
    let msg = &cycle_warnings[0].message;
    assert!(
        msg.contains("A -> B -> A") || msg.contains("B -> A -> B"),
        "cycle warning should contain 'A -> B -> A' or 'B -> A -> B', got: {}",
        msg
    );
}

/// Mixed SCC sizes: a 3-node cycle (X->Y->Z->X) plus a self-loop (W->W).
/// Each SCC should produce exactly one warning, and all four structures should be recursive.
#[test]
fn mixed_scc_sizes_each_produce_warning() {
    let source = r#"
structure X {
    param n : Int = 5
    sub y = Y(n: n - 1) where n > 0
}
structure Y {
    param n : Int = 5
    sub z = Z(n: n - 1) where n > 0
}
structure Z {
    param n : Int = 5
    sub x = X(n: n - 1) where n > 0
}
structure W {
    param n : Int = 5
    sub w = W(n: n - 1) where n > 0
}
"#;

    let (templates, diagnostics) = compile_all(source);
    let x = find_template(&templates, "X");
    let y = find_template(&templates, "Y");
    let z = find_template(&templates, "Z");
    let w = find_template(&templates, "W");

    assert!(x.is_recursive, "X in X->Y->Z->X cycle should be recursive");
    assert!(y.is_recursive, "Y in X->Y->Z->X cycle should be recursive");
    assert!(z.is_recursive, "Z in X->Y->Z->X cycle should be recursive");
    assert!(w.is_recursive, "W with self-loop should be recursive");

    // Each SCC (the 3-node cycle and the self-loop) produces exactly one warning
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
        "expected 2 cycle warnings (one per SCC), got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // The self-loop SCC warning should contain "W -> W"
    assert!(
        cycle_warnings.iter().any(|d| d.message.contains("W -> W")),
        "expected one warning to contain 'W -> W', got: {:?}",
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

    let (templates, diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        s.is_recursive,
        "structure S with collection sub `sub items : List<S>` should have is_recursive == true"
    );

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
        "expected exactly 1 cycle warning for collection self-reference, got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    assert!(
        cycle_warnings[0].message.contains("S -> S"),
        "cycle warning should contain 'S -> S', got: {}",
        cycle_warnings[0].message
    );
}

/// A structure with a sub referencing an undefined structure should not crash,
/// should still appear in the compiled templates, and must not be tagged recursive
/// (the unresolved reference produces no dependency edge).
///
/// Tightened by task 553 improvement #1: assert S is present (hard check) rather
/// than silently passing if the detection pass drops S on unknown refs.
#[test]
fn sub_referencing_unknown_structure_not_recursive() {
    let source = r#"
structure S {
    sub x = Unknown()
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        !s.is_recursive,
        "S referencing unknown structure should not be tagged recursive"
    );
}

// ─── Task 362: cycle path format and warning count ───────────────────────────

/// The cycle path in a mutual-recursion warning must use ' -> ' arrow separators
/// and include both node names.
#[test]
fn mutual_recursion_cycle_path_format() {
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

    let (_templates, diagnostics) = compile_all(source);

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();

    assert_eq!(
        cycle_warnings.len(),
        1,
        "expected 1 cycle warning for A<->B"
    );

    let msg = &cycle_warnings[0].message;
    assert!(
        msg.contains(" -> "),
        "cycle warning message should contain ' -> ' arrow separator, got: {msg}"
    );
    // The path must mention both A and B
    assert!(
        msg.contains('A') && msg.contains('B'),
        "cycle path should mention both A and B, got: {msg}"
    );
    // The full closed path must appear: cycle closes by repeating the start node.
    // Task 553 improvement #2: tightens from "A and B present" to exact path substring.
    assert!(
        msg.contains("A -> B -> A") || msg.contains("B -> A -> B"),
        "cycle warning should contain full closed path 'A -> B -> A' (or 'B -> A -> B'), got: {msg}"
    );
}

/// A three-node cycle emits exactly one warning (one SCC) with ' -> ' separators.
#[test]
fn three_node_cycle_emits_exactly_one_warning() {
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

    let (_templates, diagnostics) = compile_all(source);

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("recursive structure cycle")
        })
        .collect();

    assert_eq!(
        cycle_warnings.len(),
        1,
        "expected exactly 1 cycle warning for A->B->C->A, got {}: {:?}",
        cycle_warnings.len(),
        cycle_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        cycle_warnings[0].message.contains(" -> "),
        "cycle warning should use ' -> ' arrow separator, got: {}",
        cycle_warnings[0].message
    );
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
        warning
            .labels
            .iter()
            .any(|l| l.message.contains("references")),
        "at least one label should mention 'references', got: {:?}",
        warning
            .labels
            .iter()
            .map(|l| &l.message)
            .collect::<Vec<_>>()
    );
}

// ─── Task 367: is_recursive mixed into content_hash ──────────────────────────

/// A recursive template's content_hash must differ from an identical template
/// that is not recursive. This verifies that `is_recursive` is mixed into the hash,
/// preventing incorrect incremental compilation cache hits.
///
/// Module 1 (cyclic): A references B, B references A  → A.is_recursive = true
/// Module 2 (acyclic): A references B, B references C → A.is_recursive = false
///
/// Template A has identical raw content in both modules. Before the fix, both A
/// templates have the same content_hash. After the fix they must differ.
///
/// ## Implicit assumption
///
/// This test relies on A's `content_hash` being a function of A's OWN compiled
/// content plus (when recursive) the `"is_recursive"` domain tag — NOT a
/// transitive closure that also incorporates B's resolved content_hash. If the
/// hash ever evolved to transitively include resolved-sub-component hashes, the
/// `assert_ne!` below would pass for a different (and stronger) reason, and
/// this test would no longer isolate the `is_recursive` remix contract.
#[test]
fn is_recursive_mixed_into_content_hash() {
    // Module 1: A<->B mutual cycle — A.is_recursive = true
    let cyclic_source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
"#;

    // Module 2: A->B->C acyclic — A.is_recursive = false
    let acyclic_source = r#"
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
}
"#;

    let (cyclic_templates, _) = compile_all(cyclic_source);
    let (acyclic_templates, _) = compile_all(acyclic_source);

    let a_cyclic = find_template(&cyclic_templates, "A");
    let a_acyclic = find_template(&acyclic_templates, "A");

    // Sanity checks: recursion flag is set correctly.
    assert!(
        a_cyclic.is_recursive,
        "A in A<->B cycle should have is_recursive == true"
    );
    assert!(
        !a_acyclic.is_recursive,
        "A in acyclic A->B->C should have is_recursive == false"
    );

    // The content_hash MUST differ because is_recursive differs.
    // Before the fix both hashes are equal (is_recursive not mixed in),
    // so this assertion fails before the fix.
    assert_ne!(
        a_cyclic.content_hash, a_acyclic.content_hash,
        "template A with is_recursive=true must have a different content_hash \
         than the same template with is_recursive=false (incremental correctness)"
    );
}

/// When template A has identical source content and `is_recursive = true` in
/// two different modules, A's `content_hash` must be **equal** even though B's
/// content differs between the modules.
///
/// This is the companion equality case for [`is_recursive_mixed_into_content_hash`].
/// It directly pins the "non-transitive-closure" assumption documented there:
/// B's resolved `content_hash` must NOT flow into A's `content_hash`. If it
/// did, A's hashes would differ here solely because of B's changed content —
/// and the incremental cache would incorrectly invalidate A when B changes.
#[test]
fn recursive_a_hash_independent_of_b_content() {
    // Both modules have A<->B mutual cycles, so A.is_recursive = true in both.
    // A's source is identical in both modules.
    // B's source differs (extra param `x`) — if B's hash leaked into A's,
    // a_v1.content_hash != a_v2.content_hash, which would be wrong.
    let source_v1 = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
"#;

    let source_v2 = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    param x : Int = 99
    sub a = A(n: n - 1) where n > 0
}
"#;

    let (templates_v1, _) = compile_all(source_v1);
    let (templates_v2, _) = compile_all(source_v2);

    let a_v1 = find_template(&templates_v1, "A");
    let a_v2 = find_template(&templates_v2, "A");

    // Sanity: A is recursive in both modules.
    assert!(a_v1.is_recursive, "A in v1 should be recursive");
    assert!(a_v2.is_recursive, "A in v2 should be recursive");

    // A's content_hash must be equal: same source, same is_recursive flag.
    // B's changed content must NOT propagate into A's hash.
    assert_eq!(
        a_v1.content_hash, a_v2.content_hash,
        "template A with identical source and is_recursive=true must have the same \
         content_hash regardless of B's content (B's resolved hash must not flow into A)"
    );
}

/// A non-recursive template's content_hash must be identical whether or not it
/// appears alongside a recursive cycle in the same module.
/// This is a regression guard: the remix step must only touch recursive templates,
/// never non-recursive ones (which would unnecessarily invalidate existing caches).
#[test]
fn non_recursive_template_hash_unaffected_by_other_cycles() {
    // Module 1: A<->B mutual cycle plus an independent C
    let combined_source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
structure C {
    param x : Int = 10
}
"#;

    // Module 2: C alone (no cycle context)
    let standalone_source = r#"
structure C {
    param x : Int = 10
}
"#;

    let (combined_templates, _) = compile_all(combined_source);
    let (standalone_templates, _) = compile_all(standalone_source);

    let c_combined = find_template(&combined_templates, "C");
    let c_standalone = find_template(&standalone_templates, "C");

    // Sanity: C is not recursive in either case.
    assert!(
        !c_combined.is_recursive,
        "C alongside A<->B should not be recursive"
    );
    assert!(
        !c_standalone.is_recursive,
        "C compiled alone should not be recursive"
    );

    // C's hash must be identical in both compilations.
    assert_eq!(
        c_combined.content_hash, c_standalone.content_hash,
        "non-recursive template C must have the same content_hash whether or not \
         it appears in a module that also contains a recursive cycle"
    );
}

/// The module-level content_hash must differ between two modules that have
/// different recursion topology. Since template hashes feed into the module hash,
/// the remix of is_recursive at the template level propagates through to the
/// module level — ensuring incremental compilation correctly invalidates at
/// the module granularity too.
#[test]
fn module_hash_changes_when_recursion_topology_changes() {
    // Module 1: A<->B mutual cycle (A.is_recursive = true, B.is_recursive = true)
    let cyclic_source = r#"
structure A {
    param n : Int = 5
    sub b = B(n: n - 1) where n > 0
}
structure B {
    param n : Int = 5
    sub a = A(n: n - 1) where n > 0
}
"#;

    // Module 2: A->B->C linear acyclic (nothing is recursive)
    let acyclic_source = r#"
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
}
"#;

    let cyclic_module = compile_source(cyclic_source);
    let acyclic_module = compile_source(acyclic_source);

    // The module hashes must differ: the cyclic module has recursive templates
    // whose hashes were remixed, so the aggregated module hash differs.
    assert_ne!(
        cyclic_module.content_hash, acyclic_module.content_hash,
        "module content_hash must differ between cyclic and acyclic topology \
         (is_recursive remix propagates to module level)"
    );
}
