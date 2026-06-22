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
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
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

/// Recursively collect the string keys of every
/// `IndexAccess { index: Literal(String(k)), .. }` node in a compiled expression
/// tree. A desugared `self.member` lowers to `IndexAccess(self, "member")`
/// (`expr.rs` StructureRef member-access path), so the presence of a key proves
/// the bare member ref was resolved as a field access on the `self` receiver
/// rather than left as a free name / poison literal.
fn collect_index_access_keys(expr: &CompiledExpr, out: &mut Vec<String>) {
    use CompiledExprKind as K;
    match &expr.kind {
        K::IndexAccess { object, index } => {
            if let K::Literal(Value::String(k)) = &index.kind {
                out.push(k.clone());
            }
            collect_index_access_keys(object, out);
            collect_index_access_keys(index, out);
        }
        K::BinOp { left, right, .. } => {
            collect_index_access_keys(left, out);
            collect_index_access_keys(right, out);
        }
        K::UnOp { operand, .. } => collect_index_access_keys(operand, out),
        K::FunctionCall { args, .. } | K::UserFunctionCall { args, .. } => {
            for a in args {
                collect_index_access_keys(a, out);
            }
        }
        K::MethodCall { object, args, .. } => {
            collect_index_access_keys(object, out);
            for a in args {
                collect_index_access_keys(a, out);
            }
        }
        K::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_index_access_keys(condition, out);
            collect_index_access_keys(then_branch, out);
            collect_index_access_keys(else_branch, out);
        }
        // Leaves (Literal, ValueRef, …) and node kinds that cannot occur in these
        // arithmetic assoc-fn bodies: no further descent needed.
        _ => {}
    }
}

/// Locate the compiled `(trait_name, fn_name)` associated-function body on the
/// named conformer template and return its `result_expr`. Panics with a clear
/// message if the template or the assoc-fn table entry is missing.
fn assoc_fn_result_expr<'m>(
    module: &'m reify_compiler::CompiledModule,
    conformer: &str,
    trait_name: &str,
    fn_name: &str,
) -> &'m CompiledExpr {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == conformer)
        .unwrap_or_else(|| panic!("compiled module should contain a '{conformer}' template"));
    let af = template
        .assoc_fns
        .iter()
        .find(|a| a.trait_name == trait_name && a.fn_name == fn_name)
        .unwrap_or_else(|| {
            panic!(
                "'{conformer}' should carry an assoc-fn table entry for \
                 ({trait_name}, {fn_name}); got entries: {:?}",
                template
                    .assoc_fns
                    .iter()
                    .map(|a| (a.trait_name.as_str(), a.fn_name.as_str()))
                    .collect::<Vec<_>>()
            )
        });
    &af.function.body.result_expr
}

/// (step-3a/b) The trait-default body `pi * diameter * length` uses bare member
/// refs. ζ desugars each bare `Identifier(x)` that names a conformer member into
/// `self.x`, which lowers to `IndexAccess(self, "x")`. After the desugar:
///   (a) no `UnresolvedName` error is emitted for `diameter` / `length`;
///   (b) the compiled `Pin` assoc-fn body resolves both bare members as
///       `IndexAccess` keyed by `"diameter"` / `"length"` on the `self` receiver.
///
/// RED until step-4: δ compiles the body in a scope holding only `self` + params,
/// so bare `diameter` / `length` hit the `UnresolvedName` poison path and the
/// body's leaves are poison literals — no `IndexAccess` keys.
#[test]
fn assoc_fn_body_bare_member_desugars_to_self_index_access() {
    let module = compile_source(CYLINDER_SRC);

    // (a) No UnresolvedName error naming the conformer members.
    let unresolved_members: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::UnresolvedName)
        })
        .filter(|d| d.message.contains("diameter") || d.message.contains("length"))
        .collect();
    assert!(
        unresolved_members.is_empty(),
        "bare `diameter` / `length` in the assoc-fn body should desugar to \
         `self.member` (no UnresolvedName); got: {unresolved_members:?}"
    );

    // (b) The body's bare members resolve to IndexAccess-on-self keyed by name.
    let body = assoc_fn_result_expr(&module, "Pin", "Cylindrical", "lateral_area");
    let mut keys = Vec::new();
    collect_index_access_keys(body, &mut keys);
    assert!(
        keys.contains(&"diameter".to_string()),
        "bare `diameter` should lower to IndexAccess(self, \"diameter\"); \
         IndexAccess keys found: {keys:?}"
    );
    assert!(
        keys.contains(&"length".to_string()),
        "bare `length` should lower to IndexAccess(self, \"length\"); \
         IndexAccess keys found: {keys:?}"
    );
}

/// (step-3 parity) The explicit `self.diameter` / `self.length` form lowers to the
/// SAME `IndexAccess`-on-self nodes that the bare-`diameter` sugar must produce.
/// This pins the desugar target: bare `diameter` ≡ `self.diameter` (PRD §4.4).
#[test]
fn assoc_fn_body_explicit_self_member_lowers_to_index_access() {
    const SRC: &str = r#"
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * self.diameter * self.length }
}

structure def Pin : Cylindrical {
    param diameter : Length = 8mm
    param length : Length = 40mm
}
"#;
    let module = compile_source(SRC);

    let body = assoc_fn_result_expr(&module, "Pin", "Cylindrical", "lateral_area");
    let mut keys = Vec::new();
    collect_index_access_keys(body, &mut keys);
    assert!(
        keys.contains(&"diameter".to_string()) && keys.contains(&"length".to_string()),
        "explicit `self.diameter` / `self.length` should lower to IndexAccess-on-self \
         keyed by member name; IndexAccess keys found: {keys:?}"
    );
}
