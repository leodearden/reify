//! `ground(sub)` / `fix(sub)` desugar tests (geometric-relations η, task 4387).
//!
//! The leaf signal: `ground(sub)` sugar RESOLVES TO `fasten(sub.frame,
//! self.frame)` (design §4). The desugar is a compile-time AST rewrite in
//! `check_relate_relations` (entity.rs), scoped to the `relate {}` block, so the
//! relation stored on `TopologyTemplate::relations` is a real `fasten`
//! `FunctionCall` — directly observable here and fed to ζ's solver unchanged.
//! `fix(sub)` is the identical desugar.
//!
//! Both operands are intrinsic-frame projections:
//!  - `sub.frame` lowers to the cross-sub datum-access shape
//!    `IndexAccess { ValueRef(<sub> : StructureRef), Literal(String("frame")) }`
//!    (the shape `reify-eval`'s `decode_operand` decodes), typed `Frame(3)`.
//!  - `self.frame` lowers to `MethodCall { ValueRef(__self : StructureRef),
//!    "frame", [] }`, typed `Frame(3)` (η step-8).
//!
//! RED (step-13): ground/fix are not desugared — `ground` is an unknown function
//! and `sub.frame` does not yet resolve (an intrinsic datum is not a declared
//! member of the sub's structure), so the stored relation is not a `fasten` call.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::compile_source;

/// The two `frame` operands of a desugared `ground`/`fix`, classified by their
/// compiled shape so the test does not over-bind on value-cell id internals.
#[derive(Debug, PartialEq)]
enum FrameOperand {
    /// `sub.frame`: `IndexAccess { ValueRef(_ : StructureRef(s)), String("frame") }`.
    SubDatum { structure: String, member: String },
    /// `self.frame`: `MethodCall { ValueRef(_ : StructureRef(s)), "frame", [] }`.
    SelfDatum { structure: String, member: String },
    Other,
}

fn classify(e: &CompiledExpr) -> FrameOperand {
    match &e.kind {
        CompiledExprKind::IndexAccess { object, index } => {
            let member = match &index.kind {
                CompiledExprKind::Literal(Value::String(s)) => s.clone(),
                _ => return FrameOperand::Other,
            };
            match &object.result_type {
                Type::StructureRef(s) => FrameOperand::SubDatum {
                    structure: s.clone(),
                    member,
                },
                _ => FrameOperand::Other,
            }
        }
        CompiledExprKind::MethodCall { object, method, args } if args.is_empty() => {
            match &object.result_type {
                Type::StructureRef(s) => FrameOperand::SelfDatum {
                    structure: s.clone(),
                    member: method.clone(),
                },
                _ => FrameOperand::Other,
            }
        }
        _ => FrameOperand::Other,
    }
}

/// Compile a structure whose `relate {}` block holds a single `verb(a)` member,
/// and return the lone compiled relation plus the diagnostics.
fn compile_single_relation(verb: &str) -> (CompiledExpr, Vec<reify_core::Diagnostic>) {
    let src = format!(
        r#"structure Widget {{ param w : Length = 1mm }}
structure S {{
    sub a : Widget at auto
    relate {{ {verb}(a) }}
}}"#
    );
    let compiled = compile_source(&src);
    let s = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("structure S present");
    assert_eq!(
        s.relations.len(),
        1,
        "expected exactly one threaded relation, got {}",
        s.relations.len()
    );
    (s.relations[0].clone(), compiled.diagnostics.clone())
}

/// Assert the relation is `fasten(a.frame, self.frame)` over two Frame operands.
fn assert_fasten_ground_desugar(relation: &CompiledExpr) {
    assert_eq!(
        relation.result_type,
        Type::Relation,
        "a desugared ground/fix relation types to Relation"
    );
    let (function_name, args) = match &relation.kind {
        CompiledExprKind::FunctionCall { function, args } => (function.name.clone(), args),
        other => panic!("expected a fasten FunctionCall, got {other:?}"),
    };
    assert_eq!(function_name, "fasten", "ground/fix desugars to fasten");
    assert_eq!(args.len(), 2, "fasten over two frame operands");

    // arg0 = a.frame (a StructureRef->Frame projection on sub `a`).
    assert_eq!(
        classify(&args[0]),
        FrameOperand::SubDatum {
            structure: "Widget".to_string(),
            member: "frame".to_string(),
        },
        "first operand is the sub's frame projection (a.frame)"
    );
    assert_eq!(args[0].result_type, Type::Frame(3), "a.frame : Frame(3)");

    // arg1 = self.frame (a StructureRef->Frame projection on self).
    assert_eq!(
        classify(&args[1]),
        FrameOperand::SelfDatum {
            structure: "S".to_string(),
            member: "frame".to_string(),
        },
        "second operand is self.frame projection"
    );
    assert_eq!(args[1].result_type, Type::Frame(3), "self.frame : Frame(3)");
}

fn assert_no_relate_expects_relation(diags: &[reify_core::Diagnostic]) {
    assert!(
        !diags.iter().any(|d| d.code == Some(DiagnosticCode::RelateExpectsRelation)),
        "a desugared fasten must not draw E_RELATE_EXPECTS_RELATION; diags: {:?}",
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn ground_desugars_to_fasten_sub_frame_self_frame() {
    let (relation, diags) = compile_single_relation("ground");
    assert_fasten_ground_desugar(&relation);
    assert_no_relate_expects_relation(&diags);
}

#[test]
fn fix_desugars_identically_to_ground() {
    let (relation, diags) = compile_single_relation("fix");
    assert_fasten_ground_desugar(&relation);
    assert_no_relate_expects_relation(&diags);
}
