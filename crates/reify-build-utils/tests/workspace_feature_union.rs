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

// --- the real workspace ------------------------------------------------------

/// The gate: the real workspace, resolved with the inherited environment so
/// the verify env's CARGO and CARGO_HOME reach cargo. On failure the full
/// output carries every SPLIT/UNCOVERED line and the remedy.
#[test]
fn real_workspace_resolves_every_member_like_the_workspace() {
    let out = Command::new("python3")
        .arg("-I")
        .arg(script())
        .arg("check")
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn python3 {}: {e}", script().display()));
    assert!(
        out.status.success(),
        "a member resolves an external package unlike --workspace; \
         docs/notes/workspace-feature-union.md explains the remedy\n{}",
        describe(&out)
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
/// logs every argv to cargo-argv.log. `tree -p NAME` serves
/// tree-NAME.after.txt once the union manifest matches the grep pattern in
/// after-pattern, and tree-NAME.txt before that.
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
        if [ -e "$here/tree-$arg.after.txt" ] \
          && grep -qf "$here/after-pattern" "$here/crates/workspace-feature-union/Cargo.toml"; then
          exec cat "$here/tree-$arg.after.txt"
        fi
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

    /// Serves `text` for `cargo tree -p <name>` once the union manifest
    /// matches the grep pattern `pinned`.
    fn tree_after(self, name: &str, pinned: &str, text: &str) -> Fixture {
        self.write(&format!("tree-{name}.after.txt"), text);
        self.write("after-pattern", &format!("{pinned}\n"));
        self
    }

    fn union_manifest_path(&self) -> PathBuf {
        manifest_path(self.dir.path(), UNION)
    }

    fn with_union_manifest(self, text: &str) -> Fixture {
        let path = self.union_manifest_path();
        fs::create_dir_all(path.parent().expect("manifest has a parent"))
            .unwrap_or_else(|e| panic!("mkdir for {}: {e}", path.display()));
        fs::write(&path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        self
    }

    fn union_manifest(&self) -> String {
        let path = self.union_manifest_path();
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    fn run_with(&self, subcommand: &str, extra: &[&str]) -> Output {
        Command::new("python3")
            .arg("-I")
            .arg(script())
            .arg(subcommand)
            .arg("--root")
            .arg(self.dir.path())
            .args(extra)
            .env("CARGO", self.path("cargo"))
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn python3 {}: {e}", script().display()))
    }

    fn run(&self, subcommand: &str) -> Output {
        self.run_with(subcommand, &[])
    }

    fn check(&self) -> Output {
        self.run("check")
    }

    fn generate(&self) -> Output {
        self.run("generate")
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

// --- generate ----------------------------------------------------------------

const BEGIN_MARKER: &str =
    "# BEGIN generated by scripts/workspace-feature-union.py generate — do not edit by hand\n";
const END_MARKER: &str = "# END generated\n";
const UNION_PACKAGE: &str = "\
[package]
name = \"workspace-feature-union\"
version = \"0.1.0\"
edition = \"2024\"
publish = false

";

fn empty_union_manifest() -> String {
    format!("{UNION_PACKAGE}{BEGIN_MARKER}{END_MARKER}")
}

const HOST: &str = "x86_64-unknown-linux-gnu";

fn parse_toml(text: &str, what: &str) -> toml::Value {
    toml::from_str(text).unwrap_or_else(|e| panic!("{what} is not TOML: {e}\n{text}"))
}

/// The pin `key` of the union manifest's `[target.<host>.<table>]`.
fn pin<'a>(manifest: &'a toml::Value, table: &str, key: &str) -> &'a toml::Value {
    manifest
        .get("target")
        .and_then(|t| t.get(HOST))
        .and_then(|t| t.get(table))
        .and_then(|t| t.get(key))
        .unwrap_or_else(|| panic!("union manifest has no {table} pin {key:?}\n{manifest:#?}"))
}

fn assert_pin(manifest: &toml::Value, table: &str, key: &str, expected: &str) {
    assert_eq!(
        pin(manifest, table, key),
        &parse_toml(expected, "expected pin"),
        "{table} pin {key:?}\n{manifest:#?}"
    );
}

fn assert_region_only_rewritten(manifest: &str) {
    assert!(
        manifest.starts_with(&format!("{UNION_PACKAGE}{BEGIN_MARKER}")),
        "the [package] table and BEGIN marker must survive byte-for-byte\n{manifest}"
    );
    assert!(
        manifest.ends_with(END_MARKER),
        "the END marker must survive byte-for-byte\n{manifest}"
    );
}

const WS_A_CONVERGED: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- serde v1.0.0|default,derive,rc,std
|-- tiny v0.0.3|a,b
`-- helper v1.0.0 (proc-macro)|
    `-- syn v2.0.0|full,visit
";

#[test]
fn generate_pins_each_split_on_its_side_until_every_member_resolves_like_the_workspace() {
    let before = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- serde v1.0.0|derive,std
|-- tiny v0.0.3|a
`-- helper v1.0.0 (proc-macro)|
    `-- syn v2.0.0|full
";
    let fixture = Fixture::new(&covered_a(), &workspace_of(&[WS_A_CONVERGED, TREE_UNION]))
        .with_union_manifest(&empty_union_manifest())
        .tree("a", before)
        .tree_after("a", "^serde ", WS_A_CONVERGED);

    let out = fixture.generate();
    assert_exit(
        &out,
        0,
        "generate converges once serde, tiny and syn are pinned",
    );
    let text = fixture.union_manifest();
    assert_region_only_rewritten(&text);
    let manifest = parse_toml(&text, "the rewritten union manifest");
    assert_pin(
        &manifest,
        "dependencies",
        "serde",
        r#"version = "1"
features = ["derive", "rc", "std"]"#,
    );
    assert_pin(
        &manifest,
        "dependencies",
        "tiny",
        r#"version = "=0.0.3"
default-features = false
features = ["a", "b"]"#,
    );
    assert_pin(
        &manifest,
        "build-dependencies",
        "syn",
        r#"version = "2"
default-features = false
features = ["full", "visit"]"#,
    );

    let log = fixture.cargo_argv_log();
    assert!(
        log.lines()
            .filter(|line| line.starts_with("tree") || line.starts_with("metadata"))
            .all(|line| line.contains("--offline") && !line.contains("--frozen")),
        "generate must resolve with --offline so a manifest rewrite may update Cargo.lock\n{log}"
    );

    assert_green(&fixture.check(), "check after a converged generate");
}

#[test]
fn generate_keys_two_versions_of_one_package_by_their_major_compatible_requirement() {
    let ws_a = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- hashbrown v0.14.5|ahash,inline-more
`-- indexmap v2.0.0|std
    `-- hashbrown v0.17.1|default-hasher
";
    let before = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- hashbrown v0.14.5|
`-- indexmap v2.0.0|std
    `-- hashbrown v0.17.1|
";
    let fixture = Fixture::new(&covered_a(), &workspace_of(&[ws_a, TREE_UNION]))
        .with_union_manifest(&empty_union_manifest())
        .tree("a", before)
        .tree_after("a", "^hashbrown-v0-14 ", ws_a);

    let out = fixture.generate();
    assert_exit(&out, 0, "generate pins both hashbrown versions");
    let manifest = parse_toml(&fixture.union_manifest(), "the rewritten union manifest");
    assert_pin(
        &manifest,
        "dependencies",
        "hashbrown-v0-14",
        r#"package = "hashbrown"
version = "0.14"
default-features = false
features = ["ahash", "inline-more"]"#,
    );
    assert_pin(
        &manifest,
        "dependencies",
        "hashbrown-v0-17",
        r#"package = "hashbrown"
version = "0.17"
default-features = false
features = ["default-hasher"]"#,
    );
}

#[test]
fn generate_exits_1_naming_the_remaining_split_when_pinning_does_not_converge() {
    let ws_a = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- serde v1.0.0|default,derive,rc,std
";
    let stuck = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- serde v1.0.0|derive,std
";
    let fixture = Fixture::new(&covered_a(), &workspace_of(&[ws_a, TREE_UNION]))
        .with_union_manifest(&empty_union_manifest())
        .tree("a", stuck);

    let out = fixture.run_with("generate", &["--max-iterations", "2"]);
    assert_findings(
        &out,
        &[&["SPLIT a ", "serde v1.0.0", "(target)", "{derive,std}"]],
        "-p a still splits serde after it is pinned",
    );
    let text = fixture.union_manifest();
    assert_region_only_rewritten(&text);
    assert_pin(
        &parse_toml(&text, "the rewritten union manifest"),
        "dependencies",
        "serde",
        r#"version = "1"
features = ["derive", "rc", "std"]"#,
    );
}

#[test]
fn generate_exits_1_on_a_split_the_workspace_has_no_variant_to_pin_for() {
    let ws_a = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|-- syn v2.0.0|full
`-- helper v1.0.0 (proc-macro)|
";
    let host_syn = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- helper v1.0.0 (proc-macro)|
    `-- syn v2.0.0|full
";
    let fixture = Fixture::new(&covered_a(), &workspace_of(&[ws_a, TREE_UNION]))
        .with_union_manifest(&empty_union_manifest())
        .tree("a", host_syn);

    let out = fixture.generate();
    assert_findings(
        &out,
        &[&["SPLIT a ", "syn v2.0.0", "(host)", "no host variant"]],
        "--workspace builds syn on the target side only",
    );
}

#[test]
fn generate_refuses_to_write_while_the_coverage_rule_has_findings() {
    let members = vec![
        member("a", &[UNION]).build_dep("u"),
        member("b", &[]),
        member("u", &[UNION]),
        member(UNION, &[]),
    ];
    let original =
        empty_union_manifest().replace(END_MARKER, &format!("stale = \"1\"\n{END_MARKER}"));
    let fixture =
        Fixture::new(&members, &workspace_of(&[TREE_A, TREE_UNION])).with_union_manifest(&original);

    assert_findings(
        &fixture.generate(),
        &[&["UNCOVERED b"], &["HOST-MEMBER u", "{a}"]],
        "b lacks the union and build-dependency u depends on it",
    );
    assert_eq!(
        fixture.union_manifest(),
        original,
        "generate must not touch the union manifest while coverage findings stand"
    );
}

#[test]
fn generate_exits_2_naming_the_union_manifest_when_a_marker_is_missing() {
    let fixture = Fixture::new(&covered_a(), &workspace_of(&[TREE_A, TREE_UNION]))
        .with_union_manifest(&format!("{UNION_PACKAGE}{BEGIN_MARKER}"))
        .tree("a", TREE_A);
    let manifest = fixture.union_manifest_path().display().to_string();
    assert_input_error(
        &fixture.generate(),
        &[&manifest, "END"],
        "the union manifest has no END marker",
    );
}
