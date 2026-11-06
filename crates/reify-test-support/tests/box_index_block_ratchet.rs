//! Workspace ratchet: the axis-aligned box's triangle index block has exactly
//! one definition, `crates/reify-ir/src/box_triangle_indices.rs`.
//!
//! Every box fixture builds its triangles from that data file, through
//! `reify_test_support::mesh_fixtures::prismatic_box_mesh` /
//! `unit_cube_mesh`, `reify_kernel_openvdb::test_fixtures::axis_aligned_box` /
//! `box_mesh`, or an `include!` of the file where neither is linkable. A
//! second copy of the block is how fixtures drift apart, so this sweep reds
//! on any copy outside the definition.
//!
//! # The needle
//!
//! The needle is the block's first 12 indices, its bottom and top faces. Copies
//! that differ only in how a side quad is split into two triangles all share
//! them, so the prefix catches a reintroduced copy of any such variant. It is
//! read from the data file with `include!`, which gives the needle a single
//! point of truth and keeps this file's own source free of the literal.
//!
//! Matching ignores `//` comments, whitespace and `[`/`]`, so flat, compact
//! and nested `[[u32; 3]]` spellings all match, and it requires a number
//! boundary on both sides. Accepted blind spots: block comments, typed
//! literals such as `0u32`, and a box whose bottom or top faces use another
//! diagonal or triangle order.
//!
//! # The positive control
//!
//! The one sanctioned site is the definition itself. If the needle ever stops
//! matching it, `partition_enforced_and_stale` reports that row stale and the
//! sweep reds instead of passing over a workspace it can no longer see.

use reify_test_support::ignore_hygiene::walk_rs_files;
use reify_test_support::workspace_root::workspace_root;
use reify_test_support::workspace_sweep::assert_workspace_rs_walk_is_healthy;
use reify_test_support::{partition_enforced_and_stale, prefixed_tempdir};
use std::path::Path;

const BOX_INDICES: [u32; 36] = include!("../../reify-ir/src/box_triangle_indices.rs");

const DEFINITION: &str = "crates/reify-ir/src/box_triangle_indices.rs";

const SANCTIONED_SITES: &[(&str, usize, &str)] = &[(
    DEFINITION,
    1,
    "the single definition of the box index block, which every box fixture include!s",
)];

/// The block's bottom and top faces, comma-joined with no whitespace.
fn needle() -> String {
    join_indices(&BOX_INDICES[..12], ",")
}

fn join_indices(indices: &[u32], separator: &str) -> String {
    indices
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(separator)
}

/// The 1-based line on which each occurrence of `needle` in `source` starts.
///
/// Each line is cut at `//`, and whitespace, `[` and `]` are dropped before
/// matching. An occurrence counts only if no digit touches it on either side.
fn needle_lines(source: &str, needle: &str) -> Vec<usize> {
    let mut kept = Vec::new();
    let mut line_of_kept = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        for byte in code.bytes() {
            if !byte.is_ascii_whitespace() && byte != b'[' && byte != b']' {
                kept.push(byte);
                line_of_kept.push(index + 1);
            }
        }
    }

    let needle = needle.as_bytes();
    let is_digit_at =
        |at: Option<usize>| at.and_then(|i| kept.get(i)).is_some_and(u8::is_ascii_digit);
    (0..kept.len())
        .filter(|&start| kept[start..].starts_with(needle))
        .filter(|&start| {
            !is_digit_at(start.checked_sub(1)) && !is_digit_at(Some(start + needle.len()))
        })
        .map(|start| line_of_kept[start])
        .collect()
}

/// Every occurrence of `needle` in a `.rs` file under `workspace_root`, as
/// `"{path}: line {n}"` with a `/`-separated workspace-relative path, sorted.
///
/// Unreadable files are skipped; `assert_workspace_rs_walk_is_healthy` bounds
/// how many there may be.
fn needle_sites(workspace_root: &Path, needle: &str) -> Vec<String> {
    let mut sites: Vec<String> = walk_rs_files(workspace_root, |_| true)
        .into_iter()
        .filter_map(|path| {
            std::fs::read_to_string(&path)
                .ok()
                .map(|source| (path, source))
        })
        .flat_map(|(path, source)| {
            let relative = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            needle_lines(&source, needle)
                .into_iter()
                .map(move |line| format!("{relative}: line {line}"))
        })
        .collect();
    sites.sort();
    sites
}

/// The block as a flat literal, one face per line, starting on line 2.
fn flat_spelling() -> String {
    let faces: Vec<String> = BOX_INDICES
        .chunks_exact(6)
        .map(|face| format!("    {},\n", join_indices(face, ", ")))
        .collect();
    format!("let indices: Vec<u32> = vec![\n{}];\n", faces.concat())
}

/// The block with no space inside a triangle, starting on line 3.
fn compact_spelling() -> String {
    let triangles: Vec<String> = BOX_INDICES
        .chunks_exact(3)
        .map(|triangle| join_indices(triangle, ","))
        .collect();
    format!(
        "fn cube() {{\n    let i = [\n        {},\n    ];\n}}\n",
        triangles.join(", ")
    )
}

/// The block as nested `[u32; 3]` triangles, starting on line 2.
fn nested_spelling() -> String {
    let triangles: Vec<String> = BOX_INDICES
        .chunks_exact(3)
        .map(|triangle| format!("    [{}],\n", join_indices(triangle, ", ")))
        .collect();
    format!(
        "let tris: Vec<[u32; 3]> = vec![\n{}];\n",
        triangles.concat()
    )
}

#[test]
fn each_spelling_of_the_block_is_found_once_at_its_first_line() {
    assert_eq!(needle_lines(&flat_spelling(), &needle()), vec![2], "flat");
    assert_eq!(
        needle_lines(&compact_spelling(), &needle()),
        vec![3],
        "compact"
    );
    assert_eq!(
        needle_lines(&nested_spelling(), &needle()),
        vec![2],
        "nested"
    );
}

#[test]
fn a_commented_out_block_is_not_found() {
    let commented: String = flat_spelling()
        .lines()
        .map(|line| format!("// {line}\n"))
        .collect();

    assert_eq!(needle_lines(&commented, &needle()), Vec::<usize>::new());
}

#[test]
fn a_block_touching_another_digit_is_not_found() {
    let flat = join_indices(&BOX_INDICES, ", ");
    let preceded = format!("let i = [1{flat}];\n");
    let followed = format!("let i = [{}0];\n", join_indices(&BOX_INDICES[..12], ", "));

    assert_eq!(
        needle_lines(&preceded, &needle()),
        Vec::<usize>::new(),
        "preceded"
    );
    assert_eq!(
        needle_lines(&followed, &needle()),
        Vec::<usize>::new(),
        "followed"
    );
}

#[test]
fn two_blocks_are_found_at_two_lines() {
    let source = format!("{}{}", flat_spelling(), nested_spelling());

    assert_eq!(needle_lines(&source, &needle()), vec![2, 10]);
}

/// A fixture workspace holding `files`, each `(relative path, contents)`.
fn fixture_workspace(files: &[(&str, String)]) -> reify_test_support::TempDir {
    let workspace = prefixed_tempdir("box-index-ratchet-");
    for (relative, contents) in files {
        let path = workspace.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture path has a parent"))
            .expect("create fixture dir");
        std::fs::write(&path, contents).expect("write fixture file");
    }
    workspace
}

#[test]
fn a_copy_outside_the_definition_is_enforced() {
    let workspace = fixture_workspace(&[
        (DEFINITION, flat_spelling()),
        ("crates/other/tests/cube.rs", nested_spelling()),
    ]);

    let (enforced, stale) =
        partition_enforced_and_stale(&needle_sites(workspace.path(), &needle()), SANCTIONED_SITES);

    assert_eq!(
        enforced,
        vec!["crates/other/tests/cube.rs: line 2".to_string()]
    );
    assert_eq!(stale, Vec::<String>::new());
}

#[test]
fn a_definition_without_the_block_is_stale() {
    let workspace = fixture_workspace(&[(DEFINITION, "[]\n".to_string())]);

    let (enforced, stale) =
        partition_enforced_and_stale(&needle_sites(workspace.path(), &needle()), SANCTIONED_SITES);

    assert_eq!(enforced, Vec::<String>::new());
    assert_eq!(stale, vec![DEFINITION.to_string()]);
}

#[test]
fn box_index_block_has_one_definition_in_the_workspace() {
    let root = workspace_root();
    assert_workspace_rs_walk_is_healthy(root, DEFINITION);

    let (enforced, stale) =
        partition_enforced_and_stale(&needle_sites(root, &needle()), SANCTIONED_SITES);

    assert!(
        stale.is_empty(),
        "the needle no longer matches its own definition in {stale:?}, so this \
         sweep is blind to copies of the box index block. Check that \
         needle_lines still recognises the data file's spelling."
    );
    assert!(
        enforced.is_empty(),
        "found {} copy(ies) of the box index block outside {DEFINITION}:\n  {}\n\n\
         Build the mesh with reify_test_support::mesh_fixtures::prismatic_box_mesh / \
         unit_cube_mesh or reify_kernel_openvdb::test_fixtures::axis_aligned_box / \
         box_mesh, or include! {DEFINITION} where neither is linkable.",
        enforced.len(),
        enforced.join("\n  ")
    );
}
