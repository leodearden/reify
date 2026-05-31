//! Grammar gate for task #4067 — pre-1: verify the EXISTING grammar handles
//! struct-typed params (precedent: `param material : ElasticMaterial`) and
//! deep member-access (`result.shell_channels.top`) without any grammar change.
//!
//! These tests must be GREEN before any code is written (no code change needed).
//!
//! NOTE: In Reify, `let` declarations must live inside a structure body.
//! Bare `let x = expr` at the top level is invalid — wrap in `structure S { ... }`.

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Pre-1 (a): struct-typed param declaration parses cleanly.
/// Precedent: `param material : ElasticMaterial` is already used in solver_elastic.ri.
#[test]
fn pre1_struct_typed_param_shell_channels_parses() {
    let mut parser = make_parser();
    let source = b"structure def ElasticResult {
    param displacement : Field<Point3<Length>, Vector3<Length>>
    param stress : Field<Point3<Length>, Tensor<2, 3, Pressure>>
    param frame : Field<Point3<Length>, Matrix<3, 3, Real>>
    param shell_channels : ShellStress
    param max_von_mises : Pressure
    param converged : Bool
    param iterations : Int
}";
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "struct-typed param `shell_channels : ShellStress` must parse cleanly (no grammar change needed)"
    );
}

/// Pre-1 (b): deep member-access `result.shell_channels.top` and `.mid` parse cleanly.
///
/// Note: `let` declarations must be inside a `structure S { }` wrapper because
/// Reify top-level syntax is declaration-only; bare `let` is a structure member.
#[test]
fn pre1_deep_member_access_parses() {
    let mut parser = make_parser();

    // Deep member access: result.shell_channels.mid
    let src_a = b"structure S { let m = result.shell_channels.mid }";
    let tree_a = parser.parse(src_a, None).unwrap();
    assert!(
        !tree_a.root_node().has_error(),
        "deep member access `result.shell_channels.mid` must parse cleanly; tree: {}",
        tree_a.root_node().to_sexp()
    );

    // Free-function call with deep member access: von_mises(result.shell_channels.top)
    let src_b = b"structure S { let vt = von_mises(result.shell_channels.top) }";
    let tree_b = parser.parse(src_b, None).unwrap();
    assert!(
        !tree_b.root_node().has_error(),
        "von_mises call with deep member access must parse cleanly; tree: {}",
        tree_b.root_node().to_sexp()
    );
}
