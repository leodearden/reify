//! Test fixtures for walkers bounded by
//! [`MAX_COMPILE_RECURSION_DEPTH`](super::MAX_COMPILE_RECURSION_DEPTH).
//!
//! [`NestingArm`] nests a probe expression N layers deep through ONE recursion
//! arm of `ExprKind`. Placing the probe at the cap and one layer past it pins
//! that a walker forwards `depth + 1` through that arm: an arm that forwarded
//! `depth` would still reach the probe past the cap, and would recurse without
//! bound on deep input.
//!
//! Every layer is lint-neutral: it holds no member-access chain longer than 2
//! and binds no name that another layer binds, so whatever a walker reports
//! comes from the probe alone.

use reify_ast::{Expr, ExprKind, MatchArm, QuantifierKind, StringPart};
use reify_core::SourceSpan;

/// One recursion position of `ExprKind`. Variadic arms also have a second-element
/// entry, which catches a loop that forwards `depth + 1` only on its first pass.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NestingArm {
    UnOp,
    BinOpLeft,
    BinOpRight,
    FunctionCallFirstArg,
    FunctionCallSecondArg,
    ConditionalCondition,
    ConditionalThen,
    ConditionalElse,
    ListLiteralFirst,
    ListLiteralSecond,
    SetLiteralFirst,
    SetLiteralSecond,
    MapLiteralFirstKey,
    MapLiteralFirstValue,
    MapLiteralSecondKey,
    MapLiteralSecondValue,
    IndexAccessObject,
    IndexAccessIndex,
    MatchScrutinee,
    MatchFirstArmBody,
    MatchSecondArmBody,
    LambdaBody,
    QuantifierCollection,
    QuantifierPredicate,
    AdHocSelectorBase,
    AdHocSelectorFirstArg,
    AdHocSelectorSecondArg,
    QualifiedAccessQualifier,
    InstanceQualifiedAccessObject,
    InstanceQualifiedAccessQualified,
    RangeLower,
    RangeUpper,
    MemberAccessObject,
    TraitMethodCallObject,
    TraitMethodCallFirstArg,
    TraitMethodCallSecondArg,
    TraitStaticCallFirstArg,
    TraitStaticCallSecondArg,
    VariantConstructSecondField,
    InterpolatedStringSecondHole,
    AutoSecondParam,
}

impl NestingArm {
    pub(crate) const ALL: &[NestingArm] = &[
        NestingArm::UnOp,
        NestingArm::BinOpLeft,
        NestingArm::BinOpRight,
        NestingArm::FunctionCallFirstArg,
        NestingArm::FunctionCallSecondArg,
        NestingArm::ConditionalCondition,
        NestingArm::ConditionalThen,
        NestingArm::ConditionalElse,
        NestingArm::ListLiteralFirst,
        NestingArm::ListLiteralSecond,
        NestingArm::SetLiteralFirst,
        NestingArm::SetLiteralSecond,
        NestingArm::MapLiteralFirstKey,
        NestingArm::MapLiteralFirstValue,
        NestingArm::MapLiteralSecondKey,
        NestingArm::MapLiteralSecondValue,
        NestingArm::IndexAccessObject,
        NestingArm::IndexAccessIndex,
        NestingArm::MatchScrutinee,
        NestingArm::MatchFirstArmBody,
        NestingArm::MatchSecondArmBody,
        NestingArm::LambdaBody,
        NestingArm::QuantifierCollection,
        NestingArm::QuantifierPredicate,
        NestingArm::AdHocSelectorBase,
        NestingArm::AdHocSelectorFirstArg,
        NestingArm::AdHocSelectorSecondArg,
        NestingArm::QualifiedAccessQualifier,
        NestingArm::InstanceQualifiedAccessObject,
        NestingArm::InstanceQualifiedAccessQualified,
        NestingArm::RangeLower,
        NestingArm::RangeUpper,
        NestingArm::MemberAccessObject,
        NestingArm::TraitMethodCallObject,
        NestingArm::TraitMethodCallFirstArg,
        NestingArm::TraitMethodCallSecondArg,
        NestingArm::TraitStaticCallFirstArg,
        NestingArm::TraitStaticCallSecondArg,
        NestingArm::VariantConstructSecondField,
        NestingArm::InterpolatedStringSecondHole,
        NestingArm::AutoSecondParam,
    ];

    /// The most layers of this arm that keep a probe nested under them within
    /// `budget` walker levels of the outermost layer; one more layer exceeds it.
    pub(crate) fn layers_within(self, budget: usize) -> usize {
        budget / self.depth_per_layer()
    }

    /// `probe` nested `layers` deep through this arm; the outermost layer is the
    /// returned root.
    pub(crate) fn nest(self, probe: Expr, layers: usize) -> Expr {
        let span = probe.span;
        (0..layers).fold(probe, |inner, layer| Expr {
            kind: self.wrap(inner, layer, span),
            span,
        })
    }

    /// Walker levels one layer adds. Exhaustive, so a new variant must state its
    /// cost before it compiles; add it to [`NestingArm::ALL`] as well.
    fn depth_per_layer(self) -> usize {
        match self {
            // `(-inner).f`: the UnOp ends dot_chain_lint's iterative chain walk,
            // so every layer recurses twice (MemberAccess, then UnOp).
            NestingArm::MemberAccessObject => 2,
            NestingArm::UnOp
            | NestingArm::BinOpLeft
            | NestingArm::BinOpRight
            | NestingArm::FunctionCallFirstArg
            | NestingArm::FunctionCallSecondArg
            | NestingArm::ConditionalCondition
            | NestingArm::ConditionalThen
            | NestingArm::ConditionalElse
            | NestingArm::ListLiteralFirst
            | NestingArm::ListLiteralSecond
            | NestingArm::SetLiteralFirst
            | NestingArm::SetLiteralSecond
            | NestingArm::MapLiteralFirstKey
            | NestingArm::MapLiteralFirstValue
            | NestingArm::MapLiteralSecondKey
            | NestingArm::MapLiteralSecondValue
            | NestingArm::IndexAccessObject
            | NestingArm::IndexAccessIndex
            | NestingArm::MatchScrutinee
            | NestingArm::MatchFirstArmBody
            | NestingArm::MatchSecondArmBody
            | NestingArm::LambdaBody
            | NestingArm::QuantifierCollection
            | NestingArm::QuantifierPredicate
            | NestingArm::AdHocSelectorBase
            | NestingArm::AdHocSelectorFirstArg
            | NestingArm::AdHocSelectorSecondArg
            | NestingArm::QualifiedAccessQualifier
            | NestingArm::InstanceQualifiedAccessObject
            | NestingArm::InstanceQualifiedAccessQualified
            | NestingArm::RangeLower
            | NestingArm::RangeUpper
            | NestingArm::TraitMethodCallObject
            | NestingArm::TraitMethodCallFirstArg
            | NestingArm::TraitMethodCallSecondArg
            | NestingArm::TraitStaticCallFirstArg
            | NestingArm::TraitStaticCallSecondArg
            | NestingArm::VariantConstructSecondField
            | NestingArm::InterpolatedStringSecondHole
            | NestingArm::AutoSecondParam => 1,
        }
    }

    /// One layer around `inner`. `layer` names the quantifier variable, so nested
    /// quantifier layers never shadow one another.
    fn wrap(self, inner: Expr, layer: usize, span: SourceSpan) -> ExprKind {
        let leaf = || Expr {
            kind: ExprKind::NumberLiteral {
                value: 0.0,
                is_real: false,
            },
            span,
        };
        let quantifier = |collection: Box<Expr>, predicate: Box<Expr>| ExprKind::Quantifier {
            kind: QuantifierKind::ForAll,
            variable: format!("q{layer}"),
            variable_span: span,
            collection,
            predicate,
        };
        let match_over = |discriminant: Box<Expr>, bodies: Vec<Expr>| ExprKind::Match {
            discriminant,
            arms: bodies
                .into_iter()
                .map(|body| MatchArm {
                    patterns: vec![],
                    body,
                    span,
                })
                .collect(),
        };
        match self {
            NestingArm::UnOp => ExprKind::UnOp {
                op: "-".into(),
                operand: Box::new(inner),
            },
            NestingArm::BinOpLeft => ExprKind::BinOp {
                op: "+".into(),
                left: Box::new(inner),
                right: Box::new(leaf()),
            },
            NestingArm::BinOpRight => ExprKind::BinOp {
                op: "+".into(),
                left: Box::new(leaf()),
                right: Box::new(inner),
            },
            NestingArm::FunctionCallFirstArg => ExprKind::FunctionCall {
                name: "f".into(),
                args: vec![inner],
                arg_names: vec![None],
            },
            NestingArm::FunctionCallSecondArg => ExprKind::FunctionCall {
                name: "f".into(),
                args: vec![leaf(), inner],
                arg_names: vec![None, None],
            },
            NestingArm::ConditionalCondition => ExprKind::Conditional {
                condition: Box::new(inner),
                then_branch: Box::new(leaf()),
                else_branch: Box::new(leaf()),
            },
            NestingArm::ConditionalThen => ExprKind::Conditional {
                condition: Box::new(leaf()),
                then_branch: Box::new(inner),
                else_branch: Box::new(leaf()),
            },
            NestingArm::ConditionalElse => ExprKind::Conditional {
                condition: Box::new(leaf()),
                then_branch: Box::new(leaf()),
                else_branch: Box::new(inner),
            },
            NestingArm::ListLiteralFirst => ExprKind::ListLiteral(vec![inner]),
            NestingArm::ListLiteralSecond => ExprKind::ListLiteral(vec![leaf(), inner]),
            NestingArm::SetLiteralFirst => ExprKind::SetLiteral(vec![inner]),
            NestingArm::SetLiteralSecond => ExprKind::SetLiteral(vec![leaf(), inner]),
            NestingArm::MapLiteralFirstKey => ExprKind::MapLiteral(vec![(inner, leaf())]),
            NestingArm::MapLiteralFirstValue => ExprKind::MapLiteral(vec![(leaf(), inner)]),
            NestingArm::MapLiteralSecondKey => {
                ExprKind::MapLiteral(vec![(leaf(), leaf()), (inner, leaf())])
            }
            NestingArm::MapLiteralSecondValue => {
                ExprKind::MapLiteral(vec![(leaf(), leaf()), (leaf(), inner)])
            }
            NestingArm::IndexAccessObject => ExprKind::IndexAccess {
                object: Box::new(inner),
                index: Box::new(leaf()),
            },
            NestingArm::IndexAccessIndex => ExprKind::IndexAccess {
                object: Box::new(leaf()),
                index: Box::new(inner),
            },
            NestingArm::MatchScrutinee => match_over(Box::new(inner), vec![leaf()]),
            NestingArm::MatchFirstArmBody => match_over(Box::new(leaf()), vec![inner]),
            NestingArm::MatchSecondArmBody => match_over(Box::new(leaf()), vec![leaf(), inner]),
            NestingArm::LambdaBody => ExprKind::Lambda {
                params: vec![],
                body: Box::new(inner),
            },
            NestingArm::QuantifierCollection => quantifier(Box::new(inner), Box::new(leaf())),
            NestingArm::QuantifierPredicate => quantifier(Box::new(leaf()), Box::new(inner)),
            NestingArm::AdHocSelectorBase => ExprKind::AdHocSelector {
                base: Box::new(inner),
                selector: "_".into(),
                args: vec![],
            },
            NestingArm::AdHocSelectorFirstArg => ExprKind::AdHocSelector {
                base: Box::new(leaf()),
                selector: "_".into(),
                args: vec![inner],
            },
            NestingArm::AdHocSelectorSecondArg => ExprKind::AdHocSelector {
                base: Box::new(leaf()),
                selector: "_".into(),
                args: vec![leaf(), inner],
            },
            NestingArm::QualifiedAccessQualifier => ExprKind::QualifiedAccess {
                qualifier: Box::new(inner),
                member: "m".into(),
            },
            NestingArm::InstanceQualifiedAccessObject => ExprKind::InstanceQualifiedAccess {
                object: Box::new(inner),
                qualified: Box::new(leaf()),
            },
            NestingArm::InstanceQualifiedAccessQualified => ExprKind::InstanceQualifiedAccess {
                object: Box::new(leaf()),
                qualified: Box::new(inner),
            },
            NestingArm::RangeLower => ExprKind::Range {
                lower: Some(Box::new(inner)),
                upper: Some(Box::new(leaf())),
                lower_inclusive: true,
                upper_inclusive: true,
            },
            NestingArm::RangeUpper => ExprKind::Range {
                lower: Some(Box::new(leaf())),
                upper: Some(Box::new(inner)),
                lower_inclusive: true,
                upper_inclusive: true,
            },
            NestingArm::MemberAccessObject => ExprKind::MemberAccess {
                object: Box::new(Expr {
                    kind: ExprKind::UnOp {
                        op: "-".into(),
                        operand: Box::new(inner),
                    },
                    span,
                }),
                member: "f".into(),
            },
            NestingArm::TraitMethodCallObject => ExprKind::TraitMethodCall {
                object: Box::new(inner),
                trait_name: "T".into(),
                method: "m".into(),
                args: vec![],
            },
            NestingArm::TraitMethodCallFirstArg => ExprKind::TraitMethodCall {
                object: Box::new(leaf()),
                trait_name: "T".into(),
                method: "m".into(),
                args: vec![inner],
            },
            NestingArm::TraitMethodCallSecondArg => ExprKind::TraitMethodCall {
                object: Box::new(leaf()),
                trait_name: "T".into(),
                method: "m".into(),
                args: vec![leaf(), inner],
            },
            NestingArm::TraitStaticCallFirstArg => ExprKind::TraitStaticCall {
                trait_name: "T".into(),
                method: "m".into(),
                args: vec![inner],
            },
            NestingArm::TraitStaticCallSecondArg => ExprKind::TraitStaticCall {
                trait_name: "T".into(),
                method: "m".into(),
                args: vec![leaf(), inner],
            },
            NestingArm::VariantConstructSecondField => ExprKind::VariantConstruct {
                name: "V".into(),
                fields: vec![("a".into(), leaf()), ("b".into(), inner)],
            },
            // The Literal part between the holes checks that literals are skipped.
            NestingArm::InterpolatedStringSecondHole => ExprKind::InterpolatedString(vec![
                StringPart::Hole(Box::new(leaf())),
                StringPart::Literal(" ".into()),
                StringPart::Hole(Box::new(inner)),
            ]),
            NestingArm::AutoSecondParam => ExprKind::Auto {
                free: false,
                params: vec![("seed".into(), leaf()), ("p".into(), inner)],
            },
        }
    }
}

/// Runs `f` on a fresh thread with tokio's default 2 MiB worker stack, the stack
/// the GUI compiles on, and re-raises any panic so the test reports it as is.
pub(crate) fn on_tokio_worker_stack<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(f)
        .expect("spawn a 2 MiB thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
