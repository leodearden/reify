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

// `reify_eval::EvalResult.values` is keyed by `ValueCellId`; the eval-based
// step-5 case below reads it via `.get(&id)`. Mirror the sibling static-dispatch
// e2e test's crate-level allow so the `mutable_key_type` lint stays quiet.
#![allow(clippy::mutable_key_type)]

use reify_core::{DiagnosticCode, DimensionVector, Severity, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::{compile_source, make_simple_engine, parse_and_compile_with_stdlib};

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

/// (step-5a) Each conformer's instance associated function must be registered
/// into the module's function table (`CompiledModule.functions`) under the
/// per-conformer mangled symbol so the evaluator can resolve it via
/// `find_matching_compiled_function`. The symbol is
/// `instance_assoc_fn_symbol("Pin", "Cylindrical", "lateral_area")`
/// (`pub(crate)` in `expr.rs`); its `"{conformer}::{trait}::{method}"` shape is
/// pinned here as the externally observable contract shared with the dispatch
/// site. The registered fn must keep δ's compiled shape: a leading `self`
/// receiver typed `StructureRef("Pin")` and the trait's declared `Scalar<Area>`
/// return type.
///
/// RED until step-6: δ stores the per-conformer `CompiledFunction` only on
/// `TopologyTemplate.assoc_fns`, never in `CompiledModule.functions`, so the
/// lookup below finds nothing.
#[test]
fn instance_assoc_fn_registered_in_module_function_table() {
    let module = compile_source(CYLINDER_SRC);

    // The registration-pass symbol mirrors `instance_assoc_fn_symbol(conformer,
    // trait, method)` == `"{conformer}::{trait}::{method}"`.
    let symbol = "Pin::Cylindrical::lateral_area";
    let func = module
        .functions
        .iter()
        .find(|f| f.name == symbol)
        .unwrap_or_else(|| {
            panic!(
                "module function table should contain the registered instance assoc fn \
                 '{symbol}'; registered functions: {:?}",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        });

    // (a) First param is the bound `self` receiver, typed StructureRef("Pin").
    let (self_name, self_ty) = func
        .params
        .first()
        .expect("registered instance assoc fn should carry a leading `self` receiver param");
    assert_eq!(
        self_name, "self",
        "the registered fn's first param should be the `self` receiver"
    );
    assert_eq!(
        *self_ty,
        Type::StructureRef("Pin".to_string()),
        "the `self` receiver should be typed StructureRef(\"Pin\")"
    );

    // (b) Return type is the trait's declared Scalar<Area>.
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: DimensionVector::AREA
        },
        "registered instance assoc fn should return the trait's declared Scalar<Area>"
    );
}

/// (step-5b) End-to-end: with the instance assoc fn registered, the consuming
/// `pin.(Cylindrical::lateral_area)()` call must resolve at eval and produce a
/// concrete `Value::Scalar` (the `pi * diameter * length` area), NOT `Undef`.
///
/// RED until step-6: the dispatch site (step-2) lowers `wetted` to a
/// `UserFunctionCall` of the `Pin::Cylindrical::lateral_area` symbol, but with
/// that symbol absent from `CompiledModule.functions`,
/// `find_matching_compiled_function` returns `None` and the call evaluates to
/// `Value::Undef`.
#[test]
fn instance_dispatch_evaluates_to_scalar_not_undef() {
    let compiled = parse_and_compile_with_stdlib(CYLINDER_SRC);
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    let wetted_id = ValueCellId::new("Assembly", "wetted");
    match eval_result.values.get(&wetted_id) {
        Some(Value::Scalar { .. }) => {
            // GREEN once step-6 registers the per-conformer instance assoc fn.
        }
        other => panic!(
            "Assembly.wetted should evaluate to a Value::Scalar once the per-conformer \
             instance assoc fn is registered into the function table (step-6); got {other:?}"
        ),
    }
}

// ─── Reviewer amendments (robustness / correctness) ──────────────────────────

/// (reviewer amendment 1 — robustness) A receiver whose conformer does NOT
/// declare `: Trait` must be rejected, not silently lowered to an unregistered
/// per-conformer symbol that evaluates to `Value::Undef`.
///
/// Here `Plain` conforms to no trait, yet `Cylindrical::lateral_area` exists
/// globally (via `Pin`). The dispatch-site return-type lookup is keyed by trait
/// name across the whole registry, so it succeeds for `Cylindrical::lateral_area`
/// regardless of the receiver — without a conformance gate the call would lower
/// to the symbol `Plain::Cylindrical::lateral_area`, which the registration pass
/// never emits, and silently evaluate to `Value::Undef`. The conformance gate
/// must instead emit exactly one `DiagnosticCode::TraitNotImplemented`.
#[test]
fn instance_dispatch_non_conformer_receiver_emits_trait_not_implemented() {
    let source = r#"
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * length }
}

structure def Pin : Cylindrical {
    param diameter : Length = 8mm
    param length : Length = 40mm
}

structure def Plain {
    param diameter : Length = 8mm
    param length : Length = 40mm
}

structure def Assembly {
    sub plain : Plain
    let bad = plain.(Cylindrical::lateral_area)()
}
"#;
    let module = compile_source(source);

    let not_impl: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitNotImplemented))
        .collect();
    assert_eq!(
        not_impl.len(),
        1,
        "plain.(Cylindrical::lateral_area)() on a non-conformer must emit exactly one \
         TraitNotImplemented (not silently lower to an unregistered symbol); \
         all diagnostics: {:?}",
        module.diagnostics
    );
}

/// (reviewer amendment 2 — robustness) Instance dispatch over a trait-object
/// receiver is unsupported: ζ is static, compile-time, per-conformer dispatch.
///
/// A trait-typed `param holder : Cylindrical` resolves to `Type::TraitObject`,
/// whose erased trait name is not a concrete conformer — the per-conformer symbol
/// could never resolve, so the call would silently evaluate to `Value::Undef`.
/// The dispatch arm must reject it with a clear `trait-object receiver` error
/// rather than emit an unresolvable symbol.
#[test]
fn instance_dispatch_trait_object_receiver_is_rejected() {
    let source = r#"
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * length }
}

structure def Assembly {
    param holder : Cylindrical
    let bad = holder.(Cylindrical::lateral_area)()
}
"#;
    let module = compile_source(source);

    let rejected: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.message.contains("trait-object receiver")
        })
        .collect();
    assert_eq!(
        rejected.len(),
        1,
        "dispatch on a trait-object receiver must emit exactly one 'trait-object receiver' \
         error (not lower to an unresolvable symbol); all diagnostics: {:?}",
        module.diagnostics
    );
}

/// (reviewer amendment 3 — correctness) A default-providing instance fn whose
/// declared return type references a TRAIT-level type-parameter must resolve to a
/// `Type::TypeParam` at the dispatch site, NOT collapse to `Type::Error`.
///
/// Before the fix, the dispatch-site return-type population re-resolved the raw
/// `FnDef.return_type` using only the fn's OWN type-params, so a trait-generic
/// return type (`-> T` where `T` is declared on the trait) hit
/// `unwrap_or(Type::Error)` and silently mistyped the call site. Threading the
/// trait's type-params into the resolution keeps it `Type::TypeParam("T")`.
#[test]
fn instance_dispatch_trait_generic_return_type_is_not_error() {
    let source = r#"
trait Rigid { param mass : Mass }

structure def Bolt : Rigid { param mass : Mass = 5kg }

trait Boxed<T: Rigid> {
    param content : T
    fn unwrap(self) -> T { content }
}

structure def BoltBox : Boxed<Bolt> {
    param content : Bolt
}

structure def Assembly {
    sub bx : BoltBox
    let got = bx.(Boxed::unwrap)()
}
"#;
    let module = compile_source(source);

    let assembly = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("compiled module should contain a template for 'Assembly'");
    let got = assembly
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "got")
        .expect("Assembly should have a 'got' value cell");
    let expr = got
        .default_expr
        .as_ref()
        .expect("the 'got' let-binding should carry a compiled value expr");

    assert_ne!(
        expr.result_type,
        Type::Error,
        "a trait-generic `-> T` default-fn return type must not collapse to Type::Error \
         at the dispatch site; got result_type {:?}",
        expr.result_type
    );
    assert_eq!(
        expr.result_type,
        Type::TypeParam("T".to_string()),
        "the trait type-param return type should resolve to Type::TypeParam(\"T\")"
    );
}
