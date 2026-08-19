//! Compiler-level tests for `E_OBJECTIVE_MIXED_DIMENSION` detection (task α #5018).
//!
//! PRD: `docs/prds/v0_6/multi-aspect-objective-units-coherence.md`.
//!
//! # Problem
//!
//! Two same-sense `minimize` decls (e.g. `minimize cost` + `minimize mass`) do
//! NOT trigger `E_OBJECTIVE_CONFLICT` — that only catches opposite-sense
//! Min/Max pairs. They lower to a multi-term `WeightedSum` `ObjectiveSet`,
//! whose numeric fold strips each term's dimension (`as_f64()`) before
//! summing — so a Money term + a Mass term silently produce a physically
//! meaningless scalar with no diagnostic. This compile-time gate closes that
//! hole by requiring every term of a multi-term `WeightedSum` to share ONE
//! dimension (not "each term dimensionless" — that would regress shipped
//! single-aspect Money objectives).
//!
//! # Cases covered
//!
//! (BT1) REJECT: `minimize cost` (Money) + `minimize mass` (Mass)
//!     → `DiagnosticCode::ObjectiveDimensionIncoherent`, `Severity::Error`,
//!     message contains `"E_OBJECTIVE_MIXED_DIMENSION"`, `"Money"`, and
//!     `"Mass"`, and ≥1 label with a non-empty span.
//!
//! (BT2) CLEAN — same-dimension multi-term: `minimize cost_a` (Money) +
//!     `minimize cost_b` (Money) → no `ObjectiveDimensionIncoherent` diagnostic.
//!
//! (BT3) CLEAN — single-aspect (no regression): single `minimize cost` (Money)
//!     → no `ObjectiveDimensionIncoherent` diagnostic.

use reify_core::{DiagnosticCode, ModulePath, Severity};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Parse `src` and compile it; return the compiled module.
/// Panics if parsing produces errors.
fn compile_module(src: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ── (BT1) REJECT: minimize cost (Money) + minimize mass (Mass) ──────────────

/// A structure with `minimize cost` (Money) + `minimize mass` (Mass) — a
/// multi-term `WeightedSum` whose terms do NOT share one dimension — must
/// produce exactly one `DiagnosticCode::ObjectiveDimensionIncoherent`
/// diagnostic at `Severity::Error`.
///
/// The message must contain:
/// - `"E_OBJECTIVE_MIXED_DIMENSION"` (the PRD-prose mnemonic)
/// - `"Money"` and `"Mass"` (both dimensions named)
///
/// The diagnostic must carry ≥1 label with a non-empty span (required by the
/// compiler diagnostic convention in `diagnostic_coverage_checkpoint.rs`).
#[test]
fn mixed_dimension_money_mass_emits_error() {
    let src = r#"structure S {
    param cost: Money = auto
    param mass: Mass = auto
    minimize cost
    minimize mass
}"#;

    let compiled = compile_module(src, "test_mixed_dimension");

    let diag = compiled
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ObjectiveDimensionIncoherent))
        .unwrap_or_else(|| {
            panic!(
                "expected an ObjectiveDimensionIncoherent diagnostic, got: {:#?}",
                compiled.diagnostics
            )
        });

    assert_eq!(
        diag.severity,
        Severity::Error,
        "ObjectiveDimensionIncoherent must be Severity::Error, got {:?}",
        diag.severity
    );

    assert!(
        diag.message.contains("E_OBJECTIVE_MIXED_DIMENSION"),
        "message must contain \"E_OBJECTIVE_MIXED_DIMENSION\", got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("Money"),
        "message must name the 'Money' dimension, got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("Mass"),
        "message must name the 'Mass' dimension, got: {:?}",
        diag.message
    );

    assert!(
        !diag.labels.is_empty(),
        "ObjectiveDimensionIncoherent diagnostic must carry at least one label"
    );
    assert!(
        diag.labels.iter().any(|l| !l.span.is_empty()),
        "at least one label must have a non-empty span, labels: {:#?}",
        diag.labels
    );
}

// ── (BT2) CLEAN: same-dimension multi-term (Money + Money) ──────────────────

/// Two `minimize` objectives that both resolve to `Money` are coherent (they
/// share one dimension). No `ObjectiveDimensionIncoherent` diagnostic must be
/// emitted.
#[test]
fn same_dimension_multi_term_money_is_clean() {
    let src = r#"structure S {
    param cost_a: Money = auto
    param cost_b: Money = auto
    minimize cost_a
    minimize cost_b
}"#;

    let compiled = compile_module(src, "test_same_dimension_money");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveDimensionIncoherent)),
        "two same-dimension (Money) objectives must not produce ObjectiveDimensionIncoherent, got: {:#?}",
        compiled.diagnostics
    );
}

// ── (BT3) CLEAN: single-aspect objective (no regression) ────────────────────

/// A single `minimize cost` (Money) objective is a 1-term set — the
/// units-coherence guard only fires for `terms.len() > 1`. No
/// `ObjectiveDimensionIncoherent` diagnostic must be emitted. This pins the
/// no-regression requirement for shipped single-aspect Money objectives.
#[test]
fn single_aspect_money_objective_is_clean() {
    let src = r#"structure S {
    param cost: Money = auto
    minimize cost
}"#;

    let compiled = compile_module(src, "test_single_aspect");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveDimensionIncoherent)),
        "a single Money objective must not produce ObjectiveDimensionIncoherent, got: {:#?}",
        compiled.diagnostics
    );
}
