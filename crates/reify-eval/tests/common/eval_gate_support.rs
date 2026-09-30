//! Shared primitives every reify-eval corpus-wide invariant gate needs.
//!
//! Two helpers live here and nowhere else: the corpus walker
//! ([`collect_ri_files`]) and the ONE engine constructor those gates evaluate
//! through ([`gate_engine`]). Both are needed by more than one integration-test
//! crate root — `no_stale_undef_invariant_gate` (its deliberately-undef fixture
//! test, its task-5578/6662 `build()`-surface suite, and the
//! `survey_optimized_callers` walk) and `harness_corpus_gates`'s unified corpus
//! sweep — and a module cannot be shared across integration-test crate roots,
//! so each root declares this file with
//! `#[path = "common/eval_gate_support.rs"] mod eval_gate_support;`.
//!
//! That `#[path]` form, rather than `mod common;`, follows the established
//! `common/differential.rs` precedent (`harness_cache.rs`): declaring
//! `tests/common/mod.rs` would pull its 312 unrelated lines into every consumer's
//! compile unit for nothing. Like `differential.rs`, this file is deliberately
//! NOT declared in `common/mod.rs`, and carries its own `#![allow(dead_code)]`
//! because each consumer uses only a subset of it and the workspace builds under
//! `-D warnings`.
#![allow(dead_code)]

/// Recursively collect every `.ri` file under `dir` (including subdirectories).
/// Unreadable entries/directories are silently skipped — this only ever walks
/// our own repo directories, which are expected to be readable.
pub fn collect_ri_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// The ONE engine-construction site every corpus-wide invariant gate routes
/// through — the deliberately-undef fixture test, the unified corpus sweep's
/// eval (`harness_corpus_gates::eval_invariant_corpus_sweep::run_corpus_shard`
/// and `diag_per_file_timing`), and `build_surface_violations`'s build sweep.
///
/// It existed as three hand-copied blocks until the copies drifted (task 5578):
/// the build sweep registered the shell-extract trampolines and the eval sweep
/// did not, so the eval sweep was still sweeping
/// `examples/fea_shell_too_thick_annotated.ri` on a DEGRADED dispatch —
/// `@optimized target "shell-extract::extract": no registered compute trampoline`
/// — which is the very defect class those gates exist to catch. One constructor
/// is what keeps them from drifting again. Task 7431 widened that reach: the
/// INV-EVAL-4 snapshot↔cache divergence sweep used to build its own engine with
/// the bare `register_compute_fns` (a strict SUBSET), i.e. it was running in
/// exactly the degraded state described above; unifying it onto this constructor
/// is what stops the same drift recurring on that half.
///
/// The registered arm calls [`reify_eval::Engine::register_production_compute_fns`],
/// the canonical bundler production uses (`reify-cli`'s `configured_eval_engine`
/// routes through it), rather than hand-listing individual registrars: a NEW
/// production trampoline set then reaches these sweeps automatically. The
/// mesh-morph producer is `Unavailable` because `reify-eval`'s own tests do not
/// depend on `reify-mesh-morph`; it is a producer-side optimization, not a
/// dispatch target, so no `@optimized` target goes unregistered because of it.
///
/// `register_compute` is an explicit switch, not a convenience knob: `false` is
/// what lets `seeded_build_surface_sweep_reports_a_planted_violation` reproduce
/// the task-5578 defect in miniature. Only that self-test may pass `false`.
///
/// A fresh `Engine` per call, so `register_production_compute_fns`'s
/// panic-on-double-registration contract is never at risk.
pub fn gate_engine(register_compute: bool) -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(reify_test_support::MockGeometryKernel::new())),
    );
    if register_compute {
        engine.register_production_compute_fns(reify_eval::MorphRegistration::Unavailable {
            reason: "reify-eval's own test harness does not depend on reify-mesh-morph",
        });
    }
    engine
}
