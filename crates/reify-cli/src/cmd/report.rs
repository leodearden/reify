//! `reify report --bom <file>`: the BOM / cost / waste / provenance report.

use std::process::ExitCode;

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;

use crate::{configured_eval_engine, parse_and_compile};

/// `reify report --bom <file>` — roll up and render a BOM / cost / waste /
/// provenance report (io-lifecycle-bom-cost #4292, boundary α).
///
/// Compiles + evaluates the design on the lightweight kernel-free eval path
/// (lifecycle `StructureInstance` cells populate under plain `engine.eval`; the
/// rollup needs no geometry realization), builds a [`reify_eval::BomReport`],
/// and renders it to stdout. Compile / eval `Severity::Error`s propagate to a
/// non-zero exit; diagnostics go to stderr so stdout stays the rendered report.
pub(crate) fn cmd_report(args: &[String]) -> ExitCode {
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
