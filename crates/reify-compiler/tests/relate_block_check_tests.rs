//! End-to-end `reify check` enforcement tests for the `relate { }` block —
//! geometric-relations δ (task 4384), the §7.3/§4 Relation-vs-Bool dispatch.
//!
//! A `relate { }` member (and its inline `sub … at … where { }` twin) accepts
//! ONLY `Type::Relation` members. Every member is type-checked: a member whose
//! `result_type` is neither `Type::Relation` nor `Type::Error` is rejected with
//! `DiagnosticCode::RelateExpectsRelation` (PRD mnemonic `E_RELATE_EXPECTS_RELATION`).
//!
//! The 3-verb routing falls out of γ's typing with no name re-classification:
//!   - a `check` verb (`true`, `a > 0mm`, `is_…`) types to **Bool**     → rejected;
//!   - a `derive`/`query` verb (arity-2 `distance`/`angle`) types to a **metric**
//!     (`Length`/`Angle`)                                                → rejected;
//!   - a `drive` relation (`concentric`/`flush`/`offset`/…) types to **Relation** → accepted.
//!
//! Both relate homes — the member-level `relate { }` and the inline
//! `sub … at … where { }` — enforce identically (design §4: both desugar to one
//! flat relation set). These cases pin BOTH.
//!
//! RED until step-14 wires the `MemberDecl::Relate` Relation-check + the
//! `SubDecl.relate_relations` check into `entity.rs` and adds the
//! `DiagnosticCode::RelateExpectsRelation` variant — the file fails to compile
//! against the missing variant, the established RED-by-missing-symbol convention.

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// The error-severity `RelateExpectsRelation` diagnostics emitted while
/// compiling `module` — the δ relate-block enforcement signal.
fn relate_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::RelateExpectsRelation)
                && d.severity == Severity::Error
        })
        .collect()
}

// ── Member-level `relate { }` home ───────────────────────────────────────────

/// REQUIRED (load-bearing): a member-level `relate { }` whose body member is a
/// **Bool** expression (`true`, the minimal `check`-verb shape) is rejected with
/// `E_RELATE_EXPECTS_RELATION` — a Bool belongs in `constraint`, not `relate`.
///
/// RED: `DiagnosticCode::RelateExpectsRelation` does not exist and no relate
/// Relation-check is wired (the `MemberDecl::Relate` arm is a no-op in step-12).
#[test]
fn member_relate_block_rejects_bool_member() {
    let module = compile_source_with_stdlib("structure S {\n    relate { true }\n}");
    let errs = relate_errors(&module);
    assert!(
        !errs.is_empty(),
        "a Bool member in `relate {{ }}` must emit E_RELATE_EXPECTS_RELATION.\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
}

/// ROUTING: a `derive`/`query` member that types to a **metric** (NOT Relation)
/// is also rejected. The arity-2 `distance(p1, p2)` DERIVE form over `Point`
/// operands types cleanly to `Scalar<Length>` (a non-Error metric, so step-14's
/// skip-Error gate does not suppress it) — and therefore draws
/// `E_RELATE_EXPECTS_RELATION`. This is the routing signal: a query in a `relate`
/// block is a misuse, caught by the single Relation type-check.
#[test]
fn member_relate_block_rejects_metric_query_member() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param p1 : Point3<Length>\n    param p2 : Point3<Length>\n    \
         relate { distance(p1, p2) }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        !errs.is_empty(),
        "an arity-2 metric query `distance(p1, p2)` (Scalar<Length>) in `relate {{ }}` must \
         emit E_RELATE_EXPECTS_RELATION.\nAll diagnostics: {:#?}",
        module.diagnostics
    );
}

/// NEGATIVE: a genuine `drive` relation member — `concentric(a, b)` over two
/// `Axis` operands, which γ types to `Type::Relation` — is accepted: NO
/// `E_RELATE_EXPECTS_RELATION` diagnostic.
#[test]
fn member_relate_block_accepts_relation_member() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a : Axis\n    param b : Axis\n    \
         relate { concentric(a, b) }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        errs.is_empty(),
        "a Relation member `concentric(a, b)` must NOT emit E_RELATE_EXPECTS_RELATION, got: {:#?}",
        errs
    );
}

// ── Inline `sub … at … where { }` home ───────────────────────────────────────
//
// The inline relate-block enforces identically to the member-level home (design
// §4). A `Child` target structure is declared so the `sub` resolves; the
// relation operands (`a`/`b`) live in the parent `Parent` scope.

/// REQUIRED: a Bool member in the inline `sub … at … where { }` relate-block is
/// rejected with `E_RELATE_EXPECTS_RELATION`, exactly as the member-level home.
#[test]
fn inline_where_relate_block_rejects_bool_member() {
    let module = compile_source_with_stdlib(
        "structure Child {\n    param h : Length = 10mm\n}\n\
         structure Parent {\n    sub plate : Child at auto where { true }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        !errs.is_empty(),
        "a Bool member in inline `sub … where {{ }}` must emit E_RELATE_EXPECTS_RELATION.\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
}

/// NEGATIVE: a Relation member in the inline `sub … at … where { }` relate-block
/// is accepted — NO `E_RELATE_EXPECTS_RELATION` diagnostic.
#[test]
fn inline_where_relate_block_accepts_relation_member() {
    let module = compile_source_with_stdlib(
        "structure Child {\n    param h : Length = 10mm\n}\n\
         structure Parent {\n    param a : Axis\n    param b : Axis\n    \
         sub plate : Child at auto where { concentric(a, b) }\n}",
    );
    let errs = relate_errors(&module);
    assert!(
        errs.is_empty(),
        "a Relation member `concentric(a, b)` in inline `sub … where {{ }}` must NOT emit \
         E_RELATE_EXPECTS_RELATION, got: {:#?}",
        errs
    );
}

// ── The constraint side: `constraint` rejects Type::Relation ─────────────────
//
// The symmetric half of the relate-vs-constraint dispatch (design §4/§7.3): a
// `constraint` accepts only `Bool`. A `Relation` placed in a `constraint` is a
// misuse — it belongs in a `relate { }` block — and must be rejected loudly
// (Error severity) with a dedicated message pointing the author at `relate`.
// Since neither structure here declares a `relate { }` block, the ONLY
// diagnostic that can mention `relate` is the constraint-side rejection.

/// The Error-severity diagnostics whose message mentions `relate` — for these
/// relate-block-free structures, that is exactly the constraint→Relation
/// rejection (the dedicated "a relation belongs in `relate { }`" redirect).
fn constraint_relation_rejections(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.message.to_lowercase().contains("relate")
        })
        .collect()
}

/// REQUIRED: `constraint concentric(a, b)` — a `Type::Relation` expression in a
/// `constraint` (Bool) context — is rejected loudly: an Error-severity
/// diagnostic that names `relate` (the redirect to the relate-block home).
///
/// RED: the base constraint check emits only a generic `Severity::Warning`
/// ("constraint expression has type Relation, expected Bool") that neither errors
/// loudly nor mentions `relate` — so no Error+`relate` diagnostic exists yet.
#[test]
fn constraint_rejects_relation_expression() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a : Axis\n    param b : Axis\n    \
         constraint concentric(a, b)\n}",
    );
    let rejections = constraint_relation_rejections(&module);
    assert!(
        !rejections.is_empty(),
        "a Relation in a `constraint` must be rejected loudly with a dedicated \
         `relate`-redirect Error.\nAll diagnostics: {:#?}",
        module.diagnostics
    );
}

/// NEGATIVE: a genuine Bool constraint (`a_len > 0mm`) is accepted — NO
/// constraint→Relation rejection. Holds both before and after step-16.
#[test]
fn constraint_accepts_bool_expression() {
    let module = compile_source_with_stdlib(
        "structure S {\n    param a_len : Length\n    constraint a_len > 0mm\n}",
    );
    let rejections = constraint_relation_rejections(&module);
    assert!(
        rejections.is_empty(),
        "a genuine Bool constraint `a_len > 0mm` must NOT draw a constraint→Relation \
         rejection, got: {:#?}",
        rejections
    );
}
