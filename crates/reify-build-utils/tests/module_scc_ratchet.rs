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

/// The `u -> v` of every closing-import line listed under a GREW line.
fn closing_edges(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("  ") && line.contains(" -> "))
        .filter_map(|line| line.rsplit(": ").next())
        .collect()
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

    fn remove(&self, rel: &str) {
        let path = self.root().join(rel);
        fs::remove_file(&path).unwrap_or_else(|e| panic!("remove {}: {e}", path.display()));
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

/// Exit 2 — the inputs could not be trusted — with stderr naming `culprit`.
fn assert_input_error(out: &Output, culprit: &str, context: &str) {
    assert_exit(out, 2, context);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(culprit),
        "{context}: expected stderr to name {culprit:?}\n{}",
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
fn merging_two_baselined_sccs_is_one_grew_listing_only_the_cross_scc_imports() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::b::B;\npub struct A;\n");
    ws.write(
        "crates/demo/src/b.rs",
        "use crate::a::A;\nuse crate::c::C;\npub struct B;\n",
    );
    ws.write("crates/demo/src/c.rs", "use crate::d::D;\npub struct C;\n");
    ws.write(
        "crates/demo/src/d.rs",
        "use crate::c::C;\nuse crate::a::A;\npub struct D;\n",
    );
    ws.baseline(r#"{"demo": [["a", "b"], ["c", "d"]]}"#);

    let out = ws.check();
    assert_exit(&out, 1, "two baselined SCCs merged by b -> c and d -> a");
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("GREW demo: {a, b, c, d}"),
        "{}",
        describe(&out)
    );
    assert_eq!(
        closing_edges(&stdout),
        ["b -> c", "d -> a"],
        "only the imports that join the two old SCCs close the merge\n{}",
        describe(&out)
    );
    assert!(
        has_line_with_all(&stdout, &["crates/demo/src/b.rs:2:", "b -> c"]),
        "{}",
        describe(&out)
    );
    assert!(
        has_line_with_all(&stdout, &["crates/demo/src/d.rs:2:", "d -> a"]),
        "{}",
        describe(&out)
    );
    assert!(
        !stdout.lines().any(|line| line.starts_with("STALE ")),
        "an old SCC swallowed by a GREW one is not also STALE\n{}",
        describe(&out)
    );
}

#[test]
fn a_partly_shrunk_scc_is_stale_naming_what_remains() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::b::B;\npub struct A;\n");
    ws.write("crates/demo/src/b.rs", "use crate::a::A;\npub struct B;\n");
    ws.write("crates/demo/src/c.rs", "use crate::a::A;\npub struct C;\n");
    ws.baseline(r#"{"demo": [["a", "b", "c"]]}"#);

    let out = ws.check();
    assert_exit(&out, 1, "c left the baselined {a, b, c}");
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("STALE demo: baseline {a, b, c} measured {a, b} — lower the baseline"),
        "{}",
        describe(&out)
    );
    assert!(
        !stdout.lines().any(|line| line.starts_with("GREW ")),
        "a shrink is not a growth\n{}",
        describe(&out)
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
fn super_paths_that_climb_to_the_crate_root_are_edges() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use super::b::B;\npub struct A;\n");
    ws.write("crates/demo/src/b/mod.rs", "pub mod y;\npub struct B;\n");
    ws.write(
        "crates/demo/src/b/y.rs",
        "use super::super::{a::A, c::C};\nuse super::B;\n",
    );
    ws.write(
        "crates/demo/src/c.rs",
        "mod inner {\n    use super::super::a::A;\n}\npub struct C;\n",
    );
    ws.baseline("{}");

    let out = ws.check();
    assert_exit(&out, 1, "a cycle closed only through super:: paths");
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("GREW demo: {a, b, c}"),
        "{}",
        describe(&out)
    );
    assert_eq!(
        closing_edges(&stdout),
        ["a -> b", "b -> a", "b -> c", "c -> a"],
        "{}",
        describe(&out)
    );
    for (site, edge) in [
        ("crates/demo/src/a.rs:1:", "a -> b"),
        ("crates/demo/src/b/y.rs:1:", "b -> a"),
        ("crates/demo/src/b/y.rs:1:", "b -> c"),
        ("crates/demo/src/c.rs:2:", "c -> a"),
    ] {
        assert!(
            has_line_with_all(&stdout, &[site, edge]),
            "{site} {edge}\n{}",
            describe(&out)
        );
    }
}

#[test]
fn super_paths_that_stay_inside_their_top_level_module_are_not_edges() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "use crate::e::E;\npub struct A;\n");
    ws.write(
        "crates/demo/src/e/mod.rs",
        concat!(
            "mod a;\n",
            "mod inner {\n",
            "    use super::a::Thing;\n",
            "}\n",
            "pub(super) fn visible() {}\n",
            "pub struct E;\n",
        ),
    );
    ws.write(
        "crates/demo/src/e/a.rs",
        "use super::super::e::E;\npub struct Thing;\n",
    );
    ws.baseline("{}");

    let out = ws.check();
    assert_exit(
        &out,
        0,
        "super::a inside e's inline module names e::a, not the top-level a",
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

#[test]
fn excluded_crates_are_not_measured_and_print_preserves_the_exclusion() {
    let ws = TempWorkspace::new(&[("demo", "crates/demo"), ("skipped", "crates/skipped")]);
    ws.write("crates/demo/src/a.rs", "pub struct A;\n");
    ws.write("crates/skipped/src/a.rs", "use crate::b::B;\n");
    ws.write("crates/skipped/src/b.rs", "use crate::a::A;\n");
    ws.write(
        "baseline.json",
        r#"{"schema": 1, "excluded": {"skipped": "glob imports hide edges"}, "crates": {}}"#,
    );

    assert_exit(&ws.check(), 0, "an excluded crate's cycle is not measured");

    let printed = ws.print();
    assert_exit(&printed, 0, "print with an excluded crate");
    let stdout = stdout_of(&printed);
    assert!(
        stdout.contains(r#""skipped": "glob imports hide edges""#),
        "{}",
        describe(&printed)
    );
    assert!(stdout.contains(r#""crates": {}"#), "{}", describe(&printed));
}

#[test]
fn fails_closed_on_a_missing_or_unparseable_manifest() {
    let missing = TempWorkspace::demo();
    missing.baseline("{}");
    missing.remove("Cargo.toml");
    assert_input_error(
        &missing.check(),
        "Cargo.toml",
        "check without a root manifest",
    );
    assert_input_error(
        &missing.print(),
        "Cargo.toml",
        "print without a root manifest",
    );

    let unparseable = TempWorkspace::demo();
    unparseable.baseline("{}");
    unparseable.write("Cargo.toml", "[workspace\nmembers = [\n");
    assert_input_error(
        &unparseable.check(),
        "Cargo.toml",
        "check with an unparseable root manifest",
    );
}

#[test]
fn fails_closed_on_a_missing_or_malformed_baseline() {
    let missing = TempWorkspace::demo();
    assert_input_error(&missing.check(), "baseline.json", "no baseline file");

    let cases = [
        ("{not json", "baseline.json", "unparseable baseline"),
        (
            r#"{"schema": 2, "excluded": {}, "crates": {}}"#,
            "schema",
            "unknown baseline schema",
        ),
        (
            r#"{"schema": 1, "excluded": {}, "crates": {"demo": [["a"]]}}"#,
            "demo",
            "an SCC row of fewer than two modules",
        ),
    ];
    for (baseline, culprit, context) in cases {
        let ws = TempWorkspace::demo();
        ws.write("baseline.json", baseline);
        assert_input_error(&ws.check(), culprit, context);
    }
}

#[test]
fn fails_closed_on_a_contradictory_baseline() {
    let ghost = TempWorkspace::demo();
    ghost.write(
        "baseline.json",
        r#"{"schema": 1, "excluded": {"ghost": "no such package"}, "crates": {}}"#,
    );
    assert_input_error(
        &ghost.check(),
        "ghost",
        "excluding a package that is not a workspace member",
    );

    let both = TempWorkspace::demo();
    both.write(
        "baseline.json",
        r#"{"schema": 1, "excluded": {"demo": "skip"}, "crates": {"demo": [["a", "b"]]}}"#,
    );
    assert_input_error(
        &both.check(),
        "demo",
        "a package both excluded and baselined",
    );
}

#[test]
fn fails_closed_when_the_lexer_ends_a_file_inside_a_comment() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "pub struct A;\n");
    ws.write(
        "crates/demo/src/c.rs",
        "/* unterminated\nuse crate::a::A;\n",
    );
    ws.baseline("{}");

    assert_input_error(
        &ws.check(),
        "c.rs",
        "check over a file whose lexer state is unbalanced at EOF",
    );
    assert_input_error(
        &ws.print(),
        "c.rs",
        "print over a file whose lexer state is unbalanced at EOF",
    );
}

#[test]
fn fails_closed_when_a_source_path_cannot_be_read() {
    let ws = TempWorkspace::demo();
    ws.write("crates/demo/src/a.rs", "pub struct A;\n");
    ws.write(
        "crates/demo/src/c.rs/README",
        "a directory, not a source file\n",
    );
    ws.baseline("{}");

    assert_input_error(
        &ws.check(),
        "c.rs",
        "check over a `*.rs` path that is a directory",
    );
}

#[test]
fn workspace_module_sccs_match_the_committed_baseline() {
    let root = workspace_root();
    let out = run_tool([
        OsStr::new("check"),
        OsStr::new("--root"),
        root.as_os_str(),
        OsStr::new("--baseline"),
        root.join("scripts/module-scc-baseline.json").as_os_str(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the workspace module-SCC ratchet (INV-ARCH-1) is red. \
         `python3 scripts/assert-module-scc.py --help` explains GREW and STALE \
         and how to lower a baseline row.\n{}",
        describe(&out)
    );
}
