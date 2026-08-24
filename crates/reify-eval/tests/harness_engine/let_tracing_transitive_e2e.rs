//! Task #5467 (PRD2 α): end-to-end acceptance for let-tracing transitive
//! constraint↔auto closure.
//!
//! Registered from `harness_engine.rs` with an explicit `#[path]` — see the
//! anti-re-accretion rationale there.
//!
//! This is the user-observable LEAF SIGNAL for PRD2 α (§8, B1). The fixture is
//! read FROM DISK rather than paraphrased inline, so the test is bound to the
//! PRD's literal artifact: if `docs/prds/v0_6/fixtures/discrete_let_cont.ri`
//! changes, this test moves with it instead of silently pinning a stale copy.
//!
//! LOAD-BEARING — the solver MUST be `SolverRegistry::production()`, never a
//! bare `DimensionalSolver`, or the RED below is vacuous and pins nothing. That
//! argument now lives with the code it constrains, on
//! `underdetermined_support::eval_through_production_registry`, so a future
//! refactor of the wiring meets it there rather than in a header it may never
//! open.

use reify_core::ValueCellId;
use reify_eval::EvalResult;

// The production-registry eval and the two diagnostic readers are shared with
// the sibling `instance_path_underdetermined_e2e` module — same test binary,
// same three helpers (review suggestion 5). `SOLVER_TOL` deliberately stays
// local: it is derived from THIS fixture's cost surface, below.
use crate::underdetermined_support::{
    eval_through_production_registry, scalar_si, underdetermined,
};

/// Absolute tolerance for the resolved autos.
///
/// DERIVED, not observed. `DimensionalSolver` sums squared `Eq` residuals, so
/// this fixture's cost surface is exactly
/// `c(a,b) = (a+b−10)² + (a−b−2)²` — positive definite, unique global minimum
/// `c = 0` at (6,4), with `∇²c = 4·I`, i.e. `c ≈ 2(δa² + δb²)` near the root.
/// A `Solved` verdict requires `c ≤ FEASIBILITY_THRESHOLD = 1e-12`
/// (`crates/reify-constraints/src/solver.rs:14`), which forces
/// `|δa|, |δb| ≤ √(5e-13) ≈ 7.07e-7`. So 1e-6 is IMPLIED BY THE SOLVE
/// SUCCEEDING AT ALL, with ~30% headroom — it is not a threshold chosen to fit
/// an observed run.
///
/// If this ever fails, that is a CONVERGENCE signal to investigate or
/// escalate, NOT an invitation to widen the constant.
const SOLVER_TOL: f64 = 1e-6;

/// Workspace root, two levels above `crates/reify-eval`.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf()
}

/// Repo-relative path of the PRD α fixture. Named so the panic below and this
/// module's header cannot drift apart from the actual `read_to_string`.
const ALPHA_FIXTURE_PATH: &str = "docs/prds/v0_6/fixtures/discrete_let_cont.ri";

/// The PRD α fixture source, read from disk.
///
/// # The coupling to `docs/` is ONE-DIRECTIONAL and ungated
///
/// Binding this test to the PRD's literal artifact is deliberate (see the
/// module header), but the gate is asymmetric: per CLAUDE.md, docs-only changes
/// land via a `hooks/pre-commit` run on `main` that scopes `docs/` to
/// no-heavy-checks, so an edit, rename or move of this `.ri` can land WITHOUT
/// the Rust suite ever running. The next unrelated build then fails here, in a
/// crate that has nothing to do with the change that broke it.
///
/// Closing that properly means either relocating the fixture under
/// `crates/reify-eval/tests/fixtures/` (and having the PRD reference it) or
/// adding it to `scripts/verify-pipeline-paths.txt`. Re-raised by review and
/// re-deferred deliberately: BOTH repairs edit files outside this task's lock
/// set — the first needs `docs/prds/v0_6/discrete-cost-minimisation.md` and the
/// deletion of the `docs/` original, the second a verify-pipeline manifest —
/// and a half-done relocation that leaves a copy behind is strictly WORSE than
/// today, because it reintroduces exactly the stale-paraphrase failure the
/// disk read exists to prevent.
///
/// The deferral is no longer prose-only: it is FILED as backlog ticket
/// `tkt_0RSPTRG9VG11ABAA06RJV904VB` (spawned from task #5467), which carries
/// both sanctioned repairs, the exact file list, and the
/// do-not-leave-a-copy-behind constraint, so the follow-up is scheduled rather
/// than left for a reader of this comment to rediscover. Deliberately NOT
/// written as a `TODO` marker carrying a `#NNNN` cite: the curator assigns the
/// task id asynchronously, and the house PTODO grammar requires a cite to
/// resolve to a LIVE task — a placeholder would land as `malformed-cite` and
/// fail `reify-audit --pattern PTODO`. The spelling just used is deliberate
/// too: §8.1's marker grammar keys on the keyword being immediately followed
/// by `(` or `:`, so writing the marker form even in this explanation would
/// itself land as an `untracked` finding — please do not "fix" it back.
///
/// What IS done here is making the failure self-describing: the panic names the
/// missing path, the two sanctioned repairs, and the fact that a docs-only
/// commit is the likely cause, so whoever hits it does not have to re-derive
/// any of that.
///
/// # MEMOIZED
///
/// Both tests in this module need the source, and the read is a syscall plus a
/// full file decode. `OnceLock` makes it once per suite run rather than once
/// per test, and — more usefully — guarantees both tests see the SAME bytes
/// even if the file is rewritten mid-run, so a fixture edit can never make the
/// two tests disagree about what they are pinning.
fn alpha_fixture_source() -> &'static str {
    static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SOURCE.get_or_init(|| {
        let path = workspace_root().join(ALPHA_FIXTURE_PATH);
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "the PRD α fixture must be readable at {} ({e}).\n\
             This test reads `{ALPHA_FIXTURE_PATH}` from disk on purpose, so \
             that it tracks the PRD's literal artifact instead of pinning a \
             stale paraphrase. Nothing in the docs-only landing path runs the \
             Rust suite, so a rename/move/delete of that file lands green and \
             surfaces HERE. Repair by restoring the path, or — if the move was \
             intended — relocate the fixture under \
             `crates/reify-eval/tests/fixtures/`, point the PRD at the new \
             location, and update `ALPHA_FIXTURE_PATH`. Do NOT paper over it \
             by inlining a copy of the source: the whole point of this read is \
             that the PRD and the acceptance test cannot diverge.",
                path.display()
            )
        })
    })
}

/// Assert `DCM5.a == 6` and `DCM5.b == 4` within [`SOLVER_TOL`].
fn assert_resolves_to_six_and_four(result: &EvalResult, what: &str) {
    let a = scalar_si(result, &ValueCellId::new("DCM5", "a"), what);
    let b = scalar_si(result, &ValueCellId::new("DCM5", "b"), what);
    assert!(
        (a - 6.0).abs() < SOLVER_TOL,
        "{what}: `a + b == 10` and `a - b == 2` have the unique solution \
         a = 6, b = 4; got a = {a} (|Δ| = {})",
        (a - 6.0).abs(),
    );
    assert!(
        (b - 4.0).abs() < SOLVER_TOL,
        "{what}: `a + b == 10` and `a - b == 2` have the unique solution \
         a = 6, b = 4; got b = {b} (|Δ| = {})",
        (b - 4.0).abs(),
    );
}

/// B1 — the PRD's α leaf signal. Both autos are pinned ONLY through `let`s, so
/// every one-hop-blind layer must have been widened for this to resolve.
///
/// RED before layer 2 lands: `decompose_into_components` finds no auto ref in
/// either let-indirected constraint (`collect_value_refs ∩ param_index` is one
/// hop, and each constraint's ref set is `{DCM5.s}` / `{DCM5.d}`), returns an
/// EMPTY decomposition, and `solve_inner` reads that as "all auto params are
/// unconstrained" — returning `Solved { values: {} }` and leaving both autos
/// `Undef`.
#[test]
fn discrete_let_cont_fixture_resolves_both_autos_through_the_lets() {
    let result = eval_through_production_registry(alpha_fixture_source(), "discrete_let_cont.ri");

    assert_resolves_to_six_and_four(&result, "discrete_let_cont.ri");
}

/// D3 tripwire — the other half of the leaf signal. Probe-verified baseline
/// (release binary, 2026-07-24): TWO `W_UNDERDETERMINED` lines at exit 0.
///
/// GREEN on write: layer 4 (`detect_underdetermined`) already seeds a
/// transitive read-CLOSURE on this branch. This test is the regression LOCK —
/// the warning must never come back, and it lives beside B1 because a fix that
/// resolved a=6/b=4 while still printing two `W_UNDERDETERMINED` lines would
/// be half the leaf signal, silently.
#[test]
fn discrete_let_cont_fixture_emits_no_underdetermined_warning() {
    let result = eval_through_production_registry(alpha_fixture_source(), "discrete_let_cont.ri");

    let flagged = underdetermined(&result);
    assert!(
        flagged.is_empty(),
        "`constraint s == 10.0` / `constraint d == 2.0` pin BOTH autos through \
         the `let`s, so neither may be reported underdetermined; got \
         {flagged:#?}",
    );
}

/// B2/D1 companion — the DIRECT-formulation twin of the same model, with no
/// `let`s and therefore no dependent cells at all, through the SAME
/// `production()` registry.
///
/// This is boundary B2: the transitive widening must not perturb a model that
/// has nothing to widen. It must resolve to the same 6.0/4.0 within the same
/// tolerance and likewise emit no `Underdetermined`.
#[test]
fn the_direct_formulation_twin_is_unperturbed_by_the_widening() {
    const DIRECT: &str = r#"module discrete_direct_cont

structure DCM5 {
    param a : Real = auto
    param b : Real = auto
    constraint a + b == 10.0
    constraint a - b == 2.0
}
"#;

    let result = eval_through_production_registry(DIRECT, "direct-formulation twin");

    assert_resolves_to_six_and_four(&result, "direct-formulation twin");

    let flagged = underdetermined(&result);
    assert!(
        flagged.is_empty(),
        "the direct twin's constraints read both autos DIRECTLY, so neither \
         may be reported underdetermined — and the D1 identity branch must \
         leave this path byte-identical to pre-α; got {flagged:#?}",
    );
}
