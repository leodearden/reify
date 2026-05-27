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

use reify_test_support::{compile_source, errors_only, warnings_only};
use reify_core::{Severity, Type};
use reify_ir::{CompiledExprKind, Value};

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
    let module = compile_source(source);

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
        other => panic!(
            "expected CompiledExprKind::ValueRef for 'd', got {:?}",
            other
        ),
    }
    assert_eq!(
        d_expr.result_type,
        Type::Real,
        "expected 'd' to have result_type Real, got {:?}",
        d_expr.result_type
    );
}

// ── amend: collection sub bare-identifier test ───────────────────────────────

/// Regression test: verify that `sub_member_types` is populated correctly for
/// **collection** subs (not just non-collection subs).
///
/// The entity.rs dedup refactor collapsed two `compiled_templates.iter().find(...)`
/// blocks into one.  Both blocks populated `sub_member_types`, but one was gated
/// on the non-collection path.  If the refactor accidentally restricted
/// `sub_member_types` population to one class of sub, the bare-identifier
/// resolution path in expr.rs (~line 127) would fall through to the fallback
/// (`__list_{name}` without a member suffix, wrong type).
///
/// This test uses a collection sub (`sub parts : List<Inner>`) and accesses it
/// via a bare identifier (`let gs = parts`), which exercises the
/// `scope.collection_sub_names` / `scope.sub_member_types` path in expr.rs.
///
/// Expected: `gs` compiles to `ValueRef { entity: "Outer", member: "__list_parts__radius" }`
/// with type `List(Real)` — i.e. the per-member list for the lex-first member of `Inner`.
#[test]
fn collection_sub_bare_identifier_populates_member_types() {
    let source = r#"
        structure Inner { param radius : Real = 1.0 }
        structure Outer {
            sub parts : List<Inner>
            let gs = parts
        }
    "#;
    let module = compile_source(source);

    // No errors — the collection sub should compile cleanly.
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no compile errors for collection sub, got: {:?}",
        errs
    );

    // Find the 'Outer' template.
    let outer_tmpl = module
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("should have template 'Outer'");

    // Find the 'gs' value cell.
    let gs_cell = outer_tmpl
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "gs")
        .expect("should have let binding 'gs'");

    let gs_expr = gs_cell
        .default_expr
        .as_ref()
        .expect("'gs' should have a default expression");

    // Bare collection sub identifier resolves to the lex-first member's per-member list.
    // `Inner` has exactly one member: `radius` (Real).  BTreeMap ordering is deterministic.
    match &gs_expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "Outer",
                "entity should be 'Outer' (parent structure), got '{}'",
                id.entity
            );
            assert_eq!(
                id.member, "__list_parts__radius",
                "member should be '__list_parts__radius' (lex-first member of Inner), got '{}'",
                id.member
            );
        }
        other => panic!(
            "expected ValueRef for bare collection sub 'parts', got {:?}",
            other
        ),
    }

    // Result type must be List(...) — the element type comes from sub_member_types.
    match &gs_expr.result_type {
        Type::List(_) => {}
        other => panic!(
            "expected List type for bare collection sub identifier, got {:?}",
            other
        ),
    }
}

// ── step-3: test for expr.rs QualifiedAccess ICE fallback ────────────────────

/// Test: when a structure claims trait conformance (`: MechTrait`) but does NOT
/// define the required member, `TraitName::member` (QualifiedAccess syntax) in the
/// structure body should produce `CompiledExpr::literal(Value::Undef, Type::Real)`
/// rather than a fabricated `ValueRef`.
///
/// This is the ICE fallback path in the `QualifiedAccess` match arm of expr.rs:
/// `scope.resolve(member)` returns `None` because the member was never defined
/// (despite the trait claim). Conformance checking separately emits an error about
/// the missing member.
///
/// Expected behaviour (after fix):
///   - At least one conformance error exists (missing member).
///   - The `let` binding's compiled expression is `Literal(Value::Undef)` with
///     `result_type == Type::Real`.
///
/// This test FAILS with current code (which returns ValueRef with a fabricated id)
/// and PASSES after the fix in step-4.
#[test]
fn qualified_access_ice_fallback_returns_undef_literal() {
    let source = r#"
        trait MechTrait {
            param diameter : Real
        }
        structure Bolt : MechTrait {
            let d = MechTrait::diameter
        }
    "#;
    // `Bolt` conforms to `MechTrait` but does not define `diameter`, so
    // `scope.resolve("diameter")` returns None → ICE fallback.
    let module = compile_source(source);

    // At least one conformance error should exist (missing required member).
    let errs = errors_only(&module);
    assert!(
        !errs.is_empty(),
        "expected at least one conformance error (missing 'diameter'), got none"
    );

    // Assert exactly one info-severity diagnostic for the QualifiedAccess ICE fallback.
    let info_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info && d.message.contains("member not found in scope"))
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected one info diagnostic for QualifiedAccess fallback, got: {:?}",
        info_diags
    );

    // Find the 'Bolt' template and locate the 'd' value cell.
    let bolt_tmpl = module
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("should have template 'Bolt'");

    let d_cell = bolt_tmpl
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have let binding 'd'");

    let d_expr = d_cell
        .default_expr
        .as_ref()
        .expect("'d' should have a default expression");

    // After the fix: the ICE fallback must return Literal(Undef), not a ValueRef.
    match &d_expr.kind {
        CompiledExprKind::Literal(Value::Undef) => {
            // Expected post-fix behaviour.
        }
        CompiledExprKind::ValueRef(_) => {
            panic!(
                "ICE fallback returned ValueRef instead of Literal(Undef); \
                 this should be fixed by the step-4 impl change in expr.rs"
            );
        }
        other => panic!(
            "expected Literal(Value::Undef) for 'd' ICE fallback, got {:?}",
            other
        ),
    }
    assert_eq!(
        d_expr.result_type,
        Type::Real,
        "expected 'd' to have result_type Real, got {:?}",
        d_expr.result_type
    );
}
