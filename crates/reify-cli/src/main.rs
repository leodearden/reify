use std::process::ExitCode;

use reify_compiler::cfg::CfgSet;
use reify_constraints::SimpleConstraintChecker;

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
mod cmd;
mod constraint_report;
mod dev;
mod module_capabilities;
mod sigpipe;
use reify_core::ModulePath;

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
        "check" => cmd::check::cmd_check(&args[2..]),
        "test" => cmd::test::cmd_test(&args[2..]),
        "build" => cmd::build::cmd_build(&args[2..]),
        "run" | "eval" => cmd::eval::cmd_eval(&args[2..]),
        "report" => cmd::report::cmd_report(&args[2..]),
        "doc" => cmd::doc::cmd_doc(&args[2..]),
        "lsp" => cmd::lsp::cmd_lsp(),
        "gui" => cmd::gui::cmd_gui(&args[2..]),
        "gui-debug" => {
            // `gui-debug` is sugar for `gui --debug`: prepend the flag and
            // route through the same code path as `cmd_gui` so the two entry
            // points share argument parsing and binary-launch logic.
            let mut forwarded: Vec<String> = Vec::with_capacity(args.len() - 1);
            forwarded.push("--debug".to_string());
            forwarded.extend(args[2..].iter().cloned());
            cmd::gui::cmd_gui(&forwarded)
        }
        "explain" => cmd::explain::cmd_explain(&args[2..]),
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

    /// `reify check` and `reify build` solve with a fresh profile, attach no
    /// persistent cache, and register the compute trampolines.
    #[test]
    fn configured_check_build_engine_solves_fresh_with_trampolines_and_no_cache() {
        let engine = configured_check_build_engine(|posture| {
            reify_eval::Engine::new(
                posture,
                Box::new(reify_constraints::SimpleConstraintChecker),
                None,
            )
        });
        assert_eq!(
            engine.solver_posture(),
            reify_eval::SolverPosture::Solve(reify_ir::ResolutionProfile::FRESH),
            "check and build must solve, and serve nothing persisted"
        );
        assert!(
            engine.persistent_cache_dir().is_none(),
            "check and build must not attach the persistent cache"
        );
        assert!(
            engine.compute_dispatch("solver::elastic_static").is_some(),
            "check and build must register the production compute trampolines"
        );
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
