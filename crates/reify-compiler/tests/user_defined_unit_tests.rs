//! Tests for user-defined units: cross-module integration (task 209).
//!
//! Validates that `pub unit` declarations from one module are properly seeded
//! into the unit registry of importing modules, and that private units remain
//! invisible across module boundaries.

mod common;

use std::fs;
use std::path::PathBuf;

use reify_compiler::{CompiledModule, compile_with_prelude};
use reify_test_support::{compile_source, compile_source_named, errors_only};
use reify_core::ModulePath;

// ─── helpers ───────────────────────────────────────────────────────────────────

fn compile_with_prelude_helper(source: &str, prelude: &[CompiledModule]) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, prelude)
}

/// Create a unique temp directory for filesystem-based tests.
fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("reify_unit_test_209")
        .join(name)
        .join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write the three-module transitive chain used by one-hop limitation pinning tests:
/// `a.ri` declares `pub unit mil`, `b.ri` imports a (no local units),
/// `c.ri` imports b and tries to use `5mil`.
fn write_transitive_unit_chain(dir: &std::path::Path) {
    fs::write(dir.join("a.ri"), "pub unit mil : Length = 0.0000254").unwrap();
    fs::write(dir.join("b.ri"), "import a").unwrap();
    fs::write(
        dir.join("c.ri"),
        "import b\nstructure S { param w : Length = 5mil }",
    )
    .unwrap();
}

/// Assert that `errors` contains at least one diagnostic whose lowercased message
/// contains both "unknown unit" and "mil". Used to pin the one-hop limitation.
fn assert_unknown_unit_mil(errors: &[&reify_core::Diagnostic]) {
    assert!(
        !errors.is_empty(),
        "expected 'unknown unit' error: `mil` should not be visible two hops away (one-hop limitation)"
    );
    assert!(
        errors.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("unknown unit") && msg.contains("mil")
        }),
        "error should mention 'unknown unit' and 'mil'; got: {:?}",
        errors
    );
}

// ─── step-1: user-declared unit works in a let binding expression ─────────────

#[test]
fn user_unit_in_let_binding() {
    // Declare `thou`, use it in a param default and a let binding.
    // Verifies that QuantityLiteral resolution in a let-binding BinOp context
    // resolves to the correct user-defined SI value (5 * 0.0000254 = 0.000127),
    // not merely that an expression exists. A hardcoded-fallback regression
    // would yield a different si_value (e.g. ≈0.005 for mm) rather than 0.000127.
    let module = compile_source(
        "unit thou : Length = 0.0000254\n\
         structure S {\n\
             param w : Length = 10thou\n\
             let w_thou = w + 5thou\n\
         }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    // Let binding should have produced a value cell
    let w_thou = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "w_thou")
        .expect("w_thou value cell not found");
    // Walk default_expr → BinOp → right operand → Scalar si_value + dimension
    let expr = w_thou
        .default_expr
        .as_ref()
        .expect("w_thou has no default_expr");
    let (op, _left, right) = common::expect_binop(expr);
    assert!(
        matches!(op, reify_ir::BinOp::Add),
        "expected Add op for w + 5thou, got {:?}",
        op
    );
    let expected = 5.0 * 0.0000254;
    let (si_value, dimension) = common::expect_scalar(right);
    assert!(
        (si_value - expected).abs() < common::UNIT_EPSILON,
        "expected si_value≈{} (5 * 0.0000254 = 0.000127), got {} \
         (a hardcoded-mm fallback regression would yield ≈0.005)",
        expected,
        si_value
    );
    assert_eq!(
        dimension,
        reify_core::DimensionVector::LENGTH,
        "expected Length dimension for 5thou, got {:?}",
        dimension
    );
}

// ─── step-3: user-defined unit overrides hardcoded fallback ──────────────────

#[test]
fn user_unit_overrides_hardcoded_fallback() {
    // Redeclare `mm` with factor 0.005 (intentionally different from the
    // hardcoded 0.001). The registry-first lookup in expr.rs should pick up
    // the user's value, NOT the hardcoded fallback.
    let module = compile_source(
        "unit mm : Length = 0.005\n\
         structure S { param w : Length = 10mm }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let w_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "w")
        .expect("w not found");
    let expr = w_cell.default_expr.as_ref().expect("w has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(expr);
    // registry value: 10 * 0.005 = 0.05, NOT hardcoded 10 * 0.001 = 0.01
    assert!(
        (si_value - 0.05).abs() < common::UNIT_EPSILON,
        "expected registry value 0.05 (10 * 0.005), got {} (hardcoded would be 0.01)",
        si_value
    );
}

// ─── step-5: cross-module pub unit visible via compile_with_prelude ───────────

#[test]
fn cross_module_pub_unit_visible_via_compile_with_prelude() {
    // Compile a "library" module that exports `pub unit mil`.
    let prelude_module = compile_source("pub unit mil : Length = 0.0000254");
    assert!(
        errors_only(&prelude_module).is_empty(),
        "prelude errors: {:?}",
        errors_only(&prelude_module)
    );

    // User module references `mil` — should resolve from the seeded prelude.
    let module =
        compile_with_prelude_helper("structure S { param w : Length = 5mil }", &[prelude_module]);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let w_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "w")
        .expect("w not found");
    let expr = w_cell.default_expr.as_ref().expect("w has no default_expr");
    let expected = 5.0 * 0.0000254;
    let (si_value, _dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - expected).abs() < common::UNIT_EPSILON,
        "expected si_value≈{} (5 * 0.0000254), got {}",
        expected,
        si_value
    );
}

// ─── step-7: cross-module private unit NOT visible via compile_with_prelude ───

#[test]
fn cross_module_private_unit_not_visible_via_compile_with_prelude() {
    // Compile a module with a PRIVATE unit (no `pub`).
    let prelude_module = compile_source("unit privmil : Length = 0.0000254");
    assert!(
        errors_only(&prelude_module).is_empty(),
        "prelude errors: {:?}",
        errors_only(&prelude_module)
    );

    // User source tries to reference `privmil` — should fail with unknown unit.
    let module = compile_with_prelude_helper(
        "structure S { param w : Length = 5privmil }",
        &[prelude_module],
    );
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error for private unit 'privmil' used across module boundary"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.to_lowercase().contains("unknown unit")
                && d.message.contains("privmil")),
        "error should mention 'unknown unit' and 'privmil'; got: {:?}",
        errors
    );
}

// ─── step-9: cross-module pub unit visible via ModuleDag ──────────────────────

#[test]
fn cross_module_pub_unit_visible_via_module_dag() {
    let dir = test_dir("cross_module_pub_unit");

    fs::write(
        dir.join("units_lib.ri"),
        "pub unit mil : Length = 0.0000254",
    )
    .unwrap();
    fs::write(
        dir.join("user.ri"),
        "import units_lib\nstructure S { param w : Length = 5mil }",
    )
    .unwrap();

    let resolver = reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = reify_compiler::module_dag::ModuleDag::new();
    let result = dag.compile_module("user", &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let user_module = dag.modules.get("user").expect("user module not in dag");
    let errors = errors_only(user_module);
    assert!(
        errors.is_empty(),
        "expected no errors in user module, got: {:?}",
        errors
    );

    let _ = fs::remove_dir_all(&dir);
}

// ─── step-11: cross-module private unit NOT visible via ModuleDag ─────────────

#[test]
fn cross_module_private_unit_not_visible_via_module_dag() {
    let dir = test_dir("cross_module_private_unit");

    // Private unit (no `pub`)
    fs::write(
        dir.join("units_lib.ri"),
        "unit privmil : Length = 0.0000254",
    )
    .unwrap();
    fs::write(
        dir.join("user.ri"),
        "import units_lib\nstructure S { param w : Length = 5privmil }",
    )
    .unwrap();

    let resolver = reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = reify_compiler::module_dag::ModuleDag::new();
    // compile_module succeeds (parse is fine), but user module has semantic errors
    let result = dag.compile_module("user", &resolver);
    assert!(
        result.is_ok(),
        "compile_module should succeed (no parse errors): {:?}",
        result
    );

    let user_module = dag.modules.get("user").expect("user module not in dag");
    let errors = errors_only(user_module);
    assert!(
        !errors.is_empty(),
        "expected error for private unit 'privmil' used across module boundary"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ─── step-13: compile_project entry module resolves imported pub unit ─────────

#[test]
fn compile_project_entry_sees_imported_pub_unit() {
    let dir = test_dir("compile_project_pub_unit");

    fs::write(
        dir.join("units_lib.ri"),
        "pub unit mil : Length = 0.0000254",
    )
    .unwrap();
    fs::write(
        dir.join("entry.ri"),
        "import units_lib\nstructure S { param w : Length = 5mil }",
    )
    .unwrap();

    let resolver = reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(&dir.join("entry.ri"), &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    let entry_module = modules.last().expect("no modules returned");
    let errors = errors_only(entry_module);
    assert!(
        errors.is_empty(),
        "entry module should see imported pub unit 'mil', got errors: {:?}",
        errors
    );

    let _ = fs::remove_dir_all(&dir);
}

// ─── step-14: compile_project entry does NOT see imported private unit ─────────

#[test]
fn compile_project_entry_does_not_see_imported_private_unit() {
    let dir = test_dir("compile_project_private_unit");

    // Private unit (no `pub`) — should NOT be seeded into the entry module's prelude.
    fs::write(
        dir.join("units_lib.ri"),
        "unit privmil : Length = 0.0000254",
    )
    .unwrap();
    fs::write(
        dir.join("entry.ri"),
        "import units_lib\nstructure S { param w : Length = 5privmil }",
    )
    .unwrap();

    let resolver = reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(&dir.join("entry.ri"), &resolver);
    // Parse succeeds; semantic errors are attached to the module, not returned as Err.
    assert!(
        result.is_ok(),
        "compile_project should succeed (no parse errors): {:?}",
        result
    );

    let modules = result.unwrap();
    let entry_module = modules.last().expect("no modules returned");
    let errors = errors_only(entry_module);
    assert!(
        !errors.is_empty(),
        "expected error for private unit 'privmil' used across module boundary via compile_project"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("unknown") || d.message.contains("privmil")),
        "error should mention unknown unit or 'privmil'; got: {:?}",
        errors
    );

    let _ = fs::remove_dir_all(&dir);
}

// ─── step-15: local unit duplicating an imported pub unit produces error ──────

#[test]
fn local_unit_duplicating_imported_pub_unit_produces_error() {
    // Prelude module exports pub unit `mil`.
    // Use a distinctive module name so we can assert the diagnostic references it.
    let prelude_module = compile_source_named("pub unit mil : Length = 0.0000254", "imported_mod");
    assert!(
        errors_only(&prelude_module).is_empty(),
        "prelude errors: {:?}",
        errors_only(&prelude_module)
    );

    // User module tries to re-declare `mil` — the prelude-seeded entry
    // occupies the registry, so register() returns Err (duplicate).
    let module = compile_with_prelude_helper("unit mil : Length = 0.001", &[prelude_module]);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected duplicate unit error when redeclaring an imported pub unit"
    );

    // (1) Bind the specific duplicate diagnostic
    let dup_diag = errors
        .iter()
        .find(|d| d.message.contains("duplicate") && d.message.contains("mil"));
    let dup_diag = dup_diag.unwrap_or_else(|| {
        panic!(
            "error should mention 'duplicate' and 'mil'; got: {:?}",
            errors
        )
    });

    // (a) The message must NOT say 'stdlib' — the source module is 'imported_mod', not 'std/*'
    assert!(
        !dup_diag.message.contains("stdlib"),
        "diagnostic should NOT mention 'stdlib' for user-module collision, got: {:?}",
        dup_diag.message
    );

    // (b) The message must name the source module 'imported_mod' (where `mil` was declared)
    assert!(
        dup_diag.message.contains("imported_mod"),
        "diagnostic should mention 'imported_mod' as the source module, got: {:?}",
        dup_diag.message
    );

    // (c) Two labels: labels[0] is the user's in-file dup decl span;
    // labels[1] is the prelude sentinel with provenance in its message.
    common::assert_prelude_collision_labels(dup_diag);
}

// ─── step-17: transitive pub unit NOT visible two hops via compile_with_prelude ─

#[test]
fn transitive_pub_unit_not_visible_via_compile_with_prelude() {
    // One-hop limitation pinning test.
    // A declares `pub unit mil`, B is compiled with prelude=[A] (B has no local units),
    // C is compiled with prelude=[B] and tries to use `5mil`.
    // Since B.units is empty (no locally-declared units), the prelude-seeding loop
    // for C finds nothing in B.units, so `mil` is invisible to C.
    let module_a = compile_source("pub unit mil : Length = 0.0000254");
    assert!(
        errors_only(&module_a).is_empty(),
        "module_a errors: {:?}",
        errors_only(&module_a)
    );

    // Module B: compiled with A as prelude, but B declares no units of its own.
    // B can use `mil` (it's in B's registry via prelude seeding), but
    // B's CompiledModule.units remains empty (only holds locally-declared units).
    // Positive one-hop assertion: B's source actively resolves `mil` from its
    // prelude, confirming the one-hop seeding works end-to-end.
    let module_b =
        compile_with_prelude_helper("structure T { param x : Length = 1mil }", &[module_a]);
    assert!(
        errors_only(&module_b).is_empty(),
        "module_b errors: {:?}",
        errors_only(&module_b)
    );
    assert!(
        module_b.units.is_empty(),
        "module_b should have no locally-declared units"
    );

    // Module C: compiled with B as prelude, tries to use `mil`.
    // The prelude-seeding loop iterates B.units (empty), so `mil` is not seeded into C.
    let module_c =
        compile_with_prelude_helper("structure S { param w : Length = 5mil }", &[module_b]);
    let errors = errors_only(&module_c);
    assert_unknown_unit_mil(&errors);
}

// ─── step-19: transitive pub unit NOT visible two hops via ModuleDag ──────────

#[test]
fn transitive_pub_unit_not_visible_via_module_dag() {
    // One-hop limitation pinning test via filesystem ModuleDag path.
    // a.ri declares `pub unit mil`, b.ri has `import a` (no local units),
    // c.ri has `import b` and uses `5mil`.
    // The DAG seeds A's `mil` into B's registry, but B's CompiledModule.units
    // is empty (B has no locally-declared units). When C is compiled, prelude-seeding
    // from B.units yields nothing, so `mil` is unknown in C.
    let dir = test_dir("transitive_pub_unit_module_dag");
    write_transitive_unit_chain(&dir);

    let resolver = reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = reify_compiler::module_dag::ModuleDag::new();
    // compile_module succeeds (no parse errors); semantic errors appear in the module.
    let result = dag.compile_module("c", &resolver);
    assert!(
        result.is_ok(),
        "compile_module should succeed (no parse errors): {:?}",
        result
    );

    let module_c = dag.modules.get("c").expect("module c not in dag");
    let errors = errors_only(module_c);
    assert_unknown_unit_mil(&errors);

    let _ = fs::remove_dir_all(&dir);
}

// ─── step-21: transitive pub unit NOT visible two hops via compile_project ────

#[test]
fn transitive_pub_unit_not_visible_via_compile_project() {
    // One-hop limitation pinning test via compile_project entry path.
    // Same three-module chain: a.ri → b.ri → c.ri (c.ri is the entry file).
    // C's entry-point compilation cannot see `mil` from A because B's
    // CompiledModule.units is empty (B has no locally-declared units).
    let dir = test_dir("transitive_pub_unit_compile_project");
    write_transitive_unit_chain(&dir);

    let resolver = reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(&dir.join("c.ri"), &resolver);
    // Parse succeeds; semantic errors are attached to the module, not returned as Err.
    assert!(
        result.is_ok(),
        "compile_project should succeed (no parse errors): {:?}",
        result
    );

    let modules = result.unwrap();
    let entry_module = modules
        .iter()
        .find(|m| m.path.to_string() == "c")
        .expect("module 'c' not found in compile_project output");
    let errors = errors_only(entry_module);
    assert_unknown_unit_mil(&errors);

    let _ = fs::remove_dir_all(&dir);
}
