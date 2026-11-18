//! Consolidated integration-test harness for the `reify check` subcommand's family, split
//! out of `harness_cli` by task #10100 under rule (a) — the SPLIT remedy, never a CAP_LINES
//! bump.
//!
//! Membership is a rule you can check: a member's stem is `cli_check` or begins
//! `cli_check_`, AND it consumes the shared `tests/common/` helpers (one that consumes none
//! belongs in `harness_cli_surface`). The full ordered placement rule for a new reify-cli
//! integration test lives in `harness_cli.rs`'s header; layout contract C1 lives in the
//! `tests/infra/test_harness_kloc_cap.sh` header.
//!
//! Layout-only (invariant I3): no `#[test]` fn is added, removed or renamed by this split, and
//! every module keeps its stem. Only the binary id moves, from `reify-cli::harness_cli` to
//! `reify-cli::harness_cli_check`. `#[path]` is mandatory: this is an integration-test crate
//! root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`.

mod common;

#[path = "harness_cli_check/cli_check.rs"]
mod cli_check;
#[path = "harness_cli_check/cli_check_bare_length_exit.rs"]
mod cli_check_bare_length_exit;
#[path = "harness_cli_check/cli_check_cfg.rs"]
mod cli_check_cfg;
#[path = "harness_cli_check/cli_check_cfg_example.rs"]
mod cli_check_cfg_example;
#[path = "harness_cli_check/cli_check_connect_direction.rs"]
mod cli_check_connect_direction;
#[path = "harness_cli_check/cli_check_connect_undeclared_member.rs"]
mod cli_check_connect_undeclared_member;
#[path = "harness_cli_check/cli_check_driver_parity.rs"]
mod cli_check_driver_parity;
#[path = "harness_cli_check/cli_check_import_units.rs"]
mod cli_check_import_units;
#[path = "harness_cli_check/cli_check_joint_dof_gates.rs"]
mod cli_check_joint_dof_gates;
#[path = "harness_cli_check/cli_check_parametric_rate.rs"]
mod cli_check_parametric_rate;
#[path = "harness_cli_check/cli_check_parametric_vec3.rs"]
mod cli_check_parametric_vec3;
#[path = "harness_cli_check/cli_check_qualified_ref_positions.rs"]
mod cli_check_qualified_ref_positions;
#[path = "harness_cli_check/cli_check_relate_family_exit.rs"]
mod cli_check_relate_family_exit;
#[path = "harness_cli_check/cli_check_result_prelude.rs"]
mod cli_check_result_prelude;
#[path = "harness_cli_check/cli_check_variant_construction.rs"]
mod cli_check_variant_construction;
