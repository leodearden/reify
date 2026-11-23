//! The verdict layer: reconciles the check and build passes' constraint results and
//! diagnostics into the set a command reports, and decides which of them fail the command.

use reify_core::{DiagnosticCode, DiagnosticIdentity, Severity};

/// The constraint-claim message grammar, as literals:
/// `constraint {label-or-id} indeterminate: {reason}` and
/// `constraint {label-or-id} violated`.
///
/// That is exactly what `reify_constraints`' checker emits
/// (reify-constraints/src/lib.rs), with the raw id rewritten to the constraint's
/// label by `engine_constraints::labeled_diagnostics` when it carries one.
/// Every reading of the grammar below — forward and inverse — is built from
/// these literals, so they cannot drift apart; the round trip is pinned by
/// `constraint_claim_grammar_round_trips`.
const CONSTRAINT_CLAIM_PREFIX: &str = "constraint ";
const INDETERMINACY_INFIX: &str = " indeterminate";
const VIOLATION_INFIX: &str = " violated";

/// What a coded constraint diagnostic claims about its subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstraintClaim {
    Indeterminate,
    Violated,
}

impl ConstraintClaim {
    const ALL: [ConstraintClaim; 2] = [ConstraintClaim::Indeterminate, ConstraintClaim::Violated];

    fn code(self) -> DiagnosticCode {
        match self {
            ConstraintClaim::Indeterminate => DiagnosticCode::ConstraintIndeterminate,
            ConstraintClaim::Violated => DiagnosticCode::ConstraintViolated,
        }
    }

    fn infix(self) -> &'static str {
        match self {
            ConstraintClaim::Indeterminate => INDETERMINACY_INFIX,
            ConstraintClaim::Violated => VIOLATION_INFIX,
        }
    }

    /// Whether one authoritative `verdict` contradicts the claim.  Any definite
    /// verdict settles an indeterminacy; only `Satisfied` retracts a violation,
    /// because dropping a violation Error is the heavier call.
    fn is_falsified_by(self, verdict: reify_ir::Satisfaction) -> bool {
        match self {
            ConstraintClaim::Indeterminate => verdict != reify_ir::Satisfaction::Indeterminate,
            ConstraintClaim::Violated => verdict == reify_ir::Satisfaction::Satisfied,
        }
    }

    /// Whether the verdicts of every constraint sharing the claim's subject
    /// falsify it: at least one holder, and each one contradicts the claim.
    /// Compiler labels are unique per entity, not per module, so one subject
    /// can name several constraints.
    fn is_falsified_by_every(self, verdicts: &[reify_ir::Satisfaction]) -> bool {
        !verdicts.is_empty() && verdicts.iter().all(|&v| self.is_falsified_by(v))
    }

    /// The forward reading of the grammar, kept next to the inverse so the two
    /// stay legible as one definition.  Test-only: production code goes the
    /// other way (extract once, then hash-lookup), which is the whole point of
    /// the shape.
    #[cfg(test)]
    fn anchor(self, subject: &str) -> String {
        format!("{CONSTRAINT_CLAIM_PREFIX}{subject}{}", self.infix())
    }
}

/// The SUBJECT a constraint claim about `entry` names: its label when it has
/// one (preferred exactly as `engine_constraints::labeled_diagnostics` does),
/// else its raw id.
///
/// Borrowed for a labeled entry, so the identity of a constraint costs no
/// allocation on that path.  Both falsification legs go through this one
/// definition, so they cannot drift into computing different identities
/// (esc-5748-4).
fn constraint_subject(entry: &reify_eval::ConstraintCheckEntry) -> std::borrow::Cow<'_, str> {
    match entry.label.as_deref() {
        Some(label) => std::borrow::Cow::Borrowed(label),
        None => std::borrow::Cow::Owned(entry.id.to_string()),
    }
}

/// The INVERSE reading of the grammar: the claim `d` makes and the subject it
/// makes it about, or `None` when `d` carries no claim code or its message does
/// not follow that claim's grammar.
///
/// Keyed on the CODE first, so a line is read only under the one claim its code
/// names.  Extracting the subject and hashing it is what keeps both
/// falsification legs O(diagnostics + constraints).  The match is EXACT rather
/// than a substring test: `Foo#constraint[1]` cannot match
/// `Foo#constraint[10]`'s still-true line, and a one-character label cannot
/// match nearly every message (`anchored_matcher_survives_an_id_prefix_collision`,
/// `anchored_matcher_survives_a_short_label_collision`).
///
/// A message off the grammar yields `None` and is therefore never dropped: the
/// id-less GD&T `Conforms INDETERMINATE: …` / `Conforms VIOLATED: …`, the
/// `evaluated to non-boolean value` error, and any third-party
/// `ConstraintChecker`'s own wording.  That is the safe direction: a wrongly
/// dropped line is unrecoverable output loss, a wrongly kept one is merely
/// redundant.
fn constraint_claim_in(d: &reify_core::Diagnostic) -> Option<(ConstraintClaim, &str)> {
    let claim = ConstraintClaim::ALL
        .into_iter()
        .find(|claim| d.code == Some(claim.code()))?;
    let rest = d.message.strip_prefix(CONSTRAINT_CLAIM_PREFIX)?;
    let end = rest.find(claim.infix())?;
    Some((claim, &rest[..end]))
}

/// The verdicts of the authoritative entries holding each subject, keyed by
/// subject, for exactly the `subjects` given.
type HolderVerdicts<'s> = std::collections::HashMap<&'s str, Vec<reify_ir::Satisfaction>>;

/// Collect the [`HolderVerdicts`] of `subjects` in ONE walk of
/// `constraint_results`, hashing each entry's [`constraint_subject`]:
/// O(subjects + constraints), never their product.  No subjects, no walk.
fn holder_verdicts<'s>(
    subjects: impl IntoIterator<Item = &'s str>,
    constraint_results: &[reify_eval::ConstraintCheckEntry],
) -> HolderVerdicts<'s> {
    let mut holders: HolderVerdicts<'s> = subjects
        .into_iter()
        .map(|subject| (subject, Vec::new()))
        .collect();
    if holders.is_empty() {
        return holders;
    }
    for entry in constraint_results {
        if let Some(verdicts) = holders.get_mut(constraint_subject(entry).as_ref()) {
            verdicts.push(entry.satisfaction);
        }
    }
    holders
}

/// The claim `d` makes, when its subject's `holders` positively falsify it
/// ([`ConstraintClaim::is_falsified_by_every`]); `None` for a claim still
/// standing, a subject absent from `holders`, or a line off the grammar.
///
/// The ONE falsification decision both legs make, so they read the same
/// grammar and apply the same rule.
fn falsified_claim(
    d: &reify_core::Diagnostic,
    holders: &HolderVerdicts<'_>,
) -> Option<ConstraintClaim> {
    let (claim, subject) = constraint_claim_in(d)?;
    let verdicts = holders.get(subject)?;
    claim.is_falsified_by_every(verdicts).then_some(claim)
}

/// Adopt post-realization constraint verdicts from a captured [`BuildResult`]
/// onto the authoritative [`CheckResult`], for entries `check()` left
/// `Indeterminate`.
///
/// Routing a geometry-bearing module through the realization (D1) resolves
/// geometry-query cells (`centroid`, `moment_of_inertia`, …) into the
/// realization's OWN value map only; `Engine::check()` opens with a fresh
/// `self.eval(module)` and would report every constraint reading one as
/// `Indeterminate` again.  This merge is what makes D1 observable on the verdict
/// axis — the CLI-side mirror of `engine_build::build_with_geometry_output`'s
/// task-4229 post-realization re-check, which `Engine::check()` has no
/// equivalent of.
///
/// # Contract
///
/// * **Upgrade-only.**  Only `Indeterminate` entries are touched; a definite
///   `check()` verdict is never regressed and an `Indeterminate` build verdict
///   never overwrites anything.  That is what makes build()'s copy safe to
///   consult even though it is computed before `tessellate_realizations` and
///   with cleared `realization_handles`: on the tessellate/GD&T axis it is never
///   MORE definite than `check()`, so only the geometry-query axis moves.
/// * **Entries are matched by `id`**, never by position.
/// * **An upgraded subject's now-false indeterminacy claim is dropped**, by
///   the [`falsified_claim`] decision the outward leg shares: only once every
///   entry holding that subject is definite after the upgrade, so a same-label
///   holder still `Indeterminate` keeps its true line.
/// * **Diagnostics are otherwise untouched**; [`merge_build_diagnostics`] owns
///   appending build()-only entries.
///
/// A `None` build result (the lightweight arm, or a kernel-backed module that
/// took only the `tessellate_realizations` side effect) is a total no-op, so
/// every pre-5748 input stays byte-identical.
pub(crate) fn merge_post_build_verdicts(
    result: &mut reify_eval::CheckResult,
    build_result: Option<&reify_eval::BuildResult>,
) {
    let Some(build_result) = build_result else {
        return;
    };
    // Indexed once, not re-scanned per entry: both lists are O(constraints) and
    // this pass is now on EVERY geometry-bearing `reify check`, so the naive
    // nested `find` was quadratic in the constraint count of any real assembly.
    // `or_insert`, not `collect`: a duplicate id must resolve to the FIRST
    // entry, exactly as the `find` this replaced did.
    let mut build_verdicts: std::collections::HashMap<
        &reify_core::ConstraintNodeId,
        reify_ir::Satisfaction,
    > = std::collections::HashMap::with_capacity(build_result.constraint_results.len());
    for r in &build_result.constraint_results {
        build_verdicts.entry(&r.id).or_insert(r.satisfaction);
    }
    // Subjects of the entries that upgraded — the only ones whose claims this
    // pass can have made false.  Collected during the walk and read after it
    // (one `&mut result` borrow at a time).
    let mut upgraded: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in result.constraint_results.iter_mut() {
        if entry.satisfaction != reify_ir::Satisfaction::Indeterminate {
            continue;
        }
        let Some(new_sat) = build_verdicts.get(&entry.id).copied() else {
            continue;
        };
        if new_sat == reify_ir::Satisfaction::Indeterminate {
            continue;
        }
        entry.set_verdict(new_sat, None);
        upgraded.insert(constraint_subject(entry).into_owned());
    }
    if upgraded.is_empty() {
        return;
    }
    let holders = holder_verdicts(
        upgraded.iter().map(String::as_str),
        &result.constraint_results,
    );
    result
        .diagnostics
        .retain(|d| falsified_claim(d, &holders) != Some(ConstraintClaim::Indeterminate));
}

/// Drop the constraint claims in `build_diags` that the AUTHORITATIVE
/// `constraint_results` positively falsify.
///
/// The outward mirror of [`merge_post_build_verdicts`], and D2's return leg.
/// The diagnostic merge only ever ADDS build entries, and neither retain that
/// already exists — `engine_build::build_with_geometry_output`'s task-4229 one,
/// or [`merge_post_build_verdicts`]' — knows about the other pass's verdicts.
/// Without this filter, `check` prints stdout `OK …` / `All constraints
/// satisfied.` while stderr still carries build's `… indeterminate: undefined
/// inputs: …` (measured on `tests/fixtures/dfm_with_repr_within.ri`) or `…
/// violated` for the same constraint.  The violated case is also
/// exit-relevant: [`check_gating_error`] reads the merged set, so a stale
/// `ConstraintViolated` Error would move the exit code against the `Satisfied`
/// verdict `check` prints.
///
/// # Contract
///
/// * **Both claim words are in scope**, decided by the [`falsified_claim`]
///   the inward leg shares, so the two legs cannot drift.
/// * **Drop on positive falsification only.**  A claim is removed only when at
///   least one authoritative entry holds its subject AND every such holder
///   falsifies it ([`ConstraintClaim::is_falsified_by`]): any definite verdict
///   settles an indeterminacy, only `Satisfied` retracts a violation.  An
///   unmentioned subject is left alone, which preserves PRD D2's "every
///   build()-only diagnostic appears at least once".
/// * **Id-less claims are never falsified, by design.**  The GD&T
///   `Conforms INDETERMINATE` / `Conforms VIOLATED` lines and the `evaluated to
///   non-boolean value` error name no subject, so no verdict can speak for them
///   (`keeps_idless_conformance_violation`, `keeps_idless_indeterminate_diagnostics`,
///   and on the inward leg `idless_indeterminate_warning_survives_an_upgrade`).
///
/// Real violations still exit non-zero via `ConstraintOutcome::SomeViolated`
/// in `finish_check`, which reads `constraint_results`, never this list.  An
/// empty `constraint_results` falsifies nothing → `build_diags` verbatim (C2).
pub(crate) fn drop_falsified_constraint_diagnostics(
    build_diags: &[reify_core::Diagnostic],
    constraint_results: &[reify_eval::ConstraintCheckEntry],
) -> Vec<reify_core::Diagnostic> {
    // The subjects build's list actually makes a claim about, BORROWED from the
    // messages.  None on the common no-op path where a realization reported
    // only compile/kernel errors.
    let holders = holder_verdicts(
        build_diags
            .iter()
            .filter_map(constraint_claim_in)
            .map(|(_, subject)| subject),
        constraint_results,
    );
    build_diags
        .iter()
        .filter(|d| falsified_claim(d, &holders).is_none())
        .cloned()
        .collect()
}

/// Pure exit-decision helper for `reify build`.
///
/// Returns `true` when the build should exit 0 (success), `false` when it
/// should exit non-zero (failure).  Two independent gates cause failure:
///
/// 1. A [`ConstraintOutcome::SomeViolated`] result — one or more constraints
///    were violated.
/// 2. `has_error_diagnostic` — at least one [`reify_core::Severity::Error`]
///    diagnostic was emitted (e.g. "no registered compute trampoline"), even
///    if the constraint outcome is [`ConstraintOutcome::AllSatisfied`] or
///    [`ConstraintOutcome::SomeIndeterminate`].
///
/// This resolves task-4458 concern (c): `cmd_build` previously exited 0 when
/// an `Error`-severity engine diagnostic was emitted alongside a non-violated
/// constraint outcome.  This helper aligns `cmd_build`'s exit code with
/// `cmd_eval`'s `Severity::Error` gate: since #5403 both callers compute the
/// `has_error_diagnostic` argument by calling [`has_error_diagnostic`], the
/// one shared severity predicate, so the two commands cannot drift apart.
///
/// `reify check` gates on that SAME severity filter through
/// [`check_gating_error`], over its merged diagnostic set, with nothing
/// excused, so every Error moves all three commands' exit codes alike.
///
/// Returns `bool` (not [`std::process::ExitCode`]) so the gate is directly
/// unit-testable; callers convert to `ExitCode` at the boundary.
pub(crate) fn build_is_success(outcome: &ConstraintOutcome, has_error_diagnostic: bool) -> bool {
    !has_error_diagnostic && !matches!(outcome, ConstraintOutcome::SomeViolated)
}

/// Pure exit-decision helper for `reify check`.
///
/// Returns `true` when the overall outcome should cause a non-zero exit:
/// - [`ConstraintOutcome::SomeViolated`] always fails.
/// - [`ConstraintOutcome::SomeIndeterminate`] fails only when `strict` is `true`.
/// - [`ConstraintOutcome::AllSatisfied`] never fails.
///
/// Returns `bool` (not [`std::process::ExitCode`]) so the gate is directly
/// unit-testable; callers convert to `ExitCode` at the boundary.
pub(crate) fn check_fails(outcome: &ConstraintOutcome, strict: bool) -> bool {
    match outcome {
        ConstraintOutcome::SomeViolated => true,
        ConstraintOutcome::SomeIndeterminate(_) => strict,
        ConstraintOutcome::AllSatisfied => false,
    }
}

/// Every `Severity::Error` entry of `diagnostics`, in order.
///
/// The single per-diagnostic severity test, so [`has_error_diagnostic`] and
/// [`check_gating_error`] cannot drift apart: `check`'s gate is exactly this
/// filter, never a restatement of the severity comparison.
fn error_diagnostics(
    diagnostics: &[reify_core::Diagnostic],
) -> impl Iterator<Item = &reify_core::Diagnostic> {
    diagnostics.iter().filter(|d| d.severity == Severity::Error)
}

/// The ONE definition of "this diagnostic set carries an Error".
///
/// Shared by `cmd_eval` and `cmd_build` (as [`build_is_success`]' second
/// argument) — PRD §7's "one shared helper also used by eval/build";
/// [`check_gating_error`] applies the same [`error_diagnostics`] filter.
/// `eval` and `build` have gated on `Severity::Error` since #4458 — locked by
/// `check_error_gate_tests::has_error_diagnostic_is_pure_severity`.
pub(crate) fn has_error_diagnostic(diagnostics: &[reify_core::Diagnostic]) -> bool {
    error_diagnostics(diagnostics).next().is_some()
}

/// `reify check`'s exit gate (INV-SF-2): the first `Severity::Error`
/// diagnostic, or `None`; no Error is ever excused.  The guard against
/// re-adding an excuse is
/// `check_error_gate_tests::check_gating_error_agrees_with_has_error_diagnostic`.
///
/// Applied identically on both `cmd_check` paths, over the MERGED diagnostic
/// set — what the user was just shown — so a realization-only Error is no
/// longer invisible to the exit code.  Both production callers only test
/// `.is_some()`.
pub(crate) fn check_gating_error(
    diagnostics: &[reify_core::Diagnostic],
) -> Option<&reify_core::Diagnostic> {
    error_diagnostics(diagnostics).next()
}

/// Outcome of constraint checking.
#[derive(Debug, PartialEq)]
pub(crate) enum ConstraintOutcome {
    /// Every constraint evaluated to `Satisfied`.
    AllSatisfied,
    /// No constraints violated, but some were `Indeterminate`.
    SomeIndeterminate(usize),
    /// At least one constraint evaluated to `Violated`.
    SomeViolated,
}

/// Structural-equality merge of a discarded [`reify_eval::BuildResult`]'s
/// diagnostics into the authoritative [`reify_eval::CheckResult`]'s.
///
/// The `BuildResult` used to be dropped on the floor, which swallowed every
/// realization-only diagnostic — `compile_geometry_op` gating errors and
/// kernel-dispatch failures `check()` alone never produces — so a module whose
/// geometry cannot compile at all reported "All constraints satisfied." under
/// `check` while `eval`/`build` reported a hard error on the same file (PRD
/// `check-diagnostic-truthfulness.md` D2).
///
/// Neither naive alternative works: build()'s copy of the check-style entries
/// is stale (its internal `self.check(module)` runs before
/// `realization_handles` / `achieved_repr_tol` are populated), and plain
/// concatenation double-prints every eval-level diagnostic because both passes
/// re-run `eval()` over the same input.  Hence keep the seed verbatim and
/// append only what it does not already hold.
///
/// # Invariants upheld (PRD D2)
///
/// * No diagnostic already in the seed is printed twice.
/// * Every build()-only diagnostic appears at least once.
/// * `constraint_results` come from the authoritative `check()` call — the
///   caller's contract, not this function's; the one adjacent exception is
///   [`merge_post_build_verdicts`], which only upgrades `Indeterminate`.
///
/// # Ordering: AUTHORITY, not chronology (and the two arms differ)
///
/// The seed leads the output.  Sub-path (b) seeds with `check()`'s list even
/// though the realization ran first — lead position signals authority and makes
/// "check()'s list verbatim, in order" a property a reader can rely on.
/// Sub-path (c) seeds with the realization's list, because there the seed must
/// first absorb its own internal UNCODED duplicates via [`dedup_diagnostics`].  The
/// asymmetry is stderr ordering only — membership is a union under the same
/// key either way, so no invariant depends on it and no exit code can move.
///
/// The exit gate, [`check_gating_error`], reads whichever merged set an arm
/// produced and is blind to its order, so the asymmetry costs nothing.
/// Collapsing it is owned by **#5973**, which moves sub-path (b) onto (c)'s
/// shape so both arms seed from the realization's list.
///
/// # Dedup key
///
/// [`DiagnosticIdentity`] — see its contract; this merge relies on it
/// excluding spans, for the reason below.  All three helpers — this one,
/// [`dedup_diagnostics`] and [`strip_diagnostics_reproduced_by`] — share
/// [`DiagnosticIdentity::of`], so they cannot drift onto different keys.
///
/// Membership is tested against the ACCUMULATING set — seeded from
/// `check_diags`, then grown as build entries are appended — so duplicates
/// *internal to* `build_diags` also collapse.  Measured, not hypothetical: on
/// `tests/fixtures/mirror_bare_origin.ri` the realization emits `failed to
/// compile geometry operation: mirror: expected a Plane value, got undef` twice
/// for a single call site (re-measured at task 5746 / units-length ε, which
/// gated `plane_yz(0)` at the producer; the duplicated line was the `'ox'`
/// compile error before that, and the duplication itself is unchanged).  Note the corollary
/// below: a list that can hold two DISTINCT findings with the same text must be
/// kept out of that collapse (see [`strip_diagnostics_reproduced_by`]).
///
/// ## Why the span is NOT part of the key
///
/// Tempting, because it would tell two same-text findings apart — but MEASURED
/// to be wrong: on `tests/fixtures/mirror_bare_origin.ri` the duplicated
/// geometry-compile error carries two DIFFERENT `realization_span`s for the one
/// user-visible problem, so a span-aware key stops collapsing exactly the
/// duplication this leaf exists to collapse.  (That span observation was made on
/// the `'ox'` message; task 5746 / units-length ε moved the rejection to the
/// producer, so the duplicated line now reads `mirror: expected a Plane value,
/// got undef` — same two realization passes, same duplication, re-measured.)
///
/// The corollary is that no local key can distinguish "one finding reported
/// twice" from "two findings that happen to read alike" (two GD&T callouts of
/// the same characteristic are byte-identical — `illegal_modifier_error` puts
/// the location in a label, not the message).  Two consequences follow, and
/// BOTH are needed: [`dedup_diagnostics`] never collapses a CODED entry (every
/// per-callout finding carries a code), and a re-run of a coded pass must have
/// its redundant copy withdrawn wholesale before the merge — see
/// [`strip_diagnostics_reproduced_by`], which is how `cmd_check`'s sub-path (c)
/// does it.
pub(crate) fn merge_build_diagnostics(
    check_diags: &[reify_core::Diagnostic],
    build_diags: &[reify_core::Diagnostic],
) -> Vec<reify_core::Diagnostic> {
    let mut merged = check_diags.to_vec();
    let mut seen: std::collections::HashSet<DiagnosticIdentity<'_>> =
        check_diags.iter().map(DiagnosticIdentity::of).collect();
    for diag in build_diags {
        if seen.insert(DiagnosticIdentity::of(diag)) {
            merged.push(diag.clone());
        }
    }
    merged
}

/// Collapse duplicates WITHIN one diagnostic list, keeping the first occurrence
/// of each [`DiagnosticIdentity`] in order — but ONLY among entries carrying no
/// [`reify_core::DiagnosticCode`].
///
/// The self-dedup half of [`merge_build_diagnostics`], named rather than
/// spelled `merge_build_diagnostics(&[], &diags)`, and sharing its
/// [`DiagnosticIdentity::of`] so the two cannot drift onto different keys.
/// `cmd_check`'s sub-path (c) needs it on its own: the realization re-runs the
/// eval front-end internally and emits some entries twice for a single call
/// site (measured on the `mirror(...)` bare-origin fixture: `failed to compile
/// geometry operation: mirror: expected a Plane value, got undef`, re-measured
/// at task 5746 — before it, the same duplicated slot carried the `'ox'`
/// compile error).
///
/// # Why coded entries are exempt
///
/// No local key can tell "one finding reported twice" from "two findings that
/// happen to read alike" ([`merge_build_diagnostics`]' "Dedup key" section) —
/// but the two populations split cleanly on the code:
///
/// * The measured re-run duplication this helper exists for is UNCODED —
///   `failed to compile geometry operation: …` (engine_build.rs) carries a label
///   but no code.
/// * The entries whose MULTIPLICITY is user-visible are all coded, and are
///   per-callout by construction: `GdtIllegalModifier`
///   (`engine_constraints::illegal_modifier_error` names the characteristic in
///   the message and the location in a label, so two distinct callouts of one
///   characteristic are byte-identical), `ConstraintIndeterminate`
///   (`gdt_indeterminate_diag` formats `Conforms INDETERMINATE: {reason}` with
///   no constraint id at all, so two geometric `Conforms` that are indeterminate
///   for the same reason are byte-identical), and `ConstraintViolated` (two
///   constraints sharing a DSL label both read `constraint {label} violated`).
///
/// Collapsing any of those drops a callout's only explanation and prints ONE
/// line where the non-geometry arm prints two.  Exempting coded entries makes
/// the sub-path (c) composition preserve their multiplicity exactly as the
/// concatenating non-geometry arm does — pinned by
/// `purpose_order_keeps_two_idless_conformance_warnings` and
/// `purpose_order_keeps_two_same_label_violations`.
///
/// It also makes [`strip_diagnostics_reproduced_by`] load-bearing rather than
/// merely preferable: a genuinely re-run CODED pass is no longer collapsed here
/// at all, so its redundant copy must be withdrawn wholesale.
///
/// NOT the other way round: `merge_build_diagnostics(a, b)` is deliberately
/// *not* `dedup_diagnostics(&[a, b].concat())`.  The merge reproduces its seed
/// list VERBATIM — including any duplicates internal to it — because that seed
/// is `check()`'s authoritative output and "check()'s list verbatim, in order"
/// is a property the D2 contract lets callers rely on.  Only the appended
/// `build_diags` are filtered against it.
pub(crate) fn dedup_diagnostics(diags: &[reify_core::Diagnostic]) -> Vec<reify_core::Diagnostic> {
    let mut seen: std::collections::HashSet<DiagnosticIdentity<'_>> =
        std::collections::HashSet::new();
    diags
        .iter()
        .filter(|d| {
            // Short-circuits before `seen`, so a coded entry is neither
            // collapsed nor able to collapse a later uncoded one (their keys
            // differ by construction anyway — the code is part of the key).
            d.code.is_some() || seen.insert(DiagnosticIdentity::of(d))
        })
        .cloned()
        .collect()
}

/// Drop from `diags` every entry that `rerun` reproduces, so a caller that is
/// about to append `rerun` itself does not report the same pass twice.
///
/// # Why this is not just a dedup
///
/// `cmd_check`'s sub-path (c) `used_build` arm has TWO copies of the GD&T
/// legality pass's output: the realization seeds its diagnostics from
/// `Engine::check`, which ends by extending with `run_gdt_check_passes`, and
/// `cmd_check` then runs that same pure `(module, values)` function itself.
/// Collapsing them with [`dedup_diagnostics`] is not sound, because that pass
/// can legitimately emit two byte-identical lines for two different callouts
/// (`engine_constraints::illegal_modifier_error` names the characteristic in
/// the message and the location in a label), and the dedup key cannot tell
/// those apart from a re-run — see [`merge_build_diagnostics`]' "Dedup key"
/// section.  Deduping collapsed a two-callout module to ONE printed line while
/// the non-geometry arm printed two.  [`dedup_diagnostics`] no longer touches
/// coded entries at all, which is what makes this withdrawal the ONLY thing
/// standing between a re-run coded pass and a doubled report —
/// `purpose_order_does_not_double_the_gdt_pass` is the lock.
///
/// Removing the realization's copy WHOLESALE and letting `cmd_check`'s own run
/// be the single source sidesteps the ambiguity entirely: multiplicity comes
/// from one authoritative run, so it is right by construction, and both arms
/// then append the pass's output the same plain way.
///
/// Keyed on the run we are about to append, NOT on a list of GD&T
/// [`reify_core::DiagnosticCode`]s: `run_gdt_check_passes` is documented as the
/// aggregation point for future static passes, and a code list would go stale
/// the moment one is added.  A line the realization emitted that this run does
/// NOT reproduce is therefore kept — it is a build-only diagnostic and PRD D2
/// requires it to reach the user.
///
/// Pinned by `d2_pass_ordering_tests::purpose_order_keeps_two_same_text_callouts`
/// and, end to end, by `cli_gdt_legality.rs::
/// check_purpose_gdt_two_illegal_callouts_on_geometry_module_print_twice`.
pub(crate) fn strip_diagnostics_reproduced_by(
    diags: &[reify_core::Diagnostic],
    rerun: &[reify_core::Diagnostic],
) -> Vec<reify_core::Diagnostic> {
    if rerun.is_empty() {
        return diags.to_vec();
    }
    let reproduced: std::collections::HashSet<DiagnosticIdentity<'_>> =
        rerun.iter().map(DiagnosticIdentity::of).collect();
    diags
        .iter()
        .filter(|d| !reproduced.contains(&DiagnosticIdentity::of(d)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod check_fails_tests;
#[cfg(test)]
mod build_is_success_tests;
#[cfg(test)]
mod check_error_gate_tests;
#[cfg(test)]
mod merge_build_diagnostics_tests;
#[cfg(test)]
mod drop_falsified_constraint_diagnostics_tests;
#[cfg(test)]
mod merge_post_build_verdicts_tests;
#[cfg(test)]
mod d2_pass_ordering_tests;
