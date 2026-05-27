//! Visibility compilation tests.
//!
//! Tests that visibility metadata is correctly set on compiled types
//! based on declaration kind and `pub` keyword presence.

// ── Step 7: param and let default visibility ─────────────────────

#[test]
fn compile_param_visibility_public() {
    let source = r#"structure S {
    param w: Scalar = 80mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
    let compiled = reify_compiler::compile(&parsed);

    let template = &compiled.templates[0];
    let w_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "w")
        .unwrap();
    assert_eq!(w_cell.visibility, reify_compiler::Visibility::Public);
}

#[test]
fn compile_let_visibility_private_by_default() {
    let source = r#"structure S {
    param a: Scalar = 1mm
    let vol = a * 2
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
    let compiled = reify_compiler::compile(&parsed);

    let template = &compiled.templates[0];
    let vol_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "vol")
        .unwrap();
    assert_eq!(vol_cell.visibility, reify_compiler::Visibility::Private);
}

// ── Step 9: pub let → Public visibility ─────────────────────────

#[test]
fn compile_pub_let_visibility_public() {
    let source = r#"structure S {
    param w: Scalar = 80mm
    param h: Scalar = 100mm
    pub let volume = w * h
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);

    let template = &compiled.templates[0];
    let vol_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "volume")
        .unwrap();
    assert_eq!(vol_cell.visibility, reify_compiler::Visibility::Public);
}

// ── Step 11: template visibility ─────────────────────────────────

#[test]
fn compile_template_visibility() {
    let source = r#"pub structure Bracket {
    param w: Scalar = 80mm
}
structure Internal {
    param x: Scalar = 1mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);

    assert_eq!(compiled.templates.len(), 2);

    let bracket = &compiled.templates[0];
    assert_eq!(bracket.name, "Bracket");
    assert_eq!(bracket.visibility, reify_compiler::Visibility::Public);

    let internal = &compiled.templates[1];
    assert_eq!(internal.name, "Internal");
    assert_eq!(internal.visibility, reify_compiler::Visibility::Private);
}

// ── Step 13: sub component visibility ────────────────────────────

#[test]
fn compile_sub_visibility_public() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param w: Scalar = 80mm
    sub rib = Child(h: w)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);

    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .unwrap();
    assert_eq!(parent.sub_components.len(), 1);
    let rib = &parent.sub_components[0];
    assert_eq!(rib.name, "rib");
    assert_eq!(rib.visibility, reify_compiler::Visibility::Public);
}

// ── Step 15: backward compatibility ──────────────────────────────

#[test]
fn backward_compat_bracket_compiles_cleanly() {
    // The canonical bracket source has no pub keywords — verify it compiles
    // with zero error diagnostics and correct visibility defaults.
    let source = reify_test_support::bracket_source();
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    assert_eq!(template.name, "Bracket");

    // Template should be private (no pub keyword)
    assert_eq!(template.visibility, reify_compiler::Visibility::Private);

    // 5 params + 1 let (volume) = 6 value cells (body is geometry, skipped)
    assert_eq!(template.value_cells.len(), 6);
    assert_eq!(template.constraints.len(), 3);

    // All params should be Public
    for vc in &template.value_cells {
        if vc.kind == reify_compiler::ValueCellKind::Param {
            assert_eq!(
                vc.visibility,
                reify_compiler::Visibility::Public,
                "param {} should be Public",
                vc.id.member
            );
        }
    }

    // The volume let should be Private
    let volume = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "volume")
        .expect("should have 'volume' value cell");
    assert_eq!(volume.kind, reify_compiler::ValueCellKind::Let);
    assert_eq!(volume.visibility, reify_compiler::Visibility::Private);
}
