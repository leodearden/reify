//! The three D2 passes COMPOSED, in `cmd_check`'s order.
//!
//! The comment block at `cmd_check`'s sub-path (b) declares twice that the
//! ordering `merge_post_build_verdicts` → `drop_falsified_indeterminate_
//! diagnostics` → `merge_build_diagnostics` is LOAD-BEARING, but every other
//! test in this file exercises the three helpers in ISOLATION, and the
//! integration lock uses a fixture where `check()` resolves the constraint on
//! its own — so `merge_post_build_verdicts` performs no upgrade and the
//! ordering dependency is never exercised end to end.  A future refactor could
//! reorder the calls and every existing test would still pass.

    use super::{
        check_gating_error, dedup_diagnostics, drop_falsified_indeterminate_diagnostics,
        merge_build_diagnostics, merge_post_build_verdicts, strip_diagnostics_reproduced_by,
    };
    use reify_core::{
        ConstraintNodeId, Diagnostic, DiagnosticCode, DiagnosticLabel, Severity, SourceSpan,
    };
    use reify_eval::{BuildResult, CheckResult, ConstraintCheckEntry};
    use reify_ir::Satisfaction;

    /// Deliberately message-only, NOT `super::diagnostic_identity`: assertions
    /// that reused the production key could not observe a change to it.
    fn key(d: &Diagnostic) -> (Severity, Option<DiagnosticCode>, String) {
        (d.severity, d.code, d.message.clone())
    }

    /// The identity a caller actually cares about when spans are the point:
    /// message text plus the primary label span.
    fn key_with_span(d: &Diagnostic) -> (String, Option<(u32, u32)>) {
        (
            d.message.clone(),
            d.labels.first().map(|l| (l.span.start, l.span.end)),
        )
    }

    /// A GD&T illegal-modifier line as `engine_constraints::
    /// illegal_modifier_error` builds it: the message names the characteristic
    /// only, the LOCATION lives in the label.
    fn gdt_illegal_modifier(start: u32, end: u32) -> Diagnostic {
        Diagnostic::error(
            "`Flatness` is an RFS-only tolerance characteristic; material condition \
             modifiers (MMC/LMC) are not permitted",
        )
        .with_code(DiagnosticCode::GdtIllegalModifier)
        .with_label(DiagnosticLabel::new(
            SourceSpan::new(start, end),
            "illegal material condition modifier applied here",
        ))
    }

    fn entry(index: u32, satisfaction: Satisfaction) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            id: ConstraintNodeId::new("BoltFlange", index),
            label: None,
            satisfaction,
            indeterminate_reason: None,
        }
    }

    fn indeterminate_warning(needle: &str) -> Diagnostic {
        Diagnostic::warning(format!(
            "constraint {} indeterminate: undefined inputs: BoltFlange.moi_principal",
            needle
        ))
        .with_code(DiagnosticCode::ConstraintIndeterminate)
    }

    /// Runs the three passes exactly as `cmd_check`'s sub-path (b) does and
    /// returns the diagnostic list that reaches `report_eval_output`.
    fn run_in_cmd_check_order(result: &mut CheckResult, build: &BuildResult) -> Vec<Diagnostic> {
        merge_post_build_verdicts(result, Some(build));
        let build_diagnostics = drop_falsified_indeterminate_diagnostics(
            &build.diagnostics,
            &result.constraint_results,
        );
        merge_build_diagnostics(&result.diagnostics, &build_diagnostics)
    }

    /// Runs the passes exactly as `cmd_check`'s sub-path (c) `--purpose`
    /// `used_build` arm does, and returns the list that reaches
    /// `report_eval_output`.
    ///
    /// A DIFFERENT composition from [`run_in_cmd_check_order`], not a variant of
    /// it: the seed is BUILD's list (not check's), there is an extra
    /// self-dedup pass, and the GD&T re-run merges in last.  Only
    /// `check_purpose_surfaces_geometry_compile_error` exercised it end to end,
    /// and that test's content assertions are OCCT-gated — so in a stub-mode
    /// build nothing verified this arm at all.  These are kernel-independent.
    fn run_in_cmd_check_purpose_order(
        build_diags: &[Diagnostic],
        check_diags: &[Diagnostic],
        gdt_diags: &[Diagnostic],
        constraint_results: &[ConstraintCheckEntry],
    ) -> Vec<Diagnostic> {
        let build_diags = drop_falsified_indeterminate_diagnostics(build_diags, constraint_results);
        let realization_only = strip_diagnostics_reproduced_by(&build_diags, gdt_diags);
        let deduped_build = dedup_diagnostics(&realization_only);
        let mut merged = merge_build_diagnostics(&deduped_build, check_diags);
        merged.extend(gdt_diags.to_vec());
        merged
    }

    /// Sub-path (c)'s return leg: `check_constraints_with_values` is the
    /// authoritative verdict source there too, so build's stale indeterminacy
    /// claim about a constraint it resolved definitely must survive in NEITHER
    /// list.  The sibling lock on sub-path (b) is
    /// `upgraded_constraints_warning_survives_in_neither_list`; this arm had no
    /// equivalent.
    #[test]
    fn purpose_order_drops_build_indeterminacy_the_authoritative_check_falsified() {
        let stale = indeterminate_warning("BoltFlange#constraint[1]");
        let still_true = indeterminate_warning("BoltFlange#constraint[7]");
        let results = vec![
            entry(1, Satisfaction::Satisfied),
            entry(7, Satisfaction::Indeterminate),
        ];

        let merged =
            run_in_cmd_check_purpose_order(&[stale, still_true.clone()], &[], &[], &results);

        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![key(&still_true)],
            "constraint[1] was resolved definitely by the authoritative check, so \
             build's claim that it is indeterminate is false and must not print; \
             constraint[7] is still indeterminate and its explanation must stay"
        );
    }

    /// The same composition must not become a shredder: a realization-only
    /// diagnostic survives, and build's internal re-emission of one occurrence
    /// collapses to a single line.
    #[test]
    fn purpose_order_keeps_build_only_entries_and_collapses_the_internal_rerun() {
        let compile_error = Diagnostic::error(
            "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
        );
        let check_only = Diagnostic::warning("purpose constraint could not be evaluated");

        let merged = run_in_cmd_check_purpose_order(
            &[compile_error.clone(), compile_error.clone()],
            std::slice::from_ref(&check_only),
            &[],
            &[entry(1, Satisfaction::Satisfied)],
        );

        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![key(&compile_error), key(&check_only)],
            "build's duplicated entry collapses to one, the check-side entry \
             merges in behind it, and nothing is lost"
        );
    }

    /// Two DISTINCT callouts of the same characteristic are two findings, not
    /// one repeated line: `illegal_modifier_error` names the characteristic in
    /// the message and the location in a label, so their [`DiagKey`]s are equal
    /// and nothing local can tell them from a re-run.  Both must still print.
    ///
    /// This is the case a [`dedup_diagnostics`] over the realization's list
    /// silently collapsed to ONE line while the non-geometry arm printed two.
    /// End-to-end twin: `cli_gdt_legality.rs::
    /// check_purpose_gdt_two_illegal_callouts_on_geometry_module_print_twice`.
    #[test]
    fn purpose_order_keeps_two_same_text_callouts() {
        let first = gdt_illegal_modifier(120, 180);
        let second = gdt_illegal_modifier(240, 300);

        // The realization's internal `Engine::check` reports both; `cmd_check`
        // then re-runs the same pure pass and appends its output.
        let merged = run_in_cmd_check_purpose_order(
            &[first.clone(), second.clone()],
            &[],
            &[first.clone(), second.clone()],
            &[],
        );

        assert_eq!(
            merged.iter().map(key_with_span).collect::<Vec<_>>(),
            vec![key_with_span(&first), key_with_span(&second)],
            "one line per callout, from the single authoritative run — the \
             realization's redundant copy is withdrawn, never collapsed"
        );
    }

    /// The other half of the same requirement: the pass running TWICE must not
    /// double its output.  One callout, two runs, one line (esc-5748-7).
    #[test]
    fn purpose_order_does_not_double_the_gdt_pass() {
        let callout = gdt_illegal_modifier(120, 180);

        let merged = run_in_cmd_check_purpose_order(
            std::slice::from_ref(&callout),
            &[],
            std::slice::from_ref(&callout),
            &[],
        );

        assert_eq!(
            merged.iter().map(key_with_span).collect::<Vec<_>>(),
            vec![key_with_span(&callout)],
            "the realization's copy is withdrawn, so the fold cannot double it"
        );
    }

    /// An id-less geometric-conformance warning as
    /// `engine_constraints::gdt_indeterminate_diag` builds it: the code plus a
    /// fixed reason, and NO constraint id — so two Conforms constraints that
    /// are indeterminate for the same reason are byte-identical.
    fn idless_conformance_indeterminate(reason: &str) -> Diagnostic {
        Diagnostic::warning(format!("Conforms INDETERMINATE: {reason}"))
            .with_code(DiagnosticCode::ConstraintIndeterminate)
    }

    /// A `ConstraintViolated` line as `reify_constraints`' checker emits it once
    /// `engine_constraints::labeled_diagnostics` has rewritten the raw id to the
    /// constraint's DSL label — two DIFFERENT constraints sharing one label read
    /// identically.
    fn labeled_violation(label: &str) -> Diagnostic {
        Diagnostic::error(format!("constraint {label} violated"))
            .with_code(DiagnosticCode::ConstraintViolated)
    }

    /// The [`gdt_illegal_modifier`] problem without the span that saved it:
    /// `gdt_indeterminate_diag` anchors the location in a LABEL and names no
    /// constraint, so two indeterminate geometric `Conforms` sharing a reason
    /// collide on the [`DiagKey`] outright.  Sub-path (c) sends exactly this
    /// list through the self-dedup, so both callouts must survive it.
    ///
    /// Reachable, not hypothetical: `Engine::check` — which `realize_for_check`
    /// seeds the build diagnostics from — runs `measure_gdt_conformance` for
    /// EVERY geometric `Conforms`, and a stub/no-kernel build makes all of them
    /// indeterminate for the one same reason.
    #[test]
    fn purpose_order_keeps_two_idless_conformance_warnings() {
        let reason = "no geometry kernel available to measure the `actual` deviation";
        let first = idless_conformance_indeterminate(reason);
        let second = idless_conformance_indeterminate(reason);

        let merged = run_in_cmd_check_purpose_order(
            &[first.clone(), second.clone()],
            &[],
            &[],
            &[entry(1, Satisfaction::Satisfied)],
        );

        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![key(&first), key(&second)],
            "two geometric Conforms constraints that are both indeterminate for \
             the same reason are TWO findings; collapsing them drops one \
             callout's only explanation"
        );
    }

    /// The same requirement on the extras side of the merge: two constraints
    /// sharing a DSL label both read `constraint {label} violated`, and the
    /// non-geometry arm concatenates and prints both.  The `used_build` arm must
    /// not print fewer.
    #[test]
    fn purpose_order_keeps_two_same_label_violations() {
        let first = labeled_violation("wall_thick");
        let second = labeled_violation("wall_thick");

        // Both the realization's internal `Engine::check` and the CLI's own
        // `check_constraints_with_values` see both violations.
        let merged = run_in_cmd_check_purpose_order(
            &[first.clone(), second.clone()],
            &[first.clone(), second.clone()],
            &[],
            &[],
        );

        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![key(&first), key(&second)],
            "one line per violated constraint — the two lists agree on the \
             multiplicity, so the merge must reproduce it, not halve it"
        );
    }

    /// The primitive behind both: the self-dedup collapses UNCODED re-run
    /// duplication and leaves every coded entry's multiplicity alone.
    #[test]
    fn dedup_collapses_only_uncoded_entries() {
        let uncoded = Diagnostic::error(
            "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
        );
        let coded = idless_conformance_indeterminate("kernel unavailable");

        let deduped = dedup_diagnostics(&[
            uncoded.clone(),
            coded.clone(),
            uncoded.clone(),
            coded.clone(),
        ]);

        assert_eq!(
            deduped.iter().map(key).collect::<Vec<_>>(),
            vec![key(&uncoded), key(&coded), key(&coded)],
            "the uncoded front-end re-emission collapses; the coded per-callout \
             findings keep their multiplicity, in first-occurrence order"
        );
    }

    /// Task 5311 minted `DiagnosticCode::NoRegisteredComputeTrampoline` for a
    /// diagnostic that until then was UNCODED, which moves it from the
    /// collapsing population above into the exempt one — a user-visible
    /// composition change on `cmd_check`'s realization sub-path, pinned here so
    /// it is a decision rather than an accident.
    ///
    /// The two literals below are the same message: the shape the engine emits
    /// TODAY (coded) and the shape it emitted BEFORE (uncoded). Two `@optimized`
    /// call sites in one module produce two byte-identical copies, so the
    /// uncoded pair collapsed to one printed line and the coded pair does not.
    /// Confirmed end to end against a binary built from this branch:
    /// `reify check examples/anisotropic_bar.ri` prints the
    /// `solver::elastic_static` warning twice (two call sites), and
    /// `examples/fdm_bracket.ri` prints three lines over two distinct targets.
    /// That is what `dedup_diagnostics`' own rationale asks for — a coded
    /// entry's multiplicity is a per-callout fact, not re-run noise.
    #[test]
    fn dedup_exempts_the_coded_missing_trampoline_pair() {
        const MESSAGE: &str = "@optimized target \"solver::elastic_static\": \
                               no registered compute trampoline \
                               (falling back to body-inlining)";

        let coded =
            Diagnostic::warning(MESSAGE).with_code(DiagnosticCode::NoRegisteredComputeTrampoline);
        let as_it_was_before_task_5311 = Diagnostic::warning(MESSAGE);

        assert_eq!(
            dedup_diagnostics(&[coded.clone(), coded.clone()]).len(),
            2,
            "the CODED missing-trampoline diagnostic is exempt from collapsing, \
             so one line per @optimized call site reaches the user"
        );
        assert_eq!(
            dedup_diagnostics(&[
                as_it_was_before_task_5311.clone(),
                as_it_was_before_task_5311,
            ])
            .len(),
            1,
            "its pre-5311 UNCODED twin collapsed to a single line — this is the \
             baseline the assertion above is a change from, spelled out so the \
             change is legible without rebuilding the old binary"
        );
    }

    /// A realization diagnostic the appended run does NOT reproduce is a
    /// build-only entry and must survive the withdrawal (PRD D2's "every
    /// build()-only diagnostic appears at least once").  Keyed on the run, not
    /// on a GD&T code list, precisely so this stays true.
    #[test]
    fn withdrawal_keeps_realization_entries_the_rerun_does_not_reproduce() {
        let compile_error = Diagnostic::error(
            "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
        );
        let callout = gdt_illegal_modifier(120, 180);

        let kept = strip_diagnostics_reproduced_by(
            &[compile_error.clone(), callout.clone()],
            std::slice::from_ref(&callout),
        );

        assert_eq!(
            kept.iter().map(key).collect::<Vec<_>>(),
            vec![key(&compile_error)],
            "only the reproduced entry is withdrawn"
        );
        assert_eq!(
            strip_diagnostics_reproduced_by(std::slice::from_ref(&compile_error), &[])
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
            vec![key(&compile_error)],
            "an empty rerun withdraws nothing"
        );
    }

    /// The exit gate reads the MERGED set — what `report_eval_output` just
    /// showed the user — and NOT `result.diagnostics`, check()'s own list.
    ///
    /// `E_DFM_BUILD_VOLUME` (appended by `check_constraints_post_geometry`) is
    /// realization-only: it reaches the merged set (D2) and is absent from
    /// check()'s list, so it gates only because the gate reads the merged set.
    /// This is the helper-level half of that pin; the end-to-end half is
    /// `cli_check.rs::check_exits_nonzero_on_a_realization_only_build_volume_error`.
    ///
    /// The D2 precondition assertion makes this fail loudly, rather than pass
    /// vacuously, if the harvest error ever stops reaching the merged set.
    #[test]
    fn gate_reads_the_merged_set_not_checks_own_list() {
        let harvest_error = Diagnostic::error("E_DFM_BUILD_VOLUME: realized volume is zero");
        let harvest_warning = Diagnostic::warning("W_DFM_OVERHANG: 62° exceeds 45° limit");
        let check_diags = vec![Diagnostic::warning("unrelated")];

        let merged = merge_build_diagnostics(&check_diags, &[harvest_error]);
        assert!(
            merged.iter().any(|d| d.message.contains("E_DFM_BUILD_VOLUME")),
            "precondition (D2): the harvest error must still reach the user \
             through the merged, REPORTED set"
        );
        assert!(
            check_gating_error(&check_diags).is_none(),
            "check()'s own list carries no Error at all here — so a gate \
             reading it would let this module exit 0"
        );
        assert!(
            check_gating_error(&merged).is_some(),
            "the merged set must gate. If this ever stops holding, the harvest \
             error stopped reaching the reported set and the D2 precondition \
             above is the assertion to trust"
        );
        assert!(
            check_gating_error(&merge_build_diagnostics(&check_diags, &[harvest_warning]))
                .is_none(),
            "a W_DFM_ warning in the same harvest must NOT gate on either list \
             (C1: graceful degradation never invents a failure)"
        );
    }

    /// CHARACTERIZATION, not endorsement: an id-less `ConstraintIndeterminate`
    /// diagnostic survives the inward leg even when the entry it describes was
    /// upgraded.
    ///
    /// `engine_constraints::gdt_indeterminate_diag` emits `Conforms
    /// INDETERMINATE: {reason}` with the code but no constraint id, so
    /// [`indeterminacy_subject_in`]'s `constraint {id-or-label} indeterminate`
    /// grammar cannot read a subject out of it.  Surviving is CORRECT for a build-only entry (an
    /// unmatched diagnostic must never be dropped — that is what preserves D2's
    /// "every build()-only diagnostic appears at least once"), and is the reason
    /// `keeps_idless_indeterminate_diagnostics` locks it on the outward leg.
    ///
    /// On the inward leg the same rule means an upgraded geometric `Conforms`
    /// would keep check()'s own id-less warning, reproducing the
    /// stdout-OK/stderr-indeterminate contradiction this task removes elsewhere.
    /// No measured path reaches it — build's copy runs with cleared
    /// `realization_handles`, so it is never MORE definite on the GD&T axis, and
    /// the upgrade cannot fire.  The combination is pinned here rather than
    /// argued in prose; it is the same residual gap tracked as #6048, and
    /// whoever closes that ticket should decide this case with it.
    #[test]
    fn idless_indeterminate_warning_survives_an_upgrade() {
        let idless = Diagnostic::warning("Conforms INDETERMINATE: feature handle unrealized")
            .with_code(DiagnosticCode::ConstraintIndeterminate);
        let mut result = CheckResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Indeterminate)],
            diagnostics: vec![idless.clone()],
            resolved_params: Default::default(),
            structured_detail: Vec::new(),
        };
        let build = BuildResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Satisfied)],
            geometry_output: None,
            diagnostics: vec![],
            resolved_params: Default::default(),
        };

        let merged = run_in_cmd_check_order(&mut result, &build);

        assert_eq!(
            result.constraint_results[0].satisfaction,
            Satisfaction::Satisfied,
            "precondition: the entry must have upgraded, else the id-less \
             warning's survival proves nothing"
        );
        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![key(&idless)],
            "an id-less ConstraintIndeterminate carries no needle to match, so \
             the anchored retain leaves it alone. If this now fails because the \
             line was dropped, the matcher grew a span- or code-only arm — \
             update #6048 and this test together"
        );
    }

    /// The scenario the ordering exists for: `check()` left the constraint
    /// `Indeterminate`, `build()` resolves it definitely, and BOTH lists carry
    /// the matching `ConstraintIndeterminate` warning.
    ///
    /// The warning must appear ZERO times in the final list.  That fails if
    /// `merge_build_diagnostics` runs before either filter (build's copy is
    /// re-appended after the inward leg dropped check's), or if
    /// `drop_falsified_indeterminate_diagnostics` reads the PRE-upgrade
    /// verdicts (the entry still looks `Indeterminate`, so build's copy is not
    /// falsified and survives).
    #[test]
    fn upgraded_constraints_warning_survives_in_neither_list() {
        let needle = "BoltFlange#constraint[1]";
        let mut result = CheckResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Indeterminate)],
            diagnostics: vec![indeterminate_warning(needle)],
            resolved_params: Default::default(),
            structured_detail: Vec::new(),
        };
        let build = BuildResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Satisfied)],
            geometry_output: None,
            diagnostics: vec![indeterminate_warning(needle)],
            resolved_params: Default::default(),
        };

        let merged = run_in_cmd_check_order(&mut result, &build);

        assert_eq!(
            result.constraint_results[0].satisfaction,
            Satisfaction::Satisfied,
            "precondition: the inward leg must have upgraded the entry, else \
             this test is vacuous and proves nothing about ordering"
        );
        let surviving = merged
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::ConstraintIndeterminate))
            .count();
        assert_eq!(
            surviving,
            0,
            "the upgraded constraint's now-false indeterminacy warning must \
             survive in NEITHER list; got: {:?}",
            merged.iter().map(key).collect::<Vec<_>>()
        );
    }

    /// The same composition must not become a diagnostic shredder: a build
    /// entry about a DIFFERENT constraint, and one carrying no verdict claim at
    /// all, both still reach the user (PRD D2's "every build()-only diagnostic
    /// appears at least once").
    #[test]
    fn composition_still_surfaces_unrelated_build_diagnostics() {
        let compile_error = Diagnostic::error(
            "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
        );
        let mut result = CheckResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Indeterminate)],
            diagnostics: vec![],
            resolved_params: Default::default(),
            structured_detail: Vec::new(),
        };
        let build = BuildResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Satisfied)],
            geometry_output: None,
            diagnostics: vec![
                indeterminate_warning("BoltFlange#constraint[1]"),
                indeterminate_warning("BoltFlange#constraint[7]"),
                compile_error.clone(),
            ],
            resolved_params: Default::default(),
        };

        let merged = run_in_cmd_check_order(&mut result, &build);

        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![
                key(&indeterminate_warning("BoltFlange#constraint[7]")),
                key(&compile_error),
            ],
            "only the UPGRADED constraint's warning is falsified; the untouched \
             constraint[7] warning and the realization-only compile error must \
             both survive"
        );
    }

    /// `dedup_diagnostics` is what `merge_build_diagnostics(&[], …)` used to
    /// spell at sub-path (c), so on the population it still collapses — UNCODED
    /// entries — it must stay behaviour-preserving: same collapse, same
    /// first-occurrence order.  The coded population deliberately diverges; see
    /// `dedup_collapses_only_uncoded_entries`.
    #[test]
    fn dedup_matches_the_empty_seed_merge_it_replaced() {
        let diags = vec![
            Diagnostic::error("missing or non-Length argument 'ox' for mirror"),
            Diagnostic::warning("unrelated"),
            Diagnostic::error("missing or non-Length argument 'ox' for mirror"),
        ];

        let deduped = dedup_diagnostics(&diags);

        assert_eq!(
            deduped.iter().map(key).collect::<Vec<_>>(),
            merge_build_diagnostics(&[], &diags)
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            deduped.len(),
            2,
            "the duplicated 'ox' error collapses to one"
        );
    }

    /// `merge_build_diagnostics` is deliberately NOT
    /// `dedup_diagnostics(&[seed, extras].concat())`: its seed is `check()`'s
    /// authoritative list and is reproduced VERBATIM, duplicates included.
    #[test]
    fn merge_does_not_dedup_within_its_authoritative_seed() {
        let repeated = Diagnostic::warning("check said this twice");
        let check = vec![repeated.clone(), repeated.clone()];

        let merged = merge_build_diagnostics(&check, &[]);

        assert_eq!(
            merged.len(),
            2,
            "check()'s list is verbatim; collapsing it here would silently \
             rewrite the authoritative output"
        );
        assert_eq!(
            dedup_diagnostics(&check).len(),
            1,
            "the primitive does collapse"
        );
    }

    /// The empty-`build_diags` short-circuit is the lightweight arm's common
    /// case (`build_result == None` → `unwrap_or(&[])`) and must stay a no-op
    /// no matter how many definite verdicts the authoritative list carries.
    #[test]
    fn empty_build_diags_short_circuits_regardless_of_verdicts() {
        let results = vec![
            entry(0, Satisfaction::Satisfied),
            entry(1, Satisfaction::Violated),
        ];

        assert!(drop_falsified_indeterminate_diagnostics(&[], &results).is_empty());
    }

    /// CHARACTERIZATION, not endorsement: pins the mirror case that
    /// [`drop_falsified_indeterminate_diagnostics`]' `ConstraintIndeterminate`
    /// scoping deliberately leaves open (#6048).
    ///
    /// `check()` says `Satisfied`; `build()`'s copy says `Violated` and emits a
    /// matching `ConstraintViolated` error.  The verdict axis is already locked
    /// by `never_regresses_a_definite_check_verdict` — this locks the
    /// DIAGNOSTIC axis the reviewer found unasserted, so the helper's doc claim
    /// ("a `ConstraintViolated` line naming the same constraint survives
    /// untouched") is a test rather than prose.
    ///
    /// The surviving error IS the stdout/stderr self-contradiction #6048
    /// describes.  It is pinned rather than fixed because dropping a violation
    /// error is a heavier call than dropping an indeterminacy warning.
    ///
    /// The gap is exit-relevant: `check_gating_error` reads this merged set,
    /// so a surviving stale `ConstraintViolated` Error would move `check`'s
    /// EXIT CODE against the `Satisfied` verdict on its own stdout.
    /// `CHECK_ERROR_EXIT_ALLOWLIST` entry #4 (`Code(ConstraintViolated)`,
    /// disposition `FixPath`) holds that off — this test and that entry are
    /// two views of one gap, and `FixPath` says the repair belongs in
    /// `drop_falsified_indeterminate_diagnostics`, not in the gate.
    ///
    /// When #6048 lands, THIS TEST MUST FAIL — that is the point; flip it to
    /// assert the drop, and retire allowlist entry #4 in the same change.
    #[test]
    fn mirror_case_build_side_violation_currently_survives() {
        let needle = "BoltFlange#constraint[1]";
        let violation = Diagnostic::error(format!(
            "constraint {} violated: clearance 0.4mm below minimum 0.5mm",
            needle
        ))
        .with_code(DiagnosticCode::ConstraintViolated);
        let mut result = CheckResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Satisfied)],
            diagnostics: vec![],
            resolved_params: Default::default(),
            structured_detail: Vec::new(),
        };
        let build = BuildResult {
            values: Default::default(),
            constraint_results: vec![entry(1, Satisfaction::Violated)],
            geometry_output: None,
            diagnostics: vec![violation.clone()],
            resolved_params: Default::default(),
        };

        let merged = run_in_cmd_check_order(&mut result, &build);

        assert_eq!(
            result.constraint_results[0].satisfaction,
            Satisfaction::Satisfied,
            "precondition: the upgrade-only rule must have KEPT check()'s \
             definite verdict, else this is not the mirror scenario"
        );
        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            vec![key(&violation)],
            "known gap #6048: stdout will say OK for this constraint while \
             stderr carries build's violation error. If this assertion now \
             fails because the line was dropped, #6048 is fixed — update this \
             test and the doc comment on \
             drop_falsified_indeterminate_diagnostics together"
        );
    }
