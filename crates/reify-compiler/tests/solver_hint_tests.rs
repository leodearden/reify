//! Solver hint compilation tests.
//!
//! Tests for `@solver_hint` annotations on param/let members compiling into
//! `SolverHint` entries on `ValueCellDecl`.

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("solver_hint_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics (ignoring warnings).
fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module.diagnostics.iter().filter(|d| d.severity == reify_types::Severity::Error).collect()
}

/// Helper: return only warning-severity diagnostics.
fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module.diagnostics.iter().filter(|d| d.severity == reify_types::Severity::Warning).collect()
}

// ── Step 7: @solver_hint("discrete_set", ...) on param compiles ─────────────

#[test]
fn solver_hint_discrete_set_compiles() {
    let source = r#"structure S { @solver_hint("discrete_set", bolt_lengths) param length : Length = auto }"#;
    let module = compile_module(source);
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));

    let template = &module.templates[0];
    assert!(!template.value_cells.is_empty(), "expected at least one value cell");

    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        1,
        "expected 1 solver hint, got {:?}",
        cell.solver_hints
    );
    assert_eq!(cell.solver_hints[0].kind, reify_compiler::SolverHintKind::DiscreteSet);
    assert_eq!(cell.solver_hints[0].collection, "bolt_lengths");
}

// ── Step 9: @solver_hint("prefer_stock", ...) on param compiles ─────────────

#[test]
fn solver_hint_prefer_stock_compiles() {
    let source = r#"structure S { @solver_hint("prefer_stock", sheet_thicknesses) param width : Length = auto }"#;
    let module = compile_module(source);
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));

    let template = &module.templates[0];
    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        1,
        "expected 1 solver hint, got {:?}",
        cell.solver_hints
    );
    assert_eq!(cell.solver_hints[0].kind, reify_compiler::SolverHintKind::PreferStock);
    assert_eq!(cell.solver_hints[0].collection, "sheet_thicknesses");
}

// ── Step 11: @solver_hint on let member compiles ────────────────────────────

#[test]
fn solver_hint_on_let_compiles() {
    let source = r#"structure S { @solver_hint("discrete_set", gauges) let t : Length = 5mm }"#;
    let module = compile_module(source);
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));

    let template = &module.templates[0];
    // Find the let value cell (kind == Let)
    let let_cell = template
        .value_cells
        .iter()
        .find(|c| c.kind == reify_compiler::ValueCellKind::Let)
        .expect("expected a Let value cell");

    assert_eq!(
        let_cell.solver_hints.len(),
        1,
        "expected 1 solver hint on let, got {:?}",
        let_cell.solver_hints
    );
    assert_eq!(let_cell.solver_hints[0].kind, reify_compiler::SolverHintKind::DiscreteSet);
    assert_eq!(let_cell.solver_hints[0].collection, "gauges");
}
