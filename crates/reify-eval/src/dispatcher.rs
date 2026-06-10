//! Multi-kernel dispatcher (v0.2): pure-logic plan ranking by
//! conversion-stage count.
//!
//! # What this module does
//!
//! Given a registry of kernels (each described by a
//! [`reify_types::CapabilityDescriptor`]), an [`reify_types::Operation`] to
//! perform, a demanded output [`reify_types::ReprKind`], and a set of
//! currently-available reprs, [`dispatch`] picks the kernel +
//! (possibly empty) conversion chain that minimises the number of
//! conversion stages. The result is a [`DispatchPlan`] naming the final
//! kernel and the ordered conversion stages to perform first.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions":
//! - `CapabilityDescriptor { supports: Vec<(Operation, ReprKind)> }` —
//!   feasibility table only, no `cost_hint` or `error_factor`.
//! - Dispatcher ranks candidate (kernel, conversion-chain) plans by
//!   conversion-stage count alone.
//! - Selection deterministic given the registered set of kernels.
//!
//! # Determinism contract
//!
//! 1. Plans are ranked strictly by conversion-stage count (BFS over
//!    reachable [`reify_types::ReprKind`] states; first hit wins).
//! 2. Ties at equal stage-count are broken lexicographically on kernel
//!    name. The `registry` parameter is a [`std::collections::BTreeMap`]
//!    so kernel iteration order is lexicographic and stable across calls.
//! 3. The BFS visited set is keyed on [`reify_types::ReprKind`] (4
//!    variants), so the algorithm terminates after at most 4 expansions.
//!
//! # Scope boundary (task 2641)
//!
//! This module is pure logic. [`dispatch`] IS consumed by the build
//! pipeline — the per-stage tolerance-budget allocator
//! `Engine::compute_realization_tolerance_budget` calls it to select a
//! kernel + conversion chain. The still-open work is cross-kernel
//! OP-ROUTING at the op-execution seam (`execute_realization_ops`),
//! tracked at time of writing under the multi-kernel-phase-3 DAG
//! (tasks ~3439/3443/3444). Subsequent kernel adapter tasks (2643 Manifold,
//! 2644 Fidget, 2645 OpenVDB) consume the [`reify_types::CapabilityDescriptor`]
//! type defined alongside [`reify_types::Operation`] in the
//! `reify-types` crate.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::time::Duration;

use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{CapabilityDescriptor, KernelId, Operation, ReprKind};

use crate::tolerance_budget::per_stage_tolerance;

/// PRD-default wall-time threshold for the long-chain realization warning,
/// in milliseconds.
///
/// Per `docs/prds/v0_2/multi-kernel.md` §"Resolved design decisions" →
/// "Long-chain diagnostic" and `docs/prds/v0_2/per-purpose-tolerance.md`
/// §"Resolved design decisions" → "Long-chain diagnostic gating": the
/// dispatcher emits a warning when the realization wall time **exceeds 500
/// ms** (configurable). Strict-`>` semantics — exactly 500 ms does NOT warn,
/// matching the strict-`<` decision in
/// [`crate::tolerance_promise::is_promise_insufficient`] (task 2651) and
/// the broader "tighter satisfies looser" partial-order vocabulary across
/// the tolerance subsystem.
///
/// Override at runtime via [`LONG_CHAIN_THRESHOLD_ENV_VAR`].
pub const LONG_CHAIN_DEFAULT_THRESHOLD_MS: u64 = 500;

/// Environment variable that overrides the long-chain wall-time threshold.
///
/// Accepted values:
/// - Absent / unset → uses [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`].
/// - A decimal integer string → interpreted as milliseconds.
/// - Any other value → a `tracing::warn!` is emitted and
///   [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`] is used.
///
/// Mirrors [`crate::warm_pool::BUDGET_ENV_VAR`]'s constant-named-value
/// pattern: pinning the env-var name at compile time lets tests catch a
/// typo or rename before the runtime silently ignores the user's override.
pub const LONG_CHAIN_THRESHOLD_ENV_VAR: &str = "REIFY_LONG_CHAIN_THRESHOLD_MS";

/// PRD-default minimum-conversion-stages cutoff for the long-chain realization
/// warning. The predicate uses STRICT `>` against this value, so the cutoff
/// of `2` means "≥3 conversion stages required to warn".
///
/// Per `docs/prds/v0_2/multi-kernel.md` §"Resolved design decisions" →
/// "Long-chain diagnostic": "longer than 2 stages" reads as strict in plain
/// English. Boundary cases (exactly 2 stages) do NOT warn — short-chain
/// pain is self-evident; nagging is poor ergonomics. Exposing the cutoff as
/// a const lets a future PRD revision tighten to `> 1` or relax to `> 3`
/// with a single-line change while the predicate semantics remain pinned by
/// existing tests.
pub const LONG_CHAIN_MIN_STAGES: usize = 2;

/// Strict-`>` predicate for the long-chain realization warning gate.
///
/// Returns `true` iff BOTH gates pass:
///   - `plan.conversions.len() > LONG_CHAIN_MIN_STAGES` (≥3 stages)
///   - `elapsed > threshold` (strictly exceeds the wall-time budget)
///
/// Mirrors the strict-`<` decision in
/// [`crate::tolerance_promise::is_promise_insufficient`] (task 2651) — the
/// "tighter satisfies looser" / "exactly-at-the-line satisfies the
/// constraint" partial-order vocabulary used throughout the tolerance
/// subsystem. Boundary cases (exactly 2 stages, exactly the threshold) do
/// NOT warn: short-chain pain is self-evident and a sub-threshold long
/// chain is not user-visible budget pressure, so suppressing those cases
/// is intentional ergonomics (per `docs/prds/v0_2/multi-kernel.md`
/// §"Long-chain diagnostic" and `docs/prds/v0_2/per-purpose-tolerance.md`
/// §"Long-chain diagnostic gating").
///
/// # Truth table
///
/// | stages | elapsed vs threshold | result | reason                         |
/// |--------|----------------------|--------|--------------------------------|
/// | 0      | any                  | false  | chain not long                 |
/// | 1      | any                  | false  | chain not long                 |
/// | 2      | any                  | false  | boundary; strict `>` on stages |
/// | 3+     | < threshold          | false  | elapsed gate fails             |
/// | 3+     | == threshold         | false  | boundary; strict `>` on time   |
/// | 3+     | > threshold          | true   | both gates pass                |
///
/// Decoupling the predicate from [`long_chain_diagnostic`] lets a hot
/// realization loop check the gate without paying the diagnostic-construction
/// cost (mirrors the [`crate::tolerance_promise::is_promise_insufficient`] /
/// [`crate::tolerance_promise::imported_tolerance_promise_diagnostic`]
/// predicate-plus-builder split established by task 2651).
pub fn is_long_chain_realization(
    plan: &DispatchPlan,
    elapsed: Duration,
    threshold: Duration,
) -> bool {
    plan.conversions.len() > LONG_CHAIN_MIN_STAGES && elapsed > threshold
}

/// Build the `Severity::Warning` diagnostic emitted when the dispatcher
/// selects a chain longer than 2 conversion stages AND elapsed realization
/// wall time exceeds the configured threshold.
///
/// Returns `None` when the predicate
/// [`is_long_chain_realization`] is false — short-chain pain is
/// self-evident and a sub-threshold long chain is not user-visible budget
/// pressure, so neither case warrants a warning. When `Some(diag)` is
/// returned the diagnostic carries
/// [`DiagnosticCode::LongChainRealization`] for filter-by-code downstream
/// consumers (LSP / IDE / batch pipelines) and a human-readable message
/// naming the chain so users can see exactly where the conversion budget
/// is going (per PRD `docs/prds/v0_2/multi-kernel.md` §"Long-chain
/// diagnostic": "names the chain so users can see budget pressure").
///
/// # Integration status
///
/// TODO(task-2642): wire this builder into the realization timing loop
/// in `geometry_ops.rs` once the kernel-registry mechanism + OCCT adapter
/// migration lands. Until then, `long_chain_diagnostic` is scaffolding
/// — public API with no in-tree caller — exactly mirroring the scope
/// boundary already documented at the module level (see "Scope boundary
/// (task 2641)" docblock above). Greppable callout intentionally
/// duplicated here so a future cleanup pass on the wiring task can
/// locate the seam without re-reading the module docs.
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_2/multi-kernel.md` §"Resolved design decisions" →
/// "Long-chain diagnostic" and `docs/prds/v0_2/per-purpose-tolerance.md`
/// §"Resolved design decisions" → "Long-chain diagnostic gating": the
/// runtime emits a *warning* (not error) — the realization completed; the
/// user just deserves visibility into budget pressure. Mirrors the
/// advisory-warning posture of `ImportedTolerancePromiseInsufficient`,
/// `FieldOutOfBounds`, and `KinematicSingularity` — downstream tooling
/// that wants to surface this as a harder failure (e.g. a CI gate) can
/// filter by code at the consumer side.
///
/// # Arguments
///
/// - `plan` — the [`DispatchPlan`] returned by [`dispatch`]; the chain's
///   conversion stages and final-stage kernel are rendered into the
///   diagnostic message verbatim.
/// - `elapsed` — measured realization wall time, in [`Duration`].
/// - `threshold` — the configured warn threshold; typically obtained from
///   [`long_chain_threshold_from_env`] or set explicitly by the caller.
pub fn long_chain_diagnostic(
    plan: &DispatchPlan,
    elapsed: Duration,
    threshold: Duration,
) -> Option<Diagnostic> {
    if !is_long_chain_realization(plan, elapsed, threshold) {
        return None;
    }
    // Render each conversion stage as "{kernel}: {from:?}→{to:?}" — the stage
    // kernel is named via `KernelId::as_registry_name` (the canonical
    // lowercase inventory name, e.g. "occt"), and Debug formatting on ReprKind
    // already prints human-readable variant names (BRep / Mesh / Sdf / Voxel).
    // PRD: "names the chain so users can see budget pressure" — each kernel +
    // repr transition tells the user exactly where the conversion budget is
    // going.
    let stages_rendered = plan
        .conversions
        .iter()
        .map(|(kernel, from, to)| format!("{}: {from:?}→{to:?}", kernel.as_registry_name()))
        .collect::<Vec<_>>()
        .join(" → ");
    let message = format!(
        "long-chain realization ({} stages, elapsed {}ms > {}ms): {} → {}",
        plan.conversions.len(),
        elapsed.as_millis(),
        threshold.as_millis(),
        stages_rendered,
        plan.kernel,
    );
    Some(Diagnostic::warning(message).with_code(DiagnosticCode::LongChainRealization))
}

/// Build the `Severity::Error` diagnostic emitted when the multi-kernel
/// dispatcher cannot find any kernel + conversion chain that performs `op`
/// to produce the `demanded` repr from the currently-`available` reprs.
///
/// Unlike [`long_chain_diagnostic`] (which carries an internal predicate
/// gate and returns `Option<Diagnostic>` because the caller cannot know
/// whether to skip), this builder is *unconditional*: the caller has
/// already walked the BFS to exhaustion and decided the failure applies, so
/// it returns [`Diagnostic`] directly (mirroring
/// [`crate::tolerance_promise::imported_tolerance_promise_diagnostic`]). The
/// diagnostic carries [`DiagnosticCode::NoKernelChain`] for filter-by-code
/// downstream consumers and a human-readable message naming the op, the
/// demanded repr, and every available repr so the user can see exactly
/// which conversion was impossible.
///
/// # Integration status
///
/// TODO(task-3435/δ): wire this builder into the dispatcher's `None`-return
/// path in op-execution once the multi-kernel dispatch surface lands (PRD
/// `docs/prds/v0_3/multi-kernel-phase-3.md` §8 DAG; consumers δ/ε =
/// IDs 3435/3436). Until then, `no_kernel_chain_diagnostic` is scaffolding
/// — public API with no in-tree caller — exactly mirroring the scope
/// boundary documented at the module level and the `long_chain_diagnostic`
/// precedent (task 2646). Greppable callout intentionally duplicated here so
/// a future wiring pass can locate the seam without re-reading module docs.
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §2: "The error is
/// user-visible — failing closed is the failure mode." The dispatcher
/// refuses to silently pick an incompatible kernel; the user gets a typed
/// error and can adjust their kernel set or `#kernel(...)` pragma.
///
/// # Determinism
///
/// `available` is collected into a [`BTreeSet`] before rendering so the
/// message is stable across runs — the caller's `HashSet<ReprKind>`
/// iteration order is salted by the process hash seed (see
/// `dispatch`'s `seeds: BTreeSet<ReprKind>` step and the
/// `dispatch_seeding_order_is_deterministic` test for the same
/// load-bearing convention).
///
/// # Arguments
///
/// - `op` — the [`Operation`] the dispatcher failed to route.
/// - `demanded` — the [`ReprKind`] the op was required to produce.
/// - `available` — the reprs the inputs were realised in when dispatch
///   failed; rendered sorted via [`ReprKind`]'s `Ord` derive.
// G-allow: task #3436 no-kernel-chain diagnostic builder; in-tree consumer wiring follows the long_chain_diagnostic precedent
pub fn no_kernel_chain_diagnostic(
    op: Operation,
    demanded: ReprKind,
    available: &[ReprKind],
) -> Diagnostic {
    let available_sorted: BTreeSet<ReprKind> = available.iter().copied().collect();
    let available_rendered = available_sorted
        .iter()
        .map(|r| format!("{r:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let message = format!(
        "no kernel chain found for op '{op:?}' to produce '{demanded:?}'; \
         available reprs: [{available_rendered}]"
    );
    Diagnostic::error(message).with_code(DiagnosticCode::NoKernelChain)
}

/// Build the `Severity::Warning` diagnostic emitted when a `#kernel(...)`
/// pragma names a kernel that cannot serve the demanded `(op, demanded)`
/// pair, so the dispatcher falls through to default lex-min selection.
///
/// Unconditional [`Diagnostic`]-returning builder (the caller has already
/// observed the pragma kernel does not support the demand). Carries
/// [`DiagnosticCode::KernelPragmaUnsatisfiable`] for filter-by-code
/// consumers and a message naming the pragma kernel, the op, and the
/// demanded repr.
///
/// # Integration status
///
/// TODO(task-3443/ο): wire this builder into the `#kernel(...)` pragma
/// surface once it lands (PRD `docs/prds/v0_3/multi-kernel-phase-3.md`
/// §5 + §8 DAG; consumer ο = ID 3443). Until then, scaffolding — public
/// API with no in-tree caller — mirroring the `long_chain_diagnostic`
/// precedent (task 2646).
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5: "warning, not error —
/// fall through to default lex-min selection so the user's design still
/// evaluates." The realization proceeds; the warning gives the author
/// visibility into the unmet kernel preference.
///
/// # Arguments
///
/// - `pragma_kernel` — the kernel name written in the `#kernel(...)`
///   pragma that could not be honoured.
/// - `op` — the [`Operation`] the pragma kernel was asked to perform.
/// - `demanded` — the [`ReprKind`] the op was required to produce.
// G-allow: task #3443 #kernel(...) pragma diagnostic builder; consumer wiring lands in subsequent #3443 steps (multi-kernel-phase-3 PRD)
pub fn kernel_pragma_unsatisfiable_diagnostic(
    pragma_kernel: &str,
    op: Operation,
    demanded: ReprKind,
) -> Diagnostic {
    let message = format!(
        "#kernel('{pragma_kernel}') cannot serve op '{op:?}' producing \
         '{demanded:?}'; falling through to default kernel selection"
    );
    Diagnostic::warning(message).with_code(DiagnosticCode::KernelPragmaUnsatisfiable)
}

/// Build the `Severity::Error` diagnostic emitted when `reify.toml`
/// `[kernels]` pins a kernel that the current build did not register
/// (typically because the corresponding Cargo feature was not enabled).
///
/// Unconditional [`Diagnostic`]-returning builder (the caller has already
/// observed the pinned kernel is absent from the registry). Carries
/// [`DiagnosticCode::PinnedKernelMissing`] for filter-by-code consumers
/// and a message naming the missing kernel id.
///
/// # Integration status
///
/// TODO(task-3444/π): wire this builder into `reify.toml` parsing in
/// `Engine::with_registered_kernels` once it lands (PRD
/// `docs/prds/v0_3/multi-kernel-phase-3.md` §5 + §8 DAG; consumer π =
/// ID 3444). Until then, scaffolding — public API with no in-tree caller
/// — mirroring the `long_chain_diagnostic` precedent (task 2646).
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5: "error; engine
/// refuses to start." The determinism contract requires every pinned
/// kernel to be present; the engine fails closed at startup rather than
/// silently downgrading to a different kernel set.
///
/// # Arguments
///
/// - `kernel_id` — the kernel name pinned in `reify.toml` `[kernels]`
///   that is missing from the build's registry.
// G-allow: task #3444 reify.toml [kernels] pinned-missing diagnostic builder; consumer wiring lands in subsequent #3444 steps (multi-kernel-phase-3 PRD)
pub fn pinned_kernel_missing_diagnostic(kernel_id: &str) -> Diagnostic {
    let message = format!(
        "kernel '{kernel_id}' is pinned in reify.toml but not registered in \
         this build; rebuild with the required kernel feature enabled"
    );
    Diagnostic::error(message).with_code(DiagnosticCode::PinnedKernelMissing)
}

/// Build the `Severity::Warning` diagnostic emitted when a kernel is present
/// in the registry but not listed in `reify.toml` `[kernels]`.
///
/// Unconditional [`Diagnostic`]-returning builder (the caller has already
/// observed the registered kernel is absent from the pin set). Carries
/// [`DiagnosticCode::UnpinnedKernelLoaded`] for filter-by-code consumers
/// and a message naming the unpinned kernel id.
///
/// # Integration status
///
/// TODO(task-3444/π): wire this builder into `reify.toml` parsing in
/// `Engine::with_registered_kernels` once it lands (PRD
/// `docs/prds/v0_3/multi-kernel-phase-3.md` §5 + §8 DAG; consumer π =
/// ID 3444). Until then, scaffolding — public API with no in-tree caller
/// — mirroring the `long_chain_diagnostic` precedent (task 2646).
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5: "warning; engine
/// starts." The kernel is usable, so the realization proceeds; the missing
/// pin only weakens the determinism contract (a future build that omits
/// the same kernel feature could shift selection), so the author gets an
/// advisory rather than a hard failure.
///
/// # Arguments
///
/// - `kernel_id` — the kernel name present in the registry but absent from
///   `reify.toml` `[kernels]`.
// G-allow: task #3444 unpinned-kernel-loaded diagnostic builder; consumer wiring lands in subsequent #3444 steps (multi-kernel-phase-3 PRD)
pub fn unpinned_kernel_loaded_diagnostic(kernel_id: &str) -> Diagnostic {
    let message = format!(
        "kernel '{kernel_id}' is registered but not listed in reify.toml \
         [kernels]; consider pinning it for build determinism"
    );
    Diagnostic::warning(message).with_code(DiagnosticCode::UnpinnedKernelLoaded)
}

/// Build the `Severity::Error` diagnostic emitted when `reify.toml` pins a
/// kernel version that disagrees with the adapter's compiled-in `VERSION`
/// constant.
///
/// Unconditional [`Diagnostic`]-returning builder (the caller has already
/// compared the pinned version against the adapter `VERSION`). Carries
/// [`DiagnosticCode::KernelVersionMismatch`] for filter-by-code consumers
/// and a message naming the kernel id, the pinned version, and the actual
/// adapter version.
///
/// # Integration status
///
/// TODO(task-3444/π): wire this builder into `reify.toml` parsing in
/// `Engine::with_registered_kernels` once it lands (PRD
/// `docs/prds/v0_3/multi-kernel-phase-3.md` §5 + §8 DAG; consumer π =
/// ID 3444). Until then, scaffolding — public API with no in-tree caller
/// — mirroring the `long_chain_diagnostic` precedent (task 2646).
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5: "error. Determinism
/// contract enforcement." Matching versions is load-bearing for
/// reproducible realization across build hosts; the engine fails closed
/// rather than silently using a different adapter than the project pins.
///
/// # Arguments
///
/// - `kernel_id` — the kernel name whose version disagrees.
/// - `pinned` — the version string pinned in `reify.toml` `[kernels]`.
/// - `actual` — the adapter's compiled-in `VERSION` constant.
// G-allow: task #3444 kernel-version-mismatch diagnostic builder; consumer wiring lands in subsequent #3444 steps (multi-kernel-phase-3 PRD)
pub fn kernel_version_mismatch_diagnostic(
    kernel_id: &str,
    pinned: &str,
    actual: &str,
) -> Diagnostic {
    let message = format!(
        "kernel '{kernel_id}' version mismatch: reify.toml pins '{pinned}' \
         but adapter VERSION = '{actual}'; determinism contract requires \
         matching versions"
    );
    Diagnostic::error(message).with_code(DiagnosticCode::KernelVersionMismatch)
}

/// Resolve the long-chain wall-time threshold from the
/// [`LONG_CHAIN_THRESHOLD_ENV_VAR`] environment variable, falling back to
/// [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`] when unset or unparseable.
///
/// Production wrapper around [`long_chain_threshold_from_env_value`]. This
/// function reads the process environment exactly once and delegates the
/// parse-and-fallback semantics to the test seam — mirroring the
/// [`crate::warm_pool::WarmStatePool::from_env_or_default`] /
/// [`crate::warm_pool::WarmStatePool::from_env_value`] split (warm_pool.rs:160-205).
///
/// # Why a seam?
///
/// `std::env::set_var` and `std::env::remove_var` are `unsafe` in Rust 2024
/// edition and race-prone across parallel tests. Unit-testing this thin
/// wrapper directly would require `unsafe` env mutation; instead, the
/// public seam takes `Option<&str>` (matching `std::env::var(...).ok().as_deref()`'s
/// shape) so the parser branches can be exercised without touching the
/// process environment. See `warm_pool.rs:166-171` for the original rationale.
pub fn long_chain_threshold_from_env() -> Duration {
    long_chain_threshold_from_env_value(std::env::var(LONG_CHAIN_THRESHOLD_ENV_VAR).ok().as_deref())
}

/// Test seam for [`long_chain_threshold_from_env`]: the parser-with-fallback
/// half of the env-var read pipeline.
///
/// | `value`              | Result                                              |
/// |----------------------|-----------------------------------------------------|
/// | `None`               | [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`] ms (unset env)  |
/// | `Some("")`           | [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`] ms (shell `VAR=`)|
/// | `Some(parseable u64)`| `Duration::from_millis(parsed)`                     |
/// | `Some(other)`        | `tracing::warn!`; [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`]|
///
/// Mirrors [`crate::warm_pool::WarmStatePool::from_env_value`]'s
/// shell-empty-string posture: `VAR=` exports `""` rather than unsetting,
/// so treat empty the same as absent without emitting a spurious warn.
pub fn long_chain_threshold_from_env_value(value: Option<&str>) -> Duration {
    let parsed_ms: u64 = match value {
        None => LONG_CHAIN_DEFAULT_THRESHOLD_MS,
        Some("") => LONG_CHAIN_DEFAULT_THRESHOLD_MS,
        Some(s) => match s.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                tracing::warn!(
                    env_var = LONG_CHAIN_THRESHOLD_ENV_VAR,
                    raw = %s,
                    default_ms = LONG_CHAIN_DEFAULT_THRESHOLD_MS,
                    "could not parse long-chain threshold; using default ({} ms)",
                    LONG_CHAIN_DEFAULT_THRESHOLD_MS,
                );
                LONG_CHAIN_DEFAULT_THRESHOLD_MS
            }
        },
    };
    Duration::from_millis(parsed_ms)
}

/// Returns the per-stage tolerance budget for a conversion chain described by a
/// [`DispatchPlan`].
///
/// This is the conversion-chain budget allocator: `n_stages` is resolved from
/// `plan.conversions.len()`.  For chains with at least one conversion stage, the
/// function delegates to [`crate::tolerance_budget::per_stage_tolerance`], which
/// applies a geometric split with the 0.8 `SAFETY_FACTOR` (see
/// `docs/prds/v0_2/per-purpose-tolerance.md` §"Conversion-budget allocation
/// heuristic").  For an empty chain (demanded repr already in `available`, no
/// kernel boundary crossed), the function returns `requested_tol` unchanged —
/// applying the safety factor would gratuitously tighten the user's budget on a
/// non-existent chain.
///
/// Co-located with [`is_long_chain_realization`] / [`long_chain_diagnostic`]
/// because all three functions resolve stage count from `plan.conversions.len()`;
/// keeping them together minimises grep-and-edit cost for future refactors.
///
/// # Why not `per_stage_tolerance(tol, plan.conversions.len().max(1))`?
///
/// For an empty chain, `len().max(1)` would pass `n_stages = 1`, yielding
/// `tol × 0.8` — the safety factor fires even though there is no conversion
/// error to budget.  The correct contract for a zero-conversion plan is strict
/// pass-through: the demanded repr is already present, so no chain budget is
/// allocated at all.  This wrapper captures that semantic distinction so
/// callers do not have to replicate the `is_empty()` guard themselves.
///
/// # Truth table
///
/// | `plan.conversions.len()` | result                                        |
/// |--------------------------|-----------------------------------------------|
/// | 0 (empty chain)          | `requested_tol` (pass-through, no factor)     |
/// | 1                        | `requested_tol × 0.8` (via delegation)        |
/// | N ≥ 2                    | `requested_tol^(1/N) × 0.8` (via delegation) |
///
/// # Panics (debug builds only)
///
/// In debug builds, panics if `requested_tol` is not finite or is negative,
/// keeping the precondition uniform across both the empty-chain and non-empty
/// branches (the non-empty branch delegates to `per_stage_tolerance`, which
/// carries the same assertion).
pub fn per_stage_tolerance_for_plan(plan: &DispatchPlan, requested_tol: f64) -> f64 {
    debug_assert!(
        crate::tolerance_gate::is_valid_tolerance_si(requested_tol),
        "dispatcher: requested_tol must be finite and non-negative, got {requested_tol}"
    );
    if plan.conversions.is_empty() {
        // No kernel boundary crossed: demanded repr was already in `available`.
        // Pass through unchanged — the 0.8 SAFETY_FACTOR only applies when
        // stages exist to accumulate conversion error.
        requested_tol
    } else {
        per_stage_tolerance(requested_tol, plan.conversions.len())
    }
}

/// A v0.3-ε-executable projection for a single conversion stage `(from, to)`.
///
/// [`dispatch`] returns a [`DispatchPlan`] whose `conversions` chain names the
/// reprs to cross *before* the final op runs, but it does not say *how* each
/// crossing is performed. [`v03_conversion_projection`] is that bridge: it maps
/// a `(from, to)` repr pair to the concrete kernel work the conversion executor
/// must run.
///
/// v0.3 ε ships exactly one projection — `BRep → Mesh` via the source kernel's
/// `GeometryKernel::tessellate` — so the enum has a single variant today. It is
/// a closed enum (not a bare bool) so that future conversions (`Mesh → Sdf`,
/// voxelisation, …) extend it by adding variants and rows to
/// [`v03_conversion_projection`], with the executor's `match` forcing every
/// call site to handle the new shape.
// `#[allow(dead_code)]`: constructed/consumed only from `#[cfg(test)]` until the
// conversion executor in `execute_realization_ops` wires it into the non-test
// build path (task 4050 step-8). Mirrors the `compute_demanded_reprs`
// "deferred to task 4050" precedent in engine_build.rs.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversionProjection {
    /// Tessellate the source kernel's BRep handle into a mesh — the
    /// `BRep → Mesh` realisation. The executor calls
    /// `source_kernel.tessellate(handle, per_stage_tol)` to produce the mesh,
    /// then hands it to the target kernel's `ingest_mesh`.
    Tessellate,
    /// Voxelize a Mesh handle via the target kernel's `ingest_mesh` — the sole
    /// `Mesh → Voxel` realisation. The executor calls `plan.kernel.ingest_mesh(&mesh)`
    /// where `mesh` was produced by the preceding `Tessellate` stage (or is the
    /// direct input). `ingest_mesh` on the OpenVDB kernel converts the interchange
    /// mesh into a voxel grid on ingest, so no separate voxelise call is needed.
    Voxelize,
}

/// Classifies a single conversion stage `(from, to)` into the v0.3-β-executable
/// [`ConversionProjection`], or `None` when β cannot perform that crossing.
///
/// The conversion executor walks a [`DispatchPlan`]'s `conversions` chain and
/// calls this for each `(from, to)` stage: a `Some(projection)` is run, while a
/// `None` surfaces as the realization-failed diagnostic (NOT a panic) — the
/// plan named a crossing the current β slice cannot execute.
///
/// v0.3 β supports exactly two crossings:
/// - `(BRep, Mesh) ⇒ Tessellate` — BRep→Mesh via source kernel `tessellate`.
/// - `(Mesh, Voxel) ⇒ Voxelize` — Mesh→Voxel via target kernel `ingest_mesh`.
///
/// Every other ordered pair returns `None`. Adding a conversion to β means
/// adding a [`ConversionProjection`] variant and a row to the match below.
// `#[allow(dead_code)]`: see [`ConversionProjection`] — wired into the non-test
// build path by `execute_realization_ops` in task 4050 step-8.
#[allow(dead_code)]
pub(crate) fn v03_conversion_projection(
    from: ReprKind,
    to: ReprKind,
) -> Option<ConversionProjection> {
    match (from, to) {
        (ReprKind::BRep, ReprKind::Mesh) => Some(ConversionProjection::Tessellate),
        (ReprKind::Mesh, ReprKind::Voxel) => Some(ConversionProjection::Voxelize),
        _ => None,
    }
}

/// Ordered sequence of conversion stages: each entry is
/// `(kernel, from_repr, to_repr)`, where `kernel` is the [`KernelId`] that
/// performs that stage. Factored as a type alias to keep the internal BFS
/// frontier type below clippy's `type_complexity` threshold and to give the
/// conversion-chain shape a single named home.
type ConversionChain = Vec<(KernelId, ReprKind, ReprKind)>;

/// A concrete plan returned by [`dispatch`]: which kernel runs the final op,
/// preceded by zero or more conversion stages.
///
/// Each conversion entry is `(kernel, from, to)`: the named [`KernelId`] is
/// expected to convert from `from` to `to`. The conversions are ordered so
/// the final entry's `to` matches the input repr expected by `kernel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchPlan {
    /// Name of the kernel that runs the final (target) operation.
    pub kernel: String,
    /// Sequence of conversion stages to perform before invoking `kernel`.
    /// Each tuple = `(kernel, from_repr, to_repr)`, where `kernel` is a
    /// [`KernelId`]. Empty when the demanded repr is already in `available`.
    pub conversions: ConversionChain,
}

/// Pick a kernel + conversion chain to perform `op` and produce `demanded`,
/// given that the inputs are currently realised in the reprs listed by
/// `available`.
///
/// **Algorithm.** BFS over reachable [`ReprKind`] states. The frontier is
/// seeded with `{(r, vec![]) | r ∈ available}`. At each pop, the current
/// repr is the *input* repr available to the final-stage op. We probe
/// `descriptor.supports(op, demanded)`. By the input==output invariant on
/// [`CapabilityDescriptor::supports`] (see its doc), the `current_repr ==
/// demanded` check on the popped state verifies both the kernel's expected
/// input repr and its produced output repr in one comparison.
/// [`Operation::Convert { from }`] entries are the only shape where the
/// second tuple element diverges from the input repr; those are handled
/// exclusively by the expansion step below. We probe
/// every registered kernel for `(op, demanded)`: if any kernel supports
/// the demanded `(op, output_repr)` pair AND the popped state's repr
/// equals `demanded`, we return immediately. Otherwise we expand by every
/// kernel-declared conversion `(Convert{from: popped_repr}, to)`,
/// enqueuing `(to, chain ++ (kernel_name, popped_repr, to))` for any `to`
/// not yet visited. BFS termination is guaranteed because the visited set
/// is keyed on [`ReprKind`] (4 variants → at most 4 expansions).
///
/// **Tie-break.** Ties at equal stage-count are broken lexicographically on
/// kernel name; the registry is a [`BTreeMap`] so kernel iteration is
/// deterministic across BTreeMap iteration order (lexicographic). Selection
/// is therefore deterministic given a fixed `registry` (PRD
/// `docs/prds/v0_2/multi-kernel.md`: "Selection deterministic given pinned
/// runtime configuration"). Ties at equal stage-count and equal final
/// kernel choice fall through to the order in which we enqueue conversion
/// expansions, which is itself a BTreeMap-order traversal.
///
/// **`None` returns** in three branches:
///   - (a) no conversion path from any repr in `available` reaches
///     `demanded` (the BFS visited set covers all 4 [`ReprKind`] variants
///     without producing the demanded one);
///   - (b) no registered kernel claims `(op, demanded)` in its supports
///     table — even when the demanded repr IS reachable;
///   - (c) the registry is empty (or `available` is empty AND no
///     conversion can synthesise a repr ex nihilo, which by construction
///     cannot happen since [`Operation::Convert { from }`] always
///     requires an input repr).
pub fn dispatch(
    registry: &BTreeMap<String, &CapabilityDescriptor>,
    op: Operation,
    demanded: ReprKind,
    available: &HashSet<ReprKind>,
) -> Option<DispatchPlan> {
    // BFS state: (currently-realised repr, conversion chain so far).
    let mut frontier: VecDeque<(ReprKind, ConversionChain)> = VecDeque::new();
    let mut visited: HashSet<ReprKind> = HashSet::new();

    // Seed in deterministic [`ReprKind`] order. The caller hands us a
    // `&HashSet<ReprKind>` whose iteration order is salted by the process's
    // hashing key — iterating it directly would let the multi-seed final-stage
    // probe pick a different kernel across runs even when the registered set
    // is identical, breaking the PRD's "Selection deterministic given pinned
    // runtime configuration" contract. `BTreeSet` traversal is ordered by
    // `Ord` (BRep < Mesh < Sdf < Voxel per the enum declaration order); BFS
    // by stage-count is preserved because all available reprs sit at distance
    // 0 regardless of seed order.
    let seeds: BTreeSet<ReprKind> = available.iter().copied().collect();
    for r in seeds {
        frontier.push_back((r, vec![]));
        visited.insert(r);
    }

    while let Some((current_repr, chain)) = frontier.pop_front() {
        // Final-stage probe: does any kernel support (op, demanded), AND is
        // the current repr equal to `demanded` (so the kernel can consume
        // what we have / will have)? Iterate in BTreeMap order for
        // lexicographic determinism.
        if current_repr == demanded {
            for (name, descriptor) in registry.iter() {
                if descriptor.supports(op, demanded) {
                    return Some(DispatchPlan {
                        kernel: name.clone(),
                        conversions: chain,
                    });
                }
            }
        }

        // Expansion step: for every kernel-declared conversion
        // (Convert{from: current_repr}, to), enqueue (to, chain + entry).
        //
        // TODO(perf): O(K · S) per popped state where K=#kernels, S=avg
        // supports size. v0.2 has ~50 entries × 4 kernels so this is fine,
        // but if a future kernel grows a large supports table, pre-index
        // conversion edges into a `BTreeMap<ReprKind, Vec<(kernel_name,
        // ReprKind)>>` keyed by `from` to avoid re-scanning the full
        // supports vec at each pop.
        for (kernel_name, descriptor) in registry.iter() {
            for &(decl_op, decl_to) in descriptor.supports.iter() {
                if let Operation::Convert { from } = decl_op
                    && from == current_repr
                    && !visited.contains(&decl_to)
                {
                    visited.insert(decl_to);
                    let mut new_chain = chain.clone();
                    // Bridge the registry name to its typed KernelId. Sound by
                    // construction: this registry is built from the inventory,
                    // so it only ever contains canonical kernel names; a missing
                    // mapping is a programming error that should fail loudly.
                    // (The sibling bridge `engine_build::kernel_id_for_registry_name`
                    // takes the OPPOSITE policy — a silent `KernelId::Occt`
                    // fallback rather than a panic — because it runs on the build
                    // path where non-canonical names are legitimate and its
                    // `.kernel` tag is informational only. See that fn's docs for
                    // why the two policies intentionally differ.)
                    new_chain.push((
                        KernelId::from_registry_name(kernel_name)
                            .expect("registered conversion kernel must have a KernelId"),
                        current_repr,
                        decl_to,
                    ));
                    frontier.push_back((decl_to, new_chain));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use reify_ir::{CapabilityDescriptor, KernelId, Operation, ReprKind};

    use super::{
        DispatchPlan, LONG_CHAIN_DEFAULT_THRESHOLD_MS, LONG_CHAIN_MIN_STAGES,
        LONG_CHAIN_THRESHOLD_ENV_VAR, dispatch, is_long_chain_realization,
        kernel_pragma_unsatisfiable_diagnostic, kernel_version_mismatch_diagnostic,
        long_chain_diagnostic, long_chain_threshold_from_env_value, no_kernel_chain_diagnostic,
        per_stage_tolerance_for_plan, pinned_kernel_missing_diagnostic,
        unpinned_kernel_loaded_diagnostic,
    };
    use crate::tolerance_budget::{SAFETY_FACTOR, per_stage_tolerance};
    use std::time::Duration;

    /// Pins the three module-level long-chain constants by literal value:
    /// the PRD-default threshold (500 ms wall, per
    /// `docs/prds/v0_2/per-purpose-tolerance.md` §"Long-chain diagnostic
    /// gating"), the min-stages cutoff (`>` 2 ⇒ ≥3), and the env-var name
    /// `REIFY_LONG_CHAIN_THRESHOLD_MS`. A typo or rename loudly fails this
    /// test — mirrors `warm_pool::budget_env_var_name` (warm_pool.rs:830).
    #[test]
    fn long_chain_constants_are_pinned() {
        assert_eq!(LONG_CHAIN_DEFAULT_THRESHOLD_MS, 500);
        assert_eq!(
            LONG_CHAIN_THRESHOLD_ENV_VAR,
            "REIFY_LONG_CHAIN_THRESHOLD_MS"
        );
        assert_eq!(LONG_CHAIN_MIN_STAGES, 2);
    }

    /// Negative-path coverage for [`is_long_chain_realization`]: each branch
    /// where one or both gates fail must return `false`. Pins the
    /// strict-`>` boundary semantics on BOTH gates independently — a future
    /// `>=` flip on either gate would silently invert PRD-prose intent
    /// ("longer than 2 stages" / "exceeds 500 ms").
    #[test]
    fn is_long_chain_realization_returns_false_when_chain_short() {
        let threshold = Duration::from_millis(500);

        // (a) Zero conversions + huge elapsed → chain not long ⇒ false.
        let plan_zero = DispatchPlan {
            kernel: "occt".to_string(),
            conversions: vec![],
        };
        assert!(
            !is_long_chain_realization(&plan_zero, Duration::from_secs(60), threshold),
            "0 conversion stages must NOT trip the long-chain warning even with huge elapsed",
        );

        // (b) Exactly 2 conversions + huge elapsed → boundary on the
        //     stage-count gate (strict `>` LONG_CHAIN_MIN_STAGES) ⇒ false.
        let plan_two = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                (KernelId::Occt, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Manifold, ReprKind::Mesh, ReprKind::Sdf),
            ],
        };
        assert!(
            !is_long_chain_realization(&plan_two, Duration::from_secs(60), threshold),
            "exactly 2 conversion stages must NOT warn (strict > on LONG_CHAIN_MIN_STAGES)",
        );

        // (c) 3+ conversions + zero elapsed → elapsed gate fails ⇒ false.
        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert!(
            !is_long_chain_realization(&plan_three, Duration::ZERO, threshold),
            "elapsed = 0 must NOT warn even with 3 stages — both gates must hold",
        );

        // (d) 3 conversions + elapsed exactly == threshold → strict-`>`
        //     boundary on the elapsed gate ⇒ false.
        assert!(
            !is_long_chain_realization(&plan_three, threshold, threshold),
            "elapsed exactly equal to threshold must NOT warn (strict > on threshold)",
        );
    }

    /// Positive-path coverage for [`is_long_chain_realization`]: when both
    /// gates strictly pass, the predicate returns `true`. Independent from
    /// the negative-path test (`is_long_chain_realization_returns_false_…`)
    /// so a regression that breaks one direction (e.g. inverts the
    /// predicate, or drops one of the two `&&` gates) doesn't mask the
    /// other.
    #[test]
    fn is_long_chain_realization_returns_true_when_both_gates_pass() {
        let threshold = Duration::from_millis(500);

        // Just-over the boundary on both gates: 3 stages > 2, 501 > 500.
        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert!(
            is_long_chain_realization(&plan_three, Duration::from_millis(501), threshold),
            "3 stages + 501ms > 500ms threshold: both gates strictly pass ⇒ true",
        );

        // Larger margin on both gates: 4 stages, elapsed 2s.
        let plan_four = DispatchPlan {
            kernel: "kernel_e".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
                (KernelId::Occt, ReprKind::Voxel, ReprKind::Mesh),
            ],
        };
        assert!(
            is_long_chain_realization(&plan_four, Duration::from_secs(2), threshold),
            "4 stages + 2s elapsed >> 500ms threshold: both gates pass ⇒ true",
        );
    }

    /// Pins the `Option<Diagnostic>` return shape's negative branch: when
    /// the predicate gate is false, the builder must return `None`. The
    /// gate must short-circuit BEFORE any `Diagnostic` is constructed —
    /// otherwise an Engine layer that sees `Some(diag)` and forwards
    /// downstream would log spurious warnings on every short-chain call.
    #[test]
    fn long_chain_diagnostic_returns_none_when_predicate_false() {
        let threshold = Duration::from_millis(500);

        // Stage-count gate fails: 2 conversions (boundary), even though
        // elapsed >> threshold.
        let plan_two = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                (KernelId::Occt, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Manifold, ReprKind::Mesh, ReprKind::Sdf),
            ],
        };
        assert!(
            long_chain_diagnostic(&plan_two, Duration::from_secs(60), threshold).is_none(),
            "2 conversion stages must NOT emit a diagnostic (stage gate fails)",
        );

        // Elapsed gate fails: 3 conversions but elapsed == threshold (boundary).
        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert!(
            long_chain_diagnostic(&plan_three, threshold, threshold).is_none(),
            "elapsed exactly == threshold must NOT emit (elapsed gate fails)",
        );
    }

    /// Pins the wire-contract of [`long_chain_diagnostic`] when the predicate
    /// is true: the emitted [`reify_types::Diagnostic`] carries
    /// `Severity::Warning` and `Some(DiagnosticCode::LongChainRealization)`.
    /// This is the load-bearing assertion downstream LSP / MCP consumers
    /// filter on. Mirrors
    /// `imported_tolerance_promise_diagnostic_builds_warning_with_code_and_template_name`
    /// (tolerance_promise.rs:557-580).
    #[test]
    fn long_chain_diagnostic_carries_warning_severity_and_code_when_emitted() {
        use reify_core::{DiagnosticCode, Severity};

        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        let threshold = Duration::from_millis(500);
        let elapsed = Duration::from_millis(750);

        let diag = long_chain_diagnostic(&plan_three, elapsed, threshold)
            .expect("3 stages + elapsed > threshold must emit a diagnostic");

        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic severity must be Warning (PRD: warn, not error — \
             realization completed; user just deserves visibility into \
             budget pressure)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::LongChainRealization),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers (LSP / MCP)"
        );
    }

    /// Pins the PRD-mandated chain-naming requirement at the structural
    /// level: every stage's kernel name AND the final-stage kernel must
    /// appear in the diagnostic message so users can see exactly where the
    /// conversion budget is going. Asserts only `contains()` of each kernel
    /// name — does NOT pin specific surrounding prose
    /// ("realization", "elapsed", separator chars), keeping the test
    /// wording-churn-resistant per the
    /// `imported_tolerance_promise_diagnostic_builds_warning_with_code_and_template_name`
    /// precedent.
    #[test]
    fn long_chain_diagnostic_message_names_each_chain_kernel_and_final_stage() {
        let plan = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Occt, ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        let threshold = Duration::from_millis(500);
        let elapsed = Duration::from_millis(900);

        let diag = long_chain_diagnostic(&plan, elapsed, threshold)
            .expect("3 stages + elapsed > threshold must emit a diagnostic");

        for kernel in ["fidget", "gmsh", "occt", "manifold"] {
            assert!(
                diag.message.contains(kernel),
                "diagnostic message must name kernel {:?} so users can see budget pressure (got: {:?})",
                kernel,
                diag.message,
            );
        }
    }

    /// Pins the repr-name component of "names the chain": the message must
    /// include the from/to ReprKind variants for each conversion stage.
    /// Prevents a future refactor that drops the `from:?→to:?` portion (e.g.
    /// emitting kernel names alone) from silently regressing the
    /// user-visible budget-pressure signal.
    #[test]
    fn long_chain_diagnostic_message_includes_repr_transitions() {
        let plan = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Sdf),
                (KernelId::Gmsh, ReprKind::Sdf, ReprKind::Mesh),
                (KernelId::Manifold, ReprKind::Mesh, ReprKind::Voxel),
            ],
        };
        let threshold = Duration::from_millis(500);
        let elapsed = Duration::from_millis(750);

        let diag = long_chain_diagnostic(&plan, elapsed, threshold)
            .expect("3 stages + elapsed > threshold must emit a diagnostic");

        for repr in ["BRep", "Sdf", "Mesh", "Voxel"] {
            assert!(
                diag.message.contains(repr),
                "diagnostic message must surface ReprKind variant {:?} so users \
                 can see the conversion-budget shape (got: {:?})",
                repr,
                diag.message,
            );
        }
    }

    /// Pins the env-resolver default branch: when the env var is unset
    /// (i.e. `std::env::var(LONG_CHAIN_THRESHOLD_ENV_VAR)` returns
    /// `Err(NotPresent)`, modeled by `None` at the test seam) the resolver
    /// falls back to [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`].
    ///
    /// Test mechanics: this codebase deliberately avoids
    /// `std::env::set_var`/`remove_var` (unsafe in Rust 2024 edition,
    /// race-prone across parallel tests — see `warm_pool.rs:166-171`).
    /// Instead, [`long_chain_threshold_from_env_value`] is the public test
    /// seam mirroring [`crate::warm_pool::WarmStatePool::from_env_value`];
    /// the production wrapper [`super::long_chain_threshold_from_env`] is a
    /// one-liner that reads `std::env::var(...)` and delegates here. This
    /// pins the same parser contract a `remove_var`-driven test would
    /// without unsafe env mutation.
    #[test]
    fn long_chain_threshold_from_env_returns_default_when_unset() {
        let resolved = long_chain_threshold_from_env_value(None);
        assert_eq!(
            resolved,
            Duration::from_millis(LONG_CHAIN_DEFAULT_THRESHOLD_MS),
            "unset env var must resolve to the PRD-default threshold ({} ms), got {:?}",
            LONG_CHAIN_DEFAULT_THRESHOLD_MS,
            resolved,
        );
    }

    /// Pins the configurability knob: a project setting
    /// `REIFY_LONG_CHAIN_THRESHOLD_MS=1000` (modeled by `Some("1000")` at
    /// the seam) actually changes the threshold to 1000ms — the env var is
    /// not silently ignored. Independent from the unset-default test
    /// (`long_chain_threshold_from_env_returns_default_when_unset`) so a
    /// regression that always returned the default would fail this test
    /// specifically while passing the unset-default test.
    #[test]
    fn long_chain_threshold_from_env_uses_env_value_when_valid() {
        let resolved = long_chain_threshold_from_env_value(Some("1000"));
        assert_eq!(
            resolved,
            Duration::from_millis(1000),
            "env var '1000' must resolve to Duration::from_millis(1000), got {:?}",
            resolved,
        );
    }

    /// Pins the silent-fallback posture for unparseable env values: a
    /// malformed string (e.g. `"not_a_number"`) must NOT panic and must NOT
    /// silently use 0ms (which would spam warnings on every long-chain
    /// plan). The resolver falls back to [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`]
    /// while emitting a `tracing::warn!` so operators see the misconfig at
    /// log-level rather than discovering it via a runtime panic. Mirrors
    /// `warm_pool::from_env_value`'s "Invalid value … falling back to
    /// default" branch (warm_pool.rs:189-202).
    #[test]
    fn long_chain_threshold_from_env_falls_back_when_unparseable() {
        let resolved = long_chain_threshold_from_env_value(Some("not_a_number"));
        assert_eq!(
            resolved,
            Duration::from_millis(LONG_CHAIN_DEFAULT_THRESHOLD_MS),
            "unparseable env value must fall back to default ({} ms), got {:?}",
            LONG_CHAIN_DEFAULT_THRESHOLD_MS,
            resolved,
        );
    }

    /// Trivial happy path: one kernel that supports the demanded op directly on
    /// a repr already in `available`. Plan must be `(kernel, no conversions)`.
    /// This locks the zero-conversion code path before BFS expansion is added.
    #[test]
    fn dispatch_zero_conversion_returns_plan_with_kernel_only() {
        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::BRep,
            &available,
        );
        assert_eq!(
            plan,
            Some(DispatchPlan {
                kernel: "occt".to_string(),
                conversions: vec![],
            }),
            "zero-conversion path: occt supports (BooleanUnion, BRep) and BRep is available",
        );
    }

    /// Single-conversion chain: input is BRep but the requesting op is a Mesh
    /// boolean. The plan must invoke occt's BRep→Mesh tessellation, then run
    /// manifold's BooleanUnion on the resulting Mesh.
    ///
    /// This locks BFS's first expansion step — discovering reachable reprs by
    /// applying any kernel's `Convert{from: ...}` entry.
    #[test]
    fn dispatch_single_conversion_chain() {
        // occt only knows how to tessellate BRep into Mesh.
        let occt = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::BRep,
                },
                ReprKind::Mesh,
            )],
        };
        // manifold only knows how to perform BooleanUnion on Mesh.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);
        registry.insert("manifold".to_string(), &manifold);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::Mesh,
            &available,
        )
        .expect("a single-stage chain BRep→Mesh→Union must be findable");

        assert_eq!(
            plan.kernel, "manifold",
            "the final-stage Mesh BooleanUnion must run on manifold"
        );
        assert_eq!(
            plan.conversions.len(),
            1,
            "exactly one conversion stage (BRep→Mesh) is required, got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            (KernelId::Occt, ReprKind::BRep, ReprKind::Mesh),
            "the conversion stage must be (occt, BRep, Mesh), got {:?}",
            plan.conversions[0],
        );
    }

    /// Two competing chains lead to (BooleanUnion, Mesh): a 1-stage path via
    /// alpha (BRep→Mesh→Union) and a 2-stage path via beta (BRep→Sdf→Mesh→
    /// Union). BFS by stage-count must pick the shorter one. Locks the
    /// "rank by conversion-stage count alone" PRD requirement.
    #[test]
    fn dispatch_prefers_shorter_chain() {
        let alpha = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let beta = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Sdf,
                ),
                (
                    Operation::Convert {
                        from: ReprKind::Sdf,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("fidget".to_string(), &alpha);
        registry.insert("gmsh".to_string(), &beta);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::Mesh,
            &available,
        )
        .expect("a 1-stage chain via alpha must be findable");

        assert_eq!(
            plan.conversions.len(),
            1,
            "BFS must pick the 1-stage chain, not the 2-stage chain via Sdf — got {plan:?}",
        );
        // Pin the final-stage kernel and the conversion-stage content so a
        // regression that flips the chosen kernel (e.g. by reversing the
        // BTreeMap probe direction) breaks loudly instead of slipping past
        // the length-only assertion. With both kernels listing
        // `(BooleanUnion, Mesh)`, lexicographic tie-break selects "alpha";
        // with both listing `(Convert{BRep}→Mesh)` reachable in one step,
        // "alpha" is also the BTreeMap-first kernel that names the
        // conversion edge.
        assert_eq!(
            plan.kernel, "fidget",
            "lexicographic tie-break must pick 'fidget' over 'gmsh', got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
            "the 1-stage conversion must be (fidget, BRep, Mesh), got {:?}",
            plan.conversions[0],
        );
    }

    /// Two-stage chain as winner: the only path from `{BRep}` to
    /// `(BooleanUnion, Mesh)` is BRep→Sdf (via alpha) then Sdf→Mesh (via
    /// beta), because no kernel declares `(Convert{BRep}, Mesh)`. Locks BFS
    /// multi-stage expansion as the *accepted-path winner*, not just the
    /// rejected-path loser as in `dispatch_prefers_shorter_chain`.
    #[test]
    fn dispatch_two_stage_chain_is_shortest() {
        // alpha: converts BRep → Sdf only. No direct BRep→Mesh anywhere.
        let alpha = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::BRep,
                },
                ReprKind::Sdf,
            )],
        };
        // beta: converts Sdf → Mesh only.
        let beta = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::Sdf,
                },
                ReprKind::Mesh,
            )],
        };
        // manifold: runs BooleanUnion on Mesh. No conversion edges declared.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("fidget".to_string(), &alpha);
        registry.insert("gmsh".to_string(), &beta);
        registry.insert("manifold".to_string(), &manifold);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::Mesh,
            &available,
        )
        .expect("a 2-stage chain BRep→Sdf→Mesh→Union must be findable");

        assert_eq!(
            plan.conversions.len(),
            2,
            "exactly two conversion stages (BRep→Sdf, Sdf→Mesh) are required, got {plan:?}",
        );
        assert_eq!(
            plan.kernel, "manifold",
            "the final-stage Mesh BooleanUnion must run on manifold, got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            (KernelId::Fidget, ReprKind::BRep, ReprKind::Sdf),
            "first conversion stage must be (fidget, BRep, Sdf), got {:?}",
            plan.conversions[0],
        );
        assert_eq!(
            plan.conversions[1],
            (KernelId::Gmsh, ReprKind::Sdf, ReprKind::Mesh),
            "second conversion stage must be (gmsh, Sdf, Mesh), got {:?}",
            plan.conversions[1],
        );
    }

    /// Locks the `seeds: BTreeSet<ReprKind>` seeding step, which canonicalises
    /// the hash-randomised `HashSet<ReprKind>` input into `Ord`-sorted order
    /// before the BFS frontier is populated.
    ///
    /// Registry shape: kappa converts BRep→Mesh, lambda converts Sdf→Mesh, and
    /// manifold runs BooleanUnion on Mesh. With both BRep and Sdf available,
    /// the `seeds` BTreeSet ensures BRep < Sdf in frontier order, so kappa is
    /// always chosen over lambda. Without the `seeds: BTreeSet<ReprKind>`
    /// seeding step, the outcome would depend on hash-randomised HashSet
    /// iteration, making CI output non-deterministic across hash-seed
    /// perturbations.
    #[test]
    fn dispatch_seeding_order_is_deterministic() {
        // kappa: converts BRep → Mesh in one step.
        let kappa = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::BRep,
                },
                ReprKind::Mesh,
            )],
        };
        // lambda: converts Sdf → Mesh in one step.
        let lambda = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::Sdf,
                },
                ReprKind::Mesh,
            )],
        };
        // manifold: runs BooleanUnion on Mesh.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("fidget".to_string(), &kappa);
        registry.insert("gmsh".to_string(), &lambda);
        registry.insert("manifold".to_string(), &manifold);

        // Both reprs available. The `seeds: BTreeSet<ReprKind>` seeding step
        // guarantees BRep < Sdf traversal order so kappa always wins over
        // lambda, irrespective of the HashSet's per-process hash randomisation.
        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);
        available.insert(ReprKind::Sdf);

        let plan = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::Mesh,
            &available,
        )
        .expect("kappa (BRep→Mesh) path must be findable");

        assert_eq!(
            plan.kernel, "manifold",
            "the final-stage Mesh BooleanUnion must run on manifold, got {plan:?}",
        );
        assert_eq!(
            plan.conversions.len(),
            1,
            "exactly one conversion stage (BRep→Mesh via kappa) expected, got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
            "conversion stage must be (fidget, BRep, Mesh) — BRep < Sdf in BTreeSet order, got {:?}",
            plan.conversions[0],
        );
    }

    /// Two kernels both directly support the demanded (op, repr) with zero
    /// conversions. The lexicographically smaller kernel name wins.
    ///
    /// Five repeated calls confirm determinism — a HashMap-based registry
    /// would otherwise return a random kernel each call. Locks the PRD's
    /// "Selection deterministic given pinned runtime configuration".
    #[test]
    fn dispatch_tie_break_lexicographic_kernel_name() {
        let alpha = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("alpha".to_string(), &alpha);
        registry.insert("manifold".to_string(), &manifold);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::Mesh);

        // Repeat 5x: every call must return the same kernel name.
        for trial in 0..5 {
            let plan = dispatch(
                &registry,
                Operation::BooleanUnion,
                ReprKind::Mesh,
                &available,
            )
            .expect("both kernels can answer the demand directly");
            assert_eq!(
                plan.kernel, "alpha",
                "trial {trial}: lexicographically smaller name 'alpha' must win over 'manifold', got {plan:?}",
            );
            assert!(
                plan.conversions.is_empty(),
                "trial {trial}: zero-conversion path expected, got {plan:?}",
            );
        }
    }

    /// Three None-return branches must all hold:
    ///   (a) no conversion path from any available repr to the demanded repr;
    ///   (b) op never declared on any reachable repr;
    ///   (c) registry empty.
    #[test]
    fn dispatch_returns_none_when_no_chain_exists() {
        // (a) occt only supports BRep ops, no conversion to Mesh; Mesh demand
        //     is unreachable.
        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);
        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);
        assert_eq!(
            dispatch(
                &registry,
                Operation::BooleanUnion,
                ReprKind::Mesh,
                &available
            ),
            None,
            "(a) demanded repr Mesh unreachable from {{BRep}} via no conversions ⇒ None",
        );

        // (b) Demand-repr matches kernel's declared support repr (BRep), but
        //     `available` is empty AND no conversion exists to bring any repr
        //     into scope. Frontier seeded empty ⇒ never enters the probe.
        let empty_available: HashSet<ReprKind> = HashSet::new();
        assert_eq!(
            dispatch(
                &registry,
                Operation::BooleanUnion,
                ReprKind::BRep,
                &empty_available
            ),
            None,
            "(b) demanded BRep is in occt's supports table but `available` is empty ⇒ None",
        );

        // (c) Op not in any descriptor (registry has only Convert + a single
        //     boolean) — query a Modify op and expect None.
        assert_eq!(
            dispatch(
                &registry,
                Operation::ModifyFillet,
                ReprKind::BRep,
                &available
            ),
            None,
            "(c) ModifyFillet not in any kernel's supports ⇒ None",
        );

        // Edge case: empty registry. Frontier is seeded but nothing matches.
        let empty_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        assert_eq!(
            dispatch(
                &empty_registry,
                Operation::BooleanUnion,
                ReprKind::Mesh,
                &available
            ),
            None,
            "edge: empty registry ⇒ None",
        );
    }

    /// Integration: a small registry mirroring the v0.2 planned setup —
    /// "occt" owns BRep primitives + BRep→Mesh tessellation, "manifold" owns
    /// Mesh booleans. This locks the contract shape that downstream tasks
    /// (2642 kernel-registry wiring, 2643 manifold adapter) will consume:
    ///
    ///   1. `BooleanUnion → Mesh` from `available = {BRep}` → "manifold" via
    ///      one conversion stage performed by "occt" (BRep→Mesh).
    ///   2. `PrimitiveBox → BRep` from `available = {BRep}` → "occt" with
    ///      zero conversions. Primitives are passed `available = {demanded}`
    ///      because they produce the demanded repr from non-geometric inputs
    ///      (size/dimension scalars), so the BFS treats the demanded repr as
    ///      "trivially in scope" with no conversion required.
    ///
    /// No new dispatcher logic is exercised here beyond what step-7's
    /// single-conversion test and step-9's shortest-chain test already lock;
    /// this test exists so future kernel-registry refactors break loudly if
    /// the v0.2 occt+manifold contract regresses.
    #[test]
    fn dispatch_uses_capability_descriptor_for_v02_kernels() {
        // occt: BRep primitives (Box/Cylinder/Sphere) + BRep→Mesh tessellation.
        let occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::PrimitiveCylinder, ReprKind::BRep),
                (Operation::PrimitiveSphere, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        // manifold: Mesh booleans (Union/Difference/Intersection).
        let manifold = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (Operation::BooleanDifference, ReprKind::Mesh),
                (Operation::BooleanIntersection, ReprKind::Mesh),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);
        registry.insert("manifold".to_string(), &manifold);

        // Scenario 1: BooleanUnion demanded as Mesh, inputs realised as BRep.
        // Plan must invoke occt's BRep→Mesh conversion, then manifold's union.
        let mut available_brep: HashSet<ReprKind> = HashSet::new();
        available_brep.insert(ReprKind::BRep);
        let plan_union = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::Mesh,
            &available_brep,
        )
        .expect("v0.2 occt+manifold mix must satisfy BRep→Mesh→Union");
        assert_eq!(
            plan_union.kernel, "manifold",
            "Mesh BooleanUnion must run on manifold per v0.2 capability split",
        );
        assert_eq!(
            plan_union.conversions.len(),
            1,
            "BRep→Mesh requires exactly one conversion stage, got {plan_union:?}",
        );
        assert_eq!(
            plan_union.conversions[0],
            (KernelId::Occt, ReprKind::BRep, ReprKind::Mesh),
            "the conversion stage must be (occt, BRep, Mesh), got {:?}",
            plan_union.conversions[0],
        );

        // Scenario 2: PrimitiveBox demanded as BRep. Primitives pass
        // `available = {demanded}` since they produce the demanded repr
        // without consuming a geometric input. Plan picks occt directly.
        let plan_box = dispatch(
            &registry,
            Operation::PrimitiveBox,
            ReprKind::BRep,
            &available_brep,
        )
        .expect("v0.2 occt+manifold mix must satisfy PrimitiveBox→BRep");
        assert_eq!(
            plan_box.kernel, "occt",
            "BRep PrimitiveBox must run on occt per v0.2 capability split",
        );
        assert!(
            plan_box.conversions.is_empty(),
            "PrimitiveBox→BRep requires zero conversions, got {plan_box:?}",
        );
    }

    /// Empty conversion chain must return `requested_tol` unchanged (bit-exact
    /// pass-through, no arithmetic applied).
    ///
    /// Pins the no-chain-no-allocation contract: when `plan.conversions` is
    /// empty the demanded repr was already in `available` — no kernel boundary
    /// is crossed, so the 0.8 `SAFETY_FACTOR` must NOT be applied.  Applying
    /// it would silently strip 20 % of the user's budget on every direct-
    /// dispatch path, which the PRD explicitly rules out ("When a request
    /// crosses kernel boundaries … the orchestrator divides the bound across
    /// stages" — no boundary ⇒ no division).
    ///
    /// Three distinct magnitudes (1e-3, 1e-6, 1.0) demonstrate the pass-
    /// through is independent of the scale of `requested_tol`.
    #[test]
    fn per_stage_tolerance_for_plan_empty_chain_returns_requested_tol_unchanged() {
        let plan = DispatchPlan {
            kernel: "occt".to_string(),
            conversions: vec![],
        };

        // Bit-exact pass-through: no arithmetic must touch the value.
        assert_eq!(
            per_stage_tolerance_for_plan(&plan, 1e-3),
            1e-3,
            "empty conversion chain must return requested_tol unchanged (bit-exact pass-through)",
        );

        // Independence of magnitude: pass-through holds for small tolerances.
        assert_eq!(
            per_stage_tolerance_for_plan(&plan, 1e-6),
            1e-6,
            "empty chain pass-through must be independent of requested_tol magnitude (1e-6)",
        );

        // Independence of magnitude: pass-through holds for unit tolerance.
        assert_eq!(
            per_stage_tolerance_for_plan(&plan, 1.0),
            1.0,
            "empty chain pass-through must be independent of requested_tol magnitude (1.0)",
        );
    }

    /// Multi-stage chains must delegate to `per_stage_tolerance` verbatim —
    /// i.e. `per_stage_tolerance_for_plan(&plan, req)` ==
    /// `per_stage_tolerance(req, plan.conversions.len())`.
    ///
    /// Two chain shapes are checked to catch off-by-one bugs in the n_stages
    /// resolution (e.g. `len() + 1` vs `len()`, or a hard-coded `n_stages = 1`):
    ///   - 2-conversion chain → N=2, expected = req^(1/2) × 0.8
    ///   - 3-conversion chain → N=3, expected = req^(1/3) × 0.8
    ///
    /// Each case has two complementary assertions:
    ///
    /// 1. **Delegation assertion** (`assert_eq!` against `per_stage_tolerance`
    ///    directly): catches wiring divergence between the two functions — e.g.
    ///    if `per_stage_tolerance_for_plan` stopped delegating and hard-coded a
    ///    wrong exponent.
    ///
    /// 2. **Hand-computed numeric pin** (`assert_eq!` against the literal
    ///    `req^(1/N) × 0.8` expression): secondary defence-in-depth against
    ///    a shared formula regression. Note that
    ///    `tolerance_budget::tests::geometric_split_multi_stages` already pins
    ///    the same formula for the underlying `per_stage_tolerance` function, so
    ///    these pins do not claim unique coverage — they simply make the expected
    ///    output of this test self-evident without tracing into a helper.
    #[test]
    fn per_stage_tolerance_for_plan_multi_stage_chain_uses_geometric_split() {
        // 2-conversion chain: BRep → Sdf → Mesh (N = 2).
        let plan_two = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Sdf),
                (KernelId::Gmsh, ReprKind::Sdf, ReprKind::Mesh),
            ],
        };
        let req = 1e-3_f64;
        assert_eq!(
            per_stage_tolerance_for_plan(&plan_two, req),
            per_stage_tolerance(req, plan_two.conversions.len()),
            "2-conversion chain must delegate to per_stage_tolerance(req, 2) verbatim",
        );
        // Hand-computed numeric pin: secondary confirmation the formula is
        // req^(1/N) * 0.8. Both sides execute identical IEEE-754 operations on
        // the same inputs so the result is bit-identical — assert_eq! is valid.
        let expected_two = 0.001_f64.powf(0.5) * 0.8;
        assert_eq!(
            per_stage_tolerance_for_plan(&plan_two, req),
            expected_two,
            "2-conversion chain must equal req^(1/2) * 0.8",
        );

        // 3-conversion chain: BRep → Mesh → Sdf → Voxel (N = 3).
        let plan_three = DispatchPlan {
            kernel: "fidget".to_string(),
            conversions: vec![
                (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
                (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
                (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert_eq!(
            per_stage_tolerance_for_plan(&plan_three, req),
            per_stage_tolerance(req, plan_three.conversions.len()),
            "3-conversion chain must delegate to per_stage_tolerance(req, 3) verbatim",
        );
        let expected_three = 0.001_f64.powf(1.0 / 3.0) * 0.8;
        assert_eq!(
            per_stage_tolerance_for_plan(&plan_three, req),
            expected_three,
            "3-conversion chain must equal req^(1/3) * 0.8",
        );
    }

    /// A single-conversion chain (N=1) must return `requested_tol × SAFETY_FACTOR`
    /// bit-exactly (the N=1 path in `per_stage_tolerance` short-circuits without
    /// `powf`, so the result is a simple multiply — `assert_eq!` is valid here).
    ///
    /// This is a separate test from the multi-stage case because it pins a
    /// specific regression: if the empty-vs-non-empty branch were accidentally
    /// flipped to `if plan.conversions.len() <= 1` the single-conversion case
    /// would fall into the pass-through branch and return `requested_tol` instead
    /// of `requested_tol × 0.8`, which step-1 and step-3 alone would not catch.
    #[test]
    fn per_stage_tolerance_for_plan_single_conversion_applies_safety_factor() {
        let plan = DispatchPlan {
            kernel: "occt".to_string(),
            conversions: vec![(KernelId::Occt, ReprKind::BRep, ReprKind::Mesh)],
        };
        let req = 1e-3_f64;
        // N=1: no powf, so bit-exact multiply.
        assert_eq!(
            per_stage_tolerance_for_plan(&plan, req),
            req * SAFETY_FACTOR,
            "single-conversion chain must return requested_tol × SAFETY_FACTOR (N=1 short-circuit)",
        );
    }

    /// Task 4050 (steps 5/6) + task 4422 (β): [`v03_conversion_projection`]
    /// classifies a single conversion stage `(from, to)` into the
    /// v0.3-β-executable projection.
    ///
    /// β supports exactly TWO conversion shapes:
    /// - `(BRep, Mesh)` ⇒ `Tessellate` — the source kernel tessellates its
    ///   BRep handle into a mesh, which the target kernel then ingests via
    ///   `ingest_mesh`.
    /// - `(Mesh, Voxel)` ⇒ `Voxelize` — the target kernel voxelises the
    ///   interchange mesh via `ingest_mesh` (producing a voxel grid).
    ///
    /// EVERY other ordered `(from, to)` pair over the four [`ReprKind`]
    /// variants is NOT runnable in β and must classify as `None`, so the
    /// conversion executor surfaces it as a realization-failed diagnostic
    /// rather than attempting (or panicking on) an unsupported stage.
    ///
    /// Exhaustively pins all 16 ordered pairs: the two supported cells return
    /// `Some(Tessellate)` for `(BRep, Mesh)` and `Some(Voxelize)` for
    /// `(Mesh, Voxel)`; the other 14 return `None`. Adding a new conversion
    /// to β means adding a [`ConversionProjection`] variant and a row to
    /// `v03_conversion_projection`, and updating this table explicitly.
    #[test]
    fn v03_conversion_projection_supports_brep_to_mesh_and_mesh_to_voxel() {
        use super::{ConversionProjection, v03_conversion_projection};

        let all = [
            ReprKind::BRep,
            ReprKind::Mesh,
            ReprKind::Sdf,
            ReprKind::Voxel,
        ];

        for &from in &all {
            for &to in &all {
                let got = v03_conversion_projection(from, to);
                if from == ReprKind::BRep && to == ReprKind::Mesh {
                    assert_eq!(
                        got,
                        Some(ConversionProjection::Tessellate),
                        "(BRep, Mesh) must classify as the Tessellate projection \
                         (BRep→Mesh realised by source kernel tessellate)",
                    );
                } else if from == ReprKind::Mesh && to == ReprKind::Voxel {
                    assert_eq!(
                        got,
                        Some(ConversionProjection::Voxelize),
                        "(Mesh, Voxel) must classify as the Voxelize projection \
                         (Mesh→Voxel realised by target kernel ingest_mesh)",
                    );
                } else {
                    assert_eq!(
                        got, None,
                        "({from:?}, {to:?}) is not β-executable and must classify \
                         as None (only BRep→Mesh and Mesh→Voxel are supported in v0.3-β)",
                    );
                }
            }
        }
    }

    /// Pins the wire-contract of [`no_kernel_chain_diagnostic`]: the emitted
    /// [`reify_types::Diagnostic`] carries `Severity::Error` and
    /// `Some(DiagnosticCode::NoKernelChain)`. This is the load-bearing
    /// assertion downstream tasks δ/ε (3435/3436) filter on when wiring the
    /// dispatcher None-return into op-execution. Mirrors
    /// `long_chain_diagnostic_carries_warning_severity_and_code_when_emitted`
    /// (the severity+code half of the long-chain precedent), except severity
    /// is Error here per PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §2
    /// "failing closed is the failure mode".
    #[test]
    fn no_kernel_chain_diagnostic_carries_error_severity_and_code() {
        use reify_core::{DiagnosticCode, Severity};

        let diag = no_kernel_chain_diagnostic(
            Operation::BooleanUnion,
            ReprKind::BRep,
            &[ReprKind::Mesh, ReprKind::Voxel],
        );

        assert_eq!(
            diag.severity,
            Severity::Error,
            "diagnostic severity must be Error (PRD §2: failing closed is \
             the failure mode — the dispatcher exhausted its BFS without \
             reaching the demanded repr)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::NoKernelChain),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers (tasks δ/ε wiring + LSP / MCP)"
        );
    }

    /// Pins the user-visible-content requirement: the message must name the
    /// op (Debug-rendered), the demanded repr, AND every available repr so
    /// the user can see exactly which conversion was impossible. Asserts
    /// only `contains()` of each named element — does NOT pin surrounding
    /// prose — keeping the test wording-churn-resistant per the
    /// `long_chain_diagnostic_message_names_each_chain_kernel_and_final_stage`
    /// precedent.
    #[test]
    fn no_kernel_chain_diagnostic_message_names_op_demanded_and_available() {
        let diag = no_kernel_chain_diagnostic(
            Operation::BooleanUnion,
            ReprKind::BRep,
            &[ReprKind::Mesh, ReprKind::Voxel],
        );

        for needle in ["BooleanUnion", "BRep", "Mesh", "Voxel"] {
            assert!(
                diag.message.contains(needle),
                "diagnostic message must surface {:?} so the user can see \
                 which op/repr conversion was impossible (got: {:?})",
                needle,
                diag.message,
            );
        }
    }

    /// Pins the empty-`available` rendering boundary: when the caller passes
    /// `&[]` (e.g. a dispatcher whose inputs were realised in zero reprs, or
    /// a future op that demands a fresh-synthesis repr from nothing), the
    /// available-reprs list must render as `[]` rather than panicking or
    /// producing a malformed string like `[, ]`. Locks the rendering
    /// contract that downstream tasks δ/ε (3435/3436) can rely on when
    /// wiring this builder into op-execution. Also implicitly covers
    /// the dedup contract: `BTreeSet` silently drops duplicates, which is
    /// load-bearing for deterministic rendering — empty input is the
    /// degenerate-but-valid case at one end of that spectrum.
    #[test]
    fn no_kernel_chain_diagnostic_renders_empty_available_as_brackets() {
        let diag = no_kernel_chain_diagnostic(Operation::BooleanUnion, ReprKind::BRep, &[]);

        assert!(
            diag.message.contains("[]"),
            "empty `available` slice must render as `[]` so the message stays \
             well-formed when the dispatcher fails before any input is \
             realised (got: {:?})",
            diag.message,
        );
    }

    /// Pins the wire-contract of [`kernel_pragma_unsatisfiable_diagnostic`]:
    /// `Severity::Warning` + `Some(DiagnosticCode::KernelPragmaUnsatisfiable)`.
    /// Warning (not Error) per PRD `docs/prds/v0_3/multi-kernel-phase-3.md`
    /// §5 "warning, not error — fall through to default lex-min selection so
    /// the user's design still evaluates". Consumed by task ο (ID 3443).
    #[test]
    fn kernel_pragma_unsatisfiable_diagnostic_carries_warning_severity_and_code() {
        use reify_core::{DiagnosticCode, Severity};

        let diag = kernel_pragma_unsatisfiable_diagnostic(
            "manifold",
            Operation::BooleanUnion,
            ReprKind::Mesh,
        );

        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic severity must be Warning (PRD §5: warning, not \
             error — fall through to default kernel selection)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::KernelPragmaUnsatisfiable),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers (task ο wiring + LSP / MCP)"
        );
    }

    /// Pins the user-visible-content requirement: the message must name the
    /// pragma kernel, the op (Debug-rendered), and the demanded repr so the
    /// user can see which `#kernel(...)` preference could not be honoured.
    /// Asserts only `contains()` — wording-churn-resistant per the
    /// long-chain precedent.
    #[test]
    fn kernel_pragma_unsatisfiable_diagnostic_message_names_pragma_op_demanded() {
        let diag = kernel_pragma_unsatisfiable_diagnostic(
            "manifold",
            Operation::BooleanUnion,
            ReprKind::Mesh,
        );

        for needle in ["manifold", "BooleanUnion", "Mesh"] {
            assert!(
                diag.message.contains(needle),
                "diagnostic message must surface {:?} so the user can see \
                 which pragma preference was unmet (got: {:?})",
                needle,
                diag.message,
            );
        }
    }

    /// Pins the wire-contract of [`pinned_kernel_missing_diagnostic`]:
    /// `Severity::Error` + `Some(DiagnosticCode::PinnedKernelMissing)`.
    /// Error per PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5 "error;
    /// engine refuses to start". Consumed by task π (ID 3444).
    #[test]
    fn pinned_kernel_missing_diagnostic_carries_error_severity_and_code() {
        use reify_core::{DiagnosticCode, Severity};

        let diag = pinned_kernel_missing_diagnostic("truck");

        assert_eq!(
            diag.severity,
            Severity::Error,
            "diagnostic severity must be Error (PRD §5: error; engine \
             refuses to start — the determinism contract requires every \
             pinned kernel to be present)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::PinnedKernelMissing),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers (task π wiring + LSP / MCP)"
        );
    }

    /// Pins the user-visible-content requirement: the message must name the
    /// missing pinned kernel so the user can see which `reify.toml` pin is
    /// unsatisfied. Asserts only `contains()` — wording-churn-resistant.
    #[test]
    fn pinned_kernel_missing_diagnostic_message_names_kernel_id() {
        let diag = pinned_kernel_missing_diagnostic("truck");

        assert!(
            diag.message.contains("truck"),
            "diagnostic message must surface the missing pinned kernel id \
             so the user can see which reify.toml pin is unsatisfied \
             (got: {:?})",
            diag.message,
        );
    }

    /// Pins the wire-contract of [`unpinned_kernel_loaded_diagnostic`]:
    /// `Severity::Warning` + `Some(DiagnosticCode::UnpinnedKernelLoaded)`.
    /// Warning per PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5
    /// "warning; engine starts" — the kernel is usable; the missing pin
    /// only weakens the determinism contract. Consumed by task π (ID 3444).
    #[test]
    fn unpinned_kernel_loaded_diagnostic_carries_warning_severity_and_code() {
        use reify_core::{DiagnosticCode, Severity};

        let diag = unpinned_kernel_loaded_diagnostic("fidget");

        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic severity must be Warning (PRD §5: warning; engine \
             starts — the kernel is usable, the missing pin only weakens \
             the determinism contract)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::UnpinnedKernelLoaded),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers (task π wiring + LSP / MCP)"
        );
    }

    /// Pins the user-visible-content requirement: the message must name the
    /// unpinned kernel so the user can see which kernel to add to
    /// `reify.toml` for build determinism. Asserts only `contains()`.
    #[test]
    fn unpinned_kernel_loaded_diagnostic_message_names_kernel_id() {
        let diag = unpinned_kernel_loaded_diagnostic("fidget");

        assert!(
            diag.message.contains("fidget"),
            "diagnostic message must surface the unpinned kernel id so the \
             user can see which kernel to pin for build determinism \
             (got: {:?})",
            diag.message,
        );
    }

    /// Pins the wire-contract of [`kernel_version_mismatch_diagnostic`]:
    /// `Severity::Error` + `Some(DiagnosticCode::KernelVersionMismatch)`.
    /// Error per PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §5 "error.
    /// Determinism contract enforcement" — matching versions is
    /// load-bearing for reproducible realization. Consumed by task π
    /// (ID 3444).
    #[test]
    fn kernel_version_mismatch_diagnostic_carries_error_severity_and_code() {
        use reify_core::{DiagnosticCode, Severity};

        let diag = kernel_version_mismatch_diagnostic("manifold", "1.2.0", "1.3.0");

        assert_eq!(
            diag.severity,
            Severity::Error,
            "diagnostic severity must be Error (PRD §5: error — \
             determinism contract enforcement; the engine fails closed \
             rather than using a different adapter than the project pins)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::KernelVersionMismatch),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers (task π wiring + LSP / MCP)"
        );
    }

    /// Pins the user-visible-content requirement: the message must name the
    /// kernel id, the pinned version, and the actual adapter version so the
    /// user can see exactly which pin is unsatisfied and by how much.
    /// Asserts only `contains()` — wording-churn-resistant.
    #[test]
    fn kernel_version_mismatch_diagnostic_message_names_kernel_and_versions() {
        let diag = kernel_version_mismatch_diagnostic("manifold", "1.2.0", "1.3.0");

        assert!(
            diag.message.contains("manifold"),
            "diagnostic message must surface the kernel id (got: {:?})",
            diag.message,
        );
        assert!(
            diag.message.contains("1.2.0"),
            "diagnostic message must surface the pinned reify.toml version \
             (got: {:?})",
            diag.message,
        );
        assert!(
            diag.message.contains("1.3.0"),
            "diagnostic message must surface the actual adapter VERSION \
             (got: {:?})",
            diag.message,
        );
    }
}
