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
//! This unit is deliberately SMALL — two modules — and that is not an oversight.
//! `dynamics_` and `trajectory_` are two of the prefix clusters EVAL-3 was asked to home,
//! so they get a named root of their own; it is also the pre-named landing zone for PRD
//! W2/B (#4935), which moves the `dynamics_*`/`trajectory_ops` solver source into a new
//! `reify-eval-fea` crate. Padding it toward the band with the obvious semantic
//! neighbours (`rigid_mass_props_autoderive_gui_path`,
//! `rigid_moment_of_inertia_autoderive_smoke`, `forward_kinematics_e2e`) would be scope
//! creep past EVAL-3's enumerated prefixes, and merging it into `harness_mechanism` would
//! blur a subsystem boundary EVAL-1 drew. The cost of keeping it honest is one binary
//! saved instead of two.
//!
//! RETIRE CONDITION — #4935 is the standing justification for a root this far below the
//! PRD §7 10-20 kLOC band, and nothing in CI will notice if that justification dies:
//! `test_harness_kloc_cap.sh` rule (a) only fails units that are too LARGE, so an
//! orphaned small root is not self-correcting. If #4935 is cancelled or reshaped so that
//! no `reify-eval-fea` landing zone is coming, fold these two modules into
//! `harness_mechanism` and delete this root rather than leaving it stranded.
//!
//! Whole-unit size (this root plus every `harness_dynamics/*.rs` module below) is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
#[path = "harness_dynamics/dynamics_body_mass_props.rs"]
mod dynamics_body_mass_props;
#[path = "harness_dynamics/trajectory_gcode_dialect_eval.rs"]
mod trajectory_gcode_dialect_eval;
