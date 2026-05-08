//! Quality-check pass for mesh morphing (PRD task #9).
//!
//! Runs after the morph engine produces a deformed [`reify_types::VolumeMesh`]
//! and returns a two-tier verdict:
//!
//! - [`QualityVerdict::HardFail`] — one or more tetrahedra are inverted
//!   (negative Jacobian determinant). Hard-fail strictly preempts soft-fail.
//! - [`QualityVerdict::SoftFail`] — no inversions, but one or more quality
//!   metrics breach their configured thresholds: minimum scaled Jacobian,
//!   fraction of elements below 0.25, or maximum aspect-ratio increase.
//! - [`QualityVerdict::Pass`] — all checks passed.
//!
//! ## Preconditions
//!
//! - **P1 elements only.** `morphed.tet_indices` must be segmented in 4-node
//!   chunks (P1 tetrahedra). P2 input with 10-node elements will be
//!   mis-segmented by `chunks_exact(4)` without a structured error. Engine
//!   integration in PRD task #10 guarantees P1 before calling this function.
//! - **Matched connectivity.** `morphed.tet_indices.len()` is expected to equal
//!   `source.tet_indices.len()` (morph operations preserve topology). When
//!   lengths differ, the aspect-ratio-increase comparison is skipped
//!   (threshold 3 is effectively disabled); the hard-fail and min-scaled-J /
//!   pct-below-025 checks still run on the morphed mesh.
//! - **Valid vertex indices.** Elements referencing out-of-range vertex indices
//!   are silently skipped (same defensive discipline as `laplacian.rs`).
