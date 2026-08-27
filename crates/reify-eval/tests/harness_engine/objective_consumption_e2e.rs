//! Task 5417 (DIC γ) — `E_OBJECTIVE_UNCONSUMED`, the runtime half.
//!
//! PRD `docs/prds/v0_6/declared-intent-consumption-accounting.md` §3/§4.2.
//!
//! The compile half (`E_OBJECTIVE_INERT`) rejects an objective that provably
//! governs nothing. This half covers the case the compiler cannot see: an
//! objective that IS well-posed — it reaches a real solver variable — but that
//! the solver then silently discards, because the decomposition built no
//! component for it to attach to. `docs/prds/v0_6/fixtures/dic_min_unconstrained.ri`
//! is the canonical probe: `param a = auto(free)` + `minimize (a-3)*(a-3)` and
//! no constraints. Baseline before this rule: `a = undef (awaiting solve)` and
//! the objective is never mentioned — the declaration evaporates in silence,
//! which is the INV-SF-3 failure the PRD exists to eradicate.
//!
//! The rule must be quiet everywhere else. The negative cases below pin the
//! three ways it could go wrong: an objective the solver DID consume (O1), one
//! whose reachable autos are all concretely bound this run (O2, the
//! vacuous-healthy rule), and the synthesised Chebyshev-centre objective a
//! scope never declared (task 4013's exemption).
//!
//! Assertions target `DiagnosticCode`, never message substrings (INV-SF-6), and
//! the sources are byte-mirrors of the committed PRD fixtures so the tests track
//! the same user-observable signal the PRD measured.
//!
//! Written RED in step-9: nothing emits `ObjectiveUnconsumed` until step-10.
//!
//! **Placement note for the implementer.** This module is the arbiter of WHERE
//! the emission goes. The gate belongs beside the #4804
//! `W_SOLVER_OPTIMALITY_UNPROVEN` site on the single-scope objective path; if
//! `eval()` turns out to skip that path entirely for a zero-constraint scope,
//! the emission must move to whichever objective-path site actually executes for
//! the `dic_min_unconstrained` fixture, and the reason recorded there.

use reify_constraints::SolverRegistry;
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_test_support::{MockConstraintChecker, compile_source_with_stdlib};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Compile `source` (asserting it is compile-clean) and evaluate it with the
/// production [`SolverRegistry`] — the same one `reify-cli` wires
/// (`main.rs`'s `.with_solver(Box::new(SolverRegistry::production()))`) — so the
/// objective path these tests describe is the one users actually hit. A bare
/// `DimensionalSolver` is NOT equivalent: it solves the unconstrained fixture
/// outright and leaves an `==`-pinned auto undef, so both would describe an
/// engine no user runs.
///
/// The compile-clean assertion is load-bearing: every fixture here must pass the
/// compile half untouched, or a `ObjectiveUnconsumed`-shaped hole could be
/// masked by an `ObjectiveInert` Error raised earlier on the same source.
fn eval_with_solver(source: &str) -> reify_eval::EvalResult {
    eval_with_solver_keeping_engine(source).1
}

/// As [`eval_with_solver`], but hands back the `Engine` too so a caller can
/// read `engine.snapshot()`.
///
/// The merged-cluster cases below need it: `SnapshotProvenance::Resolution`'s
/// comma-joined member label is the only merged-vs-single-scope signal that
/// survives into a post-`eval()` observation, and it is what makes their
/// anti-vacuity assertion possible (see `assert_merged_cluster_spanned`).
fn eval_with_solver_keeping_engine(source: &str) -> (Engine, reify_eval::EvalResult) {
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "fixture must be compile-clean so the runtime rule is what is under \
         test; got {compile_errors:?}"
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(SolverRegistry::production()));
    let result = engine.eval(&compiled);
    (engine, result)
}

/// Every `ObjectiveUnconsumed` diagnostic in an eval result.
fn unconsumed(result: &reify_eval::EvalResult) -> Vec<&Diagnostic> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ObjectiveUnconsumed))
        .collect()
}

/// Assert exactly one `ObjectiveUnconsumed` Error, that it names `cell`, and
/// that it is renderable.
///
/// "Exactly one" is the #5014 aggregation rule: one diagnostic per objective
/// *declaration* naming the FULL unconsumed set — never one per component, per
/// trial, or per auto.
fn assert_one_unconsumed_error_naming(result: &reify_eval::EvalResult, cell: &str) {
    let found = unconsumed(result);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one ObjectiveUnconsumed (the #5014 aggregation rule); \
         got {found:?}"
    );
    let diag = found[0];
    assert_eq!(
        diag.severity,
        Severity::Error,
        "E_* mnemonics are Errors by house convention; got {:?}",
        diag.severity
    );
    assert!(
        diag.message.contains("E_OBJECTIVE_UNCONSUMED"),
        "the message must carry the PRD-prose mnemonic so the CLI renders it; \
         got {:?}",
        diag.message
    );
    assert!(
        diag.message.contains(cell),
        "the message must name the unconsumed auto `{cell}`; got {:?}",
        diag.message
    );
}

/// Assert the run reports no unconsumed objective at all.
fn assert_no_unconsumed(result: &reify_eval::EvalResult, why: &str) {
    let found = unconsumed(result);
    assert!(found.is_empty(), "{why}; got {found:?}");
}

/// Anti-vacuity guard for the negative cases: the fixture must really have
/// reached the solver and bound the auto.
///
/// Without this a negative case degenerates into "nothing solved, so of course
/// nothing was reported" — it would keep passing even if the rule were rewritten
/// to fire on everything.
fn assert_bound(result: &reify_eval::EvalResult, entity: &str, member: &str) {
    let id = ValueCellId::new(entity, member);
    let got = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("cell {entity}.{member} is absent from the values map"));
    assert!(
        !matches!(got, reify_ir::Value::Undef),
        "cell {entity}.{member} must be concretely bound for this negative case \
         to mean anything; got {got:?}"
    );
}

/// Anti-vacuity guard for the MERGED-CLUSTER cases: the shape must really have
/// been solved as one cross-scope merged problem, spanning exactly
/// `expected_members`.
///
/// Load-bearing in both directions. A merged fixture that quietly stopped
/// forming a cluster would fall through to the single-scope emission site, so
/// the positive case would keep passing while testing nothing about the arm it
/// names; and the false-positive canary would stop guarding the merged arm at
/// all. `SnapshotProvenance::Resolution.scope` is the comma-joined
/// `cluster.scopes` member-name label `dispatch_merged_cluster_solve` writes
/// (`merged_scope_label`) — a per-template solve writes a single name there,
/// so the member SET is the discriminator. Asserted as a set-and-order over the
/// split parts rather than the exact joined string, mirroring
/// `merged_cluster_solve.rs`'s
/// `merged_cluster_snapshot_provenance_scope_is_comma_joined_member_names`, so
/// a benign separator change does not fail it.
fn assert_merged_cluster_spanned(engine: &Engine, expected_members: &[&str]) {
    let snapshot = engine
        .snapshot()
        .expect("engine must have a snapshot after eval()");
    match &snapshot.provenance {
        reify_ir::SnapshotProvenance::Resolution { scope, .. } => {
            let members: Vec<&str> = scope.split(", ").collect();
            assert_eq!(
                members, expected_members,
                "this fixture must be solved as ONE merged cross-scope cluster \
                 spanning {expected_members:?} — otherwise it exercises the \
                 single-scope emission site, not the merged one; got {scope:?}"
            );
        }
        other => panic!(
            "expected SnapshotProvenance::Resolution after a merged solve; got \
             {other:?}"
        ),
    }
}

// ── (B7) the target: a well-posed objective the solver drops ────────────────

/// Byte-mirror of `docs/prds/v0_6/fixtures/dic_min_unconstrained.ri`.
///
/// `minimize (a-3)*(a-3)` genuinely reaches the auto `a`, so it passes the
/// compile half. But with no constraints the decomposition builds zero
/// components, the registry has nothing to attach the cost to, and the
/// objective is dropped. That drop is exactly what must stop being silent.
const UNCONSTRAINED: &str = "\
module dic_min_unconstrained

structure DicMinUnconstrained {
    param a : Real = auto(free)
    minimize (a - 3.0) * (a - 3.0)
}
";

#[test]
fn unconstrained_objective_reports_unconsumed() {
    let result = eval_with_solver(UNCONSTRAINED);
    assert_one_unconsumed_error_naming(&result, "a");
}

/// The diagnostic is additive: `a`'s pre-existing undef classification is the
/// PRD's recorded baseline (`a = undef (awaiting solve)`) and must not shift.
/// γ reports the silence — it does not change what the solver does (O1).
#[test]
fn unconstrained_objective_leaves_the_undef_classification_alone() {
    let result = eval_with_solver(UNCONSTRAINED);
    let id = ValueCellId::new("DicMinUnconstrained", "a");
    let got = result
        .values
        .get(&id)
        .expect("DicMinUnconstrained.a must be present in the values map");
    assert!(
        matches!(got, reify_ir::Value::Undef),
        "baseline: `a` stays undef awaiting solve — γ adds a diagnostic, it does \
         not change the solve; got {got:?}"
    );
}

// ── (B8 / O1) a governing objective that solves stays quiet ─────────────────

/// The `bt1_single_scope.ri` shape: an auto bracketed by two constraints, with
/// an objective over it. The decomposition builds a component, the component
/// consumes the objective, the solve happens — nothing to report.
///
/// This is the case that proves the rule keys off *consumption*, not off
/// "an objective exists".
#[test]
fn governing_objective_that_solves_reports_nothing() {
    let source = "\
module dic_governing

structure DicGoverning {
    param w : Length = auto
    constraint w >= 10mm
    constraint w <= 50mm
    minimize w
}
";
    let result = eval_with_solver(source);
    assert_bound(&result, "DicGoverning", "w");
    assert_no_unconsumed(
        &result,
        "a component consumed this objective and solved it (O1)",
    );
}

// ── (O2) vacuous-healthy: every reachable auto is bound this run ────────────

/// An objective whose reachable autos are all concretely bound this run has
/// nothing left to optimise, and saying so would be noise on a healthy model.
///
/// The reach here spans BOTH an auto (`w`) and a concrete param (`base`), which
/// is what distinguishes this case from
/// `let_indirected_objective_over_a_solved_auto_reports_nothing` above: it pins
/// that the unbound-remainder test intersects with `auto_params` per id, so
/// `base` never enters the set and cannot keep the diagnostic alive once `w` is
/// bound. Consumption is `FallbackComponentZero` (the registry matches the
/// objective's DIRECT refs, which name only `total`), so condition 2 does not
/// fire — the vacuous-healthy rule is the sole reason this stays quiet.
///
/// **Pin-shape note (resolved in step-10).** The shape step-9 first wrote here —
/// `w` pinned by two mutually-tight inequalities — is RED on its *premise*, not
/// on the rule. Probing `SolverRegistry::production()` directly, none of
/// `constraint w == 25mm`, `w >= 25mm` + `w <= 25mm`, or `w == base` binds `w`
/// at all: each reports `constraints could not be satisfied (max absolute
/// residual: 5.00e-7)` and leaves the cell undef, so the anti-vacuity guard
/// tripped before the rule was ever consulted. The same probe found that
/// multi-auto shapes (two bracketed autos, autos coupled by an equality, or a
/// shared `w + h` bracket) also leave every auto undef. What DOES bind under the
/// production registry is a SINGLE auto with a two-sided inequality bracket,
/// which is the shape used here. That is a pre-existing solver characteristic,
/// independent of DIC γ — γ adds a diagnostic, it does not change what solves.
#[test]
fn objective_whose_autos_are_all_bound_reports_nothing() {
    let source = "\
module dic_vacuous

structure DicVacuous {
    param base : Length = 4mm
    param w : Length = auto
    constraint w >= 10mm
    constraint w <= 50mm
    let total = w + base
    minimize total
}
";
    let result = eval_with_solver(source);
    assert_bound(&result, "DicVacuous", "w");
    assert_no_unconsumed(
        &result,
        "every objective-reachable auto is bound this run — the vacuous-healthy \
         rule (O2)",
    );
}

// ── (task 4013) the synthesised centrality objective is exempt ──────────────

/// A scope that declares NO objective but picks up the synthesised
/// Chebyshev-centre one must stay silent: the rule reports *declared* intent
/// that the engine discarded, and there is no declaration here to discard.
///
/// The gate keys off `template.objective.is_some()`, which is the exact
/// structural test — a synthetic-centrality scope has `objective == None` at
/// compile time by construction.
#[test]
fn synthesised_centrality_objective_reports_nothing() {
    let source = "\
module dic_synthetic

structure DicSynthetic {
    param w : Length = auto
    constraint w >= 10mm
    constraint w <= 50mm
}
";
    let result = eval_with_solver(source);
    assert_no_unconsumed(
        &result,
        "no user-declared objective exists, so nothing was discarded (task 4013)",
    );
}

// ── order-independence w.r.t. PRD 2 ─────────────────────────────────────────

/// An objective that reads a `let` which reads a solved auto is transitively
/// consumed, and must stay silent — `dependent_cells` already encodes the
/// let-indirection, so this holds whichever order γ and PRD 2's let-tracing fix
/// (tasks 5396 / 5467-5474) land in.
#[test]
fn let_indirected_objective_over_a_solved_auto_reports_nothing() {
    let source = "\
module dic_let_indirect

structure DicLetIndirect {
    param w : Length = auto
    let doubled = w * 2.0
    constraint w >= 10mm
    constraint w <= 50mm
    minimize doubled
}
";
    let result = eval_with_solver(source);
    assert_bound(&result, "DicLetIndirect", "w");
    assert_no_unconsumed(
        &result,
        "the objective reaches `w` through the let and the component consumed \
         it — order-independent w.r.t. PRD 2",
    );
}

// ── the MERGED-CLUSTER arm ──────────────────────────────────────────────────
//
// Everything above exercises the SINGLE-SCOPE emission site. The cases below
// cover `dispatch_merged_cluster_solve` — the second path
// `objective_unconsumed_diagnostic`'s own doc comment already claims calls it
// ("the single-scope (`eval`) and merged-cluster (`dispatch_merged_cluster_solve`)
// paths both call it"). Written RED in step-14: until step-15 wires that site,
// only the single-scope one calls it and that claim is FALSE in-tree, so
// INV-SF-3 has a real hole on every merged model.
//
// Why a merged cluster can reach `NoComponents` at all: `compute_clusters`
// (`resolve_order.rs`) seeds a cluster on cross-scope OBJECTIVE reads ALONE —
// constraint reads are deliberately NOT unioned — so an objective-span cluster
// with zero constraints anywhere is a first-class shape, and
// `decompose_into_components` returns `vec![]` when `constraints.is_empty()`.
// That is the merged analogue of `dic_min_unconstrained.ri`.

/// The `examples/whole_model_joint_drive.ri` shape with the child's two
/// bracketing constraints DELETED.
///
/// The parent's INLINED `minimize cost(self.descendants)` expands to
/// `[RivetedPanel.rivets.line_cost].sum`, which reads a cell of the CHILD — so
/// the objective span couples `{RivetedPanel, Rivet}` into one `MergedSolve`
/// cluster even though no constraint exists anywhere. The merged problem then
/// carries one auto and zero constraints ⇒ zero components ⇒ the objective is
/// dropped, and `Rivet.quantity_produced` is never written back.
///
/// **The aggregate MUST stay inlined in the `minimize`.** Putting it behind a
/// `let` forms NO cluster at all (pinned by `resolve_order.rs`'s
/// `objective_must_inline_the_aggregate_to_couple`: δ's C1 rule expands
/// objective TERMS only, so an unexpanded `cost(self.descendants)` surfaces no
/// `line_cost` read), which would silently demote this to a single-scope test
/// that passes for the wrong reason. `assert_merged_cluster_spanned` is the
/// executable guard against exactly that.
const MERGED_UNCONSTRAINED: &str = r#"
module dic_merged_unconstrained

structure def Rivet : Costed {
    param supplier          : String = "Acme Fastener"
    param part_number       : String = "R-4210"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h

    param quantity_produced : Real   = auto(free)
}

structure RivetedPanel {
    sub rivets = Rivet()

    minimize cost(self.descendants)
}
"#;

#[test]
fn merged_cluster_unconstrained_objective_reports_unconsumed() {
    let (engine, result) = eval_with_solver_keeping_engine(MERGED_UNCONSTRAINED);
    assert_merged_cluster_spanned(&engine, &["Rivet", "RivetedPanel"]);
    assert_one_unconsumed_error_naming(&result, "Rivet.quantity_produced");
}

/// FALSE-POSITIVE CANARY — and the load-bearing case of the pair.
///
/// `examples/whole_model_joint_drive.ri` UNCHANGED must stay quiet, and it is
/// the one in-tree model that proves the merged wiring passes a genuinely
/// populated `bound_this_run`. It ALREADY classifies `FallbackComponentZero`
/// today: the expanded objective's DIRECT refs are
/// `{RivetedPanel.rivets.line_cost}` (an instance-path let) while the
/// component's autos are structure-keyed `{Rivet.quantity_produced}`, so the
/// registry's first-match scan finds nothing even though a component exists —
/// i.e. gate condition (2) FIRES here. The only thing keeping it quiet is gate
/// condition (4): the merged write-back binds `Rivet.quantity_produced` into
/// `resolved_params`.
///
/// So `assert_bound` is not decoration — it is the assertion that proves the O2
/// rule is what silences this, rather than an accidental early return. A step-15
/// that passed an empty or stale map for `bound_this_run` would raise an Error
/// on a published example, and this test is what catches it.
///
/// Read from the published file rather than mirrored inline, following
/// `joint_drive_expansion_boundary.rs`'s BT-5 idiom, so the canary degrades
/// loudly if the example itself drifts.
#[test]
fn joint_drive_example_stays_quiet_because_its_auto_is_bound() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/whole_model_joint_drive.ri"
    ))
    .expect("examples/whole_model_joint_drive.ri must be readable");

    let (engine, result) = eval_with_solver_keeping_engine(&source);
    assert_merged_cluster_spanned(&engine, &["Rivet", "RivetedPanel"]);
    assert_bound(&result, "Rivet", "quantity_produced");
    assert_no_unconsumed(
        &result,
        "the merged write-back bound every objective-reachable auto this run — \
         the vacuous-healthy rule (O2) is what silences this, and it must keep \
         silencing it once the merged site is wired",
    );
}

/// (O1) The second existing objective-bearing merged fixture — the same shape
/// carrying a per-sub parameter override (`joint_drive_expansion_boundary.rs`'s
/// `OVERRIDE_SRC`) — stays byte-quiet too.
///
/// An override changes which value the instance-path cost cell folds to, not
/// whether the auto is bound, so it must not perturb the rule. Pinning it
/// separately keeps a step-15 that accidentally keyed off the folded objective
/// VALUE (rather than the binding of the auto) from passing the canary above.
#[test]
fn merged_cluster_with_sub_override_stays_quiet() {
    let source = r#"
module dic_merged_override

structure def Rivet : Costed {
    param supplier          : String = "Acme Fastener"
    param part_number       : String = "R-4210"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h

    param quantity_produced : Real   = auto(free)
    constraint quantity_produced >= 0.0
    constraint quantity_produced <= 100.0
}

structure RivetedPanel {
    sub rivets = Rivet(unit_cost: 0.90USD)

    minimize cost(self.descendants)
}
"#;
    let (engine, result) = eval_with_solver_keeping_engine(source);
    assert_merged_cluster_spanned(&engine, &["Rivet", "RivetedPanel"]);
    assert_bound(&result, "Rivet", "quantity_produced");
    assert_no_unconsumed(
        &result,
        "a per-sub override changes the folded cost, not whether the auto is \
         bound — this merged fixture must stay byte-quiet (O1)",
    );
}
