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
