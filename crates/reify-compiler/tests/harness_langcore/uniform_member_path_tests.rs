//! End-to-end floor for the uniform member-path resolver (task #5424, PRD
//! `docs/prds/v0_6/uniform-member-access.md` task α).
//!
//! Two jobs, both of them *characterization*:
//!
//! 1. **Regression floor** (PRD boundary rows 5 and 11) — the dotted-chain shapes
//!    that already work must keep evaluating byte-identically, and an unknown
//!    member mid-chain must keep naming the CONCRETE structure at that hop rather
//!    than degrading to a generic sentence or a silent `undef`.
//! 2. **Pre-refactor baseline for the priv consolidation** — every
//!    `E_PRIV_MEMBER_ACCESS` verdict the `expr.rs` external dot-access path emits
//!    today, pinned with its verbatim message text, so the step that replaces
//!    `template_member_is_priv` with `member_path::resolve_hop` is provably
//!    behaviour-preserving rather than argued to be.
//!
//! Everything here is green from birth. A failure means a behaviour change, not
//! an unimplemented feature.

use reify_core::{DiagnosticCode, Severity};
use reify_ir::Value;
use reify_test_support::{cell_value, compile_source, eval_source};

// ── Boundary row 5: the committed PRD fixtures keep evaluating identically ────

/// Extract the SI value of a `Length`-typed cell, or panic with the actual value.
fn si_length(result: &reify_eval::EvalResult, structure: &str, member: &str) -> f64 {
    match cell_value(result, structure, member) {
        Value::Scalar { si_value, .. } => si_value,
        other => panic!("{structure}.{member}: expected Value::Scalar, got {other:?}"),
    }
}

/// `member_chain_scalar.ri` — the two-hop `m.inner.doubled` / `m.inner.x` chain
/// through nested `let`-instances. 5424's floor: these numbers must not move.
#[test]
fn member_chain_scalar_fixture_evaluates_unchanged() {
    let result = eval_source(include_str!(
        "../../../../docs/prds/v0_6/fixtures/member_chain_scalar.ri"
    ));
    // Inner(x: 7mm) → doubled = 14mm; the chain reads it through Mid.inner.
    assert!(
        (si_length(&result, "Test", "v") - 0.014).abs() < 1e-12,
        "Test.v must stay 0.014 m (14mm), got {}",
        si_length(&result, "Test", "v")
    );
    // The same chain reading a `param` rather than a `let`.
    assert!(
        (si_length(&result, "Test", "direct") - 0.007).abs() < 1e-12,
        "Test.direct must stay 0.007 m (7mm), got {}",
        si_length(&result, "Test", "direct")
    );
}

/// `member_let_read.ri` — the single-hop `s.tap` read of a derived `let` on a
/// `let`-instance receiver.
#[test]
fn member_let_read_fixture_evaluates_unchanged() {
    let result = eval_source(include_str!(
        "../../../../docs/prds/v0_6/fixtures/member_let_read.ri"
    ));
    // Spec(major: 10mm) → tap = 10mm * 0.8 = 8mm.
    assert!(
        (si_length(&result, "Test", "drill") - 0.008).abs() < 1e-12,
        "Test.drill must stay 0.008 m (8mm), got {}",
        si_length(&result, "Test", "drill")
    );
}

/// `member_sub_geom_baseline.ri` — the `self.<sub>.<geometry-let>` shape.
///
/// Compile-only on purpose: the fixture builds real geometry (`cylinder` /
/// `box` / `difference`), so evaluating it would drag the kernel into this
/// harness. The volume assertion for that shape belongs to the geometry
/// boundary suite; what α owes is that the member path still RESOLVES, i.e.
/// no member-shape diagnostic appears.
#[test]
fn member_sub_geom_baseline_fixture_compiles_without_errors() {
    let module = compile_source(include_str!(
        "../../../../docs/prds/v0_6/fixtures/member_sub_geom_baseline.ri"
    ));
    let errors: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        errors.is_empty(),
        "member_sub_geom_baseline.ri must compile clean; got {errors:?}"
    );
}

// ── Boundary row 11: unknown member mid-chain, attributed concretely ──────────

/// Collect the error-severity messages carrying `code`.
fn messages_with_code(module: &reify_compiler::CompiledModule, code: DiagnosticCode) -> Vec<String> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(code))
        .map(|d| d.message.clone())
        .collect()
}

/// An unknown member at a NON-terminal position of a chain must name the member
/// AND the concrete structure it was looked up on — never the chain root, never a
/// generic geometry sentence, and never a silently-`undef` value.
#[test]
fn unknown_member_mid_chain_names_the_concrete_structure_at_that_hop() {
    let module = compile_source(
        r#"
structure def Leaf {
    param x: Length = 1mm
}
structure def Mid {
    let leaf = Leaf()
}
structure def Test {
    let m = Mid()
    let bad = m.nope.x
}
"#,
    );
    let found = messages_with_code(&module, DiagnosticCode::StructureMemberNotFound);
    assert_eq!(
        found,
        vec!["structure 'Mid' has no member 'nope'".to_string()],
        "the hop is taken on Mid (the type of `m`), so Mid is the attribution — \
         not Leaf, and not a generic 'no such member' sentence"
    );
}

// ── Pre-refactor characterization: every external priv verdict, verbatim ──────

/// A `Motor` declaring all four `priv`-able member kinds next to their public
/// twins, plus a default-visibility `let` (which is `Visibility::Private` in the
/// IR yet must NOT be reported as a priv violation).
fn motor_with_all_priv_kinds(read: &str) -> String {
    format!(
        r#"
trait SomeTrait {{}}
structure def Leaf {{}}
structure def Motor {{
    priv param p : Length = 1mm
    param q : Length = 2mm
    let internal : Length = 3mm
    priv sub a = Leaf()
    sub b = Leaf()
    priv port pt : SomeTrait {{}}
    port pu : SomeTrait {{}}
    param on : Bool = true
    where on {{
        priv param gp : Length = 4mm
        param gq : Length = 5mm
    }}
}}
structure def Parent {{
    let m = Motor()
    let r = m.{read}
}}
"#
    )
}

/// Assert `m.<member>` emits EXACTLY ONE `E_PRIV_MEMBER_ACCESS` with today's
/// verbatim text.
fn assert_denied_as_private(member: &str) {
    let module = compile_source(&motor_with_all_priv_kinds(member));
    let found = messages_with_code(&module, DiagnosticCode::PrivMemberAccess);
    assert_eq!(
        found,
        vec![format!(
            "E_PRIV_MEMBER_ACCESS: member '{member}' of structure 'Motor' is private"
        )],
        "external access on the priv member '{member}'"
    );
}

/// Assert `m.<member>` emits NO priv diagnostic at all.
fn assert_not_denied_as_private(member: &str) {
    let module = compile_source(&motor_with_all_priv_kinds(member));
    assert_eq!(
        messages_with_code(&module, DiagnosticCode::PrivMemberAccess),
        Vec::<String>::new(),
        "'{member}' is externally readable and must stay silent"
    );
}

#[test]
fn external_access_denies_a_priv_param() {
    assert_denied_as_private("p");
}

#[test]
fn external_access_denies_a_priv_sub() {
    assert_denied_as_private("a");
}

#[test]
fn external_access_denies_a_priv_port() {
    assert_denied_as_private("pt");
}

/// Task #5171: a `priv param` nested in a block-form `where` group. The priv
/// check must run over the FULL member union BEFORE the not-found fallback,
/// otherwise `StructureMemberNotFound` wins and the member leaks.
#[test]
fn external_access_denies_a_priv_param_inside_a_guarded_group() {
    assert_denied_as_private("gp");
}

#[test]
fn external_access_allows_the_public_twin_of_each_priv_kind() {
    assert_not_denied_as_private("q");
    assert_not_denied_as_private("b");
    assert_not_denied_as_private("pu");
}

/// The load-bearing negative behind the `kind == Param` guard: a default
/// (non-`pub`) `let` carries `Visibility::Private` in the IR, but `let`s are
/// never externally nameable, so reporting one would be a behaviour change.
#[test]
fn external_access_does_not_report_a_default_visibility_let_as_private() {
    assert_not_denied_as_private("internal");
}

// ── Pinned gap: the guarded-group membership divergence ───────────────────────

/// A PUBLIC `param` declared inside a `where { }` block is still rejected as
/// "no member" on external dot-access.
///
/// This is a DELIBERATE pin of a known gap, not an endorsement. The resolver
/// (`member_path::resolve_hop`, pinned by
/// `resolve_hop_scans_guarded_group_members_not_just_value_cells`) DOES scan
/// `guarded_groups[].members` and classifies `gq` as a public param. The
/// `expr.rs` acceptance policy deliberately does not consume that yet, because
/// guarded params are absent from the runtime `StructureInstanceData.fields`
/// (measured: `let m = Inner()` on a structure with a guarded `param g`
/// evaluates to `Inner { base, on }` — no `g`). Accepting the member here would
/// therefore trade this loud diagnostic for a silent runtime `undef`, which the
/// PRD's D9 decision forbids — the same pre-existing class already documented
/// for ports and subs at `expr.rs`'s `member_known` note.
///
/// Closing it properly needs the constructor/eval materialization half
/// (`StructureInstanceCtor` field population and `materialize_template_lets` in
/// `crates/reify-expr/src/lib.rs`), which is outside this task's module scope
/// and is tracked as #5935. Whichever task lands that MUST flip this assertion
/// rather than delete or weaken it.
#[test]
fn public_guarded_group_param_is_still_not_externally_readable_pinned_gap() {
    let module = compile_source(&motor_with_all_priv_kinds("gq"));
    assert_eq!(
        messages_with_code(&module, DiagnosticCode::StructureMemberNotFound),
        vec!["structure 'Motor' has no member 'gq'".to_string()],
        "pinned gap — flip this when the ctor/eval materialization half lands"
    );
    // …and it must fail LOUD, never as a silent undef: the priv path did not
    // swallow it, and the not-found diagnostic really is an error.
    assert_eq!(
        messages_with_code(&module, DiagnosticCode::PrivMemberAccess),
        Vec::<String>::new(),
        "a PUBLIC guarded param must not be misreported as private"
    );
}
