//! Invariant: every unit `-p <covered member>` compiles under a key that
//! `--workspace` also compiles carries one of that key's `--workspace` profiles.

use std::path::Path;

use crate::harness::{
    Fixture, TREE_A, TREE_UNION, assert_findings, assert_green, assert_input_error, covered_a,
    describe, stdout_of, workspace_of,
};

const B_PKG_ID: &str = "path+file:///fixture/crates/b#0.1.0";

/// One `cargo test --unit-graph` unit: library target `target` of `pkg_id`,
/// built in mode `build` with `features`, under a dev profile whose only
/// varying field is `debuginfo`.
fn unit(pkg_id: &str, target: &str, features: &[&str], debuginfo: u32) -> String {
    let features: Vec<String> = features.iter().map(|f| format!("\"{f}\"")).collect();
    format!(
        "{{\"pkg_id\": \"{pkg_id}\", \
         \"target\": {{\"kind\": [\"lib\"], \"crate_types\": [\"lib\"], \"name\": \"{target}\"}}, \
         \"profile\": {{\"name\": \"dev\", \"opt_level\": \"0\", \"debuginfo\": {debuginfo}, \
         \"debug_assertions\": true, \"strip\": {{\"deferred\": \"None\"}}}}, \
         \"platform\": null, \"mode\": \"build\", \"features\": [{}], \"dependencies\": []}}",
        features.join(", ")
    )
}

fn graph_of(units: &[String]) -> String {
    format!(
        "{{\"version\": 1, \"units\": [{}], \"roots\": [0]}}",
        units.join(", ")
    )
}

/// `-p a` resolves exactly like `--workspace` in `cargo tree`, so any finding
/// comes from the unit graphs.
fn feature_clean_fixture() -> Fixture {
    Fixture::new(&covered_a(), &workspace_of(&[TREE_A, TREE_UNION])).tree("a", TREE_A)
}

#[test]
fn check_reds_on_a_profile_split_naming_the_unit_and_both_profiles() {
    let out = feature_clean_fixture()
        .unit_graph("workspace", &graph_of(&[unit(B_PKG_ID, "b", &[], 2)]))
        .unit_graph("a", &graph_of(&[unit(B_PKG_ID, "b", &[], 0)]))
        .check();
    assert_findings(
        &out,
        &[&["PROFILE-SPLIT a", B_PKG_ID, "debuginfo", "0", "2"]],
        "-p a compiles b at debuginfo 0, --workspace at 2",
    );
    assert!(
        stdout_of(&out).contains("[profile.dev.build-override]"),
        "a PROFILE-SPLIT must name the remedy\n{}",
        describe(&out)
    );
}

#[test]
fn check_reports_an_unreadable_unit_graph_as_an_input_error() {
    let out = feature_clean_fixture()
        .unit_graph("a", r#"{"version":2,"units":[],"roots":[]}"#)
        .check();
    assert_input_error(
        &out,
        &["--unit-graph", "version"],
        "a unit graph of an unknown format version",
    );
}

#[test]
fn check_resolves_unit_graphs_frozen_bootstrapped_in_a_scratch_target_dir() {
    let fixture = feature_clean_fixture();
    let out = fixture.check();
    let log = fixture.cargo_argv_log();
    let tests: Vec<&str> = log
        .lines()
        .filter(|line| line.starts_with("test"))
        .collect();
    assert!(
        tests.iter().any(|line| {
            line.contains("--frozen")
                && line.contains("--unit-graph")
                && line.contains("--workspace")
        }) && tests.iter().any(|line| line.contains("-p a")),
        "check must resolve the --workspace and -p a unit graphs with --frozen\n{log}\n{}",
        describe(&out)
    );
    assert!(
        !tests
            .iter()
            .any(|line| line.contains("-p workspace-feature-union")),
        "check must not resolve the union crate's own unit graph\n{log}"
    );

    let env_log = fixture.cargo_env_log();
    let fixture_dir = fixture.path("");
    let scratch_target = |line: &str| {
        line.split_once("CARGO_TARGET_DIR=")
            .map(|(_, dir)| dir.trim())
            .filter(|dir| !dir.is_empty() && !Path::new(dir).starts_with(&fixture_dir))
            .is_some()
    };
    assert!(
        !env_log.is_empty()
            && env_log
                .lines()
                .all(|line| line.contains("RUSTC_BOOTSTRAP=1") && scratch_target(line)),
        "every unit-graph resolution must run bootstrapped in a scratch target dir outside \
         the workspace\n{env_log}\n{}",
        describe(&out)
    );
}

/// A key `--workspace` never compiles differs in features, which SPLIT and
/// MEMBER-SPLIT cover; it is never also a PROFILE-SPLIT.
#[test]
fn check_ignores_a_unit_whose_features_the_workspace_never_compiles() {
    let out = feature_clean_fixture()
        .unit_graph("workspace", &graph_of(&[unit(B_PKG_ID, "b", &[], 2)]))
        .unit_graph("a", &graph_of(&[unit(B_PKG_ID, "b", &["x"], 0)]))
        .check();
    assert_green(
        &out,
        "-p a compiles b with features --workspace never compiles",
    );
}

/// Without `--target` a host copy and a runtime copy of one package share a
/// key, so `--workspace` may compile the key under two profiles.
#[test]
fn check_accepts_any_profile_the_workspace_compiles_for_the_key() {
    let out = feature_clean_fixture()
        .unit_graph(
            "workspace",
            &graph_of(&[unit(B_PKG_ID, "b", &[], 0), unit(B_PKG_ID, "b", &[], 2)]),
        )
        .unit_graph("a", &graph_of(&[unit(B_PKG_ID, "b", &[], 0)]))
        .check();
    assert_green(&out, "--workspace compiles b at debuginfo 0 and at 2");
}
