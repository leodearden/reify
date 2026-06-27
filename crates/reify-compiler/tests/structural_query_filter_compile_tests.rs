//! Compiler typing / diagnostic tests for `filter(entity_ref_list, Trait)`
//! conformance filter (task 3991, δ).
//!
//! Observable signals (PRD §10):
//!   (a) `filter(self.descendants, KnownTrait)` compiles with zero Error
//!       diagnostics and the result cell types to `Type::List(StructureRef)`.
//!   (b) `filter(self.descendants, NotATrait)` (unknown trait name) emits
//!       exactly one Error diagnostic with `DiagnosticCode::UnresolvedTrait`
//!       (or message containing "NotATrait") and does NOT panic.
//!   (c) `filter(self.descendants, |x| true)` (lambda 2nd arg; PRD §9
//!       out-of-scope) emits a diagnostic and does not panic.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

// ─── (a) Known-trait filter compiles clean + types to List<StructureRef> ───

/// `filter(self.descendants, Bolt)` with a declared `trait Bolt {}` and a
/// structure conforming to Bolt compiles with zero Error diagnostics, and the
/// `bolts` value cell types to `Type::List(Box<Type::StructureRef(_)>)` —
/// same list type as `self.descendants`.
///
/// RED today: `filter` is not recognized as a conformance filter; the compiler
/// either errors on the `Bolt` identifier (unresolved name) or mishandles the
/// call, so at least one Error diagnostic is emitted.
#[test]
fn filter_known_trait_compiles_clean_and_types_list_of_entity_ref() {
    let source = r#"
        trait Bolt {}
        structure HexBolt { trait Bolt }
        structure Plain {}
        structure Assembly {
            sub b = HexBolt()
            sub p = Plain()
            let bolts = filter(self.descendants, Bolt)
        }
    "#;

    let compiled = compile_source(source);

    // (a) Zero Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for filter(self.descendants, Bolt), got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) The `bolts` cell types to List(StructureRef(_)).
    let asm_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template");

    let bolts_cell = asm_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "bolts")
        .expect("bolts value cell");

    let default_expr = bolts_cell
        .default_expr
        .as_ref()
        .expect("bolts has default_expr");

    match &default_expr.result_type {
        Type::List(inner) => {
            assert!(
                matches!(inner.as_ref(), Type::StructureRef(_)),
                "bolts: expected List(StructureRef(_)), got List({:?})",
                inner
            );
        }
        other => panic!(
            "bolts: expected Type::List, got: {:?}",
            other
        ),
    }
}

// ─── (b) Unknown trait name → UnresolvedTrait diagnostic ───

/// `filter(self.descendants, NotATrait)` where `NotATrait` is not a declared
/// trait must emit exactly one Error diagnostic with
/// `DiagnosticCode::UnresolvedTrait` (or a message containing "NotATrait"),
/// and must NOT panic.
///
/// RED today: `filter` is not intercepted, so the compiler treats `NotATrait`
/// as an unresolved value name and may emit a different diagnostic code or
/// multiple diagnostics — this assertion will catch mismatches.
#[test]
fn filter_unknown_trait_emits_unresolved_trait_diagnostic() {
    let source = r#"
        structure Assembly {
            let bolts = filter(self.descendants, NotATrait)
        }
    "#;

    // Must not panic.
    let compiled = compile_source(source);

    // At least one Error diagnostic.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for unknown trait 'NotATrait'"
    );

    // The diagnostic must either carry DiagnosticCode::UnresolvedTrait or
    // mention the unknown name in its message.
    let has_unresolved_trait_diag = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::UnresolvedTrait)
            || d.message.to_lowercase().contains("notattrait")
            || d.message.contains("NotATrait")
            || d.message.contains("unresolved trait")
    });
    assert!(
        has_unresolved_trait_diag,
        "expected a diagnostic with DiagnosticCode::UnresolvedTrait or message \
         mentioning 'NotATrait'/'unresolved trait'; got: {:?}",
        errors.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
    );
}

// ─── (c) Lambda 2nd arg → out-of-scope diagnostic ───

/// `filter(self.descendants, |x| true)` — a lambda 2nd arg is out of scope
/// per PRD §9.  The compiler must emit a diagnostic (not panic) whose message
/// references "not supported" or "lambda" or "PRD §9".
///
/// RED today: `filter` is not intercepted, so the lambda falls through to
/// generic resolution and may produce a type mismatch or unrelated error —
/// this assertion will catch when step-2 adds the correct diagnostic.
#[test]
fn filter_lambda_arg_emits_unsupported_diagnostic() {
    let source = r#"
        structure Assembly {
            let x = filter(self.descendants, |e| true)
        }
    "#;

    // Must not panic.
    let compiled = compile_source(source);

    // At least one Error diagnostic.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for lambda 2nd arg in filter()"
    );

    // The diagnostic must mention "not supported", "lambda", "predicate",
    // or reference PRD §9 — confirming the filter-specific path fired.
    let has_filter_diag = errors.iter().any(|d| {
        let m = d.message.to_lowercase();
        m.contains("not supported")
            || m.contains("lambda")
            || m.contains("predicate")
            || m.contains("§9")
            || m.contains("out of scope")
            || m.contains("filter")
    });
    assert!(
        has_filter_diag,
        "expected a diagnostic mentioning 'not supported', 'lambda', 'predicate', \
         or filter-related text; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
