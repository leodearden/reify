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
