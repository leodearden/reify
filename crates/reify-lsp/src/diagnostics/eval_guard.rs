//! Containment guard for `reify-eval`'s unrepresentable-value-cell panic
//! (root cause owned by task **#6851**; this module is its caller-side
//! "fix-site 2", scoped to `reify-lsp`).
//!
//! One predicate, asked by all three LSP production entry points —
//! [`crate::diagnostics::compute_diagnostics_with_state`],
//! [`crate::diagnostics::compute_diagnostics`] and
//! `crate::analysis::AnalysisContext::from_parsed` — immediately before they
//! would hand a compiled module to the engine. It is a `reify-eval`
//! graph-safety question, not diagnostics logic, so it lives in its own file
//! rather than in `diagnostics.rs`: `analysis.rs` gets an import that names
//! what it is asking, and the predicate's ~250 lines of derivation stay
//! readable next to the tests that pin them.
//!
//! (A top-level `crate::eval_guard` would read better still at the
//! `analysis.rs` call site, but declaring one means a `mod` line in `lib.rs`,
//! which is outside this task's file scope.)

use reify_core::{Type, ValueCellId};

/// The value-cell decls [`first_unrepresentable_cell`]'s stage-1 absence proof
/// scans — see that function's "## Cost" section for WHICH collections these
/// are and why that set is sound.
///
/// Extracted so the production scan has ONE definition, shared with
/// `tests::stage_one_absence_proof_covers_every_graph_cell_type`. That test
/// asserts a property OF this scan; if it kept its own copy of the
/// collections, a future narrowing of the scan would leave the test asserting
/// the property of the wider copy and passing while production went unsound —
/// which is the precise shape of the defect that motivated it.
fn stage_one_scanned_cells(
    compiled: &reify_compiler::CompiledModule,
) -> impl Iterator<Item = &reify_compiler::ValueCellDecl> {
    compiled.templates.iter().flat_map(|t| {
        t.value_cells.iter().chain(
            t.guarded_groups
                .iter()
                .flat_map(|g| g.members.iter().chain(g.else_members.iter())),
        )
    })
}

/// `Some((id, cell_type))` when the evaluation graph `compiled` would build
/// carries a value cell whose `cell_type` has no runtime `Value` counterpart
/// — the exact condition `reify-eval`'s `#[cfg(debug_assertions)]`
/// `assert_value_cell_types_representable` panics on. `None` when the graph is
/// safe to evaluate.
///
/// This is THE containment predicate; [`compiled_graph_has_unrepresentable_cell`]
/// is a test-only `bool` face over it. It returns the offender rather than a
/// bare bit so the three guard sites can say WHICH cell suppressed their
/// eval/check pass. Without that the degradation is silent: the document loses
/// every eval-time diagnostic and every constraint result (and in
/// `analysis.rs`, every hover/completion computed value) with nothing in the
/// server log, making "my constraints stopped being checked" indistinguishable
/// in the field from "the engine thinks they're satisfied".
///
/// "First" is first in `graph.value_cells`' iteration order — an `im::HashMap`,
/// so hash order, NOT source order. A module with several unrepresentable
/// cells reports an arbitrary one of them; the value is for a log line naming
/// a concrete offender, never for an ordering-sensitive assertion.
///
/// ## What this is for (task #6851 containment)
///
/// `phase_auto_type_param_resolution`
/// (`crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs`)
/// gates ALL substitution behind `if !sigma.is_empty()`. On a FAILED
/// resolution `outcome.substitution` is empty, so no monomorph is
/// synthesized: the sub-component still names the generic template, and a
/// member declared `param seal : T` keeps `cell_type = Type::TypeParam("T")`
/// all the way into the evaluation graph. `reify-eval`'s
/// `assert_value_cell_types_representable`
/// (`crates/reify-eval/src/engine_eval.rs`) then PANICS —
/// "unrepresentable cell_type" — taking the language server down mid-keystroke
/// in a debug build. In release the assertion is elided and eval instead
/// yields a confusing `TypeKindMismatch`/`Undef`.
///
/// The root-cause fix (synthesize a fallback substitution, or make the cell
/// representable) is owned by **task #6851**, whose addendum independently
/// designates a caller-side error gate as its "fix-site 2
/// (defence-in-depth)". This predicate is that gate, scoped to `reify-lsp`
/// only: the three LSP production entry points skip their eval/check pass
/// when it returns `true`, so the user still gets the compile-stage
/// diagnostics that explain what is wrong, and the server stays alive.
/// `crates/reify-cli/src/mcp_context.rs`'s ungated `engine.eval` sites
/// remain #6851's to fix.
///
/// ## Why the graph, and not the compile diagnostics
///
/// The predicate asks the *actual hazard* — "does the graph the engine is
/// about to build contain an unrepresentable cell" — rather than a proxy for
/// it. It is exact by construction:
/// `reify_eval::graph::EvaluationGraph::from_templates` is literally the call
/// `Snapshot::from_compiled_module` makes
/// (`crates/reify-eval/src/snapshot.rs`) to build the graph the assertion
/// then walks, and `reify_eval::is_representable_cell_type` is the
/// assertion's own single-source-of-truth predicate. There is no third
/// notion of "unrepresentable" here to drift out of step with reify-eval's.
///
/// Two cheaper-looking formulations were measured and rejected:
///
/// - **An `AutoTypeParam*` diagnostic-code prefix match.** It over-fires and
///   under-fires simultaneously. Over: a module whose failing `auto:` bound
///   leaves `T` UNUSED in the target body (exactly
///   `auto_type_param_fixtures::BT8_CONSTANT_CONSTRAINT_SRC`'s shape) has no
///   unrepresentable cell and evals safely, yet a code match suppresses eval
///   for the WHOLE document — measured: three real diagnostics lost (a
///   circular let-binding plus two constraint violations), and via the
///   identical guard in `analysis.rs` an empty `check_result.values`, so
///   hover/completion show no computed values file-wide. Under:
///   `phase_auto_type_param_resolution`'s `monomorph_name_would_collide`
///   path pushes a CODE-LESS error and `continue`s, leaving the same
///   unsubstituted cells invisible to any code match. The over-fire half is
///   pinned by case (2) of
///   `tests::unrepresentable_cell_predicate_tracks_the_graph_not_the_diagnostic_codes`;
///   the under-fire half is NOT test-pinned, because the collision is
///   unreachable from valid `.ri` source (`$` is illegal in identifiers —
///   the compiler itself calls that arm "impossible from source; this is a
///   compiler bug"). Structural detection needs no code, so it holds anyway.
/// - **A direct scan of `compiled.templates`' `value_cells`.** Measured to
///   over-fire on every SUCCESSFUL generic instantiation: the generic
///   `Bearing` template SURVIVES monomorphisation carrying
///   `Bearing.seal : TypeParam("T")`, and the synthesized
///   `Bearing$GasketSeal` monomorph re-uses the SAME `ValueCellId`
///   (`Bearing.seal`) with the substituted type. Only the last writer into
///   `graph.value_cells` wins, which is what makes the successful case safe —
///   a flat template scan cannot see that, and would suppress eval on healthy
///   modules.
///
/// ## Cost
///
/// Two-stage, because the exact question is not cheap enough to ask on every
/// keystroke. `EvaluationGraph::from_templates` is not free — per value cell
/// it does a `format!("{}", cell.id)` allocation, two content hashes and a
/// map insert, and it also walks constraints, realizations and sub-component
/// elaboration (`crates/reify-eval/src/graph.rs`).
///
/// Stage 1 is an allocation-free `all(is_representable_cell_type)` scan over
/// the cell decls `compiled.templates` carry, used ONLY to prove ABSENCE. Its
/// soundness is a claim about `from_templates`, so it is derived from that
/// function rather than restated: `crates/reify-eval/src/graph.rs` has SEVEN
/// non-test `graph.value_cells.insert` arms, and every one clones its
/// `cell_type` verbatim from a decl stage 1 sees, or is safe by construction.
///
/// - **Arm 1** — the top-level `template.value_cells` loop.
/// - **Arms 2-4** — the collection, keyed and non-collection sub-component
///   elaboration arms: all three draw from `child_template.value_cells`, bound
///   ONCE via `find_template(templates, ..)` on the SAME slice stage 1
///   iterates, which is why they need no separate coverage.
/// - **Arm 5** — the guard cell of a guarded group, whose `cell_type` is the
///   literal `Type::Bool` at its insert site rather than a clone of any decl:
///   safe by construction, and the one arm stage 1 does not scan.
/// - **Arms 6-7** — `guarded_groups[*].members` and `[*].else_members`.
///
/// Arms 6-7 are why the scan chains the guarded collections instead of reading
/// `value_cells` alone: the compiler collects a guarded member into
/// `CompiledGuardedGroup::members`, a Vec that NEVER appears in
/// `TopologyTemplate::value_cells`, so `value_cells` is a strict subset of
/// what reaches the graph. Scanning the subset made the absence proof unsound
/// in exactly one direction — the direction stage 1 uses. Measured on
/// `auto_type_param_fixtures::GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC`: stage 1
/// found every cell it scanned representable and returned `None`, while the
/// graph carried `Bearing.seal : TypeParam("T")` and the eval pass panicked.
/// Pinned by case (5) of
/// `tests::unrepresentable_cell_predicate_tracks_the_graph_not_the_diagnostic_codes`
/// and, per-arm over a corpus, by
/// `tests::stage_one_absence_proof_covers_every_graph_cell_type`.
///
/// `ports.members` is deliberately NOT scanned. It IS a type-param
/// substitution target — `substitute_value_cell_collection`'s own doc names
/// `value_cells`, `guarded_groups.members/else_members` and `ports.members`
/// together — but it never reaches `graph.value_cells`: `graph.rs` does not
/// mention `ports` at all. Stage 1's obligation is the graph's cells, not the
/// substituter's, and the two collections differ; the next reader would
/// otherwise re-derive that from scratch.
///
/// So "no scanned cell is unrepresentable" strictly implies "no graph cell is
/// unrepresentable", and the common case (every healthy document, every
/// keystroke) returns without building a graph. The converse does NOT hold — a
/// template cell can be unrepresentable while the graph is safe, which is
/// exactly the successful-monomorphisation case rejected above — so stage 1 is
/// never allowed to answer `true`. Widening it is therefore always safe: more
/// collections can only cause more fall-through to the authoritative stage-2
/// walk, never a hit of stage 1's own.
///
/// Stage 2, reached only when stage 1 finds a candidate, is the exact
/// `from_templates` walk. It is the SAME construction the gated eval would
/// itself run, so on the shape that actually trips the guard the cost is at
/// worst a doubled graph construction, and never an added eval.
///
/// The staging matters most on the WARM path. `compute_diagnostics_with_state`
/// takes the `content_unchanged` branch on a repeat request, where the gated
/// work is `eval_cached` + `check_snapshot` — neither of which calls
/// `from_templates` (`eval_cached` builds a combined param/let graph;
/// `check_snapshot` reuses the stored snapshot). An unconditional exact walk
/// there would be pure new cost with no counterpart; the stage-1 scan is not.
///
/// ## Blast radius: wider than `auto:`, deliberately
///
/// Because it asks about the graph rather than about `auto:` resolution, the
/// guard also contains two PRE-EXISTING crash shapes that have nothing to do
/// with task #6798's checker swap, both measured to panic `Engine::check` at
/// HEAD: an explicit generic instantiation (`sub b = Bearing<GasketSeal>()`
/// with `param seal : T`), and a generic structure that is merely DECLARED
/// and never instantiated. Containing them is a strict improvement — the
/// alternative on those inputs is a dead language server — but the root cause
/// stays task #6851's.
///
/// ## Why narrow rather than the CLI's blanket error gate
///
/// `reify-cli` skips eval on `any(|d| d.severity == Severity::Error)`
/// (`crates/reify-cli/src/main.rs`). The LSP deliberately evaluates THROUGH
/// non-fatal compile errors so keystroke-time eval diagnostics keep flowing
/// while the user is mid-edit. A compile error that leaves the graph
/// representable does not suppress eval here — pinned at the production entry
/// points by
/// `crate::diagnostics::tests::non_auto_compile_error_still_yields_eval_diagnostics`.
pub(crate) fn first_unrepresentable_cell(
    compiled: &reify_compiler::CompiledModule,
) -> Option<(ValueCellId, Type)> {
    // Stage 1 — cheap ABSENCE proof, no allocation and no graph. Sound only in
    // this direction (see the "## Cost" doc section): it may find a candidate
    // on a module whose graph is in fact safe, so it may only ever return
    // early with `None`, never report a hit of its own.
    if stage_one_scanned_cells(compiled)
        .all(|c| reify_eval::is_representable_cell_type(&c.cell_type))
    {
        return None;
    }

    // Stage 2 — the exact question, on the rare shape that got past stage 1.
    reify_eval::graph::EvaluationGraph::from_templates(&compiled.templates)
        .value_cells
        .iter()
        .find(|(_, node)| !reify_eval::is_representable_cell_type(&node.cell_type))
        .map(|(id, node)| (id.clone(), node.cell_type.clone()))
}

/// Boolean face of [`first_unrepresentable_cell`] — see that function's doc
/// for the mechanism, the cost model and the rejected alternatives.
///
/// **Test-only.** All three production entry points need the OFFENDER, not
/// just the bit, because they log which cell forced the skip — so nothing in
/// the lib build calls this form. It is kept because the property the unit
/// test below pins genuinely IS a boolean ("fires / does not fire", per
/// compiled shape), and spelling `.is_some()` at each of those call sites
/// would bury the predicate's contract in an accessor.
/// `#[cfg(test)]` states that split honestly rather than leaving a
/// `pub(crate)` wrapper nothing builds.
#[cfg(test)]
pub(crate) fn compiled_graph_has_unrepresentable_cell(
    compiled: &reify_compiler::CompiledModule,
) -> bool {
    first_unrepresentable_cell(compiled).is_some()
}

#[cfg(test)]
mod tests {
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::{ModulePath, Type};

    use crate::diagnostics::auto_type_param_fixtures::{
        AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC, BT8_CONSTANT_CONSTRAINT_SRC,
        GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC, assert_guarded_group_fixture_is_newly_reachable,
    };

    /// Unit-pin [`super::compiled_graph_has_unrepresentable_cell`] — the
    /// predicate that gates the LSP's eval/check pass — against the compiled
    /// shapes that decide whether it is asking the right question.
    ///
    /// **Why this shape and not a classifier test.** An earlier form of the
    /// guard matched an `AutoTypeParam*` prefix over `DiagnosticCode`, which
    /// is a PROXY for the hazard rather than the hazard itself. The cases
    /// below are the measured proof that the proxy is wrong in BOTH
    /// directions and that the structural predicate is right in both — they
    /// are the executable form of the "Why the graph, and not the compile
    /// diagnostics" section of the predicate's own doc.
    ///
    /// Deliberately compiles real fixtures instead of hand-building
    /// `CompiledModule` values: the property under test is a fact about the
    /// graph the COMPILER produces, so a synthetic module would pin the
    /// predicate against this test's own model of the compiler rather than
    /// against the compiler.
    #[test]
    fn unrepresentable_cell_predicate_tracks_the_graph_not_the_diagnostic_codes() {
        fn fires(src: &str) -> bool {
            let parsed = reify_compiler::parse_with_stdlib(src, ModulePath::single("test"));
            let compiled =
                reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
            super::compiled_graph_has_unrepresentable_cell(&compiled)
        }

        // (1) POSITIVE — the hazard itself. A failed `auto:` resolution over a
        // body that USES `T` leaves `Bearing.seal : TypeParam("T")` in the
        // graph, which is exactly what `assert_value_cell_types_representable`
        // panics on. Anti-vacuity for this fixture (stub clean, real checker
        // fails) lives in `assert_auto_fail_fixture_is_newly_reachable`.
        assert!(
            fires(AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC),
            "a failed `auto:` resolution over a body that uses `T` leaves an \
             unrepresentable cell in the graph — the guard MUST fire, or the \
             engine panics (task #6851)"
        );

        // (2) NEGATIVE — the proxy's OVER-fire. Same failed `auto:` resolution,
        // but `T` is unused in the target body, so no unrepresentable cell is
        // ever created and eval is provably safe. The `AutoTypeParam*` code
        // match fired here and blanked every eval diagnostic for the whole
        // document; the structural predicate does not. Wired end-to-end by
        // `failed_auto_resolution_with_unused_type_param_still_yields_eval_diagnostics`.
        assert!(
            !fires(BT8_CONSTANT_CONSTRAINT_SRC),
            "BT8's failed `auto:` resolution leaves `T` UNUSED, so the graph \
             carries no unrepresentable cell and eval is safe — the guard must \
             NOT fire, or one `auto:` clause silently blanks eval for the whole \
             document"
        );

        // (3) NEGATIVE — the CLI's blanket-error shape, rejected. An
        // Error-severity compile diagnostic that leaves the graph
        // representable must not suppress eval; the LSP evaluates through
        // non-fatal compile errors on purpose. Wired end-to-end by
        // `non_auto_compile_error_still_yields_eval_diagnostics`.
        assert!(
            !fires("structure S { param x : Real = nope }\n"),
            "an UnresolvedName compile error leaves the graph representable — \
             the guard must not drift into the CLI's blanket \
             `any(severity == Error)` gate"
        );

        // (4) NEGATIVE — the template-scan formulation, rejected. A SUCCESSFUL
        // `auto:` resolution leaves the generic `Bearing` template in
        // `compiled.templates` still carrying `Bearing.seal : TypeParam("T")`;
        // only the monomorph's same-id cell, written later into
        // `graph.value_cells`, makes it safe. A flat scan of
        // `compiled.templates` fires here; the graph-level predicate does not.
        //
        // This case doubles as the pin for the STAGE-1 FAST PATH inside
        // `first_unrepresentable_cell`. That stage IS a flat template scan —
        // the very formulation rejected here — and is sound only as an
        // ABSENCE proof. This fixture is exactly the shape where it finds a
        // candidate and the graph is nevertheless safe, so an edit that let
        // stage 1 answer `true` on its own (rather than falling through to
        // the exact `from_templates` walk) goes RED right here.
        assert!(
            !fires(
                r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    param seal : T
    constraint bore > 0.1
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#
            ),
            "a SUCCESSFUL `auto:` resolution is safe even though the surviving \
             generic template still carries a TypeParam cell — the monomorph \
             overwrites it at the same ValueCellId when the graph is built. A \
             flat `compiled.templates` scan would fire here and suppress eval \
             on a healthy module"
        );

        // (5) POSITIVE — case (1)'s hazard with the `param seal : T` member
        // inside a GUARDED group, which is where stage 1's absence proof was
        // UNSOUND. A guarded member lives in `CompiledGuardedGroup::members`,
        // a Vec that never appears in `TopologyTemplate::value_cells`, yet
        // `from_templates` inserts it into `graph.value_cells` all the same.
        // Measured before the fix: stage 1 found every scanned cell
        // representable and short-circuited to `None`, while the graph carried
        // `Bearing.seal : TypeParam("T")` and `compute_diagnostics` panicked at
        // `crates/reify-eval/src/engine_eval.rs:210`. So this case is not a
        // variant spelling of (1) — it is the arm (1) cannot reach, and an
        // absence proof that scans a strict subset of what reaches the graph
        // goes RED right here.
        assert_guarded_group_fixture_is_newly_reachable();
        assert!(
            fires(GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC),
            "a failed `auto:` resolution leaves an unrepresentable cell in the \
             graph whether the `param seal : T` member is plain or GUARDED — \
             the guard MUST fire on both, or the engine panics on the guarded \
             one (task #6851). Stage 1 may only ever prove ABSENCE, so it has \
             to scan every collection `EvaluationGraph::from_templates` draws \
             cells from, not just `value_cells`"
        );
    }

    /// Close the RECURRENCE behind the guarded-group hole: make stage 1's
    /// soundness IMPLICATION executable, so a future `from_templates` arm
    /// cannot silently re-open it.
    ///
    /// Widening the scan fixed the shape that shipped a panic, but not the
    /// underlying defect: `reify-lsp` mirrors, in prose, an enumeration owned
    /// by `reify-eval` — "every `cell_type` reaching `graph.value_cells` is
    /// cloned from a decl stage 1 scans" — with nothing coupling the two. That
    /// mirror is exactly what drifted. This test asserts the implication
    /// DIRECTLY rather than restating the arm list a third time: for each
    /// input, every `cell_type` in the graph must also be among the types
    /// [`super::stage_one_scanned_cells`] yields, with `Type::Bool` the single
    /// allowed exception (a guarded group's guard cell is a literal
    /// `Type::Bool` at its insert site, cloned from no decl, and safe by
    /// construction). A new arm drawing from a new collection trips this on
    /// whichever corpus input reaches it, without anyone having to remember to
    /// update a list.
    ///
    /// **Green on arrival, and honest about it.** Like
    /// `fea_bearing_constraint_produces_no_false_violation_or_false_pass`,
    /// this locks that something STAYS true rather than driving a change. It
    /// WOULD have gone red on the defect it follows: at the pre-fix scan,
    /// `GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC`'s graph carries
    /// `TypeParam("T")` and the unwidened scan yields no such type, and the
    /// same holds for the `both branches` input, whose guarded members are
    /// deliberately typed `Int` and `Enum("Finish")` — types NO cell in
    /// `value_cells` carries, so dropping either half of the `members` /
    /// `else_members` chain trips it.
    ///
    /// **Corpus-bounded, and the bound is real.** The property is over TYPES,
    /// so a future arm whose cells happen to carry a type some scanned decl
    /// already has would pass here. Full enforcement needs `reify-eval` to
    /// expose its enumeration as an iterator over the cell decls a template
    /// contributes, so both consumers share one definition instead of two —
    /// outside this leaf's `Modules: reify-lsp` scope, filed as follow-up
    /// task **#7347**. Until then the corpus is what
    /// carries the coverage, which is why each input asserts a WITNESS cell id
    /// proving its arm actually fired: a compiler change that stops producing
    /// an arm's cells makes this loudly stale rather than quietly vacuous.
    #[test]
    fn stage_one_absence_proof_covers_every_graph_cell_type() {
        // (arm under test, source, witness cell ids proving the arm fired).
        // Between them these span every non-test `graph.value_cells.insert`
        // arm in `crates/reify-eval/src/graph.rs`; the witnesses are measured,
        // not guessed.
        let corpus: [(&str, &str, &[&str]); 6] = [
            (
                "guarded group, both branches (members + else_members + guard cell)",
                r#"enum Shape { Round, Square }
enum Finish { Raw, Coated }
structure Fitting {
    let shape = Shape.Round
    param size : Real = 10.0
    where shape == Shape.Round {
        param ribs : Int = 3
    } else {
        param finish : Finish = Finish.Raw
    }
}
"#,
                &["Fitting.__guard_", "Fitting.ribs", "Fitting.finish"],
            ),
            (
                "collection sub-component",
                r#"structure def Screw { param d : Real = 3.0 }
structure Rack {
    sub screws : List<Screw>
    constraint screws.count == 2
}
"#,
                &["Rack.screws[0].d"],
            ),
            (
                "keyed sub-component",
                r#"structure def Vent { param area : Real = 1.0 }
structure Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5.0 }
        "exhaust" => { area = 8.0 }
    }
}
"#,
                &["Manifold.vents[\"intake\"].area"],
            ),
            (
                "plain (non-collection) sub-component",
                r#"structure def Inner { param x : Real = 1.0 }
structure Outer { sub i = Inner() }
"#,
                &["Outer.i.x"],
            ),
            (
                "successful generic monomorphisation",
                r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    param seal : T
    constraint bore > 0.1
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#,
                &["Assembly.b.seal"],
            ),
            (
                "guarded member left unsubstituted by a FAILED `auto:` resolution",
                GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC,
                &["Bearing.seal"],
            ),
        ];

        for (arm, src, witnesses) in corpus {
            let parsed = reify_compiler::parse_with_stdlib(src, ModulePath::single("test"));
            let compiled =
                reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
            let graph = reify_eval::graph::EvaluationGraph::from_templates(&compiled.templates);

            let cell_ids: Vec<String> = graph.value_cells.keys().map(|id| id.to_string()).collect();
            for witness in witnesses {
                assert!(
                    cell_ids.iter().any(|id| id.contains(witness)),
                    "corpus staleness ({arm}): no graph cell id contains \
                     `{witness}`, so this input no longer exercises the arm it \
                     was chosen for and its coverage here is vacuous. Fix the \
                     input rather than the witness. graph cell ids: {cell_ids:#?}"
                );
            }

            let scanned: std::collections::HashSet<Type> =
                super::stage_one_scanned_cells(&compiled)
                    .map(|decl| decl.cell_type.clone())
                    .collect();
            for (id, node) in graph.value_cells.iter() {
                assert!(
                    node.cell_type == Type::Bool || scanned.contains(&node.cell_type),
                    "stage-1 absence proof is UNSOUND ({arm}): graph cell \
                     `{id}` has cell_type {:?}, which `stage_one_scanned_cells` \
                     never yields — so a module whose ONLY unrepresentable cell \
                     is this one short-circuits to `None` and the engine \
                     panics on it. Add the collection this cell comes from to \
                     the stage-1 scan (see `first_unrepresentable_cell`'s \
                     \"## Cost\" section), not an exception here. scanned \
                     types: {scanned:#?}",
                    node.cell_type
                );
            }
        }

        // The two stages must also AGREE on the shape that motivated all of
        // this: stage 1 must decline to short-circuit, and stage 2 must then
        // name the offender. Asserting only the property above would leave a
        // stage 1 that scans the right collections but whose result is wired
        // up wrongly (e.g. inverted, or dropped) undetected here.
        let parsed = reify_compiler::parse_with_stdlib(
            GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC,
            ModulePath::single("test"),
        );
        let compiled =
            reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
        assert!(
            !super::stage_one_scanned_cells(&compiled)
                .all(|c| reify_eval::is_representable_cell_type(&c.cell_type)),
            "stage 1 must find a candidate on the guarded auto-fail fixture — \
             if it proves ABSENCE here it short-circuits to `None` and stage 2 \
             never runs, which is the exact defect this test family follows"
        );
        assert_eq!(
            super::first_unrepresentable_cell(&compiled).map(|(id, ty)| (id.to_string(), ty)),
            Some(("Bearing.seal".to_string(), Type::TypeParam("T".to_string()))),
            "stage 2 must then name the offender. The fixture's graph has \
             exactly ONE unrepresentable cell, so this equality is independent \
             of `graph.value_cells`' hash iteration order — see \
             `first_unrepresentable_cell`'s note that \"first\" is otherwise \
             arbitrary"
        );
    }
}
