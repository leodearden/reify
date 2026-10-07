//! Drift guard: `geometry::tests::unit_cube_mesh`, this crate's test cube, must
//! keep the same index block as the workspace-canonical
//! `reify_test_support::mesh_fixtures::prismatic_box_mesh`.
//!
//! The local copy cannot delegate to the canonical one. Because it is
//! `#[cfg(test)]`, its `Mesh` is the `--test` build of `reify-ir`, which is a
//! different type from the `Mesh` that `reify-test-support` links. Any way
//! around that needs a `reify-test-support` dev-dep, and that inverts the
//! layering: `reify-test-support` normal-deps `reify-compiler`, which
//! normal-deps `reify-ir`. No other crate can call a `#[cfg(test)]` item, so
//! this guard compares source text. It compares indices only. Winding is what
//! can drift silently. The vertices are spelled parametrically in the
//! canonical, and `crates/reify-test-support/tests/box_fixtures.rs` already
//! pins their values.
//!
//! Giving both literals a single definition (for example, a shared
//! `include!`d data file) would retire this scanner. That placement decision
//! belongs to #7528.

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
const CANONICAL_PATH: &str = "crates/reify-test-support/src/mesh_fixtures.rs";

/// `src` with every `//` comment cut off its line, so nothing a comment says —
/// a `]` in `// [0,1]^3`, a mention of `indices` — can be read as code.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The source of the one `fn <fn_name>` in `code`, from its header to the brace
/// that closes its body, so nothing after the fn can be read as part of it.
fn fn_source<'a>(code: &'a str, fn_name: &str) -> Result<&'a str, String> {
    let header = format!("fn {fn_name}(");
    let headers: Vec<usize> = code.match_indices(&header).map(|(at, _)| at).collect();
    let [fn_at] = headers[..] else {
        return Err(format!(
            "expected exactly one `{header}`, found {}",
            headers.len()
        ));
    };
    let mut depth = 0_usize;
    for (offset, ch) in code[fn_at..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(&code[fn_at..=fn_at + offset]);
                }
            }
            _ => {}
        }
    }
    Err(format!("`fn {fn_name}` has no closed body"))
}

/// Extracts the `u32` literals from the `vec![…]` that initialises
/// `fn <fn_name>`'s own `indices` binding in `src` — a
/// `let indices: … = vec![…]` or an `indices: vec![…]` field.
///
/// Pure and policy-free: it knows how to read an index block and nothing about
/// which blocks ought to agree. Whatever it cannot read unambiguously is an
/// `Err`, never a guess, and is returned rather than panicked so the negative
/// controls can observe it without unwinding.
fn extract_indices(src: &str, fn_name: &str) -> Result<Vec<u32>, String> {
    let code = strip_line_comments(src);
    let body = fn_source(&code, fn_name)?;

    // The leading space keeps this a whole word: `n_indices:` is not the binding.
    const BINDING: &str = " indices:";
    let init_at = body
        .find(BINDING)
        .ok_or_else(|| format!("`fn {fn_name}` has no `indices:` binding"))?
        + BINDING.len();
    let literal_at = body[init_at..]
        .find("vec![")
        .ok_or_else(|| format!("`fn {fn_name}`'s `indices` is not a `vec![…]` literal"))?
        + init_at;
    if body[init_at..literal_at].contains([',', ';']) {
        return Err(format!(
            "`fn {fn_name}`'s `indices` is initialised by something other than the next `vec![…]`"
        ));
    }
    let open = literal_at + "vec![".len();
    let close = body[open..]
        .find(']')
        .ok_or_else(|| format!("`fn {fn_name}`'s `indices` block is unterminated"))?
        + open;

    body[open..close]
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

    assert_eq!(
        a,
        vec![0, 2, 1],
        "extractor must return the literal it read"
    );
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
        "extractor must report a missing `indices` binding rather than reading another `vec![…]`"
    );
}

/// The extractor reads only the `vec![…]` that initialises the `indices`
/// binding of a uniquely named fn, so annotating or reformatting a fixture
/// cannot silently re-point it.
#[test]
fn extractor_reads_only_the_indices_initialiser_of_a_unique_fn() {
    let annotated = "fn f() {
        // indices: vec![9, 9, 9] is commented out, so it is not the binding
        let n_indices: usize = 3;
        let indices: Vec<u32> = vec![
            0, 2, 1, // a `]` in a comment, as in [0,1]^3, must not end the block
            3, 0, 2,
        ];
    }";
    assert_eq!(
        extract_indices(annotated, "f"),
        Ok(vec![0, 2, 1, 3, 0, 2]),
        "comments and `n_indices` must be skipped, and the block read up to its real `]`"
    );
    assert!(
        extract_indices(
            "fn f() { let indices: Vec<u32> = vec![0]; } \
             fn f() { let indices: Vec<u32> = vec![1]; }",
            "f"
        )
        .is_err(),
        "a repeated `fn f(` is ambiguous and must be reported, not resolved to the first"
    );
    assert!(
        extract_indices(
            "fn f() { 1 } fn g() { let indices: Vec<u32> = vec![0]; }",
            "f"
        )
        .is_err(),
        "`fn f` has no `indices` binding of its own; a later fn's must not be read in its place"
    );
    assert!(
        extract_indices(
            "fn f() { Mesh { indices: BOX.to_vec(), normals: Some(vec![0]) } }",
            "f"
        )
        .is_err(),
        "an `indices` not initialised by a `vec![…]` literal must be reported, \
         not read from a later one"
    );
}
