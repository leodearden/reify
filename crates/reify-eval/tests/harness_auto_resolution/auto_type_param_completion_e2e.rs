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

/// Absolute path to examples/auto/bearing_computed_default_unevaluated.ri.
/// Produced by task 4616 (Gap-C honesty diagnostic — W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED).
/// Mirrors bearing_constraint_select.ri but with a computed-default template cell
/// (`clearance = bore_radius - 0.5mm`) and a constraint reading it
/// (`constraint seal.thickness < clearance`).
const BEARING_COMPUTED_DEFAULT_UNEVALUATED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_computed_default_unevaluated.ri"
);

// ── Imports ───────────────────────────────────────────────────────────────────

use reify_compiler::{
    CompiledModule, compile_with_stdlib, compile_with_stdlib_checked, parse_with_stdlib,
};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, ModulePath};
use reify_eval::EvalResult;
use reify_ir::{PersistentMap, Value};
use reify_test_support::{collect_errors, make_simple_engine};

// ── Shared harness helpers ────────────────────────────────────────────────────

/// Read a fixture file from disk, panicking with a clear error naming the file.
fn read_fixture(path: &str) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read_fixture: failed to read '{}': {}", path, e))
}

/// Compile `src` under the REAL `SimpleConstraintChecker` — the exact entry
/// the CLI (`reify-cli/src/main.rs:173`) and GUI (`engine.rs:730`) binaries use.
///
/// **Do NOT use** `parse_and_compile_with_stdlib` or `compile_source_with_stdlib`
/// here: those helpers route through `compile_with_stdlib` (the stub checker)
/// and panic on any Error diagnostic, which would mask the deliberate Errors
/// that several ζ fixtures are designed to produce.
fn compile_real(src: &str, module_name: &str) -> CompiledModule {
    let parsed = parse_with_stdlib(src, ModulePath::single(module_name));
    compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker)
}

/// Compile `src` under the STUB `CompileTimeIndeterminateChecker` — the default
/// path that `compile_with_stdlib` uses. Returns `Indeterminate` for every
/// constraint; contrast with `compile_real` to demonstrate the stub-vs-real delta.
fn compile_stub(src: &str, module_name: &str) -> CompiledModule {
    let parsed = parse_with_stdlib(src, ModulePath::single(module_name));
    compile_with_stdlib(&parsed)
}

/// Evaluate a compiled module using the real `SimpleConstraintChecker` engine.
fn eval_real(compiled: &CompiledModule) -> EvalResult {
    let mut engine = make_simple_engine();
    engine.eval(compiled)
}

/// Return `true` if any diagnostic in `diags` has the given `DiagnosticCode`.
fn has_error_code(diags: &[reify_core::Diagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == Some(code))
}

/// Get a field from a `StructureInstance`'s fields map by name.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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

// ── Step-3 ─────────────────────────────────────────────────────────────────────

/// ζ §11.1 row #3 "Constraint-aware unique selection" — real→Selected half.
///
/// Reads bearing_constraint_select.ri (two Seal candidates: ThinSeal=1mm,
/// ThickSeal=5mm; constraint `seal.thickness < bore_radius=3mm`), compiles under
/// the REAL SimpleConstraintChecker, and asserts:
///   (a) zero Errors (no AutoTypeParamAmbiguous — real checker eliminated ThickSeal)
///   (b) BearingAssembly.bearing.seal is StructureInstance(ThinSeal{thickness≈0.001})
///
/// The β test (`auto_type_param_per_candidate_valuemap_tests.rs`) explicitly
/// deferred the real→Selected half to ζ (this test).
/// GREEN binds already-landed α+β+δ; RED here is an integration regression.
#[test]
fn constraint_select_real_checker_selects_thinseal() {
    let src = read_fixture(BEARING_CONSTRAINT_SELECT_PATH);
    let compiled = compile_real(&src, "bearing_constraint_select");

    // (a) No Errors — real checker eliminates ThickSeal, selects ThinSeal.
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "bearing_constraint_select.ri must compile with zero Errors under real checker \
         (ThickSeal eliminated, ThinSeal selected), got: {:?}",
        errors
    );
    assert!(
        !has_error_code(
            &compiled.diagnostics,
            DiagnosticCode::AutoTypeParamAmbiguous
        ),
        "must NOT emit AutoTypeParamAmbiguous under real checker (stub emits it; real selects ThinSeal)"
    );

    // (b) Eval: BearingAssembly.bearing.seal is StructureInstance(ThinSeal{thickness≈0.001}).
    let result = eval_real(&compiled);

    let bearing_sub = result
        .values
        .get(&reify_core::ValueCellId::new("BearingAssembly", "bearing"))
        .unwrap_or_else(|| {
            let cells: Vec<_> = result.values.iter().map(|(id, _)| id.clone()).collect();
            panic!(
                "BearingAssembly.bearing cell missing from eval result. Available: {:?}",
                cells
            )
        });

    match bearing_sub {
        Value::StructureInstance(bearing_data) => {
            let seal_val = field(&bearing_data.fields, "seal").unwrap_or_else(|| {
                let keys: Vec<_> = bearing_data.fields.iter().map(|(k, _)| k.clone()).collect();
                panic!(
                    "Bearing$ThinSeal must have a 'seal' field; fields: {:?}",
                    keys
                )
            });
            match seal_val {
                Value::StructureInstance(seal_data) => {
                    assert_eq!(
                        seal_data.type_name, "ThinSeal",
                        "seal type_name must be 'ThinSeal' (the unique constraint-satisfying survivor), \
                         got '{}'",
                        seal_data.type_name
                    );
                    let thickness = field(&seal_data.fields, "thickness").unwrap_or_else(|| {
                        let keys: Vec<_> =
                            seal_data.fields.iter().map(|(k, _)| k.clone()).collect();
                        panic!("ThinSeal must have a 'thickness' field; fields: {:?}", keys)
                    });
                    match thickness {
                        Value::Scalar { si_value, .. } => {
                            const EPSILON: f64 = 1e-10;
                            // ThinSeal.thickness = 1mm = 0.001 m in SI
                            assert!(
                                (*si_value - 0.001).abs() < EPSILON,
                                "ThinSeal.thickness must be 1mm (si_value≈0.001), got {}",
                                si_value
                            );
                        }
                        other => {
                            panic!("ThinSeal.thickness must be Value::Scalar, got {:?}", other)
                        }
                    }
                }
                Value::Undef => panic!(
                    "bearing.seal is Value::Undef — real-checker selection or δ synthesis broken"
                ),
                other => panic!(
                    "expected Value::StructureInstance for bearing.seal, got {:?}",
                    other
                ),
            }
        }
        Value::Undef => panic!("BearingAssembly.bearing is Value::Undef — sub evaluation failed"),
        other => panic!(
            "expected Value::StructureInstance for BearingAssembly.bearing, got {:?}",
            other
        ),
    }
}

// ── Step-4 ─────────────────────────────────────────────────────────────────────

/// ζ §11.1 row #3 "Constraint-aware unique selection" stub half; §11.2 row #2
/// "Stub-path callers unchanged".
///
/// The SAME fixture (bearing_constraint_select.ri) compiled under the STUB
/// checker must produce an AutoTypeParamAmbiguous Error — proving the
/// stub-vs-real delta is the injected checker, not the fixture.
///
/// GREEN binds the already-landed β-inject stub default.
#[test]
fn constraint_select_stub_is_ambiguous() {
    let src = read_fixture(BEARING_CONSTRAINT_SELECT_PATH);
    let compiled = compile_stub(&src, "bearing_constraint_select");

    assert!(
        has_error_code(
            &compiled.diagnostics,
            DiagnosticCode::AutoTypeParamAmbiguous
        ),
        "bearing_constraint_select.ri must emit AutoTypeParamAmbiguous under the stub checker \
         (both candidates are stub-feasible → ≥2 feasible → Ambiguous); \
         diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ── Step-5 ─────────────────────────────────────────────────────────────────────

/// ζ §11.1 row #5 "Bounded fallback, jointly infeasible".
///
/// Reads bounded_fallback_unsound.ri (7 LayerA params, joint constraint
/// l1.thickness+…+l7.thickness=14mm > max_stack=10mm) under the REAL checker.
/// Asserts:
///   (a) AutoTypeParamBoundedInfeasible Error is present
///   (b) No successful substitution: StackAssembly.stack is NOT a populated
///       StructureInstance (must be Undef or absent — γ joint-recheck blocked
///       the substitution).
///
/// GREEN binds already-landed γ; RED here is an integration regression.
#[test]
fn bounded_fallback_unsound_emits_bounded_infeasible() {
    let src = read_fixture(BOUNDED_FALLBACK_UNSOUND_PATH);
    let compiled = compile_real(&src, "bounded_fallback_unsound");

    // (a) BoundedInfeasible Error must be present.
    assert!(
        has_error_code(
            &compiled.diagnostics,
            DiagnosticCode::AutoTypeParamBoundedInfeasible
        ),
        "bounded_fallback_unsound.ri must emit AutoTypeParamBoundedInfeasible under real checker \
         (7 LayerA at 2mm each → 14mm > max_stack=10mm → γ joint-recheck Violated); \
         diagnostics: {:?}",
        compiled.diagnostics
    );

    // (b) Soundness via ctx/templates inspection: no accepted LayeredStack$…
    //     monomorph should exist in compiled.templates.
    //
    // When BoundedInfeasible fires, the auto_type_param phase does NOT emit a
    // substitution — the monomorphized template is never appended.  Monomorph
    // names carry the `$` separator (e.g. `LayeredStack$LayerA_LayerA_…`);
    // only the generic definition `LayeredStack` (no `$`) should be present.
    //
    // NOTE: eval_real is intentionally NOT called here.  The BoundedInfeasible
    // module contains unresolved TypeParam value cells (the LayeredStack
    // template's l1..l7 params still typed as T1..T7), which cause
    // engine_eval.rs to panic with "unrepresentable cell_type TypeParam(…)".
    // Templates inspection is the correct soundness path for this fixture.
    let monomorph_name = compiled
        .templates
        .iter()
        .find(|t| t.name.starts_with("LayeredStack$"))
        .map(|t| t.name.clone());
    assert!(
        monomorph_name.is_none(),
        "SOUNDNESS VIOLATION: compiled.templates contains '{}' despite BoundedInfeasible — \
         γ joint-recheck did not block the monomorph substitution!",
        monomorph_name.as_deref().unwrap_or("<none>")
    );
}

// ── Step-6 ─────────────────────────────────────────────────────────────────────

/// ζ new "NoCandidate negative" — all candidates rejected by constraint.
///
/// Reads examples/auto/bearing_unsat.ri (two Seal candidates: ThickSeal=5mm,
/// HugeSeal=8mm; constraint `seal.thickness < bore_radius=3mm`; both violate),
/// compiles under the REAL SimpleConstraintChecker, and asserts:
///   (a) AutoTypeParamNoCandidate Error is present
///   (b) the error message names the violated constraint
///       (contains "rejected by constraint")
///   (c) no Bearing$* monomorph template — clean diagnostic, no silent Undef
///
/// RED: examples/auto/bearing_unsat.ri does not exist yet → read_fixture's
/// .expect panics at runtime.  GREEN after step-7 authors the fixture.
#[test]
fn bearing_unsat_emits_no_candidate_naming_constraint() {
    let src = read_fixture(BEARING_UNSAT_PATH);
    let compiled = compile_real(&src, "bearing_unsat");

    // (a) NoCandidate Error must be present.
    assert!(
        has_error_code(
            &compiled.diagnostics,
            DiagnosticCode::AutoTypeParamNoCandidate
        ),
        "bearing_unsat.ri must emit AutoTypeParamNoCandidate under real checker \
         (ThickSeal=5mm and HugeSeal=8mm both violate seal.thickness < bore_radius=3mm); \
         diagnostics: {:?}",
        compiled.diagnostics
    );

    // (b) Error message names the violated constraint.
    let no_candidate_msg = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate))
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        no_candidate_msg.contains("rejected by constraint"),
        "NoCandidate message must name the violated constraint via \
         \"rejected by constraint\"; got: {:?}",
        no_candidate_msg
    );

    // (c) No Bearing$* monomorph: clean diagnostic, no silent substitution.
    let monomorph_name = compiled
        .templates
        .iter()
        .find(|t| t.name.starts_with("Bearing$"))
        .map(|t| t.name.clone());
    assert!(
        monomorph_name.is_none(),
        "SOUNDNESS VIOLATION: compiled.templates contains '{}' despite NoCandidate Error — \
         a Bearing substitution was accepted when all candidates were violated!",
        monomorph_name.as_deref().unwrap_or("<none>")
    );
}

// ── Step-7 (Gap-C regression gate, task 4616) ─────────────────────────────────

/// Gap-C regression gate: `bearing_computed_default_unevaluated.ri` compiled
/// under the REAL `SimpleConstraintChecker` must emit
/// `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` naming the computed-default cell,
/// and the LITERAL-threshold `bearing_constraint_select.ri` must NOT emit it
/// (negative control, invariant 1 — no false positives for literal cells).
///
/// Covers PRD §6 deliverable (task #4616 Gap-C leaf).
///
/// # Assertions
///
/// (a) `bearing_computed_default_unevaluated.ri` under `SimpleConstraintChecker`:
///     - One `AutoTypeParamConstraintUnevaluated` `Warning` is present.
///     - Its message names the computed-default cell `clearance`.
///     - Selection outcome is `AutoTypeParamAmbiguous` (unchanged vs pre-Gap-C:
///       clearance is still unevaluated → both ThinSeal + ThickSeal are
///       Indeterminate → ≥2 feasible → Ambiguous).
///
/// (b) No new `Error`-severity diagnostics introduced by Gap-C (invariant 3 —
///     the warning is the ONLY new diagnostic; Errors are unchanged).
///
/// (c) `bearing_constraint_select.ri` under `SimpleConstraintChecker` emits NO
///     `AutoTypeParamConstraintUnevaluated` warning (negative control — `bore_radius`
///     is a literal default and is seeded; the constraint is not in the skip-set).
///
/// **RED:** `BEARING_COMPUTED_DEFAULT_UNEVALUATED_PATH` does not exist yet →
/// `read_fixture` panics at runtime.
/// **GREEN** after step-10 creates `bearing_computed_default_unevaluated.ri` and
/// adds it to the `examples_smoke` SKIP_SET.
#[test]
fn gap_c_computed_default_unevaluated_emits_warning_literal_does_not() {
    // ── (a) + (b): positive fixture under real checker ───────────────────────

    let src = read_fixture(BEARING_COMPUTED_DEFAULT_UNEVALUATED_PATH);
    let compiled = compile_real(&src, "bearing_computed_default_unevaluated");

    // (a-i) AutoTypeParamConstraintUnevaluated Warning must be present.
    assert!(
        has_error_code(
            &compiled.diagnostics,
            DiagnosticCode::AutoTypeParamConstraintUnevaluated
        ),
        "bearing_computed_default_unevaluated.ri must emit \
         AutoTypeParamConstraintUnevaluated under the real checker \
         (clearance's default is a computed expression — skipped by the seeder); \
         diagnostics: {:?}",
        compiled.diagnostics
    );

    // (a-ii) The warning message must name the computed-default cell 'clearance'.
    let warning_msgs: Vec<&str> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamConstraintUnevaluated))
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        warning_msgs.iter().any(|msg| msg.contains("clearance")),
        "AutoTypeParamConstraintUnevaluated message must name the computed-default cell \
         'clearance'; got: {:?}",
        warning_msgs
    );

    // (a-iii) Selection outcome: AutoTypeParamAmbiguous Error must be present
    // (clearance unevaluated → both ThinSeal + ThickSeal Indeterminate → Ambiguous).
    assert!(
        has_error_code(&compiled.diagnostics, DiagnosticCode::AutoTypeParamAmbiguous),
        "bearing_computed_default_unevaluated.ri must still emit AutoTypeParamAmbiguous \
         under real checker (clearance skipped → both candidates Indeterminate → ≥2 feasible \
         → Ambiguous — selection outcome unchanged vs pre-Gap-C, invariant 3); \
         diagnostics: {:?}",
        compiled.diagnostics
    );

    // (b) No unexpected new Errors introduced by Gap-C (only the Warning is new).
    // Collect errors excluding the expected AutoTypeParamAmbiguous.
    let unexpected_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Error
                && d.code != Some(DiagnosticCode::AutoTypeParamAmbiguous)
        })
        .collect();
    assert!(
        unexpected_errors.is_empty(),
        "Gap-C (task #4616) must not introduce unexpected Error-severity diagnostics \
         (invariant 3 — only the Warning is new, selection is unchanged); \
         unexpected errors: {:?}",
        unexpected_errors
    );

    // ── (c): negative control — literal threshold does NOT emit the warning ───

    let src_literal = read_fixture(BEARING_CONSTRAINT_SELECT_PATH);
    let compiled_literal = compile_real(&src_literal, "bearing_constraint_select_neg_ctrl");

    assert!(
        !has_error_code(
            &compiled_literal.diagnostics,
            DiagnosticCode::AutoTypeParamConstraintUnevaluated
        ),
        "bearing_constraint_select.ri (literal bore_radius) must NOT emit \
         AutoTypeParamConstraintUnevaluated under the real checker \
         (bore_radius is a literal default — seeded, not in the skip-set, \
         invariant 1 — no false positives); diagnostics: {:?}",
        compiled_literal.diagnostics
    );
}
