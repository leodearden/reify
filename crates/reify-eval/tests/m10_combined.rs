//! M10 combined integration tests.
//!
//! Exercises all M10 milestone features in a single cohesive example:
//! geometric type params, Point/Vector arithmetic in lets, Frame/Transform
//! in port definitions, connect with connector type and port mapping, purpose
//! checking geometric determinacy, ad-hoc port selector, and where-block
//! reference safety. Uses examples/m10_combined.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{ModulePath, Satisfaction, Value, ValueCellId};

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
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/m10_combined.ri should exist")
    })
    .clone()
}

/// Parse, compile (with stdlib), eval with SimpleConstraintChecker, return EvalResult.
/// Use when asserting on values (geometric, scalars, etc.).
fn eval_source(src: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.eval(&compiled)
}

/// Parse, compile (with stdlib), check with SimpleConstraintChecker, return CheckResult.
/// Use when asserting on constraint satisfaction and counts.
fn check_source(src: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled)
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

// ── Test 8: geom_ready purpose compiled and activatable ──────────────────────

/// Feature 5 (purpose checking geometric determinacy).
/// Asserts the geom_ready purpose is compiled with Structure subject kind,
/// can be activated against Assembly, injects exactly 3 constraints, and
/// is cleanly deactivatable.
#[test]
fn geom_ready_purpose_compiled_and_activatable() {
    let compiled = parse_and_compile_with_stdlib(&source());

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
        geom_ready.params[0].entity_kind,
        "Structure",
        "geom_ready param entity_kind should be 'Structure', got '{}'",
        geom_ready.params[0].entity_kind
    );

    // (c) activate against Assembly and assert is_purpose_active
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
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
/// Asserts that the two nested constraints inside
/// `where determined(origin) { constraint determined(shifted); constraint determined(displacement) }`
/// are present in the check results and both Satisfied.
#[test]
fn where_block_nested_constraints_present_and_satisfied() {
    todo!("step-18 impl: verify where-block constraints appear in check results as Satisfied")
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
    let compiled = parse_and_compile_with_stdlib(&source());
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
    let check_result = check_source(&source());
    let compat_entry = check_result
        .constraint_results
        .iter()
        .find(|e| &e.id == compat_id)
        .unwrap_or_else(|| panic!("compatibility constraint {compat_id} not found in check results"));
    assert_eq!(
        compat_entry.satisfaction,
        Satisfaction::Satisfied,
        "compatibility constraint should be Satisfied"
    );
}

// ── Test 6: frame/transform lets and port frames present ─────────────────────

/// Feature 3 (Frame/Transform in let bindings and port definitions).
/// Asserts rot/base/mount/xform are non-Undef and that supply/demand ports
/// have frame_expr compiled (confirming `frame = base` and `frame = mount`).
///
/// Note: `rot = orient_identity()` triggers a compiler warning
/// ("cannot infer return type of zero-arg function 'orient_identity', defaulting to Real")
/// but the warning is NOT an error, so parse_and_compile_with_stdlib succeeds.
/// `rot` still evaluates to a concrete (non-Undef) value; the test checks presence only.
#[test]
fn frame_transform_lets_and_port_frames_present() {
    // (a) Eval assertions: rot, base, mount, xform are non-Undef
    let result = eval_source(&source());
    let assert_nondetermined = |name: &str| {
        let id = ValueCellId::new("Assembly", name);
        let v = result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("Assembly.{name} not found in eval result"));
        assert!(
            !matches!(v, Value::Undef),
            "Assembly.{name} should not be Undef (expected a concrete Frame/Transform value)"
        );
    };

    assert_nondetermined("rot");
    assert_nondetermined("base");
    assert_nondetermined("mount");
    assert_nondetermined("xform");

    // (b) Compile assertions: supply and demand ports both have a frame_expr
    let compiled = parse_and_compile_with_stdlib(&source());
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

// ── Test 5: geometric let bindings are determined ────────────────────────────

/// Feature 1 (geometric type params) + Feature 2 (Point/Vector arithmetic in lets).
/// Asserts that origin, target, offset, displacement, shifted, and total_offset
/// are present in the eval result as non-Undef values.
#[test]
fn geometric_let_bindings_determined() {
    let result = eval_source(&source());

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

// ── Test 4: total constraint count meets threshold ───────────────────────────

/// Capstone assertion: constraint_results.len() >= 15, all Satisfied.
/// Guards against silent regressions where constraints are silently dropped.
/// The example produces 18 total: 15 always-active + 2 where-block + 1 compat.
#[test]
fn total_constraint_count_meets_threshold() {
    let check_result = check_source(&source());

    let n = check_result.constraint_results.len();
    assert!(
        n >= 15,
        "expected >= 15 total constraint results, got {n}"
    );

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

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Smoke test: file produces constraint results and all are Satisfied.
/// Complements `total_constraint_count_meets_threshold`, which additionally asserts count >= 15.
#[test]
fn all_constraints_satisfied() {
    let check_result = check_source(&source());

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

// ── Test 2: compiles with Assembly template ──────────────────────────────────

/// Compile m10_combined.ri (with stdlib) and verify the compiled module contains
/// an Assembly template (confirming compile-cleanliness and top-level structure name).
#[test]
fn m10_combined_compiles_with_assembly_template() {
    let compiled = parse_and_compile_with_stdlib(&source());
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

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m10_combined.ri and verify it parses without errors.
/// Note: the file may produce compiler warnings (e.g., orient_identity type inference)
/// but no error-severity diagnostics.
#[test]
fn m10_combined_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(&src, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
