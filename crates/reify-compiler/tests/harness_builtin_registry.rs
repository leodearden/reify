//! Consolidated integration-test harness for the builtin-signature registry's
//! COMPILER seam (`crates/reify-builtins` ← `src/builtin_registry.rs`).
//!
//! Task #6001 (registry α; PRD `docs/prds/v0_6/builtin-signature-registry.md`
//! §7.3(2)). The registry's compiler-side pin is landed HERE, as a C1-sanctioned
//! compile unit, rather than as a top-level standalone
//! `tests/registry_seed_result_types.rs`. That standalone form was flagged
//! `reason=unregistered-standalone` by
//! `scripts/check-harness-baseline-registration.sh`; the sanctioned remedy is
//! consolidation, NOT a new `tests/infra/harness-layout-baseline.manifest`
//! grandfather row (SUPERSEDED — Leo 2026-07-22, esc-5056-11: the baseline is a
//! shrinking ratchet, not an allow-list to grow). No `#[test]` fn is added or
//! removed relative to the standalone form (invariant I3); the file keeps its
//! original stem, so its post-consolidation selector is
//! `registry_seed_result_types::<test>` and the binary id is
//! `reify-compiler::harness_builtin_registry`.
//!
//! This is the pre-registered home for the registry's later compiler-seam leaves
//! (β's `BuiltinId`-carrying `CompiledExpr`, ω's workspace-wide grep gate) — add
//! a module here rather than a second registry-subsystem harness.
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline
//! ratchet): see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not
//! restated here. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the
//! sibling `tests/<file>.rs`, not the `harness_builtin_registry/` subdir. The
//! shared `common` helper is declared ONCE here (via `#[path = "common/mod.rs"]`,
//! the harness_langcore.rs / harness_units.rs precedent) because
//! registry_seed_result_types.rs uses it; absorbed modules import it as `use
//! crate::common::…` rather than declaring their own `mod common;` (which would
//! load the same source file twice in this one compile unit —
//! `clippy::duplicate_mod` rejects that).
#[path = "common/mod.rs"]
mod common;

#[path = "harness_builtin_registry/registry_seed_result_types.rs"]
mod registry_seed_result_types;
