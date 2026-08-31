// Scenario 17 (PRD §8.1, lane δ-B): ordinary comments carrying a cite that must
// NOT classify. Every line here is a negative — the fixture contributes ZERO
// findings, which is what makes it an over-fire guard rather than a smoke test.
//
// δ-B fires on a `.rs` comment line carrying BOTH a canonical #NNNN cite and
// deferral prose. Every line below carries a cite; each one must be saved by
// exactly one guard. tests/ptodo.rs seeds those cites TERMINAL wherever a
// terminal status is safe, which is the worst case: a terminal cite is precisely
// what turns a `Cited` entry into a High `orphaned` finding, so silence proves a
// GUARD held rather than that the task happened to still be live.
//
// Task #6087 rejected this lane once at a 48% false-positive rate. Classes (a)
// and (b) below are the two populations that caused that, and the fixture pins
// both through the whole `check()` pipeline — not just at unit level, where an
// over-fire could never reach the merge gate.
//
// Self-match safety: this path sits under the allowlisted `crates/reify-audit/`
// prefix, so the live sweep never sees it. Copied into a temp project root by
// tests/ptodo.rs and tests/cli.rs, its path is NOT allowlisted and it IS swept.

// --- class (a): the needle is inside an IDENTIFIER, not prose --------------
// Killed by has_deferral_prose guard 3. The first four are VERBATIM from
// crates/reify-eval/src/cache.rs, where every one carries a genuine cite.

// cause via mark_pending_with_cause (task #2330 §9.2 invariant).
fn scenario17_identifier_snake_case() {}

/// / `mark_pending_with_cause` (tasks #2326, #2335) and all Failed transitions
fn scenario17_identifier_in_doc_comment() {}

// --- pending_cause / mark_failed / mark_pending_with_cause tests (task #2330 step-3) ---
fn scenario17_identifier_prefix() {}

// --- mark_pruned_pending producer tests (task #4739 γ) ---
fn scenario17_identifier_suffix() {}

// A hyphenated compound NAMES a thing rather than deferring work (guard 3).
// the pending-queue drain path is exercised by task #2330's harness
fn scenario17_hyphenated_compound() {}

// A member/path qualification is a SYMBOL reference, not prose (guard 3, left).
// self.pending is recomputed on every demand walk (task #2330)
fn scenario17_member_qualified() {}

// A quoted state name is not deferral prose (guard 2).
/// serialises the "pending" wire tag for the GUI bridge (task #2326)
fn scenario17_quoted_state() {}

// `Pending` is the NodeCache freshness enum VARIANT (guard 1, case-sensitive).
/// constructs a Pending producer for the demand-prune tests (task #2335)
fn scenario17_enum_variant() {}

// --- class (b): the cite is PRD-RELATIVE, not a task id --------------------
// Killed by §8.2's prd_relative_cite via has_canonical_cite. The first three are
// VERBATIM from the live corpus, and every one DOES carry real deferral prose —
// so the prose guards alone do not save them; only the cite grammar does.

/// Diagnostic emission is deferred to PRD task #10 (Diagnostic mapping for
fn scenario17_prd_relative_family3() {}

/// a uniaxial-stretch scenario is deferred to the downstream PRD task #12
fn scenario17_prd_relative_family3_downstream() {}

//   is not yet a hydrated Value::GeometryHandle (PRD invariant #2:
fn scenario17_prd_relative_family2() {}

// A glued PRD-artifact namespace (family 1).
/// envelope assembly is blocked on §7#5 landing first
fn scenario17_prd_relative_family1() {}

// --- class (c): δ-B is cite-ANCHORED --------------------------------------
// A deferral with no canonical cite is not a candidate at all, so the lane can
// emit no structural kind and cannot degenerate into "flag every comment
// containing the word pending".

/// wiring is pending the morph rewrite
fn scenario17_uncited_deferral() {}

// --- class (d): a G-allow marker belongs to its own lane -------------------
// That lane runs an independent scan_g_allow_markers →
// resolve_g_allow_owner_liveness pass with its own `g-allow-orphaned` kind, so
// without δ-B's guard the same line would be reported twice under two kinds.
//
// The DISCRIMINATING case is the second line below. Its owner cite is preceded
// by "PRD ", which the G-allow lane's own narrower rule (c) exempts — so that
// lane stays silent — while §8.2 does NOT treat "PRD " as a PRD-relative
// left-context, so the cite is canonical and the prose defers. δ-B's
// g_allow_marker_body guard is therefore the ONLY thing standing between this
// line and a finding, and tests/ptodo.rs seeds #7777 terminal to prove it.
//
// The first line is the live shape VERBATIM from crates/reify-ir/src/value.rs.
// Its owner cite is NOT exempt under rule (c), so it is seeded non-terminal
// (as #5235 is live today); it documents the real population rather than
// discriminating the guard.
//
// The third line is the OWNER-LESS seam: every cite on it is provenance-exempt
// under the G-allow lane's OWN rules (a) ("#4092 (done)") and (b) ("re-homed
// from cancelled"), so that lane has no owner to resolve and stays silent for a
// second, independent reason. Neither lane claims such a line — two rules
// composing, not a hole (see scan_file arm (7) choice (iv)) — and both cites are
// seeded terminal so a δ-B that reached the line would fire.

// G-allow: shared display formatter input type (PRD display-unit-preference §6.2); the four surfaces route onto it in L4 task #5235 (pending) — no non-test caller until then
fn scenario17_g_allow_marker_live_shape() {}

// G-allow: shared envelope assembler; the four surfaces are blocked on PRD #7777 — no non-test caller until then
fn scenario17_g_allow_marker_discriminating() {}

// G-allow: envelope assembly is deferred to #4092 (done); re-homed from cancelled #3429 — no non-test caller yet
fn scenario17_g_allow_marker_owner_less() {}

// --- the dominant benign class: a comment that EXPLAINS rather than defers --

/// Maps each hex/wedge outcome onto its diagnostic code (task #2330).
fn scenario17_benign_explanatory() {}
