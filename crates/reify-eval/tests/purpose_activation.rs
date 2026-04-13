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
