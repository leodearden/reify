//! Purpose activation lifecycle tests (Task 260).
//!
//! Exercises the full purpose activate/deactivate lifecycle against the
//! Engine API delivered by Task 259:
//!   - activate_purpose / deactivate_purpose / is_purpose_active
//!   - Constraint injection and removal (snapshot.graph.constraints counts)
//!   - Reflective .params inspection via CompiledPurpose.resolved_queries
//!   - Optimization objective injection (minimize / maximize)
//!   - Example-file integration (m10_purpose_activation.ri)
//!
//! Two `#[ignore]`-annotated placeholder tests (steps 23-24) document the
//! expected API for unimplemented categories (.geometric_params filtering
//! and forall-over-reflective-queries) so a follow-up task has a landing spot.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{make_engine, parse_and_compile, parse_and_compile_with_stdlib};
use reify_types::{ModulePath, OptimizationObjective, Satisfaction, Severity};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_purpose_activation.ri"
);

// ── Step 1: activate sets is_purpose_active to true ────────────────────────

#[test]
fn activate_sets_is_purpose_active_true() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("mfg_ready", "Bracket");

    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after activate_purpose call"
    );
}

// ── Step 2: deactivate sets is_purpose_active to false ─────────────────────

#[test]
fn deactivate_sets_is_purpose_active_false() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("mfg_ready", "Bracket");
    engine.deactivate_purpose("mfg_ready");

    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should NOT be active after deactivate_purpose call"
    );
}

// ── Step 3: activate is idempotent ─────────────────────────────────────────

#[test]
fn activate_is_idempotent() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("mfg_ready", "Bracket");
    let count_after_first = engine
        .snapshot()
        .expect("snapshot after first activate")
        .graph
        .constraints
        .len();

    // Second activate should be a no-op (lib.rs:412)
    engine.activate_purpose("mfg_ready", "Bracket");
    let count_after_second = engine
        .snapshot()
        .expect("snapshot after second activate")
        .graph
        .constraints
        .len();

    assert_eq!(
        count_after_first, count_after_second,
        "second activate should be a no-op: constraint count must not change"
    );
}

// ── Step 4: deactivate inactive purpose is a no-op ─────────────────────────

#[test]
fn deactivate_inactive_purpose_is_noop() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    constraint width > 0mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let count_before = engine
        .snapshot()
        .expect("snapshot before deactivate")
        .graph
        .constraints
        .len();

    // Deactivate without ever activating — should be a silent no-op
    engine.deactivate_purpose("mfg_ready");

    let count_after = engine
        .snapshot()
        .expect("snapshot after deactivate")
        .graph
        .constraints
        .len();

    assert_eq!(
        count_before, count_after,
        "deactivating an inactive purpose must not change constraint count"
    );
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should not be active"
    );
}

// ── Step 5: single constraint injection ───────────────────────────────────

#[test]
fn single_constraint_injection() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    constraint width > 0mm
}

purpose ok_basic(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let before = engine
        .snapshot()
        .expect("snapshot before")
        .graph
        .constraints
        .len();

    engine.activate_purpose("ok_basic", "Bracket");

    let after = engine
        .snapshot()
        .expect("snapshot after")
        .graph
        .constraints
        .len();

    assert_eq!(
        after,
        before + 1,
        "activating a purpose with 1 constraint should grow count by 1: before={}, after={}",
        before,
        after
    );
}

// ── Step 6: multiple constraint injection ─────────────────────────────────

#[test]
fn multiple_constraint_injection() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 80mm > 0mm
    constraint 60mm > 0mm
    constraint 5mm > 0mm
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let before = engine
        .snapshot()
        .expect("snapshot before")
        .graph
        .constraints
        .len();

    engine.activate_purpose("mfg_ready", "Bracket");

    let after = engine
        .snapshot()
        .expect("snapshot after")
        .graph
        .constraints
        .len();

    assert_eq!(
        after,
        before + 3,
        "purpose with 3 constraints should grow count by exactly 3: before={}, after={}",
        before,
        after
    );
}

// ── Step 7: constraint removal restores count ──────────────────────────────

#[test]
fn constraint_removal_restores_count() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    constraint width > 0mm
}

purpose mfg_ready(subject : Structure) {
    constraint 80mm > 0mm
    constraint 60mm > 0mm
    constraint 5mm > 0mm
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let before = engine
        .snapshot()
        .expect("snapshot before")
        .graph
        .constraints
        .len();

    engine.activate_purpose("mfg_ready", "Bracket");
    engine.deactivate_purpose("mfg_ready");

    let after = engine
        .snapshot()
        .expect("snapshot after deactivate")
        .graph
        .constraints
        .len();

    assert_eq!(
        after, before,
        "deactivating purpose must restore constraint count: before={}, after={}",
        before, after
    );
}

// ── Step 8: injected constraint IDs have purpose prefix ───────────────────

#[test]
fn injected_constraint_ids_have_purpose_prefix() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("mfg_ready", "Bracket");

    let snapshot = engine
        .snapshot()
        .expect("snapshot after activate");

    // At least one injected constraint id must have the purpose-prefix entity
    // (per lib.rs:433: format!("purpose:{}@{}", purpose_name, entity_ref))
    let has_purpose_prefix = snapshot
        .graph
        .constraints
        .keys()
        .any(|id| id.entity.starts_with("purpose:mfg_ready@Bracket"));

    assert!(
        has_purpose_prefix,
        "at least one constraint id should start with 'purpose:mfg_ready@Bracket'; found: {:?}",
        snapshot.graph.constraints.keys().collect::<Vec<_>>()
    );
}

// ── Step 9: compiled purpose resolves params query ────────────────────────

#[test]
fn compiled_purpose_resolves_params_query() {
    let source = r#"
structure Widget {
    param width : Length = 80mm
    param height : Length = 60mm
    let area = width * height
    constraint width > 0mm
}

purpose check_params(subject : Widget) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);

    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &compiled.compiled_purposes[0];
    assert_eq!(purpose.name, "check_params");
    assert_eq!(purpose.params[0].entity_kind, "Widget");

    // The reflective query must have produced 1 resolved entry for the "subject" param
    assert_eq!(
        purpose.resolved_queries.len(),
        1,
        "expected 1 resolved schema query"
    );
    let query = &purpose.resolved_queries[0];
    assert_eq!(query.param_name, "subject");
    assert_eq!(query.query_kind, "params");
}

// ── Step 10: resolved params excludes let bindings ────────────────────────

#[test]
fn resolved_params_excludes_lets() {
    let source = r#"
structure Widget {
    param width : Length = 80mm
    param height : Length = 60mm
    let area = width * height
    constraint width > 0mm
}

purpose check_params(subject : Widget) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);

    let query = &compiled.compiled_purposes[0].resolved_queries[0];
    // Should have exactly 2 ids: width and height (NOT area)
    assert_eq!(
        query.resolved_ids.len(),
        2,
        "resolved_ids should contain only params (width, height), not lets (area): {:?}",
        query.resolved_ids
    );

    let id_members: Vec<&str> = query
        .resolved_ids
        .iter()
        .map(|id| id.member.as_str())
        .collect();
    assert!(
        id_members.contains(&"width"),
        "resolved_ids should contain 'width'"
    );
    assert!(
        id_members.contains(&"height"),
        "resolved_ids should contain 'height'"
    );
    assert!(
        !id_members.contains(&"area"),
        "resolved_ids must NOT contain 'area' (a let binding)"
    );
}

// ── Step 11: resolved params contains auto params ─────────────────────────

#[test]
fn resolved_params_contains_auto_params() {
    let source = r#"
structure Widget {
    param x : Length = 10mm
    param y : Scalar = auto
}

purpose check_params(subject : Widget) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);

    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let query = &compiled.compiled_purposes[0].resolved_queries[0];
    // Both Param and Auto value cells must be included (traits.rs:342)
    assert_eq!(
        query.resolved_ids.len(),
        2,
        "both explicit param and auto param should be included: {:?}",
        query.resolved_ids
    );
    let id_members: Vec<&str> = query
        .resolved_ids
        .iter()
        .map(|id| id.member.as_str())
        .collect();
    assert!(id_members.contains(&"x"), "should contain 'x'");
    assert!(id_members.contains(&"y"), "should contain 'y'");
}

// ── Step 12: minimize objective injected on activation ────────────────────

#[test]
fn minimize_objective_injected() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose lightweight(subject : Structure) {
    minimize 80mm + 60mm
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("lightweight", "Bracket");

    let objectives = engine.active_objectives();
    assert_eq!(
        objectives.len(),
        1,
        "should have 1 active objective after activation"
    );
    assert!(
        matches!(objectives[0], OptimizationObjective::Minimize(_)),
        "objective should be Minimize, got {:?}",
        objectives[0]
    );
}

// ── Step 13: minimize objective removed on deactivate ────────────────────

#[test]
fn minimize_objective_removed_on_deactivate() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose lightweight(subject : Structure) {
    minimize 80mm + 60mm
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("lightweight", "Bracket");
    engine.deactivate_purpose("lightweight");

    assert!(
        engine.active_objectives().is_empty(),
        "active_objectives should be empty after deactivation"
    );
}

// ── Step 14: maximize objective injected ─────────────────────────────────

#[test]
fn maximize_objective_injected() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose strong(subject : Structure) {
    maximize 80mm * 2
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("strong", "Bracket");

    let objectives = engine.active_objectives();
    assert_eq!(
        objectives.len(),
        1,
        "should have 1 active objective after activation"
    );
    assert!(
        matches!(objectives[0], OptimizationObjective::Maximize(_)),
        "objective should be Maximize, got {:?}",
        objectives[0]
    );
}

// ── Step 15: purpose without objective keeps active_objectives empty ──────

#[test]
fn purpose_without_objective_keeps_active_objectives_empty() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose ok_basic(subject : Structure) {
    constraint 1 > 0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    assert!(
        engine.active_objectives().is_empty(),
        "objectives should be empty before activation"
    );

    engine.activate_purpose("ok_basic", "Bracket");

    assert!(
        engine.active_objectives().is_empty(),
        "objectives should remain empty when purpose has no minimize/maximize"
    );
}

// ── Step 16: multiple purposes, multiple objectives ───────────────────────

#[test]
fn multiple_purposes_multiple_objectives() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose lightweight(subject : Structure) {
    minimize 80mm + 60mm
}

purpose strong(subject : Structure) {
    minimize 5mm * 2
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    engine.activate_purpose("lightweight", "Bracket");
    engine.activate_purpose("strong", "Bracket");
    assert_eq!(
        engine.active_objectives().len(),
        2,
        "both purposes activated: should have 2 objectives"
    );

    engine.deactivate_purpose("lightweight");
    assert_eq!(
        engine.active_objectives().len(),
        1,
        "after deactivating lightweight: should have 1 objective"
    );

    engine.deactivate_purpose("strong");
    assert!(
        engine.active_objectives().is_empty(),
        "after deactivating both: should have 0 objectives"
    );
}

// ── Step 17: unknown purpose activation is a no-op ────────────────────────

#[test]
fn unknown_purpose_activation_is_noop() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    constraint width > 0mm
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let count_before = engine
        .snapshot()
        .expect("snapshot before")
        .graph
        .constraints
        .len();

    // Activating a non-existent purpose should be silently ignored (lib.rs:423)
    engine.activate_purpose("does_not_exist", "Bracket");

    let count_after = engine
        .snapshot()
        .expect("snapshot after")
        .graph
        .constraints
        .len();

    assert_eq!(
        count_before, count_after,
        "unknown purpose should not change constraint count"
    );
    assert!(
        !engine.is_purpose_active("does_not_exist"),
        "unknown purpose should not register as active"
    );
    assert!(
        engine.active_objectives().is_empty(),
        "unknown purpose should not inject any objectives"
    );
}

// ── Step 18: reactivation after deactivation works ────────────────────────

#[test]
fn reactivation_after_deactivation() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 80mm > 0mm
    constraint 60mm > 0mm
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    // First activation
    engine.activate_purpose("mfg_ready", "Bracket");
    let count_after_first = engine
        .snapshot()
        .expect("snapshot after first activate")
        .graph
        .constraints
        .len();

    // Deactivate
    engine.deactivate_purpose("mfg_ready");

    // Re-activate
    engine.activate_purpose("mfg_ready", "Bracket");
    let count_after_second = engine
        .snapshot()
        .expect("snapshot after second activate")
        .graph
        .constraints
        .len();

    assert_eq!(
        count_after_first, count_after_second,
        "re-activation should produce the same constraint count as first activation"
    );
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after re-activation"
    );
}

// ── Step 19: example file parses and compiles ─────────────────────────────

#[test]
fn m10_purpose_activation_example_parses_and_compiles() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");

    // Parse
    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile with stdlib (uses Length units)
    let compiled = parse_and_compile_with_stdlib(&source);

    // Should have at least 1 template named Bracket
    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("should have a Bracket template");
    assert!(
        !bracket.value_cells.is_empty(),
        "Bracket should have value cells"
    );

    // Should have at least 5 compiled purposes
    assert!(
        compiled.compiled_purposes.len() >= 5,
        "expected >=5 purposes (ok_basic, mfg_ready, lightweight, strong, dimensionally_valid), got {}",
        compiled.compiled_purposes.len()
    );

    let purpose_names: Vec<&str> = compiled
        .compiled_purposes
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    for name in &["ok_basic", "mfg_ready", "lightweight", "strong", "dimensionally_valid"] {
        assert!(
            purpose_names.contains(name),
            "expected purpose '{}' in {:?}",
            name,
            purpose_names
        );
    }
}

// ── Step 20: example file — all structure constraints satisfied ───────────

#[test]
fn m10_purpose_activation_example_constraints_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // Check constraints — all must be Satisfied
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 8,
        "expected >=8 constraint results (8 structure-level assertions), got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── Step 21: example file — activate lightweight (minimize) purpose ───────

#[test]
fn m10_purpose_activation_example_activate_minimize_purpose() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let count_before = engine
        .snapshot()
        .expect("snapshot before")
        .graph
        .constraints
        .len();

    // Activate lightweight (has 1 constraint + minimize objective)
    engine.activate_purpose("lightweight", "Bracket");

    let count_after = engine
        .snapshot()
        .expect("snapshot after activate")
        .graph
        .constraints
        .len();

    assert!(
        count_after > count_before,
        "activating lightweight should inject >=1 constraint: before={}, after={}",
        count_before,
        count_after
    );

    let objectives = engine.active_objectives();
    assert!(
        objectives.iter().any(|o| matches!(o, OptimizationObjective::Minimize(_))),
        "lightweight purpose should inject a Minimize objective"
    );

    // Deactivate — count restored, objectives empty
    engine.deactivate_purpose("lightweight");
    let count_restored = engine
        .snapshot()
        .expect("snapshot after deactivate")
        .graph
        .constraints
        .len();
    assert_eq!(
        count_restored, count_before,
        "deactivating lightweight must restore constraint count"
    );
    assert!(
        engine.active_objectives().is_empty(),
        "deactivating lightweight must clear objectives"
    );
}

// ── Step 22: example file — activate mfg_ready (multi-constraint) purpose ──

#[test]
fn m10_purpose_activation_example_activate_multi_constraint_purpose() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let count_before = engine
        .snapshot()
        .expect("snapshot before")
        .graph
        .constraints
        .len();

    // mfg_ready has exactly 3 literal constraints
    engine.activate_purpose("mfg_ready", "Bracket");

    let count_after = engine
        .snapshot()
        .expect("snapshot after activate")
        .graph
        .constraints
        .len();

    assert_eq!(
        count_after,
        count_before + 3,
        "mfg_ready has 3 constraints: count should grow by exactly 3: before={}, after={}",
        count_before,
        count_after
    );

    // Deactivate — exact restoration
    engine.deactivate_purpose("mfg_ready");
    let count_restored = engine
        .snapshot()
        .expect("snapshot after deactivate")
        .graph
        .constraints
        .len();
    assert_eq!(
        count_restored, count_before,
        "deactivating mfg_ready must restore constraint count exactly"
    );
}

// ── Step 23: placeholder — .geometric_params NOT YET IMPLEMENTED ──────────
//
// This test is intentionally `#[ignore]`-annotated so it does not run under
// normal `cargo test`. It documents the expected API shape for the
// `.geometric_params` filtering feature that Task 259 did not ship.
// A follow-up task should remove the `#[ignore]` and implement the feature.

#[test]
#[ignore = "pending Task 259 follow-up: .geometric_params filtering not implemented"]
fn geometric_params_filtering_not_yet_implemented() {
    // Expected API: a purpose whose body references subject.geometric_params
    // should produce a resolved_queries entry with query_kind == "geometric_params"
    // containing only Length/Area/Volume dimensioned params (filtering out
    // dimensionless Scalars and non-dimensional params).
    //
    // Example source (does not compile cleanly today):
    //   structure Widget {
    //       param width : Length = 80mm
    //       param height : Length = 60mm
    //       param count : Scalar = 5
    //   }
    //   purpose check_geo(subject : Widget) {
    //       constraint forall p in subject.geometric_params: determined(p)
    //   }
    //
    // Expected assertion once implemented:
    //   let query = &compiled.compiled_purposes[0].resolved_queries
    //       .iter().find(|q| q.query_kind == "geometric_params").unwrap();
    //   assert_eq!(query.resolved_ids.len(), 2); // width and height, not count
    panic!("pending Task 259 follow-up: .geometric_params filtering not implemented");
}

// ── Step 24: placeholder — forall over reflective params NOT YET IMPLEMENTED
//
// This test is intentionally `#[ignore]`-annotated so it does not run under
// normal `cargo test`. It documents the expected end-to-end behaviour for
// `forall p in subject.params: determined(p)` evaluated through a purpose body.
// A follow-up task should wire subject.params → Value::List in the evaluator
// and remove the `#[ignore]`.

#[test]
#[ignore = "pending Task 259 follow-up: forall over reflective queries not wired in evaluator"]
fn forall_over_reflective_params_not_yet_implemented() {
    // Expected API: a purpose constraint `forall p in subject.params: determined(p)`
    // should evaluate at activation time — iterating over the Value::List of
    // param ValueCellIds and checking each is determined in the eval snapshot.
    //
    // Example source (currently produces Undef or compile error):
    //   structure Bracket {
    //       param width : Length = 80mm
    //       param height : Length = 60mm
    //   }
    //   purpose all_params_determined(subject : Structure) {
    //       constraint forall p in subject.params: determined(p)
    //   }
    //
    // Expected assertion once implemented:
    //   engine.activate_purpose("all_params_determined", "Bracket");
    //   let check = engine.check(&compiled);
    //   let purpose_constraint = check.constraint_results.iter()
    //       .find(|r| r.id.entity.starts_with("purpose:all_params_determined@Bracket"))
    //       .expect("purpose constraint result should exist");
    //   assert_eq!(purpose_constraint.satisfaction, Satisfaction::Satisfied);
    panic!("pending Task 259 follow-up: forall over reflective queries not wired in evaluator");
}
