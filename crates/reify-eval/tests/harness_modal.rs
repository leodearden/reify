//! Consolidated integration-test harness for the **modal-analysis** subsystem.
//!
//! ## Why this unit exists (task #6878, PRD leaf β)
//!
//! Modal author-surface e2e tests had no home of their own: the FEA `modal_analysis`
//! producer's tests lived in `harness_fea_solver_e2e` and the lumped mechanism
//! producer's in `harness_mechanism`. Measured with the layout guard's own helper
//! (`tests/infra/harness-layout-lib.sh::harness_layout_unit_lines`),
//! `harness_fea_solver_e2e` is at **19429 lines against `CAP_LINES = 20000`** — only
//! 571 lines of headroom — while the landed `mechanism_modal_damping_e2e.rs` is 197
//! lines for a SINGLE test written in the house's doc-comment style. The damped-modal
//! PRD's drift-guard rider has β (#6878) *and* ζ (#6882) *and* η (#6883) each adding
//! gate-resident test files, so folding them into the FEA unit would leave the next two
//! leaves fighting over ~300 lines.
//!
//! The kLOC guard's own remedy for an over-cap unit is to SPLIT into a second
//! `harness_<subsystem>.rs` root, never to raise the cap, so opening the modal
//! subsystem's harness now is the sanctioned move rather than a workaround.
//!
//! ## Why no registration row is required
//!
//! `scripts/check-harness-baseline-registration.sh` skips `harness_*` basenames
//! (`harness_layout_in_scope_standalone`), and the baseline manifest's header states
//! that harness roots are governed by the kLOC cap (rule (a)), not by the
//! re-accretion rule. `.config/nextest.toml` already routes `package(reify-eval)`
//! wholesale into the `occt` test-group, so no per-binary partition row is needed
//! either. Both were re-verified empirically when this root landed.
//!
//! ## Layout contract
//!
//! Same form as `harness_mechanism.rs`: each absorbed file is a stem-named module
//! declared with an explicit `#[path]` (required at a crate root) so its
//! `<file>::<test>` module path stays stable. Per the #6096 precedent the stems need
//! not carry the `harness_`/`modal_` prefix — the naming contract keys on the harness
//! ROOT, not on the module stems it absorbs.

#[path = "harness_modal/modal_material_damping_e2e.rs"]
mod modal_material_damping_e2e;
