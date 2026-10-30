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
//!   (`Angle` → rotational, `Length` → translational, `Orientation(3)` → 3
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
/// is `Some(1)` or `Some(2)` depending on the operand combo (task 5540) yet
/// [`relation_delta_dof_kinds`] is `None` — removes DOF the
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
/// `tangent` is the motivating case: its codimension is known once the operand
/// pair is known (1 or 2 — task 5540 made the count operand-conditional), but its
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
/// - `Orientation(3)` → `(3, 0)` (a free spherical orientation) — `SO(N)` has
///   `N(N-1)/2` rotational DOF, but only `N=3` is inhabited today
///   (`Value::Orientation` is a quaternion, and `try_infer_type` returns
///   `Orientation(3)` unconditionally), so every other arity falls through to
///   `None` below rather than being answered,
/// - anything else → `None` — a DOF field that is neither an angle, a length,
///   nor an orientation has no kind to match against the residual.
///
/// Reachability note: the surface name `Orientation` IS resolvable from `.ri`
/// source as of task 6384 — the builtin-name table in `resolve_type_name`
/// surfaces both `Orientation` and the Display spelling `Orientation3` as
/// `Type::Orientation(3)`, so `stdlib/joints.ri`'s spherical and ball joints,
/// which declare `with orientation: Orientation`, reach the `Orientation(3)`
/// arm above on every prelude compile. Before that the field resolved to
/// `Type::Error` and never reached this function, leaving the arm exercised
/// only by internally-constructed `Type` values (e.g. this module's own
/// tests).
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
        Type::Orientation(3) => Some(DofKinds::new(3, 0)),
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
    use reify_core::Severity;
    use reify_core::hash::ContentHash;
    use reify_ir::{ResolvedFunction, Value};

    const JOINT_SPAN: SourceSpan = SourceSpan { start: 0, end: 10 };
    const FIELD_SPAN: SourceSpan = SourceSpan { start: 20, end: 25 };

    /// Build a relation-call body member: a `FunctionCall` node named `name`
    /// with the given operand args. Only `function.name` and each arg's
    /// `result_type` are read by the self-check; the content hash is a
    /// throwaway.
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

    fn concentric() -> CompiledExpr {
        rel("concentric", vec![arg(Type::Axis), arg(Type::Axis)])
    }

    fn tangent() -> CompiledExpr {
        rel("tangent", vec![arg(Type::Axis), arg(Type::Axis)])
    }

    fn field(name: &str, ty: Type) -> (&str, Type, SourceSpan) {
        (name, ty, FIELD_SPAN)
    }

    fn angle() -> (&'static str, Type, SourceSpan) {
        field("angle", Type::angle())
    }

    fn travel() -> (&'static str, Type, SourceSpan) {
        field("travel", Type::length())
    }

    fn run(dof: &[(&str, Type, SourceSpan)], body: &[CompiledExpr]) -> Vec<Diagnostic> {
        joint_dof_diagnostics("j", dof, body, JOINT_SPAN)
    }

    fn with_code(diags: &[Diagnostic], code: DiagnosticCode) -> Vec<&Diagnostic> {
        diags.iter().filter(|d| d.code == Some(code)).collect()
    }

    fn mismatches(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        with_code(diags, DiagnosticCode::JointDofMismatch)
    }

    fn kind_errors(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        with_code(diags, DiagnosticCode::ArgTypeMismatch)
    }

    fn assert_no_diagnostics(diags: &[Diagnostic]) {
        assert!(diags.is_empty(), "expected no diagnostics, got {diags:#?}");
    }

    /// The whole result is exactly one `E_JOINT_DOF_MISMATCH` error, labelled
    /// at the joint span, whose message contains every phrase.
    fn assert_single_mismatch(diags: &[Diagnostic], phrases: &[&str]) {
        assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {diags:#?}");
        let d = &diags[0];
        assert_eq!(d.code, Some(DiagnosticCode::JointDofMismatch));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.labels[0].span, JOINT_SPAN);
        for phrase in phrases {
            assert!(
                d.message.contains(phrase),
                "mismatch message must contain {phrase:?}: {}",
                d.message
            );
        }
    }

    /// The whole result is one `E_ARG_TYPE_MISMATCH` error per named field (in
    /// order), each labelled at the field span and naming the joint and field.
    fn assert_kind_errors_for(diags: &[Diagnostic], fields: &[&str]) {
        assert!(mismatches(diags).is_empty(), "no verdict expected: {diags:#?}");
        let errors = kind_errors(diags);
        assert_eq!(errors.len(), diags.len(), "only kind errors expected: {diags:#?}");
        assert_eq!(errors.len(), fields.len(), "one kind error per field: {diags:#?}");
        for (d, name) in errors.iter().zip(fields) {
            assert_eq!(d.severity, Severity::Error);
            assert_eq!(d.labels.len(), 1);
            assert_eq!(d.labels[0].span, FIELD_SPAN);
            let dof_phrase = format!("DOF `{name}`");
            for phrase in ["joint `j`", dof_phrase.as_str(), "not a valid joint DOF kind"] {
                assert!(
                    d.message.contains(phrase),
                    "kind error must contain {phrase:?}: {}",
                    d.message
                );
            }
        }
    }

    // ── Verdict / residual ───────────────────────────────────────────────────

    /// `concentric(Axis, Axis)` removes (2, 2), leaving (1, 1): the cylindrical
    /// pair (B4) matches `{ angle, travel }`; `angle` alone is a COUNT mismatch
    /// (B2) whose remedy names the uncovered translational freedom.
    #[test]
    fn concentric_leaves_one_rot_one_trans() {
        assert_no_diagnostics(&run(&[angle(), travel()], &[concentric()]));
        assert_single_mismatch(
            &run(&[angle()], &[concentric()]),
            &["declared 1 rotational", "leaves 1 rot + 1 trans", "declare", "Length"],
        );
    }

    /// `concentric` + `on(Point, Plane)` removes (2, 3), leaving (1, 0): the
    /// revolute (B1) matches `angle`; `travel` agrees in count but not KIND (B3).
    #[test]
    fn concentric_plus_on_is_a_revolute() {
        let body = [concentric(), rel("on", vec![pt(), arg(Type::Plane)])];
        assert_no_diagnostics(&run(&[angle()], &body));
        assert_single_mismatch(&run(&[travel()], &body), &["translational", "1 rot"]);
    }

    /// `parallel(Direction, Direction)` removes (2, 0), leaving (1, 3).
    #[test]
    fn parallel_leaves_one_rot_three_trans() {
        let body = [rel("parallel", vec![arg(Type::Direction), arg(Type::Direction)])];
        let x = field("x", Type::length());
        let y = field("y", Type::length());
        let z = field("z", Type::length());
        assert_no_diagnostics(&run(&[angle(), x, y, z], &body));
        assert_single_mismatch(&run(&[angle()], &body), &["leaves 1 rot + 3 trans"]);
    }

    /// An empty body removes nothing: the residual is the full nominal (3, 3),
    /// so an empty `joint … = { }` cannot match a sane declaration.
    #[test]
    fn empty_body_leaves_full_six_dof() {
        assert_single_mismatch(&run(&[angle()], &[]), &["leaves 3 rot + 3 trans"]);
    }

    /// Over-removal saturates at 0: `coincident(Frame, Frame)` (3, 3) +
    /// `perpendicular` (1, 0) leaves (0, 0) rather than underflowing.
    #[test]
    fn over_removal_saturates_at_zero() {
        let body = [
            rel("coincident", vec![arg(Type::Frame(3)), arg(Type::Frame(3))]),
            rel("perpendicular", vec![arg(Type::Direction), arg(Type::Direction)]),
        ];
        assert_no_diagnostics(&run(&[], &body));
        assert_single_mismatch(
            &run(&[angle()], &body),
            &["leaves 0 rot + 0 trans", "drop a declared DOF"],
        );
    }

    #[test]
    fn no_declared_dof_reads_as_declared_none() {
        assert_single_mismatch(&run(&[], &[concentric()]), &["declared no free DOF"]);
    }

    /// A non-`FunctionCall` body member removes nothing.
    #[test]
    fn non_function_call_member_is_skipped() {
        let body = [arg(Type::Relation), concentric()];
        assert_no_diagnostics(&run(&[angle(), travel()], &body));
        assert_single_mismatch(&run(&[angle()], &body), &["leaves 1 rot + 1 trans"]);
    }

    // ── Gate (b): undecidable rot/trans split ────────────────────────────────

    /// `tangent` has a curated DOF COUNT but no rot/trans split, so the residual
    /// cannot substantiate a verdict and none is drawn. Its companion is
    /// `concentric_leaves_one_rot_one_trans`, where the same `angle`
    /// declaration over `concentric` alone mismatches.
    #[test]
    fn undecidable_kind_split_draws_no_verdict() {
        assert_no_diagnostics(&run(&[angle()], &[tangent()]));
        assert_no_diagnostics(&run(&[angle()], &[concentric(), tangent()]));
    }

    /// A wholly-uncurated relation (no count either) does NOT trip gate (b): it
    /// removes nothing and the verdict still fires.
    #[test]
    fn uncurated_relation_does_not_gate_the_verdict() {
        let body = [rel("frobnicate", vec![arg(Type::Axis)]), concentric()];
        assert_single_mismatch(&run(&[angle()], &body), &["leaves 1 rot + 1 trans"]);
    }

    // ── Gate (a): kind-less DOF ──────────────────────────────────────────────

    /// A `Type::Error` DOF was already diagnosed upstream: no verdict and no
    /// second diagnostic, even beside a classifiable field.
    #[test]
    fn error_typed_dof_draws_nothing() {
        assert_no_diagnostics(&run(&[field("angle", Type::Error)], &[concentric()]));
        assert_no_diagnostics(&run(&[angle(), field("x", Type::Error)], &[concentric()]));
    }

    /// A DOF whose type has no joint-DOF kind draws one targeted kind error at
    /// its field span and no verdict. `Orientation(2)`/`(4)` are uninhabitable
    /// arities the classifier declines (see task 6335).
    #[test]
    fn non_kind_dof_draws_one_kind_error_and_no_verdict() {
        for ty in [
            Type::Axis,
            Type::dimensionless_scalar(),
            Type::Orientation(2),
            Type::Orientation(4),
        ] {
            assert_kind_errors_for(&run(&[field("foo", ty)], &[concentric()]), &["foo"]);
        }
    }

    #[test]
    fn kind_errors_are_per_non_kind_field() {
        let dof = [field("foo", Type::Axis), field("bar", Type::dimensionless_scalar())];
        assert_kind_errors_for(&run(&dof, &[concentric()]), &["foo", "bar"]);
        let dof = [field("foo", Type::Axis), field("x", Type::Error)];
        assert_kind_errors_for(&run(&dof, &[concentric()]), &["foo"]);
    }

    // ── Classification ───────────────────────────────────────────────────────

    /// `coincident(Point, Point)` leaves (3, 0): `Orientation(3)` classifies as
    /// 3 rotational (not as a kind-less gate), so adding an `angle` over-declares.
    #[test]
    fn orientation_three_is_three_rotational() {
        let body = [rel("coincident", vec![pt(), pt()])];
        let orientation = field("orientation", Type::Orientation(3));
        assert_no_diagnostics(&run(&[orientation.clone()], &body));
        assert_single_mismatch(
            &run(&[orientation, angle()], &body),
            &["declared 4 rotational free DOF"],
        );
    }
}
