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

/// The recognized datum-projection member names, in one place so the
/// `MemberAccess` gate in `expr.rs` can cheaply decide whether a member access
/// is a datum projection (and therefore route to [`datum_projection_result_type`])
/// before falling through to the generic "member access not yet supported" path.
///
/// This is the *union* of every member the table below recognizes on any datum
/// receiver — a member in this set on a datum receiver is resolved by the table
/// (possibly to `Unavailable`/`Ambiguous`); a member outside this set is not a
/// datum projection at all.
pub(crate) const DATUM_PROJECTION_MEMBERS: &[&str] =
    &["dir", "normal", "origin", "x", "y", "z", "xy_plane"];

/// Resolve the result type of a datum-projection member access `receiver.member`.
///
/// Implements the geometric-relations β projection table ("implicit projection
/// iff unique", `docs/prds/v0_6/geometric-relations.md` §9):
///
/// - `Axis`      → `.dir`: [`Type::Direction`], `.origin`: `Point3<Length>`
/// - `Plane`     → `.normal`: [`Type::Direction`], `.origin`: `Point3<Length>`
///   (`.dir` is *Unavailable* — callers must write `.normal`)
/// - `Frame(_)`  → `.x`/`.y`/`.z`: [`Type::Direction`], `.origin`: `Point3<Length>`,
///   `.xy_plane`: [`Type::Plane`]; `.dir`/`.normal` are *Ambiguous* (any of the
///   three basis directions could be meant — suggest `.x`/`.y`/`.z`)
/// - `Direction` → `.x`/`.y`/`.z`: `Real` (dimensionless components)
///
/// Any other receiver (including `Point { .. }`) or any unrecognized member on a
/// datum receiver resolves to [`DatumProjectionResolution::Unavailable`] — the
/// locked nonsense-filter (covers `point.dir`). γ/η extend this table here.
pub(crate) fn datum_projection_result_type(
    receiver: &Type,
    member: &str,
) -> DatumProjectionResolution {
    use DatumProjectionResolution::*;
    match receiver {
        Type::Axis => match member {
            "dir" => Resolved(Type::Direction),
            "origin" => Resolved(Type::point3(Type::length())),
            _ => Unavailable,
        },
        Type::Plane => match member {
            "normal" => Resolved(Type::Direction),
            "origin" => Resolved(Type::point3(Type::length())),
            // `.dir` on a plane is Unavailable — the unique direction is the normal.
            _ => Unavailable,
        },
        Type::Frame(_) => match member {
            "x" | "y" | "z" => Resolved(Type::Direction),
            "origin" => Resolved(Type::point3(Type::length())),
            "xy_plane" => Resolved(Type::Plane),
            // A bare directional projection on a frame is ambiguous: it could be
            // any of the three basis directions. Suggest the disambiguating names.
            "dir" | "normal" => Ambiguous {
                suggestions: vec!["x", "y", "z"],
            },
            _ => Unavailable,
        },
        Type::Direction => match member {
            // Dimensionless unit-vector components ("Real").
            "x" | "y" | "z" => Resolved(Type::dimensionless_scalar()),
            _ => Unavailable,
        },
        // Non-datum receivers (incl. Point { .. }) have no datum projections.
        _ => Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Point3<Length>` codomain shorthand used by the `.origin` projections.
    fn point3_length() -> Type {
        Type::point3(Type::length())
    }

    /// Every valid `(receiver, member)` projection must resolve to
    /// `Resolved(codomain)` per the geometric-relations β projection table
    /// (`docs/prds/v0_6/geometric-relations.md` §9): the downward members of
    /// Axis / Plane / Frame(3) / Direction.
    ///
    /// RED until step 8 fills in the table (the stub resolves everything to
    /// `Unavailable`, so each Resolved expectation fails).
    #[test]
    fn datum_projection_resolves_valid_members_to_codomain() {
        let cases: &[(Type, &str, Type)] = &[
            // Axis: .dir → Direction, .origin → Point3<Length>
            (Type::Axis, "dir", Type::Direction),
            (Type::Axis, "origin", point3_length()),
            // Plane: .normal → Direction, .origin → Point3<Length>
            (Type::Plane, "normal", Type::Direction),
            (Type::Plane, "origin", point3_length()),
            // Frame(3): .x/.y/.z → Direction, .origin → Point3<Length>, .xy_plane → Plane
            (Type::Frame(3), "x", Type::Direction),
            (Type::Frame(3), "y", Type::Direction),
            (Type::Frame(3), "z", Type::Direction),
            (Type::Frame(3), "origin", point3_length()),
            (Type::Frame(3), "xy_plane", Type::Plane),
            // Direction: .x/.y/.z → Real (dimensionless components)
            (Type::Direction, "x", Type::dimensionless_scalar()),
            (Type::Direction, "y", Type::dimensionless_scalar()),
            (Type::Direction, "z", Type::dimensionless_scalar()),
        ];
        for (receiver, member, expected) in cases {
            assert_eq!(
                datum_projection_result_type(receiver, member),
                DatumProjectionResolution::Resolved(expected.clone()),
                "{receiver}.{member} should resolve to {expected}"
            );
        }
    }

    /// A datum-projection member that does not exist on the receiver is a typed
    /// rejection (`Unavailable`): `point.dir` (the locked nonsense-filter signal),
    /// `plane.dir` (callers must write `.normal`), and `direction.normal`.
    #[test]
    fn datum_projection_rejects_missing_members_as_unavailable() {
        let cases: &[(Type, &str)] = &[
            (point3_length(), "dir"),
            (Type::Plane, "dir"),
            (Type::Direction, "normal"),
        ];
        for (receiver, member) in cases {
            assert_eq!(
                datum_projection_result_type(receiver, member),
                DatumProjectionResolution::Unavailable,
                "{receiver}.{member} should be Unavailable"
            );
        }
    }

    /// A bare projection that could mean several members is `Ambiguous`, carrying
    /// the disambiguating suggestions. `frame.dir` / `frame.normal` could be any
    /// of the three basis directions, so the resolver suggests `.x/.y/.z`.
    ///
    /// RED until step 8 (the stub resolves these to `Unavailable`).
    #[test]
    fn datum_projection_rejects_ambiguous_frame_direction() {
        assert_eq!(
            datum_projection_result_type(&Type::Frame(3), "dir"),
            DatumProjectionResolution::Ambiguous {
                suggestions: vec!["x", "y", "z"]
            },
            "frame.dir is ambiguous; suggest .x/.y/.z"
        );
        assert_eq!(
            datum_projection_result_type(&Type::Frame(3), "normal"),
            DatumProjectionResolution::Ambiguous {
                suggestions: vec!["x", "y", "z"]
            },
            "frame.normal is ambiguous; suggest .x/.y/.z"
        );
    }
}
