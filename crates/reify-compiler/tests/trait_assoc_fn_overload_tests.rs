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
//!
//! ## Step-7 (eval) — end-to-end eval distinguishes overloaded bodies
//!
//! Uses `parse_and_compile_with_stdlib + make_simple_engine` to assert that
//! each dispatched call selects the CORRECT body at runtime (distinct `Value`
//! results), proving the full pipeline (merge → conformance → registration →
//! dispatch resolution → eval find_matching) round-trips correctly.

// `EvalResult.values` is keyed by `ValueCellId`; the mutable_key_type lint
// would fire without this allow.
#![allow(clippy::mutable_key_type)]

use reify_core::{DiagnosticCode, DimensionVector, Severity, Type, ValueCellId};
use reify_ir::{CompiledExprKind, Value};
use reify_test_support::{compile_source, errors_only, make_simple_engine,
    parse_and_compile_with_stdlib};

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

// ── (step-5) Ambiguous instance call emits E_AMBIGUOUS_CALL ──────────────────

/// Trait `T` declares two default overloads whose non-self params are BOTH
/// trait-typed (wildcards in overload resolution):
///   `fn f(self, s: Shaper) -> Real { 1.0 }` and
///   `fn f(self, s: Scaler) -> Real { 2.0 }`
/// where `Shaper` and `Scaler` are traits.
///
/// A call `c.(T::f)(5mm)` uses a Length argument.  Both overload params are
/// trait-objects (`type_carries_trait_object` is true for both), so BOTH match
/// via the wildcard relaxation.  Neither is an exact match (Length ≠
/// TraitObject("Shaper") and Length ≠ TraitObject("Scaler")), so the exact
/// tiebreak leaves two candidates → AMBIGUOUS.
///
/// Asserts:
///   (1) exactly one error with `code == Some(DiagnosticCode::AmbiguousCall)`,
///   (2) the error message mentions "ambiguous",
///   (3) the consuming let lowers to a poison literal (`result_type == Type::Error`,
///       anti-cascade — no follow-on errors beyond the AmbiguousCall).
///
/// RED until step-6: today the dispatch arm falls through to
/// `sigs[0].return_type` for non-single-match counts and never emits
/// `AmbiguousCall`.
#[test]
fn dispatch_ambiguous_trait_object_params_emits_ambiguous_call() {
    let source = r#"
trait Shaper {}
trait Scaler {}

trait T {
    fn f(self, s: Shaper) -> Real { 1.0 }
    fn f(self, s: Scaler) -> Real { 2.0 }
}

structure def C : T {}

structure def Assembly {
    sub c : C
    let bad = c.(T::f)(5mm)
}
"#;
    let module = compile_source(source);

    // (1) Exactly one AmbiguousCall error.
    let ambig: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AmbiguousCall))
        .collect();
    assert_eq!(
        ambig.len(),
        1,
        "an ambiguous trait-object overload call must emit exactly one \
         AmbiguousCall diagnostic; all diagnostics: {:?}",
        module.diagnostics
    );

    // (2) The error message mentions "ambiguous".
    assert!(
        ambig[0].message.to_lowercase().contains("ambiguous"),
        "AmbiguousCall message must contain 'ambiguous'; got: {}",
        ambig[0].message
    );

    // (3) Consuming let lowers to a poison literal (result_type == Type::Error).
    let assembly = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("compiled module must contain an Assembly template");
    let bad_cell = assembly
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "bad")
        .expect("Assembly must have a let binding 'bad'");
    let bad_expr = bad_cell
        .default_expr
        .as_ref()
        .expect("'bad' must have a compiled default expr");
    assert_eq!(
        bad_expr.result_type,
        Type::Error,
        "an ambiguous dispatch must poison the let cell (result_type == Type::Error); \
         got: {:?}",
        bad_expr.result_type
    );

    // Anti-cascade: exactly the one AmbiguousCall, no follow-on errors.
    let all_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        all_errors.len(),
        1,
        "only the AmbiguousCall error should fire (anti-cascade poison); \
         all errors: {:?}",
        all_errors
    );
}

// ── (step-7i) Eval — intra-trait overload: distinct bodies produce distinct values ──

/// Full pipeline eval: trait `T` with two default-providing overloads of `f`
/// whose bodies return DISTINCT values (`1.0` for the Length overload, `2.0`
/// for the Angle overload).  Conformer `C : T`; `Assembly` binds
/// `let a = c.(T::f)(5mm)` and `let b = c.(T::f)(30deg)`.
///
/// Asserts:
///   (1) compile + eval clean (no errors),
///   (2) `a` evaluates to `1.0` (Length body), `b` to `2.0` (Angle body),
///   (3) `a != b` — distinct bodies are selected, not a single collapsed one.
///
/// Proves the full pipeline (merge survival → per-overload CompiledAssocFn →
/// registration → dispatch resolution → eval find_matching) round-trips
/// correctly for intra-trait overloads.
#[test]
fn eval_intra_trait_overload_distinct_bodies() {
    let source = r#"
trait T {
    fn f(self, x: Length) -> Real { 1.0 }
    fn f(self, x: Angle)  -> Real { 2.0 }
}

structure def C : T {}

structure def Assembly {
    sub c : C
    let a = c.(T::f)(5mm)
    let b = c.(T::f)(30deg)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    let a_id = ValueCellId::new("Assembly", "a");
    let b_id = ValueCellId::new("Assembly", "b");

    let a_val = eval_result
        .values
        .get(&a_id)
        .expect("Assembly.a must have an evaluated value");
    let b_val = eval_result
        .values
        .get(&b_id)
        .expect("Assembly.b must have an evaluated value");

    // (2) Check that each overload's body was called.
    assert_eq!(
        *a_val,
        Value::Real(1.0),
        "c.(T::f)(5mm) should evaluate to 1.0 (Length body); got: {:?}",
        a_val
    );
    assert_eq!(
        *b_val,
        Value::Real(2.0),
        "c.(T::f)(30deg) should evaluate to 2.0 (Angle body); got: {:?}",
        b_val
    );

    // (3) Distinct bodies were selected.
    assert_ne!(
        a_val, b_val,
        "the two overload results must differ — each dispatched to its own body; \
         a={:?}, b={:?}",
        a_val, b_val
    );
}

// ── (step-7i) Eval — two-trait same-name: distinct bodies produce distinct values ──

/// Full pipeline eval: two traits `Spinning` and `Sliding` each declare a
/// default-providing `fn rate(self) -> Real` with DISTINCT bodies (`1.0` vs
/// `2.0`).  Conformer `C : Spinning + Sliding`; `Assembly` binds
/// `let sr = c.(Spinning::rate)()` and `let lr = c.(Sliding::rate)()`.
///
/// Asserts:
///   (1) compile + eval clean (no errors),
///   (2) `sr` evaluates to `1.0` (Spinning body), `lr` to `2.0` (Sliding body),
///   (3) `sr != lr` — each trait's body was called, not a collapsed single one.
///
/// Proves the two-trait same-name disambiguation pipeline (distinct trait
/// keys → distinct per-conformer symbols → distinct registered functions →
/// eval find_matching selects by trait name via symbol) round-trips correctly.
#[test]
fn eval_two_trait_same_name_distinct_bodies() {
    let source = r#"
trait Spinning {
    fn rate(self) -> Real { 1.0 }
}

trait Sliding {
    fn rate(self) -> Real { 2.0 }
}

structure def C : Spinning + Sliding {}

structure def Assembly {
    sub c : C
    let sr = c.(Spinning::rate)()
    let lr = c.(Sliding::rate)()
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    let sr_id = ValueCellId::new("Assembly", "sr");
    let lr_id = ValueCellId::new("Assembly", "lr");

    let sr_val = eval_result
        .values
        .get(&sr_id)
        .expect("Assembly.sr must have an evaluated value");
    let lr_val = eval_result
        .values
        .get(&lr_id)
        .expect("Assembly.lr must have an evaluated value");

    // (2) Each trait's body was invoked.
    assert_eq!(
        *sr_val,
        Value::Real(1.0),
        "c.(Spinning::rate)() should evaluate to 1.0 (Spinning body); got: {:?}",
        sr_val
    );
    assert_eq!(
        *lr_val,
        Value::Real(2.0),
        "c.(Sliding::rate)() should evaluate to 2.0 (Sliding body); got: {:?}",
        lr_val
    );

    // (3) Distinct bodies were selected.
    assert_ne!(
        sr_val, lr_val,
        "the two-trait same-name results must differ — each dispatched to its own \
         body; sr={:?}, lr={:?}",
        sr_val, lr_val
    );
}

// ── (amendment §2) Anti-cascade: erroneous arg must not emit secondary diagnostic ──

/// When an argument expression fails to compile (produces `Type::Error` upstream),
/// the overload resolution short-circuits to a `propagate_poison()` return without
/// emitting an additional "no overload matches" diagnostic.
///
/// The upstream arg error is sufficient; a second dispatch-site error would be
/// redundant and confusing. (ε #3943 reviewer amendment §2)
///
/// This test verifies that a broken arg in a multi-overload trait call produces
/// exactly ONE error (the symbol-not-found error for `nonexistentSymbol`), NOT a
/// secondary `TraitMethodUnknown` or `AmbiguousCall` diagnostic from overload
/// resolution failing to find a match for `Type::Error` args.
#[test]
fn dispatch_erroneous_arg_no_secondary_diagnostic() {
    let source = r#"
trait T {
    fn f(self, x: Length) -> Real { 1.0 }
    fn f(self, x: Angle)  -> Real { 2.0 }
}

structure def C : T {}

structure def Assembly {
    sub c : C
    let bad = c.(T::f)(nonexistentSymbol)
}
"#;
    let module = compile_source(source);

    let all_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // There should be at least one error (the unknown symbol).
    assert!(
        !all_errors.is_empty(),
        "an unknown symbol arg should produce at least one error"
    );

    // But NO secondary error from the dispatch site (TraitMethodUnknown or
    // AmbiguousCall). The anti-cascade short-circuit in the `Some(sigs)` arm
    // must fire when it sees Type::Error args.
    let dispatch_site_errors: Vec<_> = all_errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TraitMethodUnknown)
                || d.code == Some(DiagnosticCode::AmbiguousCall)
        })
        .collect();
    assert!(
        dispatch_site_errors.is_empty(),
        "a Type::Error arg must short-circuit overload resolution without emitting \
         a secondary dispatch diagnostic; all errors: {:?}",
        all_errors
    );
}

// ── (amendment §3) Override-name-keying: documented scope boundary ──

/// Pins the observable consequence of the name-only override lookup in
/// `check_phase_resolve_assoc_fns` (conformance/checker.rs): with two default-
/// providing overloads of the same name (`fn f(self, x: Length)` and
/// `fn f(self, x: Angle)`) and NO structure override (the only case the reify
/// grammar currently admits — `fn` declarations inside `structure def` bodies
/// are NOT supported by the tree-sitter grammar), BOTH overloads survive
/// as `is_override = false` (default-injection path).
///
/// NOTE ON SCOPE (ε #3943 design decision §4): the conformance checker's
/// `find_structure_assoc_fn` returns the first structure member named `fn_name`
/// regardless of params (name-only keying).  If multiple overloads exist and the
/// structure provides a body, the same override body would be found for EVERY
/// iteration; only the iteration whose default sig matches would pass the override
/// sig-lock, and the others would emit a spurious `TraitFnSignatureMismatch`.
/// Keying by (name, params) is a follow-up task.  Structure-level fn override
/// syntax is currently unsupported by the grammar, so the name-only path is
/// effectively dead code and cannot be exercised from parsed source today.
#[test]
fn default_injection_both_overloads_are_not_override() {
    let source = r#"
trait T {
    fn f(self, x: Length) -> Real { 1.0 }
    fn f(self, x: Angle)  -> Real { 2.0 }
}

structure def C : T {}
"#;
    let module = compile_source(source);

    // Conformance must be clean (no errors): both overloads are defaults, no
    // override sig-lock fires.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "default-only conformance for two overloads should compile cleanly; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "C")
        .expect("compiled module should contain a template for structure 'C'");

    let f_entries: Vec<_> = template
        .assoc_fns
        .iter()
        .filter(|e| e.trait_name == "T" && e.fn_name == "f")
        .collect();

    assert_eq!(
        f_entries.len(),
        2,
        "both T::f overloads must survive to the dispatch table; assoc_fns: {:?}",
        template.assoc_fns
    );

    // Both entries are default-injected (not override): the grammar does not
    // currently allow fn declarations inside structure def bodies, so
    // find_structure_assoc_fn always returns None → is_override is always false.
    for entry in &f_entries {
        assert!(
            !entry.is_override,
            "T::f entry should be is_override=false (default injection — \
             structure fn override syntax not yet supported by the grammar); \
             entry: {:?}",
            entry
        );
    }
}
