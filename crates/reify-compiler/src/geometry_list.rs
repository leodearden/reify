//! Classification and static unrolling of *geometry-list lets* (task #5385).
//!
//! A geometry-list let is a top-level structure `let` whose initializer
//! statically unrolls to a fixed-length sequence of geometry expressions:
//!
//!   * `[<geom expr>, ...]` — a non-empty list literal whose every element is
//!     a geometry expression, or
//!   * `generate(<non-negative Int literal>, |i| <geom expr>)` — unrolled by
//!     substituting the lambda param with each index in `0..n`.
//!
//! Realizations are compile-time-declared IR nodes hydrated *by name*, so a
//! list of geometry needs N such nodes with N known at compile time. That is
//! precisely what this module computes.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Parse `structure S { let x = <src> }` and hand back the `let x`
    /// initializer expression.
    ///
    /// Parsing in-crate (rather than hand-building `reify_ast::Expr` trees as
    /// `geometry.rs`'s `is_geometry_let` tests do for arity-only cases) keeps
    /// the inputs honest: these shapes — list literals, lambdas, nested
    /// arithmetic — are exactly the ones a hand-built tree is most likely to
    /// get subtly wrong.
    fn let_init(src: &str) -> reify_ast::Expr {
        let source = format!("structure S {{\n    let x = {src}\n}}");
        let parsed = reify_syntax::parse(&source, reify_core::ModulePath::single("test_geomlist"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors for `{src}`: {:?}",
            parsed.errors
        );
        for decl in &parsed.declarations {
            if let reify_ast::Declaration::Structure(s) = decl {
                for m in &s.members {
                    if let reify_ast::MemberDecl::Let(l) = m
                        && l.name == "x"
                    {
                        return l.value.clone();
                    }
                }
            }
        }
        panic!("no `let x` found in parsed source for `{src}`");
    }

    fn classify(src: &str) -> Option<GeometryListShape> {
        let expr = let_init(src);
        let functions: Vec<CompiledFunction> = Vec::new();
        classify_geometry_list_let(&expr, &functions, &HashSet::new(), &HashSet::new())
    }

    // ── RECOGNISED shapes ────────────────────────────────────────────────

    /// A non-empty list literal whose every element is a geometry expression
    /// is a geometry-list let of exactly that length.
    #[test]
    fn list_literal_of_geometry_is_recognised() {
        assert_eq!(
            classify("[cylinder(5mm, 20mm), box(1mm, 1mm, 1mm)]"),
            Some(GeometryListShape::ListLiteral { elements: 2 }),
        );
    }

    /// `generate(<int literal>, |i| <geometry>)` is a geometry-list let whose
    /// length is the literal count and whose index param is the lambda param.
    #[test]
    fn generate_over_geometry_body_is_recognised() {
        assert_eq!(
            classify("generate(3, |i| cylinder(5mm, 20mm))"),
            Some(GeometryListShape::Generate {
                count: 3,
                param: "i".to_string(),
            }),
        );
    }

    /// A zero count is a *legal* geometry list, not a rejection: it yields an
    /// empty `List<Geometry>` with zero realizations. Rejecting it here would
    /// push the empty case back into the silent-undef class this task exists
    /// to kill.
    #[test]
    fn generate_with_zero_count_is_recognised_as_empty() {
        assert_eq!(
            classify("generate(0, |i| cylinder(5mm, 20mm))"),
            Some(GeometryListShape::Generate {
                count: 0,
                param: "i".to_string(),
            }),
        );
    }

    // ── REJECTED shapes ──────────────────────────────────────────────────

    /// A list of plain scalars is an ordinary `List<Int>` let, untouched.
    #[test]
    fn scalar_list_literal_is_rejected() {
        assert_eq!(classify("[1, 2, 3]"), None);
    }

    /// An empty list literal carries no element kind, so it cannot be
    /// classified as geometry; it stays an ordinary (empty) list let.
    #[test]
    fn empty_list_literal_is_rejected() {
        assert_eq!(classify("[]"), None);
    }

    /// `generate` over a non-geometry body is the pre-existing scalar
    /// combinator (`generate_combinator_tests.rs`) and must stay on that path.
    #[test]
    fn generate_over_scalar_body_is_rejected() {
        assert_eq!(classify("generate(3, |i| i)"), None);
    }

    /// `point3` is a datum constructor, not a `GEOMETRY_FUNCTION_NAMES`
    /// member — it already evaluates fine through the ordinary value path and
    /// must not be diverted into a realization.
    #[test]
    fn generate_over_datum_body_is_rejected() {
        assert_eq!(classify("generate(3, |i| point3(0mm, 0mm, 0mm))"), None);
    }

    /// A non-literal count cannot be unrolled at compile time. The classifier
    /// is a pure predicate, so it simply says `None` here; the *diagnostic*
    /// for this case is the caller's job.
    #[test]
    fn generate_with_non_literal_count_is_rejected() {
        assert_eq!(classify("generate(n, |i| cylinder(5mm, 20mm))"), None);
    }

    /// A list literal mixing geometry and non-geometry elements is rejected
    /// (again, the loud diagnostic belongs to the caller).
    #[test]
    fn mixed_kind_list_literal_is_rejected() {
        assert_eq!(classify("[cylinder(5mm, 20mm), 3]"), None);
    }

    /// The two classifiers must stay DISJOINT: a bare geometry call is a
    /// plain geometry let (`is_geometry_let`), never a geometry-LIST let.
    #[test]
    fn bare_geometry_call_is_not_a_list_let() {
        assert_eq!(classify("cylinder(5mm, 20mm)"), None);
        // …and the sibling predicate does claim it, proving disjointness
        // rather than mutual silence.
        let functions: Vec<CompiledFunction> = Vec::new();
        assert!(crate::geometry::is_geometry_let(
            &let_init("cylinder(5mm, 20mm)"),
            &functions,
            &HashSet::new(),
            &HashSet::new(),
        ));
    }
}
