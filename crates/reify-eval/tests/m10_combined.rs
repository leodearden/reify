//! M10 combined integration tests.
//!
//! Exercises all M10 milestone features in a single cohesive example:
//! geometric type params, Point/Vector arithmetic in lets, Frame/Transform
//! in port definitions, connect with connector type and port mapping, purpose
//! checking geometric determinacy, ad-hoc port selector, and where-block
//! reference safety. Uses examples/m10_combined.ri as the source file.

use reify_compiler::CompiledModule;
use reify_core::{ModulePath, ValueCellId};
use reify_eval::Engine;
use reify_ir::{Satisfaction, Value};
use reify_test_support::{
    check_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib,
};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_combined.ri"
);

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read and return the contents of the m10_combined.ri example file.
/// The file is read only once per test process (cached in a `OnceLock`);
/// each caller receives an owned clone.
fn source() -> String {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m10_combined.ri should exist")
    })
    .clone()
}

/// Parse and compile the canonical source file, caching the result.
/// Most tests use this to avoid repeating the stdlib compile pipeline.
/// Tests that need a mutated source must call `parse_and_compile_with_stdlib` directly.
fn compiled() -> CompiledModule {
    static C: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(&source()))
        .clone()
}

/// Eval the canonical source with SimpleConstraintChecker, return EvalResult.
/// Uses the cached compiled module to avoid a redundant compile.
fn eval_canonical() -> reify_eval::EvalResult {
    let mut engine = make_simple_engine();
    engine.eval(&compiled())
}

/// Check the canonical source with SimpleConstraintChecker, return CheckResult.
/// Uses the cached compiled module to avoid a redundant compile.
fn check_canonical() -> reify_eval::CheckResult {
    let mut engine = make_simple_engine();
    engine.check(&compiled())
}

/// Returns the constraint count from the current engine snapshot.
/// Mirrors the helper in purpose_activation.rs.
fn constraint_count(engine: &Engine) -> usize {
    engine
        .snapshot()
        .expect("snapshot should exist")
        .graph
        .constraints
        .len()
}

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m10_combined.ri, verify it parses without errors, and assert the expected
/// top-level declaration structure: 1 Trait (FluidPort), 2 Structures (BoltSet, Assembly),
/// 1 Purpose (geom_ready). This gives more signal than a pure parse check — if a
/// declaration silently disappears the count assertion catches it independently of
/// whether compile or eval tests happen to surface the same failure.
#[test]
fn m10_combined_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(&src, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    use reify_ast::Declaration;

    let trait_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Trait(_)))
        .count();
    let structure_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Structure(_)))
        .count();
    let purpose_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Purpose(_)))
        .count();

    assert_eq!(
        trait_count, 1,
        "expected 1 Trait declaration (FluidPort), got {trait_count}"
    );
    assert_eq!(
        structure_count, 2,
        "expected 2 Structure declarations (BoltSet, Assembly), got {structure_count}"
    );
    assert_eq!(
        purpose_count, 1,
        "expected 1 Purpose declaration (geom_ready), got {purpose_count}"
    );
}

// ── Test 2: compiles with Assembly template ──────────────────────────────────

/// Compile m10_combined.ri (with stdlib) and verify the compiled module contains
/// an Assembly template (confirming compile-cleanliness and top-level structure name).
#[test]
fn m10_combined_compiles_with_assembly_template() {
    let compiled = compiled();
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("should have an Assembly template");
    assert!(
        !assembly.value_cells.is_empty(),
        "Assembly should have value cells (params and lets)"
    );
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Smoke test: file produces constraint results and all are Satisfied.
/// Complements `total_constraint_count_meets_threshold`, which additionally asserts count >= 15.
#[test]
fn all_constraints_satisfied() {
    let check_result = check_canonical();

    // Must have at least some constraint results
    assert!(
        !check_result.constraint_results.is_empty(),
        "expected at least one constraint result"
    );

    // Every entry must be exactly Satisfied (Violated and Indeterminate both fail)
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── Test 4: total constraint count meets threshold ───────────────────────────

/// Capstone assertion: constraint_results.len() >= 15, all Satisfied.
/// Guards against silent regressions where constraints are silently dropped.
/// The example produces 18 total: 15 always-active + 2 where-block + 1 compat.
#[test]
fn total_constraint_count_meets_threshold() {
    let check_result = check_canonical();

    let n = check_result.constraint_results.len();
    assert!(n >= 15, "expected >= 15 total constraint results, got {n}");

    // All must be Satisfied (defensive double-check complementing all_constraints_satisfied)
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── Test 5: geometric let bindings are determined ────────────────────────────

/// Feature 1 (geometric type params) + Feature 2 (Point/Vector arithmetic in lets).
/// Asserts that origin, target, offset, displacement, shifted, and total_offset
/// are present in the eval result as non-Undef values.
#[test]
fn geometric_let_bindings_determined() {
    let result = eval_canonical();

    let assert_determined = |name: &str| {
        let id = ValueCellId::new("Assembly", name);
        let v = result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("Assembly.{name} not found in eval result"));
        assert!(
            !matches!(v, Value::Undef),
            "Assembly.{name} should not be Undef (expected a concrete geometric value)"
        );
    };

    // Feature 1: geometric values constructed via stdlib (point3 / vec3)
    assert_determined("origin");
    assert_determined("target");
    assert_determined("offset");

    // Feature 2: arithmetic results (Point - Point → Vector, Point + Vector → Point, Vector + Vector → Vector)
    assert_determined("displacement");
    assert_determined("shifted");
    assert_determined("total_offset");
}

// ── Test 6: frame/transform lets and port frames present ─────────────────────

/// Feature 3 (Frame/Transform in let bindings and port definitions).
/// Asserts rot/base/mount/xform hold their expected Value variants and that
/// supply/demand ports have frame_expr compiled (confirming `frame = base`
/// and `frame = mount`).
///
/// Note: `rot = orient_identity()` triggers a compiler warning
/// ("cannot infer return type of zero-arg function 'orient_identity', defaulting to Real")
/// but the warning is NOT an error, so compilation succeeds. At runtime the function
/// still returns `Value::Orientation` regardless of the inferred type annotation.
#[test]
fn frame_transform_lets_and_port_frames_present() {
    let compiled = compiled();

    // (a) Eval assertions: check each binding against its expected Value variant.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let get_val = |name: &str| -> &Value {
        let id = ValueCellId::new("Assembly", name);
        result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("Assembly.{name} not found in eval result"))
    };

    // rot = orient_identity() → Orientation (unit quaternion), regardless of type-inference warning
    assert!(
        matches!(get_val("rot"), Value::Orientation { .. }),
        "Assembly.rot should be Value::Orientation, got {:?}",
        get_val("rot")
    );
    // base = frame3(origin, rot) → Frame
    assert!(
        matches!(get_val("base"), Value::Frame { .. }),
        "Assembly.base should be Value::Frame, got {:?}",
        get_val("base")
    );
    // mount = frame3(target, rot) → Frame
    assert!(
        matches!(get_val("mount"), Value::Frame { .. }),
        "Assembly.mount should be Value::Frame, got {:?}",
        get_val("mount")
    );
    // xform = transform3(rot, offset) → Transform
    assert!(
        matches!(get_val("xform"), Value::Transform { .. }),
        "Assembly.xform should be Value::Transform, got {:?}",
        get_val("xform")
    );

    // (b) Compile assertions: supply and demand ports both have a frame_expr
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    for port_name in &["supply", "demand"] {
        let port = assembly
            .ports
            .iter()
            .find(|p| p.name == *port_name)
            .unwrap_or_else(|| panic!("Assembly should have a '{port_name}' port"));
        assert!(
            port.frame_expr.is_some(),
            "Assembly.{port_name} port should have frame_expr (from `frame = base|mount`)"
        );
    }
}

// ── Test 7: assembly connect has connector and explicit port mapping ──────────

/// Feature 4 (connect with connector type + explicit port mapping).
/// Asserts assembly has exactly 1 connection, connector_sub is present,
/// port_mappings contains diameter→diameter and flow_rate→flow_rate,
/// and the compatibility constraint is Satisfied.
///
/// Note: `connector_sub` stores an auto-generated name like `__connector_0`,
/// not the connector type name (BoltSet). We assert `.is_some()` rather than
/// comparing to a string — if the identifier didn't resolve, compilation would error.
#[test]
fn assembly_connect_has_connector_and_explicit_mapping() {
    let compiled = compiled();
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    // Exactly one connection (supply -> demand : BoltSet)
    assert_eq!(
        assembly.connections.len(),
        1,
        "Assembly should have exactly 1 connection, got {}",
        assembly.connections.len()
    );

    let conn = &assembly.connections[0];

    // (a) Connector sub is present (the `: BoltSet` produced a connector sub)
    assert!(
        conn.connector_sub.is_some(),
        "connection should have a connector_sub (from `: BoltSet`)"
    );

    // (b) Explicit port mappings contain diameter→diameter and flow_rate→flow_rate
    assert!(
        conn.port_mappings.len() >= 2,
        "expected >= 2 port mappings (diameter, flow_rate), got {}",
        conn.port_mappings.len()
    );
    assert!(
        conn.port_mappings
            .iter()
            .any(|(from, to)| from == "diameter" && to == "diameter"),
        "port_mappings should contain (diameter, diameter); got {:?}",
        conn.port_mappings
    );
    assert!(
        conn.port_mappings
            .iter()
            .any(|(from, to)| from == "flow_rate" && to == "flow_rate"),
        "port_mappings should contain (flow_rate, flow_rate); got {:?}",
        conn.port_mappings
    );

    // (c) Compatibility constraint is Satisfied
    let compat_id = &conn.compatibility_constraint;
    let check_result = check_canonical();
    let compat_entry = check_result
        .constraint_results
        .iter()
        .find(|e| &e.id == compat_id)
        .unwrap_or_else(|| {
            panic!("compatibility constraint {compat_id} not found in check results")
        });
    assert_eq!(
        compat_entry.satisfaction,
        Satisfaction::Satisfied,
        "compatibility constraint should be Satisfied"
    );
}

// ── Test 8: geom_ready purpose compiled and activatable ──────────────────────

/// Feature 5 (purpose checking geometric determinacy).
/// Asserts the geom_ready purpose is compiled with Structure subject kind,
/// can be activated against Assembly, injects exactly 3 constraints, and
/// is cleanly deactivatable.
#[test]
fn geom_ready_purpose_compiled_and_activatable() {
    let compiled = compiled();

    // (a) geom_ready purpose is present in compiled_purposes
    let geom_ready = compiled
        .compiled_purposes
        .iter()
        .find(|p| p.name == "geom_ready")
        .expect("geom_ready purpose should be in compiled_purposes");

    // (b) param has entity_kind == "Structure"
    assert_eq!(
        geom_ready.params.len(),
        1,
        "geom_ready should have exactly 1 param (subject)"
    );
    assert_eq!(
        geom_ready.params[0].entity_kind, "Structure",
        "geom_ready param entity_kind should be 'Structure', got '{}'",
        geom_ready.params[0].entity_kind
    );

    // (c) activate against Assembly and assert is_purpose_active
    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    let before = constraint_count(&engine);

    engine.activate_purpose("geom_ready", "Assembly");
    assert!(
        engine.is_purpose_active("geom_ready"),
        "geom_ready should be active after activate_purpose"
    );

    // (d) constraint count grows by exactly 3 (three literal constraints in purpose body)
    assert_eq!(
        constraint_count(&engine),
        before + 3,
        "geom_ready has 3 literal constraints: count should grow by exactly 3 (got {} before, {} after)",
        before,
        constraint_count(&engine)
    );

    // (e) deactivate restores count
    engine.deactivate_purpose("geom_ready");
    assert_eq!(
        constraint_count(&engine),
        before,
        "deactivating geom_ready must restore constraint count to {} (got {})",
        before,
        constraint_count(&engine)
    );
    assert!(
        !engine.is_purpose_active("geom_ready"),
        "geom_ready should NOT be active after deactivate_purpose"
    );
}

// ── Test 9: where-block nested constraints present and satisfied ──────────────

/// Feature 7 (reference safety in where blocks).
/// Asserts that the two nested constraints inside the where-block
/// (`where determined(origin) { constraint determined(shifted); constraint determined(total_offset) }`)
/// are present in the check results and both Satisfied.
///
/// Implementation strategy: extract the guarded-group constraint IDs from the compiled
/// template (canonical source of truth), then look them up in check_source results.
/// Since `determined(origin)` is satisfied (origin evaluates to a concrete Point3),
/// the where-block is active and both nested constraints should be checked.
#[test]
fn where_block_nested_constraints_present_and_satisfied() {
    let compiled = compiled();
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    // The where-block should produce exactly 1 guarded group with 2 constraints
    assert_eq!(
        assembly.guarded_groups.len(),
        1,
        "Assembly should have exactly 1 guarded group (the where determined(origin) block)"
    );
    let group = &assembly.guarded_groups[0];
    assert_eq!(
        group.constraints.len(),
        2,
        "where-block should have exactly 2 constraints (determined(shifted), determined(total_offset))"
    );

    // Extract the IDs of the guarded constraints
    let guarded_ids: Vec<_> = group.constraints.iter().map(|c| &c.id).collect();

    // Verify both appear in check_canonical results as Satisfied
    let check_result = check_canonical();
    for guarded_id in &guarded_ids {
        let entry = check_result
            .constraint_results
            .iter()
            .find(|e| &&e.id == guarded_id)
            .unwrap_or_else(|| {
                panic!(
                    "where-block constraint {} not found in check results; all ids: {:?}",
                    guarded_id,
                    check_result
                        .constraint_results
                        .iter()
                        .map(|e| &e.id)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "where-block constraint {} should be Satisfied, got {:?}",
            guarded_id,
            entry.satisfaction
        );
    }
}

// ── Test 9b: where-block guard inactive — nested constraints absent ───────────

/// Negative-path guard for Feature 7: when the where-block guard condition is false
/// at runtime, the nested constraints must NOT appear in the active check set.
///
/// The mutation changes `where determined(origin)` to `where ox > 100000mm`. Since
/// `ox` defaults to 0mm, `0mm > 100000mm` evaluates to false (Violated), making
/// the guard inactive. Nested constraints that were active in the canonical source
/// must NOT appear in the check results here.
///
/// Note: `determined()` operates on DeterminacyState (a static/compiled property),
/// so a let-bound `origin = point3(...)` is always Determined regardless of
/// component param values. To exercise the guard's inactive path we therefore
/// substitute a runtime comparison predicate instead.
///
/// This guards against a regression where guarded constraints leak into the active
/// set even when their guard condition evaluates to false at runtime.
#[test]
fn where_block_guard_inactive_constraints_absent() {
    // Change the guard to a known-false runtime condition: ox = 0mm, so 0mm > 100000mm = false.
    let inactive_src = source().replace("where determined(origin)", "where ox > 100000mm");

    // Guard: the substitution must have fired.
    assert_ne!(
        inactive_src,
        source(),
        "source mutation failed — 'where determined(origin)' not found; update the substitution"
    );

    // Extract guarded constraint IDs from the compiled template (canonical).
    let canonical = compiled();
    let assembly = canonical
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");
    let guarded_ids: Vec<_> = assembly
        .guarded_groups
        .iter()
        .flat_map(|g| g.constraints.iter().map(|c| c.id.clone()))
        .collect();
    assert!(
        !guarded_ids.is_empty(),
        "expected at least one guarded constraint to test against"
    );

    // Check the mutated source — guarded constraints should be absent.
    let check_result = check_source_with_stdlib(&inactive_src);
    for guarded_id in &guarded_ids {
        assert!(
            !check_result
                .constraint_results
                .iter()
                .any(|e| &e.id == guarded_id),
            "guarded constraint {guarded_id} should NOT appear in check results \
             when the where-block guard (determined(origin)) is inactive, \
             but it was found in: {:?}",
            check_result
                .constraint_results
                .iter()
                .map(|e| &e.id)
                .collect::<Vec<_>>()
        );
    }
}

// ── Test 10: ad-hoc port selector let binding present ────────────────────────

/// Feature 6 (ad-hoc port selector).
/// Asserts that `supply_point` (from `let supply_point = supply @ point(0mm, 0mm, 0mm)`)
/// is present as a value cell in the Assembly template and also present in eval results.
///
/// NOTE: Ad-hoc selectors currently evaluate to Value::Undef in the pure expression
/// evaluator (reify-expr/src/lib.rs:511). We intentionally do NOT assert on the
/// payload — only on presence — so this test remains valid when engine-side geometry
/// resolution lands later. See reify-expr/src/lib.rs:511 for the Undef quirk.
/// A forward-marker test below (`supply_point_resolves_to_concrete_value`) will
/// catch when the resolver is wired up and the Undef behaviour is fixed.
#[test]
fn ad_hoc_port_selector_let_binding_present() {
    let compiled = compiled();

    // (a) Compiled template has a value cell named supply_point
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    let has_supply_point = assembly
        .value_cells
        .iter()
        .any(|c| c.id.member == "supply_point");
    assert!(
        has_supply_point,
        "Assembly template should have a value cell named 'supply_point' (from `let supply_point = supply @ point(...)`)"
    );

    // (b) Eval result contains the supply_point key (payload may be Undef — see note above)
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let supply_point_id = ValueCellId::new("Assembly", "supply_point");
    assert!(
        result.values.contains(&supply_point_id),
        "eval result should contain Assembly.supply_point; Assembly-entity keys present: {:?}",
        result
            .values
            .iter()
            .filter(|(k, _)| k.entity == "Assembly")
            .map(|(k, _)| k.member.as_str())
            .collect::<Vec<_>>()
    );
}

/// `supply_point = supply @ point(0mm, 0mm, 0mm)` — a `@point` selector against
/// a named step. The pure-expression evaluator (step-2 of task 3463) handles
/// `SelectorKind::Point` directly in `eval_expr`, so this resolves to a
/// `Value::Frame` without any kernel or engine post-process.
///
/// Previously `#[ignore]`-d because the blanket `AdHocSelector => Undef` arm was
/// still in place. Now active: `supply_point` must be a `Value::Frame`.
///
/// Witness: `m10_combined.ri` line 88 — `let supply_point = supply @ point(0mm, 0mm, 0mm)`.
#[test]
fn supply_point_resolves_to_concrete_value() {
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled());
    let supply_point_id = ValueCellId::new("Assembly", "supply_point");
    let v = result
        .values
        .get(&supply_point_id)
        .expect("Assembly.supply_point should exist");
    assert!(
        matches!(v, Value::Frame { .. }),
        "supply_point should resolve to Value::Frame {{ .. }} after task-3463 step-2 \
         wired @point in eval_expr; got {:?}",
        v
    );
}

// ── Test 11: violation regression guard ──────────────────────────────────────

/// Regression guard: a deliberately invalid source produces at least one Violated result.
/// Mutates `constraint tx > 0mm` → `constraint tx > 1000mm` (tx = 100mm, so 100 > 1000 is false).
/// Asserts: (1) the substitution changed the source (guards against target drift),
///          (2) at least one constraint_result is Satisfaction::Violated,
///          (3) total count is still >= 15 (checker did not short-circuit).
/// Mirrors m9_combined.rs::violated_constraint_detected.
#[test]
fn violated_constraint_detected() {
    // tx defaults to 100mm; raise the bound to 1000mm so 100mm > 1000mm is false (VIOLATED).
    let violating = source().replace("constraint tx > 0mm", "constraint tx > 1000mm");

    // Guard: confirm the substitution actually happened.
    // If this assertion fires, the target substring drifted; update the replace call.
    assert_ne!(
        violating,
        source(),
        "replace target drifted — 'constraint tx > 0mm' not found in source; update the test"
    );

    let check_result = check_source_with_stdlib(&violating);

    // Full check must still produce >= 15 results (not short-circuited by a compile error)
    assert!(
        check_result.constraint_results.len() >= 15,
        "expected >= 15 constraint results even for violating source, got {}",
        check_result.constraint_results.len()
    );

    // At least one constraint must be Violated
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();

    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint after raising tx bound above its value (100mm), \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}
