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

impl GeometryListShape {
    /// How many elements this list has — i.e. how many `RealizationDecl`s the
    /// let emits, and hence the length of the resulting `List<Geometry>`.
    pub(crate) fn len(&self) -> usize {
        match self {
            GeometryListShape::ListLiteral { elements } => *elements,
            GeometryListShape::Generate { count, .. } => *count,
        }
    }
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

/// Upper bound on the element count of a geometry-list let.
///
/// Each element becomes a distinct `RealizationDecl` and hence a distinct
/// kernel build step, so an unbounded count would explode the realization
/// graph (and, downstream, the mesh/boolean workload) from a single short
/// source line. 256 is far above any hand-authored pattern while still
/// bounding the blow-up. Exceeding it is a loud compile-time Error, never a
/// silent truncation.
pub(crate) const GEOMETRY_LIST_MAX_ELEMENTS: usize = 256;

/// Statically unroll a geometry-list let into its element expressions.
///
/// Returns the elements in index order — one per `RealizationDecl` the let
/// will emit. `generate` bodies are cloned once per index with the loop param
/// substituted by that index's integer literal; list-literal elements are
/// returned verbatim.
///
/// Returns `None` (after pushing exactly one labelled Error) when the count
/// exceeds [`GEOMETRY_LIST_MAX_ELEMENTS`]. An empty list (`generate(0, …)`)
/// is a *successful* expansion to zero elements, not a failure.
///
/// Spans on substituted nodes are inherited from the original lambda body, so
/// a diagnostic raised while compiling element `k` still points at the single
/// source location the user actually wrote.
pub(crate) fn expand_geometry_list_elements(
    expr: &reify_ast::Expr,
    shape: &GeometryListShape,
    span: reify_core::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<reify_ast::Expr>> {
    match shape {
        GeometryListShape::ListLiteral { elements } => {
            let reify_ast::ExprKind::ListLiteral(items) = &expr.kind else {
                // Shape and expr always come from the same `classify_…` call.
                return None;
            };
            debug_assert_eq!(*elements, items.len());
            Some(items.clone())
        }
        GeometryListShape::Generate { count, param } => {
            if *count > GEOMETRY_LIST_MAX_ELEMENTS {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "generate() with a geometry-producing lambda is limited to \
                         {GEOMETRY_LIST_MAX_ELEMENTS} elements, but the count is {count}"
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!(
                            "each element becomes its own realization; \
                             reduce the count to at most {GEOMETRY_LIST_MAX_ELEMENTS}"
                        ),
                    )),
                );
                return None;
            }
            let reify_ast::ExprKind::FunctionCall { args, .. } = &expr.kind else {
                return None;
            };
            let reify_ast::ExprKind::Lambda { body, .. } = &args.get(1)?.kind else {
                return None;
            };
            Some(
                (0..*count)
                    .map(|k| substitute_index_ident(body, param, k))
                    .collect(),
            )
        }
    }
}

/// Clone `expr`, rewriting every *use* of `param` into the integer literal
/// `value`.
///
/// Shadowing-aware: recursion stops at any nested binder that rebinds the same
/// name (a `Lambda` whose params include it, or a `Quantifier` over it), so an
/// inner `|i| …` keeps its own `i`. Spans are inherited from the original
/// nodes throughout.
fn substitute_index_ident(expr: &reify_ast::Expr, param: &str, value: usize) -> reify_ast::Expr {
    use reify_ast::ExprKind as K;

    // Recurse helpers, all inheriting the original spans.
    let sub = |e: &reify_ast::Expr| substitute_index_ident(e, param, value);
    let sub_box = |e: &reify_ast::Expr| Box::new(substitute_index_ident(e, param, value));
    let sub_vec =
        |es: &[reify_ast::Expr]| -> Vec<reify_ast::Expr> { es.iter().map(&sub).collect() };

    let kind = match &expr.kind {
        // ── the substitution site ────────────────────────────────────────
        K::Ident(name) if name == param => K::NumberLiteral {
            value: value as f64,
            is_real: false,
        },

        // ── binders that SHADOW `param`: stop, keeping the subtree intact ─
        K::Lambda { params, .. } if params.iter().any(|p| p.name == param) => expr.kind.clone(),
        K::Quantifier { variable, .. } if variable == param => expr.kind.clone(),

        // ── leaves ───────────────────────────────────────────────────────
        K::Ident(_)
        | K::NumberLiteral { .. }
        | K::QuantityLiteral { .. }
        | K::StringLiteral(_)
        | K::BoolLiteral(_)
        | K::EnumAccess { .. }
        | K::Undef => expr.kind.clone(),

        // ── structural recursion ─────────────────────────────────────────
        K::BinOp { op, left, right } => K::BinOp {
            op: op.clone(),
            left: sub_box(left),
            right: sub_box(right),
        },
        K::UnOp { op, operand } => K::UnOp {
            op: op.clone(),
            operand: sub_box(operand),
        },
        K::FunctionCall {
            name,
            args,
            arg_names,
        } => K::FunctionCall {
            name: name.clone(),
            args: sub_vec(args),
            arg_names: arg_names.clone(),
        },
        K::MemberAccess { object, member } => K::MemberAccess {
            object: sub_box(object),
            member: member.clone(),
        },
        K::Conditional {
            condition,
            then_branch,
            else_branch,
        } => K::Conditional {
            condition: sub_box(condition),
            then_branch: sub_box(then_branch),
            else_branch: sub_box(else_branch),
        },
        K::ListLiteral(items) => K::ListLiteral(sub_vec(items)),
        K::SetLiteral(items) => K::SetLiteral(sub_vec(items)),
        K::MapLiteral(entries) => {
            K::MapLiteral(entries.iter().map(|(k, v)| (sub(k), sub(v))).collect())
        }
        K::IndexAccess { object, index } => K::IndexAccess {
            object: sub_box(object),
            index: sub_box(index),
        },
        // Match patterns bind payload names, but those binders are variant
        // field names — never the lambda index param, which is bound by the
        // enclosing `generate` lambda — so arm bodies recurse unconditionally.
        K::Match { discriminant, arms } => K::Match {
            discriminant: sub_box(discriminant),
            arms: arms
                .iter()
                .map(|arm| reify_ast::MatchArm {
                    patterns: arm.patterns.clone(),
                    body: sub(&arm.body),
                    span: arm.span,
                })
                .collect(),
        },
        K::Auto { free, params } => K::Auto {
            free: *free,
            params: params.iter().map(|(n, e)| (n.clone(), sub(e))).collect(),
        },
        K::Lambda { params, body } => K::Lambda {
            params: params.clone(),
            body: sub_box(body),
        },
        K::Quantifier {
            kind,
            variable,
            variable_span,
            collection,
            predicate,
        } => K::Quantifier {
            kind: *kind,
            variable: variable.clone(),
            variable_span: *variable_span,
            collection: sub_box(collection),
            predicate: sub_box(predicate),
        },
        K::AdHocSelector {
            base,
            selector,
            args,
        } => K::AdHocSelector {
            base: sub_box(base),
            selector: selector.clone(),
            args: sub_vec(args),
        },
        K::QualifiedAccess { qualifier, member } => K::QualifiedAccess {
            qualifier: sub_box(qualifier),
            member: member.clone(),
        },
        K::InstanceQualifiedAccess { object, qualified } => K::InstanceQualifiedAccess {
            object: sub_box(object),
            qualified: sub_box(qualified),
        },
        K::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => K::Range {
            lower: lower.as_ref().map(|e| sub_box(e)),
            upper: upper.as_ref().map(|e| sub_box(e)),
            lower_inclusive: *lower_inclusive,
            upper_inclusive: *upper_inclusive,
        },
        K::TraitMethodCall {
            object,
            trait_name,
            method,
            args,
        } => K::TraitMethodCall {
            object: sub_box(object),
            trait_name: trait_name.clone(),
            method: method.clone(),
            args: sub_vec(args),
        },
        K::TraitStaticCall {
            trait_name,
            method,
            args,
        } => K::TraitStaticCall {
            trait_name: trait_name.clone(),
            method: method.clone(),
            args: sub_vec(args),
        },
        K::VariantConstruct { name, fields } => K::VariantConstruct {
            name: name.clone(),
            fields: fields.iter().map(|(n, e)| (n.clone(), sub(e))).collect(),
        },
        K::InterpolatedString(parts) => K::InterpolatedString(
            parts
                .iter()
                .map(|part| match part {
                    reify_ast::StringPart::Literal(t) => reify_ast::StringPart::Literal(t.clone()),
                    reify_ast::StringPart::Hole(e) => reify_ast::StringPart::Hole(sub_box(e)),
                })
                .collect(),
        ),
    };

    reify_ast::Expr {
        kind,
        span: expr.span,
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
    pub(super) fn let_init(src: &str) -> reify_ast::Expr {
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

#[cfg(test)]
mod expand_tests {
    use super::tests::let_init;
    use super::*;
    use std::collections::HashSet;

    /// Classify + expand `src`, returning `(elements, diagnostics)`.
    ///
    /// `known` seeds `known_geometry_lets` so a list literal of bare idents
    /// (`[a, b]`) classifies without needing real constructor calls.
    fn expand(src: &str, known: &[&str]) -> (Option<Vec<reify_ast::Expr>>, Vec<Diagnostic>) {
        let expr = let_init(src);
        let functions: Vec<CompiledFunction> = Vec::new();
        let known_geometry_lets: HashSet<&str> = known.iter().copied().collect();
        let shape =
            classify_geometry_list_let(&expr, &functions, &known_geometry_lets, &HashSet::new())
                .unwrap_or_else(|| panic!("`{src}` should classify as a geometry-list let"));
        let mut diagnostics = Vec::new();
        let out = expand_geometry_list_elements(&expr, &shape, expr.span, &mut diagnostics);
        (out, diagnostics)
    }

    /// Count `Ident("<name>")` nodes in an expression tree.
    ///
    /// Counting over the derived `Debug` rendering rather than a second
    /// hand-written walker keeps the probe honest: a walker that forgot an
    /// `ExprKind` arm would silently agree with a substituter that forgot the
    /// same arm, and the test would pass vacuously. `LambdaParam` renders as
    /// `LambdaParam { name: "i", .. }`, so a *binder* never counts as a use.
    fn ident_uses(expr: &reify_ast::Expr, name: &str) -> usize {
        let needle = format!("Ident({name:?})");
        format!("{expr:?}").matches(&needle).count()
    }

    /// Every use of the index param in a `generate` body is replaced by that
    /// element's integer literal — including uses buried inside a geometry
    /// constructor's scalar arguments, which is the whole motivating idiom.
    #[test]
    fn generate_substitutes_index_at_every_use_site() {
        let (elements, diags) = expand(
            "generate(3, |i| translate(cylinder(5mm, 20mm), i * 10mm, 0mm, 0mm))",
            &[],
        );
        let elements = elements.expect("expansion must succeed");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(elements.len(), 3, "one element per index");

        for (k, element) in elements.iter().enumerate() {
            assert_eq!(
                ident_uses(element, "i"),
                0,
                "element {k} still references the index param: {element:?}"
            );
            // …substituted with *this* element's index, in order.
            let rendered = format!("{element:?}");
            assert!(
                rendered.contains(&format!(
                    "NumberLiteral {{ value: {}.0, is_real: false }}",
                    k
                )),
                "element {k} does not carry integer literal {k}: {rendered}"
            );
            // Non-index idents are untouched — `cylinder`/`translate` are
            // FunctionCall names, so probe a real Ident: none here, but the
            // constructor call itself must survive verbatim.
            assert!(
                rendered.contains("\"cylinder\""),
                "element {k} lost its geometry constructor: {rendered}"
            );
        }
    }

    /// Idents that are NOT the index param survive expansion untouched.
    #[test]
    fn generate_leaves_other_idents_untouched() {
        let (elements, _) = expand("generate(2, |i| cylinder(r, i * 1mm))", &[]);
        let elements = elements.expect("expansion must succeed");
        for (k, element) in elements.iter().enumerate() {
            assert_eq!(ident_uses(element, "i"), 0, "element {k}: index not folded");
            assert_eq!(
                ident_uses(element, "r"),
                1,
                "element {k}: unrelated ident `r` must survive verbatim"
            );
        }
    }

    /// A zero count is an empty list, not an error.
    #[test]
    fn generate_with_zero_count_expands_to_nothing() {
        let (elements, diags) = expand("generate(0, |i| cylinder(5mm, 20mm))", &[]);
        assert_eq!(elements.expect("expansion must succeed").len(), 0);
        assert!(diags.is_empty(), "empty list must be silent: {diags:?}");
    }

    /// A list literal expands to its own elements, cloned in source order.
    #[test]
    fn list_literal_expands_to_its_elements_verbatim() {
        let src = "[a, b]";
        let (elements, diags) = expand(src, &["a", "b"]);
        let elements = elements.expect("expansion must succeed");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let reify_ast::ExprKind::ListLiteral(original) = &let_init(src).kind else {
            panic!("`{src}` should parse as a list literal");
        };
        assert_eq!(&elements, original, "elements must be verbatim, in order");
    }

    /// A nested lambda that REBINDS the index name shadows it: the inner `i`
    /// is a different binding and must survive expansion unsubstituted.
    #[test]
    fn nested_lambda_rebinding_the_param_shadows_substitution() {
        let (elements, _) = expand(
            "generate(2, |i| translate(cylinder(5mm, 20mm), \
             flat_map([1], |i| [i * 1mm]).count * 1mm, 0mm, 0mm))",
            &[],
        );
        let elements = elements.expect("expansion must succeed");
        assert_eq!(elements.len(), 2);
        for (k, element) in elements.iter().enumerate() {
            assert_eq!(
                ident_uses(element, "i"),
                1,
                "element {k}: exactly the INNER (shadowed) `i` must remain: {element:?}"
            );
        }
    }

    /// The unroll is capped: each element becomes its own `RealizationDecl`,
    /// so an unbounded count would explode the realization graph. Over the
    /// cap is a loud Error, never a silent truncation.
    #[test]
    fn count_above_the_cap_is_rejected_with_one_labelled_error() {
        let n = GEOMETRY_LIST_MAX_ELEMENTS + 1;
        let (elements, diags) = expand(&format!("generate({n}, |i| cylinder(5mm, 20mm))"), &[]);
        assert!(elements.is_none(), "over-cap expansion must not succeed");
        assert_eq!(diags.len(), 1, "exactly one diagnostic: {diags:?}");
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Error);
        assert!(
            d.message.contains("generate"),
            "message must name the construct: {}",
            d.message
        );
        assert!(
            d.message.contains(&GEOMETRY_LIST_MAX_ELEMENTS.to_string()),
            "message must name the cap {GEOMETRY_LIST_MAX_ELEMENTS}: {}",
            d.message
        );
        assert!(!d.labels.is_empty(), "diagnostic must carry a span label");
    }

    /// Exactly at the cap is still accepted — the boundary is inclusive, so
    /// the rejection above is not off-by-one.
    #[test]
    fn count_exactly_at_the_cap_is_accepted() {
        let n = GEOMETRY_LIST_MAX_ELEMENTS;
        let (elements, diags) = expand(&format!("generate({n}, |i| cylinder(5mm, 20mm))"), &[]);
        assert_eq!(elements.expect("at-cap expansion must succeed").len(), n);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }
}
