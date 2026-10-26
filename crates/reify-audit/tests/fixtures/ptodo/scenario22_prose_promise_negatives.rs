// Scenario 22 (PRD §8.1, lane δ-C): comments carrying a task cite that must
// NOT classify. Every line here is a negative — the fixture contributes ZERO
// findings, which is what makes it an over-fire guard rather than a smoke test.
//
// δ-C fires on a cite in OWNER position. Each block below is one measured
// false-positive class (PRD §21) whose cite is provenance or not a task at
// all. tests/ptodo.rs and tests/infra/test_reify_audit_ptodo.sh seed every
// cited id TERMINAL, the worst case: a terminal owner is precisely what turns
// a `Cited` entry into a High `orphaned` finding, so silence proves the
// grammar held rather than that a task happened to still be open.
//
// On a `.rs` file δ-B runs first, so no line carries its deferral prose beside
// a canonical cite unless one of δ-B's own guards kills it.
//
// Self-match safety: this path sits under the allowlisted `crates/reify-audit/`
// prefix, so the live sweep never sees it. Copied into a temp project root by
// tests/ptodo.rs and tests/cli.rs, its path is NOT allowlisted and it IS swept.

// --- provenance verbs -------------------------------------------------------

// The opaque form was introduced by #4578.
// The registry landed in #4578 alongside the substrate.
// Field order is per #4578 and must not drift.
// For the full path and rationale see #4578.
/// #4578's ruling keeps the marker zero-field.
fn scenario22_provenance_verbs() {}

// --- a `Task N:` header -----------------------------------------------------

// Task 3441: snapshot this template before the rewrite.
fn scenario22_task_header() {}

// --- past-tense `until` narration ------------------------------------------

/// shipped this form until #6890 renamed it
/// was a second override until task 4168, and the merge dropped it
fn scenario22_past_tense_until() {}

// --- `lands` without a subordinator: commit-style narration -----------------

// Task #6082 lands `x.rs` here rather than in the crate root.
fn scenario22_lands_without_subordinator() {}

// --- an intervening word between the owner phrase and the cite ------------

/// Grammar CAPABILITY, owned by the #5907 series
fn scenario22_owned_by_the_series() {}

// --- a quoted owner phrase (δ-B's guard 2 also holds here) -----------------

// the reason string reads "blocked on #5951" verbatim
fn scenario22_quoted_phrase() {}

// --- an owner word glued to an identifier or a member path -----------------

// see mark_pending #4578 for the transition
// self.pending #4578 is recomputed on every demand walk
fn scenario22_identifier_glue() {}

// --- PRD-relative indices: §8.2's bound covers both spellings ---------------

/// Diagnostic emission is owned by PRD task #10 (diagnostic mapping)
// emission is deferred to task 10 of the PRD
fn scenario22_prd_relative() {}

// --- a task-shaped identifier, not a cite -----------------------------------

// covered by the task_4578_regression test
fn scenario22_identifier_not_a_cite() {}

// --- a G-allow marker belongs to its own lane ------------------------------
// The DISCRIMINATING case: its cite IS in δ-C owner position, but the G-allow
// lane exempts it as provenance under that lane's rule (a) ("(done)" right
// after the cite), so with the cite seeded terminal that lane stays silent and
// δ-C's g_allow_marker_body guard is the ONLY thing preventing a finding. A
// "PRD "-prefixed cite would not discriminate: `owned by PRD #N` is not owner
// position for δ-C either.

// G-allow: shared envelope seam; owned by #7778 (done) — no non-test caller yet
fn scenario22_g_allow_marker_discriminating() {}
