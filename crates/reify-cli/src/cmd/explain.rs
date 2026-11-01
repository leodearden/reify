//! `reify explain <file>`: per-cell objective provenance for resolved auto parameters.

use std::process::ExitCode;

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;

use crate::args::{EXPLAIN_USAGE, parse_single_file_arg};
use crate::{configured_eval_engine, parse_and_compile};

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
pub(crate) fn cmd_explain(args: &[String]) -> ExitCode {
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
