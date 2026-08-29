// Scenario 14 (PRD §8.1, lane δ-A): #[allow(dead_code)] rationales that must
// NOT classify. Every line here is a negative — the fixture contributes ZERO
// findings, which is what makes it an over-fire guard rather than a smoke test.
//
// Two of the three false-positive guards are pinned against live code by
// unit-level negatives in src/ptodo.rs. Guard 3 (word boundary) cannot be:
// the real identifier-class sites (mark_pending_with_cause,
// mark_pruned_pending in crates/reify-eval/src/cache.rs) are ordinary
// comments carrying no #[allow(dead_code)] attribute, so they are not δ-A
// candidates at all. This fixture supplies the synthetic pin (task #6087).
//
// Self-match safety: this path sits under the allowlisted `crates/reify-audit/`
// prefix, so the live sweep never sees it. Copied into a temp project root by
// tests/ptodo.rs and tests/cli.rs, its path is NOT allowlisted and it IS swept.

// Guard 3 — the needle `pending` is inside an identifier, not prose.
#[allow(dead_code)] // superseded by mark_pending_with_cause at the wire site
fn scenario14_identifier_prefix() {}

#[allow(dead_code)] // see the mark_pruned_pending producer path instead
fn scenario14_identifier_suffix() {}

// Guard 1 — `Pending` is the NodeCache freshness enum VARIANT, not deferral.
#[allow(dead_code)] // constructs a Pending producer for the demand-prune tests
fn scenario14_enum_variant() {}

// Guard 2 — a quoted state name is not deferral prose.
#[allow(dead_code)] // serialises the "pending" wire tag for the GUI bridge
fn scenario14_quoted_state() {}

// The dominant benign class: a rationale that EXPLAINS rather than defers.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
fn scenario14_benign_explanatory() {}

// A bare attribute carries no rationale at all.
#[allow(dead_code)]
fn scenario14_bare_attribute() {}
