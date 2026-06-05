//! Pin: the `pub fn` / `pub(crate) fn` producers landed since 2026-06-02 that
//! must each carry a `// G-allow:` marker.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test new_orphans_2026_06_02_g_allow`
//!
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! flags every `pub fn` whose only references are same-file / tests / `use` /
//! comments (zero external callers).  A `// G-allow: <reason>` comment on the
//! line immediately above the declaration moves it from the report's
//! "Orphan candidates" table to "Allow-listed".  This test asserts list
//! membership (absent from `orphans[]`, present exactly once in `allowed[]`).
//! The owning task number is single-sourced in the source `// G-allow:` marker;
//! this test deliberately does NOT inspect marker prose / carries no second copy
//! that could drift.  Neither assertion implies `orphan_count == 0`; 450+
//! pre-existing baseline orphans in unrelated files are intentionally not in scope
//! here.
//!
//! # Buckets
//!
//! All pins in this file are **Bucket 2 — tracked producer-before-consumer.**
//! The producer task has landed (DONE) but its consumer task is still PENDING,
//! so no in-tree caller exists yet.  These pins carry an AUTO-RETIREMENT
//! contract (see below).
//!
//! - `crates/reify-stdlib/src/trajectory/simulate.rs`
//!   (simulate_trajectory θ, task #3869, DONE; consumer π Value/ComputeNode
//!   trampoline, PENDING; 4 producers)
//!
//! - `crates/reify-stdlib/src/trajectory/tots.rs`
//!   (TOTS SQP optimizer κ, task #3870, DONE; consumer PENDING; 14 internal
//!   helpers exposed ahead of full consumer wiring)
//!
//! - `crates/reify-eval/src/trajectory_ops.rs`
//!   (robustness metric seam; deferred consumers #3869 θ/ι and #3870 κ; 1 fn)
//!
//! - `crates/reify-solver-elastic/src/prestress_stability.rs`
//!   (Tensegrity T2, task #3796, DONE; Type-A: crate-root re-exported but
//!   consumed ONLY by tests/tensegrity_t2_stability.rs — no production/DSL
//!   consumer yet; 5 fns)
//!
//! # Excluded functions (do NOT pin)
//!
//! - `solve_tots` (`tots.rs`) — already wired via `input_shape::run_tots`,
//!   has callers > 0, and is in NEITHER list; pinning it would fail assertion (b).
//! - `run_inverse_dynamics` (`dynamics_ops.rs`) — documented false positive
//!   (reachable ComputeFn), not a target file.
//!
//! # Wide-scope trade-off
//!
//! The audit runs at the wide `crates/reify-*/src` scope (same as the baseline
//! report).  All pinned names were verified collision-free at this scope when
//! the file was authored.  Before deleting any PINS row after an assertion-(b)
//! failure, confirm a real call edge was wired:
//! `rg '\bFN_NAME\b' crates/reify-*/src`.
//!
//! # Removal contract
//!
//! All pins are Bucket-2 and owned by the consumer task cited in each
//! function's source `// G-allow:` marker.  Once that task wires its consumer
//! the function gains a non-test caller, leaves `allowed[]`, and assertion (b)
//! auto-trips.  The owning consumer task MUST delete the corresponding
//! per-file `#[test]` fn (or its rows) as part of the consumer-wiring commit:
//!   - `simulate_producers`       — owned by consumer task #3869 (θ/π).
//!   - `tots_producers`           — owned by consumer task #3870 (κ).
//!   - `trajectory_ops_producer`  — owned by consumer tasks #3869 (θ/ι) and #3870 (κ).
//!   - `prestress_producers`      — owned by consumer task #3796 production wiring.
//!
//! The failure message lists every failing (file_suffix, fn_name) pair — search
//! for them in this file when `G-allow pin(s) failed` appears unexpectedly.
//!
//! Audit invocation: the wide-scope audit is run ONCE per test binary and
//! shared across all four per-file `#[test]` fns via `cached_audit()` (a
//! process-wide `OnceLock`) — the whole-corpus `python3` sweep fires a single
//! time, not once per pin.  Graceful skip: if `python3`, `git`, or the audit
//! script are absent from PATH/disk the test prints a note to stderr and
//! returns without failing.  The authoritative CI lane that owns this check
//! MUST set `REIFY_REQUIRE_ORPHAN_AUDIT=1`, which promotes that skip to a hard
//! failure so a dropped `// G-allow:` marker cannot hide on a minimal image.
//! The shared helper is `reify_test_support::run_orphan_audit`.

use std::sync::OnceLock;

use reify_test_support::run_orphan_audit;

/// The wide-scope orphan audit, run **once** per test binary and shared across
/// every per-file pin `#[test]`.
///
/// # Why cache
///
/// Every pin test interrogates the same single fact — the current
/// orphan/allowed classification of `crates/reify-*/src`. Calling
/// `run_orphan_audit` once per `#[test]` would re-scan the entire corpus (≈1800
/// `pub fn` across ≈390 files) four times for one logical query. A process-wide
/// `OnceLock` pays that cost a single time; Rust's parallel test runner blocks
/// the other test threads inside `get_or_init` until the first finishes, then
/// they all read the cached envelope.
///
/// # Authoritative-lane enforcement (`REIFY_REQUIRE_ORPHAN_AUDIT`)
///
/// `run_orphan_audit` returns `None` — a graceful skip — when `python3`, `git`,
/// or the audit script are absent. That keeps the suite green on minimal images
/// but means a dropped `// G-allow:` marker would go UNDETECTED there, with only
/// an easily-lost stderr note. The canonical CI lane that owns this check MUST
/// set `REIFY_REQUIRE_ORPHAN_AUDIT` (to any non-empty value other than `0`);
/// under that flag a missing-tooling skip is promoted to a hard panic so the
/// regression cannot hide. With the flag unset the graceful skip is preserved
/// for local/partial runs.
fn cached_audit() -> Option<&'static serde_json::Value> {
    static AUDIT: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let audit = AUDIT.get_or_init(|| run_orphan_audit("crates/reify-*/src"));
    if audit.is_none() {
        let required = std::env::var("REIFY_REQUIRE_ORPHAN_AUDIT")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        assert!(
            !required,
            "REIFY_REQUIRE_ORPHAN_AUDIT is set but the orphan audit could not run \
             (python3, git, or scripts/audit-orphan-producers.sh missing). This is \
             the authoritative G-allow-pin lane and must not skip silently — install \
             the tooling or unset the flag."
        );
    }
    audit.as_ref()
}

/// Shared assertion body for one slice of `(file_suffix, fn_name)` pins.
///
/// For each pin asserts:
///   (a) absent from `result["orphans"]` (match `file.ends_with(suffix)` &&
///       EXACT `name` — so distinct same-named fns in other files never collide);
///   (b) present EXACTLY ONCE in `result["allowed"]` (same match).
///
/// Collects all failures across the slice and panics once with a combined
/// message so a single run pinpoints every missing/misplaced marker.
fn assert_pins_are_g_allow_marked(result: &serde_json::Value, pins: &[(&str, &str)]) {
    let orphans = result["orphans"]
        .as_array()
        .expect("orphans must be an array");
    let allowed = result["allowed"]
        .as_array()
        .expect("allowed must be an array");

    let mut failures: Vec<String> = Vec::new();

    for &(file_suffix, fn_name) in pins {
        // (a) Must NOT appear in orphans[] for the given file.
        let in_orphans = orphans.iter().any(|entry| {
            entry["file"]
                .as_str()
                .map(|f| f.ends_with(file_suffix))
                .unwrap_or(false)
                && entry["name"].as_str() == Some(fn_name)
        });
        if in_orphans {
            failures.push(format!(
                "  FAIL (a) `{fn_name}` ({file_suffix}): still listed as an orphan — \
                 the `// G-allow:` marker may be missing or misplaced (must be on the \
                 line immediately above the declaration, between the `///` doc block \
                 and `pub fn`, with no blank line)."
            ));
        }

        // (b) Must appear EXACTLY ONCE in allowed[] for the given file.
        let matching_allowed: Vec<_> = allowed
            .iter()
            .filter(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(file_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            })
            .collect();

        if matching_allowed.len() != 1 {
            let n = matching_allowed.len();
            let detail = if n == 0 {
                format!(
                    "0 entries. Disambiguate via assertion (a) for `{fn_name}`: if (a) \
                     ALSO failed, the `// G-allow:` marker is simply missing/misplaced — \
                     add it on the line immediately above the declaration. If (a) \
                     PASSED, the fn left BOTH lists, which means either a real consumer \
                     call was wired (delete this pin per the removal contract) OR a \
                     same-name token elsewhere in `crates/reify-*/src` pushed callers > 0 \
                     (incidental collision); run `rg '\\b{fn_name}\\b' crates/reify-*/src` \
                     to distinguish before removing the row."
                )
            } else {
                format!("{n} entries — unexpected duplicate `// G-allow:` markers")
            };
            failures.push(format!(
                "  FAIL (b) `{fn_name}` ({file_suffix}): expected exactly 1 entry in \
                 allowed[]; {detail}"
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} G-allow pin(s) failed:\n{}\n\n\
             PINS file: `crates/reify-audit/tests/new_orphans_2026_06_02_g_allow.rs`\n\
             Full orphans list:\n{:#}\n\
             Full allowed list:\n{:#}",
            failures.len(),
            failures.join("\n"),
            result["orphans"],
            result["allowed"],
        );
    }
}

/// Bucket 2 — `simulate_trajectory` forward-pass producers
/// (`crates/reify-stdlib/src/trajectory/simulate.rs`).
///
/// These 4 `pub(crate) fn` are internal helpers for the simulate_trajectory
/// pipeline (task #3869 θ, DONE). The π Value/ComputeNode trampoline consumer
/// is PENDING, so no in-tree caller exists yet.  Owned by consumer task #3869.
#[test]
fn simulate_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        (
            "crates/reify-stdlib/src/trajectory/simulate.rs",
            "modal_aware_dt",
        ),
        (
            "crates/reify-stdlib/src/trajectory/simulate.rs",
            "nominal_fk_chain",
        ),
        (
            "crates/reify-stdlib/src/trajectory/simulate.rs",
            "superpose_modes",
        ),
        (
            "crates/reify-stdlib/src/trajectory/simulate.rs",
            "forces_to_forcing_history",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 2 — TOTS SQP optimizer internal helpers
/// (`crates/reify-stdlib/src/trajectory/tots.rs`).
///
/// These 14 `pub(crate) fn` are internal helpers for the TOTS SQP optimizer
/// (task #3870 κ, DONE).  The consumer task is still PENDING, so no in-tree
/// caller exists yet.  Owned by consumer task #3870.
///
/// NOTE: `solve_tots` is DELIBERATELY EXCLUDED — it is already wired via
/// `input_shape::run_tots` and has callers > 0; it appears in NEITHER list,
/// and pinning it would fail assertion (b).
#[test]
fn tots_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        ("crates/reify-stdlib/src/trajectory/tots.rs", "n_vars"),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "variable_vector",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "unpack_variable_vector",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "build_spline",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "constraint_violations",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "is_feasible",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "max_violation",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "objective_gradient",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "constraint_jacobian",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "bfgs_update",
        ),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "solve_qp_step",
        ),
        ("crates/reify-stdlib/src/trajectory/tots.rs", "merit"),
        (
            "crates/reify-stdlib/src/trajectory/tots.rs",
            "line_search",
        ),
        ("crates/reify-stdlib/src/trajectory/tots.rs", "code_str"),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 2 — trajectory robustness metric seam
/// (`crates/reify-eval/src/trajectory_ops.rs`).
///
/// `worst_case_residual_fraction` is an engine-side seam exposed ahead of its
/// consumers (simulate_trajectory θ/ι, task #3869; TOTS κ, task #3870).
/// Currently exercised only by in-module unit tests.  Owned by consumer tasks
/// #3869 and #3870.
#[test]
fn trajectory_ops_producer_is_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[(
        "crates/reify-eval/src/trajectory_ops.rs",
        "worst_case_residual_fraction",
    )];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 2 — Tensegrity T2 prestress stability producers
/// (`crates/reify-solver-elastic/src/prestress_stability.rs`).
///
/// These 5 `pub fn` / `pub(crate) fn` are the stability-analysis API landed
/// under task #3796 (Tensegrity T2, DONE).  Type-A: crate-root re-exported but
/// consumed ONLY by `tests/tensegrity_t2_stability.rs` — no production/DSL
/// consumer yet.  Owned by the production wiring task for #3796.
#[test]
fn prestress_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        (
            "crates/reify-solver-elastic/src/prestress_stability.rs",
            "analyze_prestress_stability",
        ),
        (
            "crates/reify-solver-elastic/src/prestress_stability.rs",
            "count_self_stress_states",
        ),
        (
            "crates/reify-solver-elastic/src/prestress_stability.rs",
            "assemble_equilibrium_matrix",
        ),
        (
            "crates/reify-solver-elastic/src/prestress_stability.rs",
            "extract_internal_mechanisms",
        ),
        (
            "crates/reify-solver-elastic/src/prestress_stability.rs",
            "assemble_geometric_stiffness",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}
