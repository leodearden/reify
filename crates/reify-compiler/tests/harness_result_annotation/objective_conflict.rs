//! Compiler-level tests for the two objective-semantics diagnostics that are
//! decided at compile time: `E_OBJECTIVE_CONFLICT` (task 4010) and
//! `E_OBJECTIVE_INERT` (DIC γ, task #5417).
//!
//! PRDs: `docs/prds/v0_6/constraint-solver-completion.md` task ζ §3.3/§6.3
//! boundary-sketch B3; `docs/prds/v0_6/declared-intent-consumption-accounting.md`
//! §3 decision 3 / §4.2 boundary-sketch B6.
//!
//! # Conflict predicate (§6.3)
//!
//! Emit `DiagnosticCode::ObjectiveConflict` iff:
//! - `combination == WeightedSum`
//! - `terms.len() > 1`
//! - every term has default `weight == 1.0` and `priority == 0`
//! - at least one pair of terms has **opposite sense** (`Minimize` vs `Maximize`)
//!   over **distinct expressions** (compared by `CompiledExpr.content_hash`)
//!
//! # Cases covered
//!
//! (a) CONFLICT: `minimize mass` + `maximize stiffness` (distinct exprs, opposite sense)
//!     → `DiagnosticCode::ObjectiveConflict`, `Severity::Error`, message contains
//!     `"E_OBJECTIVE_CONFLICT"`, three escape hints (weights / priorities /
//!     combine-into-one-expression), and ≥1 label with a non-empty span.
//!
//! (b) NO-CONFLICT same-sense: `minimize mass` + `minimize cost`
//!     → no `ObjectiveConflict` diagnostic.
//!
//! (c) Single objective: `minimize mass`
//!     → no `ObjectiveConflict` diagnostic.
//!
//! (d) Mixed-sense SAME-expr: `minimize mass` + `maximize mass`
//!     → no `ObjectiveConflict` diagnostic (distinct-expression qualifier).
//!
//! # Inert predicate (DIC γ §3 decision 3)
//!
//! Emit `DiagnosticCode::ObjectiveInert` iff a template's declared objective
//! provably governs nothing — every cell it can reach, transitively through
//! `let` indirection and across the whole module, is permanently non-`auto`,
//! so no solver variable can ever move the cost.
//!
//! The rule is deliberately one-sided: a false positive would be a compile
//! Error on legal code, so anything unproven stays silent. The negative cases
//! below are therefore the *load-bearing* half — each pins one shape that must
//! never be reported, and together they are what stops the rule degenerating
//! into "any objective the checker cannot follow".

use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_test_support::compile_source_with_stdlib;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Parse `src` and compile it; return the compiled module.
/// Panics if parsing produces errors.
fn compile_module(src: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ── (a) CONFLICT: minimize mass + maximize stiffness ─────────────────────────

/// A structure with `minimize mass` + `maximize stiffness` (distinct params,
/// opposite sense, both at default weight 1.0 / priority 0) must produce exactly
/// one `DiagnosticCode::ObjectiveConflict` diagnostic at `Severity::Error`.
///
/// The message must contain:
/// - `"E_OBJECTIVE_CONFLICT"` (the PRD-prose mnemonic, embedded so the CLI
///   renders it via the `"{severity}: {message}"` format)
/// - at least one reference to the "weight" escape (letting the user assign
///   non-default weights to resolve the conflict)
/// - at least one reference to the "priority" escape (letting the user assign
///   non-default priorities to lexicographically order the objectives)
/// - at least one reference to combining objectives into a single expression
///   (the third escape path)
///
/// The diagnostic must carry ≥1 label with a non-empty span (required by the
/// compiler diagnostic convention in `diagnostic_coverage_checkpoint.rs`).
#[test]
fn conflict_minimize_mass_maximize_stiffness_emits_error() {
    let src = r#"structure S {
    param mass: Length = auto
    param stiffness: Length = auto
    minimize mass
    maximize stiffness
}"#;

    let compiled = compile_module(src, "test_conflict");

    // Find the ObjectiveConflict diagnostic.
    let conflict_diag = compiled
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ObjectiveConflict))
        .unwrap_or_else(|| {
            panic!(
                "expected an ObjectiveConflict diagnostic, got: {:#?}",
                compiled.diagnostics
            )
        });

    // Severity must be Error.
    assert_eq!(
        conflict_diag.severity,
        Severity::Error,
        "ObjectiveConflict must be Severity::Error, got {:?}",
        conflict_diag.severity
    );

    // Message must contain the PRD-prose mnemonic so the CLI renders it.
    assert!(
        conflict_diag.message.contains("E_OBJECTIVE_CONFLICT"),
        "message must contain \"E_OBJECTIVE_CONFLICT\", got: {:?}",
        conflict_diag.message
    );

    // Message must name the three escape routes.
    assert!(
        conflict_diag.message.contains("weight"),
        "message must mention the 'weight' escape, got: {:?}",
        conflict_diag.message
    );
    assert!(
        conflict_diag.message.contains("priority"),
        "message must mention the 'priority' escape, got: {:?}",
        conflict_diag.message
    );
    // The combine-into-one-expression escape: the message should mention
    // combining / merging the objectives into a single expression.
    assert!(
        conflict_diag.message.contains("expression")
            || conflict_diag.message.contains("combine")
            || conflict_diag.message.contains("single"),
        "message must mention combining into one expression, got: {:?}",
        conflict_diag.message
    );

    // Must carry ≥1 label with a non-empty span.
    assert!(
        !conflict_diag.labels.is_empty(),
        "ObjectiveConflict diagnostic must carry at least one label"
    );
    assert!(
        conflict_diag.labels.iter().any(|l| !l.span.is_empty()),
        "at least one label must have a non-empty span, labels: {:#?}",
        conflict_diag.labels
    );
}

// ── (b) NO-CONFLICT: same-sense, minimize mass + minimize cost ───────────────

/// Two `minimize` objectives over distinct expressions are **not** a conflict
/// (both have the same sense). No `ObjectiveConflict` diagnostic must be emitted.
#[test]
fn no_conflict_same_sense_minimize_minimize() {
    let src = r#"structure S {
    param mass: Length = auto
    param cost: Length = auto
    minimize mass
    minimize cost
}"#;

    let compiled = compile_module(src, "test_no_conflict_same_sense");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveConflict)),
        "two same-sense objectives must not produce ObjectiveConflict, got: {:#?}",
        compiled.diagnostics
    );
}

// ── (c) NO-CONFLICT: single objective ────────────────────────────────────────

/// A single `minimize` objective is never a conflict (terms.len() == 1).
/// No `ObjectiveConflict` diagnostic must be emitted.
#[test]
fn no_conflict_single_objective() {
    let src = r#"structure S {
    param mass: Length = auto
    minimize mass
}"#;

    let compiled = compile_module(src, "test_no_conflict_single");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveConflict)),
        "a single objective must not produce ObjectiveConflict, got: {:#?}",
        compiled.diagnostics
    );
}

// ── (d) NO-CONFLICT: mixed-sense SAME-expr ───────────────────────────────────

/// `minimize mass` + `maximize mass` — opposite sense over the **same** expression.
/// The distinct-expression qualifier (§6.3) excludes this case: the content
/// hashes of the two terms' expressions are equal, so no conflict is emitted.
#[test]
fn no_conflict_mixed_sense_same_expression() {
    let src = r#"structure S {
    param mass: Length = auto
    minimize mass
    maximize mass
}"#;

    let compiled = compile_module(src, "test_no_conflict_same_expr");

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveConflict)),
        "mixed-sense over the same expression must not produce ObjectiveConflict, got: {:#?}",
        compiled.diagnostics
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// E_OBJECTIVE_INERT — DIC γ (task #5417)
// ═════════════════════════════════════════════════════════════════════════════

/// Every `ObjectiveInert` diagnostic in a compiled module.
fn inert_diags(compiled: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ObjectiveInert))
        .collect()
}

/// Assert the module reports exactly one inert objective, that it is an Error
/// naming `cell`, and that it is renderable.
///
/// "Exactly one" is the #5014 aggregation rule: one diagnostic per objective
/// *declaration*, however many cells it names — not one per never-auto cell.
fn assert_one_inert_error_naming(compiled: &reify_compiler::CompiledModule, cell: &str) {
    let found = inert_diags(compiled);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one ObjectiveInert diagnostic, got {}: {:#?}",
        found.len(),
        compiled.diagnostics
    );
    let diag = found[0];

    assert_eq!(
        diag.severity,
        Severity::Error,
        "ObjectiveInert must be Severity::Error so `reify check` exits non-zero, got {:?}",
        diag.severity
    );
    assert!(
        diag.message.contains("E_OBJECTIVE_INERT"),
        "message must carry the PRD-prose mnemonic, got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains(cell),
        "message must name the never-auto cell `{cell}`, got: {:?}",
        diag.message
    );

    // The `diagnostic_coverage_checkpoint.rs` convention: a diagnostic the CLI
    // renders must point somewhere in the source.
    assert!(
        diag.labels.iter().any(|l| !l.span.is_empty()),
        "ObjectiveInert must carry ≥1 label with a non-empty span, labels: {:#?}",
        diag.labels
    );
}

/// Assert the module reports no inert objective at all.
fn assert_no_inert(compiled: &reify_compiler::CompiledModule, why: &str) {
    let found = inert_diags(compiled);
    assert!(
        found.is_empty(),
        "must not report ObjectiveInert ({why}), got: {found:#?}"
    );
}

/// Anti-vacuity guard for the negative cases: assert `template` compiled and
/// really does carry a declared objective.
///
/// Without this a negative case degenerates into "the source failed to build an
/// objective, so of course nothing was reported" — it would keep passing even
/// if the rule were rewritten to fire on everything.
fn assert_template_has_objective(compiled: &reify_compiler::CompiledModule, name: &str) {
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "fixture must compile a `{name}` template; got {:?}, diagnostics: {:#?}",
                compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>(),
                compiled.diagnostics
            )
        });
    assert!(
        template.objective.is_some(),
        "`{name}` must carry a declared objective or the negative case is vacuous; \
         diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// The purpose-side counterpart of [`assert_template_has_objective`]. Also
/// pins the structural exclusion the rule relies on: a purpose objective must
/// land on `CompiledPurpose`, never on a `TopologyTemplate`.
fn assert_purpose_has_objective(compiled: &reify_compiler::CompiledModule, name: &str) {
    let purpose = compiled
        .compiled_purposes
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| {
            panic!(
                "fixture must compile a `{name}` purpose; diagnostics: {:#?}",
                compiled.diagnostics
            )
        });
    assert!(
        purpose.objective.is_some(),
        "purpose `{name}` must carry an objective or the negative case is vacuous; \
         diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert!(
        compiled.templates.iter().all(|t| t.objective.is_none()),
        "a purpose objective must not appear on any template — that separation is \
         what excludes purposes from the pass"
    );
}

// ── POSITIVE: the two structurally-inert fixtures ────────────────────────────

/// Byte-mirror of `docs/prds/v0_6/fixtures/dic_min_no_autos.ri`.
///
/// A declared objective in a scope with **no autos at all**. `k` is a
/// literal-backed `param`, so `k * k` is a constant the solver cannot move:
/// the `minimize` is a statement of intent the language silently discards.
/// Baseline before this rule: total silence, exit 0.
#[test]
fn inert_objective_over_a_never_auto_param_errors() {
    let src = r#"module dic_min_no_autos

structure DicMinNoAutos {
    param k : Real = 3.0
    minimize k * k
}
"#;

    assert_one_inert_error_naming(&compile_source_with_stdlib(src), "k");
}

/// Byte-mirror of `docs/prds/v0_6/fixtures/dic_min_unread.ri`.
///
/// The scope *does* have an auto (`a`, bracketed by two constraints), but the
/// objective reads only `k`. Scope-level auto count is the wrong question —
/// what matters is reachability **from the objective** — so the presence of
/// `a` must not rescue the declaration.
#[test]
fn inert_objective_with_an_unrelated_auto_in_scope_errors() {
    let src = r#"module dic_min_unread

structure DicMinUnread {
    param a : Real = auto(free)
    param k : Real = 3.0
    constraint a >= 1.0
    constraint a <= 5.0
    minimize k * k
}
"#;

    assert_one_inert_error_naming(&compile_source_with_stdlib(src), "k");
}

// ── NEGATIVE: the objective genuinely reaches an auto ────────────────────────

/// Byte-mirror of `docs/prds/v0_6/fixtures/dic_min_unconstrained.ri`.
///
/// The objective reads `a` directly, so it is well-posed at compile time and
/// must stay compile-clean. That the solver then *drops* it (zero components,
/// nothing to attach the cost to) is the runtime half of DIC γ,
/// `E_OBJECTIVE_UNCONSUMED` — a different diagnostic on a different channel.
/// Reporting it here too would make the two halves fight over the same source.
#[test]
fn objective_reading_an_auto_directly_is_compile_clean() {
    let src = r#"module dic_min_unconstrained

structure DicMinUnconstrained {
    param a : Real = auto(free)
    minimize (a - 3.0) * (a - 3.0)
}
"#;

    let compiled = compile_source_with_stdlib(src);
    assert_template_has_objective(&compiled, "DicMinUnconstrained");
    assert_no_inert(&compiled, "the objective reads the auto `a` directly");
}

/// A `let` may not launder an auto out of existence. `minimize v` where
/// `let v = a * 2.0` and `a` is auto is exactly as governing as `minimize a`,
/// so the closure must follow the binding.
#[test]
fn objective_through_a_let_that_reads_an_auto_is_not_inert() {
    let src = r#"structure Indirect {
    param a : Real = auto(free)
    let v : Real = a * 2.0
    constraint a >= 1.0
    constraint a <= 5.0
    minimize v
}
"#;

    let compiled = compile_source_with_stdlib(src);
    assert_template_has_objective(&compiled, "Indirect");
    assert_no_inert(&compiled, "the objective reaches auto `a` through the let `v`");
}

// ── NEGATIVE: the coupling is invisible at compile time ──────────────────────

/// The `examples/whole_model_joint_drive.ri` shape. `cost(self.descendants)`
/// lowers to a node carrying **zero** compile-time `ValueRef`s, yet genuinely
/// couples to the child's `auto` at eval time. Judging it on its ref set alone
/// would report the single most important objective in the corpus as inert.
#[test]
fn cost_over_descendants_objective_is_not_inert() {
    let src = r#"structure Rivet {
    param unit_cost : Money = 0.50USD
    param quantity_produced : Real = auto(free)
    constraint quantity_produced >= 0.0
    constraint quantity_produced <= 100.0
}

structure RivetedPanel {
    sub rivets = Rivet()
    minimize cost(self.descendants)
}
"#;

    let compiled = compile_source_with_stdlib(src);
    assert_template_has_objective(&compiled, "RivetedPanel");
    assert_no_inert(
        &compiled,
        "an opaque aggregation term proves nothing about autos",
    );
}

// ── NEGATIVE: purposes are not templates ─────────────────────────────────────

/// A purpose body's objective lives on `CompiledPurpose.objective`, never on a
/// `TopologyTemplate`, and its `subject` is bound at application time — there
/// is no template whose autos could be counted. The pass must exclude purposes
/// structurally rather than by accident.
#[test]
fn purpose_objective_over_a_subject_member_is_not_inert() {
    let src = r#"structure Bracket {
    param width : Length = 80mm
}

purpose lightweight(subject : Structure) {
    constraint subject.mass > 0
    minimize subject.mass
}
"#;

    let compiled = compile_source_with_stdlib(src);
    assert_purpose_has_objective(&compiled, "lightweight");
    assert_no_inert(&compiled, "a purpose objective is not a template objective");
}

/// The degenerate purpose: `minimize 1mm` names no cell at all. With an empty
/// reference set the rule has nothing to claim and nothing to name, so it must
/// abstain rather than report a vacuously-true inertness.
#[test]
fn purpose_objective_over_a_literal_is_not_inert() {
    let src = r#"structure Bracket {
    param width : Length = 80mm
}

purpose tiny(subject : Structure) {
    minimize 1mm
}
"#;

    let compiled = compile_source_with_stdlib(src);
    assert_purpose_has_objective(&compiled, "tiny");
    assert_no_inert(&compiled, "a literal objective references no cell");
}

// ── NEGATIVE: the whole-module auto override ─────────────────────────────────

/// `sub c : Child { k = auto }` makes `Child`'s `minimize k * k` genuinely
/// governing — but the override cell is minted into the **parent** template as
/// `Parent.c`/`k` while `Child`'s own `k` stays a `Param`. A check with only
/// per-template visibility would report `Child` here, which is why the rule is
/// a module-level post-pass and not a per-entity check.
#[test]
fn sub_instance_auto_override_makes_the_child_objective_governing() {
    let src = r#"structure Child {
    param k : Real = 3.0
    minimize k * k
}

structure Parent {
    sub c : Child { k = auto }
    constraint self.c.k >= 1.0
    constraint self.c.k <= 5.0
}
"#;

    let compiled = compile_source_with_stdlib(src);
    assert_template_has_objective(&compiled, "Child");

    // Anti-vacuity: the override must really have landed in the PARENT as a
    // scoped auto — that scoped cell is the only evidence the rule has, so if
    // the phase ever stops minting it this case would silently stop testing
    // anything.
    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("fixture must compile a `Parent` template");
    assert!(
        parent.value_cells.iter().any(|c| c.id.entity == "Parent.c"
            && c.id.member == "k"
            && c.kind.is_auto()),
        "expected the scoped auto override cell `Parent.c`/`k`, got: {:?}",
        parent
            .value_cells
            .iter()
            .map(|c| (c.id.to_string(), c.kind))
            .collect::<Vec<_>>()
    );

    assert_no_inert(&compiled, "the parent instantiates Child with `k = auto`");
}

// ── NEGATIVE: no second diagnostic on an already-diagnosed shape ─────────────

/// A `minimize` inside a sub specialization body is already reported as the
/// `W_SUBBODY_OBJECTIVE_IGNORED` warning (task 4823) and never reaches a
/// template's `objective`. It must stay exactly that one warning: an added
/// Error here would turn a diagnosed-and-tolerated shape into a build failure.
#[test]
fn subbody_objective_stays_exactly_one_warning() {
    let src = r#"structure T {
    param a : Length = 5mm
}

structure A {
    sub x : T { minimize a }
}
"#;

    let compiled = compile_source_with_stdlib(src);

    assert_no_inert(&compiled, "a sub-body objective is dropped, not inert");
    assert_eq!(
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::SubbodyObjectiveIgnored))
            .count(),
        1,
        "the pre-existing subbody warning must survive unchanged, got: {:#?}",
        compiled.diagnostics
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "a sub-body objective must not become a compile error, got: {:#?}",
        compiled.diagnostics
    );
}
