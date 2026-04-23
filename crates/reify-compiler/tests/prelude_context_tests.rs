//! Tests for PreludeContext — pre-built prelude cache to avoid re-flattening enums.
//!
//! TDD structure:
//!   step-1: PreludeContext::new invariants (empty + two-module enum ordering)
//!   step-3: PreludeContext::from_slice ergonomics (borrow stability + parity)
//!   step-5: compile_with_prelude_context parity with compile_with_prelude
//!   step-7: load_stdlib_context caching (pointer stability + enum parity)

use reify_compiler::{CompiledModule, PreludeContext};
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
