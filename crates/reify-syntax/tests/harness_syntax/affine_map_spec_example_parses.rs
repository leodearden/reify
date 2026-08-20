//! Parse-gate for the AffineMap spec example (task 3966).
//!
//! User-observable signal: `cargo test -p reify-syntax -- affine_map_spec_example`
//!
//! Reads the canonical fixture at tests/fixtures/affine_map_spec_example.ri and
//! asserts zero CST ERROR nodes and zero parse errors.  This guards that the spec
//! §3.3.1 example snippet remains parseable if the grammar ever changes.
//!
//! The fixture is the canonical parse-target for this gate.  Note that the spec code
//! block in §3.3.1 is an independently authored copy — there is no include/extraction
//! mechanism keeping it byte-for-byte in sync with this file.  The gate validates
//! that the fixture parses cleanly; it does not guarantee the spec snippet does.

use reify_core::ModulePath;

use crate::common::make_ts_parser;

/// Asserts that the canonical AffineMap fixture parses without errors.
///
/// Two-layer assertion:
/// - `reify_syntax::parse` returns zero `ParseError`s (reify-syntax layer);
/// - tree-sitter's raw parser reports zero CST ERROR nodes (grammar layer).
///
/// **RED state:** panics with "fixture must exist: No such file or directory" while
/// `tests/fixtures/affine_map_spec_example.ri` is absent.
/// **GREEN state:** both assertions pass once the fixture is created (step 2).
#[test]
fn affine_map_spec_example_parses() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/affine_map_spec_example.ri"
    ))
    .expect("fixture must exist");

    let module = reify_syntax::parse(&src, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected zero parse errors for affine_map_spec_example.ri, got: {:?}",
        module.errors,
    );

    let mut parser = make_ts_parser();
    let tree = parser
        .parse(src.as_bytes(), None)
        .expect("tree-sitter parse failed");
    assert!(
        !tree.root_node().has_error(),
        "expected no CST ERROR nodes in affine_map_spec_example.ri; \
         tree-sitter has_error() returned true",
    );
}
