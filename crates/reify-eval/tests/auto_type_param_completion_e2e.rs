//! ζ — auto-type-param completion integration gate.
//!
//! PRD references: docs/prds/v0_3/auto-type-param-resolution-completion.md
//!   §11 (boundary-table), §12 Phase 6 (integration gate).
//!
//! This aggregate harness binds four user-facing example fixtures end-to-end
//! under the REAL `SimpleConstraintChecker` (the same checker the CLI and GUI
//! binaries inject).  It covers the §11 rows that are genuinely end-to-end on
//! the shipped examples/auto/*.ri files:
//!
//! - §11.1 row #3 "Constraint-aware unique selection" (real→Selected) — step-3
//! - §11.1 row #5 "Bounded fallback, jointly infeasible" — step-5
//! - §11.1 row #6 "Value population" — step-1
//! - §11.1 new "NoCandidate negative" — step-6
//! - §11.2 row #2 "Stub-path callers unchanged" (stub-vs-real contrast) — step-4
//!
//! Fixtures bound:
//!   - examples/auto/bearing_resolved_value.ri   (α/δ — single candidate, value pop)
//!   - examples/auto/bearing_constraint_select.ri (β — per-candidate ValueMap + real→Selected)
//!   - examples/auto/bounded_fallback_unsound.ri  (γ — joint-recheck BoundedInfeasible)
//!   - examples/auto/bearing_unsat.ri             (ζ — NoCandidate, all candidates violated)
//!
//! Tasks that produced these fixtures: α=4431, β=4433, γ=4434, δ=4435, ζ=4437.

#![allow(clippy::mutable_key_type)]

// ── Fixture path constants ────────────────────────────────────────────────────

/// Absolute path to examples/auto/bearing_resolved_value.ri.
/// Produced by task 4431 (α) + value-population wired by task 4435 (δ).
const BEARING_RESOLVED_VALUE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_resolved_value.ri"
);

/// Absolute path to examples/auto/bearing_constraint_select.ri.
/// Produced by task 4433 (β — per-candidate ValueMap + real-checker selection).
const BEARING_CONSTRAINT_SELECT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_constraint_select.ri"
);

/// Absolute path to examples/auto/bounded_fallback_unsound.ri.
/// Produced by task 4434 (γ — BFS-fallback joint-recheck, BoundedInfeasible).
const BOUNDED_FALLBACK_UNSOUND_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bounded_fallback_unsound.ri"
);

/// Absolute path to examples/auto/bearing_unsat.ri.
/// Produced by task 4437 (ζ — NoCandidate negative fixture, all candidates violated).
const BEARING_UNSAT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_unsat.ri"
);
