use super::{
    drop_falsified_indeterminate_diagnostics, indeterminacy_anchor, indeterminacy_subject,
    indeterminacy_subject_in,
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

/// (0) The forward and inverse readings of the indeterminacy grammar are
/// ONE definition: whatever `indeterminacy_anchor` writes,
/// `indeterminacy_subject_in` must read back — for a labeled entry and an
/// unlabeled one alike.
///
/// This is the lock that lets both legs match by extracting the subject and
/// hashing it (O(diagnostics + constraints)) instead of scanning every
/// message for every constraint's anchored needle.  If the checker's
/// wording ever changes, this test fails BEFORE the silent
/// nothing-ever-matches degradation it would otherwise cause.
#[test]
fn indeterminacy_grammar_round_trips() {
    for e in [
        entry("Bracket", 12, Satisfaction::Indeterminate),
        labeled("Bracket", 3, "wall_thick", Satisfaction::Indeterminate),
    ] {
        let subject = indeterminacy_subject(&e).into_owned();
        let message = format!(
            "{}: undefined inputs: Bracket.thickness",
            indeterminacy_anchor(&subject)
        );
        assert_eq!(
            indeterminacy_subject_in(&message),
            Some(subject.as_str()),
            "the inverse must read back exactly what the forward direction \
             wrote, for {message}"
        );
    }
    assert_eq!(
        indeterminacy_subject_in("Conforms INDETERMINATE: no geometry kernel"),
        None,
        "a message that does not follow the grammar yields no subject, so \
         nothing can falsify it — see keeps_idless_indeterminate_diagnostics"
    );
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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "absence from the authoritative list must never drop a diagnostic — \
         only a positive definite verdict for that same constraint may"
    );
}

/// (5) The filter falsifies exactly ONE claim: indeterminacy. A diagnostic
/// carrying the same needle under a different code (e.g. the
/// `ConstraintViolated` summary line) is untouched even when a definite
/// verdict exists for that id.
#[test]
fn never_drops_a_diagnostic_that_is_not_coded_constraint_indeterminate() {
    let violated = Diagnostic::error("constraint Bracket#constraint[2] indeterminate-ish note")
        .with_code(DiagnosticCode::ConstraintViolated);
    let uncoded = Diagnostic::warning(
        "constraint Bracket#constraint[2] indeterminate: hand-rolled uncoded line",
    );
    let build = vec![violated, uncoded];
    let results = vec![entry("Bracket", 2, Satisfaction::Satisfied)];

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&build),
        "only ConstraintIndeterminate-coded entries are subject to the \
         filter; every other code (and the uncoded case) must survive"
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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &[]);

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

    let kept = drop_falsified_indeterminate_diagnostics(&build, &results);

    assert_eq!(
        keys(&kept),
        keys(&[first, second, third]),
        "the filter must retain in place: surviving entries keep their \
         original relative order"
    );
}
