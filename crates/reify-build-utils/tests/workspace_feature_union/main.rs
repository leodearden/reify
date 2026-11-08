//! Invariant: `cargo test -p <member>` resolves every external package to the
//! feature set the base's `--workspace` build compiles, because each member
//! depends on the `workspace-feature-union` crate. Gate and remedy:
//! `scripts/workspace-feature-union.py`; design: docs/notes/workspace-feature-union.md.
//! Layout: harness.rs holds the shared stub cargo, fixtures and assertions;
//! generate.rs holds the `generate` fixtures.

use std::path::Path;
use std::process::Command;

mod generate;
mod harness;
mod profile_split;

use harness::{
    END_MARKER, Fixture, Member, TREE_A, TREE_UNION, UNION, UNION_NODE, assert_exit,
    assert_findings, assert_green, assert_input_error, assert_pin, covered_a, describe,
    empty_union_manifest, finding_lines, member, member_split_fixture, parse_toml, real_check,
    run_script, stdout_of, workspace_building_a_with_u, workspace_of, write_file,
};

// --- the real workspace ------------------------------------------------------

/// The gate. On failure the full output carries every finding line and the
/// remedy.
#[test]
fn real_workspace_resolves_every_member_like_the_workspace() {
    let out = real_check();
    assert!(
        out.status.success(),
        "a member resolves an external package unlike --workspace, or compiles a unit to \
         another profile; docs/notes/workspace-feature-union.md explains the remedy\n{}",
        describe(out)
    );
}

/// Every shipped binary builds these with serde, so no `-p` selection may
/// build them without it.
const SERDE_VOCABULARY_CRATES: [&str; 3] = ["reify-core", "reify-ast", "reify-ir"];

#[test]
fn real_workspace_resolves_the_serde_vocabulary_crates_like_the_workspace() {
    let out = real_check();
    let stdout = stdout_of(out);
    assert!(
        matches!(out.status.code(), Some(0 | 1))
            && !finding_lines(&stdout)
                .iter()
                .any(|line| line.starts_with("HOST-UNION ")),
        "check must compare the -p selections for its MEMBER-SPLIT lines to mean anything, \
         and a HOST-UNION finding stops it before it compares\n{}",
        describe(out)
    );
    let offending: Vec<&str> = stdout
        .lines()
        .filter(|line| {
            SERDE_VOCABULARY_CRATES
                .iter()
                .any(|name| line.starts_with(&format!("MEMBER-SPLIT {name} v")))
        })
        .collect();
    assert!(
        offending.is_empty(),
        "a -p selection builds a serde vocabulary crate unlike --workspace; a feature every \
         shipped binary enables is default-on, or is not a feature — see \
         docs/notes/workspace-feature-union.md, \"Member features\"\n{}\n{}",
        offending.join("\n"),
        describe(out)
    );
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
    // --workspace lists memchr's dependencies under a, so b's line is `(*)`.
    let ws_b = "\
b v0.1.0 (/fixture/crates/b)|
`-- memchr v2.0.0|std (*)
";
    let members = vec![member("a", &[UNION]), member("b", &[]), member(UNION, &[])];
    let fixture = Fixture::new(&members, &workspace_of(&[TREE_A, ws_b, TREE_UNION]))
        .tree("a", TREE_A)
        .tree("b", tree_b);
    let manifest = fixture.manifest("b");
    assert_findings(
        &fixture.check(),
        &[&["UNCOVERED b", UNION, &manifest]],
        "member b does not depend on the union crate",
    );
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
    let out = fixture.check();
    assert_findings(
        &out,
        &[&[
            "HOST-UNION u",
            "a -[build]-> u -> workspace-feature-union",
            "remove u's dependency on workspace-feature-union",
            &manifest,
        ]],
        "u is a build-dependency of a and depends on the union",
    );
    assert!(
        stdout_of(&out).contains("not compared while a HOST-UNION finding stands"),
        "check must say that it compared nothing\n{}",
        describe(&out)
    );
    let log = fixture.cargo_argv_log();
    assert!(
        !log.lines().any(|line| line.starts_with("tree")),
        "a HOST-UNION finding changes the host graph, so check must not resolve cargo tree \
         while one stands\n{log}"
    );
    assert!(
        !log.lines().any(|line| line.starts_with("test")),
        "a HOST-UNION finding changes the host graph, so check must not compare unit-graph \
         profiles while one stands\n{log}"
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

const PM_NODE: &str = "pm v0.1.0 (proc-macro) (/fixture/crates/pm)|";

/// `--workspace` with `a` depending on the proc-macro member `pm`, whose
/// subtree is `pm_children`; pm is also a root of its own, which is `(*)`
/// when it has children, because a's line lists them.
fn workspace_using_proc_macro_pm(pm_children: &[&str]) -> String {
    let below_pm: String = pm_children
        .iter()
        .map(|line| format!("    {line}\n"))
        .collect();
    let tree_a = format!(
        "a v0.1.0 (/fixture/crates/a)|\n\
         |-- {UNION_NODE}\n\
         `-- {PM_NODE}\n\
         {below_pm}"
    );
    let tree_pm = if pm_children.is_empty() {
        format!("{PM_NODE}\n")
    } else {
        format!("{PM_NODE} (*)\n")
    };
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
        "one finding for pm, though pm is printed under a and as a root of its own",
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

// --- side attribution --------------------------------------------------------

/// Real cargo 1.96 output from a scratch workspace: u -> memchr, zz -> union
/// with `[build-dependencies] u`. u's two roots are character-identical.
const WS_ZZ_BUILDS_WITH_U_MEMCHR: &str = "\
u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|alloc,default,std

u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|alloc,default,std

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_ZZ_BUILDS_WITH_U_MEMCHR: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- memchr v2.8.1|alloc,default,std
";

#[test]
fn check_and_generate_are_green_when_a_host_side_members_host_root_is_printed_before_its_build_edge()
 {
    let members = vec![
        member("u", &[]),
        member("zz", &[UNION]).build_dep("u"),
        member(UNION, &[]),
    ];
    let fixture = Fixture::new(&members, WS_ZZ_BUILDS_WITH_U_MEMCHR)
        .tree("zz", TREE_ZZ_BUILDS_WITH_U_MEMCHR)
        .with_union_manifest(&empty_union_manifest());
    assert_green(
        &fixture.check(),
        "zz's `(*)` build edge to u resolves to u's host root, so --workspace builds a host memchr",
    );
    let out = fixture.generate();
    assert_exit(&out, 0, "generate finds no split to pin");
    assert!(
        parse_toml(&fixture.union_manifest(), "the rewritten union manifest")
            .get("target")
            .is_none(),
        "no split, so no pin\n{}",
        describe(&out)
    );
}

/// Real cargo 1.96 output from a scratch workspace: the shape of
/// [`WS_ZZ_BUILDS_WITH_U_MEMCHR`] plus yy -> log, memchr[logging] and
/// zz -> memchr, so u's target root carries logging and its host root does not.
const WS_U_ROOTS_DIFFER_IN_MEMCHR_LOGGING: &str = "\
u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|alloc,default,std

u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|alloc,default,logging,std
    `-- log v0.4.30|

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

yy v0.1.0 (/fixture/crates/yy)|
|-- log v0.4.30|
|-- memchr v2.8.1|alloc,default,logging,std (*)
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
|-- memchr v2.8.1|alloc,default,logging,std (*)
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_YY_MEMCHR_LOGGING: &str = "\
yy v0.1.0 (/fixture/crates/yy)|
|-- log v0.4.30|
|-- memchr v2.8.1|alloc,default,logging,std
|   `-- log v0.4.30|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";

const TREE_ZZ_MEMCHR_WITHOUT_LOGGING: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
|-- memchr v2.8.1|alloc,default,std
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- memchr v2.8.1|alloc,default,std
";

const TREE_ZZ_MEMCHR_PINNED_WITH_LOGGING: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
|-- memchr v2.8.1|alloc,default,logging,std
|   `-- log v0.4.30|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
    `-- memchr v2.8.1|alloc,default,logging,std (*)
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- memchr v2.8.1|alloc,default,std
";

fn yy_and_zz_build_with_u() -> Vec<Member> {
    vec![
        member("u", &[]),
        member("yy", &[UNION]),
        member("zz", &[UNION]).build_dep("u"),
        member(UNION, &[]),
    ]
}

#[test]
fn check_reds_and_generate_pins_a_target_split_that_a_host_root_used_to_mask() {
    let fixture = Fixture::new(
        &yy_and_zz_build_with_u(),
        WS_U_ROOTS_DIFFER_IN_MEMCHR_LOGGING,
    )
    .tree("yy", TREE_YY_MEMCHR_LOGGING)
    .tree("zz", TREE_ZZ_MEMCHR_WITHOUT_LOGGING)
    .tree_after("zz", "^memchr ", TREE_ZZ_MEMCHR_PINNED_WITH_LOGGING)
    .with_union_manifest(&empty_union_manifest());

    assert_findings(
        &fixture.check(),
        &[&[
            "SPLIT zz ",
            "memchr v2.8.1",
            "(target)",
            "{alloc,default,std}",
            "{alloc,default,logging,std}",
        ]],
        "--workspace builds target memchr with logging only; the root without it is u's host copy",
    );
    let out = fixture.generate();
    assert_exit(&out, 0, "generate pins target memchr with logging");
    assert_pin(
        &parse_toml(&fixture.union_manifest(), "the rewritten union manifest"),
        "dependencies",
        "memchr",
        r#"version = "2"
features = ["alloc", "logging", "std"]"#,
    );
    assert_green(&fixture.check(), "check after the pin");
}

/// Real cargo 1.96 output from a scratch workspace: u names memchr[logging] in
/// both [dependencies] and [build-dependencies], so its two roots are
/// character-identical and only the first can be its host copy.
const WS_U_IDENTICAL_ROOTS_HOST_COPY_FIRST: &str = "\
u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.3|alloc,default,logging,std
    `-- log v0.4.34|
[build-dependencies]
`-- memchr v2.8.3|alloc,default,logging,std (*)

u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.3|alloc,default,logging,std
    `-- log v0.4.34|
[build-dependencies]
`-- memchr v2.8.3|alloc,default,logging,std (*)

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_ZZ_BUILDS_WITH_U_MEMCHR_IN_BOTH_SECTIONS: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- memchr v2.8.3|alloc,default,logging,std
        `-- log v0.4.34|
    [build-dependencies]
    `-- memchr v2.8.3|alloc,default,logging,std (*)
";

#[test]
fn check_is_green_when_the_first_of_two_identical_roots_is_the_host_copy() {
    let members = vec![
        member("u", &[]),
        member("zz", &[UNION]).build_dep("u"),
        member(UNION, &[]),
    ];
    let fixture = Fixture::new(&members, WS_U_IDENTICAL_ROOTS_HOST_COPY_FIRST)
        .tree("zz", TREE_ZZ_BUILDS_WITH_U_MEMCHR_IN_BOTH_SECTIONS);
    assert_green(
        &fixture.check(),
        "with u's first root on TARGET, its [build-dependencies] memchr `(*)` precedes every \
         HOST line of memchr, so only the first root can be the host copy",
    );
}

/// Real cargo 1.96 output from a scratch workspace: u -> log, yy ->
/// log[max_level_debug] and zz -> log with `[build-dependencies] u`. log is a
/// leaf, so only one feature set per side tells u's two roots apart.
const WS_U_ROOTS_DIFFER_IN_LEAF_LOG: &str = "\
u v0.1.0 (/fixture/crates/u)|
`-- log v0.4.30|

u v0.1.0 (/fixture/crates/u)|
`-- log v0.4.30|max_level_debug

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

yy v0.1.0 (/fixture/crates/yy)|
|-- log v0.4.30|max_level_debug
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
|-- log v0.4.30|max_level_debug
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_YY_LOG_MAX_LEVEL_DEBUG: &str = "\
yy v0.1.0 (/fixture/crates/yy)|
|-- log v0.4.30|max_level_debug
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";

const TREE_ZZ_LOG_WITHOUT_FEATURES: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
|-- log v0.4.30|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- log v0.4.30|
";

#[test]
fn check_reds_on_a_leaf_target_split_that_a_host_root_used_to_mask() {
    let out = Fixture::new(&yy_and_zz_build_with_u(), WS_U_ROOTS_DIFFER_IN_LEAF_LOG)
        .tree("yy", TREE_YY_LOG_MAX_LEVEL_DEBUG)
        .tree("zz", TREE_ZZ_LOG_WITHOUT_FEATURES)
        .check();
    assert_findings(
        &out,
        &[&[
            "SPLIT zz ",
            "log v0.4.30",
            "(target)",
            "-p resolves {}",
            "{max_level_debug}",
        ]],
        "--workspace builds target log with max_level_debug only",
    );
}

/// Real cargo 1.96 output from a scratch workspace: a -> b, b dev-depends on
/// c, c and zz build with u, u -> memchr. u's host copy is expanded only
/// inside b's `[dev-dependencies]` section below a.
const WS_U_HOST_COPY_EXPANDED_BELOW_A_DEV_SECTION: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- b v0.1.0 (/fixture/crates/b)|
|   `-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|   [dev-dependencies]
|   `-- c v0.1.0 (/fixture/crates/c)|
|       `-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
|       [build-dependencies]
|       `-- u v0.1.0 (/fixture/crates/u)|
|           `-- memchr v2.8.1|alloc,default,std
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

b v0.1.0 (/fixture/crates/b)| (*)

c v0.1.0 (/fixture/crates/c)| (*)

u v0.1.0 (/fixture/crates/u)| (*)

u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|alloc,default,std

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_A_WITH_B: &str = "\
a v0.1.0 (/fixture/crates/a)|
|-- b v0.1.0 (/fixture/crates/b)|
|   `-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
";

const TREE_B_DEV_DEPENDS_ON_C: &str = "\
b v0.1.0 (/fixture/crates/b)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[dev-dependencies]
`-- c v0.1.0 (/fixture/crates/c)|
    `-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
    [build-dependencies]
    `-- u v0.1.0 (/fixture/crates/u)|
        `-- memchr v2.8.1|alloc,default,std
";

const TREE_C_BUILDS_WITH_U: &str = "\
c v0.1.0 (/fixture/crates/c)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- memchr v2.8.1|alloc,default,std
";

#[test]
fn check_finds_a_host_variant_whose_only_expanded_line_sits_below_a_dev_section() {
    let members = vec![
        member("a", &[UNION, "b"]),
        member("b", &[UNION]).dev_dep("c"),
        member("c", &[UNION]).build_dep("u"),
        member("u", &[]),
        member("zz", &[UNION]).build_dep("u"),
        member(UNION, &[]),
    ];
    assert_green(
        &Fixture::new(&members, WS_U_HOST_COPY_EXPANDED_BELOW_A_DEV_SECTION)
            .tree("a", TREE_A_WITH_B)
            .tree("b", TREE_B_DEV_DEPENDS_ON_C)
            .tree("c", TREE_C_BUILDS_WITH_U)
            .tree("zz", TREE_ZZ_BUILDS_WITH_U_MEMCHR)
            .check(),
        "the host memchr below b's dev-dependency c is one --workspace builds: b is a root",
    );
}

/// u's two roots differ only in memchr's features, and nothing else on
/// either side tells which root is the host copy.
const WS_U_ROOTS_UNPLACEABLE: &str = "\
u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|alloc

u v0.1.0 (/fixture/crates/u)|
`-- memchr v2.8.1|std

workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|

zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)| (*)
";

const TREE_ZZ_BUILDS_WITH_U_MEMCHR_ALLOC: &str = "\
zz v0.1.0 (/fixture/crates/zz)|
`-- workspace-feature-union v0.1.0 (/fixture/crates/workspace-feature-union)|
[build-dependencies]
`-- u v0.1.0 (/fixture/crates/u)|
    `-- memchr v2.8.1|alloc
";

#[test]
fn check_exits_2_when_the_tree_cannot_say_which_root_of_a_member_is_its_host_copy() {
    let members = vec![
        member("u", &[]),
        member("zz", &[UNION]).build_dep("u"),
        member(UNION, &[]),
    ];
    assert_input_error(
        &Fixture::new(&members, WS_U_ROOTS_UNPLACEABLE)
            .tree("zz", TREE_ZZ_BUILDS_WITH_U_MEMCHR_ALLOC)
            .check(),
        &["which root of u v0.1.0 is its host copy"],
        "both side assignments obey cargo tree's printing rules and put memchr's features on \
         different sides",
    );
}

// --- member features ---------------------------------------------------------

const MEMBER_SPLIT_B: &str = "MEMBER-SPLIT b v0.1.0 (target): -p resolves {} under a, d; \
                              --workspace resolves {test-support}";

fn member_split_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("MEMBER-SPLIT "))
        .collect()
}

#[test]
fn check_reports_a_member_feature_split_as_advisory_naming_the_selections_that_resolve_it() {
    let out = member_split_fixture(&[]).check();
    assert_green(&out, "a member feature split is advisory, never a finding");
    assert_eq!(
        member_split_lines(&stdout_of(&out)),
        [MEMBER_SPLIT_B],
        "one advisory line for b, naming only the selections that resolve it unlike \
         --workspace\n{}",
        describe(&out)
    );
}

/// The serde gate accepts a failing check, so it holds only while a check
/// that fails on coverage still prints every advisory line. A HOST-UNION
/// failure compares nothing
/// (check_reds_when_a_build_dependency_member_puts_the_union_on_the_host_side).
#[test]
fn check_reports_a_member_feature_split_on_a_failing_check_without_counting_it_as_a_finding() {
    let fixture = member_split_fixture(&["e"]);
    let manifest = fixture.manifest("e");
    let out = fixture.check();
    assert_findings(
        &out,
        &[&["UNCOVERED e", UNION, &manifest]],
        "member e does not depend on the union crate, and b's split is no finding",
    );
    assert_eq!(
        member_split_lines(&stdout_of(&out)),
        [MEMBER_SPLIT_B],
        "a failing check still prints b's advisory line\n{}",
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
    let seven_undecided_pairs: String = (1..=7)
        .map(|k| {
            format!(
                "\np{k} v0.1.0 (/fixture/crates/p{k})|a\n\np{k} v0.1.0 (/fixture/crates/p{k})|b"
            )
        })
        .collect();
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
        (
            "a `(*)` line before any line lists that package's dependencies",
            "|-- serde v1.0.0|std (*)",
            "serde v1.0.0|std (*)",
        ),
        (
            "two feature sets of one package on one side",
            "|-- serde v1.0.0|std\n|-- serde v1.0.0|rc",
            "serde v1.0.0|rc",
        ),
        (
            "a package printed as a root three times",
            "\na v0.1.0 (/fixture/crates/a)|\n\na v0.1.0 (/fixture/crates/a)|",
            "3 roots of a v0.1.0",
        ),
        (
            "a proc-macro printed as two roots",
            "\npm v0.1.0 (proc-macro) (/fixture/crates/pm)|\n\n\
             pm v0.1.0 (proc-macro) (/fixture/crates/pm)|",
            "2 roots of proc-macro pm v0.1.0",
        ),
        (
            "a pair of roots that breaks a printing rule on either side",
            "\nu v0.1.0 (/fixture/crates/u)|\n`-- memchr v2.0.0|std (*)\n\n\
             u v0.1.0 (/fixture/crates/u)|\n`-- memchr v2.0.0|std (*)",
            "roots printed twice (u v0.1.0) obeys cargo tree's printing rules; the last one \
             tried breaks at `cargo tree -p a` line 4",
        ),
        (
            "more pairs than the check tries side assignments for",
            seven_undecided_pairs.as_str(),
            "128 side assignments of the roots printed twice",
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
