//! Purpose activation lifecycle tests (Task 260).
//!
//! Exercises the full purpose activate/deactivate lifecycle against the Engine
//! API delivered by Task 259:
//!   - activate_purpose / deactivate_purpose / is_purpose_active
//!   - Constraint injection and removal (snapshot.graph.constraints counts)
//!   - Reflective .params inspection via CompiledPurpose.resolved_queries
//!   - Optimization objective injection (minimize / maximize)
//!   - Example-file integration (m10_purpose_activation.ri)
//!
//! This file subsumes `purpose_eval.rs` (removed in Task 260 amendment pass,
//! reviewer suggestion S3). The unique test `eval_clears_stale_purpose_state`
//! is preserved in §2 below.
//!
//! NOTE: Three feature categories remain deferred (post task-2181 / task-2200):
//!   - `.geometric_params` filtering (runtime expansion of reflective-aggregation
//!     elements against the bound entity; compile-time empty-list wiring is now done)
//!   - `forall p in subject.params: determined(p)` evaluated at runtime (vacuously
//!     true today due to empty-list emission; runtime expansion is a follow-up task)
//!   - Member-type resolution for concrete subjects: task-2200 added compile-time
//!     existence validation (unknown member → Error diagnostic) for concrete subject
//!     types, but member types still fall back to `Type::Real`. The generic
//!     `subject : Structure` wildcard still has no template to validate against,
//!     so `subject.bogus` on a wildcard subject compiles silently — a known
//!     limitation documented by the characterization test in purpose_compile_tests.rs.
//!
//! Compile-time `subject.<param>` member-access wiring (task-2181) is now complete;
//! see §5 below for the remap_entity integration test.

use reify_eval::Engine;
use reify_test_support::{
    make_engine, make_simple_engine, parse_and_compile, parse_and_compile_with_stdlib,
};
use reify_types::{
    CompiledExprKind, ModulePath, OptimizationObjective, Satisfaction, Severity, Type, ValueCellId,
};

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_purpose_activation.ri"
);

// ─── Fixture sources ──────────────────────────────────────────────────────────

/// Minimal Bracket + single-constraint purpose (lifecycle tests).
const SIMPLE_MFG_SRC: &str = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;

/// Bracket + purpose with 3 literal constraints.
const MULTI_CONSTRAINT_SRC: &str = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose mfg_ready(subject : Structure) {
    constraint 80mm > 0mm
    constraint 60mm > 0mm
    constraint 5mm > 0mm
}
"#;

/// Bracket + purpose with a minimize objective.
const MINIMIZE_SRC: &str = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose lightweight(subject : Structure) {
    minimize 80mm + 60mm
}
"#;

/// Bracket + purpose with a maximize objective.
const MAXIMIZE_SRC: &str = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose strong(subject : Structure) {
    maximize 80mm * 2
}
"#;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Returns the constraint count from the current engine snapshot.
fn constraint_count(engine: &Engine) -> usize {
    engine
        .snapshot()
        .expect("snapshot should exist")
        .graph
        .constraints
        .len()
}

// ── §1: Activate / deactivate lifecycle ──────────────────────────────────────

#[test]
fn activate_sets_is_purpose_active_true() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after activate_purpose call"
    );
}

#[test]
fn deactivate_sets_is_purpose_active_false() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    engine.deactivate_purpose("mfg_ready");
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should NOT be active after deactivate_purpose call"
    );
}

#[test]
fn activate_is_idempotent() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    let count_first = constraint_count(&engine);
    // Second activate should be a no-op (lib.rs:412)
    engine.activate_purpose("mfg_ready", "Bracket");
    assert_eq!(
        count_first,
        constraint_count(&engine),
        "second activate should be a no-op: constraint count must not change"
    );
}

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
    let before = constraint_count(&engine);
    engine.deactivate_purpose("mfg_ready"); // never activated
    assert_eq!(
        before,
        constraint_count(&engine),
        "deactivating an inactive purpose must not change constraint count"
    );
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should not be active"
    );
}

// ── §2: eval() clears stale purpose state (migrated from purpose_eval.rs) ────

#[test]
fn eval_clears_stale_purpose_state() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after activation"
    );
    // Second eval — fresh snapshot; purpose state should be cleared (lib.rs:930-931)
    engine.eval(&compiled);
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should NOT be active after a fresh eval() call"
    );
    // Re-activation should work (not blocked by stale 'already active' guard)
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be re-activatable after fresh eval()"
    );
}

// ── §3: Constraint injection and removal ─────────────────────────────────────

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
    let before = constraint_count(&engine);
    engine.activate_purpose("ok_basic", "Bracket");
    assert_eq!(
        constraint_count(&engine),
        before + 1,
        "activating a purpose with 1 constraint should grow count by 1"
    );
}

#[test]
fn multiple_constraint_injection() {
    let compiled = parse_and_compile(MULTI_CONSTRAINT_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert_eq!(
        constraint_count(&engine),
        before + 3,
        "purpose with 3 constraints should grow count by exactly 3"
    );
}

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
    let before = constraint_count(&engine);
    engine.activate_purpose("mfg_ready", "Bracket");
    engine.deactivate_purpose("mfg_ready");
    assert_eq!(
        constraint_count(&engine),
        before,
        "deactivating purpose must restore constraint count"
    );
}

#[test]
fn injected_constraint_ids_have_purpose_prefix() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    let snapshot = engine.snapshot().expect("snapshot after activate");
    // Per lib.rs:433: format!("purpose:{}@{}", purpose_name, entity_ref)
    let has_prefix = snapshot
        .graph
        .constraints
        .keys()
        .any(|id| id.entity.starts_with("purpose:mfg_ready@Bracket"));
    assert!(
        has_prefix,
        "at least one constraint id should start with 'purpose:mfg_ready@Bracket'; found: {:?}",
        snapshot.graph.constraints.keys().collect::<Vec<_>>()
    );
}

// ── §4: Reflective .params inspection ────────────────────────────────────────
//
// `resolved_queries` is populated by the compiler unconditionally for each
// purpose parameter whose entity_kind matches a registered template (see
// crates/reify-compiler/src/traits.rs:333-353). The purpose body does not
// need to reference `subject.params` — the compiler always emits the query.

#[test]
fn compiler_always_emits_params_query_per_structure_purpose_param() {
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
    assert_eq!(
        purpose.resolved_queries.len(),
        1,
        "expected 1 resolved schema query (the 'params' query for 'subject')"
    );
    let query = &purpose.resolved_queries[0];
    assert_eq!(query.param_name, "subject");
    assert_eq!(query.query_kind, "params");
}

#[test]
fn compiler_params_query_excludes_let_bindings() {
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
    assert_eq!(
        query.resolved_ids.len(),
        2,
        "resolved_ids should contain only params (width, height), not lets (area): {:?}",
        query.resolved_ids
    );
    let members: Vec<&str> = query
        .resolved_ids
        .iter()
        .map(|id| id.member.as_str())
        .collect();
    assert!(members.contains(&"width"), "should contain 'width'");
    assert!(members.contains(&"height"), "should contain 'height'");
    assert!(
        !members.contains(&"area"),
        "must NOT contain 'area' (a let binding)"
    );
}

#[test]
fn compiler_params_query_includes_auto_params() {
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
    let members: Vec<&str> = query
        .resolved_ids
        .iter()
        .map(|id| id.member.as_str())
        .collect();
    assert!(members.contains(&"x"), "should contain 'x'");
    assert!(members.contains(&"y"), "should contain 'y'");
}

// ── §5: Optimization objectives ──────────────────────────────────────────────

#[test]
fn minimize_objective_injected() {
    let compiled = parse_and_compile(MINIMIZE_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("lightweight", "Bracket");
    let objectives = engine.active_objectives();
    assert_eq!(objectives.len(), 1, "should have 1 active objective");
    assert!(
        matches!(objectives[0], OptimizationObjective::Minimize(_)),
        "objective should be Minimize, got {:?}",
        objectives[0]
    );
}

#[test]
fn minimize_objective_removed_on_deactivate() {
    let compiled = parse_and_compile(MINIMIZE_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("lightweight", "Bracket");
    engine.deactivate_purpose("lightweight");
    assert!(
        engine.active_objectives().is_empty(),
        "active_objectives should be empty after deactivation"
    );
}

#[test]
fn maximize_objective_injected() {
    let compiled = parse_and_compile(MAXIMIZE_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("strong", "Bracket");
    let objectives = engine.active_objectives();
    assert_eq!(objectives.len(), 1, "should have 1 active objective");
    assert!(
        matches!(objectives[0], OptimizationObjective::Maximize(_)),
        "objective should be Maximize, got {:?}",
        objectives[0]
    );
}

#[test]
fn purpose_without_objective_keeps_active_objectives_empty() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    assert!(
        engine.active_objectives().is_empty(),
        "objectives should be empty before activation"
    );
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.active_objectives().is_empty(),
        "objectives should remain empty when purpose has no minimize/maximize"
    );
}

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
        "both purposes: 2 objectives"
    );
    engine.deactivate_purpose("lightweight");
    assert_eq!(
        engine.active_objectives().len(),
        1,
        "after deactivating lightweight: 1 objective"
    );
    engine.deactivate_purpose("strong");
    assert!(
        engine.active_objectives().is_empty(),
        "after deactivating both: 0 objectives"
    );
}

// ── §6: Edge cases ────────────────────────────────────────────────────────────

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
    let before = constraint_count(&engine);
    engine.activate_purpose("does_not_exist", "Bracket"); // silently ignored (lib.rs:423)
    assert_eq!(
        before,
        constraint_count(&engine),
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
    engine.activate_purpose("mfg_ready", "Bracket");
    let count_after_first = constraint_count(&engine);
    engine.deactivate_purpose("mfg_ready");
    engine.activate_purpose("mfg_ready", "Bracket");
    assert_eq!(
        count_after_first,
        constraint_count(&engine),
        "re-activation should produce the same constraint count as first activation"
    );
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after re-activation"
    );
}

// ── §7: Example-file integration ─────────────────────────────────────────────

#[test]
fn m10_purpose_activation_example_parses_and_compiles() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");
    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = parse_and_compile_with_stdlib(&source);
    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("should have a Bracket template");
    assert!(
        !bracket.value_cells.is_empty(),
        "Bracket should have value cells"
    );
    assert!(
        compiled.compiled_purposes.len() >= 5,
        "expected >=5 purposes (ok_basic, mfg_ready, lightweight, strong, dimensionally_valid), got {}",
        compiled.compiled_purposes.len()
    );
    let names: Vec<&str> = compiled
        .compiled_purposes
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    for name in &[
        "ok_basic",
        "mfg_ready",
        "lightweight",
        "strong",
        "dimensionally_valid",
    ] {
        assert!(
            names.contains(name),
            "expected purpose '{}' in {:?}",
            name,
            names
        );
    }
}

#[test]
fn m10_purpose_activation_example_constraints_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    // Use make_simple_engine() (rather than make_engine()) because this test needs a
    // real checker to evaluate structural constraints to Satisfied. make_engine() uses
    // MockConstraintChecker which returns Unchecked.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 8,
        "expected >=8 constraint results, got {}",
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

#[test]
fn m10_purpose_activation_example_activate_minimize_purpose() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    let mut engine = make_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);
    // lightweight has exactly 1 constraint + a minimize objective
    engine.activate_purpose("lightweight", "Bracket");
    assert_eq!(
        constraint_count(&engine),
        before + 1,
        "lightweight has 1 constraint: count should grow by exactly 1"
    );
    assert!(
        engine
            .active_objectives()
            .iter()
            .any(|o| matches!(o, OptimizationObjective::Minimize(_))),
        "lightweight should inject a Minimize objective"
    );
    engine.deactivate_purpose("lightweight");
    assert_eq!(
        constraint_count(&engine),
        before,
        "deactivating lightweight must restore constraint count"
    );
    assert!(
        engine.active_objectives().is_empty(),
        "deactivating lightweight must clear objectives"
    );
}

#[test]
fn m10_purpose_activation_example_activate_multi_constraint_purpose() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_purpose_activation.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    let mut engine = make_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);
    // mfg_ready has exactly 3 literal constraints
    engine.activate_purpose("mfg_ready", "Bracket");
    assert_eq!(
        constraint_count(&engine),
        before + 3,
        "mfg_ready has 3 constraints: count should grow by exactly 3"
    );
    engine.deactivate_purpose("mfg_ready");
    assert_eq!(
        constraint_count(&engine),
        before,
        "deactivating mfg_ready must restore constraint count exactly"
    );
}

// §5 (was S5 deferral — task-2181): see subject_member_access_is_remapped_to_bound_entity_on_activation below.

// ── §5: subject.<param> remap_entity integration (task-2181) ─────────────────
//
// Exercises `CompiledExpr::remap_entity` on a `subject.<param>` reference
// produced by the new StructureRef-subject branch in the MemberAccess arm.
// Pre-activation: constraint's ValueRef entity == purpose name.
// Post-activation: remap_entity("weight_target", "Bracket") rewrites it to
// "Bracket".  This is the S5 acceptance criterion.

/// Verifies that `subject.mass` in a purpose body is remapped to the bound
/// entity's `mass` member when the purpose is activated.
///
/// Flow:
///  1. Compile `purpose weight_target(subject : Structure) { constraint subject.mass > 0 }`.
///  2. Pre-activation: assert constraint BinOp left ValueRef entity == "weight_target".
///  3. `engine.activate_purpose("weight_target", "Bracket")`.
///  4. Post-activation: find injected constraint, assert ValueRef entity == "Bracket".
#[test]
fn subject_member_access_is_remapped_to_bound_entity_on_activation() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose weight_target(subject : Structure) {
    constraint subject.mass > 0
}
"#;
    let compiled = parse_and_compile(source);
    assert_eq!(compiled.compiled_purposes.len(), 1);
    let purpose = &compiled.compiled_purposes[0];
    assert_eq!(purpose.name, "weight_target");

    // Pre-activation: ValueRef entity must equal the purpose name (pre-remap stamp).
    let constraint = &purpose.constraints[0];
    let pre_entity = match &constraint.expr.kind {
        CompiledExprKind::BinOp { left, .. } => match &left.kind {
            CompiledExprKind::ValueRef(id) => id.entity.clone(),
            other => panic!(
                "pre-activation: expected ValueRef for left of BinOp, got {:?}",
                other
            ),
        },
        other => panic!(
            "pre-activation: expected BinOp constraint expr, got {:?}",
            other
        ),
    };
    assert_eq!(
        pre_entity, "weight_target",
        "pre-activation: ValueRef entity must equal the purpose name"
    );

    // Activate against Bracket — remap_entity fires inside activate_purpose.
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("weight_target", "Bracket");

    let snapshot = engine.snapshot().expect("snapshot after activate_purpose");

    // Find the injected constraint by its purpose-prefixed entity id.
    let injected_data = snapshot
        .graph
        .constraints
        .iter()
        .find(|(id, _)| id.entity.starts_with("purpose:weight_target@Bracket"))
        .map(|(_, data)| data.clone())
        .expect(
            "expected at least one constraint with entity prefix 'purpose:weight_target@Bracket'",
        );

    // Post-activation: remap_entity must have rewritten the ValueRef entity to "Bracket".
    let post_entity = match &injected_data.expr.kind {
        CompiledExprKind::BinOp { left, .. } => match &left.kind {
            CompiledExprKind::ValueRef(id) => id.entity.clone(),
            other => panic!(
                "post-activation: expected ValueRef in remapped BinOp left, got {:?}",
                other
            ),
        },
        other => panic!(
            "post-activation: expected BinOp in remapped constraint expr, got {:?}",
            other
        ),
    };
    assert_eq!(
        post_entity, "Bracket",
        "post-activation: remap_entity must rewrite ValueRef entity from 'weight_target' to 'Bracket'"
    );
}

// ── §8: Reflective aggregation acceptance (task-2289) ────────────────────────

/// Acceptance test for runtime expansion of `subject.params` (task-2289).
///
/// Guards the end-to-end pipeline that fires `forall p in subject.params:
/// determined(p)` against a real entity:
///
///   1. The compiler emits a `PurposeReflectiveAggregation` placeholder in
///      place of the legacy empty `ListLiteral` (task-2289 step-7).
///   2. `activate_purpose` walks the constraint expression and rewrites the
///      placeholder into `ReflectiveCellList([ValueRef(Bracket, x)])` using the
///      bound entity's resolved param queries (task-2289 step-11, variant
///      narrowed to `ReflectiveCellList` in task-2458).
///   3. The quantifier evaluator detects the `ReflectiveCellList` variant and
///      iterates over cell IDs, calling `remap_cell` per iteration so
///      `determined(p)` is rewritten to `determined(Bracket.x)` rather than
///      `determined($loop_var)` (task-2289 step-9, trigger narrowed to
///      `ReflectiveCellList` in task-2458).
///
/// `Bracket.x` is declared with no default and no auto, so it is
/// `Undetermined` at runtime → `determined(Bracket.x)` is `false` →
/// `forall` returns `false` → the purpose reports `Violated`.
///
/// Historical note: this test previously pinned a vacuous-true trap
/// (`Satisfied` for an entity with undetermined params) under
/// task-2199; runtime expansion landing under task-2289 closed the trap.
#[test]
fn manufacturing_ready_violates_for_undetermined_params() {
    // Bracket has a deliberately undetermined param: no default, no auto.
    // Runtime expansion makes `determined(p)` for `Bracket.x` return
    // false → `forall` → false → Violated.
    let source = r#"
structure Bracket {
    param x : Real
}

purpose manufacturing_ready(subject : Structure) {
    constraint forall p in subject.params: determined(p)
}
"#;
    let compiled = parse_and_compile(source);
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "fixture failed to compile cleanly"
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    // make_simple_engine() uses SimpleConstraintChecker (not MockConstraintChecker)
    // so it can return Satisfied/Violated rather than Unchecked.
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Activate — injects the purpose constraint into snapshot.graph.constraints.
    engine.activate_purpose("manufacturing_ready", "Bracket");

    // Use check_constraints_with_values (NOT engine.check()) because purpose-injected
    // constraints live in snapshot.graph.constraints, which engine.check() does not visit.
    let (constraint_results, _) = engine
        .check_constraints_with_values(&eval_result.values)
        .expect("check_constraints_with_values must not return an error");

    // Find the injected constraint by its purpose-prefixed entity id.
    // Format: "purpose:<name>@<entity>" (engine_purposes.rs:41).
    let purpose_result = constraint_results
        .iter()
        .find(|e| {
            e.id.entity
                .starts_with("purpose:manufacturing_ready@Bracket")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a purpose-injected constraint with entity prefix \
                 'purpose:manufacturing_ready@Bracket'; found ids: {:?}",
                constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
            )
        });

    // Acceptance: runtime expansion + cell-iteration quantifier eval cause
    // `determined(Bracket.x)` to return false → forall false → Violated.
    assert_eq!(
        purpose_result.satisfaction,
        Satisfaction::Violated,
        "task-2289 acceptance: subject.params expands at activation to [ValueRef(Bracket, x)], \
         the quantifier iterates cell IDs and rewrites determined(p) into determined(Bracket.x); \
         Bracket.x is undetermined, so forall is false and the purpose must be Violated.",
    );
}

// ── §9: Reflective aggregation activation-time expansion (task-2289) ─────────
//
// `activate_purpose` walks each constraint's expression tree (and the objective)
// and rewrites every `CompiledExprKind::PurposeReflectiveAggregation` placeholder
// into a populated `ReflectiveCellList([ValueRef(entity_ref, member), ...])` sourced
// from the active purpose's `resolved_queries` (task-2458 — distinguished from
// user-written `ListLiteral`s for eval_quantifier's cell-iteration trigger).
// Element `result_type` is inherited from the looked-up `ValueCellNode.cell_type`
// (cell-type lockstep).

/// Verifies that activating `purpose check(subject : Structure) {
/// constraint forall p in subject.params: determined(p) }` against a
/// `Bracket` with two `Real` params expands the placeholder into a
/// concrete `ReflectiveCellList` of `ValueRef`s pointing at `Bracket.x` and
/// `Bracket.y` (task-2458).
///
/// Acceptance criteria (task-2289 step-10, updated in task-2458):
///  (a) collection.kind is `CompiledExprKind::ReflectiveCellList` (no longer the
///      `PurposeReflectiveAggregation` marker, and not a `ListLiteral`).
///  (b) the ReflectiveCellList has exactly two elements, each a `ValueRef`.
///  (c) the elements' cell IDs are `{Bracket.x, Bracket.y}` (any order).
///  (d) each element's `result_type` is `Type::Real` (cell-type lockstep —
///      `Bracket.x` / `Bracket.y` are declared as `Real`).
#[test]
fn activate_expands_subject_params_placeholder_to_populated_list() {
    let source = r#"
structure Bracket {
    param x : Real
    param y : Real
}

purpose check(subject : Structure) {
    constraint forall p in subject.params: determined(p)
}
"#;
    let compiled = parse_and_compile(source);
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "fixture failed to compile cleanly"
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    engine.activate_purpose("check", "Bracket");

    let snapshot = engine.snapshot().expect("snapshot after activate_purpose");

    // Locate the injected constraint by its purpose-prefixed entity id.
    let injected = snapshot
        .graph
        .constraints
        .iter()
        .find(|(id, _)| id.entity.starts_with("purpose:check@Bracket"))
        .map(|(_, data)| data.clone())
        .expect(
            "expected at least one constraint with entity prefix 'purpose:check@Bracket' \
             after activation",
        );

    // The constraint expression is `forall p in subject.params: determined(p)` →
    // a Quantifier whose collection should now be the expanded ReflectiveCellList.
    let collection = match &injected.expr.kind {
        CompiledExprKind::Quantifier { collection, .. } => collection,
        other => panic!(
            "expected Quantifier in injected constraint expr, got {:?}",
            other
        ),
    };

    // (a) collection.kind is ReflectiveCellList (no longer the placeholder marker).
    // task-2458: must be ReflectiveCellList, not ListLiteral.
    let elements = match &collection.kind {
        CompiledExprKind::ReflectiveCellList(elements) => elements,
        CompiledExprKind::PurposeReflectiveAggregation { .. } => panic!(
            "post-activation: collection is still the `PurposeReflectiveAggregation` \
             placeholder; activate_purpose must expand it into a populated \
             `ReflectiveCellList` (task-2458)"
        ),
        other => panic!(
            "post-activation: expected `ReflectiveCellList` in Quantifier collection \
             (task-2458), got {:?}",
            other
        ),
    };

    // (b) exactly two elements, each a ValueRef.
    assert_eq!(
        elements.len(),
        2,
        "expected 2 ValueRef elements (Bracket.x, Bracket.y), got {}",
        elements.len()
    );

    let mut element_cells: Vec<ValueCellId> = Vec::with_capacity(elements.len());
    for (i, element) in elements.iter().enumerate() {
        match &element.kind {
            CompiledExprKind::ValueRef(id) => element_cells.push(id.clone()),
            other => panic!(
                "expected ValueRef element at index {} of expanded ReflectiveCellList, \
                 got {:?}",
                i, other
            ),
        }
        // (d) cell-type lockstep — each element's result_type must equal the
        // declared cell type (Real for Bracket.x / Bracket.y).
        assert_eq!(
            element.result_type,
            Type::Real,
            "expected element {} result_type to be Type::Real (cell-type lockstep), got {:?}",
            i,
            element.result_type
        );
    }

    // (c) element cell IDs are exactly {Bracket.x, Bracket.y} (any order).
    element_cells.sort();
    let expected = vec![
        ValueCellId::new("Bracket", "x"),
        ValueCellId::new("Bracket", "y"),
    ];
    assert_eq!(
        element_cells, expected,
        "expected expanded element cell IDs to be {{Bracket.x, Bracket.y}}, got {:?}",
        element_cells
    );
}

/// Companion test for `activate_expands_subject_params_placeholder_to_populated_list`:
/// `subject.geometric_params` has no compile-time `ResolvedSchemaQuery` entry
/// (only `params` is populated by `compile_purpose` in `traits.rs`) and no
/// activation-time fallback heuristic (task-1904 territory). The expansion
/// helper's no-resolved-query branch must therefore replace the placeholder
/// with an empty `ReflectiveCellList` (task-2458), preserving today's
/// vacuous-true semantics for `forall p in subject.geometric_params: ...`.
///
/// Acceptance criteria (task-2289 step-12, updated in task-2458):
///   (a) collection.kind is `CompiledExprKind::ReflectiveCellList` (no longer the
///       `PurposeReflectiveAggregation` placeholder, and not a `ListLiteral`).
///   (b) the ReflectiveCellList is empty.
///
/// Should PASS immediately given step-11's no-matching-query branch.
#[test]
fn activate_expands_geometric_params_placeholder_to_empty_list() {
    let source = r#"
structure Bracket {
    param x : Real
}

purpose check(subject : Structure) {
    constraint forall p in subject.geometric_params: determined(p)
}
"#;
    let compiled = parse_and_compile(source);
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "fixture failed to compile cleanly"
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    engine.activate_purpose("check", "Bracket");

    let snapshot = engine.snapshot().expect("snapshot after activate_purpose");

    let injected = snapshot
        .graph
        .constraints
        .iter()
        .find(|(id, _)| id.entity.starts_with("purpose:check@Bracket"))
        .map(|(_, data)| data.clone())
        .expect(
            "expected at least one constraint with entity prefix 'purpose:check@Bracket' \
             after activation",
        );

    let collection = match &injected.expr.kind {
        CompiledExprKind::Quantifier { collection, .. } => collection,
        other => panic!(
            "expected Quantifier in injected constraint expr, got {:?}",
            other
        ),
    };

    // (a) collection.kind is ReflectiveCellList (not the placeholder).
    // task-2458: must be ReflectiveCellList, not ListLiteral.
    let elements = match &collection.kind {
        CompiledExprKind::ReflectiveCellList(elements) => elements,
        CompiledExprKind::PurposeReflectiveAggregation { .. } => panic!(
            "post-activation: collection is still the `PurposeReflectiveAggregation` \
             placeholder; activate_purpose must expand the geometric_params \
             placeholder into an empty `ReflectiveCellList` (task-2458)"
        ),
        other => panic!(
            "post-activation: expected `ReflectiveCellList` in Quantifier collection \
             (task-2458), got {:?}",
            other
        ),
    };

    // (b) the ReflectiveCellList is empty.
    assert!(
        elements.is_empty(),
        "expected expanded geometric_params ReflectiveCellList to be empty (no resolved \
         query and no fallback heuristic for geometric_params yet — task-1904), \
         got {} elements",
        elements.len()
    );
}

// task-2289 amendment (reviewer S2, round 2): the integration-level
// precedence test that previously lived here was brittle — its witness
// (resolved-query path preserving declaration order `[z, a]` vs
// fallback scan sorting to `[a, z]`) depended on `compile_purpose`
// preserving template declaration order in `resolved_ids`, which is
// not a documented contract of `ResolvedSchemaQuery`. A future
// refactor that sorted inside the compiler would have made the
// post-activation assertion vacuous (both paths producing `[a, z]`)
// while leaving the test green. The replacement lives in
// `crates/reify-eval/src/engine_purposes.rs`'s `tests` module:
// `expand_prefers_resolved_query_over_value_cells_scan` drives
// `expand_purpose_reflective_placeholders` directly with a hand-crafted
// `ResolvedSchemaQuery`, pinning the precedence contract independently
// of compiler-internal ordering.

// ── §10: Composed-field reverse-index preservation across activate/deactivate ─
//
// Task-2343 step-13 regression test. Pins the cache invariant that the
// reviewer flagged: after `activate_purpose` (and again after
// `deactivate_purpose`) the reverse-dependency index must STILL register
// each composed field as a dependent of every captured field cell.
//
// Background. Initial `eval()` builds the reverse index via
// `ReverseDependencyIndex::build_from_graph_and_fields(&graph,
// &module.fields)` (engine_eval.rs), which iterates `module.fields` and
// surfaces composed-field deps via the augmented `Lambda { captures, .. }`
// injected by the compiler's `phase_augment_composed_captures` post-pass.
// `engine_edit.rs::edit_param` does the same. But `engine_purposes.rs`
// historically called the no-fields wrappers
// (`build_from_graph(&graph)` / `build_trace_map(&graph)`), which forward
// to the `_and_fields` variants with an empty fields slice — so every
// composed-field edge was DROPPED from the index after any
// activate/deactivate. That broke the cache invariant: a subsequent
// `edit_param` would see no dependents for `__field.<dep>` and skip
// re-elaborating composed fields whose captures became stale.
//
// This test pins both the activate path and the deactivate path so a
// future regression that re-introduces the no-fields wrappers in
// `engine_purposes.rs` fails loudly.

/// Pre-activation: confirm initial eval seeded the composed-field edge.
/// Then activate_purpose, then deactivate_purpose, and after each rebuild,
/// confirm the reverse_index still contains the f3 → f1 dependency edge
/// AND the trace_map's `Value(__field.f3)` entry still records f1 as a read.
#[test]
fn purpose_activation_preserves_composed_field_reverse_index() {
    use reify_eval::cache::NodeId;
    use reify_types::FIELD_ENTITY_PREFIX;

    let source = r#"
field def f1 : Real -> Real { source = analytical { |p| p * 2.0 } }
field def f3 : Real -> Real { source = composed { |p| f1(p) } }

structure def S {
    param k : Real = 2.0
}

purpose p1(subject : S) {
    constraint subject.k > 0.0
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    engine.eval(&compiled);

    let f1_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f1");
    let f3_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f3");
    let f3_node = NodeId::Value(f3_cell.clone());

    // (1) Pre-activation sanity: initial eval used `_and_fields` so the
    //     reverse-dep edge f1 → f3 is present. Without this assertion the
    //     full test could pass vacuously if the index were always empty.
    {
        let state = engine.eval_state().expect("eval_state after eval()");
        let f1_deps = state.reverse_index.dependents_of(&f1_cell);
        assert!(
            f1_deps.contains(&f3_node),
            "pre-activation sanity: dependents_of(__field.f1) should contain \
             Value(__field.f3) — initial eval must seed the composed-field edge \
             via build_from_graph_and_fields; got: {:?}",
            f1_deps
        );
    }

    // (2) After activate_purpose: the rebuild path inside engine_purposes.rs
    //     must use `_and_fields` so composed-field edges survive. If
    //     activate_purpose calls the no-fields wrapper, this assertion fails:
    //     the rebuilt index drops every composed-field edge.
    engine.activate_purpose("p1", "S");
    {
        let state = engine.eval_state().expect("eval_state after activate_purpose");
        let f1_deps = state.reverse_index.dependents_of(&f1_cell);
        assert!(
            f1_deps.contains(&f3_node),
            "post-activate: dependents_of(__field.f1) must STILL contain \
             Value(__field.f3) — engine_purposes.rs::activate_purpose must \
             rebuild the reverse index with the `_and_fields` variant, otherwise \
             composed-field edges are dropped; got: {:?}",
            f1_deps
        );

        // (3) Symmetric check on trace_map — the forward edge from f3 to its
        //     reads must include f1.
        let trace = state
            .trace_map
            .get(&f3_node)
            .expect("trace_map should contain Value(__field.f3) entry after activate");
        assert!(
            trace.reads.contains(&f1_cell),
            "post-activate: trace_map[__field.f3].reads must contain __field.f1 \
             — engine_purposes.rs::activate_purpose must rebuild the trace_map \
             with the `_and_fields` variant; got reads: {:?}",
            trace.reads
        );
    }

    // (4) After deactivate_purpose: same invariants must hold. Pins that the
    //     deactivate path also uses the `_and_fields` rebuild.
    engine.deactivate_purpose("p1");
    {
        let state = engine
            .eval_state()
            .expect("eval_state after deactivate_purpose");
        let f1_deps = state.reverse_index.dependents_of(&f1_cell);
        assert!(
            f1_deps.contains(&f3_node),
            "post-deactivate: dependents_of(__field.f1) must STILL contain \
             Value(__field.f3); got: {:?}",
            f1_deps
        );
        let trace = state
            .trace_map
            .get(&f3_node)
            .expect("trace_map should contain Value(__field.f3) entry after deactivate");
        assert!(
            trace.reads.contains(&f1_cell),
            "post-deactivate: trace_map[__field.f3].reads must contain __field.f1; \
             got reads: {:?}",
            trace.reads
        );
    }
}
