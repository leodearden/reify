//! INV-ARCH-1 gate: no intra-crate module SCC grows, and a shrink is recorded.
//!
//! Drives `scripts/assert-module-scc.py` over `scripts/module-scc-baseline.json`
//! through its CLI only — against the real workspace and against hermetic
//! throwaway workspaces. Rule: docs/prds/v0_6/dependency-invalidation-stratum.md
//! §5.1. Modelled on `crate_dag_assertion.rs`.

use std::ffi::OsStr;
use std::fs;
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

fn run_tool<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let script = workspace_root().join("scripts/assert-module-scc.py");
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

fn has_line_with_all(text: &str, needles: &[&str]) -> bool {
    text.lines()
        .any(|line| needles.iter().all(|needle| line.contains(needle)))
}

struct TempWorkspace {
    dir: tempfile::TempDir,
}

impl TempWorkspace {
    /// `members` are `(package name, member path)` pairs.
    fn new(members: &[(&str, &str)]) -> Self {
        let workspace = TempWorkspace {
            dir: tempfile::tempdir().expect("create temp workspace"),
        };
        let member_list = members
            .iter()
            .map(|(_, path)| format!("\"{path}\""))
            .collect::<Vec<_>>()
            .join(", ");
        workspace.write(
            "Cargo.toml",
            &format!("[workspace]\nmembers = [{member_list}]\n"),
        );
        for (package, path) in members {
            workspace.write(
                &format!("{path}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
                ),
            );
            workspace.write(&format!("{path}/src/lib.rs"), "");
        }
        workspace
    }

    fn demo() -> Self {
        Self::new(&[("demo", "crates/demo")])
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn baseline_path(&self) -> PathBuf {
        self.root().join("baseline.json")
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.root().join(rel);
        fs::create_dir_all(path.parent().expect("a written path has a parent"))
            .unwrap_or_else(|e| panic!("create parent of {}: {e}", path.display()));
        fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    fn baseline(&self, crates_json: &str) {
        self.write(
            "baseline.json",
            &format!("{{\"schema\": 1, \"excluded\": {{}}, \"crates\": {crates_json}}}\n"),
        );
    }

    fn run(&self, subcommand: &str) -> Output {
        run_tool([
            OsStr::new(subcommand),
            OsStr::new("--root"),
            self.root().as_os_str(),
            OsStr::new("--baseline"),
            self.baseline_path().as_os_str(),
        ])
    }

    fn check(&self) -> Output {
        self.run("check")
    }

    fn print(&self) -> Output {
        self.run("print")
    }
}

fn assert_exit(out: &Output, expected: i32, context: &str) {
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{context}: expected exit {expected}\n{}",
        describe(out)
    );
}

#[test]
fn b1_new_cycle_reds_as_grew_naming_both_import_sites() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::b::B;\npub struct A;\n");
    ws.write("crates/demo/src/b.rs", "use crate::a::A;\npub struct B;\n");
    ws.baseline("{}");

    let out = ws.check();
    assert_exit(&out, 1, "a new two-module cycle against an empty baseline");
    let stdout = stdout_of(&out);
    assert!(stdout.contains("GREW demo: {a, b}"), "{}", describe(&out));
    assert!(
        has_line_with_all(&stdout, &["crates/demo/src/a.rs:1:", "a -> b"]),
        "{}",
        describe(&out)
    );
    assert!(
        has_line_with_all(&stdout, &["crates/demo/src/b.rs:1:", "b -> a"]),
        "{}",
        describe(&out)
    );
}

#[test]
fn b2_stale_baseline_reds_and_print_lowers_it() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::b::B;\npub struct A;\n");
    ws.write("crates/demo/src/b.rs", "pub struct B;\n");
    ws.baseline(r#"{"demo": [["a", "b"]]}"#);

    let out = ws.check();
    assert_exit(&out, 1, "a baselined cycle that no longer exists");
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("STALE demo: baseline {a, b} measured {}"),
        "{}",
        describe(&out)
    );
    assert!(stdout.contains("lower the baseline"), "{}", describe(&out));

    let printed = ws.print();
    assert_exit(&printed, 0, "print over a stale baseline");
    let lowered = stdout_of(&printed);
    assert!(
        lowered.contains(r#""crates": {}"#),
        "{}",
        describe(&printed)
    );
    assert!(lowered.contains(r#""schema": 1"#), "{}", describe(&printed));

    ws.write("baseline.json", &lowered);
    assert_exit(
        &ws.check(),
        0,
        "check after lowering the baseline with print",
    );
}

#[test]
fn crate_brace_lists_contribute_only_their_top_level_names() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::{\n    b::{c::Z},\n};\n");
    ws.write("crates/demo/src/b.rs", "use crate::{a};\n");
    ws.write("crates/demo/src/c.rs", "use crate::a::A;\n");
    ws.baseline(r#"{"demo": [["a", "b"]]}"#);

    let out = ws.check();
    assert_exit(
        &out,
        0,
        "a nested name inside a crate::{…} list is not an edge",
    );
}

#[test]
fn ratchet_reads_the_shared_lexers_view_so_comments_and_strings_add_no_edges() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::b::B;\n");
    ws.write(
        "crates/demo/src/b.rs",
        concat!(
            "// use crate::a::A;\n",
            "pub fn f() -> &'static str { \"crate::a\" } // crate::a\n",
            "/* crate::a */\n",
            "pub struct B;\n",
        ),
    );
    ws.baseline("{}");

    let out = ws.check();
    assert_exit(&out, 0, "crate:: paths inside comments and strings");
}

#[test]
fn b3_cfg_test_item_before_production_code_is_excluded_while_later_edges_count() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/lib.rs", "mod a; mod b; mod c;\n");
    ws.write(
        "crates/demo/src/a.rs",
        "use crate::b::B;\nuse crate::c::C;\npub struct A;\n",
    );
    ws.write(
        "crates/demo/src/b.rs",
        concat!(
            "#[cfg(test)]\n",
            "mod t {\n",
            "    use crate::a::A;\n",
            "    fn brace_noise() { let _ = '{'; let _ = \"}\"; }\n",
            "}\n",
            "pub struct B;\n",
        ),
    );
    ws.write(
        "crates/demo/src/c.rs",
        concat!(
            "#[cfg(test)]\n",
            "mod t {\n",
            "    use crate::a::A;\n",
            "    fn brace_noise() { let _ = '{'; }\n",
            "}\n",
            "use crate::a::A;\n",
            "pub struct C;\n",
        ),
    );
    ws.baseline(r#"{"demo": [["a", "c"]]}"#);

    let out = ws.check();
    assert_exit(
        &out,
        0,
        "a cfg(test) module before production code: excluded, later edges kept",
    );
}

#[test]
fn only_test_implying_cfg_predicates_exclude_an_item() {
    let ws = TempWorkspace::demo();
    ws.write(
        "crates/demo/src/a.rs",
        "use crate::d::D; use crate::e::E; use crate::f::F; use crate::g::Holder; pub struct A;\n",
    );
    ws.write(
        "crates/demo/src/d.rs",
        concat!(
            "#[cfg(all(test, unix))]\n",
            "use crate::a::A;\n",
            "#[cfg(test)]\n",
            "#[allow(dead_code)]\n",
            "pub(crate) fn helper() -> crate::a::A { crate::a::A }\n",
            "pub struct Holder {\n",
            "    #[cfg(test)]\n",
            "    probe: crate::a::A,\n",
            "    pub keep: u8,\n",
            "}\n",
            "pub struct D;\n",
        ),
    );
    ws.write(
        "crates/demo/src/e.rs",
        concat!(
            "#[cfg(any(test, feature = \"fixtures\"))]\n",
            "pub fn fixture() -> crate::a::A { crate::a::A }\n",
            "pub struct E;\n",
        ),
    );
    ws.write(
        "crates/demo/src/f.rs",
        "#[cfg(not(test))]\nuse crate::a::A;\npub struct F;\n",
    );
    ws.write(
        "crates/demo/src/g.rs",
        concat!(
            "pub struct Holder {\n",
            "    #[cfg(test)]\n",
            "    probe: u8,\n",
            "    pub keep: crate::a::A,\n",
            "}\n",
        ),
    );
    ws.baseline(r#"{"demo": [["a", "e", "f", "g"]]}"#);

    let out = ws.check();
    assert_exit(
        &out,
        0,
        "test, all(test, …) and a test-only field are excluded; \
         any(test, feature), not(test) and the field after it count",
    );
}

#[test]
fn cfg_test_module_files_are_excluded_at_every_depth() {
    let ws = TempWorkspace::demo();
    ws.write(
        "crates/demo/src/lib.rs",
        "mod a; mod b;\n#[cfg(test)]\nmod t;\n",
    );
    ws.write("crates/demo/src/t.rs", "use crate::a::A;\n");
    ws.write(
        "crates/demo/src/a.rs",
        "use crate::t::T;\nuse crate::b::B;\npub struct A;\n",
    );
    ws.write(
        "crates/demo/src/b.rs",
        "pub struct B;\n#[cfg(test)]\nmod tests;\n",
    );
    ws.write("crates/demo/src/b/tests.rs", "use crate::a::A;\n");
    ws.baseline("{}");

    let out = ws.check();
    assert_exit(
        &out,
        0,
        "files of a cfg(test)-declared module, at the crate root and nested",
    );
}
