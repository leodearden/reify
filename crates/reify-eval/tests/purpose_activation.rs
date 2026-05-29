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
use reify_core::{ContentHash, ModulePath, Severity, Type, ValueCellId, VersionId};
use reify_ir::{CompiledExprKind, OptimizationObjective, Satisfaction};

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

// ── §2: eval() preserves active purpose state across re-eval (task 3103) ─────

#[test]
fn eval_preserves_active_purpose_state_across_re_eval() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after activation"
    );
    // Second eval — fresh snapshot; active_purpose_bindings (user intent) must
    // be preserved and re-injected into the new graph (task 3103).
    engine.eval(&compiled);
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose MUST still be active after a fresh eval() call — task 3103 preserves bindings"
    );
    // Re-activation should be an idempotent no-op (not blocked, not doubled)
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should still be active after a redundant activate_purpose call"
    );
}

/// Task 3103 (S4, reviewer) — the optimization objective re-injected by the
/// preserved-binding loop in `Engine::eval` must survive the call.
///
/// `activate_purpose_constraints` (called inside the loop) inserts into
/// `active_objective_map`; `rebuild_purpose_infrastructure` follows with the
/// single shared infrastructure rebuild.  A future refactor that stops
/// re-applying the objective during preserved-binding re-injection would leave
/// `active_objectives()` empty after the second eval — this test catches that.
#[test]
fn eval_preserves_optimization_objective_across_re_eval() {
    let compiled = parse_and_compile(MINIMIZE_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("lightweight", "Bracket");

    // Precondition: objective is active before re-eval.
    let objectives_before = engine.active_objectives();
    assert_eq!(
        objectives_before.len(),
        1,
        "precondition: one objective must be active after activation"
    );
    assert!(
        matches!(objectives_before[0], OptimizationObjective::Minimize(_)),
        "precondition: objective must be Minimize"
    );

    // Second eval — task 3103 preserves active_purpose_bindings and re-injects
    // all purposes (including their objectives) into the new graph.
    engine.eval(&compiled);

    let objectives_after = engine.active_objectives();
    assert_eq!(
        objectives_after.len(),
        1,
        "optimization objective must survive eval() — task 3103 preserved-binding \
         re-injection must re-insert into active_objective_map via \
         activate_purpose_constraints()"
    );
    assert!(
        matches!(objectives_after[0], OptimizationObjective::Minimize(_)),
        "re-injected objective must still be Minimize after eval()"
    );
}

/// Task 3103 (S4, reviewer) — a preserved binding for a purpose that is absent
/// from the new module must be silently dropped (no panic, `is_purpose_active`
/// returns false) after `eval()` on the new module.
///
/// `activate_purpose_constraints` early-returns `false` when the purpose name
/// is not found in `compiled_purposes`; `active_purpose_bindings` is therefore
/// not populated for that name, and `is_purpose_active` returns false. A future
/// refactor that skips the early-return guard would insert stale graph nodes and
/// leave `is_purpose_active` returning true — this test catches that.
#[test]
fn eval_drops_stale_binding_when_purpose_removed_from_module() {
    // Module A has the purpose.
    let module_with_purpose = parse_and_compile(SIMPLE_MFG_SRC);
    // Module B is the same structure with NO purpose declaration.
    let module_without_purpose = parse_and_compile(
        r#"
structure Bracket {
    param width : Length = 80mm
}
"#,
    );

    let mut engine = make_engine();
    engine.eval(&module_with_purpose);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "precondition: purpose must be active after activation"
    );

    // eval() on a module without the purpose: the preserved binding for
    // "mfg_ready" must be silently dropped — no panic, no stale injection.
    engine.eval(&module_without_purpose);

    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "stale binding must be dropped when the purpose is absent from the new \
         module — activate_purpose_constraints() must early-return false for an \
         unknown purpose name rather than inserting stale graph nodes"
    );
}

/// Regression-pinning characterization test for the eval_cached non-interference
/// contract (task 3260).
///
/// `eval_cached` (engine_eval.rs:2066+) must NOT clear `active_purposes`
/// (the constraint-IDs map) or `active_purpose_bindings` (the user-intent
/// HashMap that the preserved-binding loop in `eval()` consumes via
/// `mem::take`). A future change adding snapshot-rebuild behaviour to
/// `eval_cached` could silently regress the post-3103 invariant — this test
/// catches that.
///
/// This test passes immediately on the current code because eval_cached already
/// upholds the invariant; it is a regression-pinning characterization test, not
/// a RED→GREEN test.
///
/// Proof structure:
///   (a) `is_purpose_active` after `eval_cached` → `active_purposes` survived.
///   (b) `is_purpose_active` after a second `eval()` → `active_purpose_bindings`
///       survived eval_cached (the second eval's preserved-binding loop
///       consumes `active_purpose_bindings` via `mem::take`; if eval_cached
///       had cleared the field the loop would have nothing to re-inject and
///       `is_purpose_active` would return false).
#[test]
fn eval_cached_preserves_active_purpose_bindings_across_call() {
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();

    // (1) Initial eval.
    engine.eval(&compiled);
    // (2) Activate purpose.
    engine.activate_purpose("mfg_ready", "Bracket");
    // (3) Precondition.
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "precondition: purpose must be active after activate_purpose"
    );

    // (4) eval_cached must not disturb active_purposes or active_purpose_bindings,
    //     nor change the injected-purpose constraint count.
    let count_before = constraint_count(&engine);
    engine.eval_cached(&compiled, VersionId(0));
    assert_eq!(
        constraint_count(&engine),
        count_before,
        "eval_cached must not change the injected-purpose constraint count (task 3260)"
    );

    // (5) active_purposes survived.
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "eval_cached must not clear active_purposes — \
         is_purpose_active must return true immediately after eval_cached \
         (engine_eval.rs:2066+, task 3260)"
    );

    // (6) active_purpose_bindings survived: second eval() re-injects only if
    //     active_purpose_bindings is intact.
    engine.eval(&compiled);
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "active_purpose_bindings must survive eval_cached — \
         a second eval() re-injects preserved bindings only if the field was not \
         cleared by eval_cached; is_purpose_active must still be true (task 3260)"
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

    // Pre-activation: ValueRef entity must equal "weight_target::subject" (post-β stamp).
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
        pre_entity, "weight_target::subject",
        "pre-activation: ValueRef entity must equal 'purpose::param' (post-β stamp)"
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

// ── §5b: Multi-param activation refusal via single-entity shim (task-2181 β, PRD §4.5 C2) ──────

/// Verifies that activating a multi-param purpose via the single-entity `activate_purpose` shim
/// is refused (no-op + warn) rather than silently mis-binding all params to the same entity.
///
/// Background (PRD §4.5 contract C2):
///   After task-β removed the compile-time multi-StructureRef rejection, a 2-param purpose like
///   `fits_within(part, envelope)` can compile cleanly. However, the single-entity shim
///   `activate_purpose(name, entity_ref)` cannot safely bind it — applying one `entity_ref`
///   to every per-param `{purpose}::{param}` stamp aliases `part.length > envelope.length`
///   into `entity.length > entity.length`, a silently meaningless constraint.
///
///   C2 mandates refusal: `activate_purpose` must return without injecting any constraints
///   and `is_purpose_active` must remain false. Per-param binding is task γ's
///   `activate_purpose_with_bindings`.
///
/// RED state (step-5, before step-6 guard):
///   Assertions #4 and #5 fail — `activate_purpose` injects the mis-bound constraint and
///   marks the purpose active. Step-6 adds the `params.len() > 1` early-return guard.
#[test]
fn activate_multi_param_purpose_via_single_entity_shim_is_refused() {
    let source = r#"
structure Bracket {
    param length : Length = 80mm
}

purpose fits_within(part : Structure, envelope : Structure) {
    constraint part.length > envelope.length
}
"#;

    // Precondition: compiles cleanly post-β (no task-2201 rejection).
    // parse_and_compile panics on any error-severity diagnostic.
    let compiled = parse_and_compile(source);
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "precondition: fits_within must compile to exactly one purpose"
    );
    assert_eq!(
        compiled.compiled_purposes[0].params.len(),
        2,
        "precondition: fits_within must have exactly 2 params (part, envelope)"
    );

    let mut engine = make_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);

    // Attempt single-entity activation of a 2-param purpose.
    engine.activate_purpose("fits_within", "Bracket");

    // C2: refusal — purpose must NOT become active via the single-entity shim.
    assert!(
        !engine.is_purpose_active("fits_within"),
        "multi-param purpose must NOT activate via the single-entity shim \
         (PRD §4.5 C2: refusal, not silent mis-bind); activate_purpose_with_bindings \
         is the correct API (task γ)"
    );

    // C2: zero constraints injected — graph is unmodified.
    assert_eq!(
        constraint_count(&engine),
        before,
        "a refused multi-param activation must inject zero constraints (PRD §4.5 C2)"
    );
}

/// Reviewer regression: activating a ZERO-param purpose via the single-entity
/// `activate_purpose` shim must be refused (no-op + warn), NOT panic.
///
/// Background:
///   `activate_purpose_constraints` builds `vec![(purpose.params[0].name.clone(), …)]`
///   (engine_purposes.rs:138) after ONLY the `params.len() > 1` guard. A zero-param
///   purpose therefore reaches `purpose.params[0]` and panics index-out-of-bounds.
///   Zero-param purposes compile cleanly: the grammar's `commaSep` accepts an empty
///   `()`, `compile_purpose` has no zero-param rejection, and `constraint 1 > 0` is a
///   literal constraint used pervasively across this suite with no diagnostic.
///
///   The single-entity shim binds exactly one entity to exactly one param; with zero
///   params there is nothing to bind, so refusal (warn + return false) is the correct
///   contract — mirroring `activate_multi_param_purpose_via_single_entity_shim_is_refused`.
///   No capability is lost: zero-param purposes remain activatable via
///   `activate_purpose_with_bindings(name, &[])` (C2/C3 pass vacuously).
///
/// RED state (before step-13): `activate_purpose` panics at `purpose.params[0]`
/// instead of refusing. Step-13 widens the refusal guard from `> 1` to `!= 1`.
#[test]
fn activate_zero_param_purpose_via_single_entity_shim_does_not_panic() {
    let source = r#"
structure Bracket {
    param length : Length = 80mm
}

purpose always_ok() {
    constraint 1 > 0
}
"#;

    // Precondition: compiles cleanly to exactly one zero-param purpose.
    // parse_and_compile panics on any error-severity diagnostic.
    let compiled = parse_and_compile(source);
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "precondition: always_ok must compile to exactly one purpose"
    );
    assert_eq!(
        compiled.compiled_purposes[0].params.len(),
        0,
        "precondition: always_ok must have zero params"
    );

    let mut engine = make_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);

    // Single-entity activation of a zero-param purpose.
    // TODAY THIS PANICS at `purpose.params[0]` (index out of bounds).
    engine.activate_purpose("always_ok", "Bracket");

    // Refusal contract (mirrors the multi-param refusal): purpose must NOT
    // activate, and zero constraints injected.
    assert!(
        !engine.is_purpose_active("always_ok"),
        "zero-param purpose must NOT activate via the single-entity shim \
         (the shim binds exactly one param; refuse rather than panic)"
    );
    assert_eq!(
        constraint_count(&engine),
        before,
        "a refused zero-param activation must inject zero constraints"
    );
}

// ── §5c: Objective ValueRef remap on activation (task-2181 β, reviewer test_coverage) ───────────

/// Verifies that the inner `ValueRef` of a `minimize subject.mass` objective is remapped
/// from the per-param stamp (`weight_target::subject`) to the bound entity (`Bracket`)
/// on activation (task-2181 β objective remap path, engine_purposes.rs:141-151).
///
/// The existing objective tests (`minimize_objective_injected`, `maximize_objective_injected`)
/// use literal expressions (e.g. `80mm + 60mm`) that contain no purpose-param `ValueRef`s —
/// they verify the *presence* of an objective variant but do not exercise the per-param
/// entity remap for objectives. This test closes that gap.
///
/// Flow:
///  1. Compile `purpose weight_target(subject : Structure) { minimize subject.mass }`.
///  2. Pre-activation: assert objective's inner `ValueRef` entity == `"weight_target::subject"`
///     (the β per-param stamp).
///  3. `engine.activate_purpose("weight_target", "Bracket")`.
///  4. Post-activation: assert the active objective's inner `ValueRef` entity == `"Bracket"`.
#[test]
fn minimize_objective_valueref_remapped_to_bound_entity_on_activation() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose weight_target(subject : Structure) {
    minimize subject.mass
}
"#;

    let compiled = parse_and_compile(source);
    assert_eq!(compiled.compiled_purposes.len(), 1);
    let purpose = &compiled.compiled_purposes[0];
    assert_eq!(purpose.name, "weight_target");

    // Pre-activation: objective's inner ValueRef entity must equal the β per-param stamp.
    let pre_obj_entity = match purpose
        .objective
        .as_ref()
        .expect("weight_target must have a minimize objective")
    {
        OptimizationObjective::Minimize(expr) => match &expr.kind {
            CompiledExprKind::ValueRef(id) => id.entity.clone(),
            other => panic!(
                "pre-activation: expected ValueRef inside Minimize objective, got {:?}",
                other
            ),
        },
        other => panic!(
            "pre-activation: expected Minimize objective, got {:?}",
            other
        ),
    };
    assert_eq!(
        pre_obj_entity, "weight_target::subject",
        "pre-activation: objective ValueRef entity must equal 'purpose::param' (post-β stamp)"
    );

    // Activate against Bracket — the per-param remap loop in
    // `activate_purpose_constraints` (engine_purposes.rs:141-151) rewrites the
    // objective expression in lockstep with the constraint expressions.
    let mut engine = make_engine();
    engine.eval(&compiled);
    engine.activate_purpose("weight_target", "Bracket");

    let objectives = engine.active_objectives();
    assert_eq!(objectives.len(), 1, "should have 1 active objective after activation");

    // Post-activation: the active objective's ValueRef entity must be remapped to "Bracket".
    let post_obj_entity = match objectives[0] {
        OptimizationObjective::Minimize(expr) => match &expr.kind {
            CompiledExprKind::ValueRef(id) => id.entity.clone(),
            other => panic!(
                "post-activation: expected ValueRef inside Minimize objective, got {:?}",
                other
            ),
        },
        other => panic!(
            "post-activation: expected Minimize objective, got {:?}",
            other
        ),
    };
    assert_eq!(
        post_obj_entity, "Bracket",
        "post-activation: remap_entity must rewrite objective ValueRef entity \
         from 'weight_target::subject' (β stamp) to 'Bracket' (bound entity)"
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
    use reify_core::FIELD_ENTITY_PREFIX;

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
        let state = engine
            .eval_state()
            .expect("eval_state after activate_purpose");
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

// ── §11: Multi-param activation via activate_purpose_with_bindings (task γ) ──

/// B1 / RED (step-01): `activate_purpose_with_bindings` remaps each param to
/// its own distinct entity, producing per-param `ValueRef` entities in the
/// injected constraint expression.
///
/// Inline source: two structures with a same-named param but distinct values,
/// and a 2-param purpose that constraints `part.length < envelope.length`.
/// Distinct binding → 80mm < 100mm → the expr has PartA on the left and BoxB
/// on the right; aliased binding would give PartA on both sides.
///
/// Verifies the DISTINCT-binding structural property by inspecting injected
/// constraint expression ValueRef entities directly (not the eval outcome).
///
/// RED because `Engine::activate_purpose_with_bindings` does not yet exist.
#[test]
fn activate_purpose_with_bindings_remaps_each_param_to_distinct_entity() {
    let source = r#"
structure PartA { param length : Length = 80mm }
structure BoxB { param length : Length = 100mm }
purpose fits_within(part : Structure, envelope : Structure) {
    constraint part.length < envelope.length
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_simple_engine();
    engine.eval(&compiled);

    // Call the new API — RED: this method does not yet exist.
    let result = engine.activate_purpose_with_bindings(
        "fits_within",
        &[
            ("part".to_string(), "PartA".to_string()),
            ("envelope".to_string(), "BoxB".to_string()),
        ],
    );
    assert!(
        result.is_ok(),
        "expected Ok from activate_purpose_with_bindings, got {:?}",
        result
    );

    assert!(
        engine.is_purpose_active("fits_within"),
        "purpose should be active after activate_purpose_with_bindings"
    );

    let snapshot = engine.snapshot().expect("snapshot after activate");

    // Find the injected constraint by purpose prefix.
    let (constraint_id, data) = snapshot
        .graph
        .constraints
        .iter()
        .find(|(id, _)| id.entity.starts_with("purpose:fits_within@"))
        .expect("expected injected constraint with entity prefix 'purpose:fits_within@'");

    // B1: expr must be a BinOp with DISTINCT ValueRef entities for each param.
    // `constraint part.length < envelope.length` compiles to BinOp(Less, left, right).
    let (left_entity, right_entity) = match &data.expr.kind {
        CompiledExprKind::BinOp { left, right, .. } => {
            let left_ent = match &left.kind {
                CompiledExprKind::ValueRef(id) => id.entity.clone(),
                other => panic!("expected ValueRef in BinOp left, got {:?}", other),
            };
            let right_ent = match &right.kind {
                CompiledExprKind::ValueRef(id) => id.entity.clone(),
                other => panic!("expected ValueRef in BinOp right, got {:?}", other),
            };
            (left_ent, right_ent)
        }
        other => panic!("expected BinOp in injected constraint expr, got {:?}", other),
    };
    assert_eq!(
        left_entity, "PartA",
        "left operand (part.length) must resolve to PartA after per-param remap"
    );
    assert_eq!(
        right_entity, "BoxB",
        "right operand (envelope.length) must resolve to BoxB after per-param remap"
    );

    // Multi-binding entity must use the digest prefix (not a raw entity name).
    // Canonical = bindings sorted by param name, each "{param}={entity}", joined by ","
    // sorted: "envelope" < "part" → "envelope=BoxB,part=PartA"
    let canonical = "envelope=BoxB,part=PartA";
    let digest = ContentHash::of_str(canonical);
    let expected_entity = format!("purpose:fits_within@{}", digest);
    assert_eq!(
        constraint_id.entity, expected_entity,
        "multi-binding activation must produce entity '{}', got '{}'",
        expected_entity, constraint_id.entity
    );
}

/// C3 / RED (step-03): `activate_purpose_with_bindings` must return Err when a
/// binding names a param not declared by the purpose, and must NOT inject any
/// constraints.
///
/// RED because step-02's implementation does not yet validate C3.
/// (Step-04 adds the C3 guard.)
#[test]
fn activate_with_bindings_unknown_param_is_diagnostic() {
    let source = r#"
structure PartA { param length : Length = 80mm }
structure BoxB { param length : Length = 100mm }
purpose fits_within(part : Structure, envelope : Structure) {
    constraint part.length < envelope.length
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);

    // "bogus" is not a declared param of fits_within.
    let result = engine.activate_purpose_with_bindings(
        "fits_within",
        &[
            ("part".to_string(), "PartA".to_string()),
            ("bogus".to_string(), "BoxB".to_string()),
        ],
    );

    // C3: must return Err naming the unknown param.
    let err_msg = result.expect_err(
        "expected Err for an unknown binding param, got Ok"
    );
    assert!(
        err_msg.contains("bogus"),
        "error message must name the unknown param 'bogus', got: {err_msg}"
    );

    // No injection must have occurred.
    assert!(
        !engine.is_purpose_active("fits_within"),
        "purpose must NOT be active after a C3 validation failure"
    );
    assert_eq!(
        constraint_count(&engine),
        before,
        "zero constraints must be injected on a C3 validation failure"
    );
}

/// C2 / (step-05): `activate_purpose_with_bindings` must return Err when a
/// declared purpose param is missing from the bindings, and must NOT inject
/// any constraints.
///
/// Note: C2 validation was included in step-02's implementation, so this test
/// is GREEN immediately (not RED as planned). It documents and pins the C2
/// contract correctly.
#[test]
fn activate_with_bindings_unbound_param_is_diagnostic() {
    let source = r#"
structure PartA { param length : Length = 80mm }
structure BoxB { param length : Length = 100mm }
purpose fits_within(part : Structure, envelope : Structure) {
    constraint part.length < envelope.length
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);

    // Only "part" is bound; "envelope" is missing.
    let result = engine.activate_purpose_with_bindings(
        "fits_within",
        &[("part".to_string(), "PartA".to_string())],
    );

    // C2: must return Err naming the unbound param.
    let err_msg = result.expect_err(
        "expected Err for an unbound purpose param, got Ok"
    );
    assert!(
        err_msg.contains("envelope"),
        "error message must name the unbound param 'envelope', got: {err_msg}"
    );

    // No injection must have occurred.
    assert!(
        !engine.is_purpose_active("fits_within"),
        "purpose must NOT be active after a C2 validation failure"
    );
    assert_eq!(
        constraint_count(&engine),
        before,
        "zero constraints must be injected on a C2 validation failure"
    );
}

/// C7 / RED (step-07): when a 2-param purpose has reflective members on BOTH
/// params, `activate_purpose_with_bindings` must resolve the `a.params` and
/// `b.params` placeholders to their RESPECTIVE bound entities — NOT to a single
/// representative entity.
///
/// RED because step-02 passes `bindings[0].1` as the representative entity to
/// `expand_purpose_reflective_placeholders` for all placeholders, so the
/// `b.params` placeholder mis-resolves to `Sa` instead of `Sb`. Step-08 will
/// fix this by passing the full bindings slice for per-param entity lookup.
#[test]
fn activate_with_bindings_resolves_reflective_query_per_param() {
    // Sa has param "pa"; Sb has param "pb" — DISTINCT member names.
    // A mis-bind would put Sa's entity on both constraints.
    let source = r#"
structure Sa { param pa : Real }
structure Sb { param pb : Real }
purpose pp(a : Structure, b : Structure) {
    constraint forall x in a.params: determined(x)
    constraint forall y in b.params: determined(y)
}
"#;
    let compiled = parse_and_compile(source);
    assert_eq!(compiled.compiled_purposes.len(), 1, "fixture must compile");

    let mut engine = make_simple_engine();
    engine.eval(&compiled);

    let result = engine.activate_purpose_with_bindings(
        "pp",
        &[
            ("a".to_string(), "Sa".to_string()),
            ("b".to_string(), "Sb".to_string()),
        ],
    );
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert!(engine.is_purpose_active("pp"));

    let snapshot = engine.snapshot().expect("snapshot after activate");

    // Collect the two injected purpose constraints.
    let mut injected: Vec<_> = snapshot
        .graph
        .constraints
        .iter()
        .filter(|(id, _)| id.entity.starts_with("purpose:pp@"))
        .collect();
    // Sort by constraint index for determinism.
    injected.sort_by_key(|(id, _)| id.index);
    assert_eq!(injected.len(), 2, "expected exactly 2 injected constraints");

    // Helper: extract entities from a ReflectiveCellList inside a Quantifier.
    let rcl_entities = |data: &reify_eval::graph::ConstraintNodeData| -> Vec<String> {
        let collection = match &data.expr.kind {
            CompiledExprKind::Quantifier { collection, .. } => collection,
            other => panic!("expected Quantifier, got {:?}", other),
        };
        match &collection.kind {
            CompiledExprKind::ReflectiveCellList(elements) => elements
                .iter()
                .map(|e| match &e.kind {
                    CompiledExprKind::ValueRef(id) => id.entity.clone(),
                    other => panic!("expected ValueRef, got {:?}", other),
                })
                .collect(),
            other => panic!("expected ReflectiveCellList, got {:?}", other),
        }
    };

    // First constraint (index 0): `forall x in a.params: determined(x)`
    // a→Sa, so collection elements must have entity "Sa".
    let entities_0 = rcl_entities(injected[0].1);
    assert!(
        !entities_0.is_empty(),
        "a.params constraint (index 0) must expand to a non-empty ReflectiveCellList"
    );
    for entity in &entities_0 {
        assert_eq!(
            entity, "Sa",
            "a.params constraint must reference entity 'Sa', got '{entity}'"
        );
    }

    // Second constraint (index 1): `forall y in b.params: determined(y)`
    // b→Sb, so collection elements must have entity "Sb".
    let entities_1 = rcl_entities(injected[1].1);
    assert!(
        !entities_1.is_empty(),
        "b.params constraint (index 1) must expand to a non-empty ReflectiveCellList"
    );
    for entity in &entities_1 {
        assert_eq!(
            entity, "Sb",
            "b.params constraint must reference entity 'Sb' (not 'Sa'), got '{entity}'"
        );
    }
}

/// C6 parity / RED (step-01): `activate_purpose_with_bindings` with a single
/// binding must produce the same `purpose:{name}@{entity}` prefix as the
/// existing `activate_purpose` shim — NO digest in the single-binding path.
///
/// RED because `Engine::activate_purpose_with_bindings` does not yet exist.
#[test]
fn activate_with_bindings_single_param_keeps_entity_prefix() {
    // SIMPLE_MFG_SRC: `purpose mfg_ready(subject : Structure) { constraint 1 > 0 }`
    let compiled = parse_and_compile(SIMPLE_MFG_SRC);
    let mut engine = make_engine();
    engine.eval(&compiled);

    // Single-param: C6 parity path — entity, NOT digest.
    let result = engine.activate_purpose_with_bindings(
        "mfg_ready",
        &[("subject".to_string(), "Bracket".to_string())],
    );
    assert!(
        result.is_ok(),
        "expected Ok from activate_purpose_with_bindings, got {:?}",
        result
    );
    assert!(engine.is_purpose_active("mfg_ready"), "purpose should be active");

    let snapshot = engine.snapshot().expect("snapshot after activate");

    // C6: entity must be exactly "purpose:mfg_ready@Bracket" (no digest).
    let injected_entity = snapshot
        .graph
        .constraints
        .keys()
        .find(|id| id.entity.starts_with("purpose:mfg_ready@"))
        .map(|id| id.entity.as_str())
        .expect("expected injected constraint with purpose:mfg_ready@ prefix");

    assert_eq!(
        injected_entity,
        "purpose:mfg_ready@Bracket",
        "single-binding activation must use '@{{entity}}' (C6 parity, no digest)"
    );
}
