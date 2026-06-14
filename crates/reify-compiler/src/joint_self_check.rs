//! Definition-time DOF self-check for `joint … with` declarations
//! (geometric-joints β, task 4396) — the §7.1 self-checking law.
//!
//! At definition time (before any solve) a
//! `joint NAME(datums) with <declared free DOF> = <relation body>` declaration
//! asserts that its **declared** free DOF matches the **geometric residual** the
//! relation body leaves — by COUNT and by KIND. This module holds the pure,
//! independently-unit-testable pieces of that law:
//!
//! - [`residual_kinds`] — the body's residual `(rot, trans)` freedoms. A
//!   mechanism nominally has 6 spatial DOF = 3 rotational + 3 translational
//!   (PRD §7.1.2); each body relation removes a curated `(rot, trans)`
//!   codimension split (via [`crate::relation_signatures::relation_delta_dof_kinds`]),
//!   so the residual is `(3 − Σrot, 3 − Σtrans)`, saturating at 0.
//! - [`declared_kinds`] — the kinds the declared DOF fields contribute
//!   (`Angle` → rotational, `Length` → translational, `Orientation` → 3
//!   rotational).
//! - [`check_joint_dof`] — compares the two `(rot, trans)` pairs by exact
//!   integer equality (no tolerance; PRD §12 G6 numeric-floor is N/A) and, on
//!   mismatch, builds the geometric `E_JOINT_DOF_MISMATCH` diagnostic.
//!
//! The compile-time wiring (building a param scope, compiling the body, running
//! the check) lives in `compile_builder/entities_phase.rs`; this module stays a
//! pure function library so the residual/declared/match/message logic is
//! testable without a full compile (mirroring the
//! `relation_signatures.rs` / `joint_signatures.rs` convention).

use crate::relation_signatures::{relation_delta_dof, relation_delta_dof_kinds};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, SourceSpan, Type};
use reify_ir::{CompiledExpr, CompiledExprKind};

/// A rotational/translational DOF split. `rot + trans` is the total DOF count.
///
/// Used three ways in the self-check: the body's residual (via
/// [`residual_kinds`]), the declared DOF fields' contribution (via
/// [`declared_kinds`]), and the two operands of the match law (via
/// `check_joint_dof`). Exact-integer — there is no tolerance (PRD §12 G6
/// numeric-floor is N/A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DofKinds {
    /// Rotational (angular) free DOF.
    pub(crate) rot: u32,
    /// Translational free DOF.
    pub(crate) trans: u32,
}

impl DofKinds {
    /// Construct a `(rot, trans)` split.
    pub(crate) const fn new(rot: u32, trans: u32) -> Self {
        Self { rot, trans }
    }
}

/// The nominal spatial DOF of an unconstrained rigid body: 3 rotational +
/// 3 translational (PRD §7.1.2). Each body relation removes some of these; the
/// residual is what is left for the joint's declared free DOF to account for.
const NOMINAL_ROT: u32 = 3;
const NOMINAL_TRANS: u32 = 3;

/// Compute the residual `(rot, trans)` freedoms a joint body leaves out of the
/// nominal 6 (3 rot + 3 trans).
///
/// Each body member that is a relation `FunctionCall` removes its curated
/// `(rot, trans)` codimension split (via [`relation_delta_dof_kinds`]); the
/// residual is `(3 − Σrot, 3 − Σtrans)`, saturating at 0 so an over-constrained
/// body reports `(0, 0)` rather than underflowing.
///
/// GRADUALISM (per-member only): a body member that is not a `FunctionCall`, or
/// whose relation has no curated kind split (`None` — e.g. `tangent`, or an
/// unknown name), contributes nothing to THIS function's residual.
///
/// This is a per-member skip, NOT a whole-check guarantee. A member with a
/// curated DOF COUNT but no kind split — `tangent`, where [`relation_delta_dof`]
/// is `Some(2)` yet [`relation_delta_dof_kinds`] is `None` — removes DOF the
/// kind table cannot attribute, so omitting it here INFLATES the residual above
/// the true geometry. Suppressing the resulting spurious mismatch is the
/// caller's responsibility: it gates the verdict off via
/// [`body_has_undecidable_kind_split`] whenever such a member is present.
pub(crate) fn residual_kinds(body: &[CompiledExpr]) -> DofKinds {
    let mut sum_rot: u32 = 0;
    let mut sum_trans: u32 = 0;
    for member in body {
        if let CompiledExprKind::FunctionCall { function, args } = &member.kind
            && let Some((rot, trans)) = relation_delta_dof_kinds(&function.name, args)
        {
            sum_rot += rot;
            sum_trans += trans;
        }
    }
    DofKinds::new(
        NOMINAL_ROT.saturating_sub(sum_rot),
        NOMINAL_TRANS.saturating_sub(sum_trans),
    )
}

/// Does `body` contain any relation member whose DOF COUNT is curated
/// ([`relation_delta_dof`] is `Some`) but whose rotational/translational KIND
/// split is NOT ([`relation_delta_dof_kinds`] is `None`)?
///
/// `tangent` is the motivating case: its codimension is a known `2`, but its
/// split is surface-conditional and nominally undecidable. [`residual_kinds`]
/// omits such a member (it has no kind to attribute), which INFLATES the residual
/// above the true geometry — so a declaration written to match the *true*
/// residual would draw a spurious `E_JOINT_DOF_MISMATCH`. When this returns
/// `true` the caller suppresses the count/kind verdict entirely (PRD §7.1
/// gradualism): a residual the kind table cannot fully account for must not force
/// a verdict.
///
/// A purely uncurated member (unknown name, or an operand shape outside the
/// vocabulary) has `relation_delta_dof` = `None` too, so it does NOT trip this
/// gate — its DOF removal is wholly unknown rather than known-count/unknown-kind,
/// and `residual_kinds` already treats it as removing nothing under the same
/// gradualism.
pub(crate) fn body_has_undecidable_kind_split(body: &[CompiledExpr]) -> bool {
    body.iter().any(|member| {
        if let CompiledExprKind::FunctionCall { function, args } = &member.kind {
            relation_delta_dof(&function.name, args).is_some()
                && relation_delta_dof_kinds(&function.name, args).is_none()
        } else {
            false
        }
    })
}

/// Classify a single resolved DOF field `Type` into its `(rot, trans)` kind
/// contribution, or `None` if it has no geometric joint-DOF kind:
/// - `Scalar<Angle>` → `(1, 0)` (1 rotational),
/// - `Scalar<Length>` → `(0, 1)` (1 translational),
/// - `Orientation(_)` → `(3, 0)` (a free spherical orientation),
/// - anything else → `None` — a DOF field that is neither an angle, a length,
///   nor an orientation has no kind to match against the residual.
///
/// The single source of truth for DOF-kind classification, shared by
/// [`declared_kinds`] (which sums the classifiable contributions) and the
/// compile-time wiring in `compile_builder/entities_phase.rs` (which surfaces
/// each `None` as a targeted `E_ARG_TYPE_MISMATCH` naming the offending field,
/// rather than silently folding it into a confusing `E_JOINT_DOF_MISMATCH`).
pub(crate) fn dof_kind_of(ty: &Type) -> Option<DofKinds> {
    match ty {
        Type::Scalar { dimension } if *dimension == DimensionVector::ANGLE => {
            Some(DofKinds::new(1, 0))
        }
        Type::Scalar { dimension } if *dimension == DimensionVector::LENGTH => {
            Some(DofKinds::new(0, 1))
        }
        Type::Orientation(_) => Some(DofKinds::new(3, 0)),
        _ => None,
    }
}

/// Sum the declared DOF field types' `(rot, trans)` kind contributions via
/// [`dof_kind_of`].
///
/// Unclassifiable types (those for which [`dof_kind_of`] is `None`) contribute
/// nothing here; they are surfaced separately, per-field, by the caller (which
/// holds the field name and span). The caller also GATES the count/kind verdict
/// on every DOF being classifiable — an unclassifiable `(0, 0)` contribution
/// would otherwise make the residual comparison meaningless — so this function
/// stays a pure sum with no diagnostic responsibility.
pub(crate) fn declared_kinds(declared: &[Type]) -> DofKinds {
    let mut rot: u32 = 0;
    let mut trans: u32 = 0;
    for ty in declared {
        if let Some(k) = dof_kind_of(ty) {
            rot += k.rot;
            trans += k.trans;
        }
    }
    DofKinds::new(rot, trans)
}

/// Render a `DofKinds` as the human-readable declared-DOF phrase used in the
/// mismatch message: `(1, 0)` → "declared 1 rotational free DOF", `(1, 1)` →
/// "declared 1 rotational + 1 translational free DOF", `(0, 0)` → "declared no
/// free DOF".
fn describe_declared(d: DofKinds) -> String {
    match (d.rot, d.trans) {
        (0, 0) => "declared no free DOF".to_string(),
        (r, 0) => format!("declared {r} rotational free DOF"),
        (0, t) => format!("declared {t} translational free DOF"),
        (r, t) => format!("declared {r} rotational + {t} translational free DOF"),
    }
}

/// Compare a joint's declared free DOF against the body's geometric residual by
/// exact-integer COUNT and KIND. Returns `None` when they match exactly
/// (`declared == residual`), or `Some(diagnostic)` coded
/// [`DiagnosticCode::JointDofMismatch`] (`Severity::Error`) describing the
/// disagreement and a geometric remedy.
///
/// The remedy names the residual freedoms the declaration fails to cover: an
/// unmatched translational residual suggests `declare travel: Length`; an
/// unmatched rotational residual suggests `declare angle: Angle`. When the
/// declaration over-specifies (more declared freedom than the body leaves), the
/// remedy is to add a constraint or drop a declared DOF.
///
/// Pure-integer — there is no tolerance (PRD §12 G6 numeric-floor is N/A). An
/// empty body has residual `(3, 3)`, so it can never match a sane declaration
/// and falls out here as a mismatch (no bespoke empty-body code needed).
pub(crate) fn check_joint_dof(
    joint_name: &str,
    declared: DofKinds,
    residual: DofKinds,
    span: SourceSpan,
) -> Option<Diagnostic> {
    if declared == residual {
        return None;
    }

    // Remedy hints: name the residual freedoms the declaration leaves uncovered.
    let mut hints: Vec<&str> = Vec::new();
    if residual.rot > declared.rot {
        hints.push("angle: Angle");
    }
    if residual.trans > declared.trans {
        hints.push("travel: Length");
    }
    let remedy = if hints.is_empty() {
        "add a constraint to the body or drop a declared DOF".to_string()
    } else {
        format!("add a constraint or declare {}", hints.join(" and "))
    };

    let msg = format!(
        "joint `{joint_name}`: {declared_desc}, but the relation leaves {rr} rot + {rt} trans; \
         {remedy}",
        declared_desc = describe_declared(declared),
        rr = residual.rot,
        rt = residual.trans,
    );
    let label = format!(
        "declared ({}, {}) but the body's residual is ({}, {})",
        declared.rot, declared.trans, residual.rot, residual.trans
    );
    Some(
        Diagnostic::error(msg)
            .with_code(DiagnosticCode::JointDofMismatch)
            .with_label(DiagnosticLabel::new(span, label)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::Type;
    use reify_core::hash::ContentHash;
    use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value};

    /// Build a relation-call body member: a `FunctionCall` node named `name`
    /// with the given operand args. Only `function.name` and each arg's
    /// `result_type` are read by `residual_kinds`; the content hash is a
    /// throwaway (it does not affect the residual computation).
    fn rel(name: &str, args: Vec<CompiledExpr>) -> CompiledExpr {
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: name.to_string(),
                },
                args,
            },
            result_type: Type::Relation,
            content_hash: ContentHash::of(&[]),
        }
    }

    /// A typed operand placeholder (only `result_type` matters downstream).
    fn arg(ty: Type) -> CompiledExpr {
        CompiledExpr::literal(Value::Undef, ty)
    }

    fn pt() -> CompiledExpr {
        arg(Type::point3(Type::length()))
    }

    // ── residual_kinds (step-5 RED / step-6 GREEN) ───────────────────────────

    /// A single `concentric(Axis, Axis)` removes (2 rot, 2 trans), leaving a
    /// residual (1, 1) out of the nominal (3, 3) — a cylindrical pair.
    #[test]
    fn residual_kinds_concentric_leaves_one_rot_one_trans() {
        let body = [rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)])];
        assert_eq!(residual_kinds(&body), DofKinds::new(1, 1));
    }

    /// `concentric(Axis, Axis)` (2,2) + `on(Point, Plane)` (0,1) removes (2,3),
    /// leaving residual (1, 0) — the canonical 1-rotational revolute (PRD §7.1.3,
    /// boundary B1).
    #[test]
    fn residual_kinds_concentric_plus_on_leaves_one_rotational() {
        let body = [
            rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)]),
            rel("on", vec![pt(), arg(Type::Plane)]),
        ];
        assert_eq!(residual_kinds(&body), DofKinds::new(1, 0));
    }

    /// `parallel(Direction, Direction)` removes (2 rot, 0 trans), leaving
    /// residual (1, 3).
    #[test]
    fn residual_kinds_parallel_leaves_one_rot_three_trans() {
        let body = [rel("parallel", vec![arg(Type::Direction), arg(Type::Direction)])];
        assert_eq!(residual_kinds(&body), DofKinds::new(1, 3));
    }

    /// An empty body removes nothing — the residual is the full nominal (3, 3).
    /// This is why β catches an empty `joint … = { }` body as a DOF mismatch.
    #[test]
    fn residual_kinds_empty_body_is_full_six_dof() {
        let body: [CompiledExpr; 0] = [];
        assert_eq!(residual_kinds(&body), DofKinds::new(3, 3));
    }

    /// A body member whose kind split is `None` (uncurated — e.g. `tangent`, or
    /// an unknown relation) is SKIPPED under gradualism: the residual is computed
    /// as if only the curated members were present.
    #[test]
    fn residual_kinds_skips_uncurated_member() {
        let body = [
            rel("tangent", vec![arg(Type::Axis), arg(Type::Axis)]),
            rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)]),
        ];
        // tangent → None (skipped); concentric → (2,2); residual (1,1).
        assert_eq!(residual_kinds(&body), DofKinds::new(1, 1));
    }

    /// Over-removal saturates at 0 rather than underflowing: `coincident(Frame,
    /// Frame)` (3,3) + `perpendicular` (1,0) would remove (4, 3), but the residual
    /// clamps to (0, 0).
    #[test]
    fn residual_kinds_saturates_at_zero() {
        let body = [
            rel("coincident", vec![arg(Type::Frame(3)), arg(Type::Frame(3))]),
            rel("perpendicular", vec![arg(Type::Direction), arg(Type::Direction)]),
        ];
        assert_eq!(residual_kinds(&body), DofKinds::new(0, 0));
    }

    /// A non-`FunctionCall` body member (e.g. a stray literal) contributes no
    /// removal — it is skipped just like an uncurated relation.
    #[test]
    fn residual_kinds_skips_non_function_call_member() {
        let body = [
            arg(Type::Relation),
            rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)]),
        ];
        assert_eq!(residual_kinds(&body), DofKinds::new(1, 1));
    }

    // ── body_has_undecidable_kind_split (count-known / kind-unknown) ──────────

    /// A body of fully-curated relations (count AND kind both `Some`) has no
    /// undecidable member — the verdict is trustworthy and is NOT gated off.
    #[test]
    fn body_has_undecidable_kind_split_false_for_fully_curated() {
        let body = [
            rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)]),
            rel("on", vec![pt(), arg(Type::Plane)]),
            rel("parallel", vec![arg(Type::Direction), arg(Type::Direction)]),
        ];
        assert!(!body_has_undecidable_kind_split(&body));
    }

    /// `tangent` has a curated COUNT (`relation_delta_dof` = `Some(2)`) but no
    /// kind split (`relation_delta_dof_kinds` = `None`) — the motivating
    /// count-known/kind-unknown member. Its presence trips the gate so the caller
    /// suppresses the verdict (`residual_kinds` would otherwise inflate the
    /// residual and draw a spurious mismatch).
    #[test]
    fn body_has_undecidable_kind_split_true_for_tangent() {
        let body = [rel("tangent", vec![arg(Type::Axis), arg(Type::Axis)])];
        assert!(body_has_undecidable_kind_split(&body));
        // And still trips when mixed with a fully-curated relation.
        let mixed = [
            rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)]),
            rel("tangent", vec![arg(Type::Axis), arg(Type::Axis)]),
        ];
        assert!(body_has_undecidable_kind_split(&mixed));
    }

    /// A purely uncurated member (unknown name → both `relation_delta_dof` and
    /// `relation_delta_dof_kinds` are `None`) does NOT trip the gate: its removal
    /// is wholly unknown, not known-count/unknown-kind. An empty body and a
    /// non-`FunctionCall` member likewise do not trip it.
    #[test]
    fn body_has_undecidable_kind_split_false_for_uncurated_empty_and_nonfn() {
        let unknown = [rel("frobnicate", vec![arg(Type::Axis)])];
        assert!(!body_has_undecidable_kind_split(&unknown));
        let empty: [CompiledExpr; 0] = [];
        assert!(!body_has_undecidable_kind_split(&empty));
        let nonfn = [arg(Type::Relation)];
        assert!(!body_has_undecidable_kind_split(&nonfn));
    }

    // ── dof_kind_of / declared_kinds (step-7 RED / step-8 GREEN) ─────────────

    /// `dof_kind_of` classifies the three valid DOF kinds and rejects everything
    /// else with `None` — the single classifier shared by `declared_kinds` (sum)
    /// and the caller's per-field surfacing.
    #[test]
    fn dof_kind_of_classifies_the_three_valid_kinds() {
        assert_eq!(dof_kind_of(&Type::angle()), Some(DofKinds::new(1, 0)));
        assert_eq!(dof_kind_of(&Type::length()), Some(DofKinds::new(0, 1)));
        assert_eq!(dof_kind_of(&Type::Orientation(3)), Some(DofKinds::new(3, 0)));
    }

    /// An unclassifiable declared type (a dimensionless `Scalar`, or a datum like
    /// `Axis`) has no geometric DOF kind → `None`. The caller surfaces each such
    /// field with a targeted diagnostic instead of folding it into the residual.
    #[test]
    fn dof_kind_of_unclassifiable_is_none() {
        assert_eq!(dof_kind_of(&Type::dimensionless_scalar()), None);
        assert_eq!(dof_kind_of(&Type::Axis), None);
    }

    /// A `Scalar<Angle>` DOF field contributes 1 rotational freedom.
    #[test]
    fn declared_kinds_angle_is_one_rotational() {
        assert_eq!(declared_kinds(&[Type::angle()]), DofKinds::new(1, 0));
    }

    /// A `Scalar<Length>` DOF field contributes 1 translational freedom.
    #[test]
    fn declared_kinds_length_is_one_translational() {
        assert_eq!(declared_kinds(&[Type::length()]), DofKinds::new(0, 1));
    }

    /// The record form `{ angle: Angle, travel: Length }` sums to (1 rot, 1 trans)
    /// — the cylindrical pair (boundary B4).
    #[test]
    fn declared_kinds_angle_and_length_sum() {
        assert_eq!(
            declared_kinds(&[Type::angle(), Type::length()]),
            DofKinds::new(1, 1)
        );
    }

    /// An `Orientation(_)` DOF field declares a full 3-rotational freedom (a free
    /// spherical/ball orientation).
    #[test]
    fn declared_kinds_orientation_is_three_rotational() {
        assert_eq!(declared_kinds(&[Type::Orientation(3)]), DofKinds::new(3, 0));
    }

    /// Unclassifiable types contribute nothing to the sum (they are surfaced
    /// per-field by the caller, not folded into the kinds here): a lone
    /// dimensionless scalar sums to (0, 0), and a mix keeps only the classifiable
    /// `angle` contribution.
    #[test]
    fn declared_kinds_skips_unclassifiable_in_the_sum() {
        assert_eq!(
            declared_kinds(&[Type::dimensionless_scalar()]),
            DofKinds::new(0, 0)
        );
        assert_eq!(
            declared_kinds(&[Type::angle(), Type::dimensionless_scalar()]),
            DofKinds::new(1, 0)
        );
    }

    // ── check_joint_dof (step-9 RED / step-10 GREEN) ─────────────────────────

    use reify_core::{Severity, SourceSpan};

    fn span() -> SourceSpan {
        SourceSpan::new(0, 10)
    }

    /// B1 — a revolute (`concentric` + `on`) leaves residual (1 rot, 0 trans) and
    /// declares `angle: Angle` = (1, 0). Exact match → no diagnostic.
    #[test]
    fn check_joint_dof_b1_revolute_matches() {
        assert!(
            check_joint_dof("revolute", DofKinds::new(1, 0), DofKinds::new(1, 0), span())
                .is_none(),
            "matching (1,0)==(1,0) must produce no diagnostic"
        );
    }

    /// B4 — a cylindrical pair (`concentric`) leaves residual (1 rot, 1 trans) and
    /// declares `{ angle: Angle, travel: Length }` = (1, 1). Exact match → None.
    #[test]
    fn check_joint_dof_b4_cylindrical_matches() {
        assert!(
            check_joint_dof("cylindrical", DofKinds::new(1, 1), DofKinds::new(1, 1), span())
                .is_none(),
            "matching (1,1)==(1,1) must produce no diagnostic"
        );
    }

    /// B2 — COUNT mismatch: declares `angle: Angle` = (1, 0) but the body
    /// (`concentric` only) leaves (1 rot, 1 trans). The uncovered translational
    /// freedom must surface a `JointDofMismatch` error naming the declared count,
    /// the residual, and a remedy ("declare … Length").
    #[test]
    fn check_joint_dof_b2_count_mismatch() {
        let d = check_joint_dof("bad", DofKinds::new(1, 0), DofKinds::new(1, 1), span())
            .expect("count mismatch must produce a diagnostic");
        assert_eq!(d.code, Some(DiagnosticCode::JointDofMismatch));
        assert_eq!(d.severity, Severity::Error);
        assert!(
            d.message.contains("declared 1 rotational"),
            "message must state the declared kinds: {}",
            d.message
        );
        assert!(
            d.message.contains("leaves 1 rot + 1 trans"),
            "message must state the geometric residual: {}",
            d.message
        );
        assert!(
            d.message.contains("declare") && d.message.contains("Length"),
            "message must offer a remedy naming the uncovered translational DOF: {}",
            d.message
        );
    }

    /// B3 — KIND mismatch: declares `travel: Length` = (0, 1) but the body leaves
    /// (1 rot, 0 trans). Counts agree (1 == 1) but the KINDS disagree — a
    /// translational declaration cannot absorb a rotational residual. The message
    /// must name the rotational-vs-translational disagreement.
    #[test]
    fn check_joint_dof_b3_kind_mismatch() {
        let d = check_joint_dof("kindbad", DofKinds::new(0, 1), DofKinds::new(1, 0), span())
            .expect("kind mismatch must produce a diagnostic");
        assert_eq!(d.code, Some(DiagnosticCode::JointDofMismatch));
        assert_eq!(d.severity, Severity::Error);
        assert!(
            d.message.contains("translational"),
            "message must name the declared translational kind: {}",
            d.message
        );
        assert!(
            d.message.contains("1 rot"),
            "message must name the rotational residual it disagrees with: {}",
            d.message
        );
    }

    /// An empty body yields residual (3, 3); any sane declared multiset (here a
    /// lone `angle: Angle` = (1, 0)) mismatches it → `JointDofMismatch`. This is
    /// how β catches an empty `joint … = { }` body.
    #[test]
    fn check_joint_dof_empty_body_residual_mismatches() {
        let d = check_joint_dof("empty", DofKinds::new(1, 0), DofKinds::new(3, 3), span())
            .expect("residual (3,3) cannot match a 1-DOF declaration");
        assert_eq!(d.code, Some(DiagnosticCode::JointDofMismatch));
        assert!(
            d.message.contains("leaves 3 rot + 3 trans"),
            "empty-body residual must read as the full nominal 6 DOF: {}",
            d.message
        );
    }
}
