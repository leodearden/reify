//! Eval tests for `filter(entity_ref_list, Trait)` conformance filter
//! (task 3991, δ).
//!
//! These tests verify end-to-end evaluation of the `filter(self.descendants,
//! Trait)` conformance filter, which is expanded by the compiler intercept
//! (step-2) and then rewritten to a concrete filtered list by the
//! `apply_trait_filters` post-pass in `engine_eval.rs` (step-4/6).
//!
//! Step numbering mirrors plan.json step IDs.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_eval::Engine;

// ─── step-3: DIRECT conformance + ordering + empty (RED) ───

/// Fixture: Assembly with two Bolt-conforming subs (B1, B2) interleaved with
/// a non-conforming sub (Plain), in declaration order B1 → Plain → B2.
///
/// `filter(self.descendants, Bolt)` should yield exactly [B1, B2] paths in
/// source order (Plain excluded), length == 2.
///
/// RED today: the `filter(list_literal, TraitObject-marker)` node produced by
/// the compiler intercept is not yet rewritten by apply_trait_filters, so
/// eval sees the raw FunctionCall and returns Undef.
#[test]
fn filter_direct_conformance_preserves_source_order() {
    let source = r#"
        trait Bolt {}
        structure def B1 : Bolt {}
        structure Plain {}
        structure def B2 : Bolt {}
        structure Assembly {
            sub b1 = B1()
            sub plain = Plain()
            sub b2 = B2()
            let bolts = filter(self.descendants, Bolt)
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {:?}",
        compile_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let bolts_id = ValueCellId::new("Assembly", "bolts");
    match result.values.get(&bolts_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                2,
                "Assembly.bolts should have 2 elements (B1 + B2, Plain excluded); \
                 got: {:?}",
                items
            );
            assert_eq!(
                items[0],
                Value::String("Assembly.b1".to_string()),
                "Assembly.bolts[0] should be Assembly.b1; got: {:?}",
                items[0]
            );
            assert_eq!(
                items[1],
                Value::String("Assembly.b2".to_string()),
                "Assembly.bolts[1] should be Assembly.b2; got: {:?}",
                items[1]
            );
        }
        other => panic!(
            "Assembly.bolts should be Value::List; got: {:?}",
            other
        ),
    }
}

/// Empty case: all descendants are non-conforming; filter returns an empty list.
///
/// RED today: filter is not rewritten, so the cell evaluates to Undef.
#[test]
fn filter_empty_when_no_conformers() {
    let source = r#"
        trait Bolt {}
        structure Nut {}
        structure Washer {}
        structure Assembly {
            sub n = Nut()
            sub w = Washer()
            let bolts = filter(self.descendants, Bolt)
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {:?}",
        compile_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let bolts_id = ValueCellId::new("Assembly", "bolts");
    match result.values.get(&bolts_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                0,
                "Assembly.bolts should be empty when no subs conform to Bolt; \
                 got: {:?}",
                items
            );
        }
        other => panic!(
            "Assembly.bolts should be an empty Value::List; got: {:?}",
            other
        ),
    }
}

// ─── step-5: TRANSITIVE refinement conformance (RED) ───

/// Fixture: `trait Fastener {}`, `trait Bolt : Fastener {}`.
/// `structure def HexBolt : Bolt {}` conforms to Bolt but NOT directly to
/// Fastener.  Filtering by Fastener should include HexBolt via the Bolt →
/// Fastener refinement chain.
///
/// RED today: step-4's DIRECT check (`trait_bounds.contains("Fastener")`) sees
/// only ["Bolt"] for HexBolt, so filtering by Fastener returns empty.
#[test]
fn filter_transitive_refinement_conformance() {
    let source = r#"
        trait Fastener {}
        trait Bolt : Fastener {}
        structure def HexBolt : Bolt {}
        structure Nut {}
        structure Assembly {
            sub hb = HexBolt()
            sub nut = Nut()
            let fasteners = filter(self.descendants, Fastener)
            let bolts = filter(self.descendants, Bolt)
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {:?}",
        compile_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // filter by Fastener: HexBolt conforms via Bolt -> Fastener chain.
    let fasteners_id = ValueCellId::new("Assembly", "fasteners");
    match result.values.get(&fasteners_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                1,
                "Assembly.fasteners should have 1 element (HexBolt via Bolt->Fastener); \
                 got: {:?}",
                items
            );
            assert_eq!(
                items[0],
                Value::String("Assembly.hb".to_string()),
                "Assembly.fasteners[0] should be Assembly.hb; got: {:?}",
                items[0]
            );
        }
        other => panic!(
            "Assembly.fasteners should be Value::List with 1 element; got: {:?}",
            other
        ),
    }

    // filter by Bolt: HexBolt directly conforms.
    let bolts_id = ValueCellId::new("Assembly", "bolts");
    match result.values.get(&bolts_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                1,
                "Assembly.bolts should have 1 element (HexBolt); got: {:?}",
                items
            );
            assert_eq!(
                items[0],
                Value::String("Assembly.hb".to_string()),
                "Assembly.bolts[0] should be Assembly.hb; got: {:?}",
                items[0]
            );
        }
        other => panic!(
            "Assembly.bolts should be Value::List with 1 element; got: {:?}",
            other
        ),
    }
}

// ─── step-7: end-to-end example fixture (RED until step-8 creates the file) ───

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/structural_query_filter.ri"
);

/// Reads `examples/structural_query_filter.ri`, parses, compiles, and
/// evaluates it.  Asserts zero Error diagnostics at both stages and that
/// the headline BOM scenario produces the expected filtered list length.
///
/// RED until step-8 creates the file (read_to_string fails / file missing).
#[test]
fn example_structural_query_filter_ri_evals_clean() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/structural_query_filter.ri should exist (created by step-8)");

    let parsed = reify_syntax::parse(&source, ModulePath::single("structural_query_filter_example"));
    assert!(
        parsed.errors.is_empty(),
        "example parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "example compile errors: {:?}",
        compile_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "example eval errors: {:?}",
        eval_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The example file must declare a `bolt_count` value cell on Assembly
    // with the expected conforming-descendant count.
    //
    // The exact count is 5, documented in the .ri file header:
    //   (1) Assembly.hex           — HexBolt    : Bolt
    //   (2) Assembly.socket        — SocketBolt : Bolt
    //   (3) Assembly.bolt_group[0] — HexBolt    : Bolt
    //   (4) Assembly.bolt_group[1] — HexBolt    : Bolt
    //   (5) Assembly.spare         — HexBolt    : Bolt  (aux sub, included per PRD §3)
    //       Assembly.washer        — Washer (no Bolt bound) — excluded
    let bolt_count_id = ValueCellId::new("Assembly", "bolt_count");
    match result.values.get(&bolt_count_id) {
        Some(Value::Int(n)) => {
            assert_eq!(
                *n, 5,
                "Assembly.bolt_count should be exactly 5 \
                 (hex + socket + bolt_group[0] + bolt_group[1] + spare); got: {}",
                n
            );
        }
        other => panic!(
            "Assembly.bolt_count should be Value::Int; got: {:?}",
            other
        ),
    }
}
