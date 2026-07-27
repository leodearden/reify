//! B+H integration gate for PRD `docs/prds/v0_6/undef-self-describing.md` §5
//! (task 4326 η): the full BT1–BT12 boundary suite.
//!
//! # Purpose
//!
//! This file is the batch's join node. BT1–BT11 already ship green from the
//! α/β/γ/δ/ε/ζ dependency tasks and run in CI; re-implementing them here
//! would be brittle duplication that drifts from the originals. The table
//! below is the authoritative BT1–BT11 cross-reference — each row names the
//! test(s) that OWN that boundary; this file is the *collected view* plus the
//! two locks no single dependency task could author on its own (BT8
//! strengthened, BT12), and the one lock upstream covered only partially
//! (BT4, capture-side-map level only — lifted here to tracer level).
//!
//! # PRD §5 BT1–BT12 canonical test homes
//!
//! | BT   | Scenario (PRD §5)                  | Owner file / function                              |
//! |------|-------------------------------------|-----------------------------------------------------|
//! | BT1  | Single unbound root (A2/B2)         | `reify-eval/src/undef_tracer.rs` unit `single_root_unbound` |
//! | BT2  | Two independent roots, deduped (B1) | `reify-eval/tests/undef_tracer.rs` fn `bt2_two_root_propagated` (real eval) + `reify-eval/src/undef_tracer.rs` unit `diamond_dedup` (synthetic) |
//! | BT3  | Chain collapse (B2)                 | `reify-eval/tests/undef_tracer.rs` fn `bt3_chain_collapse` (real eval) + `reify-eval/src/undef_tracer.rs` unit `chain_collapse` (synthetic) |
//! | BT4  | Auto / solve (A2)                   | CAPTURE-level (upstream): `reify-eval/tests/undef_cause_capture.rs` fns `layer1_non_solver_origins_recorded` (AwaitingSolve), `solve_failed_origin_recorded` + `no_progress_solve_failed_origin_recorded` (SolveFailed). TRACER-level (this file, genuinely new): `bt4_tracer_surfaces_solve_origins` below |
//! | BT5  | Op failure crossing boundary (G1/B1)| `reify-eval/tests/undef_cause_op_contract.rs` fn `bt5_x_has_op_contract_failed_in_side_map_and_both_causes_in_tracer` |
//! | BT6  | No false op cause (G2)              | `reify-eval/tests/undef_cause_op_contract.rs` fn `bt6_y_has_no_false_op_contract_failed` |
//! | BT7  | Cycle safety (B4)                   | `reify-eval/src/undef_tracer.rs` unit `cycle_terminates` (synthetic — locks visited-set termination; see design_decisions for why no real-eval BT7 is added here) |
//! | BT8  | **Transparency** (A1/G3)            | PARTIAL upstream: `reify-eval/tests/undef_cause_capture.rs` fn `capture_is_byte_transparent` + `reify-eval/tests/undef_cause_op_contract.rs` fn `g3_transparency_capture_off_is_byte_identical` (both on the geometry-free layer1/op-contract fixtures). STRENGTHENED (this file, genuinely new): `bt8_transparency_representative_design` below — realizing + constrained design |
//! | BT9  | CLI surface (S1)                    | `reify-cli/tests/cli_undef_self_describing.rs` fns `eval_undef_emits_note_with_complete_cause_set`, `eval_determined_emits_no_undef_notes`, `eval_explain_undef_surfaces_all_undef_cells`, `eval_default_gates_out_unbound_param_subject_lines` |
//! | BT10 | GUI surface (S2)                    | `gui/src-tauri/src/tests/engine_tests.rs` fns `build_gui_state_populates_reason_for_unbound_param`, `get_parameters_mcp_carries_reason_for_unbound_param`, `build_gui_state_populates_reason_for_propagated_undef` |
//! | BT11 | LSP surface (S3)                    | `reify-lsp/src/hover.rs` fns `hover_on_undef_param_appends_cause_set`, `hover_on_determined_param_appends_no_cause_set`, `hover_on_undef_let_shows_propagated_root_cause`; `reify-lsp/src/analysis.rs` fns `undef_cause_line_unbound_param_returns_cause`, `undef_cause_line_determined_param_returns_none` |
//! | BT12 | Surface agreement (S4)              | NOT covered anywhere upstream — inherently an integration test no single dependency task could write. Added (genuinely new) as SET-level anchors across `reify-cli/tests/cli_undef_self_describing.rs`, `gui/src-tauri/src/tests/engine_tests.rs`, `reify-lsp/src/hover.rs` (see those files; see design_decisions for why set-level, not byte-identical strings) |
//!
//! This gate does not re-run BT1–BT11 — see the table above for their owner
//! tests, which run daily in CI. It adds BT8-strengthened and BT4-tracer-level
//! below, plus the BT12 cross-surface anchors in the three surface test files.

use std::collections::BTreeSet;

use reify_core::{Diagnostic, ValueCellId};
use reify_eval::Engine;
use reify_ir::{ExportFormat, UndefCause};
use reify_test_support::{
    MockConstraintChecker, MockConstraintSolver, MockGeometryKernel, collect_errors,
    compile_source_with_stdlib,
};

// ── fixture helpers ───────────────────────────────────────────────────────────

fn representative_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_boundary_representative.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_boundary_representative.ri should compile without errors: {errors:#?}"
    );
    m
}

// ── BT8 STRENGTHENED: transparency over a realizing + constrained design ─────

/// BT8 (A1/G3), strengthened beyond the geometry-free layer1/op-contract
/// fixtures: capture on vs. off over a design that both realizes geometry
/// (`body: Solid = box(...)`, exercising the realization cache /
/// `Value::GeometryHandle` content-hash-excludes-kernel_handle dimension) and
/// carries a checked constraint (a non-trivial constraint report), built via
/// `Engine::build` with `MockGeometryKernel` + `MockConstraintChecker`.
///
/// Extends the `capture_is_byte_transparent` / `g3_transparency_capture_off_is_byte_identical`
/// pattern (BTreeSet cell-id compare, per-cell (Value,DeterminacyState) +
/// content_hash equality, Debug-string diagnostics compare) with the two
/// dimensions the geometry-free fixtures could not exercise: realization-cache
/// outcomes (geometry-handle cells + exported bytes) and a real constraint
/// report (`constraint_results`).
///
/// True by construction — capture is a purely additive side-map — so this is
/// expected GREEN on authoring; it locks the property against regression on a
/// design shape the upstream fixtures didn't cover.
#[test]
fn bt8_transparency_representative_design() {
    let module = representative_module();

    // Capture OFF.
    let mut engine_off = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(MockGeometryKernel::new())),
    );
    let result_off = engine_off.build(&module, ExportFormat::Step);
    let snap_off = engine_off
        .snapshot()
        .expect("snapshot present after build");

    // Capture ON.
    let mut engine_on = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine_on.set_capture_undef_causes(true);
    let result_on = engine_on.build(&module, ExportFormat::Step);
    let snap_on = engine_on.snapshot().expect("snapshot present after build");

    // (1) Identical cell-id set.
    let ids_off: BTreeSet<_> = snap_off.values.keys().cloned().collect();
    let ids_on: BTreeSet<_> = snap_on.values.keys().cloned().collect();
    assert_eq!(
        ids_off, ids_on,
        "cell id sets must match across capture on/off"
    );

    // (2)+(3) Per-cell (Value,DeterminacyState) and content_hash byte-identical.
    // Covers the realization cell `body` too: both `==` and `content_hash()`
    // for Value::GeometryHandle intentionally exclude the ephemeral
    // `kernel_handle` (GHR-β §DD), so two independently-built engines compare
    // equal despite minting different MockGeometryKernel handle ids.
    for id in &ids_off {
        let (val_off, det_off) = snap_off.values.get(id).unwrap();
        let (val_on, det_on) = snap_on.values.get(id).unwrap();
        assert_eq!(
            (val_off, det_off),
            (val_on, det_on),
            "cell {id}: (Value,DeterminacyState) must be identical across capture on/off"
        );
        assert_eq!(
            val_off.content_hash(),
            val_on.content_hash(),
            "cell {id}: content_hash must be identical across capture on/off"
        );
    }

    // (4) Realization-cache outcome: the exported geometry bytes are identical.
    assert_eq!(
        result_off.geometry_output, result_on.geometry_output,
        "BuildResult.geometry_output must be identical across capture on/off"
    );

    // (5) Constraint report (`constraint_results`) identical, and non-empty —
    // proves the fixture's `constraint` clause was genuinely checked.
    assert!(
        !result_off.constraint_results.is_empty(),
        "representative design must have at least one checked constraint"
    );
    assert_eq!(
        format!("{:?}", result_off.constraint_results),
        format!("{:?}", result_on.constraint_results),
        "BuildResult.constraint_results must be identical across capture on/off"
    );

    // BuildResult diagnostics identical. `Diagnostic` has no `PartialEq`, so
    // compare via Debug-format strings (mirrors capture_is_byte_transparent).
    assert_eq!(
        format!("{:?}", result_off.diagnostics),
        format!("{:?}", result_on.diagnostics),
        "BuildResult.diagnostics must be identical across capture on/off"
    );

    // (6) Capture ON is not a silent no-op; capture OFF stays empty.
    assert!(
        !engine_on.undef_causes().is_empty(),
        "capture ON engine must have non-empty undef_causes after build"
    );
    assert!(
        engine_off.undef_causes().is_empty(),
        "capture OFF engine must have empty undef_causes after build"
    );
}

// ── BT4 TRACER-LEVEL: solve-origin variants surfaced end-to-end ──────────────

/// Reuses `undef_cause_capture.rs`'s Layer-1 fixture-loading pattern (test
/// binaries are compiled independently, so the helper is duplicated rather
/// than shared — matching the repo convention, e.g. `undef_cause_op_contract.rs`
/// also defines its own local fixture helpers).
fn layer1_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_causes_layer1.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_causes_layer1.ri should compile without errors: {errors:#?}"
    );
    m
}

fn solve_failed_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_cause_solve_failed.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_cause_solve_failed.ri should compile without errors: {errors:#?}"
    );
    m
}

/// BT4 (A2), lifted from CAPTURE-side-map level (already covered by
/// `undef_cause_capture.rs`'s `layer1_non_solver_origins_recorded` /
/// `solve_failed_origin_recorded`) to end-to-end TRACER level: asserts
/// `Engine::trace_undef_causes` surfaces the solve-origin variants
/// (`AwaitingSolve`, `SolveFailed`) — not just `Unbound`/`UserUndef` — when
/// walked from a real cell id after a real eval.
#[test]
fn bt4_tracer_surfaces_solve_origins() {
    // (a) auto cell "k", no solver attached → tracer surfaces AwaitingSolve.
    let layer1 = layer1_module();
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&layer1);

    let k_id = ValueCellId::new("UndefDemo", "k");
    let k_causes = engine.trace_undef_causes(&k_id);
    assert!(
        k_causes
            .iter()
            .any(|c| matches!(c, UndefCause::AwaitingSolve { .. })),
        "trace_undef_causes(k) must surface AwaitingSolve, got {k_causes:?}"
    );

    // (b) infeasible solve on auto param "x" → tracer surfaces SolveFailed
    // with a non-empty detail string.
    let solve_failed = solve_failed_module();
    let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
        "constraints are infeasible",
    )]);
    let mut engine2 =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));
    engine2.set_capture_undef_causes(true);
    engine2.eval(&solve_failed);

    let x_id = ValueCellId::new("SolveFail", "x");
    let x_causes = engine2.trace_undef_causes(&x_id);
    let solve_failed_cause = x_causes
        .iter()
        .find(|c| matches!(c, UndefCause::SolveFailed { .. }))
        .unwrap_or_else(|| {
            panic!("trace_undef_causes(x) must surface SolveFailed, got {x_causes:?}")
        });
    if let UndefCause::SolveFailed { detail } = solve_failed_cause {
        assert!(!detail.is_empty(), "SolveFailed.detail must not be empty");
    }
}
