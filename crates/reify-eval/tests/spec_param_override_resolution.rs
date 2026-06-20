//! Eval e2e tests for non-auto specialization-scope param_assignment overrides
//! (task 4694, steps 1–2).
//!
//! ## Goal
//! Make `sub b : Bearing { bore = 3mm }` actually resolve `A.b.bore` to 3mm at
//! runtime, not the child default 5mm.  PRD docs/prds/specialization-scope.md AC4.
//!
//! ## RED phase (step 1)
//! entity.rs currently does `continue` for non-auto entries in spec_param_overrides,
//! so the override is silently discarded and `A.b.bore` resolves to the Bearing
//! default (5mm).  Both tests below fail.
//!
//! ## GREEN phase (step 2)
//! entity.rs injects non-auto overrides into `SubComponentDecl.args`.  The eval
//! args-precedence path (`unfold.rs:elaborate_child_params_only:336`) applies the
//! override before the child default, writing `(3mm, Determined)` to the snapshot.
//! Both tests pass.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::parse_and_compile_with_stdlib;

// ── Shared fixtures ───────────────────────────────────────────────────────────

/// Bearing has a `bore` param with default 5mm so that the 3mm override is
/// clearly distinct from the default value.
const BEARING_5MM: &str = "structure Bearing { param bore : Length = 5mm }";

/// Build an Engine backed by `SimpleConstraintChecker` + `DimensionalSolver`.
fn engine_with_solver() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(DimensionalSolver))
}

/// Filter `diagnostics` to error-severity entries.
fn errors_only(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── Test (a): literal override resolves to the overridden value ───────────────

/// AC4 runtime: `sub b : Bearing { bore = 3mm }` must resolve `A.b.bore` to 3mm
/// (0.003 SI), NOT the child default 5mm (0.005 SI).
///
/// Discriminator: child default 5mm ≠ override 3mm.  If the override is dropped
/// the snapshot holds 0.005; if it is applied correctly it holds 0.003.
///
/// RED (step 1): entity.rs drops the non-auto override → A.b.bore = 5mm.
/// GREEN (step 2): entity.rs injects (bore, 3mm) into SubComponentDecl.args.
#[test]
fn non_auto_override_resolves_to_literal_value() {
    let source = format!(
        "{BEARING_5MM}  structure A {{ sub b : Bearing {{ bore = 3mm }} }}"
    );
    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "unexpected eval errors: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "A.b.bore should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "A.b.bore should be Determined; got {:?}",
        det
    );

    // 3mm = 0.003 SI — must NOT be 5mm (0.005 SI, child default).
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.003).abs() < 1e-6),
        "A.b.bore should equal 3mm (0.003 SI); got {:?} \
         (if 0.005: override was dropped and child default was used)",
        val
    );
}

// ── Test (b, amend suggestion 1/4): duplicate override resolves to first value ──

/// When a specialization body repeats a member (`{ bore = 3mm  bore = 4mm }`),
/// the FIRST value (3mm) must win at runtime — the second is warned and skipped.
///
/// Pins the "first assignment wins" eval behaviour (amend 4694, suggestion 1).
#[test]
fn duplicate_non_auto_override_resolves_to_first_value() {
    let source = format!(
        "{BEARING_5MM}  structure A {{ sub b : Bearing {{ bore = 3mm  bore = 4mm }} }}"
    );
    let compiled = parse_and_compile_with_stdlib(&source);

    // Duplicate is a warning, not an error.
    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);
    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "unexpected eval errors: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "A.b.bore should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "A.b.bore should be Determined; got {:?}",
        det
    );

    // First assignment (3mm = 0.003 SI) must win, NOT 4mm (0.004 SI).
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.003).abs() < 1e-6),
        "A.b.bore should equal 3mm (0.003 SI) — first assignment wins; got {:?}",
        val
    );
}

// ── Test (c): AC4 no-error — override with a constraint sees the override value ─

/// AC4 no-error: `sub b : Bearing { bore = 3mm }` with `constraint self.b.bore > 1mm`
/// must compile and eval with zero error-severity diagnostics, and `A.b.bore` must
/// equal 3mm.
///
/// Validates that the parent's constraint can observe the overridden value (3mm)
/// and that this combination is legal (3mm > 1mm is trivially satisfied).
///
/// RED (step 1): override is dropped → bore = 5mm (child default). The
///   constraint 5mm > 1mm holds, so the test would PASS on the eval-error check.
///   But the bore == 3mm assertion fails — the test is still RED.
/// GREEN (step 2): bore = 3mm. No eval errors. Bore assertion passes.
#[test]
fn non_auto_override_with_constraint_no_error() {
    let source = format!(
        "{BEARING_5MM}  \
         structure A {{ \
             sub b : Bearing {{ bore = 3mm }}  \
             constraint self.b.bore > 1mm \
         }}"
    );
    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "AC4 no-error: unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "AC4 no-error: unexpected eval errors: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("A.b", "bore");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "AC4 no-error: A.b.bore should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "AC4 no-error: A.b.bore should be Determined; got {:?}",
        det
    );

    // Must be 3mm (0.003 SI), not 5mm (child default).
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.003).abs() < 1e-6),
        "AC4 no-error: A.b.bore should equal 3mm (0.003 SI); got {:?}",
        val
    );
}
