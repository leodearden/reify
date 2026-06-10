use std::process::ExitCode;
use std::sync::Arc;

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

mod cache;
mod mcp_context;
use reify_core::{ModulePath, Severity};
use reify_ir::{ExportFormat, Satisfaction};

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
        "  mcp-server [file] [--project-dir <dir>]  Start MCP server (stdin/stdout)"
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
    let _ = writeln!(out, "  --version                  Print version");
    let _ = writeln!(out, "  --help                     Show this list");
}

fn main() -> ExitCode {
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
        "mcp-server" => cmd_mcp_server(&args[2..]),
        "cache" => cache::cmd_cache(&args[2..]),
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage(&mut std::io::stderr());
            ExitCode::FAILURE
        }
    }
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
        for err in &parsed.errors {
            eprintln!("Parse error: {}", err.message);
        }
        return Err(ExitCode::FAILURE);
    }

    let mut compiled = reify_compiler::compile_with_stdlib(&parsed);

    // Enforce module-path declaration (spec §7.1/§7.2, task γ).
    // parsed.path == ModulePath::single(module_name) by construction (PRD D-6).
    if let Some(diag) = reify_compiler::check_module_path_decl(
        parsed.declared_module_path.as_ref(),
        &parsed.path,
    ) {
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
        for err in &parsed.errors {
            eprintln!("Parse error: {}", err.message);
        }
        return Err(ExitCode::FAILURE);
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

    let compiled =
        reify_compiler::module_dag::compile_entry_with_stdlib_cfg(&parsed, &resolver, cfg);

    for diag in &compiled.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    Ok(compiled)
}

/// One per-param binding parsed from a `--purpose` flag value.
///
/// `param` is the per-param name in the multi-pair form (`p:A`), or `None`
/// in the single-pair form (`name=entity`). `entity` is the structure ref.
#[derive(Debug, PartialEq)]
struct PurposeBinding {
    param: Option<String>,
    entity: String,
}

/// A single `--purpose <value>` activation: a purpose name and its bindings.
#[derive(Debug, PartialEq)]
struct PurposeActivation {
    name: String,
    bindings: Vec<PurposeBinding>,
}

/// Parse a `--purpose <value>` flag value.
///
/// Grammar:
/// - single-pair: `name=entity` → one binding `{ param: None, entity }`.
/// - multi-pair:  `name=p:A,q:B` → ordered bindings, each `{ param: Some(p), entity: A }`.
///
/// Errors on: missing `=`, empty name, empty binding list, empty segment
/// (e.g. trailing `,`), malformed `p:` / `:e` (empty side of `:`), or
/// multi-segment values where any segment lacks its `param:` name.
fn parse_purpose_flag(value: &str) -> Result<PurposeActivation, String> {
    let (name, rest) = value
        .split_once('=')
        .ok_or_else(|| format!("--purpose value '{}' is missing '='", value))?;
    if name.is_empty() {
        return Err(format!(
            "--purpose value '{}' has an empty purpose name",
            value
        ));
    }
    if rest.is_empty() {
        return Err(format!("--purpose value '{}' has no binding", value));
    }

    let mut bindings: Vec<PurposeBinding> = Vec::new();
    for segment in rest.split(',') {
        if segment.is_empty() {
            return Err(format!(
                "--purpose value '{}' has an empty binding segment",
                value
            ));
        }
        let binding = match segment.split_once(':') {
            Some((param, entity)) => {
                if param.is_empty() || entity.is_empty() {
                    return Err(format!(
                        "--purpose value '{}' has a malformed binding segment '{}'",
                        value, segment
                    ));
                }
                PurposeBinding {
                    param: Some(param.to_string()),
                    entity: entity.to_string(),
                }
            }
            None => PurposeBinding {
                param: None,
                entity: segment.to_string(),
            },
        };
        bindings.push(binding);
    }

    // Multi-binding values must use named bindings (per-param `p:E` form) so
    // each binding knows which purpose param it targets. Allowing
    // `name=A,B` would silently rely on positional order against a
    // user-declared param list, which is too brittle.
    if bindings.len() >= 2 && bindings.iter().any(|b| b.param.is_none()) {
        return Err(format!(
            "--purpose value '{}' has multiple bindings but at least one is missing its 'param:' name",
            value
        ));
    }

    Ok(PurposeActivation {
        name: name.to_string(),
        bindings,
    })
}

/// One parsed `--cfg <value>` argument.
///
/// - `Flag(name)` — a bare boolean flag (`--cfg debug`).
/// - `KeyValue { key, value }` — a `key=value` entry (`--cfg target=wasm`,
///   `--cfg feature=x`). An empty `value` is permitted (`--cfg target=`),
///   matching `CfgSet`'s kv empty-string semantics.
#[derive(Debug, PartialEq)]
enum CfgArg {
    Flag(String),
    KeyValue { key: String, value: String },
}

/// Parse a single `--cfg <value>` flag value into a [`CfgArg`].
///
/// Grammar:
/// - no `=` → bare flag; the value must be non-empty (`""` is an error).
/// - `key=value` → key/value entry; the key must be non-empty (`=v` is an
///   error). The value may be empty (`target=` yields an empty-string value).
///
/// Mirrors [`parse_purpose_flag`]'s error-message style.
fn parse_cfg_flag(value: &str) -> Result<CfgArg, String> {
    match value.split_once('=') {
        None => {
            if value.is_empty() {
                return Err("--cfg value is empty".to_string());
            }
            Ok(CfgArg::Flag(value.to_string()))
        }
        Some((key, val)) => {
            if key.is_empty() {
                return Err(format!("--cfg value '{}' has an empty key", value));
            }
            Ok(CfgArg::KeyValue {
                key: key.to_string(),
                value: val.to_string(),
            })
        }
    }
}

/// Build the active [`CfgSet`] from the repeated `--cfg <value>` arguments.
///
/// Starts from [`CfgSet::host_default`] (target = the compiling host's platform)
/// and folds each parsed [`CfgArg`] in order:
/// - `target=<v>` overrides the target;
/// - any other `key=value` is inserted into `kv`;
/// - a bare flag is inserted into `flags`.
///
/// Per PRD §4 D-2, `target` is host-defaulted and overridable ONLY by an explicit
/// `--cfg target=<v>`; bare flags and non-`target` key/values never clear it, so
/// passing a feature flag cannot silently disable platform gating.
fn build_cfg_set(values: &[String]) -> Result<CfgSet, String> {
    let mut cfg = CfgSet::host_default();
    for value in values {
        match parse_cfg_flag(value)? {
            CfgArg::KeyValue { key, value } if key == "target" => {
                cfg.target = Some(value);
            }
            CfgArg::KeyValue { key, value } => {
                cfg.kv.insert(key, value);
            }
            CfgArg::Flag(flag) => {
                cfg.flags.insert(flag);
            }
        }
    }
    Ok(cfg)
}

/// Usage line printed to stderr for any `reify check` usage error.
const CHECK_USAGE: &str =
    "Usage: reify check [--purpose <name>=<binding>]... [--cfg <key=value|flag>]... <file>";

fn cmd_check(args: &[String]) -> ExitCode {
    // Flag walk modeled on cmd_doc/cmd_gui: explicit handling of known flags
    // and explicit rejection of unknown `--`-prefixed tokens so a typo like
    // `--purpouse` fails loud instead of being silently treated as a file path.
    let mut purpose_values: Vec<String> = Vec::new();
    let mut cfg_values: Vec<String> = Vec::new();
    let mut file: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
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
        // When the module carries a `RepresentationWithin` assertion (detected
        // by `module_has_representation_within`), use the kernel-backed path:
        //   1. `set_capture_repr_tol(true)` — record deviation during tessellation.
        //   2. `tessellate_realizations(&compiled)` — populate `achieved_repr_tol`.
        //   3. `engine.check(&compiled)` — `dispatch_constraints` intercepts
        //      `RepresentationWithin` entries and reads from the populated map
        //      (type-name-scan fallback resolves the key; absent key → Indeterminate).
        //
        // This satisfies C1 ordering (tessellate-before-check) and C1 graceful
        // degradation (no OCCT kernel → `with_registered_kernel` returns a
        // None-kernel engine → tessellation skips → map stays empty →
        // Indeterminate → exit 0).
        //
        // When the module has NO `RepresentationWithin` constraints, keep the
        // existing `Engine::new(None)+check()` path verbatim (C2 — byte-identical
        // behavior and exit codes for all existing `reify check` inputs).
        let checker = SimpleConstraintChecker;
        let result = if module_has_representation_within(&compiled) {
            // Kernel-backed path for RepresentationWithin assertions (task-4199 γ).
            let mut engine =
                reify_eval::Engine::with_registered_kernel(Box::new(checker));
            engine.set_capture_repr_tol(true);
            engine.tessellate_realizations(&compiled);
            engine.check(&compiled)
        } else {
            // Existing lightweight path: no kernel, no tessellation (C2).
            let mut engine = reify_eval::Engine::new(Box::new(checker), None);
            engine.check(&compiled)
        };

        let outcome = report_eval_output(
            &result.constraint_results,
            &result.diagnostics,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        match outcome {
            ConstraintOutcome::AllSatisfied => {
                println!("All constraints satisfied.");
                ExitCode::SUCCESS
            }
            ConstraintOutcome::SomeIndeterminate(n) => {
                println!("No constraints violated ({n} indeterminate).");
                ExitCode::SUCCESS
            }
            ConstraintOutcome::SomeViolated => {
                println!("Some constraints violated.");
                ExitCode::FAILURE
            }
        }
    } else {
        // --purpose path: replicates the canonical
        // eval → activate_purpose → check_constraints_with_values sequence
        // (see crates/reify-eval/tests/purpose_activation.rs:1151-1177).
        // engine.check() does NOT visit purpose-injected constraints —
        // they live in snapshot.graph.constraints, visited only by
        // check_constraints_with_values.

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
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let eval_result = engine.eval(&compiled);

        // Activate each purpose in flag order; one check_constraints_with_values
        // call after the loop collects results for ALL injected constraints.
        for activation in &activations {
            // Single-binding form (name=entity, param==None): route through the
            // activate_purpose(name, entity) shim — byte-identical @entity prefix,
            // preserves existing single-param CLI tests (C6).
            // Everything else (len>=2, or len==1 with a named param like part:PartA):
            // route through activate_purpose_with_bindings for C2/C3 validation.
            let is_bare_single = activation.bindings.len() == 1
                && activation.bindings[0].param.is_none();

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
                if let Err(e) =
                    engine.activate_purpose_with_bindings(&activation.name, &pairs)
                {
                    eprintln!("Error: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }

        let (constraint_results, check_diags) =
            match engine.check_constraints_with_values(&eval_result.values) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return ExitCode::FAILURE;
                }
            };

        // Eval diagnostics first, then check diagnostics — chronological order.
        let mut diagnostics = eval_result.diagnostics.clone();
        diagnostics.extend(check_diags);

        let outcome = report_eval_output(
            &constraint_results,
            &diagnostics,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        // Same outcome → summary + exit-code mapping as the no-purpose path,
        // so a purpose-injected violation behaves identically to a structure
        // constraint violation in stdout and shell exit semantics.
        match outcome {
            ConstraintOutcome::AllSatisfied => {
                println!("All constraints satisfied.");
                ExitCode::SUCCESS
            }
            ConstraintOutcome::SomeIndeterminate(n) => {
                println!("No constraints violated ({n} indeterminate).");
                ExitCode::SUCCESS
            }
            ConstraintOutcome::SomeViolated => {
                println!("Some constraints violated.");
                ExitCode::FAILURE
            }
        }
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
    if args.is_empty() {
        eprintln!("Usage: reify build <file.ri> [-o <output>] [--verbose]");
        return ExitCode::FAILURE;
    }

    // Detect `--verbose` anywhere in the args.
    let verbose = args.iter().any(|a| a == "--verbose");

    // Pre-compute the index of the value that follows `-o` (if present and
    // followed by an argument).  This is reused both to build `output_path`
    // and to exclude the `-o` value from the positional-file scan below so
    // that `reify build -o out.step file.ri` doesn't mistakenly treat
    // `out.step` as the input file.
    let o_value_pos: Option<usize> = args
        .iter()
        .position(|a| a == "-o")
        .and_then(|i| if i + 1 < args.len() { Some(i + 1) } else { None });

    // Pick the first positional token: not a flag (`-`-prefixed) and not the
    // value following `-o`.  This makes flag ordering irrelevant, so both
    // `reify build file.ri --verbose` and `reify build --verbose file.ri`
    // correctly identify the input file.
    let file = match args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with('-') && Some(*i) != o_value_pos)
    {
        Some((_, f)) => f,
        None => {
            eprintln!("Usage: reify build <file.ri> [-o <output>] [--verbose]");
            return ExitCode::FAILURE;
        }
    };

    // Under `--verbose`, `-o` is optional (the full geometry build still runs
    // and provenance is printed; the file is only written if `-o` is present).
    // Without `--verbose`, `-o` is required (no behavior change).
    let output_path: Option<&String> = match o_value_pos {
        Some(i) => Some(&args[i]),
        None if verbose => None,
        None => {
            eprintln!("Usage: reify build <file.ri> [-o <output>] [--verbose]");
            return ExitCode::FAILURE;
        }
    };

    let format = match output_path {
        Some(p) if p.ends_with(".step") || p.ends_with(".stp") => ExportFormat::Step,
        Some(p) if p.ends_with(".stl") => ExportFormat::Stl,
        Some(p) if p.ends_with(".3mf") => ExportFormat::ThreeMF,
        Some(_) => {
            eprintln!("Unknown output format, defaulting to STEP");
            ExportFormat::Step
        }
        // No -o under --verbose: still run the full geometry build as STEP.
        None => ExportFormat::Step,
    };

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
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
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
    }

    match result.geometry_output {
        Some(data) => {
            if let Some(path) = output_path {
                if let Err(e) = std::fs::write(path, &data) {
                    eprintln!("Error writing {}: {}", path, e);
                    return ExitCode::FAILURE;
                }
                println!("Wrote {} ({} bytes)", path, data.len());
            }
            match outcome {
                ConstraintOutcome::AllSatisfied => ExitCode::SUCCESS,
                ConstraintOutcome::SomeIndeterminate(n) => {
                    println!("No constraints violated ({n} indeterminate).");
                    ExitCode::SUCCESS
                }
                ConstraintOutcome::SomeViolated => {
                    println!("Some constraints violated.");
                    ExitCode::FAILURE
                }
            }
        }
        None => {
            eprintln!("No geometry output produced");
            ExitCode::FAILURE
        }
    }
}

/// Configure a freshly-constructed [`reify_eval::Engine`] for use in `cmd_eval`:
/// wire the production [`reify_constraints::SolverRegistry`] and register all
/// compute trampolines so `@optimized` targets dispatch correctly.
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
/// here eliminates the duplicated `.with_solver` + `register_compute_fns` block
/// that would otherwise appear verbatim in each branch.
///
/// Both the FEA/buckling/modal trampolines (`register_compute_fns`) and the
/// shell-extract trampoline (`register_shell_extract_compute_fns`) are registered
/// here, mirroring the GUI's call pair (gui/src-tauri/src/engine.rs).  Without
/// the shell-extract registration, shell-classified `@optimized("solver::elastic_static")`
/// solves would hit `DispatchError::Failed` in `insert_shell_extract_upstream` and
/// emit a misleading "falling back to tet meshing" warning even though the FEA
/// trampoline independently re-classifies and runs the correct shell solve.
fn configured_eval_engine(engine: reify_eval::Engine) -> reify_eval::Engine {
    let mut engine = engine.with_solver(Box::new(reify_constraints::SolverRegistry::production()));
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
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
fn cmd_eval(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify eval <file>");
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

    // Normalise both branches to (values, diagnostics) for the shared print loop.
    // `configured_eval_engine` handles the shared `.with_solver` +
    // `register_compute_fns` setup; only the constructor and terminal call differ.
    let (values, diagnostics) = if module_has_geometry(&compiled) {
        // Geometry-bearing module: route through the kernel-backed build() path so
        // that run_post_processes/post_process_geometry_queries fires and resolves
        // geometry-query value cells (mass, centroid, volume, …).
        // geometry_output is discarded — reify eval is a value inspector only.
        let result = configured_eval_engine(
            reify_eval::Engine::with_registered_kernel(Box::new(SimpleConstraintChecker)),
        )
        .build(&compiled, reify_ir::ExportFormat::Step);
        (result.values, result.diagnostics)
    } else {
        // Plain numeric module: keep the existing lightweight eval() path so
        // non-geometry eval tests (cli_eval_auto_resolve, cli_stackup_eval,
        // cli_integration_smoke) remain on the exact unchanged code path.
        // Note: register_compute_fns is still required so `@optimized` targets
        // dispatch to their solver kernels (task 3794 / esc-3794-183).
        let result = configured_eval_engine(
            reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None),
        )
        .eval(&compiled);
        (result.values, result.diagnostics)
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

    if diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Usage line printed to stderr for any `reify doc` usage error.
const DOC_USAGE: &str =
    "Usage: reify doc <input.ri> [-o <path>] [--format html|markdown|json] [--split] [--compact]\n       reify doc --stdlib --out <dir>";

/// Output format for `reify doc`.
///
/// Default is `Html` per the PRD; the `--format` flag accepts `html`,
/// `markdown`, or `json`.  Bad values exit 2 with a usage error written to
/// stderr; the match is inline in `cmd_doc` since it has only one call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Markdown,
    Json,
}


fn cmd_doc(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
    }

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
                    eprintln!("Error: --format requires a value");
                    eprintln!("{}", DOC_USAGE);
                    return ExitCode::from(2u8);
                }
                format = Some(args[i + 1].clone());
                i += 2;
            }
            "-o" | "--out" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: {} requires a path", a);
                    eprintln!("{}", DOC_USAGE);
                    return ExitCode::from(2u8);
                }
                output = Some(args[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `doc`: {}", flag);
                eprintln!("{}", DOC_USAGE);
                return ExitCode::from(2u8);
            }
            _ => {
                if input.is_some() {
                    eprintln!("Error: unexpected extra positional argument: {}", a);
                    eprintln!("{}", DOC_USAGE);
                    return ExitCode::from(2u8);
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
            eprintln!("Error: --stdlib requires --out <dir>");
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
        if input.is_some() {
            eprintln!("Error: --stdlib does not accept an input file positional");
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
        if split {
            eprintln!("Error: --split is not valid with --stdlib");
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
        if compact {
            eprintln!("Error: --compact is not valid with --stdlib");
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
        if matches!(format.as_deref(), Some("json") | Some("markdown")) {
            eprintln!("Error: --stdlib only supports --format html (the default)");
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
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

    let input = match input {
        Some(s) => s,
        None => {
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
    };

    // Resolve `--format` (default `html`) into a typed `Format`.  Bad values
    // exit 2 with a usage-error on stderr.
    let format = match format.as_deref() {
        Some("html") => Format::Html,
        Some("markdown") => Format::Markdown,
        Some("json") => Format::Json,
        Some(other) => {
            eprintln!(
                "Error: unknown --format value: {} (expected html|markdown|json)",
                other
            );
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
        None => Format::Html,
    };

    // `--split` is markdown-only.  Reject json/html + split before doing any
    // expensive parse/compile work so usage errors are fast and stderr stays
    // crisp.
    if split && format != Format::Markdown {
        eprintln!("Error: --split is only valid with --format markdown");
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
    }

    // `--compact` is json-only.  Mirror the `--split` guard.
    if compact && format != Format::Json {
        eprintln!("Error: --compact is only valid with --format json");
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
    }

    // `--split` requires `-o <dir>` so we know where to write the per-item
    // files.  Hoisted above `parse_and_compile` so usage errors don't pay for parsing.
    // Reachable only when format == Markdown thanks to the guard above.
    if split && output.is_none() {
        eprintln!("Error: --split requires -o <directory>");
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
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
            let rendered =
                reify_doc::fmt_markdown::render_markdown(&model, Some(&xrefs), &opts);
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

/// Pure exit-decision helper for `reify check`.
///
/// Returns `true` when the overall outcome should cause a non-zero exit:
/// - [`ConstraintOutcome::SomeViolated`] always fails.
/// - [`ConstraintOutcome::SomeIndeterminate`] fails only when `strict` is `true`.
/// - [`ConstraintOutcome::AllSatisfied`] never fails.
///
/// Returns `bool` (not [`std::process::ExitCode`]) so the gate is directly
/// unit-testable; callers convert to `ExitCode` at the boundary.
fn check_fails(outcome: &ConstraintOutcome, strict: bool) -> bool {
    match outcome {
        ConstraintOutcome::SomeViolated => true,
        ConstraintOutcome::SomeIndeterminate(_) => strict,
        ConstraintOutcome::AllSatisfied => false,
    }
}

/// Outcome of constraint checking.
#[derive(Debug, PartialEq)]
enum ConstraintOutcome {
    /// Every constraint evaluated to `Satisfied`.
    AllSatisfied,
    /// No constraints violated, but some were `Indeterminate` (undef inputs).
    SomeIndeterminate(usize),
    /// At least one constraint evaluated to `Violated`.
    SomeViolated,
}

/// Return the display label for a constraint entry: the `label` field when
/// present, or the [`ConstraintNodeId`] Display representation as a fallback.
///
/// Shared by [`report_constraint_results`] and [`report_indeterminate_detail`]
/// so both use the same label-or-id formatting without duplication.
fn constraint_display_label(entry: &reify_eval::ConstraintCheckEntry) -> String {
    match entry.label.as_deref() {
        Some(l) => l.to_string(),
        None => format!("{}", entry.id),
    }
}

/// Write the strict-failure detail block for indeterminate constraints.
///
/// Emits a header naming the count of `Indeterminate` entries and a generic
/// "why" (inputs undefined), then one indented line per `Indeterminate` entry
/// using [`constraint_display_label`]. Only `Indeterminate` entries are listed;
/// `Satisfied` and `Violated` entries are silently skipped.
fn report_indeterminate_detail(
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) {
    let indet: Vec<_> = results
        .iter()
        .filter(|e| e.satisfaction == reify_ir::Satisfaction::Indeterminate)
        .collect();
    let count = indet.len();
    let _ = writeln!(
        out,
        "Strict check failed: {count} constraint(s) INDETERMINATE \
         \u{2014} inputs undefined (e.g. auto-params unresolved or geometry did not realize):"
    );
    for entry in indet {
        let _ = writeln!(out, "  {}", constraint_display_label(entry));
    }
}

/// Report constraint check results to the given writer.
///
/// Returns a [`ConstraintOutcome`] indicating the overall result.
/// Each entry is printed as `  {STATUS} {label}` where label falls back to the
/// constraint id's Display representation when `entry.label` is `None`.
///
/// **Indeterminate constraints are intentionally treated as non-violating.**
/// `Indeterminate` arises when a constraint's inputs are undefined — typically
/// from `auto` parameters not yet resolved by the solver. Treating these as
/// violations would block evaluations that are otherwise valid and break the
/// incremental evaluation engine. Only explicit `Violated` results cause
/// a `SomeViolated` outcome.
fn report_constraint_results(
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) -> ConstraintOutcome {
    let mut violated = false;
    let mut indeterminate_count: usize = 0;
    for entry in results {
        let status = match entry.satisfaction {
            Satisfaction::Satisfied => "OK",
            Satisfaction::Violated => {
                violated = true;
                "VIOLATED"
            }
            // Indeterminate does not count as violated — undef inputs
            // (auto params, partial evaluation) are not violations.
            // Undef propagates as quiet-NaN semantics.
            Satisfaction::Indeterminate => {
                indeterminate_count += 1;
                "INDETERMINATE"
            }
        };
        let _ = writeln!(out, "  {} {}", status, constraint_display_label(entry));
    }
    if violated {
        ConstraintOutcome::SomeViolated
    } else if indeterminate_count > 0 {
        ConstraintOutcome::SomeIndeterminate(indeterminate_count)
    } else {
        ConstraintOutcome::AllSatisfied
    }
}

/// Returns `true` if the compiled module contains geometry — i.e. any template
/// has a realization with at least one geometry operation, OR any value cell
/// is typed `reify_core::Type::Geometry`.
///
/// Two compile-time signals are OR'd (no kernel required):
///
/// * **(a) Realization with ops** — any template has a realization with at
///   least one geometry operation. This is the exact signal used by
///   `engine_build.rs`'s `had_realization_ops` gate internally.
///
/// * **(b) `Type::Geometry` value cell** — any template has a value cell
///   typed [`reify_core::Type::Geometry`]. This clause is intentionally
///   conservative/defensive: a module with only (b) true (e.g. a structure
///   that exposes a `Solid`-typed parameter without a realization op) is
///   still routed through `with_registered_kernel + build()`. In that
///   sub-case `build()` will skip the geometry pipeline (no ops → no
///   handles) and geometry-query cells stay `undef`, but the routing is
///   harmless: the kernel block is a no-op without ops and the broader gate
///   future-proofs detection for geometry-forwarding structures.
///
/// Both signals are present for `examples/spec-shape-physical.ri` (the
/// `box(...)` realization op + the `geometry : Solid` cell) and absent for
/// all existing non-geometry eval fixtures.
///
/// Used by `cmd_eval` to decide whether to route through the kernel-backed
/// `Engine::with_registered_kernel + build()` path (so that geometry-query
/// value cells such as `mass`/`centroid` are resolved by
/// `run_post_processes`/`post_process_geometry_queries`) or to stay on the
/// existing lightweight `Engine::new(None) + eval()` path for plain numeric
/// modules.
fn module_has_geometry(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        t.realizations.iter().any(|r| !r.operations.is_empty())
            || t.value_cells
                .iter()
                .any(|vc| vc.cell_type == reify_core::Type::Geometry)
    })
}

/// Returns `true` when any template in the module carries at least one
/// `RepresentationWithin(subject, bound)` constraint.
///
/// Used by [`cmd_check`] to decide whether to route through the kernel-backed
/// `set_capture_repr_tol(true)` → `tessellate_realizations` → `check` path
/// (so that `dispatch_constraints` can evaluate the assertion against the
/// populated `achieved_repr_tol` map) or to stay on the existing lightweight
/// `Engine::new(None)+check()` path for modules with no such assertion.
///
/// Reuses [`reify_eval::tolerance_combine::recognize_representation_within`]
/// so the recognition gate (UFC name + arity + arg0 ValueRef:StructureRef +
/// arg1 Literal Scalar LENGTH finite≥0) is the same canonical matcher used by
/// the engine's dispatch interception — a single gate implementation that
/// cannot drift (retiring the drift risk that lived in the extractor's TODO
/// before task 4199 γ).
///
/// Non-assertion modules: this function returns `false` and `cmd_check` keeps
/// the existing path verbatim (C2 — byte-identical behavior for all existing
/// `reify check` inputs).
fn module_has_representation_within(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        // Check direct template constraints first (the common case).
        let direct = t.constraints.iter().any(|c| {
            reify_eval::tolerance_combine::recognize_representation_within(&c.expr).is_some()
        });
        if direct {
            return true;
        }
        // Also check guarded-group constraints (true-branch + else-branch)
        // so a RepresentationWithin inside a `when ... { constraint ... }` block
        // is also detected.
        t.guarded_groups.iter().any(|g| {
            g.constraints.iter().chain(g.else_constraints.iter()).any(|c| {
                reify_eval::tolerance_combine::recognize_representation_within(&c.expr).is_some()
            })
        })
    })
}

/// Report constraint results and eval diagnostics in a consistent order.
///
/// Writes constraint status lines to `out` (via [`report_constraint_results`]),
/// then writes each diagnostic to `err`. This ensures both `cmd_check` and
/// `cmd_build` produce output in the same order: constraints first, diagnostics
/// second.
fn report_eval_output(
    constraint_results: &[reify_eval::ConstraintCheckEntry],
    diagnostics: &[reify_core::Diagnostic],
    out: &mut impl std::io::Write,
    err: &mut impl std::io::Write,
) -> ConstraintOutcome {
    let outcome = report_constraint_results(constraint_results, out);
    for diag in diagnostics {
        let _ = writeln!(err, "{}: {}", diag.severity, diag.message);
    }
    outcome
}

fn cmd_mcp_server(args: &[String]) -> ExitCode {
    // Parse optional file argument and --project-dir flag
    let mut file_path: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--project-dir" {
            if i + 1 < args.len() {
                project_dir = Some(args[i + 1].clone());
                i += 2;
                continue;
            } else {
                eprintln!("--project-dir requires a value");
                return ExitCode::FAILURE;
            }
        } else if file_path.is_none() {
            file_path = Some(args[i].clone());
        }
        i += 1;
    }

    let project_dir = project_dir
        .map(std::path::PathBuf::from)
        .or_else(|| {
            file_path
                .as_ref()
                .and_then(|f| std::path::Path::new(f).parent().map(|p| p.to_path_buf()))
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let context = mcp_context::CliToolContext::new(project_dir);

    if let Some(ref path) = file_path
        && let Err(e) = context.load_file(path)
    {
        eprintln!("Error loading {}: {}", path, e);
        return ExitCode::FAILURE;
    }

    let server = reify_mcp::McpServer::new(Arc::new(context));

    match tokio::runtime::Runtime::new() {
        Ok(rt) => {
            rt.block_on(server.run_stdio());
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
    use reify_eval::ConstraintCheckEntry;
    use reify_core::ConstraintNodeId;
    use reify_ir::Satisfaction;

    /// Helper: capture `report_constraint_results` output into an in-memory
    /// buffer and return the outcome plus the formatted output as a `String`.
    fn run_report(entries: &[ConstraintCheckEntry]) -> (ConstraintOutcome, String) {
        let mut buf = Vec::new();
        let result = report_constraint_results(entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        (result, output)
    }

    #[test]
    fn empty_entries_returns_all_satisfied_with_no_output() {
        let (result, output) = run_report(&[]);

        assert_eq!(
            result,
            ConstraintOutcome::AllSatisfied,
            "empty entries should return AllSatisfied (vacuous truth)"
        );
        assert!(
            output.is_empty(),
            "empty entries should produce no output, got: {:?}",
            output
        );
    }

    fn make_entry(
        entity: &str,
        index: u32,
        label: Option<&str>,
        satisfaction: Satisfaction,
    ) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            id: ConstraintNodeId::new(entity, index),
            label: label.map(|s| s.to_string()),
            satisfaction,
        }
    }

    #[test]
    fn all_satisfied_returns_true_and_formats_ok() {
        let entries = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Satisfied),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::AllSatisfied,
            "should return AllSatisfied when all satisfied"
        );
        assert!(output.contains("  OK stress_limit"));
        assert!(output.contains("  OK size_bound"));
        assert!(!output.contains("VIOLATED"));
    }

    #[test]
    fn violated_returns_false_and_formats_violated() {
        let entries = vec![
            make_entry("Part", 0, Some("max_force"), Satisfaction::Satisfied),
            make_entry("Part", 1, Some("clearance"), Satisfaction::Violated),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "should return SomeViolated when any violated"
        );
        assert!(output.contains("  OK max_force"));
        assert!(output.contains("VIOLATED clearance"));
    }

    #[test]
    fn indeterminate_formats_correctly_and_counts_as_satisfied() {
        let entries = vec![make_entry(
            "Beam",
            0,
            Some("load"),
            Satisfaction::Indeterminate,
        )];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeIndeterminate(1),
            "indeterminate should return SomeIndeterminate with count"
        );
        assert!(output.contains("INDETERMINATE load"));
    }

    #[test]
    fn violated_with_indeterminate_returns_some_violated() {
        let entries = vec![
            make_entry("Bracket", 0, Some("thickness"), Satisfaction::Violated),
            make_entry("Bracket", 1, Some("tolerance"), Satisfaction::Indeterminate),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "should return SomeViolated when violated + indeterminate coexist"
        );
        assert!(
            output.contains("VIOLATED thickness"),
            "output should contain 'VIOLATED thickness', got: {}",
            output
        );
        assert!(
            output.contains("INDETERMINATE tolerance"),
            "output should contain 'INDETERMINATE tolerance', got: {}",
            output
        );
        assert!(
            !output.contains("  OK "),
            "output should NOT contain '  OK ' when no constraints are satisfied, got: {}",
            output
        );
    }

    #[test]
    fn three_way_satisfied_violated_indeterminate_returns_some_violated() {
        let entries = vec![
            make_entry("Assembly", 0, Some("weight_limit"), Satisfaction::Satisfied),
            make_entry("Assembly", 1, Some("clearance"), Satisfaction::Violated),
            make_entry("Assembly", 2, Some("thermal"), Satisfaction::Indeterminate),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "violated takes priority over indeterminate: should return SomeViolated"
        );
        assert!(
            output.contains("  OK weight_limit"),
            "output should contain '  OK weight_limit', got: {}",
            output
        );
        assert!(
            output.contains("VIOLATED clearance"),
            "output should contain 'VIOLATED clearance', got: {}",
            output
        );
        assert!(
            output.contains("INDETERMINATE thermal"),
            "output should contain 'INDETERMINATE thermal', got: {}",
            output
        );
    }

    #[test]
    fn uses_id_display_as_fallback_when_label_is_none() {
        let entries = vec![make_entry("Gear", 2, None, Satisfaction::Satisfied)];
        let (_result, output) = run_report(&entries);

        // ConstraintNodeId Display: "Gear#constraint[2]"
        assert!(
            output.contains("  OK Gear#constraint[2]"),
            "should use id Display as fallback, got: {}",
            output
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
    fn uses_label_when_present() {
        let entries = vec![make_entry(
            "Axle",
            0,
            Some("torque_limit"),
            Satisfaction::Violated,
        )];
        let (_result, output) = run_report(&entries);

        assert!(
            output.contains("VIOLATED torque_limit"),
            "should use label, got: {}",
            output
        );
        assert!(
            !output.contains("Axle#constraint"),
            "should NOT contain id fallback when label is present"
        );
    }

    #[test]
    fn report_eval_output_writes_constraints_to_out_and_diagnostics_to_err() {
        let constraints = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Violated),
        ];
        let diagnostics = vec![
            reify_core::Diagnostic::warning("some msg"),
            reify_core::Diagnostic::error("bad thing"),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = report_eval_output(&constraints, &diagnostics, &mut out, &mut err);

        let out_str = String::from_utf8(out).unwrap();
        let err_str = String::from_utf8(err).unwrap();

        // (a) out buffer contains constraint status lines
        assert!(
            out_str.contains("  OK stress_limit"),
            "out should contain constraint OK line, got: {}",
            out_str
        );
        assert!(
            out_str.contains("VIOLATED size_bound"),
            "out should contain constraint VIOLATED line, got: {}",
            out_str
        );

        // (b) err buffer contains diagnostic lines
        assert!(
            err_str.contains("warning: some msg"),
            "err should contain warning diagnostic, got: {}",
            err_str
        );
        assert!(
            err_str.contains("error: bad thing"),
            "err should contain error diagnostic, got: {}",
            err_str
        );

        // (c) correct outcome
        assert_eq!(outcome, ConstraintOutcome::SomeViolated);
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

    #[test]
    fn parse_purpose_flag_accepts_single_pair() {
        // `name=entity` is the single-binding form: one binding with no
        // per-param name and the entity as the structure ref.
        let activation =
            parse_purpose_flag("mfg_ready=Bracket").expect("single-pair form should parse");
        assert_eq!(activation.name, "mfg_ready");
        assert_eq!(activation.bindings.len(), 1);
        assert_eq!(activation.bindings[0].param, None);
        assert_eq!(activation.bindings[0].entity, "Bracket");
    }

    #[test]
    fn parse_purpose_flag_accepts_multi_pair_named_bindings() {
        // `name=p:A,q:B` is the multi-pair form: ordered, each segment carries
        // its per-param name.
        let activation = parse_purpose_flag("fits_within=part:A,envelope:B")
            .expect("multi-pair form should parse");
        assert_eq!(activation.name, "fits_within");
        assert_eq!(activation.bindings.len(), 2);
        assert_eq!(activation.bindings[0].param.as_deref(), Some("part"));
        assert_eq!(activation.bindings[0].entity, "A");
        assert_eq!(activation.bindings[1].param.as_deref(), Some("envelope"));
        assert_eq!(activation.bindings[1].entity, "B");
    }

    #[test]
    fn parse_purpose_flag_rejects_malformed_values() {
        // Missing `=` — no purpose name vs. binding-list separator.
        assert!(parse_purpose_flag("noequals").is_err());
        // Empty purpose name.
        assert!(parse_purpose_flag("=Bracket").is_err());
        // Empty binding list.
        assert!(parse_purpose_flag("mfg_ready=").is_err());
        // Trailing empty segment after a comma (`p=a,`).
        assert!(parse_purpose_flag("p=a,").is_err());
    }

    #[test]
    fn parse_cfg_flag_parses_target_key_value() {
        // `target=wasm` is the key=value form: an explicit platform override.
        assert_eq!(
            parse_cfg_flag("target=wasm"),
            Ok(CfgArg::KeyValue {
                key: "target".to_string(),
                value: "wasm".to_string(),
            }),
        );
    }

    #[test]
    fn parse_cfg_flag_parses_bare_flag() {
        // A value with no `=` is a bare boolean flag.
        assert_eq!(
            parse_cfg_flag("linux"),
            Ok(CfgArg::Flag("linux".to_string())),
        );
    }

    #[test]
    fn parse_cfg_flag_parses_non_target_key_value() {
        // Any `key=value` (not just `target=`) is a key/value cfg entry.
        assert_eq!(
            parse_cfg_flag("feature=x"),
            Ok(CfgArg::KeyValue {
                key: "feature".to_string(),
                value: "x".to_string(),
            }),
        );
    }

    #[test]
    fn parse_cfg_flag_allows_empty_value() {
        // `target=` is the explicit empty-value form: the key is present and the
        // value is the empty string, matching cfg.rs's kv empty-string semantics.
        assert_eq!(
            parse_cfg_flag("target="),
            Ok(CfgArg::KeyValue {
                key: "target".to_string(),
                value: String::new(),
            }),
        );
    }

    #[test]
    fn parse_cfg_flag_rejects_empty_key() {
        // `=v` has an empty key — there is no cfg name to set.
        assert!(parse_cfg_flag("=v").is_err());
    }

    #[test]
    fn parse_cfg_flag_rejects_empty_input() {
        // An empty value is neither a flag nor a `key=value` — rejected.
        assert!(parse_cfg_flag("").is_err());
    }

    #[test]
    fn build_cfg_set_empty_is_host_default() {
        // No `--cfg` args ⇒ the host-default active cfg (target = host platform,
        // empty flags/kv), identical to CfgSet::host_default (PRD §4 D-2).
        assert_eq!(
            build_cfg_set(&[]),
            Ok(reify_compiler::cfg::CfgSet::host_default()),
        );
    }

    #[test]
    fn build_cfg_set_target_override_replaces_host() {
        // `--cfg target=wasm` overrides the host-default target.
        let cfg = build_cfg_set(&["target=wasm".to_string()]).expect("valid cfg");
        assert_eq!(cfg.target.as_deref(), Some("wasm"));
    }

    #[test]
    fn build_cfg_set_flag_keeps_host_target() {
        // A bare flag must NOT clear the host-default target (D-2 robustness): a
        // feature flag should never silently disable platform gating.
        let cfg = build_cfg_set(&["feat".to_string()]).expect("valid cfg");
        assert_eq!(cfg.target.as_deref(), Some(std::env::consts::OS));
        assert!(cfg.flags.contains("feat"));
    }

    #[test]
    fn build_cfg_set_non_target_kv_keeps_host_target() {
        // A non-`target` key=value lands in `kv` and leaves the host target intact.
        let cfg = build_cfg_set(&["k=v".to_string()]).expect("valid cfg");
        assert_eq!(cfg.kv.get("k").map(String::as_str), Some("v"));
        assert_eq!(cfg.target.as_deref(), Some(std::env::consts::OS));
    }

    #[test]
    fn build_cfg_set_rejects_malformed_value() {
        // A malformed `--cfg` value (empty key) propagates parse_cfg_flag's error.
        assert!(build_cfg_set(&["=bad".to_string()]).is_err());
    }

    // ── step-1: RED unit tests for check_fails ────────────────────────────────

    #[test]
    fn check_fails_all_satisfied_is_false_regardless_of_strict() {
        assert!(
            !check_fails(&ConstraintOutcome::AllSatisfied, false),
            "AllSatisfied + strict=false should be false"
        );
        assert!(
            !check_fails(&ConstraintOutcome::AllSatisfied, true),
            "AllSatisfied + strict=true should be false"
        );
    }

    #[test]
    fn check_fails_some_violated_is_true_regardless_of_strict() {
        assert!(
            check_fails(&ConstraintOutcome::SomeViolated, false),
            "SomeViolated + strict=false should be true"
        );
        assert!(
            check_fails(&ConstraintOutcome::SomeViolated, true),
            "SomeViolated + strict=true should be true"
        );
    }

    #[test]
    fn check_fails_some_indeterminate_false_when_not_strict() {
        assert!(
            !check_fails(&ConstraintOutcome::SomeIndeterminate(1), false),
            "SomeIndeterminate + strict=false should be false (indeterminate is not a failure without --strict)"
        );
        assert!(
            !check_fails(&ConstraintOutcome::SomeIndeterminate(3), false),
            "SomeIndeterminate(3) + strict=false should be false"
        );
    }

    #[test]
    fn check_fails_some_indeterminate_true_when_strict() {
        assert!(
            check_fails(&ConstraintOutcome::SomeIndeterminate(1), true),
            "SomeIndeterminate + strict=true should be true (--strict promotes indeterminate to failure)"
        );
        assert!(
            check_fails(&ConstraintOutcome::SomeIndeterminate(2), true),
            "SomeIndeterminate(2) + strict=true should be true"
        );
    }

    // ── end step-1 ────────────────────────────────────────────────────────────

    // ── step-3: RED unit tests for report_indeterminate_detail ───────────────

    #[test]
    fn report_indeterminate_detail_lists_only_indeterminate_entries() {
        // Mix: satisfied, indeterminate (with label), violated, indeterminate
        // (no label — must fall back to id Display "Foo#constraint[3]").
        let entries = vec![
            make_entry("Bracket", 0, Some("c_ok"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("c_bad"), Satisfaction::Indeterminate),
            make_entry("Bracket", 2, Some("c_v"), Satisfaction::Violated),
            make_entry("Foo", 3, None, Satisfaction::Indeterminate),
        ];
        let mut buf = Vec::new();
        report_indeterminate_detail(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        // (a) Header names the count (2) and mentions undefined inputs.
        assert!(
            output.contains("2"),
            "header should name the indeterminate count (2), got: {output}"
        );
        assert!(
            output.contains("undefined"),
            "header should mention undefined inputs, got: {output}"
        );

        // (b) Lists "c_bad" and id-Display fallback "Foo#constraint[3]".
        assert!(
            output.contains("c_bad"),
            "output should list 'c_bad', got: {output}"
        );
        assert!(
            output.contains("Foo#constraint[3]"),
            "output should list id fallback 'Foo#constraint[3]', got: {output}"
        );

        // (c) Does NOT list "c_ok" or "c_v" (only Indeterminate entries).
        assert!(
            !output.contains("c_ok"),
            "output must NOT list satisfied constraint 'c_ok', got: {output}"
        );
        assert!(
            !output.contains("c_v"),
            "output must NOT list violated constraint 'c_v', got: {output}"
        );
    }

    #[test]
    fn report_indeterminate_detail_single_entry_count_one() {
        let entries = vec![
            make_entry("Part", 0, Some("load"), Satisfaction::Indeterminate),
        ];
        let mut buf = Vec::new();
        report_indeterminate_detail(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        // Count is 1 and the labelled constraint is listed.
        assert!(
            output.contains("1"),
            "header should name the indeterminate count (1), got: {output}"
        );
        assert!(
            output.contains("load"),
            "output should list 'load', got: {output}"
        );
    }

    // ── end step-3 ────────────────────────────────────────────────────────────

    // ── step-5: RED unit tests for finish_check writer output ────────────────

    #[test]
    fn finish_check_non_strict_indeterminate_emits_unchanged_summary() {
        // (a) !strict + SomeIndeterminate(1) → byte-identical "No constraints
        // violated (1 indeterminate).\n" regression guard.
        let entries = vec![
            make_entry("Bracket", 1, Some("tolerance"), Satisfaction::Indeterminate),
        ];
        let outcome = ConstraintOutcome::SomeIndeterminate(1);
        let mut buf = Vec::new();
        finish_check(&outcome, &entries, false, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(
            output,
            "No constraints violated (1 indeterminate).\n",
            "non-strict SomeIndeterminate(1) must produce the exact legacy summary line"
        );
    }

    #[test]
    fn finish_check_strict_indeterminate_emits_detail_not_legacy_line() {
        // (b) strict + SomeIndeterminate → buffer contains "Strict check failed"
        // and names the indeterminate constraint; must NOT contain "No constraints
        // violated".
        let entries = vec![
            make_entry("Bracket", 1, Some("tolerance"), Satisfaction::Indeterminate),
        ];
        let outcome = ConstraintOutcome::SomeIndeterminate(1);
        let mut buf = Vec::new();
        finish_check(&outcome, &entries, true, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Strict check failed"),
            "strict SomeIndeterminate must contain 'Strict check failed', got: {output}"
        );
        assert!(
            output.contains("tolerance"),
            "strict SomeIndeterminate must name the constraint 'tolerance', got: {output}"
        );
        assert!(
            !output.contains("No constraints violated"),
            "strict SomeIndeterminate must NOT contain 'No constraints violated', got: {output}"
        );
    }

    #[test]
    fn finish_check_all_satisfied_either_strict() {
        // (c) AllSatisfied (either strict value) → "All constraints satisfied.\n".
        let entries: Vec<reify_eval::ConstraintCheckEntry> = vec![];
        let outcome = ConstraintOutcome::AllSatisfied;
        for strict in [false, true] {
            let mut buf = Vec::new();
            finish_check(&outcome, &entries, strict, &mut buf);
            let output = String::from_utf8(buf).unwrap();
            assert_eq!(
                output,
                "All constraints satisfied.\n",
                "AllSatisfied (strict={strict}) must produce 'All constraints satisfied.'"
            );
        }
    }

    #[test]
    fn finish_check_some_violated_either_strict() {
        // (d) SomeViolated (either strict value) → "Some constraints violated.\n".
        let entries: Vec<reify_eval::ConstraintCheckEntry> = vec![];
        let outcome = ConstraintOutcome::SomeViolated;
        for strict in [false, true] {
            let mut buf = Vec::new();
            finish_check(&outcome, &entries, strict, &mut buf);
            let output = String::from_utf8(buf).unwrap();
            assert_eq!(
                output,
                "Some constraints violated.\n",
                "SomeViolated (strict={strict}) must produce 'Some constraints violated.'"
            );
        }
    }

    // ── end step-5 ────────────────────────────────────────────────────────────

    #[test]
    fn report_eval_output_returns_correct_outcome_variants() {
        let no_diags: Vec<reify_core::Diagnostic> = vec![];

        // AllSatisfied: all constraints OK
        {
            let entries = vec![
                make_entry("A", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("A", 1, Some("c2"), Satisfaction::Satisfied),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::AllSatisfied);
        }

        // SomeViolated: at least one violated
        {
            let entries = vec![
                make_entry("B", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("B", 1, Some("c2"), Satisfaction::Violated),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::SomeViolated);
        }

        // SomeIndeterminate: indeterminate but no violated
        {
            let entries = vec![
                make_entry("C", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("C", 1, Some("c2"), Satisfaction::Indeterminate),
                make_entry("C", 2, Some("c3"), Satisfaction::Indeterminate),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::SomeIndeterminate(2));
        }
    }
}

#[cfg(test)]
mod eval_geometry_gate_tests {
    use super::module_has_geometry;

    /// RED until `module_has_geometry` is implemented (step-2).
    ///
    /// Compiles two sources with the stdlib:
    /// 1. A geometry-bearing `Bracket : Physical` module (has a `box(...)` realization
    ///    op and a `geometry : Solid` value cell) — expects `true`.
    /// 2. A plain numeric module with no realization ops and no `Geometry`-typed
    ///    cells — expects `false`.
    ///
    /// No OCCT required: the predicate is compile-time only.
    #[test]
    fn module_has_geometry_detects_geometry_vs_plain() {
        // Geometry-bearing: Bracket : Physical has `param geometry : Solid = box(...)`
        // (a realization with operations) and a `geometry : Solid` value cell.
        let geometry_source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Steel_AISI_1045()
}
"#;
        let compiled_geo =
            reify_test_support::parse_and_compile_with_stdlib(geometry_source);
        assert!(
            module_has_geometry(&compiled_geo),
            "Bracket : Physical should be detected as a geometry module"
        );

        // Plain numeric: no realization, no Geometry-typed cells.
        let plain_source = r#"
structure def Plain {
    param x : Real = 1.0
    let y = x + 2.0
}
"#;
        let compiled_plain =
            reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_geometry(&compiled_plain),
            "Plain numeric module should NOT be detected as a geometry module"
        );

        // Third case: Type::Geometry cell only — no realization operations.
        // This exercises clause (b) of module_has_geometry independently of
        // clause (a). The Bracket test above triggers both clauses simultaneously;
        // this case ensures a regression that breaks the cell_type check while
        // leaving the realization check intact would still fail.
        //
        // Constructed directly via the builder API (no stdlib compile needed)
        // so we can precisely control which fields are set.
        let geo_cell_only = reify_test_support::CompiledModuleBuilder::new(
            reify_core::ModulePath::new(vec!["test".to_string()]),
        )
        .template(
            reify_test_support::TopologyTemplateBuilder::new("GeoCell")
                .param("GeoCell", "shape", reify_core::Type::Geometry, None)
                .build(),
        )
        .build();
        assert!(
            module_has_geometry(&geo_cell_only),
            "Module with a Type::Geometry value cell (no realization ops) should be \
             detected as geometry (clause (b) of module_has_geometry)"
        );
    }
}

#[cfg(test)]
mod representation_within_gate_tests {
    use super::module_has_representation_within;

    /// Non-OCCT routing gate test: `module_has_representation_within` must
    /// correctly detect a `RepresentationWithin` constraint in real compiled
    /// IR, and must return `false` for a plain module without one.
    ///
    /// This test is always-running (no OCCT guard) so that a regression in
    /// template-level recognition (e.g. if the compiler changes the IR shape
    /// for resolved stdlib calls) fails CI independently of OCCT availability.
    /// Without this test, the OCCT-gated CLI test would silently pass even if
    /// routing is broken: in stub mode `cmd_check` exits 0 regardless of
    /// whether it took the kernel-backed path or the lightweight path.
    ///
    /// Uses `parse_and_compile` (no stdlib) because `mm` is a built-in length
    /// unit, mirroring the INTERCEPTION_SOURCE fixture used by the engine-level
    /// interception tests in `representation_within_assertion.rs`.
    #[test]
    fn module_has_representation_within_detects_assertion_vs_plain() {
        // Assertion module: Checker carries a `RepresentationWithin(subject, 1mm)`
        // template constraint — must be detected (returns `true`) so that
        // `cmd_check` routes through the kernel-backed
        // `set_capture_repr_tol(true)` → `tessellate_realizations` → `check`
        // path.
        let assertion_source = r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom
    param w : Real = 5.0
    constraint RepresentationWithin(subject, 1mm)
    constraint w > 0.0
}
"#;
        let compiled_assertion = reify_test_support::parse_and_compile(assertion_source);
        assert!(
            module_has_representation_within(&compiled_assertion),
            "module with a RepresentationWithin template constraint should be \
             detected (routing gate must return true)"
        );

        // Plain module: no RepresentationWithin constraints anywhere — must NOT
        // be detected (returns `false`) so that `cmd_check` keeps the existing
        // lightweight `Engine::new(None)+check()` path (C2).
        let plain_source = r#"
structure Plain {
    param x : Real = 1.0
    constraint x > 0.0
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile(plain_source);
        assert!(
            !module_has_representation_within(&compiled_plain),
            "module without RepresentationWithin constraints must NOT be detected \
             (routing gate must return false — C2 path preserved)"
        );
    }
}
