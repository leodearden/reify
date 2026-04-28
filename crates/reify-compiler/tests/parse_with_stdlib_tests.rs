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
    };
    let enum_b = EnumDef {
        name: "EnumB".to_string(),
        variants: vec!["B1".to_string()],
    };
    let enum_c = EnumDef {
        name: "EnumC".to_string(),
        variants: vec!["C1".to_string(), "C2".to_string(), "C3".to_string()],
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
