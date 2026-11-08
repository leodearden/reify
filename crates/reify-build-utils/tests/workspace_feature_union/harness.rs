//! The shared harness of the workspace_feature_union tests: the script runner,
//! the output assertions, the stub cargo with its fixtures, and the union manifest.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

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
pub(crate) fn run_script(
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

pub(crate) const UNION: &str = "workspace-feature-union";

pub(crate) fn describe(out: &Output) -> String {
    format!(
        "{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

pub(crate) fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn line_has_all(line: &str, needles: &[&str]) -> bool {
    needles.iter().all(|needle| line.contains(needle))
}

const FINDING_KINDS: [&str; 4] = ["SPLIT ", "UNCOVERED ", "HOST-UNION ", "PROFILE-SPLIT "];

pub(crate) fn finding_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| FINDING_KINDS.iter().any(|kind| line.starts_with(kind)))
        .collect()
}

pub(crate) fn assert_exit(out: &Output, expected: i32, context: &str) {
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{context}: expected exit {expected}\n{}",
        describe(out)
    );
}

pub(crate) fn assert_green(out: &Output, context: &str) {
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
pub(crate) fn assert_findings(out: &Output, expected: &[&[&str]], context: &str) {
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
pub(crate) fn assert_input_error(out: &Output, needles: &[&str], context: &str) {
    assert_exit(out, 2, context);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        line_has_all(&stderr, needles),
        "{context}: expected stderr to hold {needles:?}\n{}",
        describe(out)
    );
}

/// One `check` of the real workspace (about 38 `cargo tree` runs and as many
/// `cargo test --unit-graph` resolutions), shared by every real-workspace
/// test and resolved with the inherited environment so the verify env's
/// CARGO and CARGO_HOME reach cargo.
pub(crate) fn real_check() -> &'static Output {
    static CHECK: OnceLock<Output> = OnceLock::new();
    CHECK.get_or_init(|| run_script("check", None, None, &[]))
}

// --- fixtures ----------------------------------------------------------------

/// One workspace member of a fixture's `cargo metadata --no-deps`.
pub(crate) struct Member {
    name: &'static str,
    deps: Vec<(&'static str, Option<&'static str>)>,
    lib_kind: &'static str,
}

pub(crate) fn member(name: &'static str, normal_deps: &[&'static str]) -> Member {
    Member {
        name,
        deps: normal_deps.iter().map(|dep| (*dep, None)).collect(),
        lib_kind: "lib",
    }
}

impl Member {
    pub(crate) fn build_dep(mut self, name: &'static str) -> Member {
        self.deps.push((name, Some("build")));
        self
    }

    pub(crate) fn dev_dep(mut self, name: &'static str) -> Member {
        self.deps.push((name, Some("dev")));
        self
    }

    pub(crate) fn proc_macro(mut self) -> Member {
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

pub(crate) fn write_file(path: &Path, contents: &str) {
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
/// `test --workspace` serves unit-graph-workspace.json and `test -p NAME`
/// serves unit-graph-NAME.json, or the empty version-1 unit graph when that
/// file is absent; every `test` call appends its RUSTC_BOOTSTRAP and
/// CARGO_TARGET_DIR to cargo-env.log.
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
  test)
    printf 'RUSTC_BOOTSTRAP=%s CARGO_TARGET_DIR=%s\n' "${RUSTC_BOOTSTRAP:-}" "${CARGO_TARGET_DIR:-}" \
      >> "$here/cargo-env.log"
    prev=""
    for arg in "$@"; do
      graph=""
      if [ "$arg" = "--workspace" ]; then
        graph="$here/unit-graph-workspace.json"
      elif [ "$prev" = "-p" ]; then
        graph="$here/unit-graph-$arg.json"
      fi
      if [ -n "$graph" ]; then
        if [ -e "$graph" ]; then
          exec cat "$graph"
        fi
        printf '%s\n' '{"version":1,"units":[],"roots":[]}'
        exit 0
      fi
      prev="$arg"
    done ;;
esac
echo "stub cargo: unexpected argv: $*" >&2
exit 3
"#;

pub(crate) struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    pub(crate) fn new(members: &[Member], workspace_tree: &str) -> Fixture {
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
    pub(crate) fn tree(self, name: &str, text: &str) -> Fixture {
        self.write(&format!("tree-{name}.txt"), text);
        self
    }

    /// Serves `json` for `cargo test --unit-graph` of `selection`: a member
    /// name for `-p <name>`, or "workspace" for `--workspace`.
    pub(crate) fn unit_graph(self, selection: &str, json: &str) -> Fixture {
        self.write(&format!("unit-graph-{selection}.json"), json);
        self
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub(crate) fn write(&self, name: &str, contents: &str) {
        write_file(&self.path(name), contents);
    }

    pub(crate) fn manifest(&self, name: &str) -> String {
        manifest_path(self.dir.path(), name).display().to_string()
    }

    /// Serves `text` for `cargo tree -p <name>` once the union manifest
    /// matches the grep pattern `pinned` and every pattern of the stages
    /// added for `name` before this one.
    pub(crate) fn tree_after(self, name: &str, pinned: &str, text: &str) -> Fixture {
        let stage_file =
            |stage: u32, extension: &str| format!("tree-{name}.after-{stage}.{extension}");
        let stage = (1..)
            .find(|&candidate| !self.path(&stage_file(candidate, "txt")).exists())
            .expect("an unbounded range always has a free stage number");
        self.write(&stage_file(stage, "txt"), text);
        self.write(&stage_file(stage, "pattern"), &format!("{pinned}\n"));
        self
    }

    pub(crate) fn union_manifest_path(&self) -> PathBuf {
        manifest_path(self.dir.path(), UNION)
    }

    pub(crate) fn with_union_manifest(self, text: &str) -> Fixture {
        write_file(&self.union_manifest_path(), text);
        self
    }

    pub(crate) fn union_manifest(&self) -> String {
        let path = self.union_manifest_path();
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    pub(crate) fn run_with(&self, subcommand: &str, extra: &[&str]) -> Output {
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

    pub(crate) fn check(&self) -> Output {
        self.run("check")
    }

    pub(crate) fn generate(&self) -> Output {
        self.run("generate")
    }

    /// Every argv the stub cargo was invoked with, one per line.
    pub(crate) fn cargo_argv_log(&self) -> String {
        fs::read_to_string(self.path("cargo-argv.log")).unwrap_or_default()
    }

    /// The RUSTC_BOOTSTRAP and CARGO_TARGET_DIR of every stub `cargo test`
    /// call, one call per line.
    pub(crate) fn cargo_env_log(&self) -> String {
        fs::read_to_string(self.path("cargo-env.log")).unwrap_or_default()
    }
}

/// `-p a`: the union crate, a target-side serde and its proc-macro below it,
/// a deduped repeat, and the root's build- and dev-dependency sections.
pub(crate) const TREE_A: &str = "\
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

pub(crate) const TREE_UNION: &str =
    "workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|\n";

/// `--workspace` over roots `a` and the union crate.
pub(crate) fn workspace_of(roots: &[&str]) -> String {
    roots.join("\n")
}

pub(crate) fn covered_a() -> Vec<Member> {
    vec![member("a", &[UNION]), member(UNION, &[])]
}

pub(crate) const UNION_NODE: &str =
    "workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|";

/// `--workspace` with `a` building with member `u`, whose subtree below its
/// `[build-dependencies]` line is `u_children` (each line indented under u).
pub(crate) fn workspace_building_a_with_u(u_children: &[&str]) -> String {
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

/// `--workspace` and `-p c` resolve member b with `test-support`; `-p a` and
/// `-p d` resolve it without. Each of `uncovered` joins as a member that
/// depends on nothing, the union crate included.
pub(crate) fn member_split_fixture(uncovered: &[&'static str]) -> Fixture {
    let mut members = vec![
        member("a", &[UNION]),
        member("b", &[UNION]),
        member("c", &[UNION]),
        member("d", &[UNION]),
        member(UNION, &[]),
    ];
    members.extend(uncovered.iter().map(|name| member(name, &[])));
    let root_with_b = |name: &str, b_features: &str| {
        format!(
            "{name} v0.1.0 (/fixture/crates/{name})|\n\
             |-- {UNION_NODE}\n\
             `-- b v0.1.0 (/fixture/crates/b)|{b_features}\n"
        )
    };
    let bare_root = |name: &str| format!("{name} v0.1.0 (/fixture/crates/{name})|\n");
    let tree_b = "b v0.1.0 (/fixture/crates/b)|test-support\n";
    let mut roots = vec![
        root_with_b("a", "test-support"),
        tree_b.to_string(),
        root_with_b("c", "test-support"),
        root_with_b("d", "test-support"),
        TREE_UNION.to_string(),
    ];
    roots.extend(uncovered.iter().map(|name| bare_root(name)));
    let roots: Vec<&str> = roots.iter().map(String::as_str).collect();
    let fixture = Fixture::new(&members, &workspace_of(&roots))
        .tree("a", &root_with_b("a", ""))
        .tree("b", tree_b)
        .tree("c", &root_with_b("c", "test-support"))
        .tree("d", &root_with_b("d", ""));
    uncovered.iter().fold(fixture, |fixture, name| {
        fixture.tree(name, &bare_root(name))
    })
}

// --- the union manifest ------------------------------------------------------

pub(crate) const BEGIN_MARKER: &str =
    "# BEGIN generated by scripts/workspace-feature-union.py generate — do not edit by hand\n";
pub(crate) const END_MARKER: &str = "# END generated\n";
pub(crate) const UNION_PACKAGE: &str = "\
[package]
name = \"workspace-feature-union\"
version = \"0.1.0\"
edition = \"2024\"
publish = false

";

pub(crate) fn empty_union_manifest() -> String {
    format!("{UNION_PACKAGE}{BEGIN_MARKER}{END_MARKER}")
}

pub(crate) const HOST: &str = "x86_64-unknown-linux-gnu";

pub(crate) fn parse_toml(text: &str, what: &str) -> toml::Value {
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

pub(crate) fn assert_pin(manifest: &toml::Value, table: &str, key: &str, expected: &str) {
    assert_eq!(
        pin(manifest, table, key),
        &parse_toml(expected, "expected pin"),
        "{table} pin {key:?}\n{manifest:#?}"
    );
}
