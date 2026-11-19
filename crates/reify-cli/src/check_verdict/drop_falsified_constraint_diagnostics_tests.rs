use super::{
    ConstraintClaim, constraint_claim_in, constraint_subject, drop_falsified_constraint_diagnostics,
};
use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, Severity};
use reify_eval::ConstraintCheckEntry;
use reify_ir::Satisfaction;

/// Renders a diagnostic down to the fields the filter reasons about, so
/// assertions read as data (`reify_core::Diagnostic` derives only
/// `Debug, Clone`, no `PartialEq`).
fn key(d: &Diagnostic) -> (Severity, Option<DiagnosticCode>, String) {
    (d.severity, d.code, d.message.clone())
}

fn keys(ds: &[Diagnostic]) -> Vec<(Severity, Option<DiagnosticCode>, String)> {
    ds.iter().map(key).collect()
}

fn entry(entity: &str, index: u32, satisfaction: Satisfaction) -> ConstraintCheckEntry {
    ConstraintCheckEntry {
        id: ConstraintNodeId::new(entity, index),
        label: None,
        satisfaction,
        indeterminate_reason: None,
    }
}

fn labeled(
    entity: &str,
    index: u32,
    label: &str,
    satisfaction: Satisfaction,
) -> ConstraintCheckEntry {
    ConstraintCheckEntry {
        id: ConstraintNodeId::new(entity, index),
        label: Some(label.to_string()),
        satisfaction,
        indeterminate_reason: None,
    }
}

/// (0) The forward and inverse readings of the claim grammar are ONE
/// definition: whatever `ConstraintClaim::anchor` writes, `constraint_claim_in`
/// must read back — for both claim words, for a labeled entry and an unlabeled
/// one alike.
///
/// This is the lock that lets both legs match by extracting the subject and
/// hashing it (O(diagnostics + constraints)) instead of scanning every
/// message for every constraint's anchored needle.  If the checker's
/// wording ever changes, this test fails BEFORE the silent
/// nothing-ever-matches degradation it would otherwise cause.
#[test]
fn constraint_claim_grammar_round_trips() {
    for claim in ConstraintClaim::ALL {
        for e in [
            entry("Bracket", 12, Satisfaction::Indeterminate),
            labeled("Bracket", 3, "wall_thick", Satisfaction::Indeterminate),
        ] {
            let subject = constraint_subject(&e).into_owned();
            let d = Diagnostic::warning(format!(
                "{}: undefined inputs: Bracket.thickness",
                claim.anchor(&subject)
            ))
            .with_code(claim.code());
            assert_eq!(
                constraint_claim_in(&d),
                Some((claim, subject.as_str())),
                "the inverse must read back exactly what the forward direction \
                 wrote, for {:?}",
                d.message
            );
        }
    }
    for message in [
        "Conforms INDETERMINATE: no geometry kernel",
        "Conforms VIOLATED: measured deviation 0.3000 mm exceeds the 0.1000 mm tolerance zone",
    ] {
        for claim in ConstraintClaim::ALL {
            assert_eq!(
                constraint_claim_in(&Diagnostic::warning(message).with_code(claim.code())),
                None,
                "a message that does not follow the grammar yields no subject, \
                 so nothing can falsify it: {message:?}"
            );
        }
    }
}

/// Builds the exact shape `reify_constraints`' checker emits:
/// `constraint {needle} indeterminate: undefined inputs: {cells}`, coded
/// `ConstraintIndeterminate`, at `Warning` severity.
fn indeterminate_warning(needle: &str, undefined: &str) -> Diagnostic {
    Diagnostic::warning(format!(
        "constraint {} indeterminate: undefined inputs: {}",
        needle, undefined
    ))
    .with_code(DiagnosticCode::ConstraintIndeterminate)
}

/// (1) The headline case: `check()` resolved the constraint to `Satisfied`,
/// so `build()`'s surviving "indeterminate" claim about it is FALSE and
/// must not reach stderr.
///
/// This is the reviewer-measured self-contradiction verbatim: stdout says
/// `OK SphereCheck#constraint[0]` + `All constraints satisfied.` while
/// stderr says that same constraint is indeterminate.
#[test]
fn drops_indeterminate_claim_falsified_by_a_satisfied_verdict() {
    let build = vec![indeterminate_warning(
        "SphereCheck#constraint[0]",
        "SphereCheck.subject",
    )];
    let results = vec![entry("SphereCheck", 0, Satisfaction::Satisfied)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert!(
        kept.is_empty(),
        "a ConstraintIndeterminate claim about a constraint the authoritative \
         list reports Satisfied is false and must be dropped, got {:?}",
        keys(&kept)
    );
}

/// (2) `Violated` falsifies the indeterminacy claim exactly as `Satisfied`
/// does — the property that matters is DEFINITENESS, not the polarity of
/// the verdict.
#[test]
fn drops_indeterminate_claim_falsified_by_a_violated_verdict() {
    let build = vec![indeterminate_warning("Bracket#constraint[2]", "Bracket.t")];
    let results = vec![entry("Bracket", 2, Satisfaction::Violated)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert!(
        kept.is_empty(),
        "a Violated verdict falsifies an indeterminacy claim just as a \
         Satisfied one does, got {:?}",
        keys(&kept)
    );
}

/// (3) When the authoritative verdict is ALSO `Indeterminate`, the
/// diagnostic is still TRUE — dropping it would delete the user's only
/// explanation of why the constraint could not be decided.
#[test]
fn keeps_indeterminate_claim_when_the_verdict_is_also_indeterminate() {
    let build = vec![indeterminate_warning(
        "Bracket#constraint[2]",
        "Bracket.tolerance",
    )];
    let results = vec![entry("Bracket", 2, Satisfaction::Indeterminate)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "an indeterminacy claim matching an Indeterminate verdict is still \
         true and must survive"
    );
}

/// (4) Drop only on POSITIVE falsification. An id the authoritative list
/// never mentions is not evidence of anything, so the diagnostic stays —
/// this is what preserves PRD D2's "every build()-only diagnostic appears
/// at least once".
#[test]
fn keeps_indeterminate_claim_when_no_entry_matches_the_id() {
    let build = vec![indeterminate_warning("Ghost#constraint[0]", "Ghost.x")];
    let results = vec![entry("Bracket", 0, Satisfaction::Satisfied)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "absence from the authoritative list must never drop a diagnostic — \
         only a positive definite verdict for that same constraint may"
    );
}

/// (5) A claim is read off its CODE and its grammar together.  A line whose
/// code names one claim but whose text does not follow that claim's grammar,
/// and an uncoded line that does follow a grammar, are untouched even when a
/// definite verdict exists for that id.
#[test]
fn never_drops_a_diagnostic_off_the_coded_claim_grammar() {
    let violated = Diagnostic::error("constraint Bracket#constraint[2] indeterminate-ish note")
        .with_code(DiagnosticCode::ConstraintViolated);
    let uncoded = Diagnostic::warning(
        "constraint Bracket#constraint[2] indeterminate: hand-rolled uncoded line",
    );
    let build = vec![violated, uncoded];
    let results = vec![entry("Bracket", 2, Satisfaction::Satisfied)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "only a claim-coded entry following that claim's grammar is subject \
         to the filter; everything else (and the uncoded case) must survive"
    );
}

/// (6) Label-preferring needle. When a constraint carries a label the
/// checker embeds the LABEL in the message, not the raw id
/// (`engine_constraints::labeled_diagnostics`' rewrite), so the
/// matcher must prefer `label` exactly as `merge_post_build_verdicts` does.
#[test]
fn matches_on_the_label_when_the_entry_carries_one() {
    let build = vec![indeterminate_warning("wall_thick", "Bracket.t")];
    let results = vec![labeled("Bracket", 3, "wall_thick", Satisfaction::Satisfied)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert!(
        kept.is_empty(),
        "a labeled constraint's diagnostic names the LABEL, so the matcher \
         must use it in preference to the raw id, got {:?}",
        keys(&kept)
    );
}

/// (7) PREFIX-COLLISION SAFETY — the reason the matcher is anchored.
///
/// `Foo#constraint[1]` is a strict prefix of `Foo#constraint[10]`, so a
/// bare `message.contains(needle)` would let a definite verdict for
/// `[1]` wrongly delete `[10]`'s still-true warning. Anchoring on the full
/// `constraint {needle} indeterminate` span makes that impossible:
/// `constraint Foo#constraint[1] indeterminate` is not a substring of
/// `constraint Foo#constraint[10] indeterminate`.
#[test]
fn anchored_matcher_survives_an_id_prefix_collision() {
    let build = vec![indeterminate_warning("Foo#constraint[10]", "Foo.x")];
    let results = vec![
        entry("Foo", 1, Satisfaction::Satisfied),
        entry("Foo", 10, Satisfaction::Indeterminate),
    ];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "a definite verdict for Foo#constraint[1] must NOT falsify the \
         warning about Foo#constraint[10] — the matcher must be anchored, \
         not a bare contains()"
    );
}

/// (8) Not every `ConstraintIndeterminate` diagnostic names a constraint
/// id: `Conforms INDETERMINATE: <reason>` (`engine_constraints::gdt_indeterminate_diag`)
/// carries the code with no id at all. Nothing can falsify it, so it always
/// survives — the filter must not fall back to a broad code-only drop.
#[test]
fn keeps_idless_indeterminate_diagnostics() {
    let build = vec![
        Diagnostic::warning("Conforms INDETERMINATE: subject has no realized geometry")
            .with_code(DiagnosticCode::ConstraintIndeterminate),
    ];
    let results = vec![entry("SphereCheck", 0, Satisfaction::Satisfied)];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "a ConstraintIndeterminate diagnostic naming no constraint id \
         cannot be falsified by any verdict and must survive"
    );
}

/// (9) An empty authoritative list falsifies nothing → `build_diags`
/// verbatim. This is the C2 no-op guarantee for the arm where no
/// constraints exist at all.
#[test]
fn empty_constraint_results_returns_build_diags_verbatim() {
    let build = vec![
        indeterminate_warning("A#constraint[0]", "A.x"),
        Diagnostic::error("failed to compile geometry operation"),
    ];

    let kept = drop_falsified_constraint_diagnostics(&build, &[]);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "with no verdicts to falsify anything, the build list must pass \
         through untouched"
    );
}

/// (10) Surviving entries keep their relative order — the filter is a
/// `retain`, never a reorder. `report_eval_output` prints the list in
/// order, so chronology is user-visible.
#[test]
fn preserves_the_order_of_surviving_entries() {
    let first = Diagnostic::error("failed to compile geometry operation: 'ox' for mirror");
    let dropped = indeterminate_warning("A#constraint[0]", "A.x");
    let second = indeterminate_warning("A#constraint[1]", "A.y");
    let third = Diagnostic::warning("W_DFM_OVERHANG: face dips past the overhang limit");
    let build = vec![first.clone(), dropped, second.clone(), third.clone()];
    let results = vec![
        entry("A", 0, Satisfaction::Satisfied),
        entry("A", 1, Satisfaction::Indeterminate),
    ];

    let kept = drop_falsified_constraint_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&[first, second, third]),
        "the filter must retain in place: surviving entries keep their \
         original relative order"
    );
}

// -------------------------------------------------------------------
// The VIOLATION claim (#6048 shape a)
// -------------------------------------------------------------------

/// The exact shape `reify_constraints`' checker emits for a false predicate:
/// `constraint {subject} violated`, coded `ConstraintViolated`, at `Error`
/// severity.
fn violated(subject: &str) -> Diagnostic {
    Diagnostic::error(format!("constraint {subject} violated"))
        .with_code(DiagnosticCode::ConstraintViolated)
}

/// A build-side violation claim is false once the authoritative list says
/// `Satisfied`, in both the raw-id and the label-rewritten form.
#[test]
fn drops_violation_claim_when_every_holder_is_satisfied() {
    let by_id = drop_falsified_constraint_diagnostics(
        &[violated("Bracket#constraint[12]")],
        &[entry("Bracket", 12, Satisfaction::Satisfied)],
    );
    assert!(
        by_id.is_empty(),
        "a violation claim about a Satisfied constraint is false, got {:?}",
        keys(&by_id)
    );

    let by_label = drop_falsified_constraint_diagnostics(
        &[violated("wall_thick")],
        &[labeled("Bracket", 3, "wall_thick", Satisfaction::Satisfied)],
    );
    assert!(
        by_label.is_empty(),
        "a labeled constraint's violation line names the LABEL, got {:?}",
        keys(&by_label)
    );
}

/// Only `Satisfied` falsifies a violation.  A `Violated` verdict confirms the
/// claim (the true line after an Indeterminate→Violated upgrade, pinned end to
/// end by `cli_check.rs::check_geometry_module_upgrades_indeterminate_to_violated_and_exits_failure`),
/// and an undecided verdict is not evidence the violation is stale.
#[test]
fn keeps_violation_claim_when_the_verdict_is_violated_or_indeterminate() {
    let build = vec![violated("Bracket#constraint[2]")];
    for verdict in [Satisfaction::Violated, Satisfaction::Indeterminate] {
        let kept = drop_falsified_constraint_diagnostics(&build, &[entry("Bracket", 2, verdict)]);
        assert_eq!(
            keys(&kept),
            keys(&build),
            "a {verdict:?} verdict does not falsify a violation claim"
        );
    }
}

/// Compiler labels are unique per entity, not per module, so one subject can
/// name several constraints.  The claim is falsified only when EVERY holder
/// contradicts it; a same-label `Violated` holder elsewhere keeps its true
/// line.
#[test]
fn keeps_violation_claim_when_only_some_same_subject_holders_are_satisfied() {
    let build = vec![violated("wall_thick")];
    let kept = drop_falsified_constraint_diagnostics(
        &build,
        &[
            labeled("Bracket", 3, "wall_thick", Satisfaction::Satisfied),
            labeled("Lid", 0, "wall_thick", Satisfaction::Violated),
        ],
    );
    assert_eq!(
        keys(&kept),
        keys(&build),
        "one Satisfied holder must not retract a claim another holder of the \
         same label still makes true"
    );
}

/// The SAME every-holder rule on the indeterminate word: one definite holder
/// does not retract a claim a same-label holder still makes true.
#[test]
fn every_holder_rule_also_applies_to_the_indeterminate_word() {
    let build = vec![indeterminate_warning("wall_thick", "Lid.t")];
    let kept = drop_falsified_constraint_diagnostics(
        &build,
        &[
            labeled("Bracket", 3, "wall_thick", Satisfaction::Satisfied),
            labeled("Lid", 0, "wall_thick", Satisfaction::Indeterminate),
        ],
    );
    assert_eq!(
        keys(&kept),
        keys(&build),
        "an indeterminacy claim survives while any same-label holder is still \
         Indeterminate"
    );
}

/// The GD&T `Conforms VIOLATED` line names no constraint, so no verdict can
/// falsify it, by design.
#[test]
fn keeps_idless_conformance_violation() {
    let build = vec![
        Diagnostic::error(
            "Conforms VIOLATED: measured deviation 0.3000 mm exceeds the 0.1000 mm \
             tolerance zone",
        )
        .with_code(DiagnosticCode::ConstraintViolated),
    ];
    let kept = drop_falsified_constraint_diagnostics(
        &build,
        &[entry("Bracket", 2, Satisfaction::Satisfied)],
    );
    assert_eq!(
        keys(&kept),
        keys(&build),
        "an id-less violation carries no subject and must survive"
    );
}

/// `evaluated to non-boolean value` shares the code but not the violation
/// grammar, so it is never read as a violation claim, even when its subject is
/// `Satisfied`.
#[test]
fn keeps_non_boolean_value_error() {
    let build = vec![
        Diagnostic::error("constraint Bracket#constraint[2] evaluated to non-boolean value")
            .with_code(DiagnosticCode::ConstraintViolated),
    ];
    let kept = drop_falsified_constraint_diagnostics(
        &build,
        &[entry("Bracket", 2, Satisfaction::Satisfied)],
    );
    assert_eq!(
        keys(&kept),
        keys(&build),
        "a non-boolean constraint error has no ` violated` infix and must survive"
    );
}
