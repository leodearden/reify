//! The verdict layer: reconciles the check and build passes' constraint results and
//! diagnostics into the set a command reports, and decides which of them fail the command.

use reify_core::{DiagnosticCode, Severity};

/// The constraint-indeterminacy message grammar, as one pair of literals:
/// `constraint {label-or-id} indeterminate: {reason}`.
///
/// That is exactly what `reify_constraints`' checker emits
/// (reify-constraints/src/lib.rs), with the raw id rewritten to the constraint's
/// label by `engine_constraints::labeled_diagnostics` when it carries one.
/// Every reading of the grammar below — forward and inverse — is built from
/// these two literals, so they cannot drift apart; the round trip is pinned by
/// `indeterminacy_grammar_round_trips`.
const INDETERMINACY_PREFIX: &str = "constraint ";
const INDETERMINACY_INFIX: &str = " indeterminate";

/// The SUBJECT an indeterminacy diagnostic about `entry` names: its label when
/// it has one (preferred exactly as `engine_constraints::labeled_diagnostics`
/// does), else its raw id.
///
/// Borrowed for a labeled entry, so the identity of a constraint costs no
/// allocation on that path.  Both falsification legs go through this one
/// definition, so they cannot drift into computing different identities
/// (esc-5748-4).
fn indeterminacy_subject(entry: &reify_eval::ConstraintCheckEntry) -> std::borrow::Cow<'_, str> {
    match entry.label.as_deref() {
        Some(label) => std::borrow::Cow::Borrowed(label),
        None => std::borrow::Cow::Owned(entry.id.to_string()),
    }
}

/// The INVERSE reading of the grammar: the subject `message` claims is
/// indeterminate, or `None` when the message does not follow the grammar.
///
/// Extracting the subject and hashing it is what keeps both falsification legs
/// O(diagnostics + constraints).  The shape this replaces built one anchored
/// `constraint {subject} indeterminate` `String` per definite constraint and ran
/// a `message.contains(…)` for every (diagnostic, constraint) pair — quadratic
/// in the constraint count of any real assembly, on EVERY geometry-bearing
/// `reify check` since D1 widened the routing.
///
/// The match is now EXACT rather than a substring test, which is strictly
/// stronger than the anchoring it replaces: `Foo#constraint[1]` cannot match
/// `Foo#constraint[10]`'s still-true warning, and a one-character label cannot
/// match nearly every message (`anchored_matcher_survives_an_id_prefix_collision`,
/// `anchored_matcher_survives_a_short_label_collision`).
///
/// A message that does not follow the grammar at all yields `None` and is
/// therefore never dropped — `gdt_indeterminate_diag`'s id-less `Conforms
/// INDETERMINATE: {reason}` (`keeps_idless_indeterminate_diagnostics`), and any
/// third-party `ConstraintChecker`'s own wording.  That is the safe direction: a
/// wrongly dropped line is unrecoverable output loss (it is the only explanation
/// the user gets for a printed `INDETERMINATE`), a wrongly kept one is merely
/// redundant.
fn indeterminacy_subject_in(message: &str) -> Option<&str> {
    let rest = message.strip_prefix(INDETERMINACY_PREFIX)?;
    let end = rest.find(INDETERMINACY_INFIX)?;
    Some(&rest[..end])
}

/// The forward reading of the grammar, kept next to the inverse so the two stay
/// legible as one definition.  Test-only: production code goes the other way
/// (extract once, then hash-lookup), which is the whole point of the shape.
#[cfg(test)]
fn indeterminacy_anchor(subject: &str) -> String {
    format!("{INDETERMINACY_PREFIX}{subject}{INDETERMINACY_INFIX}")
}

/// `true` when `d` is a `ConstraintIndeterminate` claim about one of `subjects`.
///
/// The single matcher both falsification legs — [`merge_post_build_verdicts`]'s
/// retain and [`drop_falsified_indeterminate_diagnostics`]' filter — go through,
/// so they cannot drift into deleting different sets.
fn is_falsified_indeterminacy<S>(
    d: &reify_core::Diagnostic,
    subjects: &std::collections::HashSet<S>,
) -> bool
where
    S: std::borrow::Borrow<str> + Eq + std::hash::Hash,
{
    d.code == Some(reify_core::DiagnosticCode::ConstraintIndeterminate)
        && indeterminacy_subject_in(&d.message).is_some_and(|s| subjects.contains(s))
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
/// * **The upgraded entry's now-false `ConstraintIndeterminate` warning is
///   dropped**, matched by the shared [`is_falsified_indeterminacy`].
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
    // Subjects of the entries that upgraded, collected during the walk and
    // applied to `diagnostics` afterwards (one `&mut result` borrow at a time).
    // A set, not a Vec: the retain below is then O(diagnostics), not
    // O(diagnostics x upgraded).
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
        upgraded.insert(indeterminacy_subject(entry).into_owned());
    }
    if upgraded.is_empty() {
        return;
    }
    // The warnings check() emitted for the upgraded constraints are now false —
    // drop them, through the shared matcher.
    result
        .diagnostics
        .retain(|d| !is_falsified_indeterminacy(d, &upgraded));
}

/// Drop the `ConstraintIndeterminate` diagnostics in `build_diags` that the
/// AUTHORITATIVE `constraint_results` positively falsify.
///
/// The outward mirror of [`merge_post_build_verdicts`], and D2's return leg.
/// The diagnostic merge only ever ADDS build entries, and neither retain that
/// already exists — `engine_build::build_with_geometry_output`'s task-4229 one,
/// or [`merge_post_build_verdicts`]' — knows about the other pass's verdicts.
/// Without this filter, `check` prints stdout `OK …` / `All constraints
/// satisfied.` while stderr still carries build's `… indeterminate: undefined
/// inputs: …` for the same constraint (measured on
/// `tests/fixtures/dfm_with_repr_within.ri`).
///
/// # Contract
///
/// * **Drop on positive falsification only.**  An entry is removed only when
///   `constraint_results` holds a DEFINITE verdict for that same constraint; an
///   id the authoritative list never mentions is left alone.  That is what
///   preserves PRD D2's "every build()-only diagnostic appears at least once".
/// * **Only `DiagnosticCode::ConstraintIndeterminate` is in scope.**  A verdict
///   falsifies the indeterminacy claim and nothing else, so a
///   `ConstraintViolated` line naming the same constraint survives.
/// * **The matcher is the shared [`is_falsified_indeterminacy`]**, over the
///   shared [`indeterminacy_subject`] identity, so this leg and the inward one
///   cannot drift.
///
/// Two residual gaps are pinned as tests rather than argued here, both tracked
/// under #6048: build reporting `Violated` where check says `Satisfied` keeps
/// build's error line (`mirror_case_build_side_violation_currently_survives`),
/// and an id-less `ConstraintIndeterminate` carries no needle to match
/// (`idless_indeterminate_warning_survives_an_upgrade`).
///
/// The first gap is exit-relevant: [`check_gating_error`] reads the merged set,
/// so a surviving stale `ConstraintViolated` Error would move the exit code
/// against the `Satisfied` verdict `check` prints on stdout.
/// [`CHECK_ERROR_EXIT_ALLOWLIST`] entry #4 (`FixPath`) holds that off; its fix
/// belongs HERE — widening this helper's scope past `ConstraintIndeterminate`
/// (#6048) — not in the gate.  Real violations still exit non-zero via
/// `ConstraintOutcome::SomeViolated` in `finish_check`.
///
/// No exit code can move either way: `report_eval_output`'s outcome derives
/// solely from `constraint_results`, never from the diagnostic list.  An empty
/// `constraint_results`, or one holding no definite verdict, falsifies nothing
/// → `build_diags` verbatim (C2).
pub(crate) fn drop_falsified_indeterminate_diagnostics(
    build_diags: &[reify_core::Diagnostic],
    constraint_results: &[reify_eval::ConstraintCheckEntry],
) -> Vec<reify_core::Diagnostic> {
    if build_diags.is_empty() {
        return Vec::new();
    }
    // The subjects build's list actually CLAIMS are indeterminate, extracted
    // once and BORROWED from the messages — no allocation, and empty on the
    // common no-op path where a realization reported only compile/kernel errors.
    let claimed: std::collections::HashSet<&str> = build_diags
        .iter()
        .filter(|d| d.code == Some(reify_core::DiagnosticCode::ConstraintIndeterminate))
        .filter_map(|d| indeterminacy_subject_in(&d.message))
        .collect();
    if claimed.is_empty() {
        return build_diags.to_vec();
    }
    // ONE walk of the authoritative verdicts, hashing each definite entry's
    // subject into `claimed`.  The shape this replaces allocated an anchored
    // needle per definite entry and then scanned every message for each of them
    // — O(diagnostics x constraints) on every geometry-bearing `reify check`.
    let falsified: std::collections::HashSet<&str> = constraint_results
        .iter()
        .filter(|e| e.satisfaction != reify_ir::Satisfaction::Indeterminate)
        .filter_map(|e| claimed.get(indeterminacy_subject(e).as_ref()).copied())
        .collect();
    if falsified.is_empty() {
        return build_diags.to_vec();
    }
    build_diags
        .iter()
        .filter(|d| !is_falsified_indeterminacy(d, &falsified))
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
/// `reify check` gates on that SAME predicate and then layers one extra thing
/// on top: [`check_gating_error`] excuses any Error a
/// [`CHECK_ERROR_EXIT_ALLOWLIST`] entry claims (a bounded burn-down ratchet,
/// #5404).  `build`/`eval` are deliberately NOT allowlist-aware — they have
/// gated on raw `Severity::Error` since #4458 and must keep gating on e.g.
/// the trampoline Error that `check` excuses.
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

/// How one [`CHECK_ERROR_EXIT_ALLOWLIST`] entry selects the diagnostics it
/// excuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckErrorAllowlistMatcher {
    /// Preferred: match the machine-readable [`DiagnosticCode`].
    Code(DiagnosticCode),
    /// LEGACY code-less emissions ONLY — enforced, not merely documented:
    /// [`allowlist_excuses`] never lets this arm match a diagnostic that
    /// carries a [`DiagnosticCode`].  Coding a legacy emission therefore drops
    /// it out of the allowlist with no table edit, and a code minted after
    /// the gate can never be excused through a legacy substring
    /// (`check_error_gate_tests::message_entries_excuse_only_code_less_diagnostics`).
    ///
    /// PRD §7 sketches this as `MessagePrefix`; entry #2's marker
    /// (`is unresolved (Undef)`) sits mid-string, after the per-argument
    /// detail, so a prefix match could not express it.  Substring is the
    /// honest spelling of the same intent.
    MessageContains(&'static str),
}

/// What retiring one [`CHECK_ERROR_EXIT_ALLOWLIST`] entry will take.  Recorded
/// so the burn-down owner does not have to re-derive it from the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckErrorAllowlistDisposition {
    /// The emission's `Severity::Error` is simply wrong for this path; the
    /// producer should emit `Severity::Warning` instead.  A Warning never
    /// gates, so the entry can then be deleted with no other change.
    Demote,
    /// The emission needs a machine-readable [`DiagnosticCode`] before it can
    /// be reasoned about (or excused) precisely.
    ///
    /// Part of the documented taxonomy but not exercised by the seeded table:
    /// both code-less seed entries are `Demote`, because their severity — not
    /// their lack of a code — is what is wrong on `check`'s path.  Kept so
    /// the burn-down owner has the vocabulary without having to invent it, and
    /// carrying a bare `#[allow(dead_code)]` (no trailing `//` rationale, so it
    /// anchors no PTODO marker — there is no deferred work here to cite).
    #[allow(dead_code)]
    Recode,
    /// The emission is a real Error, but `check` reaching it at all is the
    /// bug; the fix is on the code path, not on the diagnostic.
    FixPath,
}

/// One row of the `reify check` Error-exit burn-down allowlist.
struct CheckErrorExitAllowance {
    /// The only field the gate itself reads — see [`allowlist_excuses`].
    matcher: CheckErrorAllowlistMatcher,
    /// What retiring this entry will take.  BURN-DOWN METADATA: read by
    /// `check_error_exit_allowlist_ratchet`, never by the gate, so it carries
    /// a bare `#[allow(dead_code)]` (no trailing `//` rationale, so it anchors
    /// no PTODO marker — the work is cited by `cite` below, not here).
    /// Deliberately data rather than prose: the burn-down owner reads it off
    /// the table instead of re-deriving it from each message.
    #[allow(dead_code)]
    disposition: CheckErrorAllowlistDisposition,
    /// PTODO-canonical cite (`#NNNN`) of the LIVE task that retires this entry.
    /// Burn-down metadata, same as `disposition` above.  PTODO never reads a
    /// string literal, so this field is liveness-checked only through a marker
    /// comment in this file citing the same task: that marker is load-bearing,
    /// not a duplicate to delete, and `every_allowlist_cite_has_a_ptodo_liveness_marker`
    /// turns red if it goes missing.
    #[allow(dead_code)]
    cite: &'static str,
}

/// The BOUNDED burn-down set for INV-SF-2's `reify check` exit gate.
///
/// [`check_gating_error`] makes any `Severity::Error` diagnostic on any
/// channel exit non-zero — *unless* an entry here excuses it.  Every entry was
/// seeded for a legacy emission that predates the gate and whose Error
/// severity is wrong (or wrongly reachable) on `check`'s deliberately
/// kernel-less, solver-less path; none of them is a policy decision about what
/// `check` should tolerate.  The enforced excusal is the MATCHER, not that
/// intent, and entry #2's matcher is knowingly broader than its intent — see
/// that entry.
///
/// The table is pinned whole by `check_error_exit_allowlist_ratchet` and is
/// burned to ZERO by #5404, converging on INV-SF-2's end state where no
/// per-code list mediates the exit code at all.  **Do not add entries** — see
/// that module's doc for the standing obligation.
///
/// Entries #2 and #3 were seeded from a MEASURED sweep of `reify check` over
/// all `examples/*.ri` and `crates/reify-cli/tests/fixtures/*.ri`
/// (2026-08-29): of the files that exited 0 while printing an `error:` line,
/// the `auto`-param case they target was the only one expected on a healthy
/// path.  Their code-less substrings excuse more than that case.  Entry #1 is a posture entry that
/// excuses nothing `check` reaches today, kept by ruling until its deadline
/// (see its marker); entry #4 is a merge hazard, not a corpus finding.  The
/// genuine design errors in that same sweep —
/// `mirror: o{x,y,z} argument expects Length`, `unresolvable GeomRef::*`,
/// `transform_{log,exp}: ... dimensionless`, `E_StackupEmptyChain` — are
/// deliberately NOT excused and now exit 1, matching `reify eval`, which
/// already exits 1 on every one of them.
const CHECK_ERROR_EXIT_ALLOWLIST: &[CheckErrorExitAllowance] = &[
    // TODO(#5404): delete this entry no later than the change that gives
    // `check` a non-empty compute registry (#6693).
    //
    // `cmd_check` attaches NO compute trampoline BY DESIGN (see its doc
    // contract: registering one would run a potentially slow FEA solve inside
    // the lightweight static-check path).  The engine conditions this
    // diagnostic's severity on that posture: on an EMPTY compute registry it
    // is a `Severity::Warning`, so today this entry excuses nothing `check`
    // can reach, and `cli_build_fea.rs::check_fea_violated_constraint_is_not_gated`
    // holds its exit 0 by severity alone.  The entry is kept deliberately (the
    // ruling is recorded on #5404).  On a non-empty registry the same emission
    // is a correct Error that this entry would silence — hence the deadline.
    //
    // Keyed on the code rather than the message stem, as `engine_compute.rs`
    // advises: the co-resident code-less "compute trampoline was cancelled"
    // Error shares the `@optimized target {t:?}: ` prefix and must keep gating.
    CheckErrorExitAllowance {
        matcher: CheckErrorAllowlistMatcher::Code(DiagnosticCode::NoRegisteredComputeTrampoline),
        disposition: CheckErrorAllowlistDisposition::Demote,
        cite: "#5404",
    },
    // TODO(#5404): give the `auto`-param-awaiting-solver case its own code or
    // message, key this excusal on that, then delete this entry.
    //
    // Seeded for a geometry op argument left `Undef` because an `auto` param
    // awaits a solver `check` deliberately does not run (`examples/
    // fea_bracket_minimize_mass.ri`, `param thickness : Length = auto(free)`).
    // `reify eval` on that same file exits 0 (MEASURED — it takes >60s because
    // it actually solves), so gating there would make `check` newly DISAGREE
    // with `eval` about a healthy design.
    //
    // KNOWINGLY OVER-BROAD: the text comes from
    // `geometry_ops.rs::unresolved_arg_message`, which words EVERY Undef
    // geometry argument this way, so this entry also excuses genuine design
    // errors that `reify eval` rejects (a `box` width divided by a zero param
    // exits 0 under `check`, 1 under `eval`), as well as the code-less
    // "per-instance re-realization compile error … is unresolved (Undef)"
    // family #6608 replaces with coded Errors.  That gap is pinned by
    // `cli_check.rs::check_excuses_every_code_less_undef_geometry_argument`,
    // which #5404 flips.
    CheckErrorExitAllowance {
        matcher: CheckErrorAllowlistMatcher::MessageContains("is unresolved (Undef)"),
        disposition: CheckErrorAllowlistDisposition::Demote,
        cite: "#5404",
    },
    // TODO(#5404): demote alongside the entry above — this is its rollup.
    //
    // "all geometry operations failed; no geometry output produced" is the
    // rollup printed whenever every op failed, WHATEVER the cause; `check`
    // writes no geometry, so "no geometry output produced" is not a fact about
    // the design.  Excusing it is harmless only while the per-op errors beside
    // it still gate; where entry #2 excuses those too, this entry inherits
    // entry #2's over-breadth.
    //
    // `examples/sweep_degenerate.ri` carries the same rollup beside
    // `unresolvable GeomRef::Step(0)` / `GeomRef::Sub('s1')` errors that
    // nothing here matches, so it still exits 1 — as `reify eval` does.
    CheckErrorExitAllowance {
        matcher: CheckErrorAllowlistMatcher::MessageContains("all geometry operations failed"),
        disposition: CheckErrorAllowlistDisposition::Demote,
        cite: "#5404",
    },
    // TODO(#5404): retire once #6048 widens
    // `drop_falsified_indeterminate_diagnostics` past `ConstraintIndeterminate`.
    //
    // MERGE HAZARD, not a corpus finding: build's copy can emit a stale
    // `ConstraintViolated` Error that survives the merge while `check`'s
    // AUTHORITATIVE verdict for the same constraint is `Satisfied` — pinned by
    // `d2_pass_ordering_tests::mirror_case_build_side_violation_currently_
    // survives`.  `drop_falsified_indeterminate_diagnostics` scopes only to
    // `ConstraintIndeterminate`, so nothing withdraws it today.  Gating on it
    // would produce a FALSE exit 1 contradicting `check`'s own stdout.
    //
    // No real signal is lost: a genuine violation still exits non-zero through
    // `ConstraintOutcome::SomeViolated` in `finish_check`, which reads the
    // authoritative `constraint_results`, never the diagnostic list.
    CheckErrorExitAllowance {
        matcher: CheckErrorAllowlistMatcher::Code(DiagnosticCode::ConstraintViolated),
        disposition: CheckErrorAllowlistDisposition::FixPath,
        cite: "#5404",
    },
];

/// Every `Severity::Error` entry of `diagnostics`, in order.
///
/// The single per-diagnostic severity test, so [`has_error_diagnostic`] and
/// [`check_gating_error`] cannot drift apart: `check`'s gate is exactly this
/// filter plus [`allowlist_excuses`], never a restatement of the severity
/// comparison.
fn error_diagnostics(
    diagnostics: &[reify_core::Diagnostic],
) -> impl Iterator<Item = &reify_core::Diagnostic> {
    diagnostics.iter().filter(|d| d.severity == Severity::Error)
}

/// The ONE definition of "this diagnostic set carries an Error".
///
/// Shared by `cmd_eval`, `cmd_build` (as [`build_is_success`]' second
/// argument) and [`check_gating_error`] — PRD §7's "one shared helper also
/// used by eval/build".
///
/// Deliberately allowlist-BLIND: [`CHECK_ERROR_EXIT_ALLOWLIST`] is a
/// `check`-only migration ratchet.  `eval` and `build` have gated on
/// `Severity::Error` since #4458 and must keep gating on the trampoline Error
/// — locked by `check_error_gate_tests::
/// has_error_diagnostic_is_pure_severity_and_allowlist_blind`.
pub(crate) fn has_error_diagnostic(diagnostics: &[reify_core::Diagnostic]) -> bool {
    error_diagnostics(diagnostics).next().is_some()
}

/// Whether some [`CHECK_ERROR_EXIT_ALLOWLIST`] entry excuses `d` from moving
/// `reify check`'s exit code.
fn allowlist_excuses(d: &reify_core::Diagnostic) -> bool {
    CHECK_ERROR_EXIT_ALLOWLIST
        .iter()
        .any(|entry| match entry.matcher {
            CheckErrorAllowlistMatcher::Code(code) => d.code == Some(code),
            CheckErrorAllowlistMatcher::MessageContains(needle) => {
                d.code.is_none() && d.message.contains(needle)
            }
        })
}

/// `reify check`'s exit gate (INV-SF-2): the first `Severity::Error`
/// diagnostic that no [`CHECK_ERROR_EXIT_ALLOWLIST`] entry excuses, or `None`.
///
/// Applied identically on both `cmd_check` paths, over the MERGED diagnostic
/// set — what the user was just shown — so a realization-only Error is no
/// longer invisible to the exit code.
///
/// Both production callers only test `.is_some()`.  The diagnostic is returned
/// rather than a `bool` solely so `an_excused_error_neither_gates_nor_masks`
/// can assert that an allowlisted Error never masks a co-resident gating one.
pub(crate) fn check_gating_error(
    diagnostics: &[reify_core::Diagnostic],
) -> Option<&reify_core::Diagnostic> {
    error_diagnostics(diagnostics).find(|d| !allowlist_excuses(d))
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
/// [`DiagKey`], a hand-built tuple rather than a derived `PartialEq`:
/// [`reify_core::Diagnostic`] is `#[non_exhaustive]` and derives only
/// `Debug, Clone`, so deriving equality would be a reify-core API change
/// outside this leaf's scope AND would drag `labels`/`candidates` into
/// identity, which the PRD excludes.
///
/// Membership is tested against the ACCUMULATING set — seeded from
/// `check_diags`, then grown as build entries are appended — so duplicates
/// *internal to* `build_diags` also collapse.  Measured, not hypothetical: on
/// `tests/fixtures/mirror_bare_origin.ri` the realization emits `failed to
/// compile geometry operation: mirror: expected a Plane value, got undef` twice
/// for a single call site (re-measured at task 5746 / units-length ε, which
/// gated `plane_yz(0)` at the producer; the duplicated line was the `'ox'`
/// compile error before that, and the duplication itself is unchanged).  Note the corollary in
/// [`DiagKey`]: a list that can hold two DISTINCT findings with the same text
/// must be kept out of that collapse (see [`strip_diagnostics_reproduced_by`]).
pub(crate) fn merge_build_diagnostics(
    check_diags: &[reify_core::Diagnostic],
    build_diags: &[reify_core::Diagnostic],
) -> Vec<reify_core::Diagnostic> {
    let mut merged = check_diags.to_vec();
    let mut seen: std::collections::HashSet<DiagKey> =
        check_diags.iter().map(diagnostic_identity).collect();
    for diag in build_diags {
        if seen.insert(diagnostic_identity(diag)) {
            merged.push(diag.clone());
        }
    }
    merged
}

/// The identity under which [`merge_build_diagnostics`] and
/// [`dedup_diagnostics`] consider two diagnostics the same line.
///
/// Extracted so the merge and the self-dedup cannot drift onto different keys —
/// the same anti-drift move [`is_falsified_indeterminacy`] makes for the two
/// falsification legs (esc-5748-4).  See [`merge_build_diagnostics`]' "Dedup
/// key" section for why this is a hand-built tuple rather than a derived
/// `PartialEq` on `#[non_exhaustive] reify_core::Diagnostic`.
///
/// # Why the span is NOT part of the key
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
type DiagKey = (Severity, Option<reify_core::DiagnosticCode>, String);

fn diagnostic_identity(d: &reify_core::Diagnostic) -> DiagKey {
    (d.severity, d.code, d.message.clone())
}

/// Collapse duplicates WITHIN one diagnostic list, keeping the first occurrence
/// of each [`DiagKey`] in order — but ONLY among entries carrying no
/// [`reify_core::DiagnosticCode`].
///
/// The self-dedup half of [`merge_build_diagnostics`], named rather than
/// spelled `merge_build_diagnostics(&[], &diags)`, and sharing its
/// [`diagnostic_identity`] so the two cannot drift onto different keys.
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
/// happen to read alike" ([`DiagKey`]) — but the two populations split cleanly
/// on the code:
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
    let mut seen: std::collections::HashSet<DiagKey> = std::collections::HashSet::new();
    diags
        .iter()
        .filter(|d| {
            // Short-circuits before `seen`, so a coded entry is neither
            // collapsed nor able to collapse a later uncoded one (their keys
            // differ by construction anyway — the code is part of the key).
            d.code.is_some() || seen.insert(diagnostic_identity(d))
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
/// those apart from a re-run — see [`DiagKey`].  Deduping collapsed a
/// two-callout module to ONE printed line while the non-geometry arm printed
/// two.  [`dedup_diagnostics`] no longer touches coded entries at all, which is
/// what makes this withdrawal the ONLY thing standing between a re-run coded
/// pass and a doubled report — `purpose_order_does_not_double_the_gdt_pass` is
/// the lock.
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
    let reproduced: std::collections::HashSet<DiagKey> =
        rerun.iter().map(diagnostic_identity).collect();
    diags
        .iter()
        .filter(|d| !reproduced.contains(&diagnostic_identity(d)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod check_fails_tests;
#[cfg(test)]
mod build_is_success_tests;
#[cfg(test)]
mod check_error_exit_allowlist_ratchet;
#[cfg(test)]
mod check_error_gate_tests;
#[cfg(test)]
mod merge_build_diagnostics_tests;
#[cfg(test)]
mod drop_falsified_indeterminate_diagnostics_tests;
#[cfg(test)]
mod merge_post_build_verdicts_tests;
#[cfg(test)]
mod d2_pass_ordering_tests;
