use std::process::ExitCode;

use reify_compiler::cfg::CfgSet;
use reify_constraints::SimpleConstraintChecker;
use reify_eval::TestStatus;

// Ensure reify_kernel_occt's object files are included in the link so its
// cfg(has_occt)-gated `inventory::submit!` fires and populates the global
// kernel registry used by `Engine::with_registered_kernel`.  An `extern crate`
// reference is more durable than a const read (which rustc may inline without
// emitting a symbol reference into the rlib); the linker passes the rlib
// unconditionally when the crate appears in `extern crate` position.
extern crate reify_kernel_occt as _;
// Ensure reify_kernel_manifold's object files are included in the link so its
// unconditional `inventory::submit!` fires and populates the global kernel
// registry with the Manifold entry.  Manifold's submit has no cfg gate (unlike
// OCCT's cfg(has_occt)), so this extern crate reference is always active and
// the "manifold" key is always present in the binary's registry.
extern crate reify_kernel_manifold as _;
// Ensure reify_kernel_openvdb's object files are included in the link under
// cfg(has_openvdb) so its `inventory::submit!` registration fires and
// populates the global kernel registry used by `ensure_openvdb_kernel`.
// Gated on `has_openvdb` to match the production registration gate
// (`cfg(any(has_openvdb, feature=stub_register))`; `stub_register` is
// test-only, so `has_openvdb` is exactly the production trigger).
// reify-cli's build.rs already emits `has_openvdb` + declares the check-cfg.
// No cfg gate is needed at the `ensure_openvdb_kernel()` call site — the
// method degrades internally when OpenVDB is absent from the registry (C1/D5).
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

mod args;
mod artifact_triangles;
mod cache;
mod check_verdict;
mod constraint_report;
mod dev;
mod module_capabilities;
mod sigpipe;
use args::{
    CHECK_USAGE, EXPLAIN_USAGE, PurposeActivation, build_cfg_set, doc_usage_error,
    parse_purpose_flag, parse_single_file_arg,
};
use artifact_triangles::{stl_triangle_count, threemf_triangle_count};
use check_verdict::{
    ConstraintOutcome, build_is_success, check_gating_error, dedup_diagnostics,
    drop_falsified_indeterminate_diagnostics, has_error_diagnostic, merge_build_diagnostics,
    merge_post_build_verdicts, strip_diagnostics_reproduced_by,
};
use constraint_report::{finish_check, format_undef_cause, report_eval_output};
use module_capabilities::{
    module_has_dfm_rule, module_has_geometric_conforms, module_has_geometry, module_has_isosurface,
    module_has_representation_within, module_has_thickness_dfm_rule,
};
use reify_core::{ModulePath, Severity};
use reify_ir::ExportFormat;

fn print_usage(out: &mut dyn std::io::Write) {
    let _ = writeln!(out, "Usage: reify <command> [options]");
    let _ = writeln!(out, "Commands:");
    let _ = writeln!(out, "  check <file>              Check constraints");
    let _ = writeln!(
        out,
        "  test <file>               Run @test-annotated structures"
    );
    let _ = writeln!(
        out,
        "  build <file> -o <output>   Build geometry and export"
    );
    let _ = writeln!(
        out,
        "  run <file>                Alias for eval (flagship: reify run <shell.ri>)"
    );
    let _ = writeln!(
        out,
        "  eval <file>               Evaluate and print every top-level value cell"
    );
    let _ = writeln!(
        out,
        "  report --bom <file>       Roll up a BOM / cost / waste / provenance report"
    );
    let _ = writeln!(
        out,
        "  lsp                        Start language server (stdin/stdout)"
    );
    let _ = writeln!(
        out,
        "  gui [--debug] <file>       Open file in GUI (--debug enables MCP debug listener)"
    );
    let _ = writeln!(
        out,
        "  gui-debug <file>           Open file in GUI with debug MCP listener (alias for `gui --debug`)"
    );
    let _ = writeln!(
        out,
        "  doc <file> [-o <path>] [--format html|markdown|json] [--split] [--compact]  Generate documentation"
    );
    let _ = writeln!(
        out,
        "  cache export <hash>        Write a single cache entry to stdout as a tarball"
    );
    let _ = writeln!(
        out,
        "  cache import               Read a cache tarball from stdin into the local cache"
    );
    let _ = writeln!(
        out,
        "  cache stats                Print cache directory, entry count, total size, and top-N largest entries"
    );
    let _ = writeln!(
        out,
        "  cache clear [--engine-version <hash>] --yes  Empty the cache (or one engine-version subdir); --yes required"
    );
    let _ = writeln!(
        out,
        "  cache gc                   Force LRU eviction down to the configured cache cap (live engine version only)"
    );
    let _ = writeln!(
        out,
        "  explain <file>             Print per-cell objective provenance (B9 triple)"
    );
    let _ = writeln!(
        out,
        "  dev inspect-node <node-id> Print node kind, traits, priority, policy, and overrides"
    );
    let _ = writeln!(out, "  --version                  Print version");
    let _ = writeln!(out, "  --help                     Show this list");
}

fn main() -> ExitCode {
    sigpipe::restore_default();
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage(&mut std::io::stderr());
        return ExitCode::FAILURE;
    }

    // (a) Early-exit arms: --help / --version short-circuit before the sweep.
    match args[1].as_str() {
        "--help" | "-h" | "help" => {
            print_usage(&mut std::io::stdout());
            return ExitCode::SUCCESS;
        }
        "--version" | "-V" => {
            println!("reify {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    // (b) Sweep stale tempfiles and orphan dirs from the persistent cache.
    // Best-effort: resolver errors are silently ignored. Runs once here so
    // all engine-using subcommands inherit the cleanup without per-command
    // wiring (task 3698).
    cache::run_startup_sweep();

    // (c) Command dispatcher.
    match args[1].as_str() {
        "check" => cmd_check(&args[2..]),
        "test" => cmd_test(&args[2..]),
        "build" => cmd_build(&args[2..]),
        "run" | "eval" => cmd_eval(&args[2..]),
        "report" => cmd_report(&args[2..]),
        "doc" => cmd_doc(&args[2..]),
        "lsp" => cmd_lsp(),
        "gui" => cmd_gui(&args[2..]),
        "gui-debug" => {
            // `gui-debug` is sugar for `gui --debug`: prepend the flag and
            // route through the same code path as `cmd_gui` so the two entry
            // points share argument parsing and binary-launch logic.
            let mut forwarded: Vec<String> = Vec::with_capacity(args.len() - 1);
            forwarded.push("--debug".to_string());
            forwarded.extend(args[2..].iter().cloned());
            cmd_gui(&forwarded)
        }
        "explain" => cmd_explain(&args[2..]),
        "dev" => dev::cmd_dev(&args[2..]),
        "cache" => cache::cmd_cache(&args[2..]),
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage(&mut std::io::stderr());
            ExitCode::FAILURE
        }
    }
}

/// Write already-rendered parse errors to `out` and yield the exit code a parse failure
/// gets, so both entry points cannot drift apart on the prefix or the code.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// Takes the RENDERED `line:col: message` strings rather than the `ParsedModule` they came
/// from: `reify-cli` does not depend on `reify-ast`, so naming that type in a signature would
/// have to go through `reify-syntax`'s `pub use reify_ast::*` block, which is marked TRANSIENT
/// and slated for removal by the PRD task η follow-up. Callers reach the renderer by method
/// resolution instead, which needs no crate path — and `ParsedModule::render_errors` is where
/// the reason for rendering the whole list at once is documented.
///
/// `out` is injected rather than written as `eprintln!`, matching
/// `report_constraint_results` in constraint_report.rs: the position prefix is the whole point
/// of the rendering step, and a test that cannot read what was written can only assert that
/// SOMETHING mentioning "Parse error" reached stderr — which passes just as well with the
/// position gone.
fn report_parse_errors(rendered: Vec<String>, out: &mut impl std::io::Write) -> ExitCode {
    for error in rendered {
        let _ = writeln!(out, "Parse error: {error}");
    }
    ExitCode::FAILURE
}

fn parse_and_compile(path: &str) -> Result<reify_compiler::CompiledModule, ExitCode> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path, e);
            return Err(ExitCode::FAILURE);
        }
    };

    // file_stem() strips only the last extension: "foo.ri" → "foo". Dotted stems
    // like "v1.2" (from a file named "v1.2.ri") yield a single-segment ModulePath
    // ["v1.2"], which will mismatch a `module v1.2` declaration (parsed as ["v1","2"]).
    // This is a known limitation: Reify module names are expected to be bare identifiers.
    let module_name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed");

    let parsed = reify_compiler::parse_with_stdlib(&source, ModulePath::single(module_name));

    if !parsed.errors.is_empty() {
        return Err(report_parse_errors(
            parsed.render_errors(&source),
            &mut std::io::stderr(),
        ));
    }

    let mut compiled = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);

    // Enforce module-path declaration (spec §7.1/§7.2, task γ).
    // parsed.path == ModulePath::single(module_name) by construction (PRD D-6).
    if let Some(diag) =
        reify_compiler::check_module_path_decl(parsed.declared_module_path.as_ref(), &parsed.path)
    {
        compiled.diagnostics.push(diag);
    }

    for diag in &compiled.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    Ok(compiled)
}

/// Like [`parse_and_compile`], but seeds the active [`CfgSet`] and walks a
/// `#cfg(...)`-gated user-import DAG via
/// [`reify_compiler::module_dag::compile_entry_with_stdlib_cfg`].
///
/// Used only by `reify check`. It preserves single-file behavior — the full
/// stdlib prelude is still seeded, so every existing `reify check` input keeps
/// resolving stdlib names — while additionally following the entry's
/// cfg-satisfied user imports, so `--cfg target=...` selects which platform
/// modules resolve (task δ's user-observable signal).
///
/// The module-path declaration check (spec §7.1/§7.2, task γ) is performed
/// *inside* `compile_entry_with_stdlib_cfg` (via `attach_module_path_diag`), so
/// — unlike [`parse_and_compile`] — this function must NOT re-run it, else the
/// diagnostic would be emitted twice.
fn parse_and_compile_with_cfg(
    path: &str,
    cfg: &CfgSet,
) -> Result<reify_compiler::CompiledModule, ExitCode> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path, e);
            return Err(ExitCode::FAILURE);
        }
    };

    // file_stem() strips only the last extension — same module-name derivation
    // as parse_and_compile (see its comment for the dotted-stem limitation).
    let module_name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed");

    let parsed = reify_compiler::parse_with_stdlib(&source, ModulePath::single(module_name));

    if !parsed.errors.is_empty() {
        return Err(report_parse_errors(
            parsed.render_errors(&source),
            &mut std::io::stderr(),
        ));
    }

    // Resolve sibling user imports relative to the entry file's parent dir.
    //
    // `stdlib_root` is INERT on this code path: `compile_entry_with_stdlib_cfg`
    // skips every `std.*` import (the full stdlib is seeded into the prelude via
    // `load_stdlib()` instead), so the resolver's stdlib_root is never consulted
    // for a `reify check`. We still pass the GUI/LSP-heuristic path
    // (parent/crates/reify-compiler/stdlib) rather than a bogus sentinel so the
    // resolver is constructed identically to that bridge; its value has no
    // observable effect here.
    let parent_dir = std::path::Path::new(path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let resolver = reify_compiler::module_dag::ModuleResolver::new(
        parent_dir,
        parent_dir.join("crates/reify-compiler/stdlib"),
    );

    let compiled = reify_compiler::module_dag::compile_entry_with_stdlib_cfg_checked(
        &parsed,
        &resolver,
        cfg,
        &SimpleConstraintChecker,
    );

    for diag in &compiled.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    Ok(compiled)
}

/// `reify check <file>` — lightweight static constraint checker.
///
/// ## Engine posture: deliberately NO compute trampolines
///
/// The non-[`RepresentationWithin`] path uses `Engine::new(None) + check()`;
/// the [`RepresentationWithin`] path uses `Engine::with_registered_kernel +
/// check()`.  Neither path calls [`configured_eval_engine`] nor registers the
/// FEA/buckling/modal compute trampolines
/// ([`register_compute_trampolines`]).
///
/// Consequence: `@optimized("solver::elastic_static")` FEA-result constraints
/// (e.g. `constraint peak_stress < limit` over `result.max_von_mises`) evaluate
/// against the body-inline `undef` fallback and report **Indeterminate** under
/// `reify check`.  They are NOT a gate; `reify build` or `reify eval` are the
/// FEA exit-code gate.
///
/// **Rationale:** registering compute trampolines here would run a potentially
/// slow FEA solve inside the lightweight static-check path, violating the design
/// intent that *check attaches no kernel by design*.  The trampoline-free posture
/// is an executable contract locked by `check_fea_violated_constraint_is_not_gated`
/// in `cli_build_fea.rs`; changing it requires updating that test intentionally.
///
/// **Severity of the missing-trampoline diagnostic (task 5311):** `reify check`
/// surfaces the engine-owned "no registered compute trampoline (falling back to
/// body-inlining)" diagnostic on stderr for `@optimized` FEA solves at
/// `Severity::Warning`, carrying
/// `DiagnosticCode::NoRegisteredComputeTrampoline`.  The engine conditions that
/// severity on its compute registry being entirely EMPTY, which is exactly this
/// function's posture — `cmd_check` never calls `register_compute_trampolines`,
/// so a missing trampoline here is the declared posture rather than a defect.
/// `reify eval` and `reify build` DO register the production bundle, so the
/// same diagnostic stays `Severity::Error` there and keeps gating their exit
/// codes.  The contrast is pinned by
/// `check_downgrades_unregistered_trampoline_fallback_to_warning_while_eval_and_build_keep_erroring`
/// in `crates/reify-cli/tests/harness_cli/cli_check.rs`.
///
/// **Other `error:` lines.** Every other `Severity::Error` diagnostic makes
/// `check` exit non-zero through [`check_gating_error`] unless a
/// `check_verdict::CHECK_ERROR_EXIT_ALLOWLIST` entry excuses it.  The residual shapes across
/// `examples/**/*.ri` are inventoried in #7308 (with the sweep command that
/// measures them), which owns triaging them — never by adding allowlist
/// entries.
fn cmd_check(args: &[String]) -> ExitCode {
    // Flag walk modeled on cmd_doc/cmd_gui: explicit handling of known flags
    // and explicit rejection of unknown `--`-prefixed tokens so a typo like
    // `--purpouse` fails loud instead of being silently treated as a file path.
    let mut purpose_values: Vec<String> = Vec::new();
    let mut cfg_values: Vec<String> = Vec::new();
    let mut strict = false;
    let mut file: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--strict" => {
                strict = true;
                i += 1;
            }
            "--purpose" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --purpose requires a value");
                    eprintln!("{}", CHECK_USAGE);
                    return ExitCode::FAILURE;
                }
                purpose_values.push(args[i + 1].clone());
                i += 2;
            }
            "--cfg" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --cfg requires a value");
                    eprintln!("{}", CHECK_USAGE);
                    return ExitCode::FAILURE;
                }
                cfg_values.push(args[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `check`: {}", flag);
                eprintln!("{}", CHECK_USAGE);
                return ExitCode::FAILURE;
            }
            _ => {
                if file.is_none() {
                    file = Some(a);
                }
                i += 1;
            }
        }
    }

    let file = match file {
        Some(f) => f,
        None => {
            eprintln!("{}", CHECK_USAGE);
            return ExitCode::FAILURE;
        }
    };

    // Build the active cfg from the repeated `--cfg` values: target is
    // host-defaulted and overridable only by `--cfg target=<v>` (PRD §4 D-2).
    let cfg = match build_cfg_set(&cfg_values) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let compiled = match parse_and_compile_with_cfg(file, &cfg) {
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

    if purpose_values.is_empty() {
        // No --purpose flag: route through the appropriate check path.
        //
        // Four routing kinds need live kernel state, and a single module may
        // carry any combination:
        //   * RepresentationWithin (task-4199 γ) — needs
        //     `set_capture_repr_tol(true)` + `tessellate_realizations` to
        //     populate `achieved_repr_tol`, which `dispatch_constraints` reads.
        //   * geometric GD&T `Conforms` (η/4480) — needs
        //     `build(ExportFormat::Step)` to realize live B-rep handles into
        //     `realization_handles` (a `MaxDeviation` query is BRepOnly; only
        //     `build()` — not `tessellate_realizations` — populates that map),
        //     which `measure_gdt_conformance` reads.
        //   * DFMRule (task-4600, γ/4408) — also needs
        //     `build(ExportFormat::Step)` to populate `realization_handles`.
        //     `measure_dfm_rules` (engine_constraints.rs) reads
        //     `self.realization_handles` to assign each rule's `subject_handle`;
        //     without `build()` the handle is None and the rule is silently
        //     skipped.  C1 (no OCCT → None-kernel → build realizes nothing →
        //     measure_dfm_rules C1 guard fires → no output, exit 0).
        //     C2 (modules without DFMRule see has_dfm_rule=false →
        //     byte-identical to their previous path).
        //   * module has geometry (task 5748, PRD
        //     docs/prds/v0_6/check-diagnostic-truthfulness.md leaf β D1) — a
        //     module whose templates carry a realization op or a
        //     `Type::Geometry` value cell ([`module_has_geometry`], whose doc
        //     contract spells out both signals) needs
        //     `build(ExportFormat::Step)` for a DIFFERENT reason than the three
        //     above: geometry-query value cells (`centroid`,
        //     `moment_of_inertia`, `mass`, …) are populated only by
        //     `run_post_processes`/`post_process_geometry_queries`, which run
        //     on the build()/tessellate() path. Without it those cells stay
        //     `undef` and every constraint reading one degrades to
        //     Indeterminate — a check that is silent about geometry it never
        //     realized. `cmd_eval` already routes this way (its
        //     `module_has_geometry` branch is the precedent); this makes
        //     `check` agree. Same predicate, no new detection logic.
        //
        // amend (reviewer suggestion: robustness_routing) — these were
        // previously two mutually-exclusive `else if` arms with geometric
        // `Conforms` first, so a module carrying BOTH kinds ran only `build()`;
        // `set_capture_repr_tol`/`tessellate_realizations` never fired and every
        // RepresentationWithin silently degraded to Indeterminate. They are now
        // a single kernel-backed arm that runs EACH kind's side effect when that
        // kind is present. The side effects touch DISJOINT engine maps and
        // neither clears the other's (`build()` clears+repopulates
        // `realization_handles` but never touches `achieved_repr_tol`;
        // `tessellate_realizations()` is the exact converse), so a combined
        // module gets a correct verdict for each kind. Each single-kind module
        // runs the identical sequence it did before (C2 — byte-identical for all
        // existing inputs).
        //
        // C1 graceful degradation: with no OCCT kernel,
        // `with_registered_kernel` returns a None-kernel engine → build /
        // tessellate realize nothing → all three kinds yield Indeterminate or
        // are skipped (never a false Violated or false W_DFM_OVERHANG) → exit 0.
        //
        // When the module has NONE of the four kinds, keep the existing
        // `Engine::new(None)+check()` path verbatim (C2).
        let checker = SimpleConstraintChecker;
        let has_geometric_conforms = module_has_geometric_conforms(&compiled);
        let has_representation_within = module_has_representation_within(&compiled);
        let has_dfm_rule = module_has_dfm_rule(&compiled);
        let has_thickness_dfm = module_has_thickness_dfm_rule(&compiled);
        let has_geometry = module_has_geometry(&compiled);
        // BOTH nested gates below learn `has_geometry`, and both are
        // load-bearing (task 5748): extending only the outer one would put a
        // geometry-only module on the kernel-backed engine but never call
        // `build()`, so `run_post_processes`/`post_process_geometry_queries`
        // would never fire and the geometry-query cells would stay `undef` —
        // the routing change would be observably inert.
        // Captured by the kernel-backed arm's `build()` call below (None on the
        // lightweight arm, and on a kernel-backed module that takes only the
        // `tessellate_realizations` side effect). Consumed by
        // `merge_post_build_verdicts` after the authoritative `check()`.
        let mut build_result: Option<reify_eval::BuildResult> = None;
        let result = if has_geometric_conforms
            || has_representation_within
            || has_dfm_rule
            || has_geometry
        {
            let mut engine = reify_eval::Engine::with_registered_kernel(reify_eval::SolverPosture::SolverFree, Box::new(checker));
            if has_representation_within {
                // Record deviation during tessellation.
                engine.set_capture_repr_tol(true);
            }
            if has_geometric_conforms || has_dfm_rule || has_geometry {
                // Realize live B-rep handles into `realization_handles`. The
                // build result is discarded; only its handle-population side
                // effect matters. Run BEFORE `tessellate_realizations` —
                // `build()` clears+repopulates `realization_handles` but does
                // not touch `achieved_repr_tol`, so the tessellate pass below
                // leaves these handles intact.
                //
                // DFMRule (has_dfm_rule): `measure_dfm_rules` reads
                // `realization_handles` to set each rule's `subject_handle`
                // and skips rules where the handle is None — the same
                // precondition as geometric Conforms.
                //
                // has_geometry (task 5748): `build()` additionally runs
                // `run_post_processes`/`post_process_geometry_queries`, which
                // is the ONLY thing that resolves geometry-query value cells
                // (`centroid`, `moment_of_inertia`, …).
                //
                // The `BuildResult` is CAPTURED, not discarded: its
                // `constraint_results` are the only post-realization verdicts
                // available here, and `merge_post_build_verdicts` below adopts
                // them where check() had nothing better. See that helper.
                //
                // `realize_for_check`, NOT `build()` (esc-5748-6): `build()`
                // also runs the Phase-B product-export walk, whose EXPORT-ONLY
                // diagnostics were harmless while the whole `BuildResult` was
                // discarded but are false errors now that the merge is live —
                // `check` exports nothing.
                //
                // COST, recorded not paid down (task 5748 → #5973): adding
                // `has_geometry` to this gate means a plain geometry module —
                // which previously took the lightweight `Engine::new(None) +
                // check()` path — now evaluates the module at least TWICE per
                // `reify check`: `realize_for_check` →
                // `build_with_geometry_output` opens with `self.check(module)`
                // (which opens with `self.eval`), and the authoritative
                // `engine.check(&compiled)` below opens with another fresh
                // `self.eval`. That second eval is also the sole reason
                // `merge_post_build_verdicts` exists — it discards the
                // post-processed value map the realization just produced, and
                // the merge papers over exactly that discard. Sub-path (c)
                // shows the alternative: it hands the realization's own values
                // to `check_constraints_with_values` and needs no verdict merge
                // at all. Moving this arm onto that shape would delete the
                // redundant eval, the verdict merge AND the (b)/(c)
                // composition asymmetry together, but it changes which passes
                // run on the check path — a behaviour change this leaf is not
                // scoped to make.
                //
                // OWNER: #5973 ("push the post-realization constraint re-check
                // down into Engine::check(), retiring cmd_check's CLI-side
                // merge_post_build_verdicts"), which is option (C) from
                // esc-5748-1's steward ruling — the ruling took the CLI-side
                // merge precisely BECAUSE (C) fell outside 5748's file scope.
                // The double eval, the verdict merge and the (b)/(c) asymmetry
                // all retire together there.
                build_result = Some(engine.realize_for_check(&compiled));
            }
            if has_representation_within {
                // Populate `achieved_repr_tol`. Does not touch
                // `realization_handles`, so the build handles above survive.
                engine.tessellate_realizations(&compiled);
            }
            if has_thickness_dfm {
                // Lazily acquire the OpenVDB kernel for the thickness-DFM
                // sub-case.  `module_has_thickness_dfm_rule` detected a
                // process conformer carrying `min_feature_size : Length` →
                // the thickness arm of `measure_dfm_rules` needs the SDF.
                // Registry-driven (§3.3 "registry not ad-hoc"), idempotent,
                // leaves `default_kernel_name` = OCCT (realize_solid_sdf
                // tessellates via default, voxelizes via openvdb_kernel_name).
                // cfg(not(has_openvdb)) → registry lacks "openvdb" → returns
                // false → no-op (C1/D5: Indeterminate, no false violation).
                engine.ensure_openvdb_kernel();
            }
            // `check()` runs `measure_gdt_conformance` (overrides the matching
            // scalar `Conforms` entry with the measured verdict),
            // `dispatch_constraints`' RepresentationWithin interception, and
            // `measure_dfm_rules` (emits W_/E_DFM_OVERHANG, W_/E_DFM_DRAFT,
            // W_/E_DFM_MIN_WALL, W_/E_DFM_MIN_FEATURE diagnostics) — each
            // reads the map its side effect populated.
            engine.check(&compiled)
        } else {
            // Existing lightweight path: no kernel, no tessellation (C2).
            let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), None);
            engine.check(&compiled)
        };

        let mut result = result;
        merge_post_build_verdicts(&mut result, build_result.as_ref());

        // D2 (task 5748): `build()`'s diagnostics are no longer discarded.
        // `check()` alone never produces the realization-only entries
        // (`compile_geometry_op` gating, kernel-dispatch failures), so without
        // this merge a module whose geometry cannot compile at all still
        // reported "All constraints satisfied." under `check`.
        //
        // ORDERING IS LOAD-BEARING and both legs run AFTER
        // `merge_post_build_verdicts`, so the filter reads the POST-upgrade
        // verdicts and the merge cannot re-append a warning the upgrade just
        // falsified. Composed and pinned in that order by
        // `d2_pass_ordering_tests::upgraded_constraints_warning_survives_in_
        // neither_list`.
        //
        // A `None` build result (the lightweight arm, and the kernel-backed
        // sub-case that takes only the `tessellate_realizations` side effect)
        // yields an empty slice, so those paths stay byte-identical (C2).
        let build_diagnostics: Vec<reify_core::Diagnostic> =
            drop_falsified_indeterminate_diagnostics(
                build_result
                    .as_ref()
                    .map(|b| b.diagnostics.as_slice())
                    .unwrap_or(&[]),
                &result.constraint_results,
            );
        let merged_diagnostics = merge_build_diagnostics(&result.diagnostics, &build_diagnostics);

        let outcome = report_eval_output(
            &result.constraint_results,
            &merged_diagnostics,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        let exit = finish_check(
            &outcome,
            &result.constraint_results,
            strict,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        // INV-SF-2: any `Severity::Error` in the set `report_eval_output` just
        // showed the user makes `check` exit non-zero, unless a
        // `CHECK_ERROR_EXIT_ALLOWLIST` entry excuses it.
        //
        // Gated on `merged_diagnostics`, NOT `result.diagnostics`, so a
        // realization-only Error moves the exit code too — e.g. the
        // post-geometry harvest's `E_DFM_BUILD_VOLUME`, which check()'s own
        // list never carries. Pinned end to end by
        // `cli_check.rs::check_exits_nonzero_on_a_realization_only_build_volume_error`.
        //
        // Runs AFTER `finish_check`, so stdout is unchanged and only the exit
        // code escalates. Warnings (GdtRemoved2018, W_DFM_*) never gate (C1).
        if check_gating_error(&merged_diagnostics).is_some() {
            return ExitCode::FAILURE;
        }

        exit
    } else {
        // --purpose path: replicates the canonical
        // eval → activate_purpose → check_constraints_with_values sequence.
        // engine.check() does NOT visit purpose-injected constraints —
        // they live in snapshot.graph.constraints, visited only by
        // check_constraints_with_values.
        //
        // GD&T legality is enforced on BOTH paths via `engine.run_gdt_check_passes`
        // (task 4589): diagnostics are folded in before `report_eval_output` below,
        // so the exit gate after `finish_check` sees a `GdtIllegalModifier` Error
        // exactly as the no-purpose path does.

        // Parse all --purpose values up front so a malformed value fails
        // before we touch the engine.
        let mut activations: Vec<PurposeActivation> = Vec::with_capacity(purpose_values.len());
        for value in &purpose_values {
            match parse_purpose_flag(value) {
                Ok(a) => activations.push(a),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return ExitCode::FAILURE;
                }
            }
        }

        let checker = SimpleConstraintChecker;
        // D1 item 2 (task 5748): a geometry-bearing module goes through
        // `build()` here, exactly as `cmd_eval` already does (main.rs, its
        // `module_has_geometry`-gated branch). Without it this branch realizes
        // nothing, so `compile_geometry_op` diagnostics are never even PRODUCED
        // and geometry-query value cells stay `undef`.
        //
        // `Engine::with_registered_kernel` (engine_admin.rs) attaches a
        // GEOMETRY kernel only. `configured_eval_engine` is deliberately NOT
        // used: it calls `register_compute_trampolines` and wires a solver +
        // persistent FEA cache, and `check` stays compute-trampoline-free by
        // design (see the design-intent comment on `cmd_check` above, and the
        // regression lock `check_fea_violated_constraint_is_not_gated`). The
        // geometry-kernel axis and the compute-trampoline axis are independent;
        // this leaf moves only the former.
        //
        // The engine must outlive the branch: `activate_purpose` /
        // `activate_purpose_with_bindings` / `is_purpose_active` /
        // `check_constraints_with_values` / `run_gdt_check_passes` are all
        // called on it below.
        let used_build = module_has_geometry(&compiled);
        let (values, front_end_diagnostics, mut engine) = if used_build {
            let mut engine = reify_eval::Engine::with_registered_kernel(reify_eval::SolverPosture::SolverFree, Box::new(checker));
            // `realize_for_check`, not `build()` — same esc-5748-6 reason as
            // sub-path (b) above: `check` writes no artifact, so the Phase-B
            // export walk's diagnostics would be user-visible false errors.
            let r = engine.realize_for_check(&compiled);
            (r.values, r.diagnostics, engine)
        } else {
            let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), None);
            let r = engine.eval(&compiled);
            (r.values, r.diagnostics, engine)
        };

        // Activate each purpose in flag order; one check_constraints_with_values
        // call after the loop collects results for ALL injected constraints.
        for activation in &activations {
            // Single-binding form (name=entity, param==None): route through the
            // activate_purpose(name, entity) shim — byte-identical @entity prefix,
            // preserves existing single-param CLI tests (C6).
            // Everything else (len>=2, or len==1 with a named param like part:PartA):
            // route through activate_purpose_with_bindings for C2/C3 validation.
            let is_bare_single =
                activation.bindings.len() == 1 && activation.bindings[0].param.is_none();

            if is_bare_single {
                engine.activate_purpose(&activation.name, &activation.bindings[0].entity);

                // activate_purpose is silent on unknown-purpose, missing eval_state,
                // and the C2 multi-param refusal. is_purpose_active is the only
                // programmatic signal — a false result surfaces all failure modes.
                if !engine.is_purpose_active(&activation.name) {
                    eprintln!(
                        "Error: could not activate purpose '{}' (no such purpose in the file, or it requires per-param bindings)",
                        activation.name
                    );
                    return ExitCode::FAILURE;
                }
            } else {
                // Multi-binding requires every binding to name its param
                // (`part:PartA`). A bare segment mixed in (`PartA,envelope:BoxB`)
                // — or an all-bare multi value (`PartA,BoxB`) — would forward an
                // empty param string below and surface as the unactionable
                // "has no parameter ''" engine diagnostic. parse_purpose_flag is
                // the first line of defense (it rejects a bare segment in a
                // len>=2 value), so this is currently unreachable via the CLI;
                // we guard here too so cmd_check stays self-consistent and never
                // forwards an empty param if the parser is ever loosened.
                if activation.bindings.iter().any(|b| b.param.is_none()) {
                    eprintln!(
                        "Error: purpose '{}' has multiple bindings; name every parameter (e.g. 'part:PartA,envelope:BoxB')",
                        activation.name
                    );
                    return ExitCode::FAILURE;
                }
                let pairs: Vec<(String, String)> = activation
                    .bindings
                    .iter()
                    .map(|b| (b.param.clone().unwrap_or_default(), b.entity.clone()))
                    .collect();
                if let Err(e) = engine.activate_purpose_with_bindings(&activation.name, &pairs) {
                    eprintln!("Error: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }

        // `check_constraints_with_values` takes the value map as an ARGUMENT and
        // never re-runs eval, so on the geometry branch the verdicts below are
        // computed directly against the realization's POST-processed values.
        // The staleness gap that forced
        // `merge_post_build_verdicts` on sub-path (b) — where `Engine::check()`
        // opens with a fresh `self.eval(module)` and throws build()'s values
        // away — simply does not exist here, so no verdict merge is needed.
        let (constraint_results, check_diags) =
            match engine.check_constraints_with_values(&values) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return ExitCode::FAILURE;
                }
            };

        // GD&T legality pass (task 4589): runs over post-eval values identically
        // to the non-purpose branch.  Computed HERE, ahead of the diagnostic
        // assembly, because the `used_build` arm needs it to identify the
        // realization's redundant copy of the same pass (see
        // `strip_diagnostics_reproduced_by`).  Folded in below, before
        // `report_eval_output`, so the error prints to stderr alongside the
        // others.
        let gdt_diagnostics = engine.run_gdt_check_passes(&compiled, &values);

        // Front-end diagnostics first, then check diagnostics — chronological order.
        let mut diagnostics = if used_build {
            // D2 (task 5748) for sub-path (c). The realization re-runs the eval
            // front-end internally, so its list BOTH carries internal
            // duplicates (measured on `tests/fixtures/mirror_bare_origin.ri`:
            // `failed to compile geometry operation: mirror: expected a Plane
            // value, got undef` twice for one call site — re-measured at task
            // 5746, which moved that fixture's rejection to the producer and
            // with it the text of the duplicated line, not the duplication)
            // AND overlaps what
            // `check_constraints_with_values` reports; the build entries seed
            // the list and the check entries merge in behind them, so an entry
            // produced by both passes is reported exactly once.
            //
            // Same return leg as sub-path (b), applied symmetrically so the two
            // cannot drift: build's stale `ConstraintIndeterminate` claims are
            // filtered against the AUTHORITATIVE `constraint_results` first. The
            // known divergence path here is `engine_fixpoint`'s UnifiedDag
            // `declined` set, whose constraints keep build's Indeterminate
            // warning while the CLI-side check still reaches a definite verdict.
            //
            // Composition pinned by `d2_pass_ordering_tests::
            // run_in_cmd_check_purpose_order`'s tests.
            let front_end_diagnostics = drop_falsified_indeterminate_diagnostics(
                &front_end_diagnostics,
                &constraint_results,
            );
            // The realization's list also carries a COPY of the GD&T legality
            // pass appended below. Withdraw it before the dedup rather than
            // letting the dedup collapse the two runs: that pass can emit two
            // byte-identical lines for two distinct callouts, which the dedup
            // key cannot tell from a re-run, so deduping printed ONE line where
            // the non-geometry arm printed two. With the copy withdrawn, the
            // fold below is the single source and multiplicity is right by
            // construction (see `strip_diagnostics_reproduced_by`).
            let realization_only =
                strip_diagnostics_reproduced_by(&front_end_diagnostics, &gdt_diagnostics);
            let deduped_build = dedup_diagnostics(&realization_only);
            merge_build_diagnostics(&deduped_build, &check_diags)
        } else {
            // Non-geometry: plain chronological concatenation, byte-identical
            // to pre-5748 (C2). Deliberately NOT routed through the merge —
            // `eval()` does not re-run itself, so there is no duplication to
            // collapse, and running the dedup here could only ever REMOVE a
            // line this branch prints today.
            let mut d = front_end_diagnostics.clone();
            d.extend(check_diags);
            d
        };

        // BOTH arms append the legality pass's output the same plain way, so
        // multiplicity is whatever this one authoritative run produced —
        // one line per callout, two distinct callouts of the same
        // characteristic included. The `used_build` arm's realization carried a
        // second copy of the same pass (`realize_for_check` →
        // `build_with_geometry_output` seeds its diagnostic list from
        // `Engine::check`, which ends by extending with `run_gdt_check_passes`);
        // that copy was already withdrawn above, so this `extend` cannot double
        // anything. esc-5748-7 first closed the doubling with an identity merge
        // instead, which was over-strong for the same reason the dedup is.
        diagnostics.extend(gdt_diagnostics);

        let outcome = report_eval_output(
            &constraint_results,
            &diagnostics,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        // Same outcome → summary + exit-code mapping as the no-purpose path,
        // so a purpose-injected violation behaves identically to a structure
        // constraint violation in stdout and shell exit semantics.
        let exit = finish_check(
            &outcome,
            &constraint_results,
            strict,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        // INV-SF-2: the same gate as the no-purpose branch, over this branch's
        // already-merged, already-reported list — the build/eval front end,
        // `check_constraints_with_values` and `run_gdt_check_passes` folded
        // together above — and placed after `finish_check` for the same
        // reason. `cli_check.rs::check_purpose_gate_matches_the_no_purpose_gate`
        // asserts the two paths' exit codes agree.
        if check_gating_error(&diagnostics).is_some() {
            return ExitCode::FAILURE;
        }

        exit
    }
}

fn cmd_test(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify test <file>");
        return ExitCode::FAILURE;
    }

    let compiled = match parse_and_compile(&args[0]) {
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

    let results = reify_eval::run_tests(&compiled, || Box::new(SimpleConstraintChecker));

    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut indeterminate: usize = 0;

    for result in &results {
        let label = match result.status {
            TestStatus::Pass => {
                passed += 1;
                "PASS"
            }
            TestStatus::Fail => {
                failed += 1;
                "FAIL"
            }
            TestStatus::Indeterminate => {
                indeterminate += 1;
                "INDETERMINATE"
            }
        };
        println!("  {}  {}", label, result.name);
    }

    let overall = if failed > 0 { "FAIL" } else { "ok" };
    println!(
        "test result: {}. {} passed; {} failed; {} indeterminate",
        overall, passed, failed, indeterminate
    );

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_build(args: &[String]) -> ExitCode {
    // Shared usage text for the empty-args and no-positional-file guards.
    const USAGE: &str = "Usage: reify build <file.ri> [-o <output>] [--out-dir <dir>] [--verbose]\n  \
        With -o:    write a single file in that format (imperative).\n  \
        Without -o: every `: Output` occurrence in the design drives its own file\n              \
        (declarative); each relative path resolves against the .ri file's\n              \
        directory, or against --out-dir when given.";

    if args.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    }

    // Detect `--verbose` anywhere in the args.
    let verbose = args.iter().any(|a| a == "--verbose");

    // Pre-compute the index of the value that follows `-o` (if present and
    // followed by an argument).  This is reused both to build `output_path`
    // and to exclude the `-o` value from the positional-file scan below so
    // that `reify build -o out.step file.ri` doesn't mistakenly treat
    // `out.step` as the input file.
    let o_value_pos: Option<usize> = args.iter().position(|a| a == "-o").and_then(|i| {
        if i + 1 < args.len() {
            Some(i + 1)
        } else {
            None
        }
    });

    // Likewise for `--out-dir <dir>` (declarative mode's CI escape hatch): its
    // value must be excluded from the positional-file scan so it is never
    // mistaken for the input file. The following token only counts as the
    // directory value when it is NOT itself a flag, so `--out-dir` immediately
    // followed by another flag (or appearing as the last token) is treated as
    // "no value given": `out_dir_value_pos` stays None, and the malformed
    // override is warned about + dropped below rather than silently consuming a
    // flag like `--verbose` as the directory.
    let out_dir_present = args.iter().any(|a| a == "--out-dir");
    let out_dir_value_pos: Option<usize> =
        args.iter().position(|a| a == "--out-dir").and_then(|i| {
            if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                Some(i + 1)
            } else {
                None
            }
        });

    // Pick the first positional token: not a flag (`-`-prefixed) and not the
    // value following `-o` or `--out-dir`.  This makes flag ordering irrelevant,
    // so both `reify build file.ri --verbose` and `reify build --verbose file.ri`
    // correctly identify the input file.
    let file = match args.iter().enumerate().find(|(i, a)| {
        !a.starts_with('-') && Some(*i) != o_value_pos && Some(*i) != out_dir_value_pos
    }) {
        Some((_, f)) => f,
        None => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    // `-o` present selects imperative single-output mode; its absence selects
    // the declarative occurrence-driven driver (which writes nothing when the
    // design declares no `: Output` occurrences). Either mode is valid with or
    // without `--verbose` — the historical "no -o requires --verbose" guard is
    // gone now that a bare `reify build f.ri` runs the driver.
    let output_path: Option<&String> = o_value_pos.map(|i| &args[i]);

    // `--out-dir` only affects the declarative (no-`-o`) driver. Warn rather than
    // silently discard the user's intent when it can have no effect (`-o` present)
    // or is malformed (no directory argument follows).
    if out_dir_present {
        if output_path.is_some() {
            eprintln!(
                "warning: --out-dir is ignored when -o is given \
                 (imperative single-output mode writes to the -o path)"
            );
        } else if out_dir_value_pos.is_none() {
            eprintln!(
                "warning: --out-dir was given without a directory argument; ignoring it \
                 (relative output paths resolve against the design file's directory)"
            );
        }
    }

    let compiled = match parse_and_compile(file) {
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

    let checker = SimpleConstraintChecker;
    // Register FEA/buckling/modal + shell-extract compute trampolines so that
    // `@optimized("solver::elastic_static")` targets dispatch to the real solver
    // rather than body-inlining.  This is also what keeps the missing-trampoline
    // diagnostic GATING here: without these registrations this engine's compute
    // registry would be entirely EMPTY — `cmd_check`'s declared trampoline-free
    // posture — and task 5311's severity predicate would emit
    // `DiagnosticCode::NoRegisteredComputeTrampoline` at `Severity::Warning`,
    // leaving FEA-result constraints Indeterminate WITHOUT failing the build.
    // With the bundle registered, a target that is still unregistered is a
    // genuine defect and stays `Severity::Error`, which `reify build` gates on.
    //
    // NOTE: cmd_build intentionally does NOT call `configured_eval_engine` (which
    // constructs under a `Solve` posture, wiring the production registry).  The DimensionalSolver resolves
    // `auto` params via a synthetic Chebyshev-centre objective even when no explicit
    // `minimize`/`maximize` directive is present; wiring it here would change the
    // observable behaviour of existing `auto`-param fixtures (bracket_indeterminate,
    // bracket_all_indeterminate) from INDETERMINATE → SATISFIED.  cmd_build's
    // solver-free posture is intentional; cmd_eval wires the full solver via
    // `configured_eval_engine` because it explicitly models the full evaluation path.
    // The FEA trampoline is pure-Rust and independent of the DimensionalSolver, so
    // the solve runs correctly here under `SolverPosture::SolverFree`.  See
    // `build_is_success` for the (c) exit-code gate.
    let mut engine = reify_eval::Engine::with_registered_kernel(
        reify_eval::SolverPosture::SolverFree,
        Box::new(checker),
    );
    register_compute_trampolines(&mut engine);
    // Lazily acquire the OpenVDB kernel when the module contains an
    // `isosurface(...)` realization op (task δ/5002). Without OpenVDB
    // registered, the operand's Mesh→Voxel voxelize stage and the terminal
    // Voxel→Mesh marching-cubes stage (task γ/5001) have no kernel to
    // dispatch to, so the build degrades to an Error diagnostic and exits
    // non-zero. Mirrors cmd_check's `module_has_thickness_dfm_rule` →
    // `ensure_openvdb_kernel()` gate: a static pre-eval detector keeps every
    // non-isosurface build byte-identical and preserves the single-pick
    // OCCT alloc-cost posture (engine_admin.rs). cfg(not(has_openvdb)) →
    // registry lacks "openvdb" → returns false → no-op (an isosurface build
    // would still degrade in that configuration).
    if module_has_isosurface(&compiled) {
        engine.ensure_openvdb_kernel();
    }
    match output_path {
        // ===== Mode (A): imperative single-output (`-o` present). UNCHANGED
        //       back-compat path (B10): the `-o` extension selects the format,
        //       build() serializes the product bodies, and the bytes are written
        //       verbatim to the `-o` target. =====
        Some(path) => {
            // Export refusal (task η, PRD
            // `docs/prds/v0_6/precision-nominal-representation-guarantee.md`
            // §1.1 / C-SURFACE (2)): a design declaring a `RepresentationWithin`
            // bound this path cannot demonstrate it honours must REFUSE to write
            // the artifact rather than write it and report success.
            //
            // WHY THE GATE PRECEDES THE WRITE RATHER THAN RIDING THE DIAGNOSTIC
            // STREAM: `std::fs::write(path, &data)` below runs BEFORE
            // `has_error_diagnostic` is evaluated, so an Error diagnostic ALONE
            // cannot withhold the file — it would exit non-zero having already
            // created (or truncated) the `-o` target. Returning here is what
            // makes the refusal gate the WRITE itself. (Mode B needs no such
            // care: the engine withholds the file by emitting an empty-bytes
            // artifact, which the writer below skips.)
            //
            // Refusing BEFORE `engine.build()` also skips realization and OCCT
            // tessellation entirely, honouring PRD §6's gate-cost rule: η's
            // refusal is a static module-shape decision taken before any
            // measurement. The same helper backs the engine-side Mode-B refusal,
            // so the two surfaces cannot disagree about what counts as bounded.
            //
            // Follow-on task θ narrows this refusal to genuinely UNACHIEVABLE
            // bounds once the export path gains a real measurement (hard-dep on
            // task 6085); until then it fires for any declared bound.
            //
            // KNOWN REPORTING ASYMMETRY WITH MODE B — deliberate, not an
            // oversight. Returning here means a refused `-o` build prints the
            // refusal and NOTHING ELSE: no constraint results, no
            // "No constraints violated (N indeterminate)." summary. Mode B
            // below prints both, because it MUST realize anyway (it needs the
            // elaborated `StructureInstance`s to enumerate `: Output`
            // occurrences at all) and gets the constraint results for free from
            // the same `build_outputs_with_result` call. Mode A has no such
            // obligation: `-o` names its destination outright, so realization
            // buys nothing the refusal needs.
            //
            // The two surfaces are unified on WHAT IS REFUSED and on the
            // diagnostic itself — one shared
            // `unenforced_representation_bound_diagnostic`, one E_* token, one
            // typed code, one exit gate — and that is the property η contracts
            // for (C-SURFACE (2)). They are NOT unified on the surrounding
            // report, and buying that symmetry costs a full realization plus
            // OCCT tessellation (5-20 s on the PRD §2.2 measurements) on a build
            // that is about to write nothing, which is exactly what PRD §6's
            // gate-cost rule forbids. A user who wants the constraint state runs
            // `reify check` — which the refusal message itself tells them to do.
            if let Some(diag) =
                reify_eval::tolerance_combine::unenforced_representation_bound_diagnostic(&compiled)
            {
                // Report through the EXISTING report_eval_output path rather
                // than a bespoke stderr write, so the refusal lands in the same
                // order as every other diagnostic (PRD INV-SF-2: "η rides
                // `cmd_build`'s existing gate rather than adding a per-code
                // bolt-on").
                //
                // THE EXIT CODE IS UNCONDITIONAL, AND THAT IS THE POINT: reaching
                // this arm means the artifact is BEING REFUSED and the write below
                // is being skipped outright, so write-suppression and exit code
                // must not be able to decouple. Deriving it from
                // `build_is_success(&outcome, has_error_diagnostic)` instead would
                // couple them to the builder's severity — a contract pinned only
                // by a unit test in another crate. Were
                // `unenforced_representation_bound_diagnostic` ever to return a
                // Warning/Info, `cmd_build` would write NO file at the `-o`
                // target, print nothing on stdout (`report_constraint_results(&[])`
                // writes nothing) and exit 0: a silent no-op export, the worst
                // failure mode a build command has. This is behaviour-preserving
                // today — the builder returns `Severity::Error` and
                // `constraint_results` is empty, so `build_is_success` is already
                // `false` here — and it stays correct if that ever changes.
                //
                // The `debug_assert` is the loud half of the same guard: a
                // severity regression fails the test suite at this line instead of
                // silently degrading the report (a non-Error diagnostic would print
                // as a warning while the build still exits non-zero — accurate exit
                // code, misleading message).
                debug_assert_eq!(
                    diag.severity,
                    Severity::Error,
                    "the export refusal must be Error-severity: it is what the rest of \
                     the CLI's reporting treats as a refusal rather than an advisory"
                );
                let diagnostics = [diag];
                let _ = report_eval_output(
                    &[],
                    &diagnostics,
                    &mut std::io::stdout(),
                    &mut std::io::stderr(),
                );
                return ExitCode::FAILURE;
            }

            let format = match path {
                p if p.ends_with(".step") || p.ends_with(".stp") => ExportFormat::Step,
                p if p.ends_with(".stl") => ExportFormat::Stl,
                p if p.ends_with(".3mf") => ExportFormat::ThreeMF,
                _ => {
                    eprintln!("Unknown output format, defaulting to STEP");
                    ExportFormat::Step
                }
            };

            let result = engine.build(&compiled, format);

            let outcome = report_eval_output(
                &result.constraint_results,
                &result.diagnostics,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            );

            // Under --verbose, print per-realization kernel provenance to stdout.
            if verbose {
                let provenance = engine.realization_kernel_provenance();
                for entry in &provenance {
                    println!(
                        "  {}: kernel: {}, repr: {:?}",
                        entry.realization,
                        entry.kernel.as_registry_name(),
                        entry.repr,
                    );
                }
                // Task 4744 β (step-22): mesh-morph activity for this build
                // (morphed / remeshed / ineligible — the morph_stats counters,
                // also exposed via the mesh_morph_stats debug RPC).
                println!(
                    "  {}",
                    reify_mesh_morph::diagnostics::format_summary(
                        &reify_mesh_morph::diagnostics::snapshot()
                    )
                );
            }

            match result.geometry_output {
                Some(data) => {
                    if let Err(e) = std::fs::write(path, &data) {
                        eprintln!("Error writing {}: {}", path, e);
                        return ExitCode::FAILURE;
                    }
                    println!("Wrote {} ({} bytes)", path, data.len());
                    // Task δ/5002: print the exported triangle count for mesh
                    // formats (Stl/ThreeMF); STEP/BRep builds are unchanged.
                    // `ExportFormat::Obj` cannot reach this match: the format
                    // matcher above (this Mode-A `-o` arm) has no `.obj`
                    // case, so an `.obj` path falls through to the `_ =>`
                    // STEP default before `format` is ever `Obj` here. Both
                    // arms read the count from `data` — the bytes actually
                    // written to `path` — rather than re-deriving it by
                    // tessellating again, so the printed count can never
                    // disagree with the file on disk; see
                    // `stl_triangle_count` / `threemf_triangle_count` for the
                    // per-format byte layout and `triangle_count_tests` for
                    // coverage.
                    //
                    // Mode-A (`-o`) only: Mode-B's declarative `: Output`
                    // loop below prints no per-artifact count — see the note
                    // there for the scope rationale.
                    match format {
                        ExportFormat::Stl => println!("Triangles: {}", stl_triangle_count(&data)),
                        ExportFormat::ThreeMF => {
                            println!("Triangles: {}", threemf_triangle_count(&data))
                        }
                        _ => {}
                    }
                    // Emit the per-outcome status message (unchanged from
                    // pre-4458), then decide exit via build_is_success — which
                    // also gates on Severity::Error diagnostics, matching
                    // cmd_eval's Error gate (task 4458 fix (c)).
                    match &outcome {
                        ConstraintOutcome::AllSatisfied => {}
                        ConstraintOutcome::SomeIndeterminate(n) => {
                            println!("No constraints violated ({n} indeterminate).");
                        }
                        ConstraintOutcome::SomeViolated => {
                            println!("Some constraints violated.");
                        }
                    }
                    if build_is_success(&outcome, has_error_diagnostic(&result.diagnostics)) {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                None => {
                    eprintln!("No geometry output produced");
                    ExitCode::FAILURE
                }
            }
        }

        // ===== Mode (B): declarative occurrence-driven export (no `-o`). The
        //       DSL `: Output` occurrences drive the format(s) + path(s); the
        //       CLI is a thin writer. =====
        None => {
            // CI escape hatch: --out-dir overrides the design-file directory as
            // the base for relative occurrence paths.
            let out_dir_override =
                out_dir_value_pos.map(|i| std::path::Path::new(args[i].as_str()));
            // design_dir = the .ri file's parent (B7: occurrence paths are
            // design-file-relative, not cwd-relative). A bare "foo.ri" (empty
            // parent) resolves against ".".
            let design_dir = std::path::Path::new(file)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));

            // Realize the module ONCE: build_outputs_with_result runs a single
            // realization (Phase-B product serialization DISABLED) and returns
            // both the per-occurrence artifacts AND that realization's constraint
            // results + diagnostics. This replaces the earlier
            // `engine.build()` + `engine.build_outputs()` pair, which realized,
            // constraint-checked, and serialized the discarded Phase-B STEP output
            // twice. Kernel provenance is read from engine state below, populated
            // by this single realization.
            let outputs = engine.build_outputs_with_result(&compiled, design_dir, out_dir_override);
            let artifacts = &outputs.artifacts;

            // Surface BOTH the build diagnostics AND every per-artifact
            // diagnostic (an I_DISPLAY_OUTPUT_DEFERRED info, or a per-occurrence
            // export error) through the shared reporter + exit gate.
            let mut all_diagnostics = outputs.diagnostics.clone();
            for artifact in artifacts {
                all_diagnostics.extend(artifact.diagnostics.iter().cloned());
            }

            let outcome = report_eval_output(
                &outputs.constraint_results,
                &all_diagnostics,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            );

            // Under --verbose, print per-realization kernel provenance to stdout.
            // Preserved for a no-`-o` build even when ZERO Output occurrences are
            // declared, so `build bracket.ri --verbose` still reports
            // 'kernel: occt' and exits 0 (cli_build_verbose regression guard).
            if verbose {
                let provenance = engine.realization_kernel_provenance();
                for entry in &provenance {
                    println!(
                        "  {}: kernel: {}, repr: {:?}",
                        entry.realization,
                        entry.kernel.as_registry_name(),
                        entry.repr,
                    );
                }
                // Task 4744 β (step-22): mesh-morph activity for this build
                // (morphed / remeshed / ineligible — the morph_stats counters,
                // also exposed via the mesh_morph_stats debug RPC).
                println!(
                    "  {}",
                    reify_mesh_morph::diagnostics::format_summary(
                        &reify_mesh_morph::diagnostics::snapshot()
                    )
                );
            }

            // Write one file per artifact. Gate on non-empty bytes (NEVER on
            // format): a DisplayOutput-deferred or failed-occurrence artifact
            // carries empty bytes and must write no file.
            //
            // Unlike Mode-A above, this declarative per-artifact loop does
            // NOT print a `Triangles: N` count — task δ/5002's PRD scope is
            // the imperative `-o` CLI path only (proven by
            // `cli_build_voxel_to_mesh.rs`); see the Mode-A `-o` arm's
            // comment above for the full scope rationale. Each `artifact`
            // here does carry `format` + `bytes`, so `stl_triangle_count` /
            // `threemf_triangle_count` are reusable directly, keyed on
            // `artifact.format`, if Mode-B observability is ever wanted.
            let mut files_written = 0usize;
            for artifact in artifacts {
                if artifact.bytes.is_empty() {
                    continue;
                }
                if let Some(parent) = artifact.path.parent()
                    && !parent.as_os_str().is_empty()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    eprintln!("Error creating {}: {}", parent.display(), e);
                    return ExitCode::FAILURE;
                }
                if let Err(e) = std::fs::write(&artifact.path, &artifact.bytes) {
                    eprintln!("Error writing {}: {}", artifact.path.display(), e);
                    return ExitCode::FAILURE;
                }
                println!(
                    "Wrote {} ({} bytes)",
                    artifact.path.display(),
                    artifact.bytes.len()
                );
                files_written += 1;
            }

            // Explain a silent zero-file success: without this, a bare
            // `reify build f.ri` whose design declares no `: Output` occurrence
            // (or only deferred/failed ones) exits SUCCESS having printed nothing
            // about output — a likely "I forgot -o / forgot to declare an Output"
            // confusion. Print one informational line so the no-write outcome is
            // never unexplained. (Per-occurrence deferral/failure diagnostics were
            // already surfaced above via report_eval_output.)
            if files_written == 0 {
                if artifacts.is_empty() {
                    println!(
                        "No output files written: the design declares no `: Output` \
                         occurrences. Use `-o <file>` to write a single file, or add an \
                         output occurrence (e.g. `sub o = STLOutput(subject: <part>, \
                         path: \"out.stl\")`)."
                    );
                } else {
                    println!(
                        "No output files written: all {} `: Output` occurrence(s) were \
                         deferred or failed (see diagnostics above).",
                        artifacts.len()
                    );
                }
            }

            // Same status message + exit gate as the imperative path: 0 file
            // artifacts + no Error diagnostic + no violated constraint => SUCCESS.
            match &outcome {
                ConstraintOutcome::AllSatisfied => {}
                ConstraintOutcome::SomeIndeterminate(n) => {
                    println!("No constraints violated ({n} indeterminate).");
                }
                ConstraintOutcome::SomeViolated => {
                    println!("Some constraints violated.");
                }
            }
            if build_is_success(&outcome, has_error_diagnostic(&all_diagnostics)) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

/// CLI-local alias for
/// [`Engine::register_production_compute_fns`][reify_eval::Engine::register_production_compute_fns]
/// (task 5072 / PRD `compute-fea-hardening.md` A1 — see there for the bundle
/// membership, ordering, and the INV-FEA-1 single-source-of-truth rationale;
/// this wrapper does not restate them).
///
/// Exists to name the `MorphRegistration::Enabled` choice once and keep both
/// call sites — `cmd_build` (solver-free, calls this directly) and
/// [`configured_eval_engine`] (full solver) — one-line, with no drift between
/// them.
///
/// The morph producer is dormant-safe (task 4744 β): the dispatch only
/// attempts a morph when a prior source mesh carries a `BoundaryAssociation`,
/// which today requires an explicit boundary demand — a plain `reify build`
/// never triggers the (#4876-crashing) attributed path. Every non-success
/// morph honestly falls back to a remesh.
fn register_compute_trampolines(engine: &mut reify_eval::Engine) {
    engine.register_production_compute_fns(reify_eval::MorphRegistration::Enabled(
        reify_mesh_morph::register_morph_producer,
    ));
}

/// The eval family's (`cmd_eval`, `cmd_report`, `cmd_explain`) solver posture:
/// the solver's default budget, and results persisted by an earlier session may
/// be served, because [`configured_eval_engine`] attaches the persistent cache.
const EVAL_FAMILY_POSTURE: reify_eval::SolverPosture =
    reify_eval::SolverPosture::Solve(reify_ir::ResolutionProfile::new(
        reify_ir::IterationBudget::SolverDefault,
        reify_ir::StalenessPolicy::MayServePersisted,
    ));

/// Construct and configure a [`reify_eval::Engine`] for use in `cmd_eval`:
/// `construct` is called with [`EVAL_FAMILY_POSTURE`], whose `Solve` posture
/// wires the production [`reify_constraints::SolverRegistry`]; all compute
/// trampolines are then registered so `@optimized` targets dispatch correctly.
///
/// The production registry installs `DimensionalSolver` (dimensional constraints)
/// and `SolveSpaceSolver` (geometric constraints: `std::distance`,
/// `std::angle_between`, `std::parallel`, `std::tangent`, `std::geo::*`).
/// This mirrors the GUI's `EngineSession::with_registered_kernel` solver so that
/// CLI and GUI resolve auto-params identically.
///
/// Both the geometry branch (`with_registered_kernel + build()`) and the plain
/// branch (`Engine::new(None) + eval()`) share this setup; only the constructor
/// and the terminal `build()`/`eval()` call differ.  Factoring the shared setup
/// here keeps the posture, [`register_compute_trampolines`] and the
/// persistent-cache attach together, so no caller can pair the cache with a
/// different posture.
///
/// Both the FEA/buckling/modal trampolines and the shell-extract trampoline are
/// registered here via [`register_compute_trampolines`], mirroring the GUI's call
/// pair (gui/src-tauri/src/engine.rs).  Without the shell-extract registration,
/// shell-classified `@optimized("solver::elastic_static")` solves would hit
/// `DispatchError::Failed` in `insert_shell_extract_upstream` and emit a misleading
/// "falling back to tet meshing" warning even though the FEA trampoline independently
/// re-classifies and runs the correct shell solve.
///
/// NOTE: `cmd_build` intentionally does NOT use this helper — it calls
/// [`register_compute_trampolines`] directly (constructs `SolverPosture::SolverFree`)
/// to preserve cmd_build's solver-free posture for `auto`-param fixtures.  See the comment in
/// `cmd_build` for rationale.
fn configured_eval_engine(
    construct: impl FnOnce(reify_eval::SolverPosture) -> reify_eval::Engine,
) -> reify_eval::Engine {
    let mut engine = construct(EVAL_FAMILY_POSTURE);
    register_compute_trampolines(&mut engine);
    // Resolve the persistent FEA cache dir from env/config/defaults and wire it
    // into the engine.  Best-effort: a resolver error (e.g. bad
    // REIFY_CACHE_MAX_BYTES env var) is logged at DEBUG and the engine proceeds
    // without a persistent cache for this session.  Callers may override the
    // directory via engine.set_persistent_cache_dir (e.g. cmd_eval's
    // --cache-dir flag) after this function returns.
    match cache::resolve_cache_root() {
        Ok(cache_dir) => {
            engine.set_persistent_cache_dir(Some(cache_dir));
        }
        Err(e) => {
            tracing::debug!("persistent-cache disabled for this session — resolver error: {e}");
        }
    }
    engine
}

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
/// through [`Engine::build`] so that
/// `run_post_processes`/`post_process_geometry_queries` fires and lands
/// geometry-query value cells (e.g. `mass`, `centroid`) into `BuildResult.values`
/// (task 4145).  `geometry_output` from `BuildResult` is discarded — `reify eval`
/// is a value-cell inspector, not an exporter.
///
/// When the OCCT kernel is absent (`cfg(has_occt)` unset), the registered kernel
/// inventory is empty; `with_registered_kernel` returns a None-kernel engine and
/// `build()` skips the geometry pipeline — geometry-query cells stay `undef` and
/// exit code remains 0, matching `cmd_build`'s existing degradation in stub mode.
///
/// When OCCT is present but geometry realization fails at runtime (e.g. all ops
/// fail in the kernel), `build()` emits an `Error`-severity diagnostic and those
/// errors **do** propagate to `cmd_eval`'s exit code.  This widening is
/// intentional: a file whose geometry is fundamentally broken should not silently
/// exit 0 with all geometry-query cells reported as `undef`.
///
/// Non-geometry modules use the existing
/// `Engine::new(None) + eval()` path unchanged.
/// `reify report --bom <file>` — roll up and render a BOM / cost / waste /
/// provenance report (io-lifecycle-bom-cost #4292, boundary α).
///
/// Compiles + evaluates the design on the lightweight kernel-free eval path
/// (lifecycle `StructureInstance` cells populate under plain `engine.eval`; the
/// rollup needs no geometry realization), builds a [`reify_eval::BomReport`],
/// and renders it to stdout. Compile / eval `Severity::Error`s propagate to a
/// non-zero exit; diagnostics go to stderr so stdout stays the rendered report.
fn cmd_report(args: &[String]) -> ExitCode {
    // Parse a required `--bom` flag + exactly one positional .ri path. Reject
    // unknown flags so they are never silently misread as the file path.
    let mut want_bom = false;
    let mut file_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bom" => {
                want_bom = true;
                i += 1;
            }
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `report`: {}", flag);
                eprintln!("Usage: reify report --bom <file>");
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

    if !want_bom {
        eprintln!("Error: `report` requires the --bom flag");
        eprintln!("Usage: reify report --bom <file>");
        return ExitCode::FAILURE;
    }
    let Some(path) = file_path else {
        eprintln!("Usage: reify report --bom <file>");
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

    // Lightweight kernel-free eval path (mirrors cmd_eval's non-geometry branch):
    // the lifecycle rollup needs cost/waste/provenance cells, not geometry.
    let mut engine = configured_eval_engine(|posture| {
        reify_eval::Engine::new(posture, Box::new(SimpleConstraintChecker), None)
    });
    let result = engine.eval(&compiled);

    // Check eval-level errors BEFORE rendering. stdout is the parseable BOM
    // report, so on an eval error we must NOT emit a partial/misleading report
    // that a downstream consumer would treat as authoritative — diagnostics go
    // to stderr and the run fails with no report on stdout.
    if result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        for diag in &result.diagnostics {
            eprintln!("{}: {}", diag.severity, diag.message);
        }
        return ExitCode::FAILURE;
    }

    let report = engine.build_bom_report(&compiled, &result.values);
    if report.is_renderable_empty() {
        // Friendly empty-report message (still exit 0): a design with no
        // Buy / Discard / Input subs — and nothing to warn about — has nothing
        // to roll up. `is_renderable_empty()` owns the emptiness contract
        // (incl. the warnings clause) so it cannot drift from the fields it
        // reads — see `BomReport::is_renderable_empty`.
        println!("no BOM line items (no Buy / Discard / Input subs in this design)");
    } else {
        // A non-empty report — OR a design with zero rolled-up rows but a
        // NON-empty `report.warnings` (e.g. its only lifecycle item is a
        // *collection* Buy sub, a v1 limitation) — must route through render():
        // it is the ONLY sink for `report.warnings` (the stderr loop below
        // prints eval diagnostics, not report warnings). Taking the friendly-
        // message branch here would silently drop the under-count warning AND
        // lie ("no Buy / Discard / Input subs" — there IS a Buy sub).
        print!("{}", report.render());
    }

    // Non-error diagnostics (warnings / info) to stderr (stdout stays the report).
    for diag in &result.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }
    ExitCode::SUCCESS
}

fn cmd_eval(args: &[String]) -> ExitCode {
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

/// Print per-cell objective provenance for every auto parameter resolved by eval.
///
/// Always uses the plain `eval()` path (never `build()`) with the production
/// solver wired via `configured_eval_engine` so that auto params resolve and
/// `EvalResult.objective_provenance` is populated.  (`build()` constructs its
/// `EvalResult` with an empty provenance map — `engine_eval.rs:3884`.)
///
/// Output format (B9 triple, one line per cell, sorted by entity then member):
/// ```text
/// <entity>.<member>: objective=<N term(s)|none>, combination=<weighted-sum|lexicographic|none>, source=<explicit|synthetic-centrality>
/// ```
fn cmd_explain(args: &[String]) -> ExitCode {
    let path = match parse_single_file_arg(args, "explain", EXPLAIN_USAGE) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let compiled = match parse_and_compile(&path) {
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

    // Always use plain eval() with the production solver so provenance is recorded.
    let mut engine = configured_eval_engine(|posture| {
        reify_eval::Engine::new(posture, Box::new(SimpleConstraintChecker), None)
    });
    let result = engine.eval(&compiled);

    // Collect and sort for deterministic output (HashMap has non-deterministic order).
    let mut provenance: Vec<(&reify_core::ValueCellId, &reify_ir::ObjectiveProvenance)> =
        result.objective_provenance.iter().collect();
    provenance.sort_by(|a, b| {
        a.0.entity
            .cmp(&b.0.entity)
            .then(a.0.member.cmp(&b.0.member))
    });

    if provenance.is_empty() {
        println!("No objective provenance recorded (no auto parameters resolved).");
    } else {
        for (cell_id, prov) in &provenance {
            let objective = match &prov.objective {
                Some(obj_set) => format!("{} term(s)", obj_set.terms.len()),
                None => "none".to_string(),
            };
            let combination = match &prov.combination {
                Some(reify_ir::ObjectiveCombination::WeightedSum) => "weighted-sum",
                Some(reify_ir::ObjectiveCombination::Lexicographic) => "lexicographic",
                None => "none",
            };
            // §3.5 (γ #4824): inherited cells get a distinct source token and
            // governance clause; own/centrality cells are unchanged.
            if let Some(container) = &prov.inherited_from {
                println!(
                    "{}.{}: objective={}, combination={}, source=inherited, \
                     governed by objective inherited from {}",
                    cell_id.entity, cell_id.member, objective, combination, container
                );
            } else {
                let source = if prov.synthetic_centrality {
                    "synthetic-centrality"
                } else {
                    "explicit"
                };
                println!(
                    "{}.{}: objective={}, combination={}, source={}",
                    cell_id.entity, cell_id.member, objective, combination, source
                );
            }
        }
    }

    for diag in &result.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    if result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Output format for `reify doc`.
///
/// Default is `Html` per the PRD; the `--format` flag accepts `html`,
/// `markdown`, or `json`.  Bad values exit 1 with a usage error written to
/// stderr; the match is inline in `cmd_doc` since it has only one call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Markdown,
    Json,
}

fn cmd_doc(args: &[String]) -> ExitCode {
    // Mirrors `cmd_gui`'s explicit-flag pattern: walk args, accept the
    // documented flags, and reject any other `--`-prefixed token with a
    // usage error.  The first non-flag positional is the input path; a
    // second positional is rejected as a usage error.
    let mut format: Option<String> = None;
    let mut output: Option<String> = None;
    let mut split = false;
    let mut compact = false;
    let mut stdlib = false;
    let mut input: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--split" => {
                split = true;
                i += 1;
            }
            "--compact" => {
                compact = true;
                i += 1;
            }
            "--stdlib" => {
                stdlib = true;
                i += 1;
            }
            "--format" => {
                if i + 1 >= args.len() {
                    return doc_usage_error("--format requires a value");
                }
                format = Some(args[i + 1].clone());
                i += 2;
            }
            "-o" | "--out" => {
                if i + 1 >= args.len() {
                    return doc_usage_error(format_args!("{a} requires a path"));
                }
                output = Some(args[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                return doc_usage_error(format_args!("unknown flag for `doc`: {flag}"));
            }
            _ => {
                if input.is_some() {
                    return doc_usage_error(format_args!(
                        "unexpected extra positional argument: {a}"
                    ));
                }
                input = Some(a);
                i += 1;
            }
        }
    }

    // --stdlib mode: HTML-only, directory-output-only.  Guard all conflicting
    // flags before doing any compilation work.
    if stdlib {
        if output.is_none() {
            return doc_usage_error("--stdlib requires --out <dir>");
        }
        if input.is_some() {
            return doc_usage_error("--stdlib does not accept an input file positional");
        }
        if split {
            return doc_usage_error("--split is not valid with --stdlib");
        }
        if compact {
            return doc_usage_error("--compact is not valid with --stdlib");
        }
        if !matches!(format.as_deref(), None | Some("html")) {
            return doc_usage_error("--stdlib only supports --format html (the default)");
        }
        // Build the stdlib doc model, render multi-page HTML, and write files.
        let model = reify_doc_build::build_stdlib_doc_model();
        // Cross-refs (trait conformance) are omitted for now: build_cross_refs
        // operates on a single module's templates while the stdlib spans many.
        // A combined cross-refs pass is deferred to a follow-up task.
        let pages = reify_doc::fmt_html::render_html_pages(&model, None);
        let out_dir = std::path::PathBuf::from(output.as_deref().unwrap());
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("Error writing {}: {}", out_dir.display(), e);
            return ExitCode::FAILURE;
        }
        for (name, body) in pages {
            let file_path = out_dir.join(&name);
            if let Some(parent) = file_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                eprintln!("Error writing {}: {}", parent.display(), e);
                return ExitCode::FAILURE;
            }
            if let Err(e) = std::fs::write(&file_path, body.as_bytes()) {
                eprintln!("Error writing {}: {}", file_path.display(), e);
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    let Some(input) = input else {
        return doc_usage_error("missing input file");
    };

    // Resolve `--format` (default `html`) into a typed `Format`.  Bad values
    // exit 1 with a usage-error on stderr.
    let format = match format.as_deref() {
        Some("html") => Format::Html,
        Some("markdown") => Format::Markdown,
        Some("json") => Format::Json,
        Some(other) => {
            return doc_usage_error(format_args!(
                "unknown --format value: {other} (expected html|markdown|json)"
            ));
        }
        None => Format::Html,
    };

    // `--split` is markdown-only.  Reject json/html + split before doing any
    // expensive parse/compile work so usage errors are fast and stderr stays
    // crisp.
    if split && format != Format::Markdown {
        return doc_usage_error("--split is only valid with --format markdown");
    }

    // `--compact` is json-only.  Mirror the `--split` guard.
    if compact && format != Format::Json {
        return doc_usage_error("--compact is only valid with --format json");
    }

    // `--split` requires `-o <dir>` so we know where to write the per-item
    // files.  Hoisted above `parse_and_compile` so usage errors don't pay for parsing.
    // Reachable only when format == Markdown thanks to the guard above.
    if split && output.is_none() {
        return doc_usage_error("--split requires -o <directory>");
    }

    let compiled = match parse_and_compile(input) {
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

    // Read the source file so build_doc_model can slice SourceSpan offsets
    // into the source string for constraint expr_repr and line numbers.
    // parse_and_compile already read and validated the file, so a second
    // read error is unexpected but handled consistently with the existing
    // `Error reading {path}: {e}` convention.
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", input, e);
            return ExitCode::FAILURE;
        }
    };

    let model = reify_doc_build::build_doc_model(&compiled, &source);
    let xrefs = reify_doc_build::cross_refs::build_cross_refs(&compiled.templates);

    match format {
        Format::Json => {
            // The JSON formatter has no trailing newline of its own; we add
            // one to keep shell output tidy in stdout mode.  The `-o <file>`
            // write does NOT add the trailing newline so the file body is
            // exactly the formatter output.
            let body = reify_doc::fmt_json::render_json(&model, compact);
            write_single_file_or_stdout(output.as_deref(), &body, /*trailing_newline=*/ true)
        }
        Format::Markdown => {
            let opts = reify_doc::fmt_markdown::MarkdownOptions { split };
            let rendered = reify_doc::fmt_markdown::render_markdown(&model, Some(&xrefs), &opts);
            match rendered {
                reify_doc::fmt_markdown::MarkdownOutput::Single(body) => {
                    write_single_file_or_stdout(
                        output.as_deref(),
                        &body,
                        /*trailing_newline=*/ false,
                    )
                }
                reify_doc::fmt_markdown::MarkdownOutput::Split(files) => {
                    // The `--split requires -o <dir>` guard runs in the early
                    // usage-validation block above, so by the time we get here
                    // `output` is guaranteed `Some`.  `expect` rather than
                    // `unwrap` so a future refactor that bypasses the guard
                    // panics with a loud, attributable message instead of
                    // silently writing to a wrong path.
                    let dir = std::path::PathBuf::from(output.as_deref().expect(
                        "--split + --format markdown without -o is rejected by the early \
                             usage-validation block; reaching this branch means that guard was \
                             accidentally bypassed",
                    ));
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        eprintln!("Error writing {}: {}", dir.display(), e);
                        return ExitCode::FAILURE;
                    }
                    for (name, body) in files {
                        let file_path = dir.join(&name);
                        if let Err(e) = std::fs::write(&file_path, body.as_bytes()) {
                            eprintln!("Error writing {}: {}", file_path.display(), e);
                            return ExitCode::FAILURE;
                        }
                    }
                    ExitCode::SUCCESS
                }
            }
        }
        Format::Html => {
            // Default + explicit `--format html`: emit the real HTML formatter output.
            let body = reify_doc::fmt_html::render_html(&model, Some(&xrefs));
            write_single_file_or_stdout(output.as_deref(), &body, /*trailing_newline=*/ false)
        }
    }
}

/// Write `body` to `target` (when `Some`) or stdout (when `None`).
///
/// On stdout mode, appends a single `'\n'` after `body` iff `trailing_newline`
/// is true so JSON output ends in a newline (matches `cmd_check`'s
/// `println!` style and keeps shell output tidy).  On file-write mode the
/// trailing newline is *not* added; the on-disk body is exactly the
/// formatter output so it round-trips cleanly through tools that expect
/// canonical content.
///
/// Mirrors `cmd_build`'s `Error writing {path}: {e}` stderr format on I/O
/// failure; returns `ExitCode::FAILURE` (1) on write errors.
fn write_single_file_or_stdout(
    target: Option<&str>,
    body: &str,
    trailing_newline: bool,
) -> ExitCode {
    match target {
        Some(path) => {
            if let Err(e) = std::fs::write(path, body.as_bytes()) {
                eprintln!("Error writing {}: {}", path, e);
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        None => {
            if trailing_newline {
                println!("{body}");
            } else {
                print!("{body}");
            }
            ExitCode::SUCCESS
        }
    }
}

fn cmd_gui(args: &[String]) -> ExitCode {
    // Parse `--debug` / `--mcp` flags (both set the same `debug` boolean) and
    // strip them from the positional args before extracting the file path.
    // Any other `--`-prefixed token is rejected explicitly so a typo like
    // `--debugg` fails loud instead of being silently treated as a file path.
    let mut debug = false;
    let mut positional: Vec<&String> = Vec::with_capacity(args.len());
    for a in args {
        match a.as_str() {
            "--debug" | "--mcp" => debug = true,
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `gui`: {}", flag);
                eprintln!("Usage: reify gui [--debug] <file>");
                return ExitCode::FAILURE;
            }
            _ => positional.push(a),
        }
    }

    if positional.is_empty() {
        eprintln!("Usage: reify gui [--debug] <file>");
        return ExitCode::FAILURE;
    }

    let file = positional[0].as_str();
    let path = std::path::Path::new(file);

    // Validate .ri extension (checked before existence to give a clear error for wrong file types)
    match path.extension().and_then(|e| e.to_str()) {
        Some("ri") => {}
        _ => {
            eprintln!("Error: file must have .ri extension: {}", file);
            return ExitCode::FAILURE;
        }
    }

    // Validate file exists
    if !path.exists() {
        eprintln!("Error: file does not exist: {}", file);
        return ExitCode::FAILURE;
    }

    // Check if launch is suppressed (for testing / CI). The user-facing error
    // is kept clean (no internal flag state). Tests that need to assert on the
    // parsed debug-mode set `REIFY_GUI_DEBUG_PROBE=1` to enable a structured
    // probe line — keeping the test seam off the default error path.
    if std::env::var("REIFY_GUI_SKIP_LAUNCH").is_ok() {
        if std::env::var("REIFY_GUI_DEBUG_PROBE").is_ok() {
            eprintln!("REIFY_GUI_DEBUG_PROBE: debug={}", debug);
        }
        eprintln!("Error: could not launch reify-gui (launch skipped via REIFY_GUI_SKIP_LAUNCH)");
        return ExitCode::FAILURE;
    }

    // Locate the reify-gui binary: same directory as this binary, then PATH
    let gui_binary_name = if cfg!(target_os = "windows") {
        "reify-gui.exe"
    } else {
        "reify-gui"
    };

    let gui_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(gui_binary_name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(gui_binary_name));

    let mut cmd = build_gui_command(&gui_path, file, debug);
    match cmd.status() {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!(
                "Error: could not launch reify-gui ({}): {}",
                gui_path.display(),
                e
            );
            ExitCode::FAILURE
        }
    }
}

/// Build a [`std::process::Command`] for launching `reify-gui` with the given
/// file argument and (optionally) `REIFY_DEBUG=1` set in the child's
/// environment when `debug` is true.
///
/// Extracted as a pure helper so it can be unit-tested via `Command::get_envs()`
/// without spawning a subprocess.
fn build_gui_command(gui_path: &std::path::Path, file: &str, debug: bool) -> std::process::Command {
    let mut cmd = std::process::Command::new(gui_path);
    cmd.arg(file);
    if debug {
        cmd.env("REIFY_DEBUG", "1");
    }
    cmd
}

fn cmd_lsp() -> ExitCode {
    // Use a multi-thread runtime with a capped worker count.  A current-thread
    // runtime was tried (ceede7afc) to reduce startup latency, but tower-lsp
    // relies on `tokio::spawn` internally to drive request/response futures
    // concurrently with the stdin-reading loop.  With a single-threaded
    // executor those spawned futures may not be polled until the next I/O
    // yield, causing the initialize response to never arrive when the test
    // sends only one message.  Two worker threads is the minimum safe count:
    // one drives the serve loop, one drives handler/notification tasks.
    match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => {
            rt.block_on(reify_lsp::run_server());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to create async runtime: {}", e);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parse error the CLI prints must be one a user can JUMP TO.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
    /// Both entry points used to print `err.message` alone — no file, no line, no column — so
    /// a parse error in a long file named no place to look. The pre-existing CLI harness
    /// asserts only `stderr.contains("Parse error")`, which passes identically whether the
    /// position is there or not; this asserts the position itself.
    ///
    /// Runs the same two-step composition the entry points run (`parse_with_stdlib` then
    /// `render_errors`) against the real in-tree fixture, and derives the expected line from
    /// that fixture with `str::find` rather than hard-coding it, so the test tracks the
    /// fixture if its layout changes.
    #[test]
    fn report_parse_errors_writes_a_line_and_column_for_every_error() {
        const FIXTURE: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/bracket_parse_error.ri"
        );
        let source = std::fs::read_to_string(FIXTURE).expect("fixture must be readable");
        let fault_offset = source
            .find("@@@")
            .expect("fixture precondition: bracket_parse_error.ri must contain the `@@@` fault");
        let fault_line = source[..fault_offset].matches('\n').count() + 1;

        let parsed =
            reify_compiler::parse_with_stdlib(&source, ModulePath::single("bracket_parse_error"));
        assert!(
            !parsed.errors.is_empty(),
            "fixture precondition: bracket_parse_error.ri must fail to parse"
        );

        let mut out = Vec::new();
        report_parse_errors(parsed.render_errors(&source), &mut out);
        let printed = String::from_utf8(out).expect("CLI output must be UTF-8");

        assert!(
            !printed.is_empty(),
            "a failing parse must report something to the user"
        );
        for reported in printed.lines() {
            let located = reported.strip_prefix("Parse error: ").unwrap_or_else(|| {
                panic!("every reported line keeps the `Parse error: ` prefix; got {reported:?}")
            });
            let (line_str, rest) = located.split_once(':').unwrap_or_else(|| {
                panic!(
                    "expected a `line:column: message` prefix so the user can jump straight to \
                     the fault; got {reported:?}"
                )
            });
            let (col_str, message) = rest.split_once(':').unwrap_or_else(|| {
                panic!("expected a `line:column:` prefix — found a line but no column; got {reported:?}")
            });
            let line: usize = line_str.parse().unwrap_or_else(|_| {
                panic!("the leading field must be a 1-based line number; got {reported:?}")
            });
            let col: usize = col_str.parse().unwrap_or_else(|_| {
                panic!("the second field must be a 1-based column number; got {reported:?}")
            });
            assert!(
                line >= 1 && col >= 1,
                "line and column are 1-based; got {reported:?}"
            );
            assert!(
                !message.trim().is_empty(),
                "the position must be ADDED to the parser's message, not substituted for it; \
                 got {reported:?}"
            );
        }

        let expected_prefix = format!("Parse error: {fault_line}:");
        assert!(
            printed.lines().any(|l| l.starts_with(&expected_prefix)),
            "at least one report must land on line {fault_line}, where the fixture's `@@@` \
             fault sits; got {printed:?}"
        );
    }

    /// Piece-1 force-link pin: asserts that `reify-kernel-manifold`'s
    /// `inventory::submit!` fires inside this binary so the Manifold kernel
    /// appears in the global registry.  This MUST be an in-`main.rs` unit test
    /// because reify-cli is a `[[bin]]` crate — subprocess integration tests
    /// can't observe the binary's link set.
    ///
    /// Manifold's `inventory::submit!` is unconditional (no `cfg(has_*)` gate),
    /// so `"manifold"` is asserted without a runtime flag.  OCCT's submit is
    /// `cfg(has_occt)`-gated, so we guard that assertion on
    /// `reify_kernel_occt::OCCT_AVAILABLE`.
    #[test]
    fn manifold_kernel_is_force_linked_into_binary() {
        let registry = reify_eval::collect_registry();
        assert!(
            registry.contains_key("manifold"),
            "reify-kernel-manifold's inventory::submit! must land in this binary; \
             \"manifold\" key is absent — check Cargo.toml dep + extern crate declaration",
        );
        if reify_kernel_occt::OCCT_AVAILABLE {
            assert!(
                registry.contains_key("occt"),
                "OCCT_AVAILABLE is true but \"occt\" key missing from collect_registry() — \
                 reify-kernel-occt inventory::submit! did not fire",
            );
        }
    }

    #[test]
    fn build_gui_command_sets_reify_debug_when_debug_true() {
        // Verifies that `build_gui_command(.., debug=true)` sets REIFY_DEBUG=1
        // in the child Command's env, without spawning a subprocess.
        let path = std::path::Path::new("/tmp/fake-reify-gui");
        let cmd = build_gui_command(path, "x.ri", true);
        let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|val| val.to_os_string())))
            .collect();
        let reify_debug_set = envs.iter().any(|(k, v)| {
            k == std::ffi::OsStr::new("REIFY_DEBUG")
                && v.as_deref() == Some(std::ffi::OsStr::new("1"))
        });
        assert!(
            reify_debug_set,
            "REIFY_DEBUG=1 must be set in Command env when debug=true; got envs: {:?}",
            envs
        );
    }

    #[test]
    fn build_gui_command_does_not_set_reify_debug_when_debug_false() {
        // Verifies that `build_gui_command(.., debug=false)` does NOT add
        // REIFY_DEBUG to the child Command's env (parent env is inherited
        // automatically by the OS spawn machinery; we only assert that we
        // don't *override* it here).
        let path = std::path::Path::new("/tmp/fake-reify-gui");
        let cmd = build_gui_command(path, "x.ri", false);
        let has_reify_debug = cmd
            .get_envs()
            .any(|(k, _)| k == std::ffi::OsStr::new("REIFY_DEBUG"));
        assert!(
            !has_reify_debug,
            "REIFY_DEBUG must NOT be set in Command env when debug=false"
        );
    }
}

// ── step-10: focused CLI-context tests for persistent-cache wiring ─────────

/// Tests that `configured_eval_engine` wires the persistent cache dir onto the
/// engine (task #3428 step-10).
///
/// Two assertions:
/// 1. When `$HOME` or `$XDG_CACHE_HOME` is set (the normal environment),
///    the engine returned by `configured_eval_engine` has
///    `persistent_cache_dir() == Some(..)`.
/// 2. A subsequent `set_persistent_cache_dir` call (simulating the CLI
///    `--cache-dir` flag override) replaces the configured dir and the engine
///    reports the new path.
#[cfg(test)]
mod persistent_cache_cli_wiring_tests {
    use super::*;

    /// `configured_eval_engine` must wire a `Some(cache_dir)` from the
    /// env/default resolver so FEA evals persist results without extra config.
    ///
    /// The test only asserts `Some(..)` when `$HOME` or `$XDG_CACHE_HOME` is
    /// set — in exotic sandbox environments where neither is set the resolver
    /// falls through to a relative-path default (`.cache/reify/fea`) which
    /// still succeeds, but a relative path is less interesting to pin.  We
    /// skip the assertion in that rare case to avoid false CI failures.
    #[test]
    fn configured_eval_engine_wires_cache_dir() {
        let engine = configured_eval_engine(|posture| {
            reify_eval::Engine::new(
                posture,
                Box::new(reify_constraints::SimpleConstraintChecker),
                None,
            )
        });
        // The resolver always produces *some* path when HOME or XDG_CACHE_HOME
        // is set; only pathological envs (neither set) fall through to a
        // relative-path default that still resolves.
        if std::env::var_os("HOME").is_some() || std::env::var_os("XDG_CACHE_HOME").is_some() {
            assert!(
                engine.persistent_cache_dir().is_some(),
                "configured_eval_engine must wire Some(cache_dir) when HOME or \
                 XDG_CACHE_HOME is set in the environment"
            );
        }
    }

    /// The eval family attaches a persistent cache, so its posture must say it
    /// may serve persisted results.
    #[test]
    fn configured_eval_engine_declares_it_may_serve_persisted_results() {
        let engine = configured_eval_engine(|posture| {
            reify_eval::Engine::new(
                posture,
                Box::new(reify_constraints::SimpleConstraintChecker),
                None,
            )
        });
        match engine.solver_posture() {
            reify_eval::SolverPosture::Solve(profile) => assert!(
                profile.staleness().permits_persisted_results(),
                "the eval family's profile must permit persisted results, got {profile:?}",
            ),
            reify_eval::SolverPosture::SolverFree => {
                panic!("the eval family must construct a Solve posture, got SolverFree")
            }
        }
    }

    /// The `--cache-dir` flag override (modelled by calling
    /// `engine.set_persistent_cache_dir` after `configured_eval_engine`)
    /// must replace whatever dir was set by the env/default resolver.
    #[test]
    fn cache_dir_cli_override_replaces_configured_dir() {
        let tmp = tempfile::TempDir::new().expect("tmp dir must be creatable for --cache-dir test");
        let mut engine = configured_eval_engine(|posture| {
            reify_eval::Engine::new(
                posture,
                Box::new(reify_constraints::SimpleConstraintChecker),
                None,
            )
        });
        // Simulate the cmd_eval --cache-dir flag: override after configuration.
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
        assert_eq!(
            engine.persistent_cache_dir(),
            Some(tmp.path()),
            "--cache-dir override must make persistent_cache_dir() return the \
             specified tmp path, not the env/default-resolved path"
        );
    }
}

// ── Task 5073 (PRD compute-fea-hardening.md task A2): characterization safety
// net for `register_compute_trampolines` ────────────────────────────────────

/// Characterization test for `register_compute_trampolines` (task 5073 / PRD
/// `compute-fea-hardening.md` task A2): pins that the CLI wrapper delegates
/// to the full production compute-trampoline bundle with the morph producer
/// enabled. Refactor safety net — GREEN both before and after this task's
/// migration step, since that migration is behavior-preserving.
#[cfg(test)]
mod compute_trampoline_registration_tests {
    use super::*;

    /// `register_compute_trampolines` must run the production bundle (smoke-
    /// checked via the `register_compute_fns` leg's `solver::elastic_static`
    /// target) and must pass `MorphRegistration::Enabled` — the only
    /// CLI-specific decision — so `morph_producer()` ends up installed. Full
    /// leg-by-leg bundle coverage is `Engine::register_production_compute_fns`'s
    /// own suite's job (`compute_targets/mod.rs`).
    #[test]
    fn cli_trampolines_install_full_production_bundle() {
        let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(SimpleConstraintChecker), None);
        register_compute_trampolines(&mut engine);

        assert!(
            engine.compute_dispatch("solver::elastic_static").is_some(),
            "register_compute_trampolines must run the production bundle \
             (register_compute_fns leg)"
        );
        assert!(
            engine.morph_producer().is_some(),
            "register_compute_trampolines must pass MorphRegistration::Enabled, \
             installing the mesh-morph producer"
        );
    }
}
