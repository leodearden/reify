//! Datum-projection result-type resolver (geometric-relations β).
//!
//! Member-access projections on datum receivers (Axis/Plane/Frame/Direction)
//! resolve "downward" to a member's codomain type: e.g. `axis.dir : Direction`,
//! `plane.origin : Point3<Length>`. The rule is *implicit projection iff unique* —
//! a projection that does not exist on the receiver is a typed rejection
//! (`E_DATUM_PROJECTION_UNAVAILABLE`), and a bare projection that could mean
//! several members (`frame.dir`) is ambiguous (`E_DATUM_PROJECTION_AMBIGUOUS`).
//!
//! This module is the single source of truth for the projection table; the
//! `MemberAccess` arm in `expr.rs` consults [`datum_projection_result_type`] to
//! type-check and lower projections, and downstream γ/η extend the table here.
//!
//! See `docs/prds/v0_6/geometric-relations.md` §9 β.

use super::*;

/// Outcome of resolving a datum-projection member access `receiver.member`.
#[allow(dead_code)] // β scaffold; resolver + variants are consumed in steps 8/10.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DatumProjectionResolution {
    /// The projection is valid; the member resolves to this codomain type.
    Resolved(Type),
    /// The receiver has no such projection member — a typed rejection surfaced as
    /// `DiagnosticCode::DatumProjectionUnavailable` (`E_DATUM_PROJECTION_UNAVAILABLE`).
    Unavailable,
    /// The bare projection is ambiguous; `suggestions` names the disambiguating
    /// members to write instead (e.g. `["x", "y", "z"]` for `frame.dir`) — surfaced
    /// as `DiagnosticCode::DatumProjectionAmbiguous` (`E_DATUM_PROJECTION_AMBIGUOUS`).
    Ambiguous { suggestions: Vec<&'static str> },
}

/// Resolve the result type of a datum-projection member access `receiver.member`.
///
/// β stub: returns [`DatumProjectionResolution::Unavailable`] for every
/// `(receiver, member)` pair. Step 8 fills in the projection table per the
/// analysis (Axis/Plane/Frame/Direction → Direction/Point3<Length>/Plane/Real).
#[allow(dead_code)] // β scaffold; the projection table is implemented in step 8.
pub(crate) fn datum_projection_result_type(
    _receiver: &Type,
    _member: &str,
) -> DatumProjectionResolution {
    DatumProjectionResolution::Unavailable
}
