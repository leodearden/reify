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
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("vis_test"));
    let compiled = reify_compiler::compile(&parsed);

    let template = &compiled.templates[0];
    let w_cell = template.value_cells.iter().find(|vc| vc.id.member == "w").unwrap();
    assert_eq!(w_cell.visibility, reify_compiler::Visibility::Public);
}

#[test]
fn compile_let_visibility_private_by_default() {
    let source = r#"structure S {
    param a: Scalar = 1mm
    let vol = a * 2
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("vis_test"));
    let compiled = reify_compiler::compile(&parsed);

    let template = &compiled.templates[0];
    let vol_cell = template.value_cells.iter().find(|vc| vc.id.member == "vol").unwrap();
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
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("vis_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);

    let template = &compiled.templates[0];
    let vol_cell = template.value_cells.iter().find(|vc| vc.id.member == "volume").unwrap();
    assert_eq!(vol_cell.visibility, reify_compiler::Visibility::Public);
}
