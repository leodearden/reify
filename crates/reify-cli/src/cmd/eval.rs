//! `reify eval <file>`: evaluates the design and prints every top-level value cell.

use std::process::ExitCode;

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;

use crate::check_verdict::has_error_diagnostic;
use crate::constraint_report::format_undef_cause;
use crate::module_capabilities::module_has_geometry;
use crate::{configured_eval_engine, parse_and_compile};

/// `reify eval <file>` — parse, compile, evaluate, and print every
/// top-level value cell as `entity.member = value`.
///
/// This is the SIR-α user-observable signal (task 3540): structure
/// constructors evaluate to inspectable `Value::StructureInstance` values
/// (`TypeName { field: value, ... }` via `Value`'s `Display`) instead of
/// opaque `undef`. Cells are sorted for deterministic output.
///
/// The default [`reify_constraints::DimensionalSolver`] is wired so `auto`
/// params resolve: given box constraints and a `minimize`/`maximize` objective
/// the solver runs Nelder-Mead and prints the resulting numeric SI value
/// rather than `undef` (task 4132).
///
/// ## Geometry modules
///
/// When [`module_has_geometry`] detects geometry (realization ops or
/// `Geometry`-typed value cells), the engine is constructed with
/// [`reify_eval::Engine::with_registered_kernel`] and evaluation is routed
/// through [`reify_eval::Engine::realize_for_check`] (realization without the
/// Phase-B product-export walk) so that
/// `run_post_processes`/`post_process_geometry_queries` fires and lands
/// geometry-query value cells (e.g. `mass`, `centroid`) into `BuildResult.values`
/// (task 4145).  No product export is run — `reify eval` is a value-cell
/// inspector, not an exporter.
///
/// When the OCCT kernel is absent (`cfg(has_occt)` unset), the registered kernel
/// inventory is empty; `with_registered_kernel` returns a None-kernel engine and
/// `realize_for_check` skips the geometry pipeline — geometry-query cells stay
/// `undef` and exit code remains 0, matching `cmd_build`'s existing degradation
/// in stub mode.
///
/// When OCCT is present but geometry realization fails at runtime (e.g. all ops
/// fail in the kernel), `realize_for_check` emits an `Error`-severity diagnostic
/// and those errors **do** propagate to `cmd_eval`'s exit code.  This widening is
/// intentional: a file whose geometry is fundamentally broken should not silently
/// exit 0 with all geometry-query cells reported as `undef`.
///
/// Non-geometry modules use the existing
/// `Engine::new(None) + eval()` path unchanged.
pub(crate) fn cmd_eval(args: &[String]) -> ExitCode {
    // Parse args: walk the list to extract --explain-undef, --verbose,
    // --cache-dir <path>, and the file path.
    // Reject unknown flags so they are never silently misread as the file path.
    let mut explain_undef = false;
    let mut verbose = false;
    let mut cache_dir_override: Option<std::path::PathBuf> = None;
    let mut file_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--explain-undef" => {
                explain_undef = true;
                i += 1;
            }
            "--verbose" => {
                verbose = true;
                i += 1;
            }
            "--cache-dir" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --cache-dir requires a path argument");
                    eprintln!(
                        "Usage: reify eval [--explain-undef] [--verbose] [--cache-dir <path>] <file>"
                    );
                    return ExitCode::FAILURE;
                }
                cache_dir_override = Some(std::path::PathBuf::from(&args[i]));
                i += 1;
            }
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `eval`: {}", flag);
                eprintln!(
                    "Usage: reify eval [--explain-undef] [--verbose] [--cache-dir <path>] <file>"
                );
                return ExitCode::FAILURE;
            }
            path => {
                if file_path.is_some() {
                    eprintln!("Error: unexpected extra positional argument: {}", path);
                    return ExitCode::FAILURE;
                }
                file_path = Some(path);
                i += 1;
            }
        }
    }
    let Some(path) = file_path else {
        eprintln!(
            "Usage: reify eval [--explain-undef] [--verbose] [--cache-dir <path>] <file>"
        );
        return ExitCode::FAILURE;
    };

    let compiled = match parse_and_compile(path) {
        Ok(c) => c,
        Err(code) => return code,
    };

    if compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return ExitCode::FAILURE;
    }

    // Normalise both branches to (values, diagnostics, engine) for the shared
    // print loop.  The engine is hoisted into a binding so:
    //   (a) set_capture_undef_causes(true) fires before eval/build (A1-safe), and
    //   (b) trace_undef_causes can be called post-eval with the engine still alive.
    //
    // Both `eval` and `build` take `&mut self`, so the engine survives the call.
    let (values, diagnostics, engine) = if module_has_geometry(&compiled) {
        // Geometry-bearing module: route through the kernel-backed realization
        // path so that run_post_processes/post_process_geometry_queries fires and
        // resolves geometry-query value cells (mass, centroid, volume, …).
        // No geometry is emitted — reify eval is a value inspector only.
        //
        // `realize_for_check`, NOT `build()` (task 5318) — the same esc-5748-6
        // reason `cmd_check` gives at its two sites above: `build()` also runs
        // the Phase-B product-export walk, and `eval` writes no artifact, so
        // that walk's EXPORT-ONLY diagnostics are false errors here, and a false
        // EXIT — the tail of this function returns FAILURE on any
        // `Severity::Error`. The argument had simply never been carried across
        // to the other command that discards the artifact.
        //
        // Every value cell `build()` resolved here still resolves:
        // `realize_for_check` differs from `build` in the export walk and in
        // nothing else (both delegate to `build_with_geometry_output`, which
        // takes the export as a flag). Pinned, not assumed, by
        // `cli_gdt_integration_gate::b5_oracle_inside_oracles_agree`, which
        // asserts the `dev` and `pokeout` oracle CELLS parsed from this
        // command's stdout rather than merely its exit code.
        let mut engine = configured_eval_engine(|posture| {
            reify_eval::Engine::with_registered_kernel(posture, Box::new(SimpleConstraintChecker))
        });
        // Apply --cache-dir flag override (highest precedence over env/defaults set
        // by configured_eval_engine).
        if let Some(ref override_dir) = cache_dir_override {
            engine.set_persistent_cache_dir(Some(override_dir.clone()));
        }
        engine.set_capture_undef_causes(true);
        let result = engine.realize_for_check(&compiled);
        (result.values, result.diagnostics, engine)
    } else {
        // Plain numeric module: keep the existing lightweight eval() path so
        // non-geometry eval tests (cli_eval_auto_resolve, cli_stackup_eval,
        // cli_integration_smoke) remain on the exact unchanged code path.
        // Note: register_compute_fns is still required so `@optimized` targets
        // dispatch to their solver kernels (task 3794 / esc-3794-183).
        let mut engine = configured_eval_engine(|posture| {
            reify_eval::Engine::new(posture, Box::new(SimpleConstraintChecker), None)
        });
        // Apply --cache-dir flag override (highest precedence over env/defaults set
        // by configured_eval_engine).
        if let Some(ref override_dir) = cache_dir_override {
            engine.set_persistent_cache_dir(Some(override_dir.clone()));
        }
        engine.set_capture_undef_causes(true);
        let result = engine.eval(&compiled);
        (result.values, result.diagnostics, engine)
    };

    let mut cells: Vec<(String, String)> = values
        .iter()
        .map(|(id, v)| (format!("{}", id), format!("{}", v)))
        .collect();
    cells.sort();
    for (id, v) in &cells {
        println!("{} = {}", id, v);
    }

    for diag in &diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    // Emit undef notes: for each undef cell, report the complete root-cause
    // set from β's tracer.  Notes go to stderr so stdout stays parseable.
    //
    // Source selection (Q2 / §8.4 noise gate):
    //   DEFAULT         — undef cells in the printed `values` only (the
    //                     requested outputs the user sees; unbound input params
    //                     are absent from EvalResult.values so they are silenced
    //                     as subject lines while still appearing as because-causes
    //                     inside the wall_thickness note).
    //   --explain-undef — ALL undef cells in engine.snapshot().values, incl.
    //                     unbound input params and internal cells.
    let mut undef_cells: Vec<reify_core::ValueCellId> = if explain_undef {
        // Widen to ALL undef cells: iterate the full snapshot value map.
        engine
            .snapshot()
            .map(|snap| {
                snap.values
                    .iter()
                    .filter(|(_, (v, _))| v.is_undef())
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        // Default: only the undef cells that were printed to stdout.
        values
            .iter()
            .filter(|(_, v)| v.is_undef())
            .map(|(id, _)| id.clone())
            .collect()
    };
    undef_cells.sort_by_key(|id| id.to_string());
    for id in &undef_cells {
        let causes = engine.trace_undef_causes(id);
        if causes.is_empty() {
            continue;
        }
        let because = causes
            .iter()
            .map(format_undef_cause)
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("note: {id} is undef (because: {because})");
    }

    // Under --verbose, print a persistent-cache hit/miss summary to stderr so
    // users can confirm whether the FEA result was served from the on-disk cache
    // (hit) or required a fresh solve (miss).  Only emitted when a cache dir is
    // configured — avoids noise for non-FEA modules and lightweight check paths.
    if verbose && engine.persistent_cache_dir().is_some() {
        let hits = engine.persistent_hit_count();
        let misses = engine.persistent_miss_count();
        eprintln!("persistent-cache: {} hit(s), {} miss(es)", hits, misses);
    }

    if has_error_diagnostic(&diagnostics) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
