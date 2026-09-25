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

// ── Invariant V1: the auto-ful path is unperturbed ───────────────────────────
//
// Step-8 widened `solve_scopes`' filter, which changed WHICH structures join the
// single shared `realize_structures` sub-build: `all_refs` now also carries every
// zero-auto scope's operand structures, so `sub_module.templates.retain(...)`
// retains more and the sub-build mutates more engine state than before.
//
// That is a real change to a shared input, so V1 ("auto-ful scopes behave exactly
// as before") needs an explicit pin rather than an assumption. The argument that it
// holds is that `realize_structures` builds each structure STANDALONE in its own
// identity frame and `resolve_operands` looks datums up by `(structure, member)`,
// so adding structures adds map entries without altering existing ones — but that
// is reasoning, and this is the measurement.

/// The §1 `Bolt`/`Plate` structures plus an auto-ful `BoltPlate` scope, built from
/// the same self-contained primitives as `examples/geometric_relations/bolt_plate.ri`
/// and `relate_solve_e2e.rs`'s fixture.
const AUTOFUL_SOURCE: &str = r#"
structure Bolt {
    let shank = cylinder(3mm, 20mm)
    let shank_axis : Axis = shank.axis
    let seat = rectangle(12mm, 12mm)
    let seat_plane : Plane = seat.plane
}

structure Plate {
    let body = box(40mm, 40mm, 5mm)
    let hole = cylinder(3.2mm, 5mm)
    let hole_axis : Axis = hole.axis
    let top = rectangle(40mm, 40mm)
    let top_plane : Plane = top.plane
}

structure BoltPlate {
    sub bolt : Bolt at auto
    sub plate : Plate
    relate {
        concentric(bolt.shank_axis, plate.hole_axis)
        flush(bolt.seat_plane, plate.top_plane)
    }
}
"#;

/// A zero-auto relate scope appended to [`AUTOFUL_SOURCE`], reusing the SAME two
/// leaf structures — so the two scopes genuinely share the realization set and the
/// test measures interference rather than two independent builds sitting side by
/// side.
const ZERO_AUTO_COMPANION: &str = r#"
structure FixedPair {
    sub a : Bolt
    sub b : Plate
    relate {
        concentric(a.shank_axis, b.hole_axis)
    }
}
"#;

/// Run `solve_scopes` over `source` and return the named scope's solution.
fn solution_for(source: &str, scope_name: &str) -> reify_eval::relate_solve::RelateSolution {
    static_solution(source, scope_name)
}

/// **V1** — an auto-ful scope solves identically whether or not a zero-auto relate
/// scope shares the module (and therefore the shared realization sub-build).
///
/// The comparison is against the SAME scope compiled in a module that contains no
/// zero-auto scope at all, so the reference is the pre-step-8 behaviour rather than
/// a hand-copied constant that could drift.
///
/// `static_facts == None` on the auto-ful entry is asserted too: that `Option` is
/// the discriminator ζ (#5420) uses to tell a SOLVED scope from a
/// STATICALLY-VERIFIED one, and the two render as different ledger rows.
#[test]
fn autoful_scope_is_unperturbed_by_a_zero_auto_scope_in_the_same_module() {
    if skip_without_occt("autoful_scope_is_unperturbed_by_a_zero_auto_scope_in_the_same_module") {
        return;
    }

    let alone = solution_for(AUTOFUL_SOURCE, "BoltPlate");
    let with_companion = solution_for(
        &format!("{AUTOFUL_SOURCE}{ZERO_AUTO_COMPANION}"),
        "BoltPlate",
    );

    assert_eq!(
        with_companion.static_facts, None,
        "an auto-ful scope is SOLVED, not statically verified — `static_facts` must \
         stay `None` so ζ can tell the two apart"
    );
    assert!(
        with_companion.diagnostics.is_empty(),
        "the auto-ful solve must stay silent; got {:?}",
        with_companion
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect::<Vec<_>>()
    );

    // DOF accounting is exact integer codimension, so these compare exactly.
    assert_eq!(
        (
            with_companion.driving,
            with_companion.redundant,
            with_companion.spent,
            with_companion.free
        ),
        (alone.driving, alone.redundant, alone.spent, alone.free),
        "the DOF partition must not move when a zero-auto scope joins the shared \
         realization build"
    );
    assert_eq!(
        with_companion.driving, 2,
        "fixture guard: concentric + flush must both drive, or this test is \
         comparing two degenerate solves"
    );

    // And the solved placement itself.
    let pose_alone = alone
        .poses
        .get("bolt")
        .and_then(reify_constraints::relate_solve::pose_from_frame)
        .expect("the lone auto-ful module must place `bolt`");
    let pose_with = with_companion
        .poses
        .get("bolt")
        .and_then(reify_constraints::relate_solve::pose_from_frame)
        .expect("the auto-ful scope must still place `bolt` alongside a zero-auto scope");

    // The bound is the solver's own convergence rung: two runs of the same solve
    // agree to at least that, and anything coarser would not be a perturbation
    // test at all.
    let tol = reify_constraints::relate_solve::RelateTolerance::kernel_default()
        .solver_convergence();
    for axis in 0..3 {
        assert!(
            (pose_with.translation[axis] - pose_alone.translation[axis]).abs() <= tol,
            "translation axis {axis} moved by {} (tol {tol}): {:?} vs {:?}",
            (pose_with.translation[axis] - pose_alone.translation[axis]).abs(),
            pose_with.translation,
            pose_alone.translation
        );
        assert!(
            (pose_with.rotation[axis] - pose_alone.rotation[axis]).abs() <= tol,
            "rotation axis {axis} moved by {} (tol {tol}): {:?} vs {:?}",
            (pose_with.rotation[axis] - pose_alone.rotation[axis]).abs(),
            pose_with.rotation,
            pose_alone.rotation
        );
    }
}

/// The companion half of the same measurement: the zero-auto scope in that shared
/// module is itself verified correctly, so the co-presence is exercised in BOTH
/// directions rather than only from the auto-ful side.
///
/// `FixedPair`'s two subs realize the SAME leaf structures the auto-ful scope uses,
/// and their local datums are both at the origin (neither leaf translates), so the
/// relation genuinely holds and the scope is silent.
#[test]
fn zero_auto_scope_is_verified_alongside_an_autoful_scope() {
    if skip_without_occt("zero_auto_scope_is_verified_alongside_an_autoful_scope") {
        return;
    }

    let solution = solution_for(
        &format!("{AUTOFUL_SOURCE}{ZERO_AUTO_COMPANION}"),
        "FixedPair",
    );

    assert_eq!(
        solution.static_facts,
        Some(reify_eval::relate_solve::StaticRelateFacts {
            verified: 1,
            violated: 0,
            unverifiable: 0,
        }),
        "the zero-auto scope must be MEASURED (not skipped, not unverifiable) even \
         when it shares a module and a realization build with an auto-ful scope; \
         got diagnostics {:?}",
        solution
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect::<Vec<_>>()
    );
    assert!(solution.diagnostics.is_empty());
    assert!(
        solution.poses.is_empty(),
        "a zero-auto scope determines no placement"
    );
}

// ── Invariant V3: consumption facts reach the build surface ──────────────────
//
// `verify_static_scope` produces `StaticRelateFacts` on the returned
// `RelateSolution`, but `build_with_geometry_output` DROPS `relate_solutions`
// after its consumption loop — so the facts exist and are unreachable from a
// completed `Engine::build`. That build is the surface ζ (#5420)'s `finish_check`
// ledger reads, so without an accessor the facts are produced for nobody.
//
// This task only PRODUCES the rows; rendering them into the check summary is
// #5420's leaf.

/// A module with a relate block but no geometry output, and no zero-auto scope.
const RELATE_FREE_SOURCE: &str = r#"
structure Lonely {
    let body = box(10mm, 10mm, 10mm)
}
"#;

/// Build `source` on a fresh OCCT engine and return the engine, so the caller can
/// interrogate its per-build state.
fn build_and_keep_engine(source: &str) -> reify_eval::Engine {
    let module = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = occt_engine();
    let _ = engine.build(&module, reify_ir::ExportFormat::Step);
    engine
}

/// **V3** — a completed build exposes one consumption row per zero-auto relate
/// scope, for both the violated and the satisfied fixture.
///
/// The satisfied case is the one that matters most: it emits NO diagnostic, so the
/// ledger row is the ONLY evidence the relate block was consumed at all. Without
/// it, "verified 2" and "there was no relate block" are indistinguishable
/// downstream — which is the same conflation, one layer up, that this whole task
/// exists to remove.
#[test]
fn build_exposes_static_relate_facts_per_zero_auto_scope() {
    if skip_without_occt("build_exposes_static_relate_facts_per_zero_auto_scope") {
        return;
    }

    let violated = build_and_keep_engine(violated_fixture_source());
    assert_eq!(
        violated.relate_static_facts(),
        &[(
            "DicRelateViolated".to_string(),
            reify_eval::relate_solve::StaticRelateFacts {
                verified: 0,
                violated: 2,
                unverifiable: 0,
            }
        )][..],
        "the violated fixture must contribute exactly one ledger row"
    );

    let ok = build_and_keep_engine(ok_fixture_source());
    assert_eq!(
        ok.relate_static_facts(),
        &[(
            "DicRelateOk".to_string(),
            reify_eval::relate_solve::StaticRelateFacts {
                verified: 2,
                violated: 0,
                unverifiable: 0,
            }
        )][..],
        "a SATISFIED zero-auto scope emits no diagnostic, so this row is the only \
         evidence the relate block was consumed — without it, `verified: 2` and \
         `no relate block` are indistinguishable downstream"
    );
}

/// **V3** — an auto-ful relate scope contributes NO row, and neither does a module
/// with no relate block at all.
///
/// A solved scope is not a statically-verified one: ζ renders those as separate
/// ledger rows, and folding them together here would misreport an assembly that
/// was actually placed by the solver as one that was merely checked in place.
#[test]
fn build_reports_no_static_facts_for_autoful_or_relate_free_modules() {
    if skip_without_occt("build_reports_no_static_facts_for_autoful_or_relate_free_modules") {
        return;
    }

    let autoful = build_and_keep_engine(AUTOFUL_SOURCE);
    assert!(
        autoful.relate_static_facts().is_empty(),
        "an auto-ful scope is SOLVED, not statically verified — it must not appear \
         in the static ledger; got {:?}",
        autoful.relate_static_facts()
    );

    let none = build_and_keep_engine(RELATE_FREE_SOURCE);
    assert!(
        none.relate_static_facts().is_empty(),
        "a module with no relate block contributes no rows; got {:?}",
        none.relate_static_facts()
    );
}

/// **V3** — the rows are PER-BUILD and do not accumulate across builds on the same
/// engine.
///
/// A `Vec` on a long-lived engine that is pushed to but never cleared would grow
/// without bound and, worse, would report a PREVIOUS module's relate scopes as if
/// they belonged to the current one. Two consecutive builds on one engine — first
/// a zero-auto relate module, then a relate-free one — must leave the accessor
/// EMPTY, which is only true if the field is reset per build.
#[test]
fn static_relate_facts_do_not_accumulate_across_builds() {
    if skip_without_occt("static_relate_facts_do_not_accumulate_across_builds") {
        return;
    }

    let with_relate = reify_test_support::parse_and_compile_with_stdlib(ok_fixture_source());
    let without_relate = reify_test_support::parse_and_compile_with_stdlib(RELATE_FREE_SOURCE);
    let mut engine = occt_engine();

    let _ = engine.build(&with_relate, reify_ir::ExportFormat::Step);
    assert_eq!(
        engine.relate_static_facts().len(),
        1,
        "fixture guard: the first build must actually produce a row, or the \
         non-accumulation assertion below is vacuous"
    );

    let _ = engine.build(&without_relate, reify_ir::ExportFormat::Step);
    assert!(
        engine.relate_static_facts().is_empty(),
        "rows must be reset per build — a stale row here would report the PREVIOUS \
         module's relate scope as belonging to this one; got {:?}",
        engine.relate_static_facts()
    );
}

/// **V3** — a tessellate surface after the build must LEAVE the ledger STANDING.
///
/// `reify check` runs `tessellate_realizations` after `realize_for_check` on any
/// module carrying a `RepresentationWithin` rule
/// (`crates/reify-cli/src/main.rs`), and `tessellate_realizations` resets per-build
/// engine state. While the ledger was classified reset-on-EVERY-surface, that
/// second surface silently emptied it — and since tessellate runs no relate-solve,
/// nothing refilled it. ζ (#5420) would then read zero rows and report "no relate
/// block" for a module that has one; for a SATISFIED scope, which raises no
/// diagnostic at all, the row is the ONLY evidence of consumption, so the wipe is
/// indistinguishable downstream from the false green this whole task removes.
///
/// `static_relate_facts_do_not_accumulate_across_builds` covers build→build; this
/// is the build→tessellate leg it cannot see.
#[test]
fn static_relate_facts_survive_a_tessellate_after_the_build() {
    if skip_without_occt("static_relate_facts_survive_a_tessellate_after_the_build") {
        return;
    }

    let module = reify_test_support::parse_and_compile_with_stdlib(ok_fixture_source());
    let mut engine = occt_engine();
    let _ = engine.build(&module, reify_ir::ExportFormat::Step);

    let after_build = engine.relate_static_facts().to_vec();
    assert_eq!(
        after_build.len(),
        1,
        "fixture guard: the build must produce a row, or the assertion below is \
         vacuous"
    );

    engine.tessellate_realizations(&module);

    assert_eq!(
        engine.relate_static_facts(),
        &after_build[..],
        "a tessellate surface runs no relate-solve, so it must leave the build's \
         ledger exactly as it found it — clearing it here reads downstream as \
         `this module has no relate block`"
    );
}

// ── Nested zero-auto scopes: the sub-build's own recursion ─────────────────
//
// Widening the `solve_scopes` filter to "≥1 relation" also widened what the shared
// `realize_structures` sub-build can recurse into: that sub-build calls
// `engine.build(&sub_module, …)`, whose own `solve_scopes` now processes a retained
// operand structure that declares a relate block with NO auto subs — a case the old
// filter dropped. The doc used to assert this away ("ζ's leaf structures carry no
// relations"), which nothing in the compiler enforces. Measured here instead, the
// same way V1 measures the sharing claim rather than arguing it.
//
// Filtering relations OUT of `sub_module` to make single-level recursion structural
// is the WRONG repair, and this fixture is why: `Carrier` is simultaneously an
// operand structure (the outer scope reads its `body_axis`) and a relate-declaring
// one. Dropping it would leave the outer operand unrealized, turning a decidable
// scope unverifiable — trading a cost concern for a verdict regression.

/// An operand structure that itself declares a ZERO-AUTO relate block.
///
/// `Carrier` is referenced by `NestedOuter`'s relation AND carries its own, so
/// realizing the outer scope's operands builds a sub-module that still contains a
/// relate scope. Both scopes are geometrically FALSE (7 mm inside, 12 mm outside),
/// so a lost or duplicated verdict is visible as a count, not as silence.
const NESTED_ZERO_AUTO_SOURCE: &str = r#"
structure Pin {
    let shaft = cylinder(2mm, 8mm)
    let shaft_axis : Axis = shaft.axis
}

structure OffsetPin {
    let shaft = translate(cylinder(2mm, 8mm), 7mm, 0mm, 0mm)
    let shaft_axis : Axis = shaft.axis
}

structure Carrier {
    sub near : Pin
    sub far : OffsetPin

    let body = cylinder(6mm, 10mm)
    let body_axis : Axis = body.axis

    relate {
        concentric(near.shaft_axis, far.shaft_axis)
    }
}

structure Host {
    let bore = translate(cylinder(6mm, 10mm), 12mm, 0mm, 0mm)
    let bore_axis : Axis = bore.axis
}

structure NestedOuter {
    sub carrier : Carrier
    sub host : Host

    relate {
        concentric(carrier.body_axis, host.bore_axis)
    }
}
"#;

/// A zero-auto relate scope inside an OPERAND structure is verified exactly ONCE,
/// by the outer pass — never lost to the discarded sub-build, never doubled.
///
/// `realize_structures` throws away the nested build's diagnostics
/// (`engine.build(…).values`), so a verdict reached only there would vanish. It
/// does not: `solve_scopes` walks every template in the module, so `Carrier`'s
/// scope is processed by the OUTER pass in its own right. That is what makes the
/// nested build's duplicate work redundant rather than load-bearing — and it is
/// the property that would break if a future edit scoped the walk to the root
/// template.
#[test]
fn a_nested_zero_auto_scope_is_verified_once_by_the_outer_pass() {
    if skip_without_occt("a_nested_zero_auto_scope_is_verified_once_by_the_outer_pass") {
        return;
    }

    let module = reify_test_support::parse_and_compile_with_stdlib(NESTED_ZERO_AUTO_SOURCE);
    let mut engine = occt_engine();
    let solved = reify_eval::relate_solve::solve_scopes(&module, &mut engine);

    let names: Vec<&str> = solved.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["Carrier", "NestedOuter"],
        "both relate-declaring templates are processed, in declaration order — the \
         nested one exactly once"
    );

    for (name, solution) in &solved {
        let errors = relate_static_diagnostics(&solution.diagnostics);
        assert_eq!(
            errors.len(),
            1,
            "`{name}` declares one violated relation, so it renders ONE aggregate; \
             got {:?}",
            solution
                .diagnostics
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            solution.static_facts,
            Some(reify_eval::relate_solve::StaticRelateFacts {
                verified: 0,
                violated: 1,
                unverifiable: 0,
            }),
            "`{name}`: the nested sub-build must neither swallow a verdict nor add one"
        );
    }
}

/// The same module through a completed `Engine::build`: exactly TWO ledger rows,
/// and the nested sub-build's own rows do not survive into them.
///
/// The sub-build is a full `engine.build`, so it clears and repopulates the ledger
/// mid-flight. The outer build's reset runs AFTER `solve_scopes` returns and BEFORE
/// its consumption loop, which is what keeps the nested rows from leaking into the
/// outer ledger — an ordering the accessor's "one row per zero-auto scope of THIS
/// module" contract depends on.
#[test]
fn a_nested_zero_auto_module_reports_one_ledger_row_per_scope() {
    if skip_without_occt("a_nested_zero_auto_module_reports_one_ledger_row_per_scope") {
        return;
    }

    let engine = build_and_keep_engine(NESTED_ZERO_AUTO_SOURCE);
    let rows = engine.relate_static_facts();
    let violated_one = reify_eval::relate_solve::StaticRelateFacts {
        verified: 0,
        violated: 1,
        unverifiable: 0,
    };

    assert_eq!(
        rows,
        &[
            ("Carrier".to_string(), violated_one),
            ("NestedOuter".to_string(), violated_one),
        ][..],
        "one row per zero-auto scope of THIS module, in declaration order — no \
         nested-build leftovers, no missing nested scope"
    );
}
