//! Task 1570 regression tests.
//!
//! Covers two independent improvements in the reify-compiler crate:
//!
//! (1) entity.rs dedup: collapse two identical `compiled_templates.iter().find(...)`
//!     blocks into a single lookup that handles both deprecation warnings/sub_structure_traits
//!     and sub_member_types population.
//!
//! (2) expr.rs ICE fallback: in the `QualifiedAccess` match arm, the `None` branch of
//!     `scope.resolve(member)` should return `CompiledExpr::literal(Value::Undef, Type::Real)`
//!     rather than fabricating a `ValueCellId`.

use reify_types::{CompiledExprKind, Severity, Type};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("task_1570_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

// ── step-1: characterization test for entity.rs dedup refactor ───────────────

/// Characterization test: a structure with a sub referencing a `@deprecated` structure
/// that conforms to a trait, using `InstanceQualifiedAccess`.
///
/// This test simultaneously exercises both code paths that were previously housed
/// in two separate `if let Some(child_tmpl) = compiled_templates.iter().find(...)` blocks:
///
///   - Block 1 (lines 401-418): deprecation check + `scope.sub_structure_traits` insertion.
///   - Block 2 (lines 420-431): `scope.sub_member_types` insertion.
///
/// After the dedup refactor (step-2 in the plan), both operations happen in a single lookup.
/// This test should PASS with both the current code and after the refactor.
///
/// Regression coverage:
///   - If block 1 is accidentally dropped, `sub_structure_traits` is not populated and the
///     compiler emits "does not implement trait" errors instead of a clean ValueRef.
///   - If block 2 is accidentally dropped, `sub_member_types` is not populated and the
///     compiler emits an ICE about an unresolved sub-member type.
///   - If the deprecation check is dropped, no deprecation warning is emitted.
#[test]
fn deprecated_sub_resolves_members_and_emits_warning() {
    let source = r#"
        trait MechTrait {
            param diameter : Real
        }
        @deprecated("Use NewBolt")
        structure OldBolt : MechTrait {
            param diameter : Real = 5.0
        }
        structure Assembly {
            sub b = OldBolt()
            let d = b.(MechTrait::diameter)
        }
    "#;
    let module = compile_module(source);

    // (a) Deprecation warning must be emitted for `OldBolt`.
    let deprecation_warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("deprecated") && d.message.contains("OldBolt"))
        .collect();
    assert_eq!(
        deprecation_warns.len(),
        1,
        "expected exactly one deprecation warning for OldBolt, got: {:?}",
        deprecation_warns
    );
    assert!(
        deprecation_warns[0].message.contains("Use NewBolt"),
        "expected warning message to contain 'Use NewBolt', got: {}",
        deprecation_warns[0].message
    );

    // (b) No ICE diagnostic.
    let has_ice = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic, got: {:?}",
        module.diagnostics
    );

    // (c) No error-severity diagnostics at all.
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no compile errors, got: {:?}",
        errs
    );

    // (d) The `let d = ...` binding resolves to a ValueRef scoped to the sub-component.
    //
    // `InstanceQualifiedAccess` (`b.(MechTrait::diameter)`) should produce
    // `ValueRef { entity: "Assembly.b", member: "diameter" }` with type `Real`.
    let assembly_tmpl = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("should have template 'Assembly'");

    let d_cell = assembly_tmpl
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have let binding 'd'");

    let d_expr = d_cell
        .default_expr
        .as_ref()
        .expect("'d' should have a default expression");

    match &d_expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "Assembly.b",
                "expected ValueRef entity to be 'Assembly.b', got '{}'",
                id.entity
            );
            assert_eq!(
                id.member, "diameter",
                "expected ValueRef member to be 'diameter', got '{}'",
                id.member
            );
        }
        other => panic!("expected CompiledExprKind::ValueRef for 'd', got {:?}", other),
    }
    assert_eq!(
        d_expr.result_type,
        Type::Real,
        "expected 'd' to have result_type Real, got {:?}",
        d_expr.result_type
    );
}
