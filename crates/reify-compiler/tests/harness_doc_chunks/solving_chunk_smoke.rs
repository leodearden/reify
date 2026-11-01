//! Truth checks for the `solving` language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/solving.md`), which tells design agents how to
//! let the solver choose a value with `auto`.
//!
//! Every ```` ```reify ```` example it serves must resolve: on an engine that solves the
//! way `reify check` does, each fence compiles, every `auto` is touched by a constraint,
//! and every constraint is Satisfied. Each "not yet" the chunk documents — the
//! sub-instance caveat and the bound-seeking-objective caveats — is pinned, so the chunk
//! is corrected the day it stops holding.
//!
//! The chunk is read through `reify_mcp::tools::language_chunks::get_chunk`, the seam the
//! `reify_language_reference` tool serves from, so an unregistered chunk fails here too.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::{Engine, SolverPosture};
use reify_ir::{ResolutionProfile, Satisfaction};
use reify_mcp::tools::language_chunks::get_chunk;
use reify_test_support::{
    cell_value, collect_errors, compile_source_with_stdlib, parse_and_compile_with_stdlib,
};

use crate::chunk_io::chunk_label;
use crate::chunk_markdown::tagged_fence_bodies;

const TOPIC: &str = "solving";

/// An engine with the production solver installed, as `reify check` builds one.
fn solving_engine() -> Engine {
    Engine::new(
        SolverPosture::Solve(ResolutionProfile::FRESH),
        Box::new(SimpleConstraintChecker),
        None,
    )
}

/// Why `fence` does not fully resolve on a solving engine, or `None` if it does.
fn resolution_failures(fence: &str) -> Option<Vec<String>> {
    let compiled = compile_source_with_stdlib(fence);
    let compile_errors = collect_errors(&compiled.diagnostics);
    if !compile_errors.is_empty() {
        return Some(vec![format!("compile errors: {compile_errors:?}")]);
    }
    let result = solving_engine().check(&compiled);
    let mut failures: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error || d.code == Some(DiagnosticCode::Underdetermined)
        })
        .map(|d| format!("diagnostic: {}", d.message))
        .collect();
    if result.constraint_results.is_empty() {
        failures.push("no constraint results: the example shows nothing resolving".into());
    }
    failures.extend(
        result
            .constraint_results
            .iter()
            .filter(|entry| entry.satisfaction != Satisfaction::Satisfied)
            .map(|entry| {
                format!(
                    "{:?} is {:?} ({:?})",
                    entry.id, entry.satisfaction, entry.indeterminate_reason
                )
            }),
    );
    (!failures.is_empty()).then_some(failures)
}

#[test]
fn every_solving_chunk_reify_fence_resolves_under_a_solving_engine() {
    let label = chunk_label(TOPIC);
    let chunk = get_chunk(TOPIC).unwrap_or_else(|| {
        panic!("no `{TOPIC}` topic is served by reify_mcp::tools::language_chunks ({label})")
    });
    let fences = tagged_fence_bodies(chunk, "reify", &label);
    assert!(
        !fences.is_empty(),
        "{label} serves no ```reify fence, so nothing shows `auto` resolving"
    );
    let failures: Vec<String> = fences
        .iter()
        .enumerate()
        .filter_map(|(ordinal, fence)| {
            resolution_failures(fence).map(|reasons| {
                format!(
                    "fence #{ordinal}:\n  {}\n--- fence source ---\n{fence}",
                    reasons.join("\n  ")
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "every ```reify fence in {label} must compile, touch every `auto` with a constraint, \
         and satisfy every constraint on a solving engine:\n{}",
        failures.join("\n\n")
    );
}

const SUB_INSTANCE_CAVEAT_SOURCE: &str = "\
structure def Leaf {
    param length : Length = 5mm
    let doubled = length * 2
}

structure def Assembly {
    sub x = Leaf(length: auto)
    constraint self.x.length >= 8mm && self.x.length <= 12mm
}
";

/// Characterization pin for the chunk's sub-instance caveat: a constructor-argument
/// `auto` resolves for the parent's constraints, but the sub's derived members are not
/// re-evaluated at the solved value.
#[test]
fn the_sub_instance_caveat_the_solving_chunk_documents_still_holds() {
    let compiled = parse_and_compile_with_stdlib(SUB_INSTANCE_CAVEAT_SOURCE);
    let result = solving_engine().eval(&compiled);

    let length = cell_value(&result, "Assembly.x", "length").as_f64();
    assert!(
        length.is_some_and(|metres| (metres - 0.01).abs() < 1e-9),
        "`Assembly.x.length` must resolve to the bracket centre 0.01 m, got {length:?}"
    );

    let doubled = cell_value(&result, "Assembly.x", "doubled");
    assert!(
        doubled.is_undef(),
        "`Assembly.x.doubled` now follows the solved length ({doubled:?}): the sub-instance \
         caveat no longer holds. Delete the caveat paragraph from {} and this pin \
         (docs/prds/v0_6/solver-driver-parity.md §5 B2).",
        chunk_label(TOPIC)
    );
}

/// What a solving engine does with a strict `auto` under a bound-seeking objective.
#[derive(Debug, Clone, Copy)]
enum ObjectiveOutcome {
    Resolves { metres: f64 },
    FailsNonUnique,
}

const OBJECTIVE_STRUCTURE: &str = "Shim";
const TWO_SIDED_BOUNDS: &str = "thickness >= 2mm && thickness <= 6mm";
const ONE_SIDED_BOUND: &str = "thickness >= 2mm";

fn shim_source(bounds: &str, objective: &str) -> String {
    format!(
        "structure def {OBJECTIVE_STRUCTURE} {{\n    param thickness : Length = auto\n    \
         constraint {bounds}\n    {objective}\n}}\n"
    )
}

/// Why `source` does not show `expected` on a solving engine, or `None` if it does.
fn objective_outcome_mismatch(source: &str, expected: ObjectiveOutcome) -> Option<String> {
    let compiled = parse_and_compile_with_stdlib(source);
    let result = solving_engine().eval(&compiled);
    let thickness = cell_value(&result, OBJECTIVE_STRUCTURE, "thickness");
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let holds = match expected {
        ObjectiveOutcome::Resolves { metres } => {
            errors.is_empty()
                && thickness
                    .as_f64()
                    .is_some_and(|value| (value - metres).abs() < 1e-9)
        }
        ObjectiveOutcome::FailsNonUnique => {
            thickness.is_undef()
                && errors
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique))
        }
    };
    (!holds).then(|| format!("thickness = {thickness:?}, error diagnostics = {errors:?}"))
}

/// Characterization pin for the chunk's objective caveats: a `minimize` or `maximize`
/// that pushes a strict `auto` toward a bound neither reaches that bound nor stands in
/// for a missing one. What happens splits on the direction of the push.
#[test]
fn the_objective_caveats_the_solving_chunk_documents_still_hold() {
    let cases = [
        (
            "two-sided [2mm, 6mm], minimize thickness",
            shim_source(TWO_SIDED_BOUNDS, "minimize thickness"),
            ObjectiveOutcome::Resolves { metres: 0.004 },
        ),
        (
            "two-sided [2mm, 6mm], maximize thickness",
            shim_source(TWO_SIDED_BOUNDS, "maximize thickness"),
            ObjectiveOutcome::FailsNonUnique,
        ),
        (
            "one-sided >= 2mm, minimize thickness",
            shim_source(ONE_SIDED_BOUND, "minimize thickness"),
            ObjectiveOutcome::FailsNonUnique,
        ),
        (
            "one-sided >= 2mm, maximize thickness",
            shim_source(ONE_SIDED_BOUND, "maximize thickness"),
            ObjectiveOutcome::Resolves { metres: 10.0 },
        ),
    ];
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(label, source, expected)| {
            objective_outcome_mismatch(source, *expected)
                .map(|observed| format!("{label}: expected {expected:?}, observed {observed}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "the objective behaviour the `solving` chunk and docs/getting-started.md §3 \
         ('Let the solver choose a value') describe has changed. Re-measure, update the \
         bound-seeking-objective paragraph of {} and that getting-started sentence, then \
         update this pin:\n{}",
        chunk_label(TOPIC),
        failures.join("\n")
    );
}
