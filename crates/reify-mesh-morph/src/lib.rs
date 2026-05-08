//! Mesh morphing classifier and engine for Reify.
//!
//! This crate provides the combined eligibility predicate for mesh morphing
//! (PRD `docs/prds/v0_3/mesh-morphing.md`, tasks #3 and #10).

pub mod eligibility;
pub mod options;
pub mod types;

pub use eligibility::{Eligibility, MorphSnapshot, Reason, morph_eligible};
pub use options::{MorphFailure, MorphOptions};
pub use types::{BRep, InversionDetails, MetricsBreached};

/// Re-exported so consumers can pattern-match `Reason::BijectionFailure(_)`
/// without depending on `reify-eval` directly.
pub use reify_eval::{
    BijectionFailure, CorrespondenceMap, NamingLayerErrorReason, SubShapeKind, SubShapeSide,
};
