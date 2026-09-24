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
//! so they get a named root of their own. Padding it toward the band with the obvious
//! semantic neighbours (`rigid_mass_props_autoderive_gui_path`,
//! `rigid_moment_of_inertia_autoderive_smoke`, `forward_kinematics_e2e`) would be scope
//! creep past EVAL-3's enumerated prefixes, and merging it into `harness_mechanism` would
//! blur a subsystem boundary EVAL-1 drew. The cost of keeping it honest is one binary
//! saved instead of two.
//!
//! WHAT #4935 ACTUALLY PROMISES — stated precisely, because this root's size
//! justification leans on it and an earlier draft of this header overstated it. #4935
//! (PRD W2 / D-B, status pending as of this commit) moves
//! `crates/reify-eval/src/{dynamics_ops,dynamics_psd,trajectory_ops,…}.rs` plus their
//! CO-LOCATED `#[cfg(test)]` unit tests into a new `reify-eval-fea` crate. Its file list
//! carries no `crates/reify-eval/tests/*.rs` entry: neither `dynamics_body_mass_props.rs`
//! nor `trajectory_gcode_dialect_eval.rs` is in scope for it, and relocating them
//! afterwards would be CROSS-CRATE test motion, which the C1 contract puts explicitly out
//! of contract (invariant I2, `tests/infra/test_harness_kloc_cap.sh`). So #4935 makes a
//! future `reify-eval-fea` integration-test crate PLAUSIBLE — a natural home for these
//! two modules once the solver source lives there — it does not plan one, and it will
//! not move these files itself. This root is a convenient landing zone, not a promised
//! one.
//!
//! RETIRE CONDITION — that plausibility is the only standing justification for a root
//! this far below the PRD §7 10-20 kLOC band, and nothing in CI will notice if it dies:
//! `test_harness_kloc_cap.sh` rule (a) only fails units that are too LARGE, so an
//! orphaned small root is not self-correcting. If #4935 is cancelled or reshaped so that
//! no `reify-eval-fea` landing zone is coming — or lands without one — fold these two
//! modules into `harness_mechanism` and delete this root rather than leaving it stranded.
//! EVAL-3 could not perform that fold itself: `harness_mechanism.rs` is outside this
//! task's module locks, so it is left as a follow-up rather than done here.
//!
//! Whole-unit size (this root plus every `harness_dynamics/*.rs` module below) is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
#[path = "harness_dynamics/dynamics_body_mass_props.rs"]
mod dynamics_body_mass_props;
#[path = "harness_dynamics/trajectory_gcode_dialect_eval.rs"]
mod trajectory_gcode_dialect_eval;
