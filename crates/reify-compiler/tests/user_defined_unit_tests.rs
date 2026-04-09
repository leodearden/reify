//! Tests for user-defined units: cross-module integration (task 209).
//!
//! Validates that `pub unit` declarations from one module are properly seeded
//! into the unit registry of importing modules, and that private units remain
//! invisible across module boundaries.

use std::fs;
use std::path::PathBuf;

use reify_compiler::{CompiledModule, compile, compile_with_prelude};
use reify_types::{ModulePath, Severity};

// ─── helpers ───────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile(&parsed)
}

fn compile_with_prelude_helper(source: &str, prelude: &[CompiledModule]) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, prelude)
}

fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
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

// ─── step-1: user-declared unit works in a let binding expression ─────────────

#[test]
fn user_unit_in_let_binding() {
    // Declare `thou`, use it in a param default and a let binding.
    // Verifies that QuantityLiteral resolution works in all expression contexts,
    // not only param defaults.
    let module = parse_and_compile(
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
    assert!(
        w_thou.default_expr.is_some(),
        "w_thou should have a computed expression"
    );
}

// ─── step-3: user-defined unit overrides hardcoded fallback ──────────────────

#[test]
fn user_unit_overrides_hardcoded_fallback() {
    // Redeclare `mm` with factor 0.005 (intentionally different from the
    // hardcoded 0.001). The registry-first lookup in expr.rs should pick up
    // the user's value, NOT the hardcoded fallback.
    let module = parse_and_compile(
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
    if let Some(expr) = &w_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            // registry value: 10 * 0.005 = 0.05, NOT hardcoded 10 * 0.001 = 0.01
            assert!(
                (si_value - 0.05).abs() < 1e-9,
                "expected registry value 0.05 (10 * 0.005), got {} (hardcoded would be 0.01)",
                si_value
            );
        } else {
            panic!("expected scalar literal, got {:?}", expr.kind);
        }
    } else {
        panic!("w has no default_expr");
    }
}

// ─── step-5: cross-module pub unit visible via compile_with_prelude ───────────

#[test]
fn cross_module_pub_unit_visible_via_compile_with_prelude() {
    // Compile a "library" module that exports `pub unit mil`.
    let prelude_module = parse_and_compile("pub unit mil : Length = 0.0000254");
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
    if let Some(expr) = &w_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            let expected = 5.0 * 0.0000254;
            assert!(
                (si_value - expected).abs() < 1e-10,
                "expected si_value≈{} (5 * 0.0000254), got {}",
                expected,
                si_value
            );
        } else {
            panic!("expected scalar literal, got {:?}", expr.kind);
        }
    } else {
        panic!("w has no default_expr");
    }
}

// ─── step-7: cross-module private unit NOT visible via compile_with_prelude ───

#[test]
fn cross_module_private_unit_not_visible_via_compile_with_prelude() {
    // Compile a module with a PRIVATE unit (no `pub`).
    let prelude_module = parse_and_compile("unit privmil : Length = 0.0000254");
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
            .any(|d| d.message.contains("unknown") || d.message.contains("privmil")),
        "error should mention unknown unit; got: {:?}",
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

    let resolver =
        reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
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

    let resolver =
        reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
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

    let resolver =
        reify_compiler::module_dag::ModuleResolver::new(&dir, dir.join("stdlib"));
    let result =
        reify_compiler::module_dag::compile_project(&dir.join("entry.ri"), &resolver);
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
