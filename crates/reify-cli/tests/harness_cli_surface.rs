//! Consolidated integration-test harness for the `reify` executable's EXTERNAL SURFACE —
//! the family split out of `harness_cli` by task #7365.
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet): see
//! `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1/C2 — kept there, not restated here.
//! `harness_cli` measured 17954 lines (190 root + 17452 across 83 module files + 312
//! external) against CAP_LINES = 20000, i.e. 46 lines under the 90% warn line and crossing
//! on its next test-bearing commit. Rule (a)'s own remedy is a SPLIT into a second
//! `harness_<subsystem>.rs` — never a CAP_LINES bump, and never a
//! `tests/infra/harness-layout-baseline.manifest` grandfather row (SUPERSEDED — Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to grow).
//!
//! THE SEAM is a rule you can check, not a theme you have to judge: a test belongs here iff
//! it consumes NONE of the shared `tests/common/` helpers. `grep -rn 'common::'
//! crates/reify-cli/tests/harness_cli_surface/` is empty today and must stay empty. That
//! rule is also what the split buys — this root deliberately has NO `mod common;`, so the
//! 312-line external include stays charged to `harness_cli` alone instead of being compiled
//! into a second unit and counted twice under the C2 cap (measured: `external_files = 0`).
//!
//! What the rule selects, descriptively: the tests that exercise the `reify` executable's
//! external surface — how it is linked, what argv and help output it accepts and prints, and
//! the machine-facing protocols and interchange formats it speaks over stdio. Several
//! members DO drive a `.ri` fixture through that surface (`mcp_integration` and `cli_cache`
//! both run `tests/fixtures/bracket.ri`), so "evaluates no `.ri` design" is NOT the
//! criterion and never was: both harnesses share `tests/fixtures/`, which costs nothing
//! under C2 — it counts only `.rs` files reached by a `mod` / `#[path]` declaration.
//!
//! Layout-only (invariant I3): no `#[test]` fn is added, removed or renamed by this split,
//! and every module keeps its stem, so each `<file>::<test>` module path — and thus every
//! `test(/^<file>::/)`-shaped filterset — resolves exactly as before. Only the binary id
//! moves, from `reify-cli::harness_cli` to `reify-cli::harness_cli_surface`.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not into the
//! `harness_cli_surface/` subdir.
//!
//! `rpath_smoke` is linux-only: its former crate-level `#![cfg(target_os = "linux")]` is
//! hoisted to a `#[cfg(target_os = "linux")]` on its `mod` declaration below, since a
//! submodule cannot carry a crate-level inner attribute. The resulting
//! `#[cfg]` -> `#[path]` -> `mod` ordering is the live shape the kLOC guard's Section 10
//! moddir-boundary fixture models: a `#[cfg]`-gated member is a DECLARED member regardless
//! of cfg state.

#[path = "harness_cli_surface/cli_cache.rs"]
mod cli_cache;
#[path = "harness_cli_surface/cli_gui.rs"]
mod cli_gui;
#[path = "harness_cli_surface/cli_lsp.rs"]
mod cli_lsp;
#[path = "harness_cli_surface/cli_lsp_protocol.rs"]
mod cli_lsp_protocol;
#[path = "harness_cli_surface/cli_smoke.rs"]
mod cli_smoke;
#[path = "harness_cli_surface/mcp_integration.rs"]
mod mcp_integration;
#[cfg(target_os = "linux")]
#[path = "harness_cli_surface/rpath_smoke.rs"]
mod rpath_smoke;
