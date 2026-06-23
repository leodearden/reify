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

use reify_core::{DiagnosticCode, Severity};
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
    fn f(self) -> Length { 1mm }
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
    fn f(self) -> Real { 1.0 }
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

    // The conformer template must carry exactly one (Derived or Base) f entry.
    // (The trait that registers it as a requirement may be either after dedup.)
    let not_satisfied: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
        .collect();
    assert!(
        not_satisfied.is_empty(),
        "conformer C provides fn f — no TraitFnNotSatisfied should fire; \
         diagnostics: {:?}",
        module.diagnostics
    );
}
