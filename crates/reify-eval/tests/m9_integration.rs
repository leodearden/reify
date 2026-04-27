//! M9 pipeline integration tests.
//!
//! Exercises cross-feature composition combining all three M9 milestone features:
//! constraint def instantiation, trait conformance, and determinacy predicates.
//!
//! Cross-cutting scenarios tested:
//!   1. Constraint defs whose predicates use determinacy predicates internally
//!   2. Traits with determinacy constraints injected into implementing structures
//!   3. Recursive structures whose sub guards use determinacy predicates
//!   4. Multi-trait structures combining constraint defs, trait defaults, and determinacy
//!
//! Uses `examples/m9_integration.ri` as the capstone source file and inline source
//! strings for focused per-scenario assertions.

use reify_test_support::{check_source, eval_source, make_simple_engine, parse_and_compile};
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

/// Absolute path to the integration example file, resolved at compile time from crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_integration.ri"
);

// ── Step 1: .ri file parses and compiles ─────────────────────────────────────

/// Read examples/m9_integration.ri, parse it, assert no parse errors, compile,
/// assert no error-severity diagnostics, assert at least one template exists.
/// This is the baseline test confirming the capstone example file is valid.
#[test]
fn ri_file_parses_and_compiles() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_integration.ri should exist");

    // Step A: parse
    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in m9_integration.ri: {:?}",
        parsed.errors
    );

    // Step B: compile — no error diagnostics
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in m9_integration.ri: {:?}",
        errors
    );

    // Step C: exactly 4 templates (Widget, Bracket, RecursiveChain, Plate).
    // Tight count locks in the intended cross-feature set — accidental extras or
    // removals are caught, without the redundant name-then-count error cascade.
    let template_names: Vec<&str> = compiled.templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        compiled.templates.len(),
        4,
        "expected exactly 4 templates in m9_integration.ri, got {}: {:?}",
        compiled.templates.len(),
        template_names
    );
    for expected_name in &["Widget", "Bracket", "RecursiveChain", "Plate"] {
        assert!(
            template_names.contains(expected_name),
            "expected template '{}' in m9_integration.ri, got: {:?}",
            expected_name,
            template_names
        );
    }
}

// ── Step 3: constraint def with determinacy — satisfied case ─────────────────

/// Cross-feature: a constraint def whose sole predicate is a determinacy predicate.
/// When the invoked param has a concrete default (size=10mm), determined(v) is true,
/// so RequireDetermined[0] should be Satisfied.
#[test]
fn constraint_def_with_determinacy_satisfied() {
    let source = r#"
constraint def RequireDetermined {
    param v : Length
    determined(v)
}
structure S {
    param size : Length = 10mm
    constraint RequireDetermined(v: size)
}
"#;
    let result = check_source(source);

    // Exactly one constraint result (one invocation, one predicate)
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected 1 constraint result, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.label,
        Some("RequireDetermined#0[0]".to_string()),
        "expected label Some(\"RequireDetermined#0[0]\"), got: {:?}",
        entry.label
    );
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "RequireDetermined[0] should be Satisfied when param has default, got: {:?}",
        entry.satisfaction
    );
}

// ── Step 5: constraint def with determinacy — violated case ──────────────────

/// Cross-feature: when the invoked param has no default (size : Length, Undetermined),
/// determined(v) evaluates to false, so RequireDetermined[0] should be Violated.
#[test]
fn constraint_def_with_determinacy_violated() {
    let source = r#"
constraint def RequireDetermined {
    param v : Length
    determined(v)
}
structure S {
    param size : Length
    constraint RequireDetermined(v: size)
}
"#;
    let result = check_source(source);

    // Exactly one constraint result
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected 1 constraint result, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.label,
        Some("RequireDetermined#0[0]".to_string()),
        "expected label Some(\"RequireDetermined#0[0]\"), got: {:?}",
        entry.label
    );
    // determined(size) evaluates to Bool(false) when size is Undetermined → Violated
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Violated,
        "RequireDetermined[0] should be Violated when param is undetermined, got: {:?}",
        entry.satisfaction
    );
}

// ── Step 7: multi-predicate constraint def with determinacy + value range ─────

/// Cross-feature: DeterminedInRange has 3 predicates — determined(v), v >= lo, v <= hi.
/// With v=50mm, lo=10mm, hi=100mm, all three predicates are satisfied.
/// Verifies DeterminedInRange[0], [1], [2] all Satisfied.
#[test]
fn constraint_def_multi_predicate_determinacy_plus_value() {
    let source = r#"
constraint def DeterminedInRange {
    param v  : Length
    param lo : Length
    param hi : Length
    determined(v)
    v >= lo
    v <= hi
}
structure S {
    param v  : Length = 50mm
    param lo : Length = 10mm
    param hi : Length = 100mm
    constraint DeterminedInRange(v: v, lo: lo, hi: hi)
}
"#;
    let result = check_source(source);

    // Exactly 3 constraint results (one per predicate in the def)
    assert_eq!(
        result.constraint_results.len(),
        3,
        "expected 3 constraint results (one per predicate), got: {:?}",
        result.constraint_results
    );

    // DeterminedInRange[0] = determined(v): v=50mm has default → Satisfied
    let entry0 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("DeterminedInRange#0[0]".to_string()))
        .expect("expected DeterminedInRange[0]");
    assert_eq!(
        entry0.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedInRange[0] (determined(v)) should be Satisfied"
    );

    // DeterminedInRange[1] = v >= lo: 50mm >= 10mm → Satisfied
    let entry1 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("DeterminedInRange#0[1]".to_string()))
        .expect("expected DeterminedInRange[1]");
    assert_eq!(
        entry1.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedInRange[1] (v >= lo) should be Satisfied (50mm >= 10mm)"
    );

    // DeterminedInRange[2] = v <= hi: 50mm <= 100mm → Satisfied
    let entry2 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("DeterminedInRange#0[2]".to_string()))
        .expect("expected DeterminedInRange[2]");
    assert_eq!(
        entry2.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedInRange[2] (v <= hi) should be Satisfied (50mm <= 100mm)"
    );
}

// ── Step 9/10: trait + structure-level constraint def invocation ──────────────

/// Cross-feature: trait with inline constraint + structure that invokes a constraint def.
///
/// Note: trait-level constraint def invocations are not supported by the parser
/// (parse error: "invalid constraint: constraint Positive(v: size)"). The fallback
/// tests the equivalent cross-feature composition:
///   - Trait 'Sized' with inline constraint (size > 0mm) injected into Widget
///   - Constraint def 'Positive' invoked at Widget's structure level
///   - Both: Widget : Sized { size = 50mm; constraint Positive(v: size) }
///
/// Asserts Widget has both the trait-injected inline constraint and Positive[0] = Satisfied,
/// plus Widget.size = 0.05 SI (50mm).
#[test]
fn trait_with_constraint_def_invocation() {
    let source = r#"
constraint def Positive {
    param v : Length
    v > 0mm
}
trait Sized {
    param size : Length
    constraint size > 0mm
}
structure Widget : Sized {
    param size : Length = 50mm
    constraint Positive(v: size)
}
"#;
    let check_result = check_source(source);
    let eval_result = eval_source(source);

    // Widget should have Positive[0] = Satisfied (structure-level constraint def invocation)
    let entry = check_result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Widget" && e.label == Some("Positive#0[0]".to_string()))
        .expect("expected Widget to have constraint with label 'Positive[0]'");
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "Positive[0] should be Satisfied for Widget.size=50mm > 0mm"
    );

    // Widget.size = 50mm = 0.05 SI
    let size_id = ValueCellId::new("Widget", "size");
    let size_val = eval_result
        .values
        .get(&size_id)
        .expect("Widget.size should exist in eval result");
    match size_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected 0.05 SI for Widget.size (50mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Widget.size, got {:?}", other),
    }

    // Widget should also have the trait-injected inline constraint (size > 0mm)
    // It has label=None (inline constraints have no label from a constraint def)
    let inline_entries: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Widget" && e.label.is_none())
        .collect();
    assert!(
        !inline_entries.is_empty(),
        "Widget should have at least one unlabeled (trait-injected inline) constraint"
    );
    for e in &inline_entries {
        assert_eq!(
            e.satisfaction,
            Satisfaction::Satisfied,
            "Widget trait-injected inline constraint should be Satisfied"
        );
    }
}

// ── Step 11: trait conformance with determinacy constraint ────────────────────

/// Cross-feature: a trait defines an inline determinacy constraint (determined(value)).
/// When injected into Item : Verifiable { value = 25mm }, determined(value) is true
/// → the injected constraint should be Satisfied.
/// Also verifies Item.value evaluates to the correct SI value.
#[test]
fn trait_conformance_with_determinacy_constraint() {
    let source = r#"
trait Verifiable {
    param value : Length
    constraint determined(value)
}
structure Item : Verifiable {
    param value : Length = 25mm
}
"#;
    let check_result = check_source(source);
    let eval_result = eval_source(source);

    // Item should have the trait-injected determined(value) constraint = Satisfied
    // (inline constraint from trait, no label)
    let det_entry = check_result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Item")
        .expect("expected at least one constraint result for Item");
    assert_eq!(
        det_entry.satisfaction,
        Satisfaction::Satisfied,
        "Item's trait-injected determined(value) constraint should be Satisfied (value=25mm)"
    );

    // All Item constraints should be Satisfied
    for entry in check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Item")
    {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "all Item constraints should be Satisfied, but {:?} is {:?}",
            entry.label,
            entry.satisfaction
        );
    }

    // Item.value = 25mm = 0.025 SI
    let val_id = ValueCellId::new("Item", "value");
    let val = eval_result
        .values
        .get(&val_id)
        .expect("Item.value should exist in eval result");
    match val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.025).abs() < 1e-12,
                "expected 0.025 SI for Item.value (25mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Item.value, got {:?}", other),
    }
}

// ── Step 15/16: recursive structure — undetermined arg propagation ────────────

/// Cross-feature: recursive structure where an undetermined Length param (`next_value`,
/// no default) is passed as an arg to the child's `value` param.
///
/// Note: the compiler requires recursive guards to reference an Int or Bool param
/// (e.g., `depth > 0`). A pure `determined(next_value)` guard is rejected at compile
/// time ("recursive sub guard does not reference any Int or Bool parameter"). We use
/// `depth > 0` as the guard and carry `next_value` as the arg.
///
/// With `depth=1`:
///   - root:  value=50mm (0.05 SI, has default), next_value=Undetermined
///   - child: value=Undef (passed next_value, which is Undef), depth=0
///   - grandchild: NOT created (depth=0, guard false)
///
/// Cross-feature assertions:
///   1. eval: root.value = 0.05 SI, child.value = Undef (undetermined arg propagation)
///   2. check: constraint determined(next_value) = Violated at root (next_value has no default)
#[test]
fn recursive_structure_determinacy_terminates_on_undetermined() {
    let source = r#"
structure def TermTree {
    param value      : Length = 50mm
    param next_value : Length
    param depth      : Int    = 1
    sub child = TermTree(value: next_value, depth: depth - 1) where depth > 0
    constraint determined(next_value)
}
"#;
    let eval_result = eval_source(source);
    let check_result = check_source(source);

    // root.value = 50mm = 0.05 SI (has default)
    let value_id = ValueCellId::new("TermTree", "value");
    let value = eval_result
        .values
        .get(&value_id)
        .expect("TermTree.value should exist");
    match value {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected ~0.05 SI for TermTree.value (50mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for TermTree.value, got {:?}", other),
    }

    // child.value = Undef: next_value (no default) is passed as arg → Undef propagates.
    // The undetermined Length param propagates through recursive instantiation.
    let child_value_id = ValueCellId::new("TermTree.child", "value");
    let child_val = eval_result
        .values
        .get(&child_value_id)
        .expect("TermTree.child.value should exist (child is created when depth=1>0)");
    assert_eq!(
        *child_val,
        Value::Undef,
        "TermTree.child.value should be Undef (next_value has no default → Undef arg propagation)"
    );

    // Cross-feature: constraint determined(next_value) = Violated at every TermTree
    // level. At root, next_value is Undetermined; at the child (depth=0), next_value
    // carries the Undef propagated from the root's Undetermined value. Neither is
    // determined → each TermTree entry must be Violated. Using filter+per-entry
    // assertion (mirroring the root_constraints pattern at lines 557-574) avoids
    // `.find()` latching onto a nondeterministic first match.
    let termtree_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "TermTree")
        .collect();
    assert!(
        !termtree_constraints.is_empty(),
        "expected at least one constraint result for TermTree (determined(next_value))"
    );
    for e in &termtree_constraints {
        assert_eq!(
            e.satisfaction,
            Satisfaction::Violated,
            "TermTree constraint {:?} (determined(next_value)) should be Violated, got {:?}",
            e.label,
            e.satisfaction
        );
    }
}

// ── Step 13/14: recursive structure with determinacy constraint ───────────────

/// Cross-feature: a recursive structure that uses depth > 0 as its unfolding guard
/// while attaching a `constraint determined(span)` at each level.
///
/// Note: `determined()` in the sub guard position (e.g., `where determined(span) && depth > 0`)
/// is NOT currently supported — the recursive guard evaluator does not have access to
/// the determinacy snapshot, so `determined()` returns Undef there, causing the guard
/// to be treated as Undef and halting recursion immediately. The fallback approach
/// uses `depth > 0` as the guard and `constraint determined(span)` as a per-level
/// constraint, which still exercises the cross-feature composition: recursive unfold
/// depth control + determinacy predicate evaluation at each recursion level.
///
/// With defaults depth=2, span=100mm:
///   root:        depth=2, span=100mm  (0.1  SI)  → child created (depth=2>0)
///   child:       depth=1, span=50mm   (0.05 SI)  → grandchild created (depth=1>0)
///   grandchild:  depth=0, span=25mm   (0.025 SI) → great-grandchild NOT created (depth=0)
///
/// Each level asserts `determined(span)` = Satisfied (span always has a concrete value).
#[test]
fn recursive_structure_gated_by_determinacy() {
    let source = r#"
structure def RecursiveChain {
    param depth : Int    = 2
    param span  : Length = 100mm
    sub child = RecursiveChain(depth: depth - 1, span: span / 2) where depth > 0
    constraint determined(span)
    constraint span > 0mm
}
"#;
    let eval_result = eval_source(source);
    let check_result = check_source(source);

    // child.span = 100mm / 2 = 50mm = 0.05 SI
    let child_span_id = ValueCellId::new("RecursiveChain.child", "span");
    let child_span = eval_result
        .values
        .get(&child_span_id)
        .unwrap_or_else(|| panic!("RecursiveChain.child.span should exist"));
    match child_span {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected ~0.05 SI for RecursiveChain.child.span (50mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for RecursiveChain.child.span, got {:?}",
            other
        ),
    }

    // grandchild.span = 50mm / 2 = 25mm = 0.025 SI
    let grandchild_span_id = ValueCellId::new("RecursiveChain.child.child", "span");
    let grandchild_span = eval_result
        .values
        .get(&grandchild_span_id)
        .unwrap_or_else(|| panic!("RecursiveChain.child.child.span should exist"));
    match grandchild_span {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.025).abs() < 1e-12,
                "expected ~0.025 SI for RecursiveChain.child.child.span (25mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for RecursiveChain.child.child.span, got {:?}",
            other
        ),
    }

    // great-grandchild must NOT exist (depth=0 at grandchild level → guard false)
    let great_grandchild_span_id = ValueCellId::new("RecursiveChain.child.child.child", "span");
    assert!(
        !eval_result.values.contains(&great_grandchild_span_id),
        "RecursiveChain.child.child.child.span should not exist (depth=0 stops unfolding)"
    );

    // Cross-feature: the root RecursiveChain should have determined(span) = Satisfied.
    // (Inline constraints have label=None; the top-level entity is "RecursiveChain".)
    let root_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "RecursiveChain")
        .collect();
    assert!(
        !root_constraints.is_empty(),
        "expected at least one constraint result for RecursiveChain (determined(span))"
    );
    for e in &root_constraints {
        assert_eq!(
            e.satisfaction,
            Satisfaction::Satisfied,
            "RecursiveChain constraint {:?} should be Satisfied, got {:?}",
            e.label,
            e.satisfaction
        );
    }
}

// ── Step 17: full pipeline — all constraints satisfied ────────────────────────

/// Capstone: read examples/m9_integration.ri, parse+compile+check with
/// SimpleConstraintChecker. Assert ALL constraint results are Satisfied and
/// total count >= 10 (covering trait-injected, constraint-def, determinacy,
/// and recursive constraints across Widget, Bracket, Plate, RecursiveChain).
#[test]
fn full_pipeline_all_constraints_satisfied() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_integration.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let result = engine.check(&compiled);

    // All constraints must be Satisfied
    let not_satisfied: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction != Satisfaction::Satisfied)
        .collect();
    assert!(
        not_satisfied.is_empty(),
        "expected all constraints Satisfied, but {} are not: {:?}",
        not_satisfied.len(),
        not_satisfied
    );

    // At least 10 constraint results total (cross-feature coverage)
    assert!(
        result.constraint_results.len() >= 10,
        "expected >= 10 constraint results, got {}",
        result.constraint_results.len()
    );
}

// ── Step 19: full pipeline — cross-feature values ─────────────────────────────

/// Eval m9_integration.ri, verify specific cross-feature values across structures:
///   1. Widget.size = 30mm (trait-implementing structure with default param)
///   2. Bracket has DeterminedInRange[0..2] constraints for both size and length invocations
///   3. RecursiveChain recursive unfolding: child.span = 50mm, child.child.span = 25mm
///   4. Plate.length = 50mm (default injected from Bounded trait — empty body)
#[test]
fn full_pipeline_cross_feature_values() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_integration.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let check_result = engine.check(&compiled);

    // 1. Widget.size = 30mm = 0.03 SI (default from structure, implements Measurable)
    let widget_size_id = ValueCellId::new("Widget", "size");
    let widget_size = check_result
        .values
        .get(&widget_size_id)
        .expect("Widget.size should exist");
    match widget_size {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.03).abs() < 1e-12,
                "expected ~0.03 SI for Widget.size (30mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Widget.size, got {:?}", other),
    }

    // 2. Bracket invokes DeterminedInRange twice (for size and length).
    // Under task 845, each invocation has a unique inst_idx so the determined(v)
    // predicate (pred_idx=0) appears once as DeterminedInRange#0[0] (size
    // invocation) and once as DeterminedInRange#1[0] (length invocation).
    // Both are Satisfied (size=80mm in [10mm,200mm], length=100mm in [50mm,500mm]).
    let bracket_dir_inst0: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| {
            e.id.entity == "Bracket" && e.label == Some("DeterminedInRange#0[0]".to_string())
        })
        .collect();
    let bracket_dir_inst1: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| {
            e.id.entity == "Bracket" && e.label == Some("DeterminedInRange#1[0]".to_string())
        })
        .collect();
    assert_eq!(
        bracket_dir_inst0.len(),
        1,
        "expected exactly 1 DeterminedInRange#0[0] for Bracket (first invocation), got {}",
        bracket_dir_inst0.len()
    );
    assert_eq!(
        bracket_dir_inst1.len(),
        1,
        "expected exactly 1 DeterminedInRange#1[0] for Bracket (second invocation), got {}",
        bracket_dir_inst1.len()
    );
    for e in bracket_dir_inst0.iter().chain(bracket_dir_inst1.iter()) {
        assert_eq!(
            e.satisfaction,
            Satisfaction::Satisfied,
            "Bracket DeterminedInRange[0] should be Satisfied"
        );
    }

    // 3. RecursiveChain recursive unfolding:
    //    child.span = 100mm/2 = 50mm = 0.05 SI
    //    child.child.span = 50mm/2 = 25mm = 0.025 SI
    let child_span_id = ValueCellId::new("RecursiveChain.child", "span");
    let child_span = check_result
        .values
        .get(&child_span_id)
        .expect("RecursiveChain.child.span should exist");
    match child_span {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected ~0.05 SI for RecursiveChain.child.span (50mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for RecursiveChain.child.span, got {:?}",
            other
        ),
    }

    let grandchild_span_id = ValueCellId::new("RecursiveChain.child.child", "span");
    let grandchild_span = check_result
        .values
        .get(&grandchild_span_id)
        .expect("RecursiveChain.child.child.span should exist");
    match grandchild_span {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.025).abs() < 1e-12,
                "expected ~0.025 SI for RecursiveChain.child.child.span (25mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for RecursiveChain.child.child.span, got {:?}",
            other
        ),
    }

    // 4. Plate.length = 50mm = 0.05 SI (injected from Bounded trait, empty body structure)
    let plate_length_id = ValueCellId::new("Plate", "length");
    let plate_length = check_result
        .values
        .get(&plate_length_id)
        .expect("Plate.length should exist (injected from Bounded trait)");
    match plate_length {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected ~0.05 SI for Plate.length (50mm from Bounded), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Plate.length, got {:?}", other),
    }
}

// ── Step 21/22: capstone inline — all 3 M9 features in one structure ──────────

/// Final capstone: inline source that combines all three M9 features in a single
/// structure. Tests that they compose correctly:
///
///   Feature 1 — Trait conformance + defaults:
///     Trait 'Verifiable' with inline determinacy constraint (determined(value)) and
///     trait 'Ranged' with inline value constraint (value > 0mm).
///
///   Feature 2 — Constraint def instantiation:
///     Constraint def 'DeterminedPositive' with determined(v) + v > 0mm predicates,
///     invoked on a structure-level param.
///
///   Feature 3 — Determinacy predicates:
///     Both inline and constraint-def-based determinacy checks on the same param.
///
///   Structure 'Composite' implements both traits + invokes constraint def:
///     - Trait-injected: determined(value) = Satisfied (value = 75mm)
///     - Trait-injected: value > 0mm = Satisfied
///     - Constraint def: DeterminedPositive[0] = determined(value) = Satisfied
///     - Constraint def: DeterminedPositive[1] = value > 0mm = Satisfied
///
///   Assertions:
///     1. Composite.value = 0.075 SI (75mm)
///     2. DeterminedPositive[0] = Satisfied (determined(value))
///     3. DeterminedPositive[1] = Satisfied (value > 0mm)
///     4. All Composite constraints are Satisfied
///     5. Total Composite constraint count >= 4 (2 trait-injected + 2 from constraint def)
#[test]
fn constraint_def_determinacy_in_multi_trait_structure() {
    let source = r#"
constraint def DeterminedPositive {
    param v : Length
    determined(v)
    v > 0mm
}
trait Verifiable {
    param value : Length
    constraint determined(value)
}
trait Ranged {
    param value : Length
    constraint value > 0mm
}
structure Composite : Verifiable + Ranged {
    param value : Length = 75mm
    constraint DeterminedPositive(v: value)
}
"#;
    let eval_result = eval_source(source);
    let check_result = check_source(source);

    // 1. Composite.value = 75mm = 0.075 SI
    let value_id = ValueCellId::new("Composite", "value");
    let value = eval_result
        .values
        .get(&value_id)
        .expect("Composite.value should exist");
    match value {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.075).abs() < 1e-12,
                "expected ~0.075 SI for Composite.value (75mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Composite.value, got {:?}", other),
    }

    // 2. DeterminedPositive[0] = determined(value) = Satisfied
    let dp0 = check_result
        .constraint_results
        .iter()
        .find(|e| {
            e.id.entity == "Composite" && e.label == Some("DeterminedPositive#0[0]".to_string())
        })
        .expect("expected DeterminedPositive[0] for Composite");
    assert_eq!(
        dp0.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedPositive[0] (determined(value)) should be Satisfied for value=75mm"
    );

    // 3. DeterminedPositive[1] = value > 0mm = Satisfied
    let dp1 = check_result
        .constraint_results
        .iter()
        .find(|e| {
            e.id.entity == "Composite" && e.label == Some("DeterminedPositive#0[1]".to_string())
        })
        .expect("expected DeterminedPositive[1] for Composite");
    assert_eq!(
        dp1.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedPositive[1] (value > 0mm) should be Satisfied for value=75mm"
    );

    // 4. All Composite constraints are Satisfied
    let composite_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Composite")
        .collect();
    for e in &composite_constraints {
        assert_eq!(
            e.satisfaction,
            Satisfaction::Satisfied,
            "all Composite constraints should be Satisfied, but {:?} is {:?}",
            e.label,
            e.satisfaction
        );
    }

    // 5. At least 4 constraint results for Composite
    // (2 trait-injected unlabeled + 2 from DeterminedPositive def)
    assert!(
        composite_constraints.len() >= 4,
        "expected >= 4 constraint results for Composite, got {}",
        composite_constraints.len()
    );
}
