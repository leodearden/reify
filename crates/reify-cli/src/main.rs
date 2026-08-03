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

mod cache;
mod dev;
mod mcp_context;
use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_ir::{ExportFormat, Satisfaction, UndefCause};

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
const CHECK_USAGE: &str = "Usage: reify check [--strict] [--purpose <name>=<binding>]... [--cfg <key=value|flag>]... <file>";

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
/// **Known limitation:** `reify check` still surfaces the engine-owned
/// `Severity::Error` "no registered compute trampoline (falling back to
/// body-inlining)" diagnostic on stderr for `@optimized` FEA solves.  The
/// severity is owned by `engine_eval.rs`; downgrading it to a warning is a
/// separate engine-side concern (deferred, out of scope for this CLI task).
/// Adopt post-realization constraint verdicts from a captured [`BuildResult`]
/// onto the authoritative [`CheckResult`], for entries `check()` left
/// `Indeterminate`.
///
/// # Why this exists (task 5748, esc-5748-1 ruling)
///
/// `cmd_check`'s kernel-backed arm calls `build()` and then, separately,
/// `check()` as the constraint source of record.  Routing a geometry-bearing
/// module through `build()` (D1) resolves geometry-query value cells
/// (`centroid`, `moment_of_inertia`, …) via
/// `run_post_processes`/`post_process_geometry_queries` — but ONLY into
/// build()'s own `values` map.  `Engine::check()` then opens with a fresh
/// `self.eval(module)` and checks constraints against that pure-eval map, so
/// those cells are `Undef` again at verdict time and every constraint reading
/// one degrades to `Indeterminate`.  Routing alone is therefore observably
/// inert on the verdict axis; this merge is what makes D1 user-visible.
///
/// `build_with_geometry_output` carries a dedicated task-4229 pass
/// (engine_build.rs:4908-4990) that re-checks geometry-derived constraints
/// against the post-realization value map; `Engine::check()` has no equivalent.
/// This function is the CLI-side mirror of that pass's merge loop.
///
/// # Contract
///
/// * **Upgrade-only.**  Only entries whose `check()` verdict is
///   `Indeterminate` are touched.  A definite `Satisfied`/`Violated` from
///   `check()` is never regressed, and an `Indeterminate` build verdict never
///   overwrites anything.  This is what keeps build()'s copy safe to consult
///   despite being computed before `tessellate_realizations`: a
///   `RepresentationWithin` / GD&T `Conforms` entry that check() resolved
///   definitively is untouched, and one that check() left `Indeterminate` is
///   also `Indeterminate` in build()'s copy (build()'s internal `check()` runs
///   with an empty `achieved_repr_tol` and freshly-cleared
///   `realization_handles`), so no stale tessellate-dependent verdict can leak
///   through.  Only the geometry-query axis — where build() has strictly MORE
///   information than check() — actually moves.
/// * **Entries are matched by `id`**, never by position: the two calls build
///   their result vectors independently.
/// * **The stale warning is dropped.**  When an entry upgrades, the
///   `ConstraintIndeterminate` diagnostic `check()` emitted for it (e.g.
///   "indeterminate: undefined inputs: BoltFlange.moi_principal") is no longer
///   true, so it is removed — matched by the same needle the checker embeds
///   (the constraint label when present, else the raw id), mirroring
///   engine_build.rs's own retain.
/// * **Diagnostics are otherwise untouched** here; D2's separate
///   `merge_build_diagnostics` owns appending build()-only entries.
///
/// `None` build result (lightweight arm, or a kernel-backed module that took
/// only the `tessellate_realizations` side effect) → total no-op, so every
/// pre-5748 input stays byte-identical.
fn merge_post_build_verdicts(
    result: &mut reify_eval::CheckResult,
    build_result: Option<&reify_eval::BuildResult>,
) {
    let Some(build_result) = build_result else {
        return;
    };
    // Needles of the entries that upgraded, collected during the walk and
    // applied to `diagnostics` afterwards (one `&mut result` borrow at a time).
    let mut upgraded_needles: Vec<String> = Vec::new();
    for entry in result.constraint_results.iter_mut() {
        if entry.satisfaction != reify_ir::Satisfaction::Indeterminate {
            continue;
        }
        let Some(new_sat) = build_result
            .constraint_results
            .iter()
            .find(|r| r.id == entry.id)
            .map(|r| r.satisfaction)
        else {
            continue;
        };
        if new_sat == reify_ir::Satisfaction::Indeterminate {
            continue;
        }
        entry.satisfaction = new_sat;
        upgraded_needles.push(entry.label.clone().unwrap_or_else(|| entry.id.to_string()));
    }
    if upgraded_needles.is_empty() {
        return;
    }
    // The "indeterminate: undefined inputs: …" warnings check() emitted for the
    // upgraded constraints are now false — drop them.
    result.diagnostics.retain(|d| {
        !(d.code == Some(reify_core::DiagnosticCode::ConstraintIndeterminate)
            && upgraded_needles.iter().any(|n| d.message.contains(n)))
    });
}

/// Drop the `ConstraintIndeterminate` diagnostics in `build_diags` that the
/// AUTHORITATIVE `constraint_results` positively falsify.
///
/// # Why this exists (task 5748, reviewer_comprehensive review fix)
///
/// [`merge_post_build_verdicts`] is the inward leg — it adopts build()'s
/// verdicts onto `check()`'s results.  This is its outward mirror, and D2's
/// missing return leg: the diagnostic merge is ONE-DIRECTIONAL by construction,
/// so nothing anywhere drops a stale indeterminacy claim `build()` made about a
/// constraint the later, authoritative `check()` resolved on its own.
///
/// * `build()`'s internal task-4229 retain (engine_build.rs:5000-5004) drops
///   only the warnings BUILD itself upgraded, and is gated on its own result
///   list still holding an `Indeterminate`.
/// * [`merge_post_build_verdicts`] retains only over `check()`'s OWN list, and
///   only for the entries IT upgraded.
///
/// Neither knows about the other pass's verdicts, so without this filter
/// `check` prints a self-contradiction: stdout `OK SphereCheck#constraint[0]` +
/// `All constraints satisfied.` while stderr carries `warning: constraint
/// SphereCheck#constraint[0] indeterminate: undefined inputs:
/// SphereCheck.subject` (measured on `tests/fixtures/dfm_with_repr_within.ri`).
///
/// # Contract
///
/// * **Drop on positive falsification only.**  A diagnostic is removed only
///   when `constraint_results` holds a DEFINITE (`Satisfied`/`Violated`) verdict
///   for that same constraint.  An id the authoritative list never mentions is
///   left alone, so a build-only diagnostic about a constraint outside that list
///   still reaches the user — which is what preserves PRD D2's "every
///   build()-only diagnostic appears at least once".
/// * **Only `DiagnosticCode::ConstraintIndeterminate` is in scope.**  A verdict
///   falsifies the INDETERMINACY claim and nothing else; a `ConstraintViolated`
///   (or uncoded) line naming the same constraint survives untouched.
/// * **The needle prefers the label**, exactly as [`merge_post_build_verdicts`]
///   and engine_build.rs's `labeled_diagnostics` rewrite do: a labeled
///   constraint's diagnostic embeds the label, not the raw id.
/// * **The matcher is ANCHORED** on `constraint {needle} indeterminate` — it
///   spans the needle AND the trailing ` indeterminate` token, which every
///   emitted variant carries (reify-constraints/src/lib.rs:187/196/201).  This
///   is a deliberate STRENGTHENING over the unanchored `contains(needle)` used
///   at :560 and engine_build.rs:5003: this filter walks ALL definite verdicts
///   (a far larger needle set than those two, which only walk the entries THEY
///   upgraded), so prefix-collision exposure is materially higher.  A bare
///   `contains` would let a definite `Foo#constraint[1]` delete
///   `Foo#constraint[10]`'s still-true warning; `[1] indeterminate` is not a
///   substring of `[10] indeterminate`, so the anchored form cannot.
///
/// Severity-agnostic by code, but in practice this only ever removes
/// `Warning`-severity entries.  No exit code can move either way:
/// `report_eval_output`'s outcome derives solely from `constraint_results` via
/// `report_constraint_results`, never from the diagnostic list.
///
/// An empty `constraint_results` (or one holding no definite verdict) falsifies
/// nothing → `build_diags` verbatim, so every pre-fix input stays
/// byte-identical (C2).
fn drop_falsified_indeterminate_diagnostics(
    build_diags: &[reify_core::Diagnostic],
    constraint_results: &[reify_eval::ConstraintCheckEntry],
) -> Vec<reify_core::Diagnostic> {
    let anchors: Vec<String> = constraint_results
        .iter()
        .filter(|e| e.satisfaction != reify_ir::Satisfaction::Indeterminate)
        .map(|e| {
            format!(
                "constraint {} indeterminate",
                e.label.clone().unwrap_or_else(|| e.id.to_string())
            )
        })
        .collect();
    if anchors.is_empty() {
        return build_diags.to_vec();
    }
    build_diags
        .iter()
        .filter(|d| {
            !(d.code == Some(reify_core::DiagnosticCode::ConstraintIndeterminate)
                && anchors.iter().any(|a| d.message.contains(a)))
        })
        .cloned()
        .collect()
}

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
            let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
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
                // The `BuildResult` is CAPTURED, not discarded (task 5748,
                // esc-5748-1 ruling). Routing through `build()` is necessary
                // but NOT sufficient to make those cells reach a verdict:
                // `Engine::check()` (engine_constraints.rs:1841) opens with a
                // fresh `self.eval(module)` and checks constraints against
                // THAT pure-eval value map, so build()'s post-processed values
                // are thrown away and the constraint stays `Indeterminate`.
                // Only `build_with_geometry_output` carries the task-4229
                // "re-check geometry-derived constraints after the realization
                // loop" pass (engine_build.rs:4908-4990); `check()` has no
                // equivalent. So build()'s `constraint_results` are the ONLY
                // post-realization verdicts available on this path, and the
                // merge below adopts them where — and only where — check() had
                // nothing better. See `merge_post_build_verdicts`.
                build_result = Some(engine.build(&compiled, ExportFormat::Step));
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
            let mut engine = reify_eval::Engine::new(Box::new(checker), None);
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
        // Ordering is load-bearing and runs AFTER `merge_post_build_verdicts`:
        // that pass drops the now-false `ConstraintIndeterminate` warnings for
        // the entries it upgraded, and merging first would let build()'s copy
        // of those same warnings reappend them.
        //
        // The plan's separate `build_diagnostics` accumulator is subsumed by the
        // `build_result` capture the esc-5748-1 ruling landed: `None` (the
        // lightweight arm, and the kernel-backed sub-case that takes only the
        // `tessellate_realizations` side effect) yields an empty slice, so those
        // paths stay byte-identical (C2) with no redundant clone.
        //
        // The merge is one-directional by construction — it only ever ADDS
        // build entries — so it needs a return leg, and
        // `drop_falsified_indeterminate_diagnostics` is it: build()'s internal
        // task-4229 retain (engine_build.rs:5000-5004) drops only the warnings
        // BUILD upgraded, and `merge_post_build_verdicts` only those IT
        // upgraded; neither knows about the authoritative `check()` above, so
        // without this filter build's stale "indeterminate" claim about a
        // constraint check() resolved on its own reaches stderr and contradicts
        // check's own stdout verdict.
        //
        // ORDERING IS LOAD-BEARING: this runs AFTER `merge_post_build_verdicts`
        // so it reads the POST-upgrade `result.constraint_results` — an entry
        // that pass just upgraded to a definite verdict must also falsify
        // build's copy of the matching warning.
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

        // Both ad-hoc escalations below read the MERGED set, not check()'s alone
        // (PRD D2). Safe in both directions: merged ⊇ check()'s set, so nothing
        // that fails today can start passing; and the entries build() adds are
        // realization-only (`compile_geometry_op` / kernel dispatch), carrying
        // neither `DiagnosticCode::GdtIllegalModifier` nor an `E_DFM_` message
        // prefix, so nothing that passes today starts failing. Leaf γ (#5403)
        // replaces both predicates with a single general `Severity::Error` gate
        // over this same merged set — feeding one set to both reporting and the
        // exit decision here is what keeps γ's diff small.
        //
        // Escalate to FAILURE when a GdtIllegalModifier error is present.
        // Scoped strictly to this code so non-GD&T modules are byte-identical.
        // GdtRemoved2018 warnings remain non-fatal (exit 0 preserved).
        if merged_diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        {
            return ExitCode::FAILURE;
        }

        // Escalate to FAILURE when any DFM Error-severity diagnostic is present
        // (e.g. E_DFM_OVERHANG, E_DFM_UNDERCUT from DFMSeverity.Error rules).
        // `dfm_has_error_diagnostic` matches on the `E_DFM_` message prefix so
        // unrelated code-less Error diagnostics co-resident in a DFM module
        // (e.g. FEA "no registered compute trampoline") are NOT escalated.
        // Gated on `has_dfm_rule` as a first-pass guard so non-DFM modules
        // remain byte-identical (C2).
        // DFMSeverity.Warning diagnostics (W_DFM_OVERHANG etc.) are non-fatal —
        // exit 0, never a false positive (C1 graceful degradation).
        if has_dfm_rule && dfm_has_error_diagnostic(&merged_diagnostics) {
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
        // (task 4589): diagnostics are folded in before `report_eval_output` below
        // and the same GdtIllegalModifier → FAILURE escalation is applied after
        // `finish_check`.  The former known-limitation comment (task 4475 β scope)
        // has been resolved.

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
            let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
            let r = engine.build(&compiled, ExportFormat::Step);
            (r.values, r.diagnostics, engine)
        } else {
            let mut engine = reify_eval::Engine::new(Box::new(checker), None);
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
        // never re-runs eval (engine_constraints.rs:700), so on the geometry
        // branch the verdicts below are computed directly against build()'s
        // POST-processed values. The staleness gap that forced
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

        // Front-end diagnostics first, then check diagnostics — chronological order.
        let mut diagnostics = if used_build {
            // D2 (task 5748) for sub-path (c). `build()` re-runs the eval
            // front-end internally, so its list BOTH carries internal
            // duplicates (measured: the `mirror(...)` 'ox' compile error twice
            // for one call site) AND overlaps whatever
            // `check_constraints_with_values` reports. Route both through the
            // accumulating dedup: the build entries seed the list in
            // chronological order, the check entries merge in behind them, and
            // an entry produced by both passes is reported exactly once.
            //
            // First the same return leg sub-path (b) applies (see the D2 block
            // there): the merge only ever ADDS build entries, so build's stale
            // `ConstraintIndeterminate` claims are filtered against the
            // AUTHORITATIVE `constraint_results` from
            // `check_constraints_with_values` before they can be merged in.
            // Applied symmetrically here so the two sub-paths cannot drift.
            // The known divergence path on this branch is the `UnifiedDag`
            // `declined` set (engine_build.rs:4952-4966), whose constraints
            // keep build's Indeterminate warning while the CLI-side check can
            // still reach a definite verdict for them.
            let front_end_diagnostics = drop_falsified_indeterminate_diagnostics(
                &front_end_diagnostics,
                &constraint_results,
            );
            let deduped_build = merge_build_diagnostics(&[], &front_end_diagnostics);
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

        // GD&T legality pass (task 4589): runs over post-eval values identically
        // to the non-purpose branch.  Folded in BEFORE report_eval_output so the
        // error prints to stderr alongside other diagnostics.
        diagnostics.extend(engine.run_gdt_check_passes(&compiled, &values));

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

        // Escalate to FAILURE when a GdtIllegalModifier error is present —
        // mirrors the GdtIllegalModifier escalation in the no-purpose branch
        // of cmd_check (the block that follows `finish_check` there).
        // GdtRemoved2018 warnings remain non-fatal (exit 0 preserved).
        //
        // NOTE (task 5748): this is the ONLY ad-hoc escalation on this branch —
        // there is no DFM-Error counterpart to sub-path (b)'s
        // `has_dfm_rule && dfm_has_error_diagnostic(...)` gate. That asymmetry
        // is PRE-EXISTING and deliberately NOT fixed here; leaf γ (#5403)
        // closes it incidentally when it replaces both ad-hoc predicates with a
        // single general `Severity::Error` gate over the merged set.
        if diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        {
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
    // rather than body-inlining.  Without these registrations the engine emits an
    // Error-severity "no registered compute trampoline" diagnostic and FEA-result
    // constraints evaluate to Indeterminate.
    //
    // NOTE: cmd_build intentionally does NOT call `configured_eval_engine` (which
    // also adds `.with_solver(production())`).  The DimensionalSolver resolves
    // `auto` params via a synthetic Chebyshev-centre objective even when no explicit
    // `minimize`/`maximize` directive is present; wiring it here would change the
    // observable behaviour of existing `auto`-param fixtures (bracket_indeterminate,
    // bracket_all_indeterminate) from INDETERMINATE → SATISFIED.  cmd_build's
    // solver-free posture is intentional; cmd_eval wires the full solver via
    // `configured_eval_engine` because it explicitly models the full evaluation path.
    // The FEA trampoline is pure-Rust and independent of the DimensionalSolver, so
    // the solve runs correctly here without `.with_solver`.  See `build_is_success`
    // for the (c) exit-code gate.
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
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
                    let has_error_diagnostic =
                        result.diagnostics.iter().any(|d| d.severity == Severity::Error);
                    if build_is_success(&outcome, has_error_diagnostic) {
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
            let has_error_diagnostic =
                all_diagnostics.iter().any(|d| d.severity == Severity::Error);
            if build_is_success(&outcome, has_error_diagnostic) {
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
/// here eliminates the duplicated `.with_solver` + [`register_compute_trampolines`]
/// block that would otherwise appear verbatim in each branch.
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
/// [`register_compute_trampolines`] directly (without `.with_solver`) to preserve
/// cmd_build's solver-free posture for `auto`-param fixtures.  See the comment in
/// `cmd_build` for rationale.
fn configured_eval_engine(engine: reify_eval::Engine) -> reify_eval::Engine {
    let mut engine = engine.with_solver(Box::new(reify_constraints::SolverRegistry::production()));
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
    let mut engine = configured_eval_engine(reify_eval::Engine::new(
        Box::new(SimpleConstraintChecker),
        None,
    ));
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
        // Geometry-bearing module: route through the kernel-backed build() path so
        // that run_post_processes/post_process_geometry_queries fires and resolves
        // geometry-query value cells (mass, centroid, volume, …).
        // geometry_output is discarded — reify eval is a value inspector only.
        let mut engine =
            configured_eval_engine(reify_eval::Engine::with_registered_kernel(Box::new(
                SimpleConstraintChecker,
            )));
        // Apply --cache-dir flag override (highest precedence over env/defaults set
        // by configured_eval_engine).
        if let Some(ref override_dir) = cache_dir_override {
            engine.set_persistent_cache_dir(Some(override_dir.clone()));
        }
        engine.set_capture_undef_causes(true);
        let result = engine.build(&compiled, reify_ir::ExportFormat::Step);
        (result.values, result.diagnostics, engine)
    } else {
        // Plain numeric module: keep the existing lightweight eval() path so
        // non-geometry eval tests (cli_eval_auto_resolve, cli_stackup_eval,
        // cli_integration_smoke) remain on the exact unchanged code path.
        // Note: register_compute_fns is still required so `@optimized` targets
        // dispatch to their solver kernels (task 3794 / esc-3794-183).
        let mut engine = configured_eval_engine(reify_eval::Engine::new(
            Box::new(SimpleConstraintChecker),
            None,
        ));
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

    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Usage line printed to stderr for any `reify explain` usage error.
const EXPLAIN_USAGE: &str = "Usage: reify explain <file>";

/// Parse a single required file-path positional, rejecting unknown `--`-prefixed flags
/// and extra positionals.  Every error prints `usage` to stderr before returning
/// `Err(ExitCode::FAILURE)` — callers can `return` the `Err` value directly.
///
/// The path is returned as an owned [`String`] so it safely outlives the argument slice.
///
/// Used by `cmd_explain` (and available for other no-flag subcommands).  Commands with
/// their own optional flags (e.g. `cmd_eval` with `--explain-undef`) extract those flags
/// first, then delegate the remainder to this helper or handle the tail themselves.
fn parse_single_file_arg(args: &[String], cmd: &str, usage: &str) -> Result<String, ExitCode> {
    let mut file_path: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `{}`: {}", cmd, flag);
                eprintln!("{}", usage);
                return Err(ExitCode::FAILURE);
            }
            path => {
                if file_path.is_some() {
                    eprintln!("Error: unexpected extra positional argument: {}", path);
                    eprintln!("{}", usage);
                    return Err(ExitCode::FAILURE);
                }
                file_path = Some(path.to_string());
            }
        }
    }
    match file_path {
        Some(path) => Ok(path),
        None => {
            eprintln!("{}", usage);
            Err(ExitCode::FAILURE)
        }
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
    let mut engine = configured_eval_engine(reify_eval::Engine::new(
        Box::new(SimpleConstraintChecker),
        None,
    ));
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

/// Format a single [`reify_ir::UndefCause`] as a terse, human-readable string.
///
/// Called by `cmd_eval` to render the complete cause set for each undef output
/// cell as a comma-joined `because:` clause in the note line (Q5 / PRD §4.4).
///
/// # Variant renderings
///
/// | Variant | Rendered as |
/// |---|---|
/// | `Unbound { param }` | `"<entity>.<member> unbound"` |
/// | `AwaitingSolve { param }` | `"<entity>.<member> awaiting solve"` |
/// | `SolveFailed { detail }` | `"solve failed: <detail>"` |
/// | `OpContractFailed { code, .. }` | `"op contract failed (<code:?>)"` |
/// | `UserUndef { .. }` | `"explicit undef"` |
///
/// The `OpContractFailed` branch is wired for task γ forward-compatibility:
/// γ constructs this variant; δ just formats it so the CLI auto-enriches once
/// γ lands without any re-edit here.
fn format_undef_cause(cause: &UndefCause) -> String {
    match cause {
        UndefCause::Unbound { param, .. } => format!("{param} unbound"),
        UndefCause::AwaitingSolve { param } => format!("{param} awaiting solve"),
        UndefCause::SolveFailed { detail } => format!("solve failed: {detail}"),
        UndefCause::OpContractFailed { code, .. } => format!("op contract failed ({code:?})"),
        UndefCause::UserUndef { .. } => "explicit undef".to_string(),
    }
}

/// Usage line printed to stderr for any `reify doc` usage error.
const DOC_USAGE: &str = "Usage: reify doc <input.ri> [-o <path>] [--format html|markdown|json] [--split] [--compact]\n       reify doc --stdlib --out <dir>";

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

/// Pure exit-decision helper for `reify build`.
///
/// Returns `true` when the build should exit 0 (success), `false` when it
/// should exit non-zero (failure).  Two independent gates cause failure:
///
/// 1. A [`ConstraintOutcome::SomeViolated`] result — one or more constraints
///    were violated.
/// 2. `has_error_diagnostic` — at least one [`reify_core::Severity::Error`]
///    diagnostic was emitted (e.g. "no registered compute trampoline"), even
///    if the constraint outcome is [`ConstraintOutcome::AllSatisfied`] or
///    [`ConstraintOutcome::SomeIndeterminate`].
///
/// This resolves task-4458 concern (c): `cmd_build` previously exited 0 when
/// an `Error`-severity engine diagnostic was emitted alongside a non-violated
/// constraint outcome.  This helper aligns `cmd_build`'s exit code with
/// `cmd_eval`'s `Severity::Error` gate (see `cmd_eval` at the
/// `diagnostics.iter().any(|d| d.severity == Severity::Error)` check).
///
/// Returns `bool` (not [`std::process::ExitCode`]) so the gate is directly
/// unit-testable; callers convert to `ExitCode` at the boundary.
fn build_is_success(outcome: &ConstraintOutcome, has_error_diagnostic: bool) -> bool {
    !has_error_diagnostic && !matches!(outcome, ConstraintOutcome::SomeViolated)
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
///
/// `n` is the already-computed indeterminate count from
/// [`ConstraintOutcome::SomeIndeterminate`]; it is used directly in the header
/// to avoid recomputing the same count independently of [`report_constraint_results`].
fn report_indeterminate_detail(
    n: usize,
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) {
    let _ = writeln!(
        out,
        "Strict check failed: {n} constraint(s) INDETERMINATE \
         \u{2014} inputs undefined (e.g. auto-params unresolved or geometry did not realize):"
    );
    for entry in results
        .iter()
        .filter(|e| e.satisfaction == reify_ir::Satisfaction::Indeterminate)
    {
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
/// # Delegation (task #6170)
///
/// The walk itself lives in
/// [`reify_eval::tolerance_combine::module_declares_representation_within`],
/// which is defined as `!compute_representation_bounds(module).is_empty()` — so
/// this predicate and the C-BOUND bound table are the same traversal rather than
/// two that must be kept in sync. It reaches the canonical recognition gate (UFC
/// name + arity + arg0 ValueRef:StructureRef + arg1 Literal Scalar LENGTH
/// finite≥0) that the engine's dispatch interception also uses.
///
/// This CLI-local name is retained (rather than inlining the call) so
/// `cmd_check`'s routing reads unchanged.
///
/// # Not the export-refusal gate
///
/// Both export-refusal sites — `cmd_build`'s `-o` arm above, and
/// `Engine::build_outputs_with_result` in the engine — need the bound TABLE
/// rather than a boolean, because the refusal names the unenforced bound. They
/// therefore call
/// [`reify_eval::tolerance_combine::unenforced_representation_bound_diagnostic`]
/// instead of this predicate; its `None` case is exactly this function returning
/// `false`, so all three sites still share one traversal.
///
/// Non-assertion modules: this function returns `false` and `cmd_check` keeps
/// the existing path verbatim (C2 — byte-identical behavior for all existing
/// `reify check` inputs).
fn module_has_representation_within(module: &reify_compiler::CompiledModule) -> bool {
    reify_eval::tolerance_combine::module_declares_representation_within(module)
}

/// Returns `true` when `module` contains at least one template whose
/// [`reify_compiler::TopologyTemplate::trait_bounds`] includes `"DFMRule"`.
///
/// This gate keys on the `: DFMRule` trait declaration as a deliberate static
/// *proxy* for the engine's duck-typed DFM rule recognition.  The engine's
/// `measure_dfm_rules` (engine_constraints.rs) discovers DFM rules entirely by
/// duck-typing on field shape via `dfm_rule_spec` — it requires a
/// `severity : DFMSeverity` enum field, an `applies_to` struct instance carrying
/// an overhang/draft angle scalar, and a `subject` field — and does **not**
/// reference `trait_bounds` or `satisfies_trait_bound`.  The two predicates are
/// intentionally different: this gate is a static pre-eval check on declarations
/// (no evaluated values are available at routing time), whereas the engine's
/// recognition fires at measurement time on evaluated field shapes.
///
/// **Accepted limitation:** a structure that duck-types as a DFM rule (matching
/// `dfm_rule_spec`) but omits the `: DFMRule` declaration would make this gate
/// return `false`, silently keeping the lightweight `Engine::new(None)`+`check()`
/// path, never calling `build()`, and therefore leaving `realization_handles`
/// unpopulated — `measure_dfm_rules` would emit nothing.  By convention all DFM
/// rule structures in the stdlib and user code declare `: DFMRule` explicitly, so
/// this case is out of scope for the routing gate.
///
/// `DFMRule` is a terminal stdlib trait (process.ri: `trait DFMRule {}`; no
/// refinements, no subtraits), so a direct name-equality match (`b == "DFMRule"`)
/// is exact.  A `: DFMRule` conformer is always compiled to a top-level
/// `module.templates` entry regardless of instantiation site, so scanning
/// templates catches both of `measure_dfm_rules`' discovery sources (A:
/// top-level templates; B: sub-component instances).
///
/// When `true`, `cmd_check` routes through the kernel-backed
/// `build(ExportFormat::Step)`-before-`check` path so that
/// `realization_handles` is populated with live B-rep handles — `measure_dfm_rules`
/// reads `self.realization_handles` to set each rule's `subject_handle`, and
/// skips any rule where `subject_handle.is_none()` (the handle is only present
/// after `build()`).  The C1 no-kernel no-op (the `default_kernel_name` None
/// guard in `measure_dfm_rules` fires when OCCT is absent, emitting nothing)
/// and C2 byte-identical behavior for non-DFM modules are both preserved.
fn module_has_dfm_rule(module: &reify_compiler::CompiledModule) -> bool {
    module
        .templates
        .iter()
        .any(|t| t.trait_bounds.iter().any(|b| b == "DFMRule"))
}

/// Returns `true` when `module` has both a `DFMRule` conformer AND at least
/// one template that declares a `min_feature_size : Length` value cell.
///
/// This is a STATIC pre-eval proxy for the engine's runtime `dfm_thickness_spec`
/// (engine_constraints.rs, which requires `applies_to.min_feature_size` to be
/// of type `Length` at eval time).  `min_feature_size : Length` is declared on
/// the `Subtracting`, `Adding`, and `Parting` process traits (process.ri:55,
/// 69, 98); a conformer's template carries it as a `ValueCellDecl` with
/// `cell_type == Type::Scalar { dimension: DimensionVector::LENGTH }` (ty.rs:81).
///
/// `Forming` (draft-only: `draft_angle : Angle`, no `min_feature_size`) returns
/// `false` → `ensure_openvdb_kernel` is NOT called → alloc-cost optimization
/// preserved (the voxelization path is never needed for draft/overhang checks).
///
/// Accepted limitation: over-acquires OpenVDB in a contrived module that
/// combines a `min_feature_size` process conformer with a `DFMRule` applying
/// to a DIFFERENT process that is Forming-only.  Cost-only (wasted allocation),
/// never a wrong diagnostic; never under-acquires for realistic single-process
/// fixtures.  Mirrors the accepted-limitation note on `module_has_dfm_rule`.
///
/// `cmd_check` calls this BEFORE `engine.ensure_openvdb_kernel()` so that
/// non-thickness modules (Forming/overhang/draft) and non-DFM modules keep
/// the single-pick OCCT engine (engine_admin.rs:754-760 alloc-cost contract).
fn module_has_thickness_dfm_rule(module: &reify_compiler::CompiledModule) -> bool {
    module_has_dfm_rule(module)
        && module.templates.iter().any(|t| {
            t.value_cells.iter().any(|vc| {
                vc.id.member == "min_feature_size"
                    && matches!(
                        &vc.cell_type,
                        reify_core::Type::Scalar { dimension }
                            if *dimension == reify_core::DimensionVector::LENGTH
                    )
            })
        })
}

/// Returns `true` when `diagnostics` contains at least one DFM Error-severity
/// violation (e.g. `E_DFM_OVERHANG`, `E_DFM_UNDERCUT`, `E_DFM_DRAFT`).
///
/// All DFM Error diagnostics embed their code prefix `E_DFM_` at the start of
/// the [`reify_core::Diagnostic::message`] field (the format is
/// `"E_DFM_<KIND>: <human description>"`).  Matching on the message substring
/// is more precise than `d.code.is_none()`: it avoids escalating unrelated
/// code-less Error diagnostics (e.g. FEA "no registered compute trampoline",
/// build-volume usage errors) that may co-reside with a DFMRule in the same
/// module.
///
/// Note: `E_DFM_UNDERCUT` is always [`Severity::Error`] regardless of the
/// rule's declared `DFMSeverity` (a re-entrant wall is a hard manufacturability
/// failure per PRD §2.3), so this predicate correctly captures it alongside
/// `E_DFM_OVERHANG` / `E_DFM_DRAFT` from `DFMSeverity::Error` rules.
fn dfm_has_error_diagnostic(diagnostics: &[reify_core::Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains("E_DFM_"))
}

/// Structural-equality merge of a discarded [`reify_eval::BuildResult`]'s
/// diagnostics into the authoritative [`reify_eval::CheckResult`]'s.
///
/// # Why this exists (task 5748, PRD `check-diagnostic-truthfulness.md` D2)
///
/// `cmd_check`'s kernel-backed arm calls `build()` for its handle-population
/// side effect and then, separately, `check()` as the source of record.  The
/// `BuildResult` used to be dropped on the floor, which silently swallowed
/// every realization-only diagnostic — `compile_geometry_op` gating errors and
/// kernel-dispatch failures that `check()` alone never produces.  A module
/// whose geometry cannot compile at all therefore reported "All constraints
/// satisfied." under `check` while `eval`/`build` reported a hard error on the
/// same file.
///
/// Neither naive alternative works.  Using build()'s diagnostics *instead* is
/// wrong: `build_with_geometry_output` calls `self.check(module)` internally as
/// its first step, so its copy of the check-style entries is stale (computed
/// before `realization_handles` / `achieved_repr_tol` are populated).  Plain
/// concatenation is also wrong: both passes re-run `eval()` over the same
/// deterministic input, so every genuine eval-level diagnostic would print
/// twice.  Hence: keep check()'s list authoritative and verbatim, then append
/// only what is not already present by `(severity, code, message)`.
///
/// # Invariants upheld (PRD D2, verbatim)
///
/// * No diagnostic that would appear in `check()`'s own diagnostics today is
///   ever printed twice.
/// * Every diagnostic that `build()` alone would have produced (i.e. every
///   `compile_geometry_op` / kernel-dispatch diagnostic) appears at least once
///   in the reported set.
/// * `constraint_results` come from the authoritative `check()` call only
///   (never `build()`'s stale copy) — RepresentationWithin/GD&T/DFM
///   correctness is unaffected.
///
/// That third invariant is the caller's contract, not this function's: this
/// helper takes and returns diagnostics ONLY and never sees a
/// `constraint_results`.  (The one narrow, adjacent exception the esc-5748-1
/// ruling carved out lives in [`merge_post_build_verdicts`], which upgrades
/// `Indeterminate` verdicts and can never substitute or regress a definite one.)
///
/// # Dedup key
///
/// A `HashSet<(Severity, Option<DiagnosticCode>, String)>` rather than a
/// derived `PartialEq`: [`reify_core::Diagnostic`] is `#[non_exhaustive]` and
/// derives only `Debug, Clone` (diagnostics.rs:3834), so deriving equality
/// would be a reify-core API change outside this leaf's scope AND would drag
/// `labels`/`candidates` into identity, which the PRD deliberately excludes.
/// The three compared fields are each `Eq + Hash` (`Severity` :94,
/// `DiagnosticCode` :152, `String`), so the tuple key is buildable here.
///
/// Membership is tested against the ACCUMULATING set — seeded from
/// `check_diags`, then grown as build entries are appended — so duplicates
/// *internal to* `build_diags` also collapse to one occurrence.  This is not
/// hypothetical: on the `mirror(...)` bare-origin shape (fixture
/// `tests/fixtures/mirror_bare_origin.ri`) `build()` emits
/// `failed to compile geometry operation: missing or non-Length argument 'ox'
/// for mirror` TWICE for a single call site.  The PRD requires "at least
/// once", and collapsing matches `check`'s existing output discipline.
fn merge_build_diagnostics(
    check_diags: &[reify_core::Diagnostic],
    build_diags: &[reify_core::Diagnostic],
) -> Vec<reify_core::Diagnostic> {
    type DiagKey = (Severity, Option<reify_core::DiagnosticCode>, String);
    let key = |d: &reify_core::Diagnostic| -> DiagKey {
        (d.severity, d.code, d.message.clone())
    };

    let mut merged = check_diags.to_vec();
    let mut seen: std::collections::HashSet<DiagKey> = check_diags.iter().map(key).collect();
    for diag in build_diags {
        if seen.insert(key(diag)) {
            merged.push(diag.clone());
        }
    }
    merged
}

/// Returns `true` when `module` contains at least one realization operation
/// that is `CompiledGeometryOp::Isosurface` (the `isosurface(...)` builtin,
/// which lowers to the runtime-IR `Operation::Surface` — engine_build.rs:1961).
///
/// This is a STATIC pre-eval proxy mirroring [`module_has_thickness_dfm_rule`]'s
/// routing-gate shape: [`cmd_build`] calls this BEFORE
/// `engine.ensure_openvdb_kernel()` so that non-isosurface modules keep the
/// single-pick OCCT engine (alloc-cost contract, engine_admin.rs) and stay
/// byte-identical (C2). Isosurface modules need OpenVDB registered because
/// the operand's Mesh→Voxel voxelize stage and the terminal Voxel→Mesh
/// marching-cubes stage (task γ/5001) both dispatch to the openvdb kernel.
///
/// Note: `CompiledGeometryOp::Surface { kind: SurfaceKind, .. }` (free-form
/// `nurbs_surface` construction) is a DISTINCT variant from `Isosurface` —
/// this predicate matches only the latter.
fn module_has_isosurface(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        t.realizations.iter().any(|r| {
            r.operations
                .iter()
                .any(|op| matches!(op, reify_compiler::CompiledGeometryOp::Isosurface { .. }))
        })
    })
}

/// Parses the exported triangle count directly from a binary STL byte
/// buffer's fixed-width header field: 80-byte header + `u32` triangle count
/// (bytes 80..84, little-endian) + 50 bytes/triangle. `write_stl_binary`
/// (reify-ir/src/geometry.rs) is the sole `Stl` writer in every kernel
/// (occt/manifold) — including multi-body compound exports — so this header
/// field is authoritative for any bytes produced by
/// `engine.build(_, ExportFormat::Stl)`. Returns `0` if `data` is shorter
/// than 84 bytes (defensive; a real binary-STL export always has the full
/// header). See [`threemf_triangle_count`] for the `ExportFormat::ThreeMF`
/// counterpart and `triangle_count_tests` below for coverage.
fn stl_triangle_count(data: &[u8]) -> usize {
    match data.get(80..84) {
        Some(count_bytes) => {
            u32::from_le_bytes(count_bytes.try_into().expect("slice is exactly 4 bytes")) as usize
        }
        None => 0,
    }
}

/// Parses the exported triangle count directly from `ThreeMF` bytes by
/// counting `<triangle ` element occurrences in the raw archive. `write_3mf`
/// (reify-ir/src/geometry.rs) pins every ZIP part to
/// `CompressionMethod::Stored`, specifically so the `3D/3dmodel.model` XML
/// appears literally in `data` — its doc comment sanctions substring-counting
/// `<triangle ` on raw bytes "without a zip reader", and
/// `write_3mf_box_produces_valid_3mf_package` pins the identical
/// `.matches("<triangle ").count()` technique against the unzipped XML.
///
/// Reading straight from `data` (rather than a fresh `tessellate_realizations`
/// walk) keeps this authoritative for whatever was actually written: the OCCT
/// kernel's `ThreeMF` export re-tessellates at its own hardcoded
/// `DEFAULT_STL_TESSELLATION_TOLERANCE` (0.1), not
/// `Engine::DEFAULT_TESSELLATION_TOLERANCE` (0.0001), so a tessellation
/// re-walk is not guaranteed to agree with what was actually exported (amend:
/// reviewer_comprehensive correctness_consistency finding). See
/// `triangle_count_tests` below for coverage.
///
/// This is a package-wide total, not a single-mesh count: if `data` ever
/// contained more than one `<mesh>` part, this would sum `<triangle ` across
/// all of them (and across any other archive section where the substring
/// happened to appear). Every export this CLI drives today writes exactly one
/// mesh part, so `Triangles: N` is the exported mesh's count in practice; this
/// is an acceptable semantic for a diagnostic print, not a correctness bug.
fn threemf_triangle_count(data: &[u8]) -> usize {
    const NEEDLE: &[u8] = b"<triangle ";
    data.windows(NEEDLE.len()).filter(|w| *w == NEEDLE).count()
}

/// Returns `true` when `module` carries a *geometric* `Conforms` instance — one
/// whose compiled [`reify_compiler::CompiledConstraint::arg_bindings`] include an
/// explicit `actual` binding (η/4480).
///
/// This is the CLI counterpart of the engine's own `has_geometric_conforms`
/// fast-path inside `Engine::measure_gdt_conformance`: both key on the presence
/// of an `"actual"` arg-binding on a template (or guarded-group) constraint.
/// `Conforms`'s predicate body never references `actual`, so the binding captured
/// at instantiation is the only static trace of geometric intent — a *scalar*
/// `Conforms` (whose `actual` fell to its `nominal()` default) is NOT detected,
/// so `cmd_check` keeps its scalar verdict byte-identical (B4). The two gates are
/// deliberately the same predicate so the routing decision cannot drift from the
/// pass's own no-op check.
///
/// When `true`, `cmd_check` routes through the kernel-backed
/// `build(ExportFormat::Step)`-before-`check` path so that `realization_handles`
/// is populated with live B-rep handles for the pass — a `MaxDeviation` query is
/// `BRepOnly`, and only `build()` (not `tessellate_realizations`) populates that
/// map. When `false`, the existing RepresentationWithin and lightweight paths are
/// kept verbatim (C2).
fn module_has_geometric_conforms(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        let top = t.constraints.iter();
        let guarded = t
            .guarded_groups
            .iter()
            .flat_map(|g| g.constraints.iter().chain(g.else_constraints.iter()));
        top.chain(guarded)
            .any(|c| c.arg_bindings.iter().any(|(n, _)| n == "actual"))
    })
}

/// Write the terminal summary for `reify check` and return the appropriate
/// [`ExitCode`].
///
/// Replaces the two byte-identical terminal `match outcome` blocks in
/// `cmd_check` (no-purpose path and `--purpose` path) with a single
/// implementation so the strict upgrade logic lives in one place.
///
/// * [`ConstraintOutcome::AllSatisfied`] → `"All constraints satisfied."` + SUCCESS (to `out`)
/// * [`ConstraintOutcome::SomeViolated`] → `"Some constraints violated."` + FAILURE (to `out`)
/// * [`ConstraintOutcome::SomeIndeterminate(n)`]:
///   * `strict=false` → legacy `"No constraints violated ({n} indeterminate)."` + SUCCESS (to `out`)
///   * `strict=true`  → [`report_indeterminate_detail`] output + FAILURE (to `err`)
///
/// Success-path summaries go to `out` (stdout). The strict-failure narrative goes
/// to `err` (stderr) — conventional for error diagnostics and avoids polluting the
/// stdout stream on the failure path. The exit code remains the machine-parseable
/// contract in both cases.
///
/// **Intentional duplication in strict mode:** when `strict=true` and constraints are
/// INDETERMINATE, callers first invoke [`report_eval_output`] (which writes
/// `INDETERMINATE <label>` status lines to stdout via [`report_constraint_results`])
/// and then call this function, which emits [`report_indeterminate_detail`] to `err`.
/// A machine consumer scanning both streams will therefore see each indeterminate
/// constraint listed in two different formats. This is conventional (per-item status
/// lines on stdout + a failure summary on stderr) and intentional — not a bug.
///
/// Without `--strict` every existing literal string and exit code is preserved
/// byte-for-byte (C2 — backward-compatible behavior).
fn finish_check(
    outcome: &ConstraintOutcome,
    results: &[reify_eval::ConstraintCheckEntry],
    strict: bool,
    out: &mut impl std::io::Write,
    err: &mut impl std::io::Write,
) -> std::process::ExitCode {
    match outcome {
        ConstraintOutcome::AllSatisfied => {
            let _ = writeln!(out, "All constraints satisfied.");
        }
        ConstraintOutcome::SomeViolated => {
            let _ = writeln!(out, "Some constraints violated.");
        }
        ConstraintOutcome::SomeIndeterminate(n) => {
            if strict {
                report_indeterminate_detail(*n, results, err);
            } else {
                let _ = writeln!(out, "No constraints violated ({n} indeterminate).");
            }
        }
    }
    if check_fails(outcome, strict) {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
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
    use reify_core::ConstraintNodeId;
    use reify_eval::ConstraintCheckEntry;
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
        report_indeterminate_detail(2, &entries, &mut buf);
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
        let entries = vec![make_entry(
            "Part",
            0,
            Some("load"),
            Satisfaction::Indeterminate,
        )];
        let mut buf = Vec::new();
        report_indeterminate_detail(1, &entries, &mut buf);
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
        let entries = vec![make_entry(
            "Bracket",
            1,
            Some("tolerance"),
            Satisfaction::Indeterminate,
        )];
        let outcome = ConstraintOutcome::SomeIndeterminate(1);
        let mut buf = Vec::new();
        let mut err_buf = Vec::new();
        finish_check(&outcome, &entries, false, &mut buf, &mut err_buf);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(
            output, "No constraints violated (1 indeterminate).\n",
            "non-strict SomeIndeterminate(1) must produce the exact legacy summary line"
        );
    }

    #[test]
    fn finish_check_strict_indeterminate_emits_detail_not_legacy_line() {
        // (b) strict + SomeIndeterminate → the strict-failure block goes to `err`
        // (stderr); `out` (stdout) must remain empty. The failure narrative must
        // contain "Strict check failed" and name the indeterminate constraint; the
        // legacy summary "No constraints violated" must NOT appear in either stream.
        let entries = vec![make_entry(
            "Bracket",
            1,
            Some("tolerance"),
            Satisfaction::Indeterminate,
        )];
        let outcome = ConstraintOutcome::SomeIndeterminate(1);
        let mut buf = Vec::new();
        let mut err_buf = Vec::new();
        finish_check(&outcome, &entries, true, &mut buf, &mut err_buf);
        let out_str = String::from_utf8(buf).unwrap();
        let err_str = String::from_utf8(err_buf).unwrap();
        assert!(
            err_str.contains("Strict check failed"),
            "strict SomeIndeterminate: 'Strict check failed' must appear on stderr, got err: {err_str}"
        );
        assert!(
            err_str.contains("tolerance"),
            "strict SomeIndeterminate: constraint name 'tolerance' must appear on stderr, got err: {err_str}"
        );
        assert!(
            !out_str.contains("No constraints violated"),
            "strict SomeIndeterminate: 'No constraints violated' must NOT appear on stdout, got: {out_str}"
        );
        assert!(
            !out_str.contains("Strict check failed"),
            "strict SomeIndeterminate: 'Strict check failed' must NOT appear on stdout, got: {out_str}"
        );
    }

    #[test]
    fn finish_check_all_satisfied_either_strict() {
        // (c) AllSatisfied (either strict value) → "All constraints satisfied.\n".
        let entries: Vec<reify_eval::ConstraintCheckEntry> = vec![];
        let outcome = ConstraintOutcome::AllSatisfied;
        for strict in [false, true] {
            let mut buf = Vec::new();
            let mut err_buf = Vec::new();
            finish_check(&outcome, &entries, strict, &mut buf, &mut err_buf);
            let output = String::from_utf8(buf).unwrap();
            assert_eq!(
                output, "All constraints satisfied.\n",
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
            let mut err_buf = Vec::new();
            finish_check(&outcome, &entries, strict, &mut buf, &mut err_buf);
            let output = String::from_utf8(buf).unwrap();
            assert_eq!(
                output, "Some constraints violated.\n",
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
        let compiled_geo = reify_test_support::parse_and_compile_with_stdlib(geometry_source);
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
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
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
        let geo_cell_only =
            reify_test_support::CompiledModuleBuilder::new(reify_core::ModulePath::new(vec![
                "test".to_string(),
            ]))
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

#[cfg(test)]
mod geometric_conforms_gate_tests {
    use super::module_has_geometric_conforms;

    /// Non-OCCT routing gate test: `module_has_geometric_conforms` must detect a
    /// *geometric* `Conforms` instance (one carrying an explicit `actual`
    /// binding) in real compiled IR, and must return `false` for a *scalar*
    /// `Conforms` (no `actual`) and for a plain module with no `Conforms` at all.
    ///
    /// This is the CLI counterpart of the engine's own `has_geometric_conforms`
    /// fast-path gate (`Engine::measure_gdt_conformance`): both key on the
    /// presence of an `"actual"` arg-binding on a template (or guarded-group)
    /// constraint. `Conforms`'s predicate body never references `actual`, so the
    /// arg-binding — captured at instantiation on `CompiledConstraint` — is the
    /// only static trace of geometric intent.
    ///
    /// Always-running (no OCCT guard) so a regression in template-level
    /// recognition fails CI independently of OCCT availability: in stub mode
    /// `cmd_check` exits 0 regardless of which path it took, so the OCCT-gated
    /// CLI test alone could not catch broken routing.
    ///
    /// Uses `parse_and_compile_with_stdlib` because `Conforms`, `Flatness`, and
    /// `Geometry` are stdlib-prelude entities, mirroring the engine GD&T
    /// conformance fixtures.
    #[test]
    fn module_has_geometric_conforms_detects_explicit_actual_vs_scalar_and_plain() {
        // Geometric module: a `Conforms` instance with an EXPLICIT `actual`
        // binding — must be detected (returns `true`) so that `cmd_check` routes
        // through the kernel-backed build-before-check path that populates live
        // B-rep handles for the η `measure_gdt_conformance` pass.
        let geometric_source = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;
        let compiled_geometric = reify_test_support::parse_and_compile_with_stdlib(geometric_source);
        assert!(
            module_has_geometric_conforms(&compiled_geometric),
            "module with a Conforms instance binding an explicit `actual` should be \
             detected (routing gate must return true)"
        );

        // Scalar module: a `Conforms` instance with NO `actual` (falls to its
        // `nominal()` default) — must NOT be detected (returns `false`) so that
        // `cmd_check` keeps the lightweight path and the scalar verdict stays
        // byte-identical (B4).
        let scalar_source = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm)
}
"#;
        let compiled_scalar = reify_test_support::parse_and_compile_with_stdlib(scalar_source);
        assert!(
            !module_has_geometric_conforms(&compiled_scalar),
            "module with a scalar-only Conforms (no explicit `actual`) must NOT be \
             detected (routing gate must return false — B4 scalar path preserved)"
        );

        // Plain module: no `Conforms` constraints anywhere — must NOT be detected.
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_geometric_conforms(&compiled_plain),
            "module without any Conforms constraints must NOT be detected \
             (routing gate must return false)"
        );
    }
}

#[cfg(test)]
mod dfm_rule_gate_tests {
    use super::module_has_dfm_rule;

    /// Non-OCCT routing gate test: `module_has_dfm_rule` must detect a
    /// `DFMRule` conformer template in real compiled IR, and must return
    /// `false` for a plain module without one.
    ///
    /// Always-running (no OCCT guard) so a regression in template-level
    /// recognition fails CI independently of OCCT availability: in stub mode
    /// `cmd_check` exits 0 regardless of which path it took, so the
    /// OCCT-gated CLI test alone could not catch broken routing.
    ///
    /// Uses `parse_and_compile_with_stdlib` because `DFMRule`, `DFMSeverity`,
    /// `Adding`, and `Process` are stdlib-prelude entities (std.process).
    #[test]
    fn module_has_dfm_rule_detects_conformer_vs_plain() {
        // DFM module: a `structure def OverhangRule : DFMRule {...}` conformer
        // — must be detected (returns `true`) so that `cmd_check` routes
        // through the kernel-backed build(Step)-before-check path that populates
        // `realization_handles` for `measure_dfm_rules`.
        let dfm_source = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def OverhangRule : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;
        let compiled_dfm = reify_test_support::parse_and_compile_with_stdlib(dfm_source);
        assert!(
            module_has_dfm_rule(&compiled_dfm),
            "module with a DFMRule conformer (OverhangRule : DFMRule) should be \
             detected (routing gate must return true)"
        );

        // Plain module: no DFMRule conformer anywhere — must NOT be detected
        // (returns `false`) so that `cmd_check` keeps the existing lightweight
        // `Engine::new(None)+check()` path (C2).
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_dfm_rule(&compiled_plain),
            "module without any DFMRule conformer must NOT be detected \
             (routing gate must return false — C2 path preserved)"
        );
    }
}

#[cfg(test)]
mod thickness_dfm_gate_tests {
    use super::module_has_thickness_dfm_rule;

    /// Cfg-independent routing gate test: `module_has_thickness_dfm_rule` must
    /// return `true` for a module with a Subtracting (or Adding) conformer
    /// carrying `min_feature_size : Length` plus a `DFMRule` conformer, and
    /// `false` for a Forming-based draft-only DFMRule module (no `min_feature_size`)
    /// and for a plain non-DFM module.
    ///
    /// This is the static-gate half of the "doesn't allocate by default"
    /// regression pin: the gate prevents `ensure_openvdb_kernel` from being
    /// called on Forming (draft-only) or plain modules, preserving the
    /// single-pick OCCT engine's alloc-cost contract.
    ///
    /// Always-running (no has_openvdb / OCCT guard): the gate inspects only
    /// compiled IR — it does not perform geometry operations.
    #[test]
    fn module_has_thickness_dfm_rule_detects_thickness_vs_draft_only_vs_plain() {
        // (a) TRUE — Subtracting conformer carrying `min_feature_size : Length`
        // plus a `DFMRule` conformer: the gate must return `true` so that
        // `cmd_check` calls `ensure_openvdb_kernel()`.
        let subtracting_dfm_source = r#"
import std.process

structure def Milling : Subtracting {
    param duration          : Time   = 30min
    param cost              : Money  = 10USD
    param tool_access       : Solid  = box(200mm, 200mm, 200mm)
    param min_feature_size  : Length = 2mm
    param achievable_finish : Length = 0.01mm
}

structure def ThicknessRule : DFMRule {
    param rule_name  : String      = "thickness-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Milling()
    param subject    : Solid       = box(10mm, 10mm, 1mm)
}
"#;
        let compiled_subtracting =
            reify_test_support::parse_and_compile_with_stdlib(subtracting_dfm_source);
        assert!(
            module_has_thickness_dfm_rule(&compiled_subtracting),
            "Subtracting+DFMRule module with min_feature_size : Length must return \
             true (thickness-DFM gate — OpenVDB acquisition required)"
        );

        // (b) FALSE — Forming conformer (draft-only: has `draft_angle : Angle`
        // but NO `min_feature_size : Length`) plus a `DFMRule` conformer: the
        // gate must return `false` so that `ensure_openvdb_kernel` is NOT called
        // (alloc-cost optimization preserved; overhang/draft arm never needs
        // the voxelization path).
        let forming_dfm_source = r#"
import std.process

structure def Stamping : Forming {
    param duration       : Time   = 10min
    param cost           : Money  = 3USD
    param min_bend_radius : Length = 2mm
    param max_draw_depth : Length = 50mm
    param draft_angle    : Angle  = 3deg
}

structure def DraftRule : DFMRule {
    param rule_name  : String      = "draft-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Stamping()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;
        let compiled_forming =
            reify_test_support::parse_and_compile_with_stdlib(forming_dfm_source);
        assert!(
            !module_has_thickness_dfm_rule(&compiled_forming),
            "Forming+DFMRule module (draft-only, no min_feature_size) must return \
             false (alloc-cost optimization: OpenVDB must NOT be acquired for draft/overhang)"
        );

        // (c) FALSE — plain module with no DFMRule at all: must return false
        // so the existing `Engine::new(None)+check()` lightweight path is
        // preserved (C2).
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_thickness_dfm_rule(&compiled_plain),
            "plain module (no DFMRule, no min_feature_size) must return false (C2 path)"
        );

        // (d) FALSE — Subtracting conformer WITH `min_feature_size : Length`
        // but NO `DFMRule` conformer anywhere: the gate must return `false`
        // because the `module_has_dfm_rule` conjunct is load-bearing.
        //
        // This is the configuration most likely to regress if the conjunction
        // order in `module_has_thickness_dfm_rule` is refactored (e.g. the
        // min_feature_size check is mistakenly left as the only conjunct,
        // dropping the `module_has_dfm_rule` AND). Without this pin, a
        // Subtracting process module with no DFMRule would incorrectly trigger
        // `ensure_openvdb_kernel`, breaking the single-pick OCCT alloc-cost
        // contract for non-DFM files.
        let subtracting_no_dfm_source = r#"
import std.process

structure def Milling : Subtracting {
    param duration          : Time   = 30min
    param cost              : Money  = 10USD
    param tool_access       : Solid  = box(200mm, 200mm, 200mm)
    param min_feature_size  : Length = 2mm
    param achievable_finish : Length = 0.01mm
}
"#;
        let compiled_subtracting_no_dfm =
            reify_test_support::parse_and_compile_with_stdlib(subtracting_no_dfm_source);
        assert!(
            !module_has_thickness_dfm_rule(&compiled_subtracting_no_dfm),
            "Subtracting conformer with min_feature_size but NO DFMRule must return \
             false — the DFMRule conjunct in module_has_thickness_dfm_rule is \
             load-bearing and cannot be dropped by a refactor"
        );
    }
}

#[cfg(test)]
mod isosurface_gate_tests {
    use super::module_has_isosurface;

    /// Cfg-independent routing gate test (mirrors
    /// `module_has_thickness_dfm_rule_detects_thickness_vs_draft_only_vs_plain`):
    /// `module_has_isosurface` must return `true` for a module with an
    /// `isosurface(...)` realization, and `false` for a module using the
    /// distinct `nurbs_surface(...)` builtin (a different
    /// `CompiledGeometryOp::Surface` variant — free-form NURBS construction,
    /// not marching-cubes surfacing) and for a plain module with neither.
    ///
    /// Always-running (no has_openvdb / OCCT guard): the gate inspects only
    /// compiled IR — it does not perform geometry operations.
    #[test]
    fn module_has_isosurface_detects_isosurface_vs_nurbs_surface_vs_plain() {
        // (a) TRUE — `isosurface(solid)` realization: the gate must return
        // `true` so that `cmd_build` calls `ensure_openvdb_kernel()`.
        let isosurface_source = r#"
structure IsoWire {
    param size: Length = 20mm
    let solid = box(size, size, size)
    let shell = isosurface(solid)
}
"#;
        let compiled_isosurface =
            reify_test_support::parse_and_compile_with_stdlib(isosurface_source);
        assert!(
            module_has_isosurface(&compiled_isosurface),
            "module with an isosurface(...) realization must be detected \
             (routing gate must return true)"
        );

        // (b) FALSE — `nurbs_surface(...)` lowers to `CompiledGeometryOp::Surface
        // { kind: SurfaceKind::Nurbs, .. }`, a DISTINCT variant from
        // `Isosurface`: the gate must NOT match it, so a NURBS-only module
        // keeps the single-pick OCCT engine (OpenVDB is never acquired).
        let nurbs_surface_source = r#"
structure def NurbsOnly {
    let p = nurbs_surface(
        [[point3(0mm,0mm,0mm),point3(0mm,10mm,0mm)],[point3(10mm,0mm,0mm),point3(10mm,10mm,5mm)]],
        [[1.0,1.0],[1.0,1.0]],
        [0,0,1,1],
        [0,0,1,1],
        1,
        1
    )
}
"#;
        let compiled_nurbs_surface =
            reify_test_support::parse_and_compile_with_stdlib(nurbs_surface_source);
        assert!(
            !module_has_isosurface(&compiled_nurbs_surface),
            "module with only a nurbs_surface(...) (CompiledGeometryOp::Surface, \
             distinct from Isosurface) must NOT be detected by this gate"
        );

        // (c) FALSE — plain module with no Surface-family op at all.
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_isosurface(&compiled_plain),
            "plain module (no isosurface, no nurbs_surface) must return false \
             (C2 path preserved — OpenVDB never acquired for non-surfacing builds)"
        );
    }
}

#[cfg(test)]
mod build_is_success_tests {
    use super::{build_is_success, ConstraintOutcome};

    /// (AllSatisfied, no error diagnostic) → success.
    #[test]
    fn all_satisfied_no_error_is_success() {
        assert!(build_is_success(&ConstraintOutcome::AllSatisfied, false));
    }

    /// (AllSatisfied, has error diagnostic) → failure.
    /// Mirrors cmd_eval's Severity::Error gate (task 4458 fix (c)).
    #[test]
    fn all_satisfied_with_error_is_failure() {
        assert!(!build_is_success(&ConstraintOutcome::AllSatisfied, true));
    }

    /// (SomeIndeterminate, no error diagnostic) → success.
    /// Indeterminate constraints do not gate build; geometry is still written.
    #[test]
    fn some_indeterminate_no_error_is_success() {
        assert!(build_is_success(&ConstraintOutcome::SomeIndeterminate(2), false));
    }

    /// (SomeIndeterminate, has error diagnostic) → failure.
    #[test]
    fn some_indeterminate_with_error_is_failure() {
        assert!(!build_is_success(&ConstraintOutcome::SomeIndeterminate(2), true));
    }

    /// (SomeViolated, no error diagnostic) → failure.
    #[test]
    fn some_violated_no_error_is_failure() {
        assert!(!build_is_success(&ConstraintOutcome::SomeViolated, false));
    }

    /// (SomeViolated, has error diagnostic) → failure.
    #[test]
    fn some_violated_with_error_is_failure() {
        assert!(!build_is_success(&ConstraintOutcome::SomeViolated, true));
    }
}

// ── Triangles: N helper unit tests (task δ/5002 amend: reviewer_comprehensive
// test-coverage + consistency findings) ─────────────────────────────────────
//
// `stl_triangle_count` and `threemf_triangle_count` are exercised directly,
// including a same-mesh cross-writer parity check driven through the REAL
// `reify_ir::write_stl_binary` / `write_3mf` producers — not two hand-built
// byte buffers each independently pre-loaded with the same literal count,
// which would only prove each reader parses back what it was handed. A full
// dual-format CLI/engine integration test still belongs in
// `cli_build_voxel_to_mesh.rs` / `voxel_to_mesh_e2e.rs`, both outside this
// amendment's locked scope (`main.rs` and the `.ri` fixture only).
#[cfg(test)]
mod triangle_count_tests {
    use super::{stl_triangle_count, threemf_triangle_count};
    use reify_ir::{Mesh, ThreeMfOptions, write_3mf, write_stl_binary};

    /// A well-formed binary-STL byte buffer with `count` baked into bytes
    /// 80..84 (little-endian `u32`), per the layout `stl_triangle_count`
    /// parses. The per-triangle payload is irrelevant to the function under
    /// test, so it is omitted entirely — only the 84-byte header is built.
    fn stl_bytes_with_count(count: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 84];
        bytes[80..84].copy_from_slice(&count.to_le_bytes());
        bytes
    }

    /// A minimal valid `Mesh` with `triangle_count` triangles, all reusing
    /// the same 3 vertices. Neither `write_stl_binary` nor `write_3mf`
    /// validate geometry (manifoldness, winding, area) — only buffer-length
    /// and index-bounds — so a degenerate repeated triangle is sufficient to
    /// drive both real writers.
    ///
    /// Each triangle's index triple is a distinct cyclic rotation of `0,1,2`
    /// (`0,1,2` / `1,2,0` / `2,0,1`, repeating), rather than every triangle
    /// sharing the literal triple `[0,1,2]`. The three vertices themselves
    /// still repeat (still degenerate/zero-area — irrelevant to the byte-count
    /// this test drives), but distinct index triples mean no two triangles are
    /// byte-identical records, so a hypothetical future dedup pass in either
    /// writer would visibly change the reported count instead of silently
    /// collapsing both writers in lockstep — keeping the parity assertion
    /// robust to writer-internals changes.
    fn repeated_triangle_mesh(triangle_count: usize) -> Mesh {
        let mut indices = Vec::with_capacity(triangle_count * 3);
        for i in 0..triangle_count {
            let r = i % 3;
            indices.push(r as u32);
            indices.push(((r + 1) % 3) as u32);
            indices.push(((r + 2) % 3) as u32);
        }
        Mesh { vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], indices, normals: None }
    }

    /// Normal case: reads the count straight out of the header field.
    #[test]
    fn stl_triangle_count_reads_header_field() {
        assert_eq!(stl_triangle_count(&stl_bytes_with_count(7)), 7);
    }

    /// Defensive case: a buffer shorter than the 84-byte header (e.g. an
    /// empty/degenerate export) must report `0`, not panic —
    /// `data.get(80..84)` returns `None` rather than indexing out of bounds.
    #[test]
    fn stl_triangle_count_defensive_on_short_buffer() {
        assert_eq!(stl_triangle_count(&[]), 0);
        assert_eq!(stl_triangle_count(&[0u8; 10]), 0);
    }

    /// Normal case: counts `<triangle ` elements in a real `write_3mf` output.
    #[test]
    fn threemf_triangle_count_reads_real_archive() {
        let mesh = repeated_triangle_mesh(7);
        let mut buf = Vec::new();
        write_3mf(&mesh, ThreeMfOptions::default(), &mut buf).expect("write_3mf should succeed");
        assert_eq!(threemf_triangle_count(&buf), 7);
    }

    /// An empty buffer (no `<triangle ` element at all) reports `0`, not a
    /// panic.
    #[test]
    fn threemf_triangle_count_empty_is_zero() {
        assert_eq!(threemf_triangle_count(&[]), 0);
    }

    /// Cross-writer parity (reviewer_comprehensive correctness_consistency
    /// finding): `stl_triangle_count` and `threemf_triangle_count` must
    /// report the same count when reading the REAL bytes
    /// `reify_ir::write_stl_binary` / `reify_ir::write_3mf` emit for the SAME
    /// mesh. Deriving both from one shared `Mesh` via the actual production
    /// writers — rather than two independently hand-built buffers — is what
    /// makes this a genuine dual-source check instead of a tautology.
    #[test]
    fn triangle_count_derivations_agree_for_same_mesh_via_real_writers() {
        let triangle_count = 11;
        let mesh = repeated_triangle_mesh(triangle_count);

        let mut stl_bytes = Vec::new();
        write_stl_binary(&mesh, &mut stl_bytes).expect("write_stl_binary should succeed");

        let mut mf_bytes = Vec::new();
        write_3mf(&mesh, ThreeMfOptions::default(), &mut mf_bytes)
            .expect("write_3mf should succeed");

        let stl = stl_triangle_count(&stl_bytes);
        let mf = threemf_triangle_count(&mf_bytes);
        assert_eq!(stl, triangle_count);
        assert_eq!(mf, triangle_count);
        assert_eq!(
            stl, mf,
            "stl_triangle_count and threemf_triangle_count must report the \
             same count when reading the real write_stl_binary/write_3mf \
             output for the same mesh"
        );
    }
}

// ── format_undef_cause unit tests (task 4327 / undef-self-describing δ) ──────
//
// Tests Q5: terse text rendering of every UndefCause variant.
// The function under test (`format_undef_cause`) does not yet exist —
// this module is RED until step-2 (GREEN) implements it.
#[cfg(test)]
mod format_undef_cause_tests {
    use super::format_undef_cause;
    use reify_core::{DiagnosticCode, SourceSpan, ValueCellId};
    use reify_ir::UndefCause;

    fn cell(entity: &str, member: &str) -> ValueCellId {
        ValueCellId::new(entity, member)
    }

    fn span(start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(start, end)
    }

    /// Unbound param: renders as "<entity>.<member> unbound".
    #[test]
    fn unbound_renders_cell_name_and_unbound() {
        let cause = UndefCause::Unbound {
            param: cell("S", "outer_diameter"),
            span: span(0, 14),
        };
        assert_eq!(format_undef_cause(&cause), "S.outer_diameter unbound");
    }

    /// AwaitingSolve: renders as "<entity>.<member> awaiting solve".
    #[test]
    fn awaiting_solve_renders_cell_name_and_awaiting_solve() {
        let cause = UndefCause::AwaitingSolve {
            param: cell("S", "k"),
        };
        assert_eq!(format_undef_cause(&cause), "S.k awaiting solve");
    }

    /// SolveFailed: renders as "solve failed: <detail>".
    #[test]
    fn solve_failed_renders_detail() {
        let cause = UndefCause::SolveFailed {
            detail: "infeasible".to_string(),
        };
        assert_eq!(format_undef_cause(&cause), "solve failed: infeasible");
    }

    /// UserUndef: renders as "explicit undef".
    #[test]
    fn user_undef_renders_explicit_undef() {
        let cause = UndefCause::UserUndef { span: span(5, 5) };
        assert_eq!(format_undef_cause(&cause), "explicit undef");
    }

    /// OpContractFailed (γ forward-compat): non-empty and contains "contract".
    #[test]
    fn op_contract_failed_is_nonempty_and_contains_contract() {
        let cause = UndefCause::OpContractFailed {
            code: DiagnosticCode::ConstraintViolated,
            span: span(0, 5),
        };
        let rendered = format_undef_cause(&cause);
        assert!(!rendered.is_empty(), "rendered string must not be empty");
        assert!(
            rendered.contains("contract"),
            "rendered string must contain \"contract\", got: {rendered:?}"
        );
    }
}

#[cfg(test)]
mod dfm_error_escalation_tests {
    use super::dfm_has_error_diagnostic;
    use reify_core::Diagnostic;

    /// Non-OCCT test: `dfm_has_error_diagnostic` must return `true` only for
    /// diagnostics whose message contains `E_DFM_`, distinguishing DFM Error
    /// violations from unrelated code-less Error diagnostics.
    ///
    /// This exercises the escalation predicate (used in `cmd_check`'s
    /// `has_dfm_rule && dfm_has_error_diagnostic(...)` gate) without requiring
    /// OCCT or a CLI exec — the gate logic is tested at the unit level with
    /// synthetic [`reify_core::Diagnostic`] values.
    ///
    /// Covers the reviewer concern (amend: robustness_error_handling) that a
    /// module carrying BOTH a DFMRule and an unrelated code-less Error diagnostic
    /// (e.g. FEA "no registered compute trampoline") must NOT escalate to FAILURE:
    /// the `E_DFM_` prefix match is keyed to the DFM diagnostic, not to mere
    /// code-lessness.
    #[test]
    fn dfm_error_escalation_requires_e_dfm_prefix() {
        // E_DFM_ prefix Error → escalates (DFM violation)
        let diag_e_dfm =
            Diagnostic::error("E_DFM_OVERHANG: face dips past the overhang limit");
        assert!(
            dfm_has_error_diagnostic(&[diag_e_dfm]),
            "E_DFM_ prefix Error must trigger escalation (DFM violation)"
        );

        // Another DFM Error code variant → also escalates
        let diag_e_undercut =
            Diagnostic::error("E_DFM_UNDERCUT: re-entrant wall — part cannot release");
        assert!(
            dfm_has_error_diagnostic(&[diag_e_undercut]),
            "E_DFM_UNDERCUT Error must trigger escalation"
        );

        // Code-less Error WITHOUT E_DFM_ prefix (e.g. FEA) → must NOT escalate
        let diag_fea = Diagnostic::error("no registered compute trampoline");
        assert!(
            !dfm_has_error_diagnostic(&[diag_fea]),
            "non-DFM code-less Error must NOT trigger escalation \
             (FEA 'no registered compute trampoline' must remain exit 0 under check)"
        );

        // W_DFM_ Warning → must NOT escalate (only Errors escalate)
        let diag_w_dfm =
            Diagnostic::warning("W_DFM_OVERHANG: face dips past the overhang limit");
        assert!(
            !dfm_has_error_diagnostic(&[diag_w_dfm]),
            "W_DFM_ Warning must NOT trigger escalation (non-fatal by design)"
        );

        // Empty slice → no escalation
        assert!(
            !dfm_has_error_diagnostic(&[]),
            "empty diagnostics must not trigger escalation"
        );

        // Mixed: FEA Error + W_DFM_ Warning → must NOT escalate
        // (the mix that triggered the reviewer concern: a DFM module
        // co-resident with an unrelated FEA Error must stay exit 0)
        let mixed: Vec<Diagnostic> = vec![
            Diagnostic::error("no registered compute trampoline"),
            Diagnostic::warning("W_DFM_OVERHANG: face dips past the overhang limit"),
        ];
        assert!(
            !dfm_has_error_diagnostic(&mixed),
            "FEA Error + W_DFM_ Warning must NOT trigger escalation \
             (only E_DFM_ Errors are fatal)"
        );
    }
}

#[cfg(test)]
mod merge_build_diagnostics_tests {
    use super::merge_build_diagnostics;
    use reify_core::{Diagnostic, DiagnosticCode, Severity};

    /// Renders a diagnostic down to the three fields the merge compares, so
    /// assertions read as data rather than as a hand-written `PartialEq`
    /// (`reify_core::Diagnostic` derives only `Debug, Clone` — diagnostics.rs:3834).
    fn key(d: &Diagnostic) -> (Severity, Option<DiagnosticCode>, String) {
        (d.severity, d.code, d.message.clone())
    }

    fn keys(ds: &[Diagnostic]) -> Vec<(Severity, Option<DiagnosticCode>, String)> {
        ds.iter().map(key).collect()
    }

    /// (1) check()'s list is copied VERBATIM, in order, at the front, and (2) a
    /// build-only entry is appended after it.
    ///
    /// This is PRD D2's pair of headline invariants in one assertion: the
    /// authoritative list is never reordered, filtered or deduped against
    /// itself (`check` remains the source of record), and a diagnostic that
    /// only `build()` produces still reaches the user.
    #[test]
    fn merge_copies_check_verbatim_and_appends_build_only() {
        let check = vec![
            Diagnostic::warning("first check warning"),
            Diagnostic::error("second check error"),
        ];
        let build = vec![Diagnostic::error(
            "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
        )];

        let merged = merge_build_diagnostics(&check, &build);

        assert_eq!(
            keys(&merged),
            [keys(&check), keys(&build)].concat(),
            "merged must be check's list verbatim and in order, then build-only entries"
        );
    }

    /// (3) A build entry structurally equal to a check entry by ALL THREE of
    /// `(severity, code, message)` is NOT appended.
    ///
    /// This is the "nothing check() prints today is printed twice" invariant:
    /// both `check()` and `build()` run the same eval front-end, so their
    /// diagnostic lists overlap heavily and a naive concatenation would double
    /// every shared entry on `check`'s stderr.
    #[test]
    fn merge_drops_build_entry_structurally_equal_to_a_check_entry() {
        let shared =
            Diagnostic::error("undefined value").with_code(DiagnosticCode::ConstraintIndeterminate);
        let check = vec![shared.clone()];
        let build = vec![shared.clone()];

        let merged = merge_build_diagnostics(&check, &build);

        assert_eq!(
            keys(&merged),
            keys(&check),
            "a build entry equal on all of (severity, code, message) must not be appended"
        );
    }

    /// (4) Same message, different `severity` → DISTINCT, so it IS appended.
    ///
    /// Severity is part of the key precisely because `check` degrades some
    /// conditions to warnings that `build` reports as hard errors; collapsing
    /// them would silently drop the more severe report.
    #[test]
    fn merge_treats_differing_severity_as_distinct() {
        let check = vec![Diagnostic::warning("same message")];
        let build = vec![Diagnostic::error("same message")];

        let merged = merge_build_diagnostics(&check, &build);

        assert_eq!(
            keys(&merged),
            [keys(&check), keys(&build)].concat(),
            "same message at a different severity is a distinct diagnostic and must be appended"
        );
    }

    /// (5) Same severity + message, different `code` — including `None` vs
    /// `Some(_)` — → DISTINCT, so it IS appended.
    ///
    /// `code` is the machine-readable identity downstream consumers match on
    /// (γ/#5403 gates on it), so two entries that differ only there are not
    /// interchangeable.
    #[test]
    fn merge_treats_differing_code_as_distinct() {
        // Some(a) vs Some(b)
        let check = vec![
            Diagnostic::error("coded message").with_code(DiagnosticCode::ConstraintIndeterminate)
        ];
        let build =
            vec![Diagnostic::error("coded message").with_code(DiagnosticCode::GdtIllegalModifier)];
        let merged = merge_build_diagnostics(&check, &build);
        assert_eq!(
            keys(&merged),
            [keys(&check), keys(&build)].concat(),
            "same severity+message under a different code must be appended"
        );

        // None vs Some(_) — the asymmetric case
        let check_uncoded = vec![Diagnostic::error("coded message")];
        let merged = merge_build_diagnostics(&check_uncoded, &build);
        assert_eq!(
            keys(&merged),
            [keys(&check_uncoded), keys(&build)].concat(),
            "an uncoded check entry must not absorb a coded build entry with the same message"
        );

        // …and the mirror direction: Some(_) check entry vs None build entry.
        let build_uncoded = vec![Diagnostic::error("coded message")];
        let merged = merge_build_diagnostics(&check, &build_uncoded);
        assert_eq!(
            keys(&merged),
            [keys(&check), keys(&build_uncoded)].concat(),
            "a coded check entry must not absorb an uncoded build entry with the same message"
        );
    }

    /// (6) Two identical entries WITHIN `build_diags` collapse to exactly ONE
    /// appended copy — membership is tested against the ACCUMULATING merged
    /// list, not only against check()'s original one.
    ///
    /// Empirically measured on the `mirror(...)` bare-origin fixture this task
    /// adds: `reify eval` emits
    /// `failed to compile geometry operation: missing or non-Length argument
    /// 'ox' for mirror` TWICE for a single call site. Deduping against check()'s
    /// list alone would print it twice on `check`'s stderr; PRD D2 only requires
    /// "at least once", and collapsing matches `check`'s existing output
    /// discipline.
    #[test]
    fn merge_collapses_duplicates_internal_to_build() {
        let dup = Diagnostic::error(
            "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
        );
        let check = vec![Diagnostic::warning("unrelated check warning")];
        let build = vec![dup.clone(), dup.clone()];

        let merged = merge_build_diagnostics(&check, &build);

        assert_eq!(
            keys(&merged),
            [keys(&check), keys(&[dup])].concat(),
            "a diagnostic build() emits twice for one call site must appear exactly once"
        );
    }

    /// (7) An empty `build_diags` returns exactly `check_diags`.
    ///
    /// This is the C2 byte-identity guarantee for every pre-5748 input: the
    /// lightweight (non-kernel) arm has no build result, so the merge must be a
    /// total no-op there.
    #[test]
    fn merge_with_empty_build_returns_check_unchanged() {
        let check = vec![
            Diagnostic::warning("w").with_code(DiagnosticCode::ConstraintIndeterminate),
            Diagnostic::error("e"),
        ];

        let merged = merge_build_diagnostics(&check, &[]);

        assert_eq!(
            keys(&merged),
            keys(&check),
            "an empty build list must leave check's diagnostics exactly as they were"
        );
    }
}

#[cfg(test)]
mod drop_falsified_indeterminate_diagnostics_tests {
    use super::drop_falsified_indeterminate_diagnostics;
    use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, Severity};
    use reify_eval::ConstraintCheckEntry;
    use reify_ir::Satisfaction;

    /// Renders a diagnostic down to the fields the filter reasons about, so
    /// assertions read as data (`reify_core::Diagnostic` derives only
    /// `Debug, Clone` — diagnostics.rs:3834, no `PartialEq`).
    fn key(d: &Diagnostic) -> (Severity, Option<DiagnosticCode>, String) {
        (d.severity, d.code, d.message.clone())
    }

    fn keys(ds: &[Diagnostic]) -> Vec<(Severity, Option<DiagnosticCode>, String)> {
        ds.iter().map(key).collect()
    }

    fn entry(entity: &str, index: u32, satisfaction: Satisfaction) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            id: ConstraintNodeId::new(entity, index),
            label: None,
            satisfaction,
        }
    }

    fn labeled(
        entity: &str,
        index: u32,
        label: &str,
        satisfaction: Satisfaction,
    ) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            id: ConstraintNodeId::new(entity, index),
            label: Some(label.to_string()),
            satisfaction,
        }
    }

    /// Builds the exact shape the checker emits (reify-constraints/src/lib.rs:187):
    /// `constraint {needle} indeterminate: undefined inputs: {cells}`, coded
    /// `ConstraintIndeterminate`, at `Warning` severity.
    fn indeterminate_warning(needle: &str, undefined: &str) -> Diagnostic {
        Diagnostic::warning(format!(
            "constraint {} indeterminate: undefined inputs: {}",
            needle, undefined
        ))
        .with_code(DiagnosticCode::ConstraintIndeterminate)
    }

    /// (1) The headline case: `check()` resolved the constraint to `Satisfied`,
    /// so `build()`'s surviving "indeterminate" claim about it is FALSE and
    /// must not reach stderr.
    ///
    /// This is the reviewer-measured self-contradiction verbatim: stdout says
    /// `OK SphereCheck#constraint[0]` + `All constraints satisfied.` while
    /// stderr says that same constraint is indeterminate.
    #[test]
    fn drops_indeterminate_claim_falsified_by_a_satisfied_verdict() {
        let build = vec![indeterminate_warning(
            "SphereCheck#constraint[0]",
            "SphereCheck.subject",
        )];
        let results = vec![entry("SphereCheck", 0, Satisfaction::Satisfied)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert!(
            kept.is_empty(),
            "a ConstraintIndeterminate claim about a constraint the authoritative \
             list reports Satisfied is false and must be dropped, got {:?}",
            keys(&kept)
        );
    }

    /// (2) `Violated` falsifies the indeterminacy claim exactly as `Satisfied`
    /// does — the property that matters is DEFINITENESS, not the polarity of
    /// the verdict.
    #[test]
    fn drops_indeterminate_claim_falsified_by_a_violated_verdict() {
        let build = vec![indeterminate_warning("Bracket#constraint[2]", "Bracket.t")];
        let results = vec![entry("Bracket", 2, Satisfaction::Violated)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert!(
            kept.is_empty(),
            "a Violated verdict falsifies an indeterminacy claim just as a \
             Satisfied one does, got {:?}",
            keys(&kept)
        );
    }

    /// (3) When the authoritative verdict is ALSO `Indeterminate`, the
    /// diagnostic is still TRUE — dropping it would delete the user's only
    /// explanation of why the constraint could not be decided.
    #[test]
    fn keeps_indeterminate_claim_when_the_verdict_is_also_indeterminate() {
        let build = vec![indeterminate_warning(
            "Bracket#constraint[2]",
            "Bracket.tolerance",
        )];
        let results = vec![entry("Bracket", 2, Satisfaction::Indeterminate)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert_eq!(
            keys(&kept),
            keys(&build),
            "an indeterminacy claim matching an Indeterminate verdict is still \
             true and must survive"
        );
    }

    /// (4) Drop only on POSITIVE falsification. An id the authoritative list
    /// never mentions is not evidence of anything, so the diagnostic stays —
    /// this is what preserves PRD D2's "every build()-only diagnostic appears
    /// at least once".
    #[test]
    fn keeps_indeterminate_claim_when_no_entry_matches_the_id() {
        let build = vec![indeterminate_warning("Ghost#constraint[0]", "Ghost.x")];
        let results = vec![entry("Bracket", 0, Satisfaction::Satisfied)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert_eq!(
            keys(&kept),
            keys(&build),
            "absence from the authoritative list must never drop a diagnostic — \
             only a positive definite verdict for that same constraint may"
        );
    }

    /// (5) The filter falsifies exactly ONE claim: indeterminacy. A diagnostic
    /// carrying the same needle under a different code (e.g. the
    /// `ConstraintViolated` summary line) is untouched even when a definite
    /// verdict exists for that id.
    #[test]
    fn never_drops_a_diagnostic_that_is_not_coded_constraint_indeterminate() {
        let violated = Diagnostic::error("constraint Bracket#constraint[2] indeterminate-ish note")
            .with_code(DiagnosticCode::ConstraintViolated);
        let uncoded = Diagnostic::warning(
            "constraint Bracket#constraint[2] indeterminate: hand-rolled uncoded line",
        );
        let build = vec![violated, uncoded];
        let results = vec![entry("Bracket", 2, Satisfaction::Satisfied)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert_eq!(
            keys(&kept),
            keys(&build),
            "only ConstraintIndeterminate-coded entries are subject to the \
             filter; every other code (and the uncoded case) must survive"
        );
    }

    /// (6) Label-preferring needle. When a constraint carries a label the
    /// checker embeds the LABEL in the message, not the raw id
    /// (engine_build.rs:5000-5004's `labeled_diagnostics` rewrite), so the
    /// matcher must prefer `label` exactly as `merge_post_build_verdicts` does.
    #[test]
    fn matches_on_the_label_when_the_entry_carries_one() {
        let build = vec![indeterminate_warning("wall_thick", "Bracket.t")];
        let results = vec![labeled("Bracket", 3, "wall_thick", Satisfaction::Satisfied)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert!(
            kept.is_empty(),
            "a labeled constraint's diagnostic names the LABEL, so the matcher \
             must use it in preference to the raw id, got {:?}",
            keys(&kept)
        );
    }

    /// (7) PREFIX-COLLISION SAFETY — the reason the matcher is anchored.
    ///
    /// `Foo#constraint[1]` is a strict prefix of `Foo#constraint[10]`, so a
    /// bare `message.contains(needle)` would let a definite verdict for
    /// `[1]` wrongly delete `[10]`'s still-true warning. Anchoring on the full
    /// `constraint {needle} indeterminate` span makes that impossible:
    /// `constraint Foo#constraint[1] indeterminate` is not a substring of
    /// `constraint Foo#constraint[10] indeterminate`.
    #[test]
    fn anchored_matcher_survives_an_id_prefix_collision() {
        let build = vec![indeterminate_warning("Foo#constraint[10]", "Foo.x")];
        let results = vec![
            entry("Foo", 1, Satisfaction::Satisfied),
            entry("Foo", 10, Satisfaction::Indeterminate),
        ];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert_eq!(
            keys(&kept),
            keys(&build),
            "a definite verdict for Foo#constraint[1] must NOT falsify the \
             warning about Foo#constraint[10] — the matcher must be anchored, \
             not a bare contains()"
        );
    }

    /// (8) Not every `ConstraintIndeterminate` diagnostic names a constraint
    /// id: `Conforms INDETERMINATE: <reason>` (engine_constraints.rs:2156)
    /// carries the code with no id at all. Nothing can falsify it, so it always
    /// survives — the filter must not fall back to a broad code-only drop.
    #[test]
    fn keeps_idless_indeterminate_diagnostics() {
        let build = vec![
            Diagnostic::warning("Conforms INDETERMINATE: subject has no realized geometry")
                .with_code(DiagnosticCode::ConstraintIndeterminate),
        ];
        let results = vec![entry("SphereCheck", 0, Satisfaction::Satisfied)];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert_eq!(
            keys(&kept),
            keys(&build),
            "a ConstraintIndeterminate diagnostic naming no constraint id \
             cannot be falsified by any verdict and must survive"
        );
    }

    /// (9) An empty authoritative list falsifies nothing → `build_diags`
    /// verbatim. This is the C2 no-op guarantee for the arm where no
    /// constraints exist at all.
    #[test]
    fn empty_constraint_results_returns_build_diags_verbatim() {
        let build = vec![
            indeterminate_warning("A#constraint[0]", "A.x"),
            Diagnostic::error("failed to compile geometry operation"),
        ];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &[]);

        assert_eq!(
            keys(&kept),
            keys(&build),
            "with no verdicts to falsify anything, the build list must pass \
             through untouched"
        );
    }

    /// (10) Surviving entries keep their relative order — the filter is a
    /// `retain`, never a reorder. `report_eval_output` prints the list in
    /// order, so chronology is user-visible.
    #[test]
    fn preserves_the_order_of_surviving_entries() {
        let first = Diagnostic::error("failed to compile geometry operation: 'ox' for mirror");
        let dropped = indeterminate_warning("A#constraint[0]", "A.x");
        let second = indeterminate_warning("A#constraint[1]", "A.y");
        let third = Diagnostic::warning("W_DFM_OVERHANG: face dips past the overhang limit");
        let build = vec![first.clone(), dropped, second.clone(), third.clone()];
        let results = vec![
            entry("A", 0, Satisfaction::Satisfied),
            entry("A", 1, Satisfaction::Indeterminate),
        ];

        let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

        assert_eq!(
            keys(&kept),
            keys(&[first, second, third]),
            "the filter must retain in place: surviving entries keep their \
             original relative order"
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
        let engine = configured_eval_engine(reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            None,
        ));
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

    /// The `--cache-dir` flag override (modelled by calling
    /// `engine.set_persistent_cache_dir` after `configured_eval_engine`)
    /// must replace whatever dir was set by the env/default resolver.
    #[test]
    fn cache_dir_cli_override_replaces_configured_dir() {
        let tmp = tempfile::TempDir::new().expect("tmp dir must be creatable for --cache-dir test");
        let mut engine = configured_eval_engine(reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            None,
        ));
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
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
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
