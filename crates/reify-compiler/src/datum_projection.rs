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
///
/// The `frame`/`yz_plane`/`zx_plane` members are the geometric-relations η
/// additions: `frame` is the `self`→`Frame(3)` intrinsic datum and the two new
/// principal planes extend both the `self` (`Type::StructureRef`) anchor and the
/// `Frame(_)` arm (additive to the β `xy_plane`).
pub(crate) const DATUM_PROJECTION_MEMBERS: &[&str] = &[
    "dir", "normal", "origin", "x", "y", "z", "frame", "xy_plane", "yz_plane", "zx_plane", "axis",
    "plane", "point",
];

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
            // The RULING #6089 type, not an incidental choice; narrowing the
            // eval-side `frame3` constructor to it is task #7625.
            "origin" => Resolved(Type::point3(Type::length())),
            // A frame's three principal planes (η adds yz_plane/zx_plane).
            "xy_plane" | "yz_plane" | "zx_plane" => Resolved(Type::Plane),
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
        // ── geometric-relations η: self (StructureRef) intrinsic datums ──
        // The `self` anchor projects to the structure's own (identity) frame's
        // intrinsic datums (design §6): origin/frame, the three basis directions,
        // and the three principal planes. User-declared members shadow these — the
        // expr.rs self-datum arm consults this table only AFTER `scope.resolve`
        // misses — so the table is the intrinsic fallback, not an override.
        Type::StructureRef(_) => match member {
            "origin" => Resolved(Type::point3(Type::length())),
            "frame" => Resolved(Type::Frame(3)),
            "x" | "y" | "z" => Resolved(Type::Direction),
            "xy_plane" | "yz_plane" | "zx_plane" => Resolved(Type::Plane),
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
    use reify_core::ValueCellId;
    use reify_core::ty::TypeDiscriminants;
    use reify_expr::{EvalContext, eval_expr};
    use reify_ir::{CompiledExpr, Value, ValueMap};
    use reify_test_support::{
        axis_val, frame_val, orientation_val, plane_val, point3, vec3_dimensionless,
    };
    use strum::IntoEnumIterator;

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

    // ── geometric-relations η (task 4387): self (StructureRef) intrinsic datums ──

    /// On a `StructureRef` receiver — the `self` anchor — the intrinsic self-datums
    /// resolve to their codomain: origin→Point3<Length>, frame→Frame(3),
    /// x/y/z→Direction, xy_plane/yz_plane/zx_plane→Plane. RED until step 6 adds the
    /// `Type::StructureRef(_)` arm (today StructureRef hits the `_ => Unavailable`
    /// fallthrough).
    #[test]
    fn datum_projection_resolves_self_datums_on_structure_ref() {
        let s = Type::StructureRef("S".to_string());
        let cases: &[(&str, Type)] = &[
            ("origin", point3_length()),
            ("frame", Type::Frame(3)),
            ("x", Type::Direction),
            ("y", Type::Direction),
            ("z", Type::Direction),
            ("xy_plane", Type::Plane),
            ("yz_plane", Type::Plane),
            ("zx_plane", Type::Plane),
        ];
        for (member, expected) in cases {
            assert_eq!(
                datum_projection_result_type(&s, member),
                DatumProjectionResolution::Resolved(expected.clone()),
                "self.{member} should resolve to {expected}"
            );
        }
    }

    /// An unknown member on a `StructureRef` self-anchor is a typed rejection
    /// (`Unavailable`) — the same nonsense-filter as on a datum receiver.
    #[test]
    fn datum_projection_rejects_unknown_self_datum_as_unavailable() {
        let s = Type::StructureRef("S".to_string());
        assert_eq!(
            datum_projection_result_type(&s, "wat"),
            DatumProjectionResolution::Unavailable,
            "self.wat should be Unavailable"
        );
    }

    /// η adds yz_plane / zx_plane to the existing `Frame(_)` arm (additive to
    /// xy_plane): a frame's three principal planes. RED until step 6 (today only
    /// xy_plane resolves; yz_plane/zx_plane hit the arm's `_ => Unavailable`).
    #[test]
    fn datum_projection_resolves_frame_principal_planes() {
        for member in ["xy_plane", "yz_plane", "zx_plane"] {
            assert_eq!(
                datum_projection_result_type(&Type::Frame(3), member),
                DatumProjectionResolution::Resolved(Type::Plane),
                "frame.{member} should resolve to Plane"
            );
        }
    }

    /// DATUM_PROJECTION_MEMBERS gains "frame", "yz_plane", "zx_plane" (η self-datum +
    /// frame principal-plane members) so the expr.rs gate routes them to the table.
    /// RED until step 6 extends the const.
    #[test]
    fn datum_projection_members_contains_eta_additions() {
        for m in ["frame", "yz_plane", "zx_plane"] {
            assert!(
                DATUM_PROJECTION_MEMBERS.contains(&m),
                "DATUM_PROJECTION_MEMBERS should contain {m:?}"
            );
        }
    }

    // ── task #6100: projection table ⇔ evaluator lockstep ──

    /// Whether `v` has the kind of value `codomain` denotes, for every codomain a
    /// datum receiver's `Resolved` arm yields. A codomain with no check here
    /// panics, so a new codomain must extend this helper.
    fn has_codomain_kind(v: &Value, codomain: &Type) -> bool {
        match codomain {
            Type::Direction => matches!(v, Value::Direction { .. }),
            Type::Plane => matches!(
                v,
                Value::Plane { normal, .. } if matches!(&**normal, Value::Vector(c) if c.len() == 3)
            ),
            Type::Frame(_) => matches!(v, Value::Frame { .. }),
            t if *t == point3_length() => matches!(v, Value::Point(c) if c.len() == 3),
            t if *t == Type::dimensionless_scalar() => matches!(v, Value::Real(_)),
            other => panic!("has_codomain_kind has no check for codomain {other}; extend it"),
        }
    }

    /// How the lockstep test below treats a receiver type.
    enum ReceiverFixture {
        /// A datum receiver whose projections reify-expr evaluates purely: the
        /// expression the test projects from.
        Pure(CompiledExpr),
        /// A feature receiver: its projections are kernel-backed in reify-eval,
        /// not pure, so the test does not evaluate them.
        KernelBacked(Type),
        /// A type the projection table gives no datum projection.
        NonDatum(Type),
    }

    impl ReceiverFixture {
        fn ty(&self) -> &Type {
            match self {
                Self::Pure(receiver) => &receiver.result_type,
                Self::KernelBacked(ty) | Self::NonDatum(ty) => ty,
            }
        }
    }

    /// One receiver fixture per `Type` variant. EXHAUSTIVE, with NO `_` arm: a
    /// new `Type` variant does not compile until it has a fixture here.
    fn receiver_fixture(d: TypeDiscriminants) -> ReceiverFixture {
        use ReceiverFixture::{KernelBacked, NonDatum, Pure};
        use TypeDiscriminants as D;
        let literal = |value: Value, ty: Type| Pure(CompiledExpr::literal(value, ty));
        match d {
            D::Axis => literal(
                axis_val(point3(1.0, 2.0, 3.0), vec3_dimensionless(1.0, 0.0, 0.0)),
                Type::Axis,
            ),
            D::Plane => literal(
                plane_val(point3(0.0, 0.0, 0.0), vec3_dimensionless(0.0, 0.0, 1.0)),
                Type::Plane,
            ),
            D::Direction => literal(
                Value::Direction {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                Type::Direction,
            ),
            // 120° about (1,1,1): frame x/y/z = world y/z/x.
            D::Frame => literal(
                frame_val(point3(4.0, 5.0, 6.0), orientation_val(0.5, 0.5, 0.5, 0.5)),
                Type::Frame(3),
            ),
            // The `self` anchor, exactly as expr.rs lowers `self.<datum>`: a ref
            // to the `__self` cell, which never exists at eval time.
            D::StructureRef => Pure(CompiledExpr::value_ref(
                ValueCellId::new("S", "__self"),
                Type::StructureRef("S".to_string()),
            )),
            D::Geometry => KernelBacked(Type::Geometry),
            D::Selector => KernelBacked(Type::Selector(reify_core::ty::SelectorKind::Face)),
            D::AnySelector => KernelBacked(Type::AnySelector),
            D::Bool => NonDatum(Type::Bool),
            D::Int => NonDatum(Type::Int),
            D::String => NonDatum(Type::String),
            D::Scalar => NonDatum(Type::length()),
            D::Enum => NonDatum(Type::Enum("E".to_string())),
            D::List => NonDatum(Type::List(Box::new(Type::Axis))),
            D::Set => NonDatum(Type::Set(Box::new(Type::Axis))),
            D::Map => NonDatum(Type::Map(Box::new(Type::String), Box::new(Type::Axis))),
            D::Keyed => NonDatum(Type::Keyed(Box::new(Type::Axis))),
            D::Option => NonDatum(Type::Option(Box::new(Type::Axis))),
            D::Function => NonDatum(Type::Function {
                params: vec![],
                return_type: Box::new(Type::Axis),
            }),
            D::TypeParam => NonDatum(Type::TypeParam("T".to_string())),
            D::TraitObject => NonDatum(Type::TraitObject("Tr".to_string())),
            D::Field => NonDatum(Type::Field {
                domain: Box::new(point3_length()),
                codomain: Box::new(Type::length()),
            }),
            D::Feature => NonDatum(Type::Feature),
            D::Point => NonDatum(point3_length()),
            D::Vector => NonDatum(Type::Vector {
                n: 3,
                quantity: Box::new(Type::length()),
            }),
            D::Tensor => NonDatum(Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }),
            D::Complex => NonDatum(Type::Complex(Box::new(Type::dimensionless_scalar()))),
            D::Orientation => NonDatum(Type::Orientation(3)),
            D::Transform => NonDatum(Type::Transform(3)),
            D::AffineMap => NonDatum(Type::AffineMap(3)),
            D::Range => NonDatum(Type::Range(Box::new(Type::length()))),
            D::BoundingBox => NonDatum(Type::BoundingBox),
            D::ScalarParam => NonDatum(Type::ScalarParam("Q".to_string())),
            D::Matrix => NonDatum(Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }),
            D::Error => NonDatum(Type::Error),
            D::Union => NonDatum(Type::Union(vec![
                Type::StructureRef("A".to_string()),
                Type::StructureRef("B".to_string()),
            ])),
            D::Relation => NonDatum(Type::Relation),
            D::Applied => NonDatum(Type::Applied {
                name: "C".to_string(),
                args: vec![Type::Axis],
            }),
            D::Projection => NonDatum(Type::Projection {
                base: Box::new(Type::TypeParam("T".to_string())),
                member: "M".to_string(),
            }),
        }
    }

    /// Every `Resolved` row of the projection table evaluates, through
    /// reify-expr's public `eval_expr`, to a value of its codomain's kind, so a
    /// member typed here without an eval arm reds instead of evaluating to a
    /// silent `Undef` — the drift that left `Frame.yz_plane`/`.zx_plane` typed
    /// `Plane` while they evaluated `Undef`. Every `Type` variant has a
    /// [`receiver_fixture`], and a `NonDatum` one that starts resolving a
    /// projection reds, so a new receiver arm cannot skip the lockstep. Feature
    /// receivers (`Geometry`/`Selector`) are not evaluated: their projections
    /// are kernel-backed in reify-eval, not pure.
    #[test]
    fn every_resolved_datum_projection_evaluates_to_its_codomain_kind() {
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        for d in TypeDiscriminants::iter() {
            let fixture = receiver_fixture(d);
            let receiver_ty = fixture.ty();
            let resolved: Vec<(&str, Type)> = DATUM_PROJECTION_MEMBERS
                .iter()
                .filter_map(
                    |member| match datum_projection_result_type(receiver_ty, member) {
                        DatumProjectionResolution::Resolved(codomain) => Some((*member, codomain)),
                        _ => None,
                    },
                )
                .collect();
            match &fixture {
                ReceiverFixture::NonDatum(_) => assert!(
                    resolved.is_empty(),
                    "{receiver_ty} resolves {resolved:?}; give {d:?} a Pure or KernelBacked fixture"
                ),
                ReceiverFixture::KernelBacked(_) => assert!(
                    !resolved.is_empty(),
                    "{receiver_ty} is fixtured KernelBacked but resolves no projection"
                ),
                ReceiverFixture::Pure(receiver) => {
                    assert!(
                        !resolved.is_empty(),
                        "{receiver_ty} is fixtured Pure but resolves no projection"
                    );
                    for (member, codomain) in resolved {
                        let call = CompiledExpr::method_call(
                            receiver.clone(),
                            member.to_string(),
                            vec![],
                            codomain.clone(),
                        );
                        let value = eval_expr(&call, &ctx);
                        assert!(
                            has_codomain_kind(&value, &codomain),
                            "{receiver_ty}.{member} is typed {codomain} but evaluated to {value:?}"
                        );
                    }
                }
            }
        }
    }
}
