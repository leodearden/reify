//! Tests for PreludeContext — pre-built prelude cache to avoid re-flattening enums.
//!
//! TDD structure:
//!   step-1: PreludeContext::new invariants (empty + two-module enum ordering)
//!   step-3: PreludeContext::from_slice ergonomics (borrow stability + parity)
//!   step-5: compile_with_prelude_context parity with compile_with_prelude
//!   step-7: load_stdlib_context caching (pointer stability + enum parity)

use reify_compiler::PreludeContext;
use reify_test_support::CompiledModuleBuilder;
use reify_types::{EnumDef, ModulePath};

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
