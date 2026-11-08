//! `generate` fixtures: the union manifest's generated region is rewritten until
//! every member resolves like `--workspace`, and left byte-identical on refusal.

use std::fs;

use crate::harness::{
    BEGIN_MARKER, END_MARKER, Fixture, HOST, TREE_A, TREE_UNION, UNION, UNION_NODE, UNION_PACKAGE,
    assert_exit, assert_findings, assert_green, assert_input_error, assert_pin, covered_a,
    describe, empty_union_manifest, member, member_split_fixture, parse_toml,
    workspace_building_a_with_u, workspace_of,
};

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
    let fixture = member_split_fixture(&[]).with_union_manifest(&empty_union_manifest());
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
