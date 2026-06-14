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
///
/// The trailing `axis`/`plane`/`point` members are the geometric-relations ε
/// *feature→datum* projections (`feature.axis : Axis`, `feature.plane : Plane`,
/// `feature.point : Point3<Length>`; `dir` — already present for β — doubles as
/// the feature `→Direction` projection). They only resolve on the ε *feature*
/// receivers (`Type::Geometry`/`Type::Selector(_)`); on a β datum receiver they
/// resolve to `Unavailable` (a datum is not a feature, so it has no `.axis`).
/// Note the `expr.rs` gate admits *any* non-aggregation member on a feature
/// receiver, so this set is consulted only for the β datum-receiver branch — but
/// keeping the ε members here preserves the const's "union of every recognized
/// member" contract.
pub(crate) const DATUM_PROJECTION_MEMBERS: &[&str] =
    &["dir", "normal", "origin", "x", "y", "z", "xy_plane", "axis", "plane", "point"];

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
/// The geometric-relations ε *feature→datum* projections extend the table
/// **downward to feature receivers** — a realized feature (`Type::Geometry`) or
/// a topology selection (`Type::Selector(_)`/`Type::AnySelector`) projects to
/// the datum its analytic/construction-history trait bundle carries
/// (design §2.2):
///
/// - `Geometry`/`Selector` → `.axis`: [`Type::Axis`], `.plane`: [`Type::Plane`],
///   `.point`: `Point3<Length>`, `.dir`: [`Type::Direction`]
///
/// These type *statically* as the datum codomain (the unambiguous arm of the
/// `Axis | Axis?` refinement); the resolve-time ambiguity (`plate.axis` → several
/// non-coaxial candidates) is a runtime select-a-subfeature diagnostic, not a
/// static type. The eval is **kernel-backed** (`reify-eval` geometry_ops),
/// distinct from β's pure `eval_datum_projection` — see `expr.rs`.
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
        // ── geometric-relations ε: feature→datum projections ──────────────
        // A realized feature (Geometry) or a topology selection (Selector)
        // projects to the datum its trait bundle carries. Unknown members are a
        // typed rejection (`feature.foo` → Unavailable → DatumProjectionUnavailable).
        Type::Geometry | Type::Selector(_) | Type::AnySelector => match member {
            "axis" => Resolved(Type::Axis),
            "plane" => Resolved(Type::Plane),
            "point" => Resolved(Type::point3(Type::length())),
            "dir" => Resolved(Type::Direction),
            _ => Unavailable,
        },
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

/// For an *Unavailable* datum projection, an optional redirect hint naming the
/// member the author most likely meant. `plane.dir` is unavailable because a
/// plane's unique direction is its `.normal`, so this returns `Some(".normal")`;
/// the `MemberAccess` arm in `expr.rs` appends the hint to the
/// `DatumProjectionUnavailable` message (`"Plane has no projection '.dir'; use
/// .normal"`), matching the documented canonical form. Returns `None` when no
/// single obvious redirect exists (e.g. `point.dir`). γ/η extend this alongside
/// the projection table above.
pub(crate) fn datum_projection_unavailable_hint(
    receiver: &Type,
    member: &str,
) -> Option<&'static str> {
    match (receiver, member) {
        // A plane's unique direction is its normal — redirect `.dir` to `.normal`.
        (Type::Plane, "dir") => Some(".normal"),
        _ => None,
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

    /// The `plane.dir` *Unavailable* case offers a `.normal` redirect hint (a
    /// plane's unique direction is its normal) so `expr.rs` can emit the
    /// documented `"Plane has no projection '.dir'; use .normal"` form. Other
    /// unavailable projections have no single obvious redirect.
    #[test]
    fn datum_projection_unavailable_hint_redirects_plane_dir_to_normal() {
        assert_eq!(
            datum_projection_unavailable_hint(&Type::Plane, "dir"),
            Some(".normal"),
            "plane.dir should redirect to .normal"
        );
        assert_eq!(
            datum_projection_unavailable_hint(&point3_length(), "dir"),
            None,
            "point.dir has no obvious redirect"
        );
        assert_eq!(
            datum_projection_unavailable_hint(&Type::Direction, "normal"),
            None,
            "direction.normal has no obvious redirect"
        );
    }
}
