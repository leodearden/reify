//! L5 ds-sentinel forward behavioral matrix (task #4650).
//!
//! ## Purpose
//!
//! Integration-gate / H-component of the dimensionless-scalar-sentinel-stampout
//! batch (PRD `docs/prds/dimensionless-scalar-sentinel-stampout.md` §8/§9).
//! Asserts the USER-OBSERVABLE diagnostic headline across the SURFACE-REACHABLE
//! cells:
//!
//! > **Exactly one root-cause UnresolvedType-class error** AND
//! > **no secondary ParamDefaultTypeMismatch / conformance "type mismatch for
//! > trait member" / codomain cascade.**
//!
//! This is a DIFFERENT assertion altitude from the per-leaf `.is_error()`
//! producer-cell probes in `ds_sentinel_l0_poison_tests.rs` /
//! `ds_sentinel_l1_poison_tests.rs` / `wrong_receiver_member_tests.rs`.
//! Those tests pin the internal cell-type state; this matrix pins the
//! operator-observable `reify check` diagnostic stream.
//!
//! ## Coverage: surface-reachable cells
//!
//! | Cell | Fixture | Asserted by |
//! |------|---------|-------------|
//! | (a) structure param | `structure W { param p : Bogus = 5kg }` | 1 error, UnresolvedType, no ParamDefaultTypeMismatch |
//! | (b) port-param | port MockPort scenario | 1 error, UnresolvedType |
//! | (c) fn-return | `fn f() -> Bogus { 0 }` | UnresolvedType, no cascade |
//! | (d) fn-param | `fn g(x : Bogus) -> Real { 0.0 }` | UnresolvedType |
//! | (e) trait-member unknown-name | `trait T { param x : Bogus }` | UnresolvedType |
//! | (f) trait-member missing-annotation (L2) | `structure def W : T { param x }` | 1 error "no type annotation", no "type mismatch for trait member" |
//! | (g) type-arg integer literal | `Foo<5>()`, `List<5>` | Type::Error on sub/type-arg |
//! | (h) L4 method-receiver trio | `(5kg).sum`, `.keys`, `.values` | AggregationReceiverNotCollection + 1 error |
//! | (h2) struct-ref missing member | `w.nonexistent` | StructureMemberNotFound + 1 error |
//!
//! ## NON-asserted cells (documented here, NOT via compile_source)
//!
//! - **Field domain/codomain structurally-invalid** (e.g. `field f : Int -> 5kg`):
//!   these are PARSE-ERRORs per the L1 test header and a fresh 2026-06-17 probe
//!   (the `5kg` in type-expression position is rejected by the parser before
//!   entity/functions.rs even runs). Covered by the type-arg substitution cell (g)
//!   and the L1 in-crate `#[cfg(test)]` tests. Asserting them via `compile_source`
//!   would be a doomed RED (parse error → empty module → no UnresolvedType).
//!
//! - **Assoc-fn unresolved-type sites**: the `pub(crate)` functions.rs path is
//!   only reachable from inside the crate. Covered by functions.rs in-crate tests.
//!
//! - **Arrow field-domain/codomain cells** (esc-4646-36, resolved by #4657):
//!   the Function/arrow arms in compile_field now return `Type::Error` (poison),
//!   so the secondary `FieldCodomainMismatch` cascade no longer fires. Behavioral
//!   coverage lives in `ds_sentinel_l1_poison_tests.rs` —
//!   `field_arrow_{codomain,domain}_resolves_to_error_no_cascade`. No active test
//!   cell here (L1 in-crate tests cover the producer; L5 pins the surface stream).
//!
//! ## Status
//!
//! All cells are **GREEN-on-landing** — their "implementation" is the union of
//! the already-landed dep fixes (#4645/4646/4647/4648/4649). This file is a
//! regression guard: a future reintroduction of a `dimensionless_scalar()` fallback
//! at any surface-reachable producer site will flip the corresponding cell RED.

use reify_compiler::{find_template, CompiledModule};
use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::{
    assert_no_type_cascade, collect_errors, compile_source, errors_only, get_let_expr,
    get_let_expr_in,
};

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers (mirrors wrong_receiver_member_tests.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// True iff some `Severity::Error` diagnostic in `m` carries the given code.
fn has_error_code(m: &CompiledModule, code: DiagnosticCode) -> bool {
    m.diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

/// Count of `Severity::Error` diagnostics in `m`.
fn error_count(m: &CompiledModule) -> usize {
    m.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

// ─────────────────────────────────────────────────────────────────────────────
// (a) Structure param — unknown name
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (a): `structure W { param p : Bogus = 5kg }` — an unknown type name in
/// a structure-param annotation must produce exactly one error (the root-cause
/// `UnresolvedType`) with NO secondary `ParamDefaultTypeMismatch`.
///
/// Without the L0 fix (#4645), entity.rs fell back to `Type::dimensionless_scalar()`
/// after pushing the `UnresolvedType` diagnostic, and the `check_param_default_type`
/// guard (which fires on `declared.is_error()`) did not fire — leaving the `5kg`
/// default-vs-`Real` mismatch as a second error.
#[test]
fn structure_param_unknown_name_headline_one_error_no_cascade() {
    let module = compile_source("structure W { param p : Bogus = 5kg }");
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for `structure W {{ param p : Bogus = 5kg }}`; got: {:?}",
        errors
    );
    assert!(
        has_error_code(&module, DiagnosticCode::UnresolvedType),
        "the single error must carry code UnresolvedType; got: {:?}",
        errors
    );
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch)),
        "no ParamDefaultTypeMismatch must be emitted (anti-cascade); diagnostics: {:?}",
        module.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (b) Port-param — unknown name
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (b): port-param position with an unknown type name must produce exactly
/// one error (the root-cause `UnresolvedType`) with NO secondary
/// `ParamDefaultTypeMismatch`.
///
/// Without the L0 fix (#4645), entity.rs:1282 fell back to
/// `Type::dimensionless_scalar()`, which leaked a silent `Real` into
/// `check_param_default_type`'s `cell_type` readback — the anti-cascade guard
/// (`declared.is_error()`) did not fire, and a spurious mismatch was emitted.
#[test]
fn port_param_unknown_name_headline_one_error_no_cascade() {
    let source = r#"
structure S {
    port p : out MockPort {
        param x : Bogus = 5kg
    }
}
trait MockPort {
    param x : Real
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for port-param `x : Bogus = 5kg`; got: {:?}",
        errors
    );
    assert!(
        has_error_code(&module, DiagnosticCode::UnresolvedType),
        "the single error must carry code UnresolvedType; got: {:?}",
        errors
    );
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch)),
        "no ParamDefaultTypeMismatch must be emitted (anti-cascade); diagnostics: {:?}",
        module.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (c) Function return type — unknown name
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (c): `fn f() -> Bogus { 0 }` — an unresolved return-type name must
/// produce the root-cause `UnresolvedType` error with no secondary cascade.
///
/// The end-to-end headline is verified by combining the root-cause probe with a
/// use site: `f() + 5mm` would, if `f()` leaked a silent `Real`, spawn a second
/// "dimension mismatch" error. With `Type::Error` poison the BinOp short-circuits.
#[test]
fn fn_return_unknown_name_headline_no_cascade() {
    // Root-cause probe.
    let module = compile_source("module m\nfn f() -> Bogus { 0 }");
    assert!(
        has_error_code(&module, DiagnosticCode::UnresolvedType),
        "expected an UnresolvedType error for `fn f() -> Bogus {{ 0 }}`; diagnostics: {:?}",
        module.diagnostics
    );

    // End-to-end anti-cascade: use site must not spawn a dimension-mismatch.
    let module2 =
        compile_source("fn f() -> Bogus { 0 }\nstructure S { let broken = f() + 5mm }");
    let broken = get_let_expr_in(&module2, "S", "broken");
    assert_eq!(
        broken.result_type,
        Type::Error,
        "`f() + 5mm` must short-circuit to Type::Error (anti-cascade), got: {:?}",
        broken.result_type
    );
    let errors2 = collect_errors(&module2.diagnostics);
    assert_eq!(
        errors2.len(),
        1,
        "expected exactly 1 error (root cause; no dimension-mismatch cascade), got: {:?}",
        errors2
    );
    assert_no_type_cascade(&module2.diagnostics, &["unresolved return type", "Bogus"]);
}

// ─────────────────────────────────────────────────────────────────────────────
// (d) Function parameter type — unknown name
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (d): `fn g(x : Bogus) -> Real { 0.0 }` — an unresolved param-type name
/// must produce the root-cause `UnresolvedType` with no cascade.
#[test]
fn fn_param_unknown_name_headline_one_error() {
    let module = compile_source("module m\nfn g(x : Bogus) -> Real { 0.0 }");
    assert!(
        has_error_code(&module, DiagnosticCode::UnresolvedType),
        "expected an UnresolvedType error for `fn g(x : Bogus) -> Real {{ 0.0 }}`; \
         diagnostics: {:?}",
        module.diagnostics
    );
    // No cascade: only errors carrying UnresolvedType are expected.
    let errors = errors_only(&module);
    let non_unresolved: Vec<_> = errors
        .iter()
        .filter(|d| d.code != Some(DiagnosticCode::UnresolvedType))
        .collect();
    assert!(
        non_unresolved.is_empty(),
        "expected no secondary errors beyond UnresolvedType; got: {:?}",
        non_unresolved
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (e) Trait member — unknown name
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (e): `trait T { param x : Bogus }` — an unresolved trait-member type
/// name must produce the root-cause `UnresolvedType` with no secondary
/// conformance-mismatch cascade when a structure conforms to the trait.
#[test]
fn trait_member_unknown_name_headline_no_cascade() {
    // Root-cause probe.
    let module = compile_source("module m\ntrait T { param x : Bogus }");
    assert!(
        has_error_code(&module, DiagnosticCode::UnresolvedType),
        "expected an UnresolvedType error for `trait T {{ param x : Bogus }}`; \
         diagnostics: {:?}",
        module.diagnostics
    );

    // Anti-cascade: conforming structure must not spawn a secondary mismatch.
    let module2 = compile_source(
        "module m\ntrait T { param x : Bogus }\nstructure def S : T { param x : Real = 1.0 }",
    );
    // The root-cause UnresolvedType must be present.
    assert!(
        has_error_code(&module2, DiagnosticCode::UnresolvedType),
        "expected an UnresolvedType error in the conforming scenario; diagnostics: {:?}",
        module2.diagnostics
    );
    // No "type mismatch for trait member" cascade.
    let type_mismatch_cascade = module2
        .diagnostics
        .iter()
        .any(|d| d.message.contains("type mismatch for trait member"));
    assert!(
        !type_mismatch_cascade,
        "no 'type mismatch for trait member' cascade must be emitted; \
         diagnostics: {:?}",
        module2.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (f) Trait member — missing annotation (L2)
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (f): a conforming-structure scenario where a trait param member lacks
/// its annotation (`param x` without `: Length`). Must produce exactly one error
/// ("no type annotation") and NO secondary "type mismatch for trait member".
///
/// Before the L2 fix (#4647), `conformance/checker.rs` returned
/// `Type::dimensionless_scalar()` from the missing-annotation arm, which leaked
/// a silent `Real` into the member-vs-requirement check and spawned a second
/// "type mismatch for trait member 'x': expected Length, got Real" error.
#[test]
fn trait_member_missing_annotation_headline_one_error_no_conformance_cascade() {
    let source = r#"
trait T {
    param x : Length
}
structure def W : T {
    param x
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Exactly one error.
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for missing-annotation trait-member scenario; got: {:?}",
        errors
    );
    // Root cause must say "no type annotation" or "cannot infer type".
    assert!(
        errors[0].message.contains("no type annotation")
            || errors[0].message.contains("cannot infer type"),
        "expected the single error to contain 'no type annotation' or 'cannot infer type'; \
         got: {:?}",
        errors[0]
    );
    // No "type mismatch for trait member" cascade (at any severity).
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "no 'type mismatch for trait member' cascade must be emitted (anti-cascade); \
         diagnostics: {:?}",
        module.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (g) Type-argument position — integer literal
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (g): an integer literal in a type-argument position (`Foo<5>()`)
/// must lower the type-arg to `Type::Error` (poison sentinel) — not a silent
/// dimensionless `Real` — and produce no secondary cascade.
///
/// Before the L0 fix (#4645), entity.rs:1865 `_ =>` arm returned
/// `Type::dimensionless_scalar()`, making `.is_error()` false.
#[test]
fn sub_invalid_type_arg_headline_type_error_no_cascade() {
    let source = r#"
structure def Foo<T> { param x : Real = 1.0 }
structure def Asm { sub b = Foo<5>() }
"#;
    let module = compile_source(source);

    // The type-arg must be Type::Error (not dimensionless_scalar).
    let tmpl = find_template(&module.templates, "Asm")
        .expect("template Asm should compile despite the invalid type arg");
    let sub_b = tmpl
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("sub 'b' must be present");
    let first_type_arg = sub_b
        .type_args
        .first()
        .expect("sub 'b' must carry the integer type arg as type_args[0]");
    assert!(
        first_type_arg.is_error(),
        "invalid IntegerLiteral type arg `5` must lower to Type::Error (poison), \
         not a silent dimensionless Real; got: {:?}",
        first_type_arg
    );
    // No cascade: at most 1 error (the invalid type-arg root cause).
    assert!(
        error_count(&module) <= 1,
        "expected at most 1 error for `Foo<5>()` (no cascade); diagnostics: {:?}",
        module.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (h) L4 method-receiver trio — AggregationReceiverNotCollection
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (h1): `(5kg).sum` — a wrong receiver for `.sum` must emit exactly one
/// `AggregationReceiverNotCollection` error and produce `Type::Error` (no cascade).
#[test]
fn method_receiver_sum_headline_one_error() {
    let source = "structure S { let broken = (5kg).sum }";
    let m = compile_source(source);

    assert!(
        has_error_code(&m, DiagnosticCode::AggregationReceiverNotCollection),
        "expected AggregationReceiverNotCollection for `(5kg).sum`; diagnostics: {:#?}",
        m.diagnostics
    );
    assert_eq!(
        error_count(&m),
        1,
        "expected exactly 1 error (no cascade) for `(5kg).sum`; diagnostics: {:#?}",
        m.diagnostics
    );
    let expr = get_let_expr(&m, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for `(5kg).sum`; got: {:?}",
        expr.result_type
    );
}

/// Cell (h2): `(5kg).keys` — a wrong receiver for `.keys` must emit exactly one
/// `AggregationReceiverNotCollection` error and produce `Type::Error` (no cascade).
#[test]
fn method_receiver_keys_headline_one_error() {
    let source = "structure S { let broken = (5kg).keys }";
    let m = compile_source(source);

    assert!(
        has_error_code(&m, DiagnosticCode::AggregationReceiverNotCollection),
        "expected AggregationReceiverNotCollection for `(5kg).keys`; diagnostics: {:#?}",
        m.diagnostics
    );
    assert_eq!(
        error_count(&m),
        1,
        "expected exactly 1 error (no cascade) for `(5kg).keys`; diagnostics: {:#?}",
        m.diagnostics
    );
    let expr = get_let_expr(&m, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for `(5kg).keys`; got: {:?}",
        expr.result_type
    );
}

/// Cell (h3): `(5kg).values` — a wrong receiver for `.values` must emit exactly
/// one `AggregationReceiverNotCollection` error and produce `Type::Error` (no
/// cascade).
#[test]
fn method_receiver_values_headline_one_error() {
    let source = "structure S { let broken = (5kg).values }";
    let m = compile_source(source);

    assert!(
        has_error_code(&m, DiagnosticCode::AggregationReceiverNotCollection),
        "expected AggregationReceiverNotCollection for `(5kg).values`; diagnostics: {:#?}",
        m.diagnostics
    );
    assert_eq!(
        error_count(&m),
        1,
        "expected exactly 1 error (no cascade) for `(5kg).values`; diagnostics: {:#?}",
        m.diagnostics
    );
    let expr = get_let_expr(&m, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for `(5kg).values`; got: {:?}",
        expr.result_type
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (h4) Struct-ref missing member
// ─────────────────────────────────────────────────────────────────────────────

/// Cell (h4): `w.nonexistent` where `w : Widget` — accessing a nonexistent
/// member on a StructureRef-typed value must emit exactly one error
/// (`StructureMemberNotFound`) and produce `Type::Error` (no cascade).
#[test]
fn structref_missing_member_headline_one_error() {
    let source = r#"
structure Widget {
    param mass : Mass = 5kg
}
structure Holder {
    let w = Widget()
    let broken = w.nonexistent
}
"#;
    let m = compile_source(source);

    assert!(
        has_error_code(&m, DiagnosticCode::StructureMemberNotFound),
        "expected StructureMemberNotFound for `w.nonexistent`; diagnostics: {:#?}",
        m.diagnostics
    );
    assert!(
        error_count(&m) <= 1,
        "expected at most 1 error (no cascade) for `w.nonexistent`; diagnostics: {:#?}",
        m.diagnostics
    );
    let expr = get_let_expr_in(&m, "Holder", "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for `w.nonexistent`; got: {:?}",
        expr.result_type
    );
}
