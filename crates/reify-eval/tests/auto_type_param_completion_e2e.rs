//! ζ — auto-type-param completion integration gate.
//!
//! PRD references: docs/prds/v0_3/auto-type-param-resolution-completion.md
//!   §11 (boundary-table), §12 Phase 6 (integration gate).
//!
//! This aggregate harness binds four user-facing example fixtures end-to-end
//! under the REAL `SimpleConstraintChecker` (the same checker the CLI and GUI
//! binaries inject).  It covers the §11 rows that are genuinely end-to-end on
//! the shipped examples/auto/*.ri files:
//!
//! - §11.1 row #3 "Constraint-aware unique selection" (real→Selected) — step-3
//! - §11.1 row #5 "Bounded fallback, jointly infeasible" — step-5
//! - §11.1 row #6 "Value population" — step-1
//! - §11.1 new "NoCandidate negative" — step-6
//! - §11.2 row #2 "Stub-path callers unchanged" (stub-vs-real contrast) — step-4
//!
//! Fixtures bound:
//!   - examples/auto/bearing_resolved_value.ri   (α/δ — single candidate, value pop)
//!   - examples/auto/bearing_constraint_select.ri (β — per-candidate ValueMap + real→Selected)
//!   - examples/auto/bounded_fallback_unsound.ri  (γ — joint-recheck BoundedInfeasible)
//!   - examples/auto/bearing_unsat.ri             (ζ — NoCandidate, all candidates violated)
//!
//! Tasks that produced these fixtures: α=4431, β=4433, γ=4434, δ=4435, ζ=4437.

#![allow(clippy::mutable_key_type)]

// ── Fixture path constants ────────────────────────────────────────────────────

/// Absolute path to examples/auto/bearing_resolved_value.ri.
/// Produced by task 4431 (α) + value-population wired by task 4435 (δ).
const BEARING_RESOLVED_VALUE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_resolved_value.ri"
);

/// Absolute path to examples/auto/bearing_constraint_select.ri.
/// Produced by task 4433 (β — per-candidate ValueMap + real-checker selection).
const BEARING_CONSTRAINT_SELECT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_constraint_select.ri"
);

/// Absolute path to examples/auto/bounded_fallback_unsound.ri.
/// Produced by task 4434 (γ — BFS-fallback joint-recheck, BoundedInfeasible).
const BOUNDED_FALLBACK_UNSOUND_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bounded_fallback_unsound.ri"
);

/// Absolute path to examples/auto/bearing_unsat.ri.
/// Produced by task 4437 (ζ — NoCandidate negative fixture, all candidates violated).
const BEARING_UNSAT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_unsat.ri"
);

// ── Imports (populated in step-2 when helpers are defined) ───────────────────
//
// These imports are declared up-front so the RED test (step-1) compiles only
// with the missing helper names as the failure cause, not missing `use` items.

use reify_compiler::compile_with_stdlib_checked;
use reify_compiler::compile_with_stdlib;
use reify_compiler::parse_with_stdlib;
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, ModulePath};
use reify_ir::{PersistentMap, Value};
use reify_test_support::{collect_errors, make_simple_engine};

// ── Step-1 RED test ───────────────────────────────────────────────────────────

/// ζ §11.1 row #6 "Value population" — end-to-end on the shipped fixture.
///
/// Reads examples/auto/bearing_resolved_value.ri from disk, compiles under the
/// REAL SimpleConstraintChecker, evals, and asserts:
///   (a) zero Error diagnostics
///   (b) BearingResolved.b is StructureInstance whose `seal` field is
///       StructureInstance(type_name=="GasketSeal") carrying
///       `thickness` Value::Scalar si_value ≈ 0.002 (2mm, exact-by-construction).
///
/// RED until step-2 defines `compile_real`/`eval_real` (compile error).
/// GREEN after step-2; no production edits needed (α+δ already landed).
#[test]
fn resolved_value_eval_populates_gasketseal_2mm() {
    let src = read_fixture(BEARING_RESOLVED_VALUE_PATH);
    let compiled = compile_real(&src, "bearing_resolved_value");

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "bearing_resolved_value.ri must compile with zero Errors under real checker, got: {:?}",
        errors
    );

    let result = eval_real(&compiled);

    let sub_b = result
        .values
        .get(&reify_core::ValueCellId::new("BearingResolved", "b"))
        .unwrap_or_else(|| {
            let cells: Vec<_> = result.values.iter().map(|(id, _)| id.clone()).collect();
            panic!(
                "BearingResolved.b cell missing from eval result. Available: {:?}",
                cells
            )
        });

    match sub_b {
        Value::StructureInstance(bearing_data) => {
            let seal_val = field(&bearing_data.fields, "seal").unwrap_or_else(|| {
                let keys: Vec<_> = bearing_data.fields.iter().map(|(k, _)| k.clone()).collect();
                panic!(
                    "Bearing$GasketSeal instance must have a 'seal' field; fields: {:?}",
                    keys
                )
            });
            match seal_val {
                Value::StructureInstance(seal_data) => {
                    assert_eq!(
                        seal_data.type_name, "GasketSeal",
                        "seal instance type_name must be 'GasketSeal', got '{}'",
                        seal_data.type_name
                    );
                    let thickness = field(&seal_data.fields, "thickness").unwrap_or_else(|| {
                        let keys: Vec<_> =
                            seal_data.fields.iter().map(|(k, _)| k.clone()).collect();
                        panic!(
                            "GasketSeal must have a 'thickness' field; fields: {:?}",
                            keys
                        )
                    });
                    match thickness {
                        Value::Scalar { si_value, .. } => {
                            const EPSILON: f64 = 1e-10;
                            assert!(
                                (*si_value - 0.002).abs() < EPSILON,
                                "GasketSeal.thickness must be 2mm (si_value≈0.002), got {}",
                                si_value
                            );
                        }
                        other => panic!(
                            "GasketSeal.thickness must be Value::Scalar, got {:?}",
                            other
                        ),
                    }
                }
                Value::Undef => panic!(
                    "bearing.seal is Value::Undef — δ synthesis not wired or real-checker path broken"
                ),
                other => panic!(
                    "expected Value::StructureInstance for bearing.seal, got {:?}",
                    other
                ),
            }
        }
        Value::Undef => panic!("BearingResolved.b is Value::Undef — sub evaluation failed"),
        other => panic!(
            "expected Value::StructureInstance for BearingResolved.b, got {:?}",
            other
        ),
    }
}
