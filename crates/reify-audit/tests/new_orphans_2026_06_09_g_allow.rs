//! Pin: the `pub fn` producers swept in the 2026-06-09 orphan review that must
//! each carry a `// G-allow:` marker.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test new_orphans_2026_06_09_g_allow`
//!
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! flags every `pub fn` whose only references are same-file / tests / `use` /
//! comments (zero external callers).  A `// G-allow: <reason>` comment on the
//! line immediately above the declaration moves it from the report's
//! "Orphan candidates" table to "Allow-listed".  This test asserts list
//! membership (absent from `orphans[]`, present exactly once in `allowed[]`).
//! The marker's effect on classification — not its prose — is what this test
//! checks; marker wording is never inspected here.  Neither assertion implies
//! `orphan_count == 0`; 500+ pre-existing baseline orphans in unrelated files
//! are intentionally not in scope here.
//!
//! # Buckets
//!
//! ## Same-file caller (false positive)
//!
//! The audit counts only cross-file callers.  These functions are called from
//! within the same source file (not across module boundaries) and have no
//! external callers, so the tool misclassifies them as orphans.
//!
//! - `crates/reify-solver-elastic/src/solver.rs`: `solve_cg_warm`
//! - `crates/reify-eval/src/geometry_ops.rs`: `decode_axis`
//! - `crates/reify-compiler/src/lib.rs`: `merge_prelude_purposes`
//! - `crates/reify-compiler/src/module_dag.rs`: `with_cfg`,
//!   `compile_project_with_entry_source_cfg`
//! - `crates/reify-runtime/src/commitment.rs`: `default_overrides`
//! - `crates/reify-stdlib/src/trajectory/trampoline.rs`:
//!   `value_to_multijoint_spline`, `value_to_modal_model`,
//!   `value_to_mechanism_model`
//! - `crates/reify-lsp/src/analysis.rs`: `count_members_recursive`
//! - `crates/reify-lsp/src/completion.rs`: `determine_context`
//! - `crates/reify-lsp/src/references.rs`: `collect_references`
//!
//! ## Re-export alias (false positive)
//!
//! These functions are consumed via a renamed `pub use` re-export that the
//! audit's cross-file reference scanner cannot trace.
//!
//! - `crates/reify-stdlib/src/dfm.rs`: `diagnose` (re-exported as
//!   `dfm_diagnose` in reify-expr/src/lib.rs)
//! - `crates/reify-stdlib/src/tolerancing.rs`: `diagnose` (re-exported as
//!   `tolerancing_diagnose`; consumer wiring tracked separately)
//!
//! ## LSP public-API entry-point shims
//!
//! The LSP server calls `_in_context` / `_with_parsed` / `_from_parsed`
//! variants; the bare `compute_*` functions are the public API surface but
//! have no direct in-tree caller yet.
//!
//! - `crates/reify-lsp/src/completion.rs`: `compute_completions`
//! - `crates/reify-lsp/src/goto_def.rs`: `compute_goto_definition`,
//!   `compute_goto_definition_cross_file`
//! - `crates/reify-lsp/src/hover.rs`: `compute_hover`
//! - `crates/reify-lsp/src/analysis.rs`: `compute_document_symbols`
//!
//! ## Test-support
//!
//! - `crates/reify-eval/src/engine_admin.rs`: `set_achieved_repr_tol_for_test`
//!
//! ## Library-API / producer-before-consumer
//!
//! Public API producers landed ahead of their consumer; no in-tree caller
//! exists yet but the function is intentionally exposed as a library surface.
//!
//! - `crates/reify-ir/src/geometry.rs`: `write_stl_ascii`
//! - `crates/reify-eval/src/engine_admin.rs`: `shell_gui_mesh_data`
//! - `crates/reify-mesh-morph/src/diagnostics.rs`: `reset` — asserted at the
//!   NARROW `crates/reify-mesh-morph/src` scope, not the wide one; see
//!   "Wide-scope trade-off" below for why.
//!
//! # Removal contract
//!
//! When a pinned function gains a real cross-file caller it leaves `allowed[]`
//! and assertion (b) auto-trips.  The task that wires the consumer MUST delete
//! the corresponding pin row (or whole `#[test]` fn if it becomes empty) as
//! part of the consumer-wiring commit.  Before deleting a row after an
//! assertion-(b) failure, confirm a genuine call edge was wired:
//! `rg '\bFN_NAME\b' crates/reify-*/src`.
//!
//! # Wide-scope trade-off
//!
//! The audit runs at the wide `crates/reify-*/src` scope (same as the baseline
//! report).  All pinned names were verified collision-free at this scope when
//! the file was authored.  Common names (`reset`, `diagnose`) are particularly
//! vulnerable to future incidental collisions: if an unrelated `fn reset` or
//! `.reset()` token appears in another reify-* crate, the pinned function's
//! caller count may rise above zero, dropping it from both `orphans[]` and
//! `allowed[]` and tripping assertion (b) with a misleading message.  If
//! assertion (b) fires unexpectedly, run `rg '\bNAME\b' crates/reify-*/src`
//! to distinguish a genuine new caller from an incidental collision before
//! removing the pin row.
//!
//! ## Materialized collision: `reset` (task #5212)
//!
//! The anticipated collision above HAS now happened, exactly as predicted, for
//! `reset`.  Task #5212 added an unrelated `GeometryKernel::reset` (kernel
//! shape-table eviction) in `crates/reify-kernel-occt`, `crates/reify-geometry`,
//! `crates/reify-eval` and `crates/reify-ir`.  Measured with
//! `scripts/audit-orphan-producers.sh --scope 'crates/reify-*/src'`: the bare
//! `reset` token went from **0** counted corpus hits to **34**, none of them in
//! `crates/reify-mesh-morph/src/diagnostics.rs`.  The audit keys its caller
//! counter on the bare fn NAME (`by_name` / `hits[name][file]`, with
//! `external = total_hits - hits_in_own_file`), so all 34 were attributed to
//! mesh-morph's `reset`, taking its `callers` from 0 to 34.  Both `orphans[]`
//! and `allowed[]` are filtered on `callers == 0`, so the entry vanished from
//! BOTH lists and assertion (b) reported "0 entries".
//!
//! Nothing in #5212 calls `reify_mesh_morph::diagnostics::reset` — the branch
//! diff contains no `mesh_morph` reference at all — so this is the incidental
//! collision, NOT a wired consumer, and the removal contract does NOT apply.
//!
//! Resolution: the `reset` pin is asserted against a SECOND audit run scoped to
//! `crates/reify-mesh-morph/src` ([`cached_mesh_morph_audit`]) instead of the
//! wide one.  At that scope `reset` has exactly one definition, `callers == 0`,
//! and the entry is back in `allowed[]` — so the property this pin exists to
//! protect (the marker is present and still drives the classification) is
//! asserted unchanged.
//!
//! Why not "fix" the caller counter instead: attributing a bare `.reset()`
//! token to one of several same-named definitions needs receiver-type
//! resolution, which a text-scanning tool does not have.  The cheap proxy —
//! treating any name with more than one definition as unattributable — is
//! measurably worse here: `diagnose` already has two definitions
//! (`reify-stdlib/src/dfm.rs`, `reify-stdlib/src/tolerancing.rs`) and BOTH are
//! currently green pins in this file, so that rule would break two passing
//! pins to fix one.  Narrowing the scope for the one pin that has provably
//! lost discrimination is the smaller true change.
//!
//! What this costs: at the narrow scope the removal contract can no longer
//! auto-trip for this fn if a cross-crate consumer inside `crates/reify-*/src`
//! is wired later.  That discrimination was already gone at the wide scope
//! (any `reset` token anywhere in the corpus masks it, and there are now 34),
//! and this fn's real consumer lives in `debug_server.rs` — outside
//! `crates/reify-*/src` — so the wide scope could never observe it either way.
//! The narrow scope trades a discrimination that does not exist for one that
//! is stable.  Scoped audits are an established pattern here: `g_allow.rs` uses
//! `crates/reify-audit/src`, and both `crates/reify-mesh-morph/tests/*_g_allow.rs`
//! already use `crates/reify-mesh-morph/src`.
//!
//! # Graceful skip / authoritative lane
//!
//! If `python3`, `git`, or the audit script are absent from PATH/disk the test
//! prints a note to stderr and returns without failing.  The authoritative CI
//! lane that owns this check MUST set `REIFY_REQUIRE_ORPHAN_AUDIT=1`, which
//! promotes that skip to a hard failure so a dropped `// G-allow:` marker
//! cannot hide on a minimal image.

use std::sync::OnceLock;

use reify_test_support::run_orphan_audit;

/// The wide-scope orphan audit, run **once** per test binary and shared across
/// every per-file pin `#[test]`.
///
/// # Why cache
///
/// Every pin test interrogates the same single fact — the current
/// orphan/allowed classification of `crates/reify-*/src`. Calling
/// `run_orphan_audit` once per `#[test]` would re-scan the entire corpus
/// multiple times for one logical query. A process-wide `OnceLock` pays that
/// cost a single time; Rust's parallel test runner blocks the other test
/// threads inside `get_or_init` until the first finishes, then they all read
/// the cached envelope.
///
/// # Authoritative-lane enforcement (`REIFY_REQUIRE_ORPHAN_AUDIT`)
///
/// `run_orphan_audit` returns `None` — a graceful skip — when `python3`, `git`,
/// or the audit script are absent. That keeps the suite green on minimal images
/// but means a dropped `// G-allow:` marker would go UNDETECTED there, with
/// only an easily-lost stderr note. The canonical CI lane that owns this check
/// MUST set `REIFY_REQUIRE_ORPHAN_AUDIT` (to any non-empty value other than
/// `0`); under that flag a missing-tooling skip is promoted to a hard panic so
/// the regression cannot hide.
fn cached_audit() -> Option<&'static serde_json::Value> {
    static AUDIT: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let audit = AUDIT.get_or_init(|| run_orphan_audit("crates/reify-*/src"));
    enforce_require_flag(audit.is_none());
    audit.as_ref()
}

/// The same audit scoped to `crates/reify-mesh-morph/src`, cached once per test
/// binary exactly like [`cached_audit`].
///
/// Used only by [`mesh_morph_diagnostics_reset_is_g_allow_marked`]. `reset` is a
/// common enough name that unrelated `fn reset` / `.reset()` tokens elsewhere in
/// `crates/reify-*/src` drive the wide audit's name-keyed caller counter above
/// zero, which drops the pinned fn out of BOTH `orphans[]` and `allowed[]` and
/// makes the wide-scope assertion permanently unusable for it. See
/// "Materialized collision: `reset`" in the module header for the measurement
/// and for why the caller counter is not the thing being fixed.
///
/// Scoping to the defining crate restores a single definition and a truthful
/// `callers == 0`, so the marker-presence property is asserted unchanged. The
/// extra run costs ~0.15s (25 pub fns scanned) against the wide run's ~3.4s
/// (2688 scanned) — measured on this box, 2026-08-29.
fn cached_mesh_morph_audit() -> Option<&'static serde_json::Value> {
    static AUDIT: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let audit = AUDIT.get_or_init(|| run_orphan_audit("crates/reify-mesh-morph/src"));
    enforce_require_flag(audit.is_none());
    audit.as_ref()
}

/// Shared `REIFY_REQUIRE_ORPHAN_AUDIT` enforcement for both cached audits: a
/// missing-tooling graceful skip is promoted to a hard panic when the flag is
/// set to any non-empty value other than `0`.
fn enforce_require_flag(audit_missing: bool) {
    if !audit_missing {
        return;
    }
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
             PINS file: `crates/reify-audit/tests/new_orphans_2026_06_09_g_allow.rs`\n\
             Full orphans list:\n{:#}\n\
             Full allowed list:\n{:#}",
            failures.len(),
            failures.join("\n"),
            result["orphans"],
            result["allowed"],
        );
    }
}

/// Same-file caller false positives — the audit counts only cross-file
/// references; these functions have a genuine same-file caller but no external
/// caller, so the tool misclassifies them as orphans.
///
/// Covers 12 functions across 9 files (module_dag.rs hosts 2 pins,
/// trampoline.rs hosts 3).
#[test]
fn same_file_caller_false_positives_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        // solver: cold-start delegate calls solve_cg_warm from within solver.rs
        (
            "crates/reify-solver-elastic/src/solver.rs",
            "solve_cg_warm",
        ),
        // eval: decode_axis called within geometry_ops.rs
        (
            "crates/reify-eval/src/geometry_ops.rs",
            "decode_axis",
        ),
        // compiler lib.rs: merge_prelude_purposes called within lib.rs
        (
            "crates/reify-compiler/src/lib.rs",
            "merge_prelude_purposes",
        ),
        // compiler module_dag.rs: with_cfg and compile_project_with_entry_source_cfg
        // called within module_dag.rs
        (
            "crates/reify-compiler/src/module_dag.rs",
            "with_cfg",
        ),
        (
            "crates/reify-compiler/src/module_dag.rs",
            "compile_project_with_entry_source_cfg",
        ),
        // runtime: default_overrides called within commitment.rs
        (
            "crates/reify-runtime/src/commitment.rs",
            "default_overrides",
        ),
        // stdlib trampoline: all three called within trampoline.rs
        (
            "crates/reify-stdlib/src/trajectory/trampoline.rs",
            "value_to_multijoint_spline",
        ),
        (
            "crates/reify-stdlib/src/trajectory/trampoline.rs",
            "value_to_modal_model",
        ),
        (
            "crates/reify-stdlib/src/trajectory/trampoline.rs",
            "value_to_mechanism_model",
        ),
        // lsp: count_members_recursive called within analysis.rs
        (
            "crates/reify-lsp/src/analysis.rs",
            "count_members_recursive",
        ),
        // lsp: determine_context called within completion.rs
        (
            "crates/reify-lsp/src/completion.rs",
            "determine_context",
        ),
        // lsp: collect_references called within references.rs
        (
            "crates/reify-lsp/src/references.rs",
            "collect_references",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Re-export alias false positives — consumed via a renamed `pub use`
/// re-export that the audit's cross-file reference scanner cannot trace.
///
/// Covers 2 functions.
#[test]
fn reexport_alias_false_positives_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        // dfm::diagnose consumed via `dfm_diagnose` pub-use re-export alias
        // in reify-expr/src/lib.rs; audit cannot trace renamed re-exports
        (
            "crates/reify-stdlib/src/dfm.rs",
            "diagnose",
        ),
        // tolerancing::diagnose exposed as `tolerancing_diagnose` re-export
        // alias; consumer wiring tracked by a separate review task
        (
            "crates/reify-stdlib/src/tolerancing.rs",
            "diagnose",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// LSP public-API entry-point shims — the LSP server uses the
/// `_in_context` / `_with_parsed` / `_from_parsed` variants; the bare
/// `compute_*` functions are the public API surface but have no direct
/// in-tree caller yet.
///
/// Covers 5 functions.
#[test]
fn lsp_public_api_shims_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        (
            "crates/reify-lsp/src/completion.rs",
            "compute_completions",
        ),
        (
            "crates/reify-lsp/src/goto_def.rs",
            "compute_goto_definition",
        ),
        (
            "crates/reify-lsp/src/goto_def.rs",
            "compute_goto_definition_cross_file",
        ),
        (
            "crates/reify-lsp/src/hover.rs",
            "compute_hover",
        ),
        (
            "crates/reify-lsp/src/analysis.rs",
            "compute_document_symbols",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// Library-API / producer-before-consumer and test-support functions.
///
/// - `write_stl_ascii`: STL ASCII serializer; no CLI/GUI consumer wired yet.
/// - `shell_gui_mesh_data`: shell-extract GUI bridge; consumer pending.
/// - `set_achieved_repr_tol_for_test`: test-support setter; not consumed in
///   production builds.
///
/// `reset` (mesh-morph diagnostics) was a fourth pin here until task #5212's
/// unrelated `GeometryKernel::reset` collided with the wide audit's name-keyed
/// caller counter; it now lives in
/// [`mesh_morph_diagnostics_reset_is_g_allow_marked`] against a mesh-morph-scoped
/// audit. See "Materialized collision: `reset`" in the module header.
///
/// Covers 3 functions.
#[test]
fn library_api_and_test_support_are_g_allow_marked() {
    let Some(result) = cached_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[
        // library API: STL ASCII export, no CLI/GUI consumer wired yet
        (
            "crates/reify-ir/src/geometry.rs",
            "write_stl_ascii",
        ),
        // library API: shell-extract GUI bridge pending
        (
            "crates/reify-eval/src/engine_admin.rs",
            "shell_gui_mesh_data",
        ),
        // test-support setter; not consumed in production builds
        (
            "crates/reify-eval/src/engine_admin.rs",
            "set_achieved_repr_tol_for_test",
        ),
    ];
    assert_pins_are_g_allow_marked(result, PINS);
}

/// `reset` in `crates/reify-mesh-morph/src/diagnostics.rs` — same
/// library-API/producer-before-consumer bucket as the three pins above (its real
/// consumer is the cross-crate debug RPC `debug_server.rs`
/// `handle_mesh_morph_stats`, which lives outside `crates/reify-*/src` and is
/// invisible to this audit at ANY scope).
///
/// Split out of [`library_api_and_test_support_are_g_allow_marked`] because it is
/// the one pin in this file whose name collides with unrelated `reset` tokens in
/// the wide `crates/reify-*/src` corpus, which makes the wide audit's name-keyed
/// caller count untruthful for it. Asserting it against the mesh-morph-scoped
/// audit restores a single definition and `callers == 0`. Same assertions as
/// every other pin — (a) absent from `orphans[]`, (b) exactly one entry in
/// `allowed[]` — just against a scope where the classification is meaningful.
///
/// Covers 1 function.
#[test]
fn mesh_morph_diagnostics_reset_is_g_allow_marked() {
    let Some(result) = cached_mesh_morph_audit() else {
        return;
    };
    const PINS: &[(&str, &str)] = &[(
        "crates/reify-mesh-morph/src/diagnostics.rs",
        "reset",
    )];
    assert_pins_are_g_allow_marked(result, PINS);
}
