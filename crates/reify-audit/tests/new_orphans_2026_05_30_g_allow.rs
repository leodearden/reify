//! Pin: the `pub fn` / `pub(crate) fn` producers landed since 2026-05-30 that
//! must each carry a `// G-allow:` marker citing a tracked task as `#NNNN`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test new_orphans_2026_05_30_g_allow`
//!
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! flags every `pub fn` whose only references are same-file / tests / `use` /
//! comments (zero external callers).  A `// G-allow: <reason>` comment on the
//! line immediately above the declaration moves it from the report's
//! "Orphan candidates" table to "Allow-listed".  This test asserts list
//! membership (absent from `orphans[]`, present exactly once in `allowed[]`)
//! and that the reason cites *some* tracked owner task as `#NNNN`.  The specific
//! task number lives only in the source `// G-allow:` marker (single source of
//! truth), so this test never carries a second copy that could drift.  Neither
//! assertion implies `orphan_count == 0`; 450+ pre-existing baseline orphans in
//! unrelated files are intentionally not in scope here.
//!
//! # Two buckets
//!
//! **Bucket 1 — fn-pointer ComputeFn registration blind spot.** These
//! producers are reached only via fn pointers registered in
//! `compute_targets::register_compute_fns` (mod.rs).  The audit walks textual
//! call edges and cannot trace an fn-pointer hand-off, so the same-file callees
//! of a registered trampoline read as zero-caller orphans even though they are
//! live and tested.  Bucket-1 pins are PERMANENT: no consumer task will ever
//! wire a *direct* textual caller, so the markers stay forever.
//!   - `crates/reify-eval/src/modal_ops.rs` (modal::free_vibration pipeline)
//!   - `crates/reify-eval/src/compute_targets/elastic_static.rs`
//!   - `crates/reify-solver-elastic/src/elements/degenerate_shell.rs`
//!   - `crates/reify-solver-elastic/src/shell_assembly.rs`
//!
//! **Bucket 2 — tracked producer-before-consumer.** The producer task has
//! landed (DONE) but its consumer task is still PENDING, so no in-tree caller
//! exists yet.  These pins carry an AUTO-RETIREMENT contract (see below).
//!   - `crates/reify-stdlib/src/trajectory/impulse_shaper.rs` (consumer #3867)
//!   - `crates/reify-stdlib/src/trajectory/sampling.rs` (consumer #3869)
//!
//! # Wide-scope trade-off
//!
//! The audit runs at the wide `crates/reify-*/src` scope (same as the baseline
//! report), so any name-token occurrence of a pinned function name elsewhere in
//! that scope — e.g. a local `let mat3_inverse = ...` in another crate — can
//! push `callers > 0`, silently removing the function from `allowed[]` and
//! tripping assertion (b) without a real consumer being wired.  All pinned
//! names were verified collision-free at this scope when the file was authored
//! (live 471 orphan / 49 allowed).  Two functions originally in scope —
//! `in_plane` (degenerate_shell.rs) and `local_to_global` (shell_assembly.rs) —
//! were DROPPED for exactly this reason: `in_plane` collides with a
//! `let in_plane = ...` local elsewhere (false caller), and `local_to_global`
//! acquired genuine cross-file callers; both are now in NEITHER list, so a pin
//! would fail assertion (b).  Before deleting any PINS row after an
//! assertion-(b) failure, confirm a real call edge was wired:
//! `rg '\bFN_NAME\b' crates/reify-*/src`.
//!
//! # Removal contract
//!
//! Bucket-1 (fn-pointer) pins are permanent — leave them in place.
//!
//! Bucket-2 pins are owned by the consumer task cited in each function's source
//! `// G-allow:` marker.  Once that task wires its consumer the function gains a
//! non-test caller, leaves `allowed[]`, and assertion (b) auto-trips.  The
//! owning consumer task MUST delete the corresponding per-file `#[test]` fn (or
//! its rows) as part of the consumer-wiring commit:
//!   - `impulse_shaper_producers_*` — owned by consumer task #3867 (ζ).
//!   - `sampling_producers_*`       — owned by consumer task #3869 (θ).
//!
//! The failure message lists every failing (file_suffix, fn_name) pair — search
//! for them in this file when `G-allow pin(s) failed` appears unexpectedly.
//!
//! Audit invocation: the wide-scope audit is run ONCE per test binary and
//! shared across all six per-file `#[test]` fns via `cached_audit()` (a
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
/// `pub fn` across ≈390 files) six times for one logical query. A process-wide
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
///       EXACT `name` — so `shell_element_stiffness_degenerate` vs `_ans` are
///       distinct, and same-named fns in other files never collide);
///   (b) present EXACTLY ONCE in `result["allowed"]` (same match);
///   (c) its `allow_reason` cites some tracked task as `#NNNN`.
///
/// Collects all failures across the slice and panics once with a combined
/// message, echoing the `engine_seam_orphans_g_allow.rs` failure-collection
/// pattern so a single run pinpoints every missing/misplaced marker.
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
            // Skip (c): it indexes matching_allowed[0], which is unsafe/meaningless
            // when the count is not exactly 1.
            continue;
        }

        // (c) The allow_reason must cite SOME tracked owner task as `#NNNN`.
        //
        // INTENTIONALLY MINIMAL (design decision, plan.json): this verifies only
        // that *a* `#NNNN` token is present, NOT that it equals a specific owner
        // number. The owning/consumer task is single-sourced in the source
        // `// G-allow:` marker; copying the exact number into the test would
        // couple it to a guess and break on legitimate re-attribution. The
        // trade-off is accepted: a typo'd `#NNNN` still passes (c). By the same
        // principle this is the ONLY assertion on marker text — keep it the bare
        // presence check; do NOT grow it into substring/regex pinning of the
        // rest of the comment prose.
        let reason = matching_allowed[0]["allow_reason"]
            .as_str()
            .unwrap_or_default();
        let bytes = reason.as_bytes();
        let cites_task = bytes
            .iter()
            .enumerate()
            .any(|(i, &b)| b == b'#' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit));
        if !cites_task {
            failures.push(format!(
                "  FAIL (c) `{fn_name}` ({file_suffix}): allow_reason must cite a \
                 tracked task as `#NNNN`; got: {reason:?}"
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} G-allow pin(s) failed:\n{}\n\n\
             PINS file: `crates/reify-audit/tests/new_orphans_2026_05_30_g_allow.rs`\n\
             Full orphans list:\n{:#}\n\
             Full allowed list:\n{:#}",
            failures.len(),
            failures.join("\n"),
            result["orphans"],
            result["allowed"],
        );
    }
}

/// Bucket 1 — `modal::free_vibration` ComputeFn pipeline
/// (`crates/reify-eval/src/modal_ops.rs`).
///
/// These 5 `pub(crate) fn` are reached only via the `modal::free_vibration`
/// fn-pointer registered in `compute_targets::register_compute_fns`
/// (mod.rs:140), which the orphan audit cannot trace — so they read as
/// zero-caller orphans despite being live and tested.  PERMANENT bucket-1 pins.
#[test]
fn modal_ops_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        ("crates/reify-eval/src/modal_ops.rs", "build_beam_mesh"),
        ("crates/reify-eval/src/modal_ops.rs", "assemble_modal_km"),
        ("crates/reify-eval/src/modal_ops.rs", "eigensolve_modal"),
        ("crates/reify-eval/src/modal_ops.rs", "solve_modal_core"),
        ("crates/reify-eval/src/modal_ops.rs", "run_modal_analysis"),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 1 — elastic-static ComputeFn shell-channel helper
/// (`crates/reify-eval/src/compute_targets/elastic_static.rs`).
///
/// `shell_channels_to_value` is reached on the elastic-static ComputeFn path
/// via fn-pointer registration the orphan audit cannot trace, so it reads as a
/// zero-caller orphan despite being live and tested.  PERMANENT bucket-1 pin.
#[test]
fn elastic_static_compute_producer_is_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[(
        "crates/reify-eval/src/compute_targets/elastic_static.rs",
        "shell_channels_to_value",
    )];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 1 — degenerate-shell (MITC3+) element kinematics helpers
/// (`crates/reify-solver-elastic/src/elements/degenerate_shell.rs`).
///
/// These 3 producers are reached on the shell-routing compute path via
/// `shell_element_stiffness_degenerate` (itself fn-pointer-registered through
/// the elastic-static ComputeFn), which the orphan audit cannot trace.
/// PERMANENT bucket-1 pins.
///
/// NOTE: `in_plane` (a `pub const fn` in the same file) is DELIBERATELY
/// EXCLUDED — at the wide `crates/reify-*/src` scope a `let in_plane = ...`
/// local in `compute_targets/shell_solve.rs` collides on the bare token, giving
/// `callers >= 1`, so it is in NEITHER `orphans[]` nor `allowed[]`; pinning it
/// would fail assertion (b).  It remains live via same-file `coord.in_plane()`
/// callees and needs no marker while the collision masks it.
#[test]
fn degenerate_shell_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        (
            "crates/reify-solver-elastic/src/elements/degenerate_shell.rs",
            "degenerate_position",
        ),
        (
            "crates/reify-solver-elastic/src/elements/degenerate_shell.rs",
            "mat3_inverse",
        ),
        (
            "crates/reify-solver-elastic/src/elements/degenerate_shell.rs",
            "directors_from_facets",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 1 — degenerate-shell element-stiffness entry points
/// (`crates/reify-solver-elastic/src/shell_assembly.rs`).
///
/// Both `pub fn` are reached on the shell-routing compute path via fn-pointer
/// registration the orphan audit cannot trace, so they read as zero-caller
/// orphans despite being live and tested.  PERMANENT bucket-1 pins.
///
/// The two names share the file suffix but are matched by EXACT name
/// (`name == Some(fn_name)`, not prefix), so `shell_element_stiffness_degenerate`
/// and `shell_element_stiffness_degenerate_ans` each appear in allowed[] exactly
/// once even though the former is a prefix of the latter.
///
/// NOTE: `local_to_global` (same file) is DELIBERATELY EXCLUDED — it now has
/// genuine cross-file callers (`fr.local_to_global()` in shell_result.rs) and is
/// no longer an orphan; pinning it would fail assertion (b).
#[test]
fn shell_assembly_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        (
            "crates/reify-solver-elastic/src/shell_assembly.rs",
            "shell_element_stiffness_degenerate",
        ),
        (
            "crates/reify-solver-elastic/src/shell_assembly.rs",
            "shell_element_stiffness_degenerate_ans",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 2 — input-shaping (impulse-shaper) producers
/// (`crates/reify-stdlib/src/trajectory/impulse_shaper.rs`).
///
/// Tracked producer-before-consumer: producer task #3866 (ε) has landed; the
/// consumer task #3867 (ζ — `input_shape` dispatcher +
/// `reify-eval/src/trajectory_ops.rs` eval wiring) is PENDING, so no in-tree
/// caller exists yet.
///
/// **Removal contract**: consumer task #3867 MUST delete this entire `#[test]`
/// fn as part of its consumer-wiring commit.  Auto-retirement: once #3867 wires
/// the dispatcher these fns gain callers and leave `allowed[]`, tripping
/// assertion (b) at the wide `crates/reify-*/src` scope.
#[test]
fn impulse_shaper_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        ("crates/reify-stdlib/src/trajectory/impulse_shaper.rs", "zvd"),
        (
            "crates/reify-stdlib/src/trajectory/impulse_shaper.rs",
            "cascade",
        ),
        (
            "crates/reify-stdlib/src/trajectory/impulse_shaper.rs",
            "amplitude_sum",
        ),
        (
            "crates/reify-stdlib/src/trajectory/impulse_shaper.rs",
            "trailing_time",
        ),
        (
            "crates/reify-stdlib/src/trajectory/impulse_shaper.rs",
            "residual_vibration",
        ),
        (
            "crates/reify-stdlib/src/trajectory/impulse_shaper.rs",
            "convolve_at",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Bucket 2 — profile→MotionTrajectory sampling bridge
/// (`crates/reify-stdlib/src/trajectory/sampling.rs`).
///
/// Tracked producer-before-consumer: producer task #3855 (γ, DONE; git
/// `956fcc18c7 "(γ/3855)"`) has landed; the consumer task #3869 (θ —
/// `simulate_trajectory`) is PENDING, so no in-tree caller exists yet.
///
/// **Removal contract**: consumer task #3869 MUST delete this entire `#[test]`
/// fn as part of its consumer-wiring commit.  Auto-retirement: once #3869 wires
/// `simulate_trajectory` these fns gain callers and leave `allowed[]`, tripping
/// assertion (b) at the wide `crates/reify-*/src` scope.
#[test]
fn sampling_producers_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        (
            "crates/reify-stdlib/src/trajectory/sampling.rs",
            "resample_cubic",
        ),
        (
            "crates/reify-stdlib/src/trajectory/sampling.rs",
            "to_trajectory_samples",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}
