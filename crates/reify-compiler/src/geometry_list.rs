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
use std::collections::HashSet;

/// The statically-known shape of a geometry-list let's initializer.
///
/// Both variants carry the element count, because that count is the whole
/// point: it is how many sibling `RealizationDecl`s the let lowers to, and
/// therefore how long the resulting `List<Geometry>` value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GeometryListShape {
    /// `[<geom>, <geom>, ...]` — a non-empty list literal, every element of
    /// which is a geometry expression.
    ListLiteral { elements: usize },
    /// `generate(<count>, |<param>| <geom>)` with a non-negative integer
    /// literal count.
    Generate { count: usize, param: String },
}

/// Classify `expr` as a *geometry-list* let initializer, or `None`.
///
/// This classifier is deliberately DISJOINT from `crate::geometry::is_geometry_let`:
/// a `let` is either a single-geometry let (one `RealizationDecl`), a
/// geometry-LIST let (N sibling `RealizationDecl`s regrouped into one
/// `Value::List` cell), or neither. Nothing is both — a bare
/// `cylinder(5mm, 20mm)` is a single-geometry let and returns `None` here,
/// and every shape this function accepts (`ListLiteral`, `FunctionCall`
/// named `generate`) is one `is_geometry_let` rejects.
///
/// It is a pure predicate with no diagnostics. A construct that *plainly*
/// intends geometry-in-a-collection but cannot be unrolled — a non-literal
/// `generate` count, a mixed-kind list literal — still returns `None` here;
/// emitting the loud compile-time Error for those is the caller's job (see
/// `diagnose_unsupported_geometry_list`).
pub(crate) fn classify_geometry_list_let(
    expr: &reify_ast::Expr,
    functions: &[CompiledFunction],
    known_geometry_lets: &HashSet<&str>,
    known_selector_lets: &HashSet<&str>,
) -> Option<GeometryListShape> {
    match &expr.kind {
        // (i) `[<geom>, ...]` — non-empty, and EVERY element must be geometry.
        // An empty literal carries no element kind and stays an ordinary list
        // let; a mixed literal is rejected here and diagnosed by the caller.
        reify_ast::ExprKind::ListLiteral(elements) => {
            if elements.is_empty() {
                return None;
            }
            elements
                .iter()
                .all(|e| {
                    crate::geometry::is_geometry_let(
                        e,
                        functions,
                        known_geometry_lets,
                        known_selector_lets,
                    )
                })
                .then_some(GeometryListShape::ListLiteral {
                    elements: elements.len(),
                })
        }
        // (ii) `generate(<int literal>, |p| <geom>)`.
        reify_ast::ExprKind::FunctionCall { name, args, .. } if name == "generate" => {
            // A user-defined `fn generate(...)` shadows the builtin — same
            // guard `is_geometry_let` applies to geometry function names.
            if functions.iter().any(|f| f.name == *name) {
                return None;
            }
            let [count_expr, lambda_expr] = args.as_slice() else {
                return None;
            };
            let count = non_negative_int_literal(count_expr)?;
            let reify_ast::ExprKind::Lambda { params, body } = &lambda_expr.kind else {
                return None;
            };
            let [param] = params.as_slice() else {
                return None;
            };
            // The lambda param is an ordinary bound scalar (the loop index).
            // It is never a geometry name, so `known_geometry_lets` needs no
            // extra plumbing here: an `Ident(param)` body would fail
            // `is_geometry_let` anyway, which is exactly right.
            crate::geometry::is_geometry_let(
                body,
                functions,
                known_geometry_lets,
                known_selector_lets,
            )
            .then(|| GeometryListShape::Generate {
                count,
                param: param.name.clone(),
            })
        }
        _ => None,
    }
}

/// `Some(n)` iff `expr` is a non-negative *integer* literal (`3`, `0`).
///
/// `is_real` distinguishes `3` from `3.0` at the token level, so a Real
/// literal, a quantity (`3mm`) and a negative count are all rejected without
/// re-inspecting source text.
fn non_negative_int_literal(expr: &reify_ast::Expr) -> Option<usize> {
    match &expr.kind {
        reify_ast::ExprKind::NumberLiteral {
            value,
            is_real: false,
        } if *value >= 0.0 && value.fract() == 0.0 => Some(*value as usize),
        _ => None,
    }
}

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
