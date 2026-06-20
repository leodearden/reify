//! Integration tests: `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` honesty
//! diagnostic (Gap-C — task 4616).
//!
//! # Invariants under test
//!
//! 1. **No false positives (invariant 1):** a constraint that reads a
//!    LITERAL-default template cell does NOT emit the warning — the literal
//!    is already seeded, so the constraint is not skipped.
//! 2. **Stub suppression (invariant 2):** the compile-time STUB checker
//!    (`CompileTimeIndeterminateChecker`, used by `compile_with_stdlib`) must
//!    NOT emit the warning — it is reserved for real-checker paths so that
//!    examples_smoke and other stub-path callers remain unaffected.
//! 3. **No duplicate (implied by invariant 3):** the template-side pair
//!    collection is performed once per declaration, so one (constraint, cell)
//!    pair produces exactly one warning — no per-candidate or per-leaf copies.
//!
//! # RED state
//!
//! Before step-8 wires the emit, the assertions in
//! `computed_default_non_stub_checker_emits_unevaluated_warning` will fail
//! because zero `AutoTypeParamConstraintUnevaluated` diagnostics are emitted.
//! `computed_default_stub_checker_emits_no_unevaluated_warning` and
//! `literal_default_control_emits_no_unevaluated_warning` will pass even in
//! the RED state (the warning is absent, which is what they assert).

use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_ir::{ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction};

// ── Local non-stub checker ────────────────────────────────────────────────────

/// An always-indeterminate checker that does NOT override
/// `is_compile_time_stub()`, so it inherits the trait default of `false`.
///
/// This is the "non-stub real checker" half of invariant 2: when the emit is
/// wired, `compile_with_stdlib_checked(&parsed, &AlwaysIndeterminate)` must
/// produce the `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` warning, while
/// `compile_with_stdlib(&parsed)` (the stub, `is_compile_time_stub() == true`)
/// must not.
struct AlwaysIndeterminate;

impl ConstraintChecker for AlwaysIndeterminate {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }
}

// ── Fixture sources ──────────────────────────────────────────────────────────

/// Source with a COMPUTED-default template cell (`clearance = bore_radius - 0.5mm`)
/// referenced by the constraint `seal.thickness < clearance`.
///
/// `clearance`'s default is a non-literal BinOp, so the literal-only seeder
/// silently skips it.  Under any non-stub checker, this produces a Gap-C pair
/// → `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` must fire naming `clearance`.
///
/// Selection outcome: one ThinSeal candidate, Indeterminate under any checker
/// that returns Indeterminate for all constraints → Selected("ThinSeal") with
/// no Error.  The warning is the ONLY expected diagnostic.
const COMPUTED_DEFAULT_SOURCE: &str = r#"
    trait Seal {
        param thickness : Length
    }
    structure def ThinSeal : Seal {
        param thickness : Length = 1mm
    }
    structure def Bearing<T: Seal> {
        param bore_radius : Length = 3mm
        param seal : T
        param clearance : Length = bore_radius - 0.5mm
        constraint seal.thickness < clearance
    }
    structure def Assembly {
        sub b = Bearing<auto: Seal>()
    }
"#;

/// Same as `COMPUTED_DEFAULT_SOURCE` but `clearance` has a LITERAL default
/// (`2.5mm`).  The literal is seeded, so the constraint is not in the Gap-C
/// skip-set → `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` must NOT fire under
/// any checker.
const LITERAL_DEFAULT_SOURCE: &str = r#"
    trait Seal {
        param thickness : Length
    }
    structure def ThinSeal : Seal {
        param thickness : Length = 1mm
    }
    structure def Bearing<T: Seal> {
        param bore_radius : Length = 3mm
        param seal : T
        param clearance : Length = 2.5mm
        constraint seal.thickness < clearance
    }
    structure def Assembly {
        sub b = Bearing<auto: Seal>()
    }
"#;

/// Parse `src` under the stdlib prelude using the given module name.
fn parse(src: &str, module_name: &str) -> reify_ast::ParsedModule {
    reify_compiler::parse_with_stdlib(src, ModulePath::single(module_name))
}

// ── Invariant 2 half A: non-stub checker emits the warning ───────────────────

/// A non-stub `ConstraintChecker` (inheriting `is_compile_time_stub() == false`)
/// must cause `compile_with_stdlib_checked` to emit EXACTLY ONE
/// `AutoTypeParamConstraintUnevaluated` `Warning` whose message names the
/// computed-default cell (`clearance`).
///
/// **RED** until step-8 wires `emit_unevaluated_constraint_warnings` into
/// `resolve_auto_type_params_with_backtracking`.
#[test]
fn computed_default_non_stub_checker_emits_unevaluated_warning() {
    let parsed = parse(COMPUTED_DEFAULT_SOURCE, "test_gap_c_nonstub");
    let compiled =
        reify_compiler::compile_with_stdlib_checked(&parsed, &AlwaysIndeterminate);

    let unevaluated: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamConstraintUnevaluated))
        .collect();

    // (a) Exactly ONE AutoTypeParamConstraintUnevaluated diagnostic.
    assert_eq!(
        unevaluated.len(),
        1,
        "computed-default source must emit EXACTLY ONE AutoTypeParamConstraintUnevaluated \
         diagnostic under a non-stub checker (no duplicate per candidate/leaf); \
         got {} diagnostic(s) with this code. All diagnostics: {:?}",
        unevaluated.len(),
        compiled
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.code, d.message.as_str()))
            .collect::<Vec<_>>()
    );

    let w = unevaluated[0];

    // (b) Severity::Warning (not Error — invariant 3: selection unchanged).
    assert_eq!(
        w.severity,
        Severity::Warning,
        "AutoTypeParamConstraintUnevaluated must be Severity::Warning (not Error — \
         the constraint is skipped, not causing a rejection); got {:?}",
        w.severity
    );

    // (c) Message names the computed-default cell 'clearance'.
    assert!(
        w.message.contains("clearance"),
        "AutoTypeParamConstraintUnevaluated message must name the computed-default cell \
         'clearance'; got: {:?}",
        w.message
    );
}

// ── Invariant 2 half B: stub checker suppresses the warning ──────────────────

/// `compile_with_stdlib` uses the `CompileTimeIndeterminateChecker` stub
/// (which overrides `is_compile_time_stub() == true`).  The warning MUST be
/// suppressed on this path so that `examples_smoke` and other stub callers
/// are not affected.
///
/// This assertion passes even in the RED state (zero warnings → `is_empty()`
/// succeeds).  It becomes a regression guard after step-8.
#[test]
fn computed_default_stub_checker_emits_no_unevaluated_warning() {
    let parsed = parse(COMPUTED_DEFAULT_SOURCE, "test_gap_c_stub");
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    let unevaluated: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamConstraintUnevaluated))
        .collect();

    assert!(
        unevaluated.is_empty(),
        "compile_with_stdlib (stub checker, is_compile_time_stub()==true) must NOT emit \
         AutoTypeParamConstraintUnevaluated — the warning is gated on \
         !checker.is_compile_time_stub() (invariant 2); got: {:?}",
        unevaluated
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
}

// ── Invariant 1: literal-default control emits no warning ────────────────────

/// A template whose constraint reads a LITERAL-default cell (`clearance = 2.5mm`)
/// must NOT emit `AutoTypeParamConstraintUnevaluated` under either checker.
///
/// The literal is seeded before constraint evaluation → the cell is not in the
/// skip-set → no Gap-C pair → no warning.  This is the "no false positives"
/// invariant (invariant 1).
///
/// Both assertions pass even in the RED state.  They become regression guards
/// after step-8.
#[test]
fn literal_default_control_emits_no_unevaluated_warning() {
    let parsed_stub = parse(LITERAL_DEFAULT_SOURCE, "test_gap_c_literal_stub");
    let parsed_real = parse(LITERAL_DEFAULT_SOURCE, "test_gap_c_literal_real");

    let compiled_stub = reify_compiler::compile_with_stdlib(&parsed_stub);
    let compiled_real =
        reify_compiler::compile_with_stdlib_checked(&parsed_real, &AlwaysIndeterminate);

    for (label, compiled) in [
        ("stub", &compiled_stub),
        ("non-stub (AlwaysIndeterminate)", &compiled_real),
    ] {
        let unevaluated: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamConstraintUnevaluated))
            .collect();

        assert!(
            unevaluated.is_empty(),
            "literal-default control source must NOT emit AutoTypeParamConstraintUnevaluated \
             under {} checker (invariant 1 — no false positives; literal cells are seeded, \
             so the constraint is not in the Gap-C skip-set); got: {:?}",
            label,
            unevaluated
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
        );
    }
}
