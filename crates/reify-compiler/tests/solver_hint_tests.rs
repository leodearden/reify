//! Solver hint compilation tests.
//!
//! Tests for `@solver_hint` annotations on param/let members compiling into
//! `SolverHint` entries on `ValueCellDecl`.

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("solver_hint_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics (ignoring warnings).
fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect()
}

/// Helper: return only warning-severity diagnostics.
fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Warning)
        .collect()
}

// ── Step 7: @solver_hint("discrete_set", ...) on param compiles ─────────────

#[test]
fn solver_hint_discrete_set_compiles() {
    let source = r#"structure S { @solver_hint("discrete_set", bolt_lengths) param length : Length = auto }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    assert!(
        !template.value_cells.is_empty(),
        "expected at least one value cell"
    );

    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        1,
        "expected 1 solver hint, got {:?}",
        cell.solver_hints
    );
    assert_eq!(
        cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::DiscreteSet
    );
    assert_eq!(cell.solver_hints[0].collection, "bolt_lengths");
}

// ── Step 9: @solver_hint("prefer_stock", ...) on param compiles ─────────────

#[test]
fn solver_hint_prefer_stock_compiles() {
    let source = r#"structure S { @solver_hint("prefer_stock", sheet_thicknesses) param width : Length = auto }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        1,
        "expected 1 solver hint, got {:?}",
        cell.solver_hints
    );
    assert_eq!(
        cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::PreferStock
    );
    assert_eq!(cell.solver_hints[0].collection, "sheet_thicknesses");
}

// ── Step 11: @solver_hint on let member compiles ────────────────────────────

#[test]
fn solver_hint_on_let_compiles() {
    let source = r#"structure S { @solver_hint("discrete_set", gauges) let t : Length = 5mm }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

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
    assert_eq!(
        let_cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::DiscreteSet
    );
    assert_eq!(let_cell.solver_hints[0].collection, "gauges");
}

// ── Step 13: invalid solver hint kind emits warning ────────────────────────

#[test]
fn solver_hint_invalid_kind_warns() {
    let source =
        r#"structure S { @solver_hint("invalid_kind", collection) param length : Length = auto }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let cell = &template.value_cells[0];
    assert!(
        cell.solver_hints.is_empty(),
        "expected no solver hints for invalid kind, got {:?}",
        cell.solver_hints
    );

    let warns = warnings_only(&module);
    assert!(
        warns
            .iter()
            .any(|d| d.message.contains("unknown solver hint kind")),
        "expected warning about unknown solver hint kind, got: {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Step 15: missing collection reference emits warning ────────────────────

#[test]
fn solver_hint_missing_collection_warns() {
    let source = r#"structure S { @solver_hint("discrete_set") param length : Length = auto }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let cell = &template.value_cells[0];
    assert!(
        cell.solver_hints.is_empty(),
        "expected no solver hints when collection is missing, got {:?}",
        cell.solver_hints
    );

    let warns = warnings_only(&module);
    assert!(
        warns
            .iter()
            .any(|d| d.message.contains("collection reference")),
        "expected warning about missing collection, got: {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Step 16: @solver_hint with zero arguments emits warning ───────────────

#[test]
fn solver_hint_zero_args_warns() {
    let source = r#"structure S { @solver_hint param x : Real = auto }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let cell = &template.value_cells[0];
    assert!(
        cell.solver_hints.is_empty(),
        "expected no solver hints for zero-arg @solver_hint, got {:?}",
        cell.solver_hints
    );

    let warns = warnings_only(&module);
    assert!(
        warns.iter().any(|d| d
            .message
            .contains("requires a string literal kind as first argument")),
        "expected warning about missing kind, got: {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Step 17: @solver_hint in guarded block compiles ────────────────────────

#[test]
fn solver_hint_in_guarded_block_compiles() {
    let source = r#"structure S {
        param x : Real = 1
        where x > 0 {
            @solver_hint("discrete_set", sizes)
            param width : Length = auto
        }
    }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    // Guarded members live in guarded_groups, not value_cells
    let width_cell = template
        .guarded_groups
        .iter()
        .flat_map(|g| g.members.iter())
        .find(|c| c.id.member == "width")
        .expect("expected a 'width' value cell in guarded groups");

    assert_eq!(
        width_cell.solver_hints.len(),
        1,
        "expected 1 solver hint on guarded param, got {:?}",
        width_cell.solver_hints
    );
    assert_eq!(
        width_cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::DiscreteSet
    );
    assert_eq!(width_cell.solver_hints[0].collection, "sizes");
}

// ── Step 19: multiple @solver_hint on same param ───────────────────────────

#[test]
fn solver_hint_multiple_on_same_param() {
    let source = r#"structure S {
        @solver_hint("discrete_set", a)
        @solver_hint("prefer_stock", b)
        param length : Length = auto
    }"#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        2,
        "expected 2 solver hints, got {:?}",
        cell.solver_hints
    );
    assert_eq!(
        cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::DiscreteSet
    );
    assert_eq!(cell.solver_hints[0].collection, "a");
    assert_eq!(
        cell.solver_hints[1].kind,
        reify_compiler::SolverHintKind::PreferStock
    );
    assert_eq!(cell.solver_hints[1].collection, "b");
}

// ── Step 21: builder creates param with solver hints ───────────────────────

#[test]
fn builder_param_with_solver_hints() {
    use reify_test_support::builders::TopologyTemplateBuilder;

    let hints = vec![reify_compiler::SolverHint {
        kind: reify_compiler::SolverHintKind::DiscreteSet,
        collection: "bolt_lengths".to_string(),
        span: reify_types::SourceSpan::new(0, 0),
    }];

    let template = TopologyTemplateBuilder::new("S")
        .param_with_solver_hints("S", "length", reify_types::Type::length(), None, hints)
        .build();

    assert_eq!(template.value_cells.len(), 1);
    let cell = &template.value_cells[0];
    assert_eq!(cell.solver_hints.len(), 1);
    assert_eq!(
        cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::DiscreteSet
    );
    assert_eq!(cell.solver_hints[0].collection, "bolt_lengths");
}
