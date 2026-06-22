//! Integration tests for task 3941 ζ (trait associated-function INSTANCE
//! dispatch): the `obj.(Trait::method)(args)` lowering and per-conformer
//! registration driven through the full compile pipeline via
//! `reify_test_support::compile_source`.
//!
//! These tests pin the dispatch-site contract that ζ builds on top of δ's
//! per-conformer `TopologyTemplate.assoc_fns` table:
//!   * a `TraitMethodCall` lowers to a `UserFunctionCall` of a per-conformer
//!     mangled symbol with the object prepended as the bound `self` arg, typed
//!     from the trait's declared assoc-fn return type (step-2);
//!   * the assoc-fn body resolves bare member refs (`diameter`) as member
//!     accesses on the `self` receiver (step-4 desugar);
//!   * each conformer's instance fn is registered into the module function
//!     table under its mangled symbol so the evaluator can resolve it (step-6).
//!
//! Step-1 (RED): the dispatch-lowering assertions fail until step-2 replaces the
//! `TraitMethodCall` poison stub (`expr.rs`) with real instance dispatch.

use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};
use reify_ir::CompiledExprKind;
use reify_test_support::compile_source;

/// Shared fixture: a `Cylindrical` trait with a default-providing instance fn
/// `lateral_area(self) -> Scalar<Area>`, a `Pin` conformer, and an `Assembly`
/// that consumes it via instance dispatch `pin.(Cylindrical::lateral_area)()`.
const CYLINDER_SRC: &str = r#"
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * length }
}

structure def Pin : Cylindrical {
    param diameter : Length = 8mm
    param length : Length = 40mm
}

structure def Assembly {
    sub pin : Pin
    let wetted = pin.(Cylindrical::lateral_area)()
}
"#;

/// (step-1a/b) Instance dispatch `pin.(Cylindrical::lateral_area)()` must:
///   (a) no longer emit the `δ/ζ` poison-stub "not yet supported" error;
///   (b) lower the consuming `Assembly.wetted` let-binding to a
///       `UserFunctionCall` (not a poison literal) whose `result_type` is the
///       `Scalar<Area>` the trait declares for `lateral_area`.
///
/// RED until step-2: the `TraitMethodCall` arm in `expr.rs` is the poison stub,
/// so `wetted` compiles to a poison literal and the "not yet supported" error
/// fires.
#[test]
fn instance_dispatch_lowers_to_user_function_call() {
    let module = compile_source(CYLINDER_SRC);

    // (a) The poison-stub "not yet supported" error must be gone.
    let not_supported: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("not yet supported"))
        .collect();
    assert!(
        not_supported.is_empty(),
        "instance dispatch should be implemented — no 'not yet supported' error; got: {:?}",
        not_supported
    );

    // (b) Assembly.wetted lowers to a UserFunctionCall typed as the Area scalar.
    let assembly = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("compiled module should contain a template for 'Assembly'");
    let wetted = assembly
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "wetted")
        .expect("Assembly should have a 'wetted' value cell");
    let expr = wetted
        .default_expr
        .as_ref()
        .expect("the 'wetted' let-binding should carry a compiled value expr");
    assert!(
        matches!(expr.kind, CompiledExprKind::UserFunctionCall { .. }),
        "wetted should lower to a UserFunctionCall (not a poison literal); got: {:?}",
        expr.kind
    );
    assert_eq!(
        expr.result_type,
        Type::Scalar {
            dimension: DimensionVector::AREA
        },
        "wetted's result type should be the trait's declared Scalar<Area> return type"
    );
}

/// (step-1 negative case) An instance call naming a method the trait never
/// declares — `pin.(Cylindrical::nope)()` — must emit exactly one
/// `DiagnosticCode::TraitMethodUnknown` and must not panic the compiler.
///
/// RED until step-2: the poison stub emits a generic "not yet supported" error
/// with no `TraitMethodUnknown` code.
#[test]
fn instance_dispatch_unknown_method_emits_trait_method_unknown() {
    let source = r#"
trait Cylindrical {
    param diameter : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * diameter }
}

structure def Pin : Cylindrical {
    param diameter : Length = 8mm
}

structure def Assembly {
    sub pin : Pin
    let bad = pin.(Cylindrical::nope)()
}
"#;
    let module = compile_source(source);

    let unknown: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitMethodUnknown))
        .collect();
    assert_eq!(
        unknown.len(),
        1,
        "pin.(Cylindrical::nope)() should emit exactly one TraitMethodUnknown; \
         all diagnostics: {:?}",
        module.diagnostics
    );
}
