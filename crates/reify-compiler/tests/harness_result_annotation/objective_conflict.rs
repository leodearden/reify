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

// ── CROSS-MODULE: an override this module cannot see ────────────────────────
//
// Review round 1, finding 1. `phase_inert_objective_check` judges a template
// against `ctx.templates` — the templates of the module being compiled, and
// nothing else. `ctx.templates` is initialized EMPTY (`ctx.rs`'s ctor, whose
// doc says "no prelude content is seeded here"), imported templates live in
// borrow-only registries during compilation, and `merge_imported_pub_templates`
// appends them onto the finished `CompiledModule` only AFTER compilation
// returns. So `auto_override_possible` — whose entire purpose is to notice a
// `sub w : Widget { k = auto }` override that makes `k` reachable-as-auto —
// cannot see an override that lives in another module.
//
// The direction is what makes this unfixable by threading the prelude: a
// prelude holds the modules this one IMPORTS (upstream), never the modules that
// import it (downstream), and the override is downstream. A `pub` template's
// consumers are simply unknowable at compile time, which under PRD §3
// decision 5 ("every ambiguity returns `None`") means silence.

/// Compile a two-module DAG — `child.ri` plus a `main.ri` that imports it — and
/// hand back the DAG so a caller can read EITHER module's diagnostics.
///
/// Follows `harness_modules_ports/module_dag_tests.rs`'s established idiom
/// (tempdir + `ModuleResolver` + `ModuleDag::compile_module`) rather than a new
/// entry point. The stdlib root deliberately points at a directory that does not
/// exist: `ModuleResolver` falls back to the embedded stdlib for `std.*`
/// imports, and these fixtures import nothing from it anyway.
fn compile_child_and_main(child_src: &str, main_src: &str) -> reify_compiler::module_dag::ModuleDag {
    use reify_compiler::module_dag::{ModuleDag, ModuleResolver};

    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().to_path_buf();
    std::fs::write(dir.join("child.ri"), child_src).expect("write child.ri");
    std::fs::write(dir.join("main.ri"), main_src).expect("write main.ri");

    let resolver = ModuleResolver::new(&dir, dir.join("nonexistent_stdlib"));
    let mut dag = ModuleDag::new();
    dag.compile_module("main", &resolver)
        .unwrap_or_else(|errors| panic!("the two-module DAG must compile: {errors:#?}"));
    dag
}

/// Assert `module_name`'s compiled form in `dag` reports no inert objective.
fn assert_dag_module_has_no_inert(
    dag: &reify_compiler::module_dag::ModuleDag,
    module_name: &str,
    why: &str,
) {
    let compiled = dag
        .modules
        .get(module_name)
        .unwrap_or_else(|| panic!("module `{module_name}` must be in the DAG"));
    assert_no_inert(compiled, why);
}

/// The reviewer's repro, compiled as the DAG it belongs to: `Widget.k` IS made
/// `auto` — by a `sub` override in a downstream module — so the objective over
/// it governs a real solver variable and the program is legal.
#[test]
fn exported_template_overridden_to_auto_downstream_is_compile_clean() {
    let dag = compile_child_and_main(
        r#"module child

pub structure def Widget {
    param k : Real = 3.0
    constraint k > 0.0
    minimize k
}
"#,
        r#"module main

import child

structure def App {
    sub w : Widget { k = auto }
    constraint self.w.k == 4.0
}

structure Root {
    sub app : App {}
}
"#,
    );
    assert_dag_module_has_no_inert(
        &dag,
        "child",
        "a downstream `sub w : Widget { k = auto }` makes `k` a solver variable; \
         the objective governs it and the program is legal",
    );
}

/// The HARDER and more important half: `child.ri` compiled ALONE, with no
/// consumer anywhere in the DAG, must also stay clean.
///
/// This is the reviewer's literal repro (`reify check child.ri`), and it is the
/// case that rules out "thread the prelude" as a fix: there is no consumer to
/// find, in the prelude or anywhere else, and one may be written tomorrow. A
/// `pub` template is compiled without knowledge of its consumers by
/// construction, so no positive proof of inertness is available — and a false
/// Inert REJECTS a legal program, whereas a missed one merely leaves today's
/// silence.
#[test]
fn exported_template_compiled_alone_is_compile_clean() {
    let src = r#"module child

pub structure def Widget {
    param k : Real = 3.0
    constraint k > 0.0
    minimize k
}
"#;
    let compiled = compile_source_with_stdlib(src);
    assert_template_has_objective(&compiled, "Widget");
    assert_no_inert(
        &compiled,
        "a `pub` template's consumers are unknowable at compile time, so \
         inertness cannot be proved",
    );
}

/// NEGATIVE GUARD — visibility must be the DISCRIMINATOR, not an off switch.
///
/// The byte-identical template WITHOUT `pub` is module-private: every consumer
/// that could override `k` is in this module and therefore in `all_templates`,
/// so inertness IS provable and the rule must still fire. Without this case a
/// step-22 that simply disabled the pass would pass the two tests above.
///
/// Kept adjacent to them deliberately: the pair differs in exactly one token.
#[test]
fn module_private_template_with_the_same_inert_objective_still_errors() {
    let src = r#"module child

structure def Widget {
    param k : Real = 3.0
    constraint k > 0.0
    minimize k
}

structure Root {
    sub w : Widget {}
}
"#;
    assert_one_inert_error_naming(&compile_source_with_stdlib(src), "k");
}

/// SECOND ROUTE into the same false positive, found while verifying finding 1
/// and absent from the review: an imported GENERIC objective-bearing structure
/// reaches `ctx.templates` as a monomorph CLONE, so the pass judges ANOTHER
/// module's objective with only *this* module's visibility.
///
/// `phase_auto_type_param_resolution` resolves the target from a registry that
/// chains prelude templates in unfiltered (`auto_type_param_phase.rs`'s
/// `template_registry`, built from `prelude.iter().flat_map(|m| m.templates)`),
/// clones it (`let mut mono = target.clone();`) and pushes the clone into
/// `ctx.templates`. That phase's own known-gap comment lists `objective` among
/// the fields it does NOT substitute, so the clone carries the generic's
/// `minimize` verbatim. It runs well before `phase_inert_objective_check`
/// (`lib.rs`), which then reports on it — anchoring the label at a
/// `SourceSpan` that indexes into the *defining* module's source text, so even
/// the rendered location is wrong.
///
/// The assertion is on `main`, not `child`: `child` compiled alone is the
/// first route and is covered above.
#[test]
fn imported_generic_monomorph_objective_is_compile_clean() {
    let dag = compile_child_and_main(
        r#"module child

trait Seal {}

pub structure def ORingSeal : Seal { param d : Real = 10.0 }

pub structure def Bearing<T: Seal> {
    param bore : Real = 25.0
    constraint bore > 0.0
    minimize bore
}
"#,
        r#"module main

import child

structure def Assembly { sub b = Bearing<auto: Seal>() }

structure Root { sub a : Assembly {} }
"#,
    );
    assert_dag_module_has_no_inert(
        &dag,
        "main",
        "a monomorph clone of an IMPORTED generic carries the defining module's \
         objective; this module can see neither that module's overrides nor its \
         downstream consumers, so inertness is not provable here",
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

/// The MODULE-PRIVATE half of the monomorph route — the case that proves the
/// visibility bail alone is not enough.
///
/// MEASURED while implementing the fix: with `Bearing` declared WITHOUT `pub`
/// in `child.ri`, main's compiled templates still contain
/// `Bearing$ORingSeal visibility=Private objective=true`, and
/// `E_OBJECTIVE_INERT` still fired. `phase_auto_type_param_resolution` builds
/// its `template_registry` from `prelude.iter().flat_map(|m| m.templates)`
/// with no visibility filter, so a private target monomorphises just the same.
///
/// Kept as a sibling of the `pub` case above deliberately: the two differ in
/// exactly one token, and a fix that keyed only on visibility would pass the
/// `pub` one and fail this.
#[test]
fn imported_private_generic_monomorph_objective_is_compile_clean() {
    let dag = compile_child_and_main(
        r#"module child

trait Seal {}

pub structure def ORingSeal : Seal { param d : Real = 10.0 }

structure def Bearing<T: Seal> {
    param bore : Real = 25.0
    constraint bore > 0.0
    minimize bore
}
"#,
        r#"module main

import child

structure def Assembly { sub b = Bearing<auto: Seal>() }

structure Root { sub a : Assembly {} }
"#,
    );
    assert_dag_module_has_no_inert(
        &dag,
        "main",
        "a private imported generic monomorphises into this module all the same, \
         so the clone's `Private` visibility does not make its objective judgeable here",
    );
}
