//! B12 e2e emission test for FEA structured diagnostic payloads (task 4802, step-3 RED).
//!
//! Strategy (per plan §B12):
//!   - Reuses the fea_no_supports.ri fixture (existing; proven to yield a
//!     Completed-with-FeaUnderConstrained-Warning in fea_diagnostics_e2e.rs).
//!   - Asserts that eval_result.structured_detail == [Fea(Unconstrained{6 modes})]
//!     and that a fresh check_result.structured_detail carries the same payload.
//!
//! Each assertion uses a separate engine instance so the FEA dispatch runs fresh
//! (not from the warm in-process cache, which does not re-emit structured_detail
//! on hits — matching the same behaviour as diagnostics).
//!
//! RED at step-3: elastic_static.rs does not yet populate structured_detail
//! (the accumulator is declared but the :416 emission site is not wired).
//! GREEN after step-4 wires both emission sites.
//!
//! `fea_unconstrained_structured_detail_survives_a_warm_persistent_serve`
//! (task 7345) pins the on-disk half: a second engine sharing the first one's
//! persistent cache dir must replay the same payload. It takes two engines
//! because a same-engine re-eval is served by the in-memory NodeCache and never
//! reaches `run_compute_dispatch`, where the persistent lookup lives.

use reify_eval::{
    StructuredComputeDetail,
    compute_targets::fea_diagnostics::{DofDirection, FeaDiagnosticDetail},
};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Unconstrained-body solve (no supports) → Completed with FeaUnderConstrained warning.
///
/// (1) eval_result.structured_detail must carry exactly one payload:
///     Fea(FeaDiagnosticDetail::Unconstrained { rigid_body_modes: all 6 })
///
/// (2) check_result.structured_detail (from a fresh engine) must carry the same payload.
///     This proves the EvalResult → CheckResult propagation — the R3b-2 read point.
#[test]
fn fea_unconstrained_eval_and_check_carry_structured_detail() {
    let source = include_str!("../fixtures/fea_no_supports.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let expected_detail = vec![StructuredComputeDetail::Fea(
        FeaDiagnosticDetail::Unconstrained {
            rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
        },
    )];

    // (1) eval_result.structured_detail carries the Unconstrained payload.
    {
        let mut engine = make_simple_engine();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        let eval_result = engine.eval(&compiled);
        assert_eq!(
            eval_result.structured_detail,
            expected_detail,
            "eval_result.structured_detail must carry [Fea(Unconstrained{{6 modes}})];\
             got: {:#?}",
            eval_result.structured_detail
        );
    }

    // (2) check_result.structured_detail carries the same payload.
    // Fresh engine ensures the FEA dispatch runs (not from warm cache).
    {
        let mut engine = make_simple_engine();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        let check_result = engine.check(&compiled);
        assert_eq!(
            check_result.structured_detail,
            expected_detail,
            "check_result.structured_detail must carry [Fea(Unconstrained{{6 modes}})];\
             got: {:#?}",
            check_result.structured_detail
        );
    }
}

/// A warm persistent serve must replay the structured detail the cold solve
/// emitted: present, and not duplicated.
#[test]
fn fea_unconstrained_structured_detail_survives_a_warm_persistent_serve() {
    let source = include_str!("../fixtures/fea_no_supports.ri");
    let expected_detail = vec![StructuredComputeDetail::Fea(
        FeaDiagnosticDetail::Unconstrained {
            rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
        },
    )];
    let tmp = tempfile::TempDir::new().expect("tmp dir creation must succeed");

    // ── Engine A: cold solve, which writes the persistent entry ─────────────
    let mut engine_a = make_simple_engine();
    engine_a.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_a);
    let cold = engine_a.eval(&parse_and_compile_with_stdlib(source));

    assert_eq!(
        cold.structured_detail, expected_detail,
        "non-vacuity: the cold solve must emit the Unconstrained payload"
    );
    assert_eq!(
        engine_a.persistent_hit_count(),
        0,
        "engine A is a cold solve and must not be served from disk"
    );
    fn has_bin_file(dir: &std::path::Path) -> bool {
        std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .any(|e| {
                let p = e.path();
                if p.is_dir() {
                    has_bin_file(&p)
                } else {
                    p.extension().is_some_and(|x| x == "bin")
                }
            })
    }
    assert!(
        has_bin_file(tmp.path()),
        "the cold solve must have written a .bin entry under the cache dir"
    );

    // ── Engine B: warm serve from the same cache dir ────────────────────────
    let mut engine_b = make_simple_engine();
    engine_b.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_b);
    let warm = engine_b.eval(&parse_and_compile_with_stdlib(source));

    // >= 1 rather than == 1: a shell-classified body may also persist a
    // shell-extract dispatch. miss == 0 is what proves no dispatch fell
    // through to a solve.
    assert_eq!(
        engine_b.persistent_miss_count(),
        0,
        "engine B must not fall through to a solve"
    );
    assert!(
        engine_b.persistent_hit_count() >= 1,
        "engine B must be served from the on-disk cache"
    );
    assert_eq!(
        warm.structured_detail, expected_detail,
        "the warm serve must replay exactly the cold solve's structured detail"
    );
}
