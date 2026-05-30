//! Body partitioning: route segmented regions to shell/tet meshers and
//! identify shell↔tet interfaces (PRD task **T12**).
//!
//! Implements the routing + interface-descriptor half of
//! `docs/prds/v0_4/structural-analysis-shells.md` §124 ("mixed-region body
//! partitioning"). The T4 auto-segmenter ([`crate::segment_regions`]) labels
//! each connected component of a single body's medial mask as
//! `ShellEligible` / `TetEligible` / `MixedComponentOfBody`; this module maps
//! that classification to a per-region [`RegionMeshKind`] (shell vs. tet
//! mesher) and emits a kernel-agnostic [`ShellTetInterface`] descriptor for
//! every shell↔tet junction.
//!
//! # Why kernel-agnostic
//!
//! `reify-shell-extract` deliberately does **not** depend on
//! `reify-solver-elastic` (cycle-avoidance, task γ #3834), so the MPC tying
//! rows ([`reify_solver_elastic::mpc::MpcRow`]) cannot be produced here. This
//! module emits only the geometric tie descriptor (region pair + unit normal +
//! thickness + world location); `reify-eval`'s `engine_build` converts it to
//! `MpcRow` once it has both crates in scope. See `plan.json` design decisions.
//!
//! # Why proximity, not shared faces
//!
//! `segment_regions` builds 6-face connected components, so a shell region and
//! a tet region of one body are **disconnected** mask components (their medial
//! axes sit at different depths and do not touch). Interfaces are therefore
//! identified by world-space proximity between region voxel sets, not by shared
//! voxel faces — a shared face would have fused the two into one component.
