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
//! THE SEAM. These are the tests that exercise the `reify` executable's external surface —
//! how it is linked, what argv and help output it accepts and prints, and the machine-facing
//! protocols and interchange formats it speaks over stdio — none of which evaluates a `.ri`
//! design.
//!
//! That thematic seam is mechanically corroborated: it is exactly the set that needs none of
//! the shared `tests/common/` `.ri`-fixture helpers, so this root deliberately has NO
//! `mod common;` and the 312-line external include stays charged to `harness_cli` alone
//! rather than being compiled into a second unit and counted twice under the C2 cap.
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
