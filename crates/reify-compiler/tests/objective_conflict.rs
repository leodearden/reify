//! Compiler-level tests for `E_OBJECTIVE_CONFLICT` detection (task 4010).
//!
//! PRD: `docs/prds/v0_6/constraint-solver-completion.md` task ζ §3.3/§6.3
//! boundary-sketch B3.
//!
//! # Conflict predicate (§6.3)
//!
//! Emit `DiagnosticCode::ObjectiveConflict` iff:
//! - `combination == WeightedSum`
//! - `terms.len() > 1`
//! - every term has default `weight == 1.0` and `priority == 0`
//! - at least one pair of terms has **opposite sense** (`Minimize` vs `Maximize`)
//!   over **distinct expressions** (compared by `CompiledExpr.content_hash`)
//!
//! # Cases covered
//!
//! (a) CONFLICT: `minimize mass` + `maximize stiffness` (distinct exprs, opposite sense)
//!     → `DiagnosticCode::ObjectiveConflict`, `Severity::Error`, message contains
//!     `"E_OBJECTIVE_CONFLICT"`, three escape hints (weights / priorities /
//!     combine-into-one-expression), and ≥1 label with a non-empty span.
//!
//! (b) NO-CONFLICT same-sense: `minimize mass` + `minimize cost`
//!     → no `ObjectiveConflict` diagnostic.
//!
//! (c) Single objective: `minimize mass`
//!     → no `ObjectiveConflict` diagnostic.
//!
//! (d) Mixed-sense SAME-expr: `minimize mass` + `maximize mass`
//!     → no `ObjectiveConflict` diagnostic (distinct-expression qualifier).

use reify_core::{DiagnosticCode, ModulePath, Severity};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Parse `src` and compile it; return the compiled module.
/// Panics if parsing produces errors.
fn compile_module(src: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(module_name));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

// ── (a) CONFLICT: minimize mass + maximize stiffness ─────────────────────────

/// A structure with `minimize mass` + `maximize stiffness` (distinct params,
/// opposite sense, both at default weight 1.0 / priority 0) must produce exactly
/// one `DiagnosticCode::ObjectiveConflict` diagnostic at `Severity::Error`.
///
/// The message must contain:
/// - `"E_OBJECTIVE_CONFLICT"` (the PRD-prose mnemonic, embedded so the CLI
///   renders it via the `"{severity}: {message}"` format)
/// - at least one reference to the "weight" escape (letting the user assign
///   non-default weights to resolve the conflict)
/// - at least one reference to the "priority" escape (letting the user assign
///   non-default priorities to lexicographically order the objectives)
/// - at least one reference to combining objectives into a single expression
///   (the third escape path)
///
/// The diagnostic must carry ≥1 label with a non-empty span (required by the
/// compiler diagnostic convention in `diagnostic_coverage_checkpoint.rs`).
#[test]
fn conflict_minimize_mass_maximize_stiffness_emits_error() {
    let src = r#"structure S {
    param mass: Scalar = auto
    param stiffness: Scalar = auto
    minimize mass
    maximize stiffness
}"#;

    let compiled = compile_module(src, "test_conflict");

    // Find the ObjectiveConflict diagnostic.
    let conflict_diag = compiled
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ObjectiveConflict))
        .unwrap_or_else(|| {
            panic!(
                "expected an ObjectiveConflict diagnostic, got: {:#?}",
                compiled.diagnostics
            )
        });

    // Severity must be Error.
    assert_eq!(
        conflict_diag.severity,
        Severity::Error,
        "ObjectiveConflict must be Severity::Error, got {:?}",
        conflict_diag.severity
    );

    // Message must contain the PRD-prose mnemonic so the CLI renders it.
    assert!(
        conflict_diag.message.contains("E_OBJECTIVE_CONFLICT"),
        "message must contain \"E_OBJECTIVE_CONFLICT\", got: {:?}",
        conflict_diag.message
    );

    // Message must name the three escape routes.
    assert!(
        conflict_diag.message.contains("weight"),
        "message must mention the 'weight' escape, got: {:?}",
        conflict_diag.message
    );
    assert!(
        conflict_diag.message.contains("priority"),
        "message must mention the 'priority' escape, got: {:?}",
        conflict_diag.message
    );
    // The combine-into-one-expression escape: the message should mention
    // combining / merging the objectives into a single expression.
    assert!(
        conflict_diag.message.contains("expression")
            || conflict_diag.message.contains("combine")
            || conflict_diag.message.contains("single"),
        "message must mention combining into one expression, got: {:?}",
        conflict_diag.message
    );

    // Must carry ≥1 label with a non-empty span.
    assert!(
        !conflict_diag.labels.is_empty(),
        "ObjectiveConflict diagnostic must carry at least one label"
    );
    assert!(
        conflict_diag.labels.iter().any(|l| l.span.len() > 0),
        "at least one label must have a non-empty span, labels: {:#?}",
        conflict_diag.labels
    );
}

// ── (b) NO-CONFLICT: same-sense, minimize mass + minimize cost ───────────────

/// Two `minimize` objectives over distinct expressions are **not** a conflict
/// (both have the same sense). No `ObjectiveConflict` diagnostic must be emitted.
#[test]
fn no_conflict_same_sense_minimize_minimize() {
    let src = r#"structure S {
    param mass: Scalar = auto
    param cost: Scalar = auto
    minimize mass
    minimize cost
}"#;

    let compiled = compile_module(src, "test_no_conflict_same_sense");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveConflict)),
        "two same-sense objectives must not produce ObjectiveConflict, got: {:#?}",
        compiled.diagnostics
    );
}

// ── (c) NO-CONFLICT: single objective ────────────────────────────────────────

/// A single `minimize` objective is never a conflict (terms.len() == 1).
/// No `ObjectiveConflict` diagnostic must be emitted.
#[test]
fn no_conflict_single_objective() {
    let src = r#"structure S {
    param mass: Scalar = auto
    minimize mass
}"#;

    let compiled = compile_module(src, "test_no_conflict_single");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveConflict)),
        "a single objective must not produce ObjectiveConflict, got: {:#?}",
        compiled.diagnostics
    );
}

// ── (d) NO-CONFLICT: mixed-sense SAME-expr ───────────────────────────────────

/// `minimize mass` + `maximize mass` — opposite sense over the **same** expression.
/// The distinct-expression qualifier (§6.3) excludes this case: the content
/// hashes of the two terms' expressions are equal, so no conflict is emitted.
#[test]
fn no_conflict_mixed_sense_same_expression() {
    let src = r#"structure S {
    param mass: Scalar = auto
    minimize mass
    maximize mass
}"#;

    let compiled = compile_module(src, "test_no_conflict_same_expr");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveConflict)),
        "mixed-sense over the same expression must not produce ObjectiveConflict, got: {:#?}",
        compiled.diagnostics
    );
}
