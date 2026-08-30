//! Task #5415 (DIC α): end-to-end acceptance for the ZERO-AUTO relate static-
//! verification arm of `solve_scopes` — the arm that kills the false-green
//! relate no-op.
//!
//! Registered from `harness_engine.rs` with an explicit `#[path]` — see the
//! anti-re-accretion rationale there.
//!
//! ## What is being pinned
//!
//! A `relate { }` block whose subs are ALL fixed (no `at auto`) has nothing to
//! solve for, and was therefore skipped outright: `solve_scopes` filtered on
//! `!auto_unknowns.is_empty() && !relations.is_empty()`. The consequence,
//! measured on this branch before the fix, is that a geometrically FALSE relate
//! block is a total silent no-op — `reify eval` says nothing and `reify check`
//! prints "All constraints satisfied." That is the declared-intent
//! non-consumption INV-SF-3 forbids
//! (`docs/legibility/design-invariants.md`;
//! `docs/prds/v0_6/declared-intent-consumption-accounting.md` §3 decision 1).
//!
//! The two acceptance bounds this module carries:
//!
//! - **B1** — `dic_relate_static_violated.ri` must emit the aggregated
//!   `RelateStaticViolated` Error, and the build/eval must FAIL.
//! - **B2** — `dic_relate_static_ok.ri`, the same shape with the datums
//!   colocated, must stay SILENT and pass. A satisfied static relate block emits
//!   nothing: the dropped `W_RELATE_NO_AUTO` of the placement-relations belt's δ
//!   leaf (which would have warned even when the assertion holds) was superseded
//!   by this task at decompose, ratified 2026-07-25.
//!
//! ## Assertions match on `DiagnosticCode`, never message substrings
//!
//! The house rule stated in the `DiagnosticCode` doc header. The codes are the
//! stable contract; the message wording is not.
//!
//! ## Signal is read through eval/build, NOT `reify check`
//!
//! `check_fails` (`crates/reify-cli/src/main.rs`) keys only on
//! `ConstraintOutcome` and ignores `Severity::Error` — gating `check`'s exit on
//! Error severity is task #5403's leaf, not this one. `cmd_eval` and
//! `build_is_success` DO gate on Error, which is what PRD B1's "reify eval/build
//! emits the geometric violation Error" names. Asserting through `check` here
//! would pin the wrong layer and would fail for a reason that is not this task's.
//!
//! Guarded on [`reify_kernel_occt::OCCT_AVAILABLE`] (the runtime flag) because
//! downstream crates cannot see `reify-kernel-occt`'s compile-time `has_occt`
//! cfg. Static verification compares REALIZED datums, so it needs a kernel; the
//! no-kernel behaviour of the caller gate is deliberately out of scope.

/// Workspace root, two levels above `crates/reify-eval`.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf()
}

/// Repo-relative path of the PRD's VIOLATED fixture (B1). Named so the panic in
/// [`fixture_source`] and this module's header cannot drift apart from the
/// actual `read_to_string`.
const VIOLATED_FIXTURE_PATH: &str = "docs/prds/v0_6/fixtures/dic_relate_static_violated.ri";

/// Repo-relative path of the PRD's SATISFIED fixture (B2).
const OK_FIXTURE_PATH: &str = "docs/prds/v0_6/fixtures/dic_relate_static_ok.ri";

/// Read one of this module's PRD fixtures from disk.
///
/// # The coupling to `docs/` is ONE-DIRECTIONAL and ungated
///
/// Binding these tests to the PRD's literal artifacts is deliberate — the
/// fixtures were authored as the PRD's own INV-SF-3 probe and this is the first
/// test to consume them, so until now nothing in the tree would have caught a
/// rename. But the gate is asymmetric: per CLAUDE.md, docs-only changes land via
/// a `hooks/pre-commit` run on `main` that scopes `docs/` to no-heavy-checks, so
/// an edit, rename or move of either `.ri` can land WITHOUT the Rust suite ever
/// running. The next unrelated build then fails here, in a crate that has
/// nothing to do with the change that broke it.
///
/// This is the same standing gap `let_tracing_transitive_e2e.rs` documents for
/// `discrete_let_cont.ri`, with the same two sanctioned repairs (relocate under
/// `crates/reify-eval/tests/fixtures/` and point the PRD at the new location, or
/// add the path to `scripts/verify-pipeline-paths.txt`) and the same reason not
/// to half-do it here: both edit files outside this task's lock set, and a
/// relocation that leaves a copy behind is strictly WORSE than today because it
/// reintroduces the stale-paraphrase failure the disk read exists to prevent.
///
/// What IS done is making the failure self-describing: the panic names the
/// missing path, the sanctioned repairs, and the fact that a docs-only commit is
/// the likely cause.
///
/// # MEMOIZED
///
/// Several tests need each source, and the read is a syscall plus a full file
/// decode. `OnceLock` makes it once per suite run rather than once per test,
/// and — more usefully — guarantees every test sees the SAME bytes even if the
/// file is rewritten mid-run, so a fixture edit can never make two tests
/// disagree about what they are pinning.
fn fixture_source(rel_path: &'static str, cache: &'static std::sync::OnceLock<String>) -> &'static str {
    cache.get_or_init(|| {
        let path = workspace_root().join(rel_path);
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "the DIC α fixture must be readable at {} ({e}).\n\
                 This test reads `{rel_path}` from disk on purpose, so that it \
                 tracks the PRD's literal artifact instead of pinning a stale \
                 paraphrase. Nothing in the docs-only landing path runs the Rust \
                 suite, so a rename/move/delete of that file lands green and \
                 surfaces HERE. Repair by restoring the path, or — if the move \
                 was intended — relocate the fixture under \
                 `crates/reify-eval/tests/fixtures/`, point \
                 `docs/prds/v0_6/declared-intent-consumption-accounting.md` at \
                 the new location, and update the path constant. Do NOT paper \
                 over it by inlining a copy of the source: the whole point of \
                 this read is that the PRD and the acceptance test cannot \
                 diverge.",
                path.display()
            )
        })
    })
}

/// The B1 fixture source: a zero-auto relate block that is geometrically FALSE
/// at the subs' fixed placements.
fn violated_fixture_source() -> &'static str {
    static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    fixture_source(VIOLATED_FIXTURE_PATH, &SOURCE)
}

/// The B2 fixture source: the same shape, colocated datums, TRUE at the fixed
/// placements.
fn ok_fixture_source() -> &'static str {
    static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    fixture_source(OK_FIXTURE_PATH, &SOURCE)
}

/// An `Engine` wired to the real OCCT kernel and a mock constraint checker.
///
/// The kernel is REQUIRED, not incidental: static verification compares realized
/// datums, and datum realization is a kernel query. The mock constraint checker
/// is correct here because these fixtures declare no `constraint` members — the
/// signal under test is the relate arm's diagnostics, and a real solver would add
/// nothing but runtime.
fn occt_engine() -> reify_eval::Engine {
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(
        Box::new(reify_test_support::MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    )
}

/// The OCCT availability gate every test in this module opens with.
///
/// Returns `true` when the caller must bail out. Factored into one place so the
/// skip message stays uniform and a new test cannot silently forget the gate and
/// then fail on a kernel-less runner.
fn skip_without_occt(test_name: &str) -> bool {
    if reify_kernel_occt::OCCT_AVAILABLE {
        return false;
    }
    eprintln!("skipping {test_name} (DIC α static relate verification): OCCT not available");
    true
}

/// The relate-family codes this arm owns. A build emits plenty of unrelated
/// diagnostics, so every assertion below filters to these two rather than to the
/// whole list — and matches on the CODE, never on message text (the house rule
/// stated in `DiagnosticCode`'s doc header).
fn relate_static_diagnostics(
    diagnostics: &[reify_core::Diagnostic],
) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                Some(reify_core::DiagnosticCode::RelateStaticViolated)
                    | Some(reify_core::DiagnosticCode::RelateStaticUnverifiable)
            )
        })
        .collect()
}

/// Run the build-pass relate entry over a fixture, returning the one solution for
/// `scope_name`.
///
/// Panics with the full list of scopes `solve_scopes` DID return on a miss — the
/// pre-fix failure mode is an empty Vec (the scope is filtered out before any
/// realization), and a bare `unwrap` there would say nothing about why.
fn static_solution(
    source: &str,
    scope_name: &str,
) -> reify_eval::relate_solve::RelateSolution {
    let module = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = occt_engine();
    let solved = reify_eval::relate_solve::solve_scopes(&module, &mut engine);
    solved
        .into_iter()
        .find(|(name, _)| name == scope_name)
        .unwrap_or_else(|| {
            panic!(
                "`solve_scopes` must return a solution for the zero-auto scope \
                 `{scope_name}`. An absent entry is the pre-fix behaviour: the \
                 scope is dropped by the auto-unknowns filter before any \
                 realization happens, which is exactly the silent no-op this \
                 task exists to kill."
            )
        })
        .1
}

/// **B1** — the PRD's deliberately-violated fixture emits ONE aggregated Error
/// naming both relations, and decides both as violated.
///
/// Baseline probed on this branch before the fix: total silence, exit 0. The
/// fixture's two relations are false by construction — the bushing's datums sit at
/// the origin while the plate's sit at (30, 20, 5) mm — so `concentric` is off by
/// the 30 mm in-plane split and `flush` by the 5 mm along-normal offset, both
/// 500×–3000× the 1e-5 m assertion tolerance.
///
/// `poses` must stay empty: a zero-auto scope determines no placement, and that
/// emptiness is what lets the existing `engine_build.rs` consumption loop forward
/// the diagnostics without touching the pose/mount writeback.
#[test]
fn violated_fixture_emits_one_aggregated_static_relate_error() {
    if skip_without_occt("violated_fixture_emits_one_aggregated_static_relate_error") {
        return;
    }

    let solution = static_solution(violated_fixture_source(), "DicRelateViolated");

    let relate = relate_static_diagnostics(&solution.diagnostics);
    assert_eq!(
        relate.len(),
        1,
        "two violated relations must aggregate into ONE coded Error — coded \
         diagnostics bypass the CLI's dedup, so one per relation would reach the \
         user unfiltered. Got {:?}",
        solution
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.code, d.message.clone()))
            .collect::<Vec<_>>()
    );
    let d = relate[0];
    assert_eq!(d.severity, reify_core::Severity::Error);
    assert_eq!(
        d.code,
        Some(reify_core::DiagnosticCode::RelateStaticViolated)
    );
    assert!(
        d.message.contains("concentric") && d.message.contains("flush"),
        "the aggregate must name BOTH violated relations; got {:?}",
        d.message
    );

    assert_eq!(
        solution.static_facts,
        Some(reify_eval::relate_solve::StaticRelateFacts {
            verified: 0,
            violated: 2,
            unverifiable: 0,
        }),
        "both relations must be DECIDED and decided false — an `unverifiable` \
         here would mean the datums did not realize, a different defect"
    );
    assert!(
        solution.poses.is_empty(),
        "a zero-auto scope determines no placement"
    );
}

/// **B1, user-observable** — the same Error reaches `Engine::build`'s diagnostics,
/// so the arm genuinely surfaces through `engine_build.rs` rather than only in a
/// direct `solve_scopes` call.
///
/// Asserted through `build`, NOT `reify check`. `check_fails`
/// (`crates/reify-cli/src/main.rs`) keys only on `ConstraintOutcome` and ignores
/// `Severity::Error` — gating check's exit on Error severity is task #5403's leaf,
/// not this one. `cmd_eval` and `build_is_success` both DO fail on any Error,
/// which is what PRD B1's "reify eval/build emits the geometric violation Error"
/// names. Asserting through `check` here would pin the wrong layer and fail for a
/// reason that is not this task's.
#[test]
fn violated_fixture_surfaces_the_error_through_engine_build() {
    if skip_without_occt("violated_fixture_surfaces_the_error_through_engine_build") {
        return;
    }

    let module = reify_test_support::parse_and_compile_with_stdlib(violated_fixture_source());
    let mut engine = occt_engine();
    let result = engine.build(&module, reify_ir::ExportFormat::Step);

    let relate = relate_static_diagnostics(&result.diagnostics);
    assert_eq!(
        relate.len(),
        1,
        "the aggregated Error must reach the build's diagnostics; got {:?}",
        relate
            .iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(relate[0].severity, reify_core::Severity::Error);
    assert_eq!(
        relate[0].code,
        Some(reify_core::DiagnosticCode::RelateStaticViolated)
    );

    // The bound `build_is_success` / `cmd_eval` actually apply: any Error fails.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == reify_core::Severity::Error),
        "the build must carry an Error, which is what makes `reify build` / \
         `reify eval` exit non-zero on this fixture"
    );
}

/// **B2** — the satisfied companion fixture is COMPLETELY silent and reports both
/// relations verified.
///
/// The fixture's two structures are built from the identical `translate(...)`
/// expression, so `resolve_operands` — which keys realized datums by
/// `(structure, member)` — hands both operands bit-identical f64s and the residual
/// is exactly zero.
///
/// The silence is asserted over the WHOLE diagnostic list, not just its Errors.
/// The placement-relations belt's δ leaf would have emitted a `W_RELATE_NO_AUTO`
/// warning on every zero-auto relate block regardless of whether the assertion
/// held; that leaf was dropped at decompose and superseded by this task (ratified
/// 2026-07-25). This assertion is what fails if it is ever reintroduced.
#[test]
fn ok_fixture_is_silent_and_reports_both_relations_verified() {
    if skip_without_occt("ok_fixture_is_silent_and_reports_both_relations_verified") {
        return;
    }

    let solution = static_solution(ok_fixture_source(), "DicRelateOk");

    assert!(
        solution.diagnostics.is_empty(),
        "a satisfied zero-auto relate block must be SILENT — no error, no warning, \
         no info. Got {:?}. A warning here would be the dropped `W_RELATE_NO_AUTO` \
         resurfacing.",
        solution
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.code, d.message.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        solution.static_facts,
        Some(reify_eval::relate_solve::StaticRelateFacts {
            verified: 2,
            violated: 0,
            unverifiable: 0,
        }),
        "both relations must be measured and found satisfied — `verified: 2` is \
         what distinguishes a consumed relate block from an absent one, and is \
         the fact ζ's ledger reads"
    );

    // And the same silence through the user-observable build surface.
    let module = reify_test_support::parse_and_compile_with_stdlib(ok_fixture_source());
    let mut engine = occt_engine();
    let result = engine.build(&module, reify_ir::ExportFormat::Step);
    assert!(
        relate_static_diagnostics(&result.diagnostics).is_empty(),
        "no relate-static diagnostic of ANY severity may reach the build for a \
         satisfied fixture; got {:?}",
        relate_static_diagnostics(&result.diagnostics)
            .iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect::<Vec<_>>()
    );
}
