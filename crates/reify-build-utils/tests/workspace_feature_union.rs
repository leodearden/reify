//! Invariant: `cargo test -p <member>` resolves every external package to the
//! feature set the base's `--workspace` build compiles, because each member
//! depends on the `workspace-feature-union` crate. Gate and remedy:
//! `scripts/workspace-feature-union.py`; design: docs/notes/workspace-feature-union.md.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn script() -> PathBuf {
    workspace_root().join("scripts/workspace-feature-union.py")
}

const UNION: &str = "workspace-feature-union";

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

const FINDING_KINDS: [&str; 3] = ["SPLIT ", "UNCOVERED ", "HOST-MEMBER "];

fn finding_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| FINDING_KINDS.iter().any(|kind| line.starts_with(kind)))
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
    let stdout = stdout_of(out);
    assert!(
        finding_lines(&stdout).is_empty(),
        "{context}: expected no findings\n{}",
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

/// Exit 2 — an input could not be trusted — with stderr holding every needle.
fn assert_input_error(out: &Output, needles: &[&str], context: &str) {
    assert_exit(out, 2, context);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        line_has_all(&stderr, needles),
        "{context}: expected stderr to hold {needles:?}\n{}",
        describe(out)
    );
}

// --- fixtures ----------------------------------------------------------------

/// One workspace member of a fixture's `cargo metadata --no-deps`.
struct Member {
    name: &'static str,
    deps: Vec<(&'static str, Option<&'static str>)>,
}

fn member(name: &'static str, normal_deps: &[&'static str]) -> Member {
    Member {
        name,
        deps: normal_deps.iter().map(|dep| (*dep, None)).collect(),
    }
}

impl Member {
    fn build_dep(mut self, name: &'static str) -> Member {
        self.deps.push((name, Some("build")));
        self
    }
}

fn package_id(name: &str) -> String {
    format!("{name} 0.1.0 (path+file:///fixture/crates/{name})")
}

fn metadata_json(root: &Path, members: &[Member]) -> String {
    let packages = members
        .iter()
        .map(|m| {
            let deps = m
                .deps
                .iter()
                .map(|(name, kind)| {
                    let kind = kind.map_or_else(|| "null".to_string(), |k| format!("\"{k}\""));
                    format!("{{\"name\": \"{name}\", \"kind\": {kind}}}")
                })
                .collect::<Vec<_>>();
            format!(
                "{{\"name\": \"{}\", \"id\": \"{}\", \"manifest_path\": \"{}\", \"dependencies\": [{}]}}",
                m.name,
                package_id(m.name),
                manifest_path(root, m.name).display(),
                deps.join(", ")
            )
        })
        .collect::<Vec<_>>();
    let ids = members
        .iter()
        .map(|m| format!("\"{}\"", package_id(m.name)))
        .collect::<Vec<_>>();
    format!(
        "{{\"packages\": [{}], \"workspace_members\": [{}]}}\n",
        packages.join(",\n"),
        ids.join(", ")
    )
}

fn manifest_path(root: &Path, name: &str) -> PathBuf {
    root.join("crates").join(name).join("Cargo.toml")
}

/// A stand-in for cargo that serves the fixture's canned files by argv and
/// logs every argv to cargo-argv.log.
const STUB_CARGO: &str = r#"#!/usr/bin/env bash
set -u
here="$(cd "$(dirname "$0")" && pwd)"
printf '%s\n' "$*" >> "$here/cargo-argv.log"
case "${1:-}" in
  -vV)
    printf 'cargo 1.0.0 (stub)\nrelease: 1.0.0\nhost: x86_64-unknown-linux-gnu\n'
    exit 0 ;;
  metadata)
    exec cat "$here/metadata.json" ;;
  tree)
    if [ -e "$here/tree-fail" ]; then
      echo 'error: boom' >&2
      exit 101
    fi
    prev=""
    for arg in "$@"; do
      if [ "$arg" = "--workspace" ]; then
        exec cat "$here/tree-workspace.txt"
      fi
      if [ "$prev" = "-p" ]; then
        if [ -e "$here/tree-$arg.txt" ]; then
          exec cat "$here/tree-$arg.txt"
        fi
        echo "stub cargo: no fixture tree for -p $arg" >&2
        exit 3
      fi
      prev="$arg"
    done ;;
esac
echo "stub cargo: unexpected argv: $*" >&2
exit 3
"#;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(members: &[Member], workspace_tree: &str) -> Fixture {
        let fixture = Fixture {
            dir: tempfile::tempdir().expect("create workspace-feature-union fixture dir"),
        };
        fixture.write("cargo", STUB_CARGO);
        let stub = fixture.path("cargo");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("chmod {}: {e}", stub.display()));
        fixture.write("metadata.json", &metadata_json(fixture.dir.path(), members));
        fixture.write("tree-workspace.txt", workspace_tree);
        fixture
    }

    /// Serves `text` for `cargo tree -p <name>`.
    fn tree(self, name: &str, text: &str) -> Fixture {
        self.write(&format!("tree-{name}.txt"), text);
        self
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.path(name);
        fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    fn manifest(&self, name: &str) -> String {
        manifest_path(self.dir.path(), name).display().to_string()
    }

    fn run(&self, subcommand: &str) -> Output {
        Command::new("python3")
            .arg("-I")
            .arg(script())
            .arg(subcommand)
            .arg("--root")
            .arg(self.dir.path())
            .env("CARGO", self.path("cargo"))
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn python3 {}: {e}", script().display()))
    }

    fn check(&self) -> Output {
        self.run("check")
    }

    /// Every argv the stub cargo was invoked with, one per line.
    fn cargo_argv_log(&self) -> String {
        fs::read_to_string(self.path("cargo-argv.log")).unwrap_or_default()
    }
}

/// `-p a`: the union crate, a target-side serde and its proc-macro below it,
/// a deduped repeat, and the root's build- and dev-dependency sections.
const TREE_A: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- serde v1.0.0|derive,rc,std
|   `-- serde_derive v1.0.0 (proc-macro)|default
|       `-- syn v2.0.0|full
`-- memchr v2.0.0|std
    `-- serde v1.0.0|derive,rc,std (*)
[build-dependencies]
`-- cc v1.0.0|
[dev-dependencies]
`-- tempfile v3.0.0|default
";

const TREE_UNION: &str =
    "workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|\n";

/// `--workspace` over roots `a` and the union crate.
fn workspace_of(roots: &[&str]) -> String {
    roots.join("\n")
}

fn covered_a() -> Vec<Member> {
    vec![member("a", &[UNION]), member(UNION, &[])]
}

// --- check -------------------------------------------------------------------

#[test]
fn check_is_green_when_every_variant_occurs_on_the_same_side_in_the_workspace() {
    let fixture =
        Fixture::new(&covered_a(), &workspace_of(&[TREE_A, TREE_UNION])).tree("a", TREE_A);
    let out = fixture.check();
    assert_green(&out, "-p a resolves exactly like --workspace");

    let log = fixture.cargo_argv_log();
    let resolving: Vec<&str> = log
        .lines()
        .filter(|line| line.starts_with("tree") || line.starts_with("metadata"))
        .collect();
    assert!(
        !resolving.is_empty() && resolving.iter().all(|line| line.contains("--frozen")),
        "check must resolve with --frozen only, never writing Cargo.lock\n{log}\n{}",
        describe(&out)
    );
    assert!(
        !log.lines()
            .any(|line| line.starts_with("tree") && line.contains("-p workspace-feature-union")),
        "check must not resolve the union crate itself\n{log}"
    );
}

#[test]
fn check_reds_on_a_target_side_feature_split_naming_both_feature_lists() {
    let tree_a = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- serde v1.0.0|derive,std
";
    let ws_a = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- serde v1.0.0|derive,rc,std
";
    let out = Fixture::new(&covered_a(), &workspace_of(&[ws_a, TREE_UNION]))
        .tree("a", tree_a)
        .check();
    assert_findings(
        &out,
        &[&[
            "SPLIT a ",
            "serde v1.0.0",
            "(target)",
            "{derive,std}",
            "{derive,rc,std}",
        ]],
        "-p a resolves target serde without rc",
    );
    assert!(
        stdout_of(&out).contains("python3 scripts/workspace-feature-union.py generate"),
        "a SPLIT finding must print the generate remedy\n{}",
        describe(&out)
    );
}

/// `-p a` reaches syn only under a child's `[build-dependencies]` header and
/// quote only below a proc-macro; tempfile is a dev-dependency listed after
/// the root's own `[build-dependencies]` section.
const TREE_A_HOST_ONLY: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- b v0.1.0 (/fixture/crates/b)|
|   [build-dependencies]
|   `-- syn v2.0.0|full
`-- derive-thing v1.0.0 (proc-macro)|
    `-- quote v1.0.0|proc-macro
[build-dependencies]
`-- cc v1.0.0|
[dev-dependencies]
`-- tempfile v3.0.0|default
";

#[test]
fn check_attributes_build_dependency_and_proc_macro_descendants_to_the_host_side() {
    let target_only = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- derive-thing v1.0.0 (proc-macro)|
|-- quote v1.0.0|proc-macro
|-- syn v2.0.0|full
`-- tempfile v3.0.0|default
[build-dependencies]
`-- cc v1.0.0|
";
    let out = Fixture::new(&covered_a(), &workspace_of(&[target_only, TREE_UNION]))
        .tree("a", TREE_A_HOST_ONLY)
        .check();
    assert_findings(
        &out,
        &[
            &["SPLIT a ", "syn v2.0.0", "(host)", "{full}"],
            &["SPLIT a ", "quote v1.0.0", "(host)", "{proc-macro}"],
        ],
        "the workspace builds syn and quote on the target side only",
    );

    let both_sides = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- derive-thing v1.0.0 (proc-macro)|
|   |-- quote v1.0.0|proc-macro
|   `-- syn v2.0.0|full
|-- quote v1.0.0|proc-macro
|-- syn v2.0.0|full
`-- tempfile v3.0.0|default
[build-dependencies]
`-- cc v1.0.0|
";
    let out = Fixture::new(&covered_a(), &workspace_of(&[both_sides, TREE_UNION]))
        .tree("a", TREE_A_HOST_ONLY)
        .check();
    assert_green(
        &out,
        "the workspace also builds syn and quote on the host side, and a dev-dependency \
         after a [build-dependencies] section is target-side",
    );
}

#[test]
fn check_reds_on_a_member_without_the_union_dependency_naming_its_manifest() {
    let tree_b = "\
b v0.1.0 (/fixture/crates/b)|
`-- memchr v2.0.0|std
";
    let members = vec![member("a", &[UNION]), member("b", &[]), member(UNION, &[])];
    let fixture = Fixture::new(&members, &workspace_of(&[TREE_A, tree_b, TREE_UNION]))
        .tree("a", TREE_A)
        .tree("b", tree_b);
    let manifest = fixture.manifest("b");
    assert_findings(
        &fixture.check(),
        &[&["UNCOVERED b", UNION, &manifest]],
        "member b does not depend on the union crate",
    );
}

#[test]
fn check_reds_on_a_build_dependency_member_that_depends_on_the_union() {
    let tree_a_with_u = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- memchr v2.0.0|std
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
";
    let ws = workspace_of(&[tree_a_with_u, TREE_UNION]);

    let host_member = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &[UNION]),
        member(UNION, &[]),
    ];
    assert_findings(
        &Fixture::new(&host_member, &ws)
            .tree("a", tree_a_with_u)
            .check(),
        &[&["HOST-MEMBER u", "{a}"]],
        "u is a build-dependency of a and depends on the union",
    );

    let lean = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &[]),
        member(UNION, &[]),
    ];
    assert_green(
        &Fixture::new(&lean, &ws).tree("a", tree_a_with_u).check(),
        "a build-dependency member is neither required to depend on the union nor \
         resolved with -p (the stub exits 3 on -p u)",
    );
}

#[test]
fn check_exits_2_when_cargo_tree_fails_naming_the_command_and_its_stderr() {
    let fixture =
        Fixture::new(&covered_a(), &workspace_of(&[TREE_A, TREE_UNION])).tree("a", TREE_A);
    fixture.write("tree-fail", "");
    assert_input_error(&fixture.check(), &["tree", "boom"], "cargo tree exits 101");
}

#[test]
fn check_exits_2_on_a_tree_line_it_cannot_read_quoting_the_line() {
    let cases = [
        (
            "a package line with no `|`",
            "|-- serde v1.0.0 derive",
            "serde v1.0.0 derive",
        ),
        (
            "an unknown section header",
            "[weird-dependencies]",
            "[weird-dependencies]",
        ),
        (
            "a source other than a workspace path",
            "|-- foo v1.0.0 (https://example.invalid/foo#abc)|",
            "unsupported source",
        ),
        (
            "a line deeper than its parent",
            "|   |   `-- foo v1.0.0|",
            "foo v1.0.0",
        ),
    ];
    for (label, line, needle) in cases {
        let tree_a = format!("a v0.1.0 (/fixture/crates/a)|\n{line}\n");
        let out = Fixture::new(&covered_a(), &workspace_of(&[TREE_A, TREE_UNION]))
            .tree("a", &tree_a)
            .check();
        assert_input_error(&out, &[needle], label);
    }
}
