//! Executable drift guard between `geometry`'s in-crate test cube fixture and
//! the workspace-canonical box fixtures in `reify_test_support::fixtures`.
//!
//! # Why this file exists
//!
//! Task #6387 hoisted the box/cube fixtures into `reify_test_support::fixtures`
//! so the workspace has ONE definition of "a box". Task #7137 collapsed the
//! remaining copies onto it — except this crate's, which cannot be collapsed
//! and so is watched instead. See the cite block on
//! `geometry::tests::unit_cube_mesh` for the full argument; in short:
//!
//! 1. Delegating WOULD NOT COMPILE. That copy lives in `#[cfg(test)] mod tests`
//!    inside the crate under test, so its `Mesh` comes from the `--test` build
//!    of `reify-ir` while `reify-test-support` links the PLAIN `reify-ir` rlib.
//!    The two are distinct crate instances, so the canonical fixture returns a
//!    DIFFERENT `Mesh` type ("perhaps two different versions of crate
//!    `reify_ir`") — an unresolved-type error, not a style preference.
//! 2. The escape would invert the layering. `reify-test-support` normal-deps
//!    `reify-compiler`, which normal-deps `reify-ir`, so giving `reify-ir` a
//!    `reify-test-support` dev-dep would make `cargo test -p reify-ir` build
//!    the compiler in order to test the IR.
//!
//! # Why this guard is SOURCE-LEVEL rather than value-level
//!
//! Reason 1 above also blocks the obvious guard: no test in any crate can CALL
//! the local fixture, because `#[cfg(test)]` items are not part of any artifact
//! another crate can link. Comparing constructed `Mesh` values is therefore
//! unavailable, and comparing the two definitions' SOURCE TEXT is the only
//! executable option that does not buy the dependency edge reason 2 rejects.
//!
//! # Why indices only
//!
//! Winding/topology is precisely what drifted (#7137 found the `+Y` face
//! emitted as `3, 6, 2,  3, 7, 6` here against the canonical's
//! `3, 7, 6,  3, 6, 2`), and it is what makes independent guards disagree about
//! what "a box" is. Vertices are not textually comparable anyway — the
//! canonical `prismatic_box_mesh` spells them parametrically (`lx, 0.0, 0.0`) —
//! and their values are already pinned upstream by
//! `crates/reify-test-support/tests/box_fixtures.rs`.

/// Resolves the manifest directory to use when locating this crate's sources at
/// test time.
///
/// Prefers the runtime `CARGO_MANIFEST_DIR` (correct for whatever worktree is
/// actually running the test) over the compile-time `env!()` bake, which
/// goes stale when a seeded warm-lane `target/` is reused from a
/// since-deleted worktree (`CARGO_MANIFEST_DIR` is not part of cargo's
/// fingerprint, so a content-identical rebuild is never triggered). See
/// esc-4906-57.
fn resolve_manifest_dir(runtime: Result<String, std::env::VarError>) -> String {
    runtime.unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string())
}

fn workspace_root() -> std::path::PathBuf {
    // crates/reify-ir -> crates -> <workspace root>
    std::path::Path::new(&resolve_manifest_dir(std::env::var("CARGO_MANIFEST_DIR")))
        .parent()
        .and_then(|p| p.parent())
        .expect("manifest dir must have two ancestors (crates/reify-ir -> crates -> root)")
        .to_path_buf()
}

const LOCAL_PATH: &str = "crates/reify-ir/src/geometry.rs";
const CANONICAL_PATH: &str = "crates/reify-test-support/src/fixtures.rs";

/// Extracts the `u32` index literals from `fn <fn_name>`'s `indices` `vec![…]`
/// block in `src`.
///
/// Pure and policy-free: it knows how to read an index block and nothing about
/// which blocks ought to agree. Returns `Err` rather than panicking so the
/// negative control can observe a failure without unwinding.
fn extract_indices(src: &str, fn_name: &str) -> Result<Vec<u32>, String> {
    let fn_at = src
        .find(&format!("fn {fn_name}("))
        .ok_or_else(|| format!("no `fn {fn_name}(` in source"))?;
    let body = &src[fn_at..];

    let indices_at = body
        .find("indices")
        .ok_or_else(|| format!("`fn {fn_name}` has no `indices` binding"))?;
    let open = body[indices_at..]
        .find("vec![")
        .ok_or_else(|| format!("`fn {fn_name}`'s `indices` is not a `vec![…]` literal"))?
        + indices_at
        + "vec![".len();
    let close = body[open..]
        .find(']')
        .ok_or_else(|| format!("`fn {fn_name}`'s `indices` block is unterminated"))?
        + open;

    body[open..close]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
        .split(',')
        .map(str::trim)
        .filter(|tok| !tok.is_empty())
        .map(|tok| {
            tok.parse::<u32>()
                .map_err(|e| format!("`fn {fn_name}`: {tok:?} is not a u32 index ({e})"))
        })
        .collect()
}

fn read_indices(rel_path: &str, fn_name: &str) -> Vec<u32> {
    let path = workspace_root().join(rel_path);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    extract_indices(&src, fn_name)
        .unwrap_or_else(|e| panic!("failed to extract indices from {rel_path}: {e}"))
}

/// The in-crate cube fixture's index block is identical to the canonical one.
#[test]
fn local_cube_indices_agree_with_canonical_box_indices() {
    // The canonical `unit_cube_mesh` just forwards to `prismatic_box_mesh`, so
    // the literal lives on the latter.
    let local = read_indices(LOCAL_PATH, "unit_cube_mesh");
    let canonical = read_indices(CANONICAL_PATH, "prismatic_box_mesh");

    assert_eq!(
        local, canonical,
        "{LOCAL_PATH}'s `tests::unit_cube_mesh` has drifted from \
         {CANONICAL_PATH}'s `prismatic_box_mesh` in its index block. Winding and \
         topology are load-bearing: both fixtures are vetted OUTWARD, and every \
         signed-volume and divergence-theorem assertion built on them depends on \
         it. Re-sync the two blocks rather than weakening this guard."
    );
}

/// Neither extraction can pass by matching nothing: each must yield exactly the
/// 36 indices of a 12-triangle, 8-vertex box.
#[test]
fn both_index_blocks_are_a_well_formed_twelve_triangle_box() {
    for (path, fn_name) in [
        (LOCAL_PATH, "unit_cube_mesh"),
        (CANONICAL_PATH, "prismatic_box_mesh"),
    ] {
        let indices = read_indices(path, fn_name);
        assert_eq!(
            indices.len(),
            36,
            "{path}'s `{fn_name}` must have 36 indices (12 triangles); \
             a short read means the extractor silently matched the wrong block"
        );
        assert!(
            indices.iter().all(|&i| i < 8),
            "{path}'s `{fn_name}` indexes beyond the 8 box corners: {indices:?}"
        );
    }
}

/// Negative control: the extractor discriminates, rather than returning a
/// constant that would make the agreement assertion vacuously true.
#[test]
fn extractor_distinguishes_a_differing_index_block() {
    let reference = "fn f() { let indices: Vec<u32> = vec![0, 2, 1]; }";
    let permuted = "fn f() { let indices: Vec<u32> = vec![0, 1, 2]; }";

    let a = extract_indices(reference, "f").expect("reference block must parse");
    let b = extract_indices(permuted, "f").expect("permuted block must parse");

    assert_eq!(a, vec![0, 2, 1], "extractor must return the literal it read");
    assert_ne!(
        a, b,
        "extractor returned equal results for two DIFFERENT index blocks, so \
         the agreement assertion above proves nothing"
    );
    assert!(
        extract_indices("fn g() {}", "f").is_err(),
        "extractor must report a missing function rather than yielding an empty block"
    );
    assert!(
        extract_indices("fn f() { let verts = vec![1.0]; }", "f").is_err(),
        "extractor must report a missing `indices` binding rather than reading a neighbour"
    );
}
