//! Consolidated integration-test harness for the dynamics / trajectory subsystem.
//!
//! Task #5282 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-3):
//! folds the former standalone `tests/<file>.rs` binaries for the `dynamics_` and
//! `trajectory_` clusters into this single compile unit to cut the merge-gate link
//! count. Layout-only — no `#[test]` fn is added or removed. Each former file is
//! included as a stem-named module so its `<file>::<test>` module path (and thus every
//! `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]` is required:
//! this harness root is an integration-test crate root, where a bare `mod <file>;` would
//! resolve to the sibling `tests/<file>.rs`, not the `harness_dynamics/` subdir.
//!
//! The move was pure relocation: neither file has an `include_str!`/`include!` site, a
//! `mod` or `#[path]` declaration, a `#[global_allocator]`, or a file-level inner
//! attribute, so no content fixup was needed.
//!
//! This unit is deliberately SMALL — two modules, 517 lines — and that is not an
//! oversight. `dynamics_` and `trajectory_` are two of the prefix clusters EVAL-3 was
//! asked to home, so they get a named root of their own; it is also the pre-named
//! landing zone for PRD W2/B (task 4935), which moves the `dynamics_*`/`trajectory_ops`
//! solver source into a new `reify-eval-fea` crate. Padding it toward the band with the
//! obvious semantic neighbours (`rigid_mass_props_autoderive_gui_path`,
//! `rigid_moment_of_inertia_autoderive_smoke`, `forward_kinematics_e2e`) would be scope
//! creep past EVAL-3's enumerated prefixes, and merging it into `harness_mechanism` would
//! blur a subsystem boundary EVAL-1 drew. The cost of keeping it honest is one binary
//! saved instead of two.
//!
//! Note (task #5282): this compile unit's real size is the root file plus every
//! `harness_dynamics/*.rs` module below — at consolidation time, 38 (root) + 517
//! (modules) = 555 raw lines against the PRD §7 20,000-line cap (~97% headroom).
//! `tests/infra/test_harness_kloc_cap.sh` rule (a) currently `wc -l`s only this root
//! file, so it cannot see a unit approach the cap; a follow-up to make the guard sum
//! root + module-dir LOC was filed against task 5281. Once that follow-up is assigned a
//! task id, replace this note with a properly numbered debt-marker citation per the
//! repo's TODO-citation convention (CLAUDE.md).
#[path = "harness_dynamics/dynamics_body_mass_props.rs"]
mod dynamics_body_mass_props;
#[path = "harness_dynamics/trajectory_gcode_dialect_eval.rs"]
mod trajectory_gcode_dialect_eval;
