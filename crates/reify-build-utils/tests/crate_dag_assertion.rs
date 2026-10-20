//! The two crate-graph gates over the workspace:
//! - `scripts/assert-crate-dag.sh` B1–B6;
//! - `scripts/assert-crate-strata.py` over `scripts/crate-strata.json`, driven
//!   through its CLI against hermetic fixtures and the real workspace (PRD
//!   docs/prds/v0_6/kernel-interface-typed-seam.md §8 γ).

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Workspace-wide DAG invariant gate.
///
/// Invokes `scripts/assert-crate-dag.sh` and asserts:
///   1. The script exits 0 (all invariants pass).
///   2. Every B-check line (`B1 OK` … `B6 OK`) appears in stdout, confirming
///      each individual invariant was exercised and passed.
///
/// This is the integration gate for task η of
/// docs/prds/core-ast-ir-layering.md.
///
/// `scripts/reify_path_attribution.py` names this crate in `DAG_GATE_CRATE`
/// so a per-crate manifest diff pulls this gate into a narrowed, task-scoped
/// verify. Moving this gate to another crate means updating that constant.
#[test]
fn workspace_dag_invariant_via_assert_script() {
    use std::path::Path;
    use std::process::Command;

    // CARGO_MANIFEST_DIR resolves to `crates/reify-build-utils/` at test time.
    // Two levels up reaches the workspace root.
    // Use the runtime env var (set by cargo/nextest per test-run) rather than the
    // compile-time env!() macro, which bakes in the build-time worktree path and
    // breaks when test binaries are reused across worktrees (warm-lane CoW pool).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
    let workspace_root = Path::new(&manifest_dir)
        .parent() // crates/
        .expect("parent of CARGO_MANIFEST_DIR")
        .parent() // workspace root
        .expect("workspace root");

    let script = workspace_root.join("scripts/assert-crate-dag.sh");

    let output = Command::new("bash")
        .arg(&script)
        .current_dir(workspace_root)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to invoke assert-crate-dag.sh at {}: {e}\n\
                 (script must exist at scripts/assert-crate-dag.sh relative to workspace root)",
                script.display()
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Check each invariant is present and marked OK in stdout.
    let required_markers = ["B1 OK", "B2 OK", "B3 OK", "B4 OK", "B5 OK", "B6 OK"];
    let missing: Vec<&str> = required_markers
        .iter()
        .copied()
        .filter(|marker| !stdout.contains(marker))
        .collect();

    if !output.status.success() || !missing.is_empty() {
        panic!(
            "assert-crate-dag.sh failed.\n\
             Exit status: {}\n\
             Missing markers: {:?}\n\
             --- stdout ---\n{}\n--- stderr ---\n{}",
            output.status, missing, stdout, stderr
        );
    }
}

// --- crate strata: scripts/assert-crate-strata.py ---------------------------

fn workspace_root() -> PathBuf {
    // The runtime variable, not `env!`: a warm-lane test binary can be reused
    // from another worktree, and `env!` bakes in the build-time path.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR is crates/<crate>, two levels below the workspace root")
        .to_path_buf()
}

fn run_strata<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let script = workspace_root().join("scripts/assert-crate-strata.py");
    Command::new("python3")
        .arg("-I")
        .arg(&script)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn python3 {}: {e}", script.display()))
}

fn describe(out: &Output) -> String {
    format!(
        "{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn line_has_all(line: &str, needles: &[&str]) -> bool {
    needles.iter().all(|needle| line.contains(needle))
}

fn has_line_with_all(text: &str, needles: &[&str]) -> bool {
    text.lines().any(|line| line_has_all(line, needles))
}

/// Every stdout line except the `crate-strata:` summary.
fn finding_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| !line.starts_with("crate-strata:"))
        .collect()
}

fn assert_exit(out: &Output, expected: i32, context: &str) {
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{context}: expected exit {expected}\n{}",
        describe(out)
    );
}

fn assert_green(out: &Output, context: &str) {
    assert_exit(out, 0, context);
    assert!(
        has_line_with_all(&stdout_of(out), &["crate-strata: OK"]),
        "{context}: expected a `crate-strata: OK` summary\n{}",
        describe(out)
    );
}

/// Exit 1 with exactly one finding line per entry of `expected`, each line
/// holding every needle of its entry.
fn assert_findings(out: &Output, expected: &[&[&str]], context: &str) {
    assert_exit(out, 1, context);
    let stdout = stdout_of(out);
    let lines = finding_lines(&stdout);
    assert_eq!(
        lines.len(),
        expected.len(),
        "{context}: expected {} finding lines\n{}",
        expected.len(),
        describe(out)
    );
    for needles in expected {
        let matching = lines
            .iter()
            .filter(|line| line_has_all(line, needles))
            .count();
        assert_eq!(
            matching,
            1,
            "{context}: expected exactly one finding holding {needles:?}\n{}",
            describe(out)
        );
    }
}

#[derive(Clone, Copy)]
enum DepKind {
    Normal,
    Build,
    Dev,
}

#[derive(Clone)]
struct Dep {
    name: &'static str,
    kind: DepKind,
    optional: bool,
    target: Option<&'static str>,
}

impl Dep {
    fn new(name: &'static str, kind: DepKind) -> Dep {
        Dep {
            name,
            kind,
            optional: false,
            target: None,
        }
    }
}

#[derive(Clone)]
struct Pkg {
    name: &'static str,
    deps: Vec<Dep>,
}

fn pkg(name: &'static str, normal_deps: &[&'static str]) -> Pkg {
    Pkg {
        name,
        deps: normal_deps
            .iter()
            .map(|dep| Dep::new(dep, DepKind::Normal))
            .collect(),
    }
}

impl Pkg {
    fn dep(mut self, dep: Dep) -> Pkg {
        self.deps.push(dep);
        self
    }
}

fn package_mut<'a>(pkgs: &'a mut [Pkg], name: &str) -> &'a mut Pkg {
    pkgs.iter_mut()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("fixture has no package {name}"))
}

fn with_dep(mut pkgs: Vec<Pkg>, from: &str, dep: Dep) -> Vec<Pkg> {
    package_mut(&mut pkgs, from).deps.push(dep);
    pkgs
}

fn without_dep(mut pkgs: Vec<Pkg>, from: &str, to: &str) -> Vec<Pkg> {
    let deps = &mut package_mut(&mut pkgs, from).deps;
    let before = deps.len();
    deps.retain(|d| d.name != to);
    assert_ne!(before, deps.len(), "fixture has no edge {from} -> {to}");
    pkgs
}

fn without_package(pkgs: Vec<Pkg>, name: &str) -> Vec<Pkg> {
    let before = pkgs.len();
    let pkgs: Vec<Pkg> = pkgs.into_iter().filter(|p| p.name != name).collect();
    assert_ne!(before, pkgs.len(), "fixture has no package {name}");
    pkgs
}

fn package_id(name: &str) -> String {
    format!("{name} 0.1.0 (path+file:///fixture/{name})")
}

fn dependency_json(dep: &Dep) -> String {
    let kind = match dep.kind {
        DepKind::Normal => "null",
        DepKind::Build => "\"build\"",
        DepKind::Dev => "\"dev\"",
    };
    let target = dep
        .target
        .map_or_else(|| "null".to_string(), |t| format!("\"{t}\""));
    format!(
        "{{\"name\": \"{}\", \"kind\": {kind}, \"optional\": {}, \"target\": {target}}}",
        dep.name, dep.optional
    )
}

/// The subset of `cargo metadata --no-deps --format-version 1` the checker reads.
fn metadata_json(pkgs: &[Pkg]) -> String {
    let packages = pkgs
        .iter()
        .map(|p| {
            let deps = p.deps.iter().map(dependency_json).collect::<Vec<_>>();
            format!(
                "{{\"name\": \"{}\", \"id\": \"{}\", \"dependencies\": [{}]}}",
                p.name,
                package_id(p.name),
                deps.join(", ")
            )
        })
        .collect::<Vec<_>>();
    let members = pkgs
        .iter()
        .map(|p| format!("\"{}\"", package_id(p.name)))
        .collect::<Vec<_>>();
    format!(
        "{{\"packages\": [{}], \"workspace_members\": [{}], \"resolve\": null}}\n",
        packages.join(",\n"),
        members.join(", ")
    )
}

struct StrataFixture {
    dir: tempfile::TempDir,
}

impl StrataFixture {
    fn new(table: &str, pkgs: &[Pkg]) -> Self {
        let fixture = StrataFixture {
            dir: tempfile::tempdir().expect("create strata fixture dir"),
        };
        fixture.write("table.json", table);
        fixture.write("metadata.json", &metadata_json(pkgs));
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.path(name);
        fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    fn check(&self) -> Output {
        run_strata([
            OsStr::new("--table"),
            self.path("table.json").as_os_str(),
            OsStr::new("--metadata"),
            self.path("metadata.json").as_os_str(),
        ])
    }
}

const FIXTURE_TABLE: &str = r#"{
  "schema": 1,
  "strata": [
    {"name": "spine", "may_depend_on": [], "packages": ["reify-core", "reify-ir"]},
    {"name": "language", "may_depend_on": ["spine"], "packages": ["reify-compiler", "reify-expr"]},
    {"name": "kernel", "may_depend_on": ["spine"], "packages": ["reify-kernel-gmsh", "reify-kernel-x"]},
    {"name": "solver", "may_depend_on": ["spine"], "packages": ["reify-compute-contract", "reify-solver-elastic"]},
    {"name": "engine", "may_depend_on": ["spine", "language", "kernel", "solver"], "packages": ["reify-eval"]},
    {"name": "test-support", "may_depend_on": ["spine", "language", "kernel", "solver", "engine"], "packages": ["reify-test-support"]}
  ],
  "crate_rules": [
    {"name": "kernel-crate-deps", "from": {"packages": ["reify-kernel-gmsh", "reify-kernel-x"]}, "only": {"packages": ["reify-core", "reify-ir"]}},
    {"name": "compute-contract-spine-only", "from": {"packages": ["reify-compute-contract"]}, "only": {"strata": ["spine"]}},
    {"name": "expr-compiler-independent", "from": {"packages": ["reify-compiler", "reify-expr"]}, "never": {"packages": ["reify-compiler", "reify-expr"]}},
    {"name": "production-not-on-test-support", "from": {"strata": ["spine", "language", "kernel", "solver", "engine"]}, "never": {"strata": ["test-support"]}}
  ],
  "exceptions": [
    {"from": "reify-solver-elastic", "to": "reify-kernel-gmsh", "task": 8289, "note": "fixture: excepted solver -> kernel edge"}
  ]
}
"#;

/// FIXTURE_TABLE with the one occurrence of `from` replaced by `to`.
fn fixture_table_with(from: &str, to: &str) -> String {
    assert_eq!(
        FIXTURE_TABLE.matches(from).count(),
        1,
        "FIXTURE_TABLE must hold {from:?} exactly once"
    );
    FIXTURE_TABLE.replacen(from, to, 1)
}

/// A graph FIXTURE_TABLE accepts, with dev back-edges and the excepted edge.
fn clean_workspace() -> Vec<Pkg> {
    vec![
        pkg("reify-core", &[]),
        pkg("reify-ir", &["reify-core"]),
        pkg("reify-compiler", &["reify-core", "reify-ir"]),
        pkg("reify-expr", &["reify-ir"]),
        pkg("reify-kernel-x", &["reify-core", "reify-ir"])
            .dep(Dep::new("reify-eval", DepKind::Dev)),
        pkg("reify-kernel-gmsh", &["reify-core", "reify-ir"]),
        pkg("reify-solver-elastic", &["reify-ir", "reify-kernel-gmsh"]),
        pkg("reify-compute-contract", &["reify-core", "reify-ir"]),
        pkg(
            "reify-eval",
            &[
                "reify-compiler",
                "reify-expr",
                "reify-kernel-x",
                "reify-solver-elastic",
                "reify-compute-contract",
            ],
        )
        .dep(Dep::new("reify-test-support", DepKind::Dev)),
        pkg("reify-test-support", &["reify-eval", "reify-compiler"]),
    ]
}

fn check(table: &str, pkgs: &[Pkg]) -> Output {
    StrataFixture::new(table, pkgs).check()
}

#[test]
fn strata_clean_fixture_is_green() {
    let out = check(FIXTURE_TABLE, &clean_workspace());
    assert_green(
        &out,
        "dev back-edges are not counted and the excepted edge passes",
    );
}

#[test]
fn strata_new_kernel_to_engine_edge_reds_naming_the_edge_and_rule() {
    let graph = with_dep(
        clean_workspace(),
        "reify-kernel-x",
        Dep::new("reify-eval", DepKind::Normal),
    );
    assert_findings(
        &check(FIXTURE_TABLE, &graph),
        &[
            &[
                "VIOLATION reify-kernel-x -> reify-eval",
                "stratum rule",
                "reify-eval is engine",
            ],
            &[
                "VIOLATION reify-kernel-x -> reify-eval",
                "crate rule kernel-crate-deps",
                "may depend only on",
            ],
        ],
        "a new normal kernel -> engine edge",
    );
}

#[test]
fn strata_new_solver_to_kernel_edge_reds_naming_the_edge_and_rule() {
    let graph = with_dep(
        clean_workspace(),
        "reify-solver-elastic",
        Dep::new("reify-kernel-x", DepKind::Normal),
    );
    assert_findings(
        &check(FIXTURE_TABLE, &graph),
        &[&[
            "VIOLATION reify-solver-elastic -> reify-kernel-x",
            "stratum rule",
            "reify-kernel-x is kernel",
        ]],
        "a new solver -> kernel edge that no exception names",
    );
}

#[test]
fn strata_unclassified_package_reds_naming_the_crate() {
    let mut graph = clean_workspace();
    graph.push(pkg("reify-newcrate", &["reify-core"]));
    assert_findings(
        &check(FIXTURE_TABLE, &graph),
        &[&["UNCLASSIFIED reify-newcrate"]],
        "a workspace package no stratum lists",
    );
}

#[test]
fn strata_stale_exception_reds_naming_the_row() {
    let stale = "STALE exception reify-solver-elastic -> reify-kernel-gmsh (#8289)";

    let dropped = without_dep(
        clean_workspace(),
        "reify-solver-elastic",
        "reify-kernel-gmsh",
    );
    assert_findings(
        &check(FIXTURE_TABLE, &dropped),
        &[&[stale, "not in the graph"]],
        "the excepted edge left the graph",
    );

    let loosened = fixture_table_with(
        r#"{"name": "solver", "may_depend_on": ["spine"]"#,
        r#"{"name": "solver", "may_depend_on": ["spine", "kernel"]"#,
    );
    assert_findings(
        &check(&loosened, &clean_workspace()),
        &[&[stale, "allowed"]],
        "a table edit allowed the excepted edge",
    );
}

#[test]
fn strata_counts_normal_build_optional_and_target_edges_but_not_dev_edges() {
    let edge = |dep: Dep| {
        check(
            FIXTURE_TABLE,
            &with_dep(clean_workspace(), "reify-kernel-x", dep),
        )
    };
    let normal = Dep::new("reify-eval", DepKind::Normal);

    assert_green(
        &edge(Dep::new("reify-eval", DepKind::Dev)),
        "a dev kernel -> engine edge",
    );

    let reference = edge(normal.clone());
    assert_exit(&reference, 1, "a normal kernel -> engine edge");
    let reference_stdout = stdout_of(&reference);
    let expected = finding_lines(&reference_stdout);
    assert!(
        has_line_with_all(
            &reference_stdout,
            &["VIOLATION reify-kernel-x -> reify-eval", "stratum rule"]
        ),
        "{}",
        describe(&reference)
    );

    for (label, dep) in [
        ("a build edge", Dep::new("reify-eval", DepKind::Build)),
        (
            "an optional normal edge",
            Dep {
                optional: true,
                ..normal.clone()
            },
        ),
        (
            "a cfg(unix) target edge",
            Dep {
                target: Some("cfg(unix)"),
                ..normal.clone()
            },
        ),
    ] {
        let out = edge(dep);
        assert_exit(&out, 1, label);
        assert_eq!(
            finding_lines(&stdout_of(&out)),
            expected,
            "{label} reds exactly as a plain normal edge does\n{}",
            describe(&out)
        );
    }
}

#[test]
fn strata_a_classified_package_absent_from_the_workspace_reds() {
    let graph = without_package(
        without_dep(clean_workspace(), "reify-eval", "reify-compute-contract"),
        "reify-compute-contract",
    );
    assert_findings(
        &check(FIXTURE_TABLE, &graph),
        &[&["STALE stratum solver lists reify-compute-contract"]],
        "a classified package that left the workspace",
    );
}

#[test]
fn strata_crate_rules_fire_on_edges_the_stratum_table_allows() {
    for (from, to, needles) in [
        (
            "reify-kernel-x",
            "reify-kernel-gmsh",
            &["crate rule kernel-crate-deps"][..],
        ),
        (
            "reify-compute-contract",
            "reify-solver-elastic",
            &["crate rule compute-contract-spine-only"][..],
        ),
        (
            "reify-compiler",
            "reify-expr",
            &[
                "crate rule expr-compiler-independent",
                "must not depend on reify-expr",
            ][..],
        ),
    ] {
        let graph = with_dep(clean_workspace(), from, Dep::new(to, DepKind::Normal));
        let violation = format!("VIOLATION {from} -> {to}");
        let mut expected = vec![violation.as_str()];
        expected.extend_from_slice(needles);
        assert_findings(
            &check(FIXTURE_TABLE, &graph),
            &[&expected],
            &format!("a same-stratum {from} -> {to} edge"),
        );
    }
}

#[test]
fn strata_production_strata_must_not_depend_on_test_support() {
    let graph = with_dep(
        clean_workspace(),
        "reify-eval",
        Dep::new("reify-test-support", DepKind::Normal),
    );
    let violation = "VIOLATION reify-eval -> reify-test-support";
    assert_findings(
        &check(FIXTURE_TABLE, &graph),
        &[
            &[violation, "stratum rule", "engine may depend only on"],
            &[violation, "crate rule production-not-on-test-support"],
        ],
        "a normal engine -> test-support edge beside the clean graph's dev edge",
    );
}

#[test]
fn strata_one_exception_row_covers_every_rule_its_edge_breaks() {
    let row = r#"{"from": "reify-solver-elastic", "to": "reify-kernel-gmsh", "task": 8289, "note": "fixture: excepted solver -> kernel edge"}"#;
    let table = fixture_table_with(
        row,
        &format!(
            "{row},\n    {}",
            r#"{"from": "reify-kernel-x", "to": "reify-eval", "task": 8561, "note": "fixture"}"#
        ),
    );
    let graph = with_dep(
        clean_workspace(),
        "reify-kernel-x",
        Dep::new("reify-eval", DepKind::Normal),
    );
    assert_green(
        &check(&table, &graph),
        "one row excuses both the stratum rule and kernel-crate-deps",
    );
}
