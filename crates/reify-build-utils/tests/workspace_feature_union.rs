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

/// `python3 -I <script> <subcommand> [--root <root>] <extra>...` with the
/// inherited environment, plus CARGO=`cargo` when one is given.
fn run_script(
    subcommand: &str,
    root: Option<&Path>,
    cargo: Option<&Path>,
    extra: &[&str],
) -> Output {
    let mut command = Command::new("python3");
    command
        .arg("-I")
        .arg(script())
        .arg(subcommand)
        .current_dir(workspace_root());
    if let Some(root) = root {
        command.arg("--root").arg(root);
    }
    command.args(extra);
    if let Some(cargo) = cargo {
        command.env("CARGO", cargo);
    }
    command
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn python3 {}: {e}", script().display()))
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

const FINDING_KINDS: [&str; 3] = ["SPLIT ", "UNCOVERED ", "HOST-UNION "];

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
    let out = run_script("check", None, None, &[]);
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
    lib_kind: &'static str,
}

fn member(name: &'static str, normal_deps: &[&'static str]) -> Member {
    Member {
        name,
        deps: normal_deps.iter().map(|dep| (*dep, None)).collect(),
        lib_kind: "lib",
    }
}

impl Member {
    fn build_dep(mut self, name: &'static str) -> Member {
        self.deps.push((name, Some("build")));
        self
    }

    fn dev_dep(mut self, name: &'static str) -> Member {
        self.deps.push((name, Some("dev")));
        self
    }

    fn proc_macro(mut self) -> Member {
        self.lib_kind = "proc-macro";
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
                "{{\"name\": \"{}\", \"id\": \"{}\", \"manifest_path\": \"{}\", \"dependencies\": [{}], \
                 \"targets\": [{{\"kind\": [\"{}\"]}}]}}",
                m.name,
                package_id(m.name),
                manifest_path(root, m.name).display(),
                deps.join(", "),
                m.lib_kind
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

fn write_file(path: &Path, contents: &str) {
    let parent = path.parent().expect("a written file has a parent");
    fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkdir {}: {e}", parent.display()));
    fs::write(path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// A stand-in for cargo that serves the fixture's canned files by argv and
/// logs every argv to cargo-argv.log. `tree -p NAME` serves
/// tree-NAME.after-K.txt for the highest K whose tree-NAME.after-K.pattern,
/// and the pattern of every earlier stage, matches the union manifest
/// (`grep -f`), and serves tree-NAME.txt when stage 1's pattern does not
/// match. Like real cargo, an `--offline` tree may rewrite Cargo.lock: the
/// stub appends its argv to the fixture's Cargo.lock when one exists.
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
    if [ -e "$here/Cargo.lock" ] && [[ " $* " == *" --offline "* ]]; then
      printf 'resolved by: %s\n' "$*" >> "$here/Cargo.lock"
    fi
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
        served="$here/tree-$arg.txt"
        stage=1
        while [ -e "$here/tree-$arg.after-$stage.txt" ] \
          && grep -qf "$here/tree-$arg.after-$stage.pattern" "$here/crates/workspace-feature-union/Cargo.toml"; do
          served="$here/tree-$arg.after-$stage.txt"
          stage=$((stage + 1))
        done
        if [ -e "$served" ]; then
          exec cat "$served"
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
        write_file(&self.path(name), contents);
    }

    fn manifest(&self, name: &str) -> String {
        manifest_path(self.dir.path(), name).display().to_string()
    }

    /// Serves `text` for `cargo tree -p <name>` once the union manifest
    /// matches the grep pattern `pinned` and every pattern of the stages
    /// added for `name` before this one.
    fn tree_after(self, name: &str, pinned: &str, text: &str) -> Fixture {
        let stage_file =
            |stage: u32, extension: &str| format!("tree-{name}.after-{stage}.{extension}");
        let stage = (1..)
            .find(|&candidate| !self.path(&stage_file(candidate, "txt")).exists())
            .expect("an unbounded range always has a free stage number");
        self.write(&stage_file(stage, "txt"), text);
        self.write(&stage_file(stage, "pattern"), &format!("{pinned}\n"));
        self
    }

    fn union_manifest_path(&self) -> PathBuf {
        manifest_path(self.dir.path(), UNION)
    }

    fn with_union_manifest(self, text: &str) -> Fixture {
        write_file(&self.union_manifest_path(), text);
        self
    }

    fn union_manifest(&self) -> String {
        let path = self.union_manifest_path();
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    fn run_with(&self, subcommand: &str, extra: &[&str]) -> Output {
        run_script(
            subcommand,
            Some(self.dir.path()),
            Some(&self.path("cargo")),
            extra,
        )
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

const UNION_NODE: &str =
    "workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|";

/// `--workspace` with `a` building with member `u`, whose subtree below its
/// `[build-dependencies]` line is `u_children` (each line indented under u).
fn workspace_building_a_with_u(u_children: &[&str]) -> String {
    let below_u: String = u_children
        .iter()
        .map(|line| format!("    {line}\n"))
        .collect();
    let tree_a = format!(
        "a v0.1.0 (/fixture/crates/a)|\n\
         |-- {UNION_NODE}\n\
         `-- memchr v2.0.0|std\n\
         [build-dependencies]\n\
         `-- u v0.1.0 (/fixture/crates/u)|\n\
         {below_u}"
    );
    workspace_of(&[&tree_a, TREE_UNION])
}

const TREE_A_UNION_MEMCHR: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- memchr v2.0.0|std
";

#[test]
fn check_reds_when_a_build_dependency_member_puts_the_union_on_the_host_side() {
    let members = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &[UNION]),
        member(UNION, &[]),
    ];
    let fixture = Fixture::new(
        &members,
        &workspace_building_a_with_u(&[&format!("`-- {UNION_NODE}")]),
    )
    .tree("a", TREE_A_UNION_MEMCHR);
    let manifest = fixture.manifest("u");
    assert_findings(
        &fixture.check(),
        &[&[
            "HOST-UNION u",
            "a -[build]-> u -> workspace-feature-union",
            "remove u's dependency on workspace-feature-union",
            &manifest,
        ]],
        "u is a build-dependency of a and depends on the union",
    );
}

#[test]
fn check_reds_when_a_build_dependency_member_reaches_the_union_through_a_covered_member() {
    let members = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &["core"]),
        member("core", &[UNION]),
        member(UNION, &[]),
    ];
    let tree_core = format!("core v0.1.0 (/fixture/crates/core)|\n`-- {UNION_NODE}\n");
    let fixture = Fixture::new(
        &members,
        &workspace_building_a_with_u(&[
            "`-- core v0.1.0 (/fixture/crates/core)|",
            &format!("    `-- {UNION_NODE}"),
        ]),
    )
    .tree("a", TREE_A_UNION_MEMCHR)
    .tree("core", &tree_core);
    let manifest = fixture.manifest("u");
    assert_findings(
        &fixture.check(),
        &[&[
            "HOST-UNION u",
            "a -[build]-> u -> core -> workspace-feature-union",
            "remove u's dependency on core",
            &manifest,
        ]],
        "u never names the union, but its dependency core does",
    );
}

#[test]
fn check_names_the_covered_member_a_host_side_member_brings_on_before_it_has_the_union() {
    let members = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &["core"]),
        member("core", &[]),
        member(UNION, &[]),
    ];
    let tree_core = "core v0.1.0 (/fixture/crates/core)|\n";
    let fixture = Fixture::new(
        &members,
        &workspace_building_a_with_u(&["`-- core v0.1.0 (/fixture/crates/core)|"]),
    )
    .tree("a", TREE_A_UNION_MEMCHR)
    .tree("core", tree_core);
    let out = fixture.check();
    assert_findings(
        &out,
        &[
            &["UNCOVERED core"],
            &[
                "HOST-UNION u",
                "a -[build]-> u -> core puts covered member core on the host side",
                "remove u's dependency on core",
            ],
        ],
        "core does not depend on the union yet, but the coverage rule requires it to",
    );
    let stdout = stdout_of(&out);
    assert!(
        finding_lines(&stdout)
            .iter()
            .all(|line| !line.contains(&format!("resolves {UNION}"))),
        "no path reaches the union yet, so no finding may claim it resolves\n{}",
        describe(&out)
    );
}

#[test]
fn check_is_green_when_a_build_dependency_member_stays_off_the_union() {
    let members = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &[]),
        member(UNION, &[]),
    ];
    assert_green(
        &Fixture::new(&members, &workspace_building_a_with_u(&[]))
            .tree("a", TREE_A_UNION_MEMCHR)
            .check(),
        "a build-dependency member is neither required to depend on the union nor \
         resolved with -p (the stub exits 3 on -p u)",
    );
}

/// `--workspace` with `a` depending on the proc-macro member `pm`, whose
/// subtree is `pm_children`; pm is also a root of its own.
fn workspace_using_proc_macro_pm(pm_children: &[&str]) -> String {
    let below_pm: String = pm_children
        .iter()
        .map(|line| format!("    {line}\n"))
        .collect();
    let root_pm: String = pm_children.iter().map(|line| format!("{line}\n")).collect();
    let tree_a = format!(
        "a v0.1.0 (/fixture/crates/a)|\n\
         |-- {UNION_NODE}\n\
         `-- pm v0.1.0 (proc-macro) (/fixture/crates/pm)|\n\
         {below_pm}"
    );
    let tree_pm = format!("pm v0.1.0 (proc-macro) (/fixture/crates/pm)|\n{root_pm}");
    workspace_of(&[&tree_a, &tree_pm, TREE_UNION])
}

const TREE_A_UNION_PM: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- pm v0.1.0 (proc-macro) (/fixture/crates/pm)|
";

#[test]
fn check_exempts_a_proc_macro_member_from_coverage() {
    let members = vec![
        member("a", &[UNION, "pm"]),
        member("pm", &[]).proc_macro(),
        member(UNION, &[]),
    ];
    assert_green(
        &Fixture::new(&members, &workspace_using_proc_macro_pm(&[]))
            .tree("a", TREE_A_UNION_PM)
            .check(),
        "a proc-macro member is compiled for the host: it is neither required to depend \
         on the union nor resolved with -p (the stub exits 3 on -p pm)",
    );
}

#[test]
fn check_reds_when_a_proc_macro_member_puts_the_union_on_the_host_side() {
    let members = vec![
        member("a", &[UNION, "pm"]),
        member("pm", &[UNION]).proc_macro(),
        member(UNION, &[]),
    ];
    let fixture = Fixture::new(
        &members,
        &workspace_using_proc_macro_pm(&[&format!("`-- {UNION_NODE}")]),
    )
    .tree("a", TREE_A_UNION_PM);
    let manifest = fixture.manifest("pm");
    assert_findings(
        &fixture.check(),
        &[&[
            "HOST-UNION pm",
            "pm (proc-macro) -> workspace-feature-union",
            "remove pm's dependency on workspace-feature-union",
            &manifest,
        ]],
        "one finding for pm, though the union sits below pm under a and as pm's own root",
    );
}

/// Real cargo 1.96's `--workspace` tree for core -> union, u -> core and
/// zz -> union with `[build-dependencies] u`. A member reached on the host
/// side gets a second root, textually identical to its target root, and the
/// `(*)` dedupe spans roots, so neither u root nor zz's build edge to u
/// shows the union below it.
const WS_ZZ_BUILDS_WITH_U: &str = "\
core v0.1.0 (/fixture/crates/core)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

core v0.1.0 (/fixture/crates/core)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

u v0.1.0 (/fixture/crates/u)|
`-- core v0.1.0 (/fixture/crates/core)| (*)

u v0.1.0 (/fixture/crates/u)|
`-- core v0.1.0 (/fixture/crates/core)| (*)

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_CORE: &str = "\
core v0.1.0 (/fixture/crates/core)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";

const TREE_ZZ_BUILDS_WITH_U: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- core v0.1.0 (/fixture/crates/core)|
        `-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";

#[test]
fn check_and_generate_red_on_a_host_union_hidden_behind_a_host_root_printed_first() {
    let members = vec![
        member("core", &[UNION]),
        member("u", &["core"]),
        member("zz", &[UNION]).build_dep("u"),
        member(UNION, &[]),
    ];
    let original =
        empty_union_manifest().replace(END_MARKER, &format!("stale = \"1\"\n{END_MARKER}"));
    let fixture = Fixture::new(&members, WS_ZZ_BUILDS_WITH_U)
        .tree("core", TREE_CORE)
        .tree("zz", TREE_ZZ_BUILDS_WITH_U)
        .with_union_manifest(&original);
    let manifest = fixture.manifest("u");
    let expected: &[&[&str]] = &[&[
        "HOST-UNION u",
        "zz -[build]-> u -> core -> workspace-feature-union",
        "remove u's dependency on core",
        &manifest,
    ]];

    assert_findings(
        &fixture.check(),
        expected,
        "u reaches the union through core, though cargo tree prints u's host root \
         before zz's build edge to it",
    );
    assert_findings(
        &fixture.generate(),
        expected,
        "generate refuses on the same finding check reports",
    );
    assert_eq!(
        fixture.union_manifest(),
        original,
        "generate must not touch the union manifest while a HOST-UNION finding stands"
    );
}

#[test]
fn check_reds_when_a_covered_member_build_depends_on_the_union() {
    let tree_zz = "\
zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";
    let members = vec![member("zz", &[UNION]).build_dep(UNION), member(UNION, &[])];
    let fixture = Fixture::new(&members, &workspace_of(&[TREE_UNION, TREE_UNION, tree_zz]))
        .tree("zz", tree_zz);
    let manifest = fixture.manifest("zz");
    assert_findings(
        &fixture.check(),
        &[&[
            "HOST-UNION zz",
            "zz -[build]-> workspace-feature-union",
            "remove zz's build-dependency on workspace-feature-union",
            &manifest,
        ]],
        "zz's build script would compile the union for the host",
    );
}

#[test]
fn check_is_green_when_a_build_dependency_member_only_dev_depends_on_a_covered_member() {
    // Real cargo 1.96 lists u's dev-dependencies even below a's build edge
    // to u, though building a never compiles them.
    let workspace = "\
a v0.1.0 (/fixture/crates/a)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    [dev-dependencies]
    `-- core v0.1.0 (/fixture/crates/core)|
        `-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

core v0.1.0 (/fixture/crates/core)| (*)

core v0.1.0 (/fixture/crates/core)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

u v0.1.0 (/fixture/crates/u)| (*)

u v0.1.0 (/fixture/crates/u)|
[dev-dependencies]
`-- core v0.1.0 (/fixture/crates/core)| (*)

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";
    let tree_a = "\
a v0.1.0 (/fixture/crates/a)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
";
    let members = vec![
        member("a", &[UNION]).build_dep("u"),
        member("u", &[]).dev_dep("core"),
        member("core", &[UNION]),
        member(UNION, &[]),
    ];
    assert_green(
        &Fixture::new(&members, workspace)
            .tree("a", tree_a)
            .tree("core", TREE_CORE)
            .check(),
        "a dev-dependency is not part of u's host build",
    );
}

// --- member features ---------------------------------------------------------

/// `--workspace` and `-p c` resolve member b with `test-support`; `-p a` and
/// `-p d` resolve it without.
fn member_split_fixture() -> Fixture {
    let members = vec![
        member("a", &[UNION]),
        member("b", &[UNION]),
        member("c", &[UNION]),
        member("d", &[UNION]),
        member(UNION, &[]),
    ];
    let root_with_b = |name: &str, b_features: &str| {
        format!(
            "{name} v0.1.0 (/fixture/crates/{name})|\n\
             |-- {UNION_NODE}\n\
             `-- b v0.1.0 (/fixture/crates/b)|{b_features}\n"
        )
    };
    let tree_b = "b v0.1.0 (/fixture/crates/b)|test-support\n";
    let workspace = workspace_of(&[
        &root_with_b("a", "test-support"),
        tree_b,
        &root_with_b("c", "test-support"),
        &root_with_b("d", "test-support"),
        TREE_UNION,
    ]);
    Fixture::new(&members, &workspace)
        .tree("a", &root_with_b("a", ""))
        .tree("b", tree_b)
        .tree("c", &root_with_b("c", "test-support"))
        .tree("d", &root_with_b("d", ""))
}

#[test]
fn check_reports_a_member_feature_split_as_advisory_naming_the_selections_that_resolve_it() {
    let out = member_split_fixture().check();
    assert_green(&out, "a member feature split is advisory, never a finding");
    let stdout = stdout_of(&out);
    let advisories: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("MEMBER-SPLIT "))
        .collect();
    assert_eq!(
        advisories,
        ["MEMBER-SPLIT b v0.1.0 (target): -p resolves {} under a, d; \
          --workspace resolves {test-support}"],
        "one advisory line for b, naming only the selections that resolve it unlike \
         --workspace\n{}",
        describe(&out)
    );
}

// --- real cargo --------------------------------------------------------------

fn path_dep(name: &str) -> String {
    format!("{name} = {{ path = \"../{name}\" }}\n")
}

/// A real member crate with an empty lib.rs and `dependency_tables` appended
/// to its manifest.
fn write_real_member(root: &Path, name: &str, dependency_tables: &str) {
    let dir = root.join("crates").join(name);
    write_file(&dir.join("src/lib.rs"), "");
    write_file(
        &dir.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             publish = false\n\n{dependency_tables}"
        ),
    );
}

/// A real path-only workspace: core -> union, u -> core, and `dependent` ->
/// union with `[build-dependencies] u`, locked offline.
fn real_host_union_workspace(dependent: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a real-cargo workspace dir");
    let root = dir.path();
    write_file(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
    );
    write_real_member(root, UNION, "");
    write_real_member(
        root,
        "core",
        &format!("[dependencies]\n{}", path_dep(UNION)),
    );
    write_real_member(root, "u", &format!("[dependencies]\n{}", path_dep("core")));
    write_real_member(
        root,
        dependent,
        &format!(
            "[dependencies]\n{}\n[build-dependencies]\n{}",
            path_dep(UNION),
            path_dep("u")
        ),
    );
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let lock = Command::new(&cargo)
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {cargo}: {e}"));
    assert!(
        lock.status.success(),
        "cargo generate-lockfile --offline in {}\n{}",
        root.display(),
        describe(&lock)
    );
    dir
}

/// Real cargo prints the roots of `--workspace` in member-name order, so the
/// finding must not depend on whether the build-dependent sorts before or
/// after the members it builds with.
#[test]
fn check_reds_on_a_host_union_in_a_real_cargo_workspace_whichever_member_sorts_first() {
    for dependent in ["a", "zz"] {
        let workspace = real_host_union_workspace(dependent);
        let out = run_script("check", Some(workspace.path()), None, &[]);
        let path = format!("{dependent} -[build]-> u -> core -> workspace-feature-union");
        assert_findings(
            &out,
            &[&["HOST-UNION u", &path, "remove u's dependency on core"]],
            &format!("build-dependent {dependent}"),
        );
    }
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

/// `--workspace` for `a`, and the tree `-p a` converges to.
const WS_A_HASHBROWN_FOLDHASH: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- hashbrown v0.15.5|default-hasher,inline-more
    `-- foldhash v0.1.5|default,std
";

/// Pinning hashbrown's `default-hasher` pulls foldhash into `-p a` without the
/// `std` feature `--workspace` builds it with, which takes a second rewrite.
#[test]
fn generate_keeps_earlier_pins_when_a_pin_surfaces_a_new_split() {
    let before = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- hashbrown v0.15.5|inline-more
";
    let foldhash_split = "\
a v0.1.0 (/fixture/crates/a)|
|-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- hashbrown v0.15.5|default-hasher,inline-more
    `-- foldhash v0.1.5|
";
    let fixture = Fixture::new(
        &covered_a(),
        &workspace_of(&[WS_A_HASHBROWN_FOLDHASH, TREE_UNION]),
    )
    .with_union_manifest(&empty_union_manifest())
    .tree("a", before)
    .tree_after("a", "^hashbrown ", foldhash_split)
    .tree_after("a", "^foldhash ", WS_A_HASHBROWN_FOLDHASH);

    let out = fixture.generate();
    assert_exit(
        &out,
        0,
        "pinning hashbrown surfaces a foldhash split, which needs a second growing rewrite",
    );
    let text = fixture.union_manifest();
    assert_region_only_rewritten(&text);
    let manifest = parse_toml(&text, "the rewritten union manifest");
    assert_pin(
        &manifest,
        "dependencies",
        "hashbrown",
        r#"version = "0.15"
default-features = false
features = ["default-hasher", "inline-more"]"#,
    );
    assert_pin(
        &manifest,
        "dependencies",
        "foldhash",
        r#"version = "0.1"
features = ["std"]"#,
    );

    assert_green(
        &fixture.check(),
        "check after a generate that needed two growing rewrites",
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
fn generate_never_pins_a_workspace_member() {
    let fixture = member_split_fixture().with_union_manifest(&empty_union_manifest());
    let out = fixture.generate();
    assert_exit(
        &out,
        0,
        "a member feature split leaves no registry split to pin",
    );
    let text = fixture.union_manifest();
    assert_region_only_rewritten(&text);
    assert!(
        parse_toml(&text, "the rewritten union manifest")
            .get("target")
            .is_none(),
        "a member variant must never become a pin: the union depending on a member is a \
         cycle\n{text}\n{}",
        describe(&out)
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
    let fixture = Fixture::new(
        &members,
        &workspace_building_a_with_u(&[&format!("`-- {UNION_NODE}")]),
    )
    .with_union_manifest(&original);

    assert_findings(
        &fixture.generate(),
        &[&["UNCOVERED b"], &["HOST-UNION u"]],
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

#[test]
fn generate_restores_the_union_manifest_and_cargo_lock_when_cargo_fails_part_way() {
    let original = empty_union_manifest().replace(
        END_MARKER,
        &format!("[target.{HOST}.dependencies]\nserde = {{ version = \"1\" }}\n{END_MARKER}"),
    );
    let lock = "# the stub appends to this on every --offline cargo tree\n";
    // No `-p a` tree: the stub exits 3 on `cargo tree -p a`, which generate
    // reaches only after it has rewritten the region and resolved --workspace.
    let fixture = Fixture::new(&covered_a(), &workspace_of(&[TREE_A, TREE_UNION]))
        .with_union_manifest(&original);
    fixture.write("Cargo.lock", lock);

    let out = fixture.generate();
    assert_input_error(
        &out,
        &["-p a", "restored"],
        "cargo tree -p a fails part-way",
    );
    let log = fixture.cargo_argv_log();
    assert!(
        log.lines()
            .any(|line| line.starts_with("tree") && line.contains("-p a")),
        "the failure must come after generate began rewriting\n{log}"
    );
    assert_eq!(
        fixture.union_manifest(),
        original,
        "a failed generate must leave the union manifest byte-identical\n{}",
        describe(&out)
    );
    assert_eq!(
        fs::read_to_string(fixture.path("Cargo.lock")).expect("read the fixture Cargo.lock"),
        lock,
        "a failed generate must leave Cargo.lock byte-identical\n{}",
        describe(&out)
    );
}
