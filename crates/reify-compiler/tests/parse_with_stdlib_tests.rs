//! Tests for the prelude-aware parse path introduced by task 2525.
//!
//! TDD structure:
//!   step-3: PreludeContext::enum_names() iterator parity with resolution_enums()
//!   step-5: reify_compiler::parse_with_stdlib end-to-end behavior on stdlib enums
//!
//! These tests live here (rather than in `prelude_context_tests.rs` or one of
//! the existing parser test files) so the change set is self-contained on the
//! task's lock list and the new entry points have a single, named home.

use reify_compiler::PreludeContext;
use reify_test_support::CompiledModuleBuilder;
use reify_types::{EnumDef, ModulePath};

// ─── step-3: PreludeContext::enum_names() iterator parity ──────────────────

/// `enum_names()` must iterate the same names as
/// `resolution_enums().iter().map(|e| e.name.as_str())`, in the same order,
/// for a multi-module synthetic prelude.
///
/// The fixture mirrors the two-module pattern in
/// `prelude_context_tests.rs::new_two_module_prelude_preserves_enum_order`.
/// Equality of the collected `Vec<&str>` pins both ordering and content.
#[test]
fn enum_names_iterates_in_resolution_enums_order() {
    let enum_a = EnumDef {
        name: "EnumA".to_string(),
        variants: vec!["A1".to_string(), "A2".to_string()],
        doc: None,
    };
    let enum_b = EnumDef {
        name: "EnumB".to_string(),
        variants: vec!["B1".to_string()],
        doc: None,
    };
    let enum_c = EnumDef {
        name: "EnumC".to_string(),
        variants: vec!["C1".to_string(), "C2".to_string(), "C3".to_string()],
        doc: None,
    };

    let m1 = CompiledModuleBuilder::new(ModulePath::single("enum_names_m1"))
        .enum_def(enum_a.clone())
        .enum_def(enum_b.clone())
        .build();
    let m2 = CompiledModuleBuilder::new(ModulePath::single("enum_names_m2"))
        .enum_def(enum_c.clone())
        .build();

    let ctx = PreludeContext::new(&[&m1, &m2]);

    let from_names: Vec<&str> = ctx.enum_names().collect();
    let from_resolution: Vec<&str> = ctx
        .resolution_enums()
        .iter()
        .map(|e| e.name.as_str())
        .collect();

    assert_eq!(
        from_names, from_resolution,
        "enum_names() must iterate the same names as resolution_enums(), in the same order"
    );
    assert_eq!(
        from_names,
        vec!["EnumA", "EnumB", "EnumC"],
        "enum_names() must yield names in source order across modules"
    );
}

/// `enum_names()` on an empty prelude must yield zero items, matching
/// `resolution_enums()` on the same context.
#[test]
fn enum_names_empty_prelude_yields_no_items() {
    let ctx: PreludeContext = PreludeContext::new(&[]);

    assert_eq!(ctx.enum_names().count(), 0);
    assert_eq!(ctx.resolution_enums().len(), 0);
}

// ─── step-5: reify_compiler::parse_with_stdlib end-to-end ─────────────────

/// `reify_compiler::parse_with_stdlib` parses a source referencing
/// `CorrosionClass.C5` and `BiocompatibilityClass.USP_Class_VI` (both
/// declared only in the stdlib prelude — NOT in this source string) and
/// produces:
///   (a) zero parse errors
///   (b) two `EnumAccess` nodes — one for each stdlib enum reference
///   (c) zero `Severity::Error` diagnostics when the result is fed to
///       `compile_with_stdlib`
///
/// This is the canonical end-to-end signal that the prelude-aware parser
/// actually unblocks the inline-redecl workaround in materials_chemical_tests.rs.
/// Test fails to compile until step-6 lands `reify_compiler::parse_with_stdlib`.
#[test]
fn parse_with_stdlib_resolves_stdlib_enum_access_without_inline_redecls() {
    use reify_syntax::ExprKind;
    use reify_types::Severity;

    let source = r#"
structure def TitaniumImplant : Biocompatible + CorrosionResistant {
    param density : Real = 4500.0
    param name : String = "titanium"
    param biocompatibility_class : BiocompatibilityClass = BiocompatibilityClass.USP_Class_VI
    param corrosion_class : CorrosionClass = CorrosionClass.C5
}
"#;

    let parsed =
        reify_compiler::parse_with_stdlib(source, ModulePath::single("parse_with_stdlib_test"));

    // (a) Zero parse errors.
    assert!(
        parsed.errors.is_empty(),
        "parse_with_stdlib should produce no parse errors, got: {:?}",
        parsed.errors
    );

    // (b) Exactly two EnumAccess nodes — CorrosionClass.C5 and
    // BiocompatibilityClass.USP_Class_VI. Collected into a Vec and sorted so
    // that order differences between fixture changes do not matter, while
    // duplicate emission (e.g. the parser emitting CorrosionClass.C5 twice)
    // is still caught — a duplicate would make the Vec longer than expected
    // and fail the assert_eq.
    let mut enum_accesses: Vec<(String, String)> = Vec::new();
    reify_test_support::visit_structure_member_root_exprs(&parsed, |expr| {
        if let ExprKind::EnumAccess { type_name, variant } = &expr.kind {
            enum_accesses.push((type_name.clone(), variant.clone()));
        }
    });
    enum_accesses.sort();
    let mut expected: Vec<(String, String)> = vec![
        (
            "BiocompatibilityClass".to_string(),
            "USP_Class_VI".to_string(),
        ),
        ("CorrosionClass".to_string(), "C5".to_string()),
    ];
    expected.sort();
    assert_eq!(
        enum_accesses, expected,
        "expected exactly the prelude EnumAccess entries (sorted), got: {:?}, expected: {:?}",
        enum_accesses, expected
    );

    // (c) Zero error-severity diagnostics from compile_with_stdlib.
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile_with_stdlib should produce no Error diagnostics for prelude-aware-parsed source, got: {:?}",
        errors
    );
}
