//! Parse-gate for the sub-placement spec example (task 3907).
//!
//! User-observable signal: `cargo test -p reify-syntax -- sub_placement_spec_example`
//!
//! Reads the canonical fixture at tests/fixtures/sub_placement_spec_example.ri and
//! asserts zero CST ERROR nodes and zero parse errors.  This guards that the spec
//! §4.7/§8.3 example snippets remain parseable if the grammar ever changes.
//!
//! The fixture is the canonical parse-target for this gate.  Note that the spec code
//! blocks in §4.7/§8.3 are independently authored — there is no include/extraction
//! mechanism keeping them byte-for-byte in sync with this file.  The gate validates
//! that the fixture parses cleanly; it does not guarantee the spec snippets do.

use reify_core::ModulePath;

mod common;
use common::make_ts_parser;

/// Asserts that the canonical sub-placement fixture parses without errors.
///
/// Two-layer assertion:
/// - `reify_syntax::parse` returns zero `ParseError`s (reify-syntax layer);
/// - tree-sitter's raw parser reports zero CST ERROR nodes (grammar layer).
///
/// **RED state:** panics with "fixture must exist: No such file or directory" while
/// `tests/fixtures/sub_placement_spec_example.ri` is absent.
/// **GREEN state:** both assertions pass once the fixture is created (step 2).
#[test]
fn sub_placement_spec_example_parses() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sub_placement_spec_example.ri"
    ))
    .expect("fixture must exist");

    let module = reify_syntax::parse(&src, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected zero parse errors for sub_placement_spec_example.ri, got: {:?}",
        module.errors,
    );

    let mut parser = make_ts_parser();
    let tree = parser
        .parse(src.as_bytes(), None)
        .expect("tree-sitter parse failed");
    assert!(
        !tree.root_node().has_error(),
        "expected no CST ERROR nodes in sub_placement_spec_example.ri; \
         tree-sitter has_error() returned true",
    );
}
