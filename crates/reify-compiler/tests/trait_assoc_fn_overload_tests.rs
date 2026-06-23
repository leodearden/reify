//! Integration tests for task ε #3943 (trait assoc-fn overload by param type +
//! multi-trait same-name disambiguation).
//!
//! These tests drive the full compile pipeline via
//! `reify_test_support::compile_source` and inspect
//! `TopologyTemplate.assoc_fns` for lossless overload survival.
//!
//! ## Step-1 (RED) — assoc-fn overload SURVIVAL through merge+conformance
//!
//! Tests (a) and (b) fail RED until step-2 makes the default-assoc-fn
//! accumulation lossless (currently `seen_fn_default_traits` is first-seen-wins
//! by name, so the second overload/sibling is silently dropped).
//!
//! Test (c) is GREEN from the start and guards the sig-lock re-key:
//! the return-type-change conflict still fires EXACTLY ONE
//! `TraitFnSignatureMismatch`, and the identical-sig case still dedups to one
//! entry.
//!
//! ## Step-3 (RED) — dispatch-site overload resolution + per-overload return typing
//!
//! Fails RED until step-4 replaces `scope.trait_assoc_fn_return_types` (one
//! Type per (trait, method)) with an overload-aware map and resolves the
//! correct overload at the call site.

use reify_core::{DiagnosticCode, DimensionVector, Type};
use reify_ir::{CompiledExprKind};
use reify_test_support::{compile_source, errors_only};

// ── (a) Intra-trait overload survival ────────────────────────────────────────

/// ONE trait `T` declaring TWO default-providing overloads of `f`:
///   `fn f(self, x: Length) -> Real { ... }` and
///   `fn f(self, x: Angle)  -> Real { ... }`.
/// Conformer `C : T` must end up with EXACTLY TWO entries in
/// `template.assoc_fns` both keyed to `(trait_name="T", fn_name="f")` but with
/// different non-self parameter types.
///
/// RED until step-2: `seen_fn_default_traits` collapses by name → only the
/// first-seen Length overload survives; the Angle overload is dropped.
#[test]
fn intra_trait_overload_both_survive_in_assoc_fns() {
    let source = r#"
trait T {
    fn f(self, x: Length) -> Real { 1.0 }
    fn f(self, x: Angle)  -> Real { 2.0 }
}

structure def C : T {
}
"#;
    let module = compile_source(source);

    // Conformance must be clean (no missing-fn or sig-mismatch errors).
    let errors = errors_only(&module);
    let unexpected_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TraitFnNotSatisfied)
                || d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)
        })
        .collect();
    assert!(
        unexpected_errors.is_empty(),
        "intra-trait default overloads should conform cleanly; got: {:?}",
        unexpected_errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "C")
        .expect("compiled module must contain a template for structure 'C'");

    let f_entries: Vec<_> = template
        .assoc_fns
        .iter()
        .filter(|e| e.trait_name == "T" && e.fn_name == "f")
        .collect();

    assert_eq!(
        f_entries.len(),
        2,
        "structure 'C' should carry TWO T::f entries (one per overload, Length \
         and Angle); assoc_fns = {:?}",
        template.assoc_fns
    );

    // The two entries must differ in their non-self parameter type.
    // params[0] is the self receiver; params[1] is the x: Length / x: Angle.
    let param1_types: Vec<_> = f_entries
        .iter()
        .map(|e| e.function.params.get(1).map(|p| format!("{:?}", p.1)))
        .collect();
    assert_ne!(
        param1_types[0],
        param1_types[1],
        "the two T::f overload entries must differ in their non-self param type; \
         got: {:?}",
        param1_types
    );
}

// ── (b) Two-trait same-name survival ─────────────────────────────────────────

/// TWO traits `Spinning` and `Sliding` each declaring a default-providing
/// `fn f(self) -> Real` with DISTINCT bodies (1.0 vs 2.0).  Conformer
/// `C : Spinning, Sliding` must carry ONE `(TraitName="Spinning", fn_name="f")`
/// entry AND ONE `(TraitName="Sliding", fn_name="f")` entry in `assoc_fns`.
///
/// RED until step-2: `seen_fn_default_traits` is first-seen-wins by name, so
/// only `Spinning::f` survives; `Sliding::f` is dropped.
#[test]
fn two_trait_same_name_both_survive_in_assoc_fns() {
    let source = r#"
trait Spinning {
    fn f(self) -> Real { 1.0 }
}

trait Sliding {
    fn f(self) -> Real { 2.0 }
}

structure def C : Spinning + Sliding {
}
"#;
    let module = compile_source(source);

    // Conformance must be clean.
    let errors = errors_only(&module);
    let unexpected_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TraitFnNotSatisfied)
                || d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)
        })
        .collect();
    assert!(
        unexpected_errors.is_empty(),
        "two-trait same-name default should conform cleanly; got: {:?}",
        unexpected_errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "C")
        .expect("compiled module must contain a template for structure 'C'");

    // Both trait entries must be present.
    let spinning_f: Vec<_> = template
        .assoc_fns
        .iter()
        .filter(|e| e.trait_name == "Spinning" && e.fn_name == "f")
        .collect();
    let sliding_f: Vec<_> = template
        .assoc_fns
        .iter()
        .filter(|e| e.trait_name == "Sliding" && e.fn_name == "f")
        .collect();

    assert_eq!(
        spinning_f.len(),
        1,
        "structure 'C' must carry exactly one Spinning::f entry; assoc_fns = {:?}",
        template.assoc_fns
    );
    assert_eq!(
        sliding_f.len(),
        1,
        "structure 'C' must carry exactly one Sliding::f entry; assoc_fns = {:?}",
        template.assoc_fns
    );
}

// ── (c) Sig-lock regression pins ─────────────────────────────────────────────

/// Regression pin — GREEN from step-1 and must stay GREEN after step-2 re-keys
/// the sig-lock from (name) to (name, params).
///
/// `Base` declares required `fn f(self) -> Real` (no body); `Derived : Base`
/// re-declares required `fn f(self) -> Length` — a RETURN-TYPE change with
/// IDENTICAL non-self params.  Conforming to `Derived` must emit EXACTLY ONE
/// `TraitFnSignatureMismatch` (the same (name, params) key → conflict).
#[test]
fn sig_lock_return_type_change_still_conflicts_after_rekeying() {
    let source = r#"
trait Base {
    fn f(self) -> Real
}
trait Derived : Base {
    fn f(self) -> Length
}
structure def C : Derived {
}
"#;
    let module = compile_source(source);

    let mismatch: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
        .collect();
    assert_eq!(
        mismatch.len(),
        1,
        "a return-type change with identical params (same (name,params) key) \
         must still emit exactly one TraitFnSignatureMismatch; diagnostics: {:?}",
        module.diagnostics
    );
    assert!(
        mismatch[0].message.contains("f"),
        "the conflict diagnostic should name the fn 'f'; got: {}",
        mismatch[0].message
    );
}

/// Regression pin — GREEN from step-1 and must stay GREEN after step-2.
///
/// `Base` and `Derived : Base` both declare the IDENTICAL required signature
/// `fn f(self) -> Real` (no body).  This deduplicates to a single requirement
/// (no conflict), so ZERO `TraitFnSignatureMismatch` errors fire, and the
/// conformer `C` needs exactly one fn body to satisfy the single requirement.
#[test]
fn sig_lock_identical_sig_still_dedups_after_rekeying() {
    let source = r#"
trait Base {
    fn f(self) -> Real
}
trait Derived : Base {
    fn f(self) -> Real
}
structure def C : Derived {
}
"#;
    let module = compile_source(source);

    let mismatch: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
        .collect();
    assert!(
        mismatch.is_empty(),
        "identical sig in a refinement chain should dedup silently — \
         zero TraitFnSignatureMismatch; diagnostics: {:?}",
        module.diagnostics
    );

    // The conformer template deduplicates to one required-fn entry, so there is
    // at most ONE TraitFnNotSatisfied (not two), but that assertion is outside the
    // scope of this regression pin — we only guard the signature-lock behaviour.
}

// ── (step-3) Dispatch-site overload resolution + per-overload return typing ──

/// Trait `T` declares TWO default overloads with DISTINCT return types:
///   `fn f(self, x: Length) -> Scalar<Area> { x * x }` and
///   `fn f(self, x: Angle)  -> Real { 1.0 }`.
/// Conformer `C : T`; `Assembly` binds:
///   `let a = c.(T::f)(5mm)` and `let b = c.(T::f)(30deg)`.
///
/// Asserts:
///   (1) both lower to `UserFunctionCall` (not poison),
///   (2) `a.result_type == Scalar<Area>` (Length overload),
///   (3) `b.result_type == Real` (Angle overload — per-overload typing, NOT
///       last-write-wins from a single return-type map),
///   (4) no error diagnostics.
///
/// RED until step-4: `scope.trait_assoc_fn_return_types` maps one `Type` per
/// `(trait, method)`, so both calls receive the same last-written return type
/// instead of being resolved individually.
#[test]
fn dispatch_resolves_overload_and_types_return_from_resolved_sig() {
    let source = r#"
trait T {
    fn f(self, x: Length) -> Scalar<Area> { x * x }
    fn f(self, x: Angle)  -> Real { 1.0 }
}

structure def C : T {
}

structure def Assembly {
    sub c : C
    let a = c.(T::f)(5mm)
    let b = c.(T::f)(30deg)
}
"#;
    let module = compile_source(source);

    // (4) No error diagnostics.
    let err = errors_only(&module);
    assert!(
        err.is_empty(),
        "dispatch of two distinct overloads should compile cleanly; got: {:?}",
        err
    );

    let assembly = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("compiled module should contain an Assembly template");

    let a_cell = assembly
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "a")
        .expect("Assembly should have a let binding 'a'");
    let b_cell = assembly
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "b")
        .expect("Assembly should have a let binding 'b'");

    let a_expr = a_cell
        .default_expr
        .as_ref()
        .expect("'a' should have a compiled default expr");
    let b_expr = b_cell
        .default_expr
        .as_ref()
        .expect("'b' should have a compiled default expr");

    // (1) Both must lower to UserFunctionCall, not a poison literal.
    assert!(
        matches!(a_expr.kind, CompiledExprKind::UserFunctionCall { .. }),
        "a = c.(T::f)(5mm) should lower to UserFunctionCall; got: {:?}",
        a_expr.kind
    );
    assert!(
        matches!(b_expr.kind, CompiledExprKind::UserFunctionCall { .. }),
        "b = c.(T::f)(30deg) should lower to UserFunctionCall; got: {:?}",
        b_expr.kind
    );

    // (2) a typed as Scalar<Area> — from the `fn f(self, x: Length) -> Scalar<Area>` overload.
    assert_eq!(
        a_expr.result_type,
        Type::Scalar {
            dimension: DimensionVector::AREA
        },
        "a should be typed as Scalar<Area> (Length overload); got: {:?}",
        a_expr.result_type
    );

    // (3) b typed as Real (dimensionless scalar) — from the `fn f(self, x: Angle) -> Real` overload.
    // Proves the dispatch arm resolves each call individually, NOT last-write-wins.
    assert_eq!(
        b_expr.result_type,
        Type::dimensionless_scalar(),
        "b should be typed as Real/dimensionless (Angle overload); got: {:?}",
        b_expr.result_type
    );
}
