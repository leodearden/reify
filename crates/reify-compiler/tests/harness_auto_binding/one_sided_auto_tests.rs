//! Static arm of the one-sided strict-auto refusal (task #6692, P1-ε).
//!
//! PRD `docs/prds/v0_6/solver-driver-parity.md` §4.6 case 2 / §5 B4–B5: a
//! strict `auto` whose module bounds it on ONE side only has no designed value,
//! so the compiler refuses it with a `ConstraintNonUnique` Error naming the
//! missing bound (INV-SF-4: a compile Error rather than an `Indeterminate`).
//! The pass fires only on a positive proof and stays silent on every shape
//! whose population is not static — those fall to the runtime arm.
//!
//! Driven through the public seam only: `compile_source{,_with_stdlib}` and
//! `CompiledModule.diagnostics`.

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::{compile_source, compile_source_with_stdlib};

const NOT_DETERMINED: &str = "not uniquely determined";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../tests/prd-gate/fixtures/{name}.ri",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {path}: {e}"))
}

fn non_unique(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique))
        .collect()
}

fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Asserts the module carries exactly one one-sided refusal, an Error naming
/// `clause`, anchored on a non-empty primary label.
fn assert_fires(case: &str, diagnostics: &[Diagnostic], clause: &str) {
    let refusals = non_unique(diagnostics);
    assert_eq!(
        refusals.len(),
        1,
        "{case}: expected exactly one ConstraintNonUnique; diagnostics: {diagnostics:#?}"
    );
    let refusal = refusals[0];
    assert_eq!(
        refusal.severity,
        Severity::Error,
        "{case}: the refusal must be an Error; got {refusal:#?}"
    );
    assert!(
        refusal.message.contains(NOT_DETERMINED) && refusal.message.contains(clause),
        "{case}: message must carry {NOT_DETERMINED:?} and {clause:?}; got {:?}",
        refusal.message
    );
    let primary = refusal
        .labels
        .first()
        .unwrap_or_else(|| panic!("{case}: the refusal must carry a label; got {refusal:#?}"));
    assert!(
        !primary.span.is_empty(),
        "{case}: the primary label must anchor on a non-empty span; got {refusal:#?}"
    );
}

/// Asserts the module carries no refusal AND no other Error, so a silent row
/// cannot pass by failing to compile in some other way.
fn assert_silent(case: &str, diagnostics: &[Diagnostic]) {
    assert!(
        errors(diagnostics).is_empty(),
        "{case}: expected a clean compile (no ConstraintNonUnique, no other Error); \
         diagnostics: {diagnostics:#?}"
    );
}

// ── B4 / B5 / F2 fixtures, compiled WITH the stdlib prelude ─────────────────
//
// The prelude contributes `simulation_ready` / `design_review` purposes, so
// the B4 row also proves those bound-free purposes do not silence the pass.

#[test]
fn b4_one_sided_fixture_is_refused_at_compile_time() {
    let compiled = compile_source_with_stdlib(&fixture("driver_parity_one_sided_auto"));
    assert_fires(
        "B4 driver_parity_one_sided_auto.ri",
        &compiled.diagnostics,
        "no constraint bounds `OneSided.rod.length` above",
    );
}

#[test]
fn b5_two_sided_fixture_compiles_clean() {
    let compiled = compile_source_with_stdlib(&fixture("driver_parity_two_sided_auto"));
    assert!(
        non_unique(&compiled.diagnostics).is_empty(),
        "B5 driver_parity_two_sided_auto.ri: two-sided bounds must not draw the refusal; \
         diagnostics: {:#?}",
        compiled.diagnostics
    );
}

#[test]
fn f2_one_sided_auto_free_fixture_compiles_clean() {
    let compiled = compile_source_with_stdlib(&fixture("driver_parity_one_sided_auto_free"));
    assert!(
        non_unique(&compiled.diagnostics).is_empty(),
        "F2 driver_parity_one_sided_auto_free.ri: auto(free) is never refused; \
         diagnostics: {:#?}",
        compiled.diagnostics
    );
}

// ── Inline rows, compiled WITHOUT the prelude ───────────────────────────────

/// (name, source, missing side) rows the pass must refuse.
const FIRES: &[(&str, &str, &str)] = &[
    (
        "upper-only bound",
        "structure T {
            param x : Length = auto
            constraint x <= 12mm
        }",
        "no constraint bounds `T.x` below",
    ),
    (
        "private template, lower-only bound",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
        }",
        "no constraint bounds `T.x` above",
    ),
    (
        "pub template with its own auto (not a sub override)",
        "pub structure T {
            param x : Length = auto
            constraint x >= 8mm
        }",
        "no constraint bounds `T.x` above",
    ),
    (
        "far operand is a non-auto let",
        "structure T {
            param x : Length = auto
            let lo = 8mm
            constraint x >= lo
        }",
        "no constraint bounds `T.x` above",
    ),
    (
        "objective-less container does not rescue its child",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
        }
        structure A {
            sub t = T()
        }",
        "no constraint bounds `T.x` above",
    ),
];

/// (name, source) rows the pass must stay silent on.
const SILENT: &[(&str, &str)] = &[
    (
        "two-sided in one conjunction",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm && x <= 12mm
        }",
    ),
    (
        "explicit objective",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
            minimize x
        }",
    ),
    (
        "equality mention",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
            constraint x == 10mm
        }",
    ),
    (
        "coupled autos",
        "structure T {
            param x : Length = auto
            param y : Length = auto
            constraint x >= 8mm
            constraint y <= 12mm
            constraint x <= y
        }",
    ),
    (
        "coefficient form",
        "structure T {
            param x : Length = auto
            constraint 2 * x >= 16mm
        }",
    ),
    (
        "let-mediated bound",
        "structure T {
            param x : Length = auto
            let d = x
            constraint d >= 8mm
        }",
    ),
    (
        "auto(free)",
        "structure T {
            param x : Length = auto(free)
            constraint x >= 8mm
        }",
    ),
    (
        "Int-typed auto",
        "structure T {
            param n : Int = auto
            constraint n >= 2
        }",
    ),
    (
        "solver_hint-carrying auto",
        "structure T {
            let sizes = [22mm, 24mm, 26mm]
            @solver_hint(\"discrete_set\", sizes)
            param x : Length = auto
            constraint x >= 8mm
        }",
    ),
    (
        "guarded group holding a constraint",
        "structure T {
            param g : Real = 1.0
            param x : Length = auto
            constraint x >= 8mm
            where g > 0.0 { constraint x <= 12mm }
        }",
    ),
    (
        "deferred forall over a collection sub",
        "structure Vent {
            param mass : Mass = 10kg
        }
        structure T {
            sub vents : List<Vent>
            param n : Int
            param x : Length = auto
            constraint vents.count == n
            constraint x >= 8mm
            forall v in vents: constraint v.mass < 50kg
        }",
    ),
    (
        "opaque conjunct elsewhere in the template",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
            constraint forall i in [1, 2, 3]: i > 0
        }",
    ),
    (
        "objective-bearing container may govern its child",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
        }
        structure A {
            param k : Length = 1mm
            sub t = T()
            minimize k
        }",
    ),
    (
        "purpose holding a comparison constraint",
        "structure T {
            param x : Length = auto
            constraint x >= 8mm
        }
        purpose p(subject : Structure) {
            constraint subject.x <= 12mm
        }",
    ),
];

#[test]
fn one_sided_strict_autos_are_refused() {
    for (name, source, clause) in FIRES {
        let compiled = compile_source(source);
        assert_fires(name, &compiled.diagnostics, clause);
    }
}

#[test]
fn shapes_without_a_static_one_sided_proof_stay_silent() {
    for (name, source) in SILENT {
        let compiled = compile_source(source);
        assert_silent(name, &compiled.diagnostics);
    }
}
