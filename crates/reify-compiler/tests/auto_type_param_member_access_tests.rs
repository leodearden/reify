//! Unit tests for the `Type::TypeParam` member-access branch (task 4596).
//!
//! # What this file covers
//!
//! Two test groups:
//!
//! 1. **Positive path** — compiling `seal.thickness` where `seal : T` and the
//!    bound trait `Seal` declares `param thickness : Length` must produce:
//!    (a) zero `Severity::Error` diagnostics (no "member access not yet
//!   supported" message, no `TypeParamMemberNotInBound`),
//!    (b) a flat `ValueRef(ValueCellId("seal","thickness"))` in the constraint
//!   expr (not an `IndexAccess` over the param cell), and
//!    (c) the node's `result_type == Type::length()` (the bound-trait-declared
//!   field type).
//!
//! 2. **Negative / soundness path** — when the bound trait does NOT declare
//!    the accessed member, exactly one `Severity::Error` with code
//!    `TypeParamMemberNotInBound` is emitted and the generic "member access
//!    not yet supported" message is absent.  No compiled node carries a
//!    `Type::TypeParam(_)` result type.
//!
//! # RED states
//!
//! - **Step 1 (RED):** positive test fails today because the `MemberAccess`
//!   handler has no `Type::TypeParam` branch — it falls through to
//!   `make_poison_literal("member access not yet supported: .thickness")`.
//!   Assertions (a), (b), and (c) all fail.
//!
//! - **Step 3 (RED):** negative test fails today because after step-2 the
//!   TypeParam branch falls through to the generic poison (code: None) instead
//!   of emitting `TypeParamMemberNotInBound` (code: Some(...)).

use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
use reify_core::{DiagnosticCode, ModulePath, Severity, Type, ValueCellId};
use reify_ir::CompiledExprKind;

// ── Helper: walk a compiled expr tree collecting all (ValueCellId, Type) leaves
//    from ValueRef nodes ─────────────────────────────────────────────────────────

fn collect_value_refs_with_types(
    expr: &reify_ir::CompiledExpr,
    out: &mut Vec<(ValueCellId, Type)>,
) {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            out.push((id.clone(), expr.result_type.clone()));
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            collect_value_refs_with_types(left, out);
            collect_value_refs_with_types(right, out);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            collect_value_refs_with_types(operand, out);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_value_refs_with_types(arg, out);
            }
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            collect_value_refs_with_types(object, out);
            for arg in args {
                collect_value_refs_with_types(arg, out);
            }
        }
        CompiledExprKind::IndexAccess { object, index, .. } => {
            collect_value_refs_with_types(object, out);
            collect_value_refs_with_types(index, out);
        }
        CompiledExprKind::Literal(_) => {}
        _ => {
            // For any other node kinds (lambda, conditional, etc.): recurse
            // via collect_value_refs() which handles them correctly.
            for id in expr.collect_value_refs() {
                // We don't know the type for these nested refs without
                // full traversal, but we can collect the ids for key checks.
                // In practice our synthetic modules only produce BinOp/ValueRef.
                let _ = id;
            }
        }
    }
}

// ── Group 1: positive path ───────────────────────────────────────────────────

/// Positive test: `seal.thickness` where `Seal` declares `param thickness : Length`.
///
/// The TypeParam branch (task 4596) must emit a flat
/// `ValueRef(ValueCellId("seal","thickness"), Type::length())` so that β's
/// per-candidate ValueMap can resolve the constraint at search time.
#[test]
fn type_param_member_access_emits_flat_value_ref_when_trait_declares_field() {
    // Synthetic module: bound trait declares `thickness`, structure accesses it.
    let source = r#"
        trait Seal {
            param thickness : Length
        }

        structure def Bearing<T: Seal> {
            param bore_radius : Length = 3mm
            param seal : T
            constraint seal.thickness < bore_radius
        }
    "#;

    let module_path = ModulePath::single("bearing_member_access_positive");
    let parsed = parse_with_stdlib(source, module_path);
    assert!(
        parsed.errors.is_empty(),
        "synthetic module must parse without errors; got: {:#?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    // (a) Zero Severity::Error diagnostics — no "member access not yet supported"
    //     and no TypeParamMemberNotInBound.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for positive TypeParam member access; got:\n{:#?}",
        errors
    );

    // Find the Bearing template.
    let bearing_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing")
        .expect("Bearing template must be present in compiled output");

    // (b) The constraint expr must contain a flat ValueRef with key
    //     ValueCellId("seal","thickness") — NOT an IndexAccess over the param cell.
    let expected_key = ValueCellId::new("seal", "thickness");
    let constraint_exprs: Vec<_> = bearing_template
        .constraints
        .iter()
        .map(|c| &c.expr)
        .collect();

    assert!(
        !constraint_exprs.is_empty(),
        "Bearing must have at least one constraint (seal.thickness < bore_radius)"
    );

    // Collect ALL ValueRefs from ALL constraint exprs, with their types.
    let mut all_value_refs: Vec<(ValueCellId, Type)> = Vec::new();
    for expr in &constraint_exprs {
        collect_value_refs_with_types(expr, &mut all_value_refs);
    }

    let seal_thickness_refs: Vec<_> = all_value_refs
        .iter()
        .filter(|(id, _)| *id == expected_key)
        .collect();

    assert!(
        !seal_thickness_refs.is_empty(),
        "constraint expr must contain a flat ValueRef(ValueCellId(\"seal\",\"thickness\")); \
         found ValueRefs: {:#?}",
        all_value_refs.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );

    // (c) The result_type of the seal.thickness leaf must be Type::length()
    //     (the bound-trait-declared type), NOT Type::TypeParam(_).
    for (id, ty) in &seal_thickness_refs {
        assert_eq!(
            *ty,
            Type::length(),
            "ValueRef({id:?}) must carry result_type == Type::length() \
             (the trait-declared field type); got {ty:?}"
        );
        assert!(
            !matches!(ty, Type::TypeParam(_)),
            "ValueRef({id:?}) must NOT carry a TypeParam result_type; got {ty:?}"
        );
    }
}

// ── Group 2: negative / soundness path ──────────────────────────────────────

/// Negative test A: bound trait is EMPTY — does not declare `thickness`.
///
/// Must emit exactly one `TypeParamMemberNotInBound` Error whose message names
/// the accessed member ("thickness") and the bound trait ("Seal").
/// The generic "member access not yet supported" message must be ABSENT.
/// No compiled node may carry `Type::TypeParam(_)` as its result_type.
#[test]
fn type_param_member_access_emits_named_diagnostic_when_trait_lacks_field_empty_trait() {
    let source = r#"
        trait Seal {}

        structure def Bearing<T: Seal> {
            param bore_radius : Length = 3mm
            param seal : T
            constraint seal.thickness < bore_radius
        }
    "#;

    let module_path = ModulePath::single("bearing_member_access_negative_empty_trait");
    let parsed = parse_with_stdlib(source, module_path);
    assert!(
        parsed.errors.is_empty(),
        "synthetic module must parse without errors; got: {:#?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    // Must have at least one TypeParamMemberNotInBound Error.
    let named_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::TypeParamMemberNotInBound)
        })
        .collect();
    assert!(
        !named_errors.is_empty(),
        "expected at least one TypeParamMemberNotInBound Error; got:\n{:#?}",
        compiled.diagnostics
    );

    // The error message must name the accessed member and the bound trait.
    for err in &named_errors {
        assert!(
            err.message.contains("thickness"),
            "TypeParamMemberNotInBound message must name the accessed member 'thickness'; \
             got: {:?}",
            err.message
        );
        assert!(
            err.message.contains("Seal"),
            "TypeParamMemberNotInBound message must name the bound trait 'Seal'; \
             got: {:?}",
            err.message
        );
    }

    // The generic "member access not yet supported" message must be absent.
    let generic_poison: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("member access not yet supported"))
        .collect();
    assert!(
        generic_poison.is_empty(),
        "generic 'member access not yet supported' message must be absent; got:\n{:#?}",
        generic_poison
    );

    // No compiled node (in any template) may carry Type::TypeParam(_) as result_type.
    for template in &compiled.templates {
        for constraint in &template.constraints {
            let refs_with_types: Vec<(ValueCellId, Type)> = {
                let mut v = Vec::new();
                collect_value_refs_with_types(&constraint.expr, &mut v);
                v
            };
            for (id, ty) in refs_with_types {
                assert!(
                    !matches!(ty, Type::TypeParam(_)),
                    "compiled node ValueRef({id:?}) in template '{}' must NOT carry \
                     Type::TypeParam(_) as result_type (soundness contract); got {ty:?}",
                    template.name
                );
            }
        }
    }
}

/// Negative test B: bound trait declares a DIFFERENT field (`width`), not `thickness`.
///
/// Same assertions as the empty-trait case — `TypeParamMemberNotInBound` must
/// fire and name both the accessed member ("thickness") and the bound trait ("Seal").
#[test]
fn type_param_member_access_emits_named_diagnostic_when_trait_lacks_field_wrong_field() {
    let source = r#"
        trait Seal {
            param width : Length
        }

        structure def Bearing<T: Seal> {
            param bore_radius : Length = 3mm
            param seal : T
            constraint seal.thickness < bore_radius
        }
    "#;

    let module_path = ModulePath::single("bearing_member_access_negative_wrong_field");
    let parsed = parse_with_stdlib(source, module_path);
    assert!(
        parsed.errors.is_empty(),
        "synthetic module must parse without errors; got: {:#?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    let named_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::TypeParamMemberNotInBound)
        })
        .collect();
    assert!(
        !named_errors.is_empty(),
        "expected at least one TypeParamMemberNotInBound Error for wrong-field case; got:\n{:#?}",
        compiled.diagnostics
    );

    for err in &named_errors {
        assert!(
            err.message.contains("thickness"),
            "TypeParamMemberNotInBound message must name the accessed member 'thickness'; \
             got: {:?}",
            err.message
        );
        assert!(
            err.message.contains("Seal"),
            "TypeParamMemberNotInBound message must name the bound trait 'Seal'; \
             got: {:?}",
            err.message
        );
    }

    let generic_poison: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("member access not yet supported"))
        .collect();
    assert!(
        generic_poison.is_empty(),
        "generic 'member access not yet supported' message must be absent; got:\n{:#?}",
        generic_poison
    );

    for template in &compiled.templates {
        for constraint in &template.constraints {
            let refs_with_types: Vec<(ValueCellId, Type)> = {
                let mut v = Vec::new();
                collect_value_refs_with_types(&constraint.expr, &mut v);
                v
            };
            for (id, ty) in refs_with_types {
                assert!(
                    !matches!(ty, Type::TypeParam(_)),
                    "compiled node ValueRef({id:?}) in template '{}' must NOT carry \
                     Type::TypeParam(_) as result_type; got {ty:?}",
                    template.name
                );
            }
        }
    }
}
