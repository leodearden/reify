//! Tests for PreludeContext — pre-built prelude cache to avoid re-flattening enums.
//!
//! TDD structure:
//!   step-1: PreludeContext::new invariants (empty + two-module enum ordering)
//!   step-3: PreludeContext::from_slice ergonomics (borrow stability + parity)
//!   step-5: compile_with_prelude_context parity with compile_with_prelude
//!   step-7: load_stdlib_context caching (pointer stability + enum parity)

use reify_compiler::{CompiledModule, PreludeContext, compile_with_prelude, compile_with_prelude_context};
use reify_test_support::CompiledModuleBuilder;
use reify_types::{EnumDef, ModulePath};

// ─── step-3: PreludeContext::from_slice ergonomics ─────────────────────────

/// PreludeContext::from_slice borrows the same CompiledModule addresses as the
/// input slice AND produces the same resolution_enums() as the equivalent
/// PreludeContext::new(&borrowed_refs).
#[test]
fn from_slice_borrows_same_addresses_and_matches_new() {
    let enum_x = EnumDef {
        name: "EnumX".to_string(),
        variants: vec!["X1".to_string()],
    };
    let enum_y = EnumDef {
        name: "EnumY".to_string(),
        variants: vec!["Y1".to_string(), "Y2".to_string()],
    };

    let m1 = CompiledModuleBuilder::new(ModulePath::single("from_slice_m1"))
        .enum_def(enum_x.clone())
        .build();
    let m2 = CompiledModuleBuilder::new(ModulePath::single("from_slice_m2"))
        .enum_def(enum_y.clone())
        .build();

    let prelude: &[CompiledModule] = &[m1, m2];
    let ctx_from_slice = PreludeContext::from_slice(prelude);

    // modules() must expose references to the same allocations as the input slice.
    assert_eq!(
        ctx_from_slice.modules().len(),
        prelude.len(),
        "from_slice should borrow the same number of modules"
    );
    for (i, (ctx_ref, input_ref)) in ctx_from_slice
        .modules()
        .iter()
        .zip(prelude.iter())
        .enumerate()
    {
        assert!(
            std::ptr::eq(*ctx_ref, input_ref),
            "modules()[{i}] should point to the same allocation as prelude[{i}]"
        );
    }

    // resolution_enums() from from_slice must match new(&refs).
    let refs: Vec<&_> = prelude.iter().collect();
    let ctx_from_new = PreludeContext::new(&refs);
    assert_eq!(
        ctx_from_slice.resolution_enums(),
        ctx_from_new.resolution_enums(),
        "from_slice resolution_enums must equal new(&refs) resolution_enums"
    );
}

// ─── step-1: PreludeContext::new invariants ────────────────────────────────

/// PreludeContext::new(&[]) returns a context whose modules() is empty and
/// resolution_enums() is empty.
#[test]
fn new_empty_prelude_produces_empty_context() {
    let ctx: PreludeContext = PreludeContext::new(&[]);
    assert!(
        ctx.modules().is_empty(),
        "empty prelude should yield empty modules(), got len={}",
        ctx.modules().len()
    );
    assert!(
        ctx.resolution_enums().is_empty(),
        "empty prelude should yield empty resolution_enums(), got: {:?}",
        ctx.resolution_enums()
    );
}

/// PreludeContext::new with two synthetic modules preserves source enum order:
/// enums from m1 come first (in m1.enum_defs order), then enums from m2.
#[test]
fn new_two_module_prelude_preserves_enum_order() {
    let enum_a = EnumDef {
        name: "EnumA".to_string(),
        variants: vec!["A1".to_string(), "A2".to_string()],
    };
    let enum_b = EnumDef {
        name: "EnumB".to_string(),
        variants: vec!["B1".to_string()],
    };
    let enum_c = EnumDef {
        name: "EnumC".to_string(),
        variants: vec!["C1".to_string(), "C2".to_string(), "C3".to_string()],
    };

    let m1 = CompiledModuleBuilder::new(ModulePath::single("prelude_m1"))
        .enum_def(enum_a.clone())
        .enum_def(enum_b.clone())
        .build();
    let m2 = CompiledModuleBuilder::new(ModulePath::single("prelude_m2"))
        .enum_def(enum_c.clone())
        .build();

    let ctx = PreludeContext::new(&[&m1, &m2]);

    // modules() must expose both modules in the original order.
    assert_eq!(
        ctx.modules().len(),
        2,
        "expected 2 modules in context, got {}",
        ctx.modules().len()
    );

    // resolution_enums() must be [EnumA, EnumB, EnumC] in source order.
    let expected = vec![enum_a, enum_b, enum_c];
    assert_eq!(
        ctx.resolution_enums(),
        expected.as_slice(),
        "resolution_enums must be [EnumA, EnumB, EnumC] preserving source order"
    );
}

// ─── step-5: compile_with_prelude_context parity ───────────────────────────

/// Asserts that two `CompiledModule` values are observationally identical for
/// parity testing purposes: same content_hash, same error count, same template
/// names, same enum_def names, same trait_def names, same function names.
///
/// `content_hash` alone captures full content; the structural checks below
/// guard against subtle mismatches that might not surface in the hash
/// (e.g. wrong number of outputs even with matching hash).
fn assert_compiled_module_parity(actual: &reify_compiler::CompiledModule, expected: &reify_compiler::CompiledModule, label: &str) {
    assert_eq!(
        actual.content_hash, expected.content_hash,
        "{label}: content_hash must match"
    );
    assert_eq!(
        actual.diagnostics.len(), expected.diagnostics.len(),
        "{label}: diagnostics count must match"
    );
    let actual_error_count = actual.diagnostics.iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .count();
    let expected_error_count = expected.diagnostics.iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .count();
    assert_eq!(
        actual_error_count, expected_error_count,
        "{label}: error diagnostics count must match"
    );
    assert_eq!(
        actual.enum_defs, expected.enum_defs,
        "{label}: enum_defs must match"
    );
    let actual_template_names: Vec<&str> = actual.templates.iter().map(|t| t.name.as_str()).collect();
    let expected_template_names: Vec<&str> = expected.templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        actual_template_names, expected_template_names,
        "{label}: template names must match"
    );
    let actual_trait_names: Vec<&str> = actual.trait_defs.iter().map(|t| t.name.as_str()).collect();
    let expected_trait_names: Vec<&str> = expected.trait_defs.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        actual_trait_names, expected_trait_names,
        "{label}: trait_def names must match"
    );
    let actual_fn_names: Vec<&str> = actual.functions.iter().map(|f| f.name.as_str()).collect();
    let expected_fn_names: Vec<&str> = expected.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        actual_fn_names, expected_fn_names,
        "{label}: function names must match"
    );
}

/// Case (a): empty prelude — compile_with_prelude_context(&parsed, &ctx)
/// must produce a CompiledModule identical to compile_with_prelude(&parsed, &[]).
#[test]
fn compile_with_prelude_context_parity_empty_prelude() {
    let source = r#"
structure def S {
    param x : Scalar = 42
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("parity_empty"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let expected = compile_with_prelude(&parsed, &[]);
    let ctx = PreludeContext::from_slice(&[]);
    let actual = compile_with_prelude_context(&parsed, &ctx);

    assert_compiled_module_parity(&actual, &expected, "empty-prelude");
}

/// Case (b): non-empty 2-module synthetic prelude with at least one enum.
/// compile_with_prelude_context must produce a CompiledModule identical to
/// compile_with_prelude for the same prelude.
#[test]
fn compile_with_prelude_context_parity_two_module_prelude_with_enum() {
    let enum_status = EnumDef {
        name: "Status".to_string(),
        variants: vec!["Active".to_string(), "Inactive".to_string()],
    };
    let enum_mode = EnumDef {
        name: "Mode".to_string(),
        variants: vec!["Fast".to_string(), "Slow".to_string()],
    };

    // Build two synthetic prelude modules with enums.
    let pm1 = CompiledModuleBuilder::new(ModulePath::single("synth_pm1"))
        .enum_def(enum_status.clone())
        .build();
    let pm2 = CompiledModuleBuilder::new(ModulePath::single("synth_pm2"))
        .enum_def(enum_mode.clone())
        .build();

    let prelude: Vec<CompiledModule> = vec![pm1, pm2];

    // User module that just defines a plain structure (no enum ref needed for
    // parity — we're testing the enum phase, not user-level enum resolution).
    let source = r#"
structure def Widget {
    param count : Int = 1
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("parity_two_module"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let expected = compile_with_prelude(&parsed, &prelude);
    let ctx = PreludeContext::from_slice(&prelude);
    let actual = compile_with_prelude_context(&parsed, &ctx);

    assert_compiled_module_parity(&actual, &expected, "two-module-prelude");
}
