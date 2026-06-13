//! Feature → datum projection (geometric-relations ε).
//!
//! Builds the deduplicated bundle of datums a realized feature projects onto
//! (the "real missing bridge", PRD §7.2 / design §2.2): `feature.<projection>
//! : Datum`, total downward per the datum lattice. The provenance of a bundle
//! is the union of
//!
//!   * **analytic classification** — `BRepAdaptor_*` → `GeomAbs_*` → `.Axis()`
//!     extraction per sub-face / sub-edge (via the kernel's
//!     [`FaceAnalyticDatum`] / [`EdgeAnalyticDatum`] queries), and
//!   * **construction-history datum-traits** — `Revolute → Axis`,
//!     `Extruded → Direction` read from the topology attribute table,
//!
//! canonicalized by geometric equivalence (coaxial / coplanar / coincident
//! merge within `tol = max(confusion_floor, localTol(A), localTol(B))`).
//!
//! This module owns ε's datum-equivalence + dedup primitive so that ζ (which
//! depends on both γ and ε) can reuse the **same** primitive — fulfilling the
//! design §2.3 coherence law by shared code rather than duplicated magic
//! numbers.
//!
//! # Status
//!
//! Pre-2 scaffolding: this is the registered module path the ε GREEN steps
//! (6 / 8 / 10 / 12) fill in. The equivalence predicates (`axes_coaxial`,
//! `planes_coplanar`, `points_coincident`), the `Datum` carrier, `dedup_datums`,
//! `dedup_tolerance`, and `feature_datum_bundle` land in those steps alongside
//! their RED tests.
//!
//! [`FaceAnalyticDatum`]: reify_ir::GeometryQuery::FaceAnalyticDatum
//! [`EdgeAnalyticDatum`]: reify_ir::GeometryQuery::EdgeAnalyticDatum

use crate::sweep_classifier::SweptKind;
use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{GeometryHandleId, GeometryKernel, GeometryQuery, Value};

/// SI-metre fallback length scale used to convert the linear dedup tolerance
/// into a scale-aware angular tolerance (design §2.3:
/// `ang_tol ≈ lin_tol / characteristic_length`, deliberately **not** OCCT's
/// `Precision::Angular`).
///
/// The characteristic length is the separation of the two datums' reference
/// points: an angular misalignment `θ` between two would-be-coincident datums
/// produces a linear drift of about `separation · θ` at those points, so
/// allowing `lin_tol` of drift permits `θ ≤ lin_tol / separation`. When the
/// reference points are (near-)coincident the separation cannot supply the
/// scale, so it is floored at this value (1 m, the SI unit) instead of dividing
/// by zero. The companion point-on-line / point-on-plane *linear* check is the
/// precise positional arbiter; this angular term is the parallelism gate that
/// rejects datums meeting at a point but oriented differently.
const ANGULAR_REFERENCE_LENGTH_M: f64 = 1.0;

/// Lower bound the feature-datum dedup tolerance can never fall below: OCCT's
/// `Precision::Confusion()` (~0.1 µm), reused via the kernel-pinned
/// [`reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`] constant (itself asserted
/// equal to OCCT `Precision::Confusion()` by a reify-kernel-occt test) rather
/// than minting a fresh magic number — so ε and the kernel share one confusion
/// floor (design §2.3 coherence law).
const CONFUSION_FLOOR_M: f64 = reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M;

/// Pure geometric core of an axis datum (an infinite line): a reference point
/// and a direction, both in SI-metre kernel coordinates. The direction is a
/// *line orientation* — compared up to sign — not an oriented ray.
///
/// Distinct from the richer `Datum` carrier (step-8), which also retains the
/// projected radius / provenance; this is just the geometry the equivalence
/// predicates compute over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AxisGeom {
    pub(crate) origin: [f64; 3],
    pub(crate) direction: [f64; 3],
}

/// Pure geometric core of a plane datum: a reference point and a normal, both
/// in SI-metre kernel coordinates. The normal is compared up to sign (an
/// *unsigned* plane), so a plane and its flip are coplanar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaneGeom {
    pub(crate) origin: [f64; 3],
    pub(crate) normal: [f64; 3],
}

#[inline]
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn norm3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}

#[inline]
fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm3(sub3(a, b))
}

/// Normalize to unit length; a (near-)zero vector is returned unchanged so the
/// downstream checks degrade predictably rather than producing NaNs. Datum
/// directions arriving from the analytic FFI are already unit, so this is
/// defensive.
#[inline]
fn normalize3(a: [f64; 3]) -> [f64; 3] {
    let m = norm3(a);
    if m < f64::EPSILON {
        a
    } else {
        [a[0] / m, a[1] / m, a[2] / m]
    }
}

/// Scale-aware angular tolerance for a comparison whose reference points are
/// `separation` apart — see [`ANGULAR_REFERENCE_LENGTH_M`].
#[inline]
fn angular_tol(separation: f64, lin_tol: f64) -> f64 {
    lin_tol / separation.max(ANGULAR_REFERENCE_LENGTH_M)
}

/// Two axes are **coaxial** iff they lie on the same infinite line: their
/// directions are parallel *up to sign* (within the scale-aware angular tol)
/// AND each reference point lies on the other's line within `lin_tol`.
///
/// Sign-insensitive by construction — `|da × db|` ignores the relative sense —
/// which is the property B8 relies on (a revolved rectangle's two end-arc
/// circles carry opposite-sense axis directions on one shared line).
pub(crate) fn axes_coaxial(a: AxisGeom, b: AxisGeom, lin_tol: f64) -> bool {
    let da = normalize3(a.direction);
    let db = normalize3(b.direction);
    // (1) Directions parallel up to sign: |da × db| = sin(angle) for unit dirs.
    let separation = dist3(a.origin, b.origin);
    if norm3(cross3(da, db)) > angular_tol(separation, lin_tol) {
        return false;
    }
    // (2) Each reference point lies on the other's infinite line: the
    //     perpendicular distance |Δ × dir| (dir unit) is within the linear tol.
    let perp_b = norm3(cross3(sub3(b.origin, a.origin), da));
    let perp_a = norm3(cross3(sub3(a.origin, b.origin), db));
    perp_b <= lin_tol && perp_a <= lin_tol
}

/// Two planes are **coplanar** iff they are the same unsigned plane: their
/// normals are parallel *up to sign* (within the scale-aware angular tol) AND a
/// reference point of one lies on the other within `lin_tol` (the perpendicular
/// point-to-plane distance `|Δ · n|`, itself sign-insensitive).
pub(crate) fn planes_coplanar(a: PlaneGeom, b: PlaneGeom, lin_tol: f64) -> bool {
    let na = normalize3(a.normal);
    let nb = normalize3(b.normal);
    let separation = dist3(a.origin, b.origin);
    if norm3(cross3(na, nb)) > angular_tol(separation, lin_tol) {
        return false;
    }
    dot3(sub3(b.origin, a.origin), na).abs() <= lin_tol
}

/// Two points are **coincident** iff their separation is within `lin_tol`.
pub(crate) fn points_coincident(a: [f64; 3], b: [f64; 3], lin_tol: f64) -> bool {
    dist3(a, b) <= lin_tol
}

/// Two directions are **parallel up to sign** — the equivalence used to dedup
/// [`Datum::Direction`] candidates (e.g. several sub-faces reporting the same
/// extrusion direction). Positionless, so the angular tolerance falls back to
/// the [`ANGULAR_REFERENCE_LENGTH_M`] floor.
pub(crate) fn directions_parallel(a: [f64; 3], b: [f64; 3], lin_tol: f64) -> bool {
    let na = normalize3(a);
    let nb = normalize3(b);
    norm3(cross3(na, nb)) <= angular_tol(0.0, lin_tol)
}

/// A datum candidate in a feature's projection bundle (geometric-relations ε),
/// tagged by projection kind. Carries the pure geometry the equivalence
/// predicates compare, plus optional analytic scalar metadata threaded through
/// from the FFI (cylinder / circle `radius`).
///
/// Per design decision, `radius` (and, in later tasks, apex / half-angle) is
/// **retained in the bundle record but not consumed** by the `.axis` / `.plane`
/// / `.point` / `.dir` projections delivered here; it rides along for future
/// scalar projections (`cylinder.radius`).
///
/// A plane's signed offset is the derived quantity `origin · normal`, computed
/// on demand rather than stored as a redundant, drift-prone field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Datum {
    /// Axis / line datum — cylinder, cone, revolute history, or a
    /// line / circle / ellipse edge.
    Axis {
        origin: [f64; 3],
        direction: [f64; 3],
        /// Analytic radius (cylinder / circle), retained as bundle metadata;
        /// not read by any projection in this task — see the type-level note.
        #[allow(dead_code)]
        radius: Option<f64>,
    },
    /// Plane datum — a planar face.
    Plane { origin: [f64; 3], normal: [f64; 3] },
    /// Point datum — a sphere centre or vertex.
    Point { position: [f64; 3] },
    /// Direction datum — an extrusion direction.
    Direction { direction: [f64; 3] },
}

impl Datum {
    /// Two datums are equivalent iff they are the **same kind** and their
    /// geometry coincides under the step-6 equivalence at `lin_tol`. Different
    /// kinds never merge — dedup is kind-partitioned.
    fn equivalent_to(&self, other: &Datum, lin_tol: f64) -> bool {
        match (self, other) {
            (
                Datum::Axis {
                    origin: oa,
                    direction: da,
                    ..
                },
                Datum::Axis {
                    origin: ob,
                    direction: db,
                    ..
                },
            ) => axes_coaxial(
                AxisGeom {
                    origin: *oa,
                    direction: *da,
                },
                AxisGeom {
                    origin: *ob,
                    direction: *db,
                },
                lin_tol,
            ),
            (
                Datum::Plane {
                    origin: oa,
                    normal: na,
                },
                Datum::Plane {
                    origin: ob,
                    normal: nb,
                },
            ) => planes_coplanar(
                PlaneGeom {
                    origin: *oa,
                    normal: *na,
                },
                PlaneGeom {
                    origin: *ob,
                    normal: *nb,
                },
                lin_tol,
            ),
            (Datum::Point { position: pa }, Datum::Point { position: pb }) => {
                points_coincident(*pa, *pb, lin_tol)
            }
            (Datum::Direction { direction: da }, Datum::Direction { direction: db }) => {
                directions_parallel(*da, *db, lin_tol)
            }
            _ => false,
        }
    }
}

/// Deduplicate a datum bundle by geometric equivalence (design §2.3): cross-kind
/// candidates never merge, and within each kind candidates are canonicalized by
/// the step-6 equivalence at `lin_tol`, **first-representative-wins** — the
/// first occurrence is kept and later equivalents are dropped, preserving the
/// relative order of the survivors.
pub(crate) fn dedup_datums(datums: Vec<Datum>, lin_tol: f64) -> Vec<Datum> {
    let mut canonical: Vec<Datum> = Vec::new();
    for d in datums {
        if !canonical.iter().any(|c| c.equivalent_to(&d, lin_tol)) {
            canonical.push(d);
        }
    }
    canonical
}

/// Linear dedup tolerance for comparing two datums whose source sub-shapes carry
/// local modelling tolerances `local_a` / `local_b` (from the
/// `ShapeLocalTolerance` / `BRep_Tool::Tolerance` query): the three-way maximum
/// `max(confusion_floor, local_a, local_b)` (design §2.3 coherence law).
///
/// A clean model — both sub-shapes at/under the [`CONFUSION_FLOOR_M`] floor —
/// collapses to the floor, so machine-precision agreement still merges. A coarse
/// or imprecise sub-shape (large local tolerance) widens the comparison so its
/// own modelling drift does not spuriously split an otherwise-coincident datum.
pub(crate) fn dedup_tolerance(local_a: f64, local_b: f64) -> f64 {
    CONFUSION_FLOOR_M.max(local_a).max(local_b)
}

/// The deduplicated datum bundle a realized feature projects onto, grouped by
/// projection target (geometric-relations ε, design §2.2 / PRD §7.2).
///
/// Each group is already canonicalized by [`dedup_datums`] at the bundle's
/// dedup tolerance, so a group of length one is the feature's unambiguous datum
/// for that projection and a group of length ≠ one is the ambiguous /
/// select-a-subfeature case the `feature.<proj>` eval surfaces as a diagnostic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureDatumBundle {
    /// Axis / line datums (cylinder, cone, line/circle/ellipse edge, revolution
    /// history) → `feature.axis`.
    pub axes: Vec<Datum>,
    /// Plane datums (planar face) → `feature.plane`.
    pub planes: Vec<Datum>,
    /// Point datums (sphere centre, vertex) → `feature.point`.
    pub points: Vec<Datum>,
    /// Direction datums (extrusion direction) → `feature.dir`.
    pub directions: Vec<Datum>,
}

/// Build a realized feature's deduplicated datum bundle from the **union** of
/// analytic classification and construction-history datum-traits (design §2.2):
///
///  1. **analytic** — enumerate the feature's sub-faces / sub-edges
///     (`extract_faces` / `extract_edges`) and project each via the kernel's
///     [`FaceAnalyticDatum`] / [`EdgeAnalyticDatum`] queries. A non-analytic
///     sub-shape — e.g. a `GeomAbs_SurfaceOfRevolution` revolved side face —
///     fails its query and is skipped.
///  2. **construction history** — the recovered [`SweptKind`] contributes the
///     revolution **Axis** (`Revolve`) or extrusion **Direction** (`Extrude`),
///     restoring robustness for the non-analytic tail (PRD §7.2: "analytic ∪
///     construction-history").
///
/// Candidates are partitioned by projection target and each group is
/// deduplicated by geometric equivalence ([`dedup_datums`]) at
/// `lin_tol = max(confusion_floor, localTol of every queried sub-shape)` — a
/// bundle-wide fold of [`dedup_tolerance`] over each sub-shape's
/// `ShapeLocalTolerance`, with sub-shapes whose tolerance query fails falling
/// back to the confusion floor.
///
/// `history` is the feature's recovered swept-body classification (obtained by
/// the caller via `Engine::swept_kind_table().lookup(feature_handle)`), or
/// `None` when the feature is not a recognised swept body. The axis/direction
/// geometry lives in the `SweptKindTable` rather than the role-only
/// `TopologyAttributeTable`, so the recovered [`SweptKind`] — not the attribute
/// table — is ε's construction-history input.
///
/// [`FaceAnalyticDatum`]: reify_ir::GeometryQuery::FaceAnalyticDatum
/// [`EdgeAnalyticDatum`]: reify_ir::GeometryQuery::EdgeAnalyticDatum
pub fn feature_datum_bundle(
    feature_handle: GeometryHandleId,
    kernel: &mut dyn GeometryKernel,
    history: Option<&SweptKind>,
) -> FeatureDatumBundle {
    let mut candidates: Vec<Datum> = Vec::new();
    // Bundle-wide linear tolerance, folded from each queried sub-shape's local
    // modelling tolerance; seeded at the confusion floor.
    let mut lin_tol = dedup_tolerance(0.0, 0.0);

    // (1a) Analytic sub-FACE datums.
    if let Ok(faces) = kernel.extract_faces(feature_handle) {
        for f in faces {
            if let Ok(v) = kernel.query(&GeometryQuery::FaceAnalyticDatum(f)) {
                if let Some(d) = datum_from_value(&v) {
                    candidates.push(d);
                }
            }
            if let Ok(t) = kernel.query(&GeometryQuery::ShapeLocalTolerance(f)) {
                if let Some(tol) = t.as_f64() {
                    lin_tol = dedup_tolerance(lin_tol, tol);
                }
            }
        }
    }
    // (1b) Analytic sub-EDGE datums.
    if let Ok(edges) = kernel.extract_edges(feature_handle) {
        for e in edges {
            if let Ok(v) = kernel.query(&GeometryQuery::EdgeAnalyticDatum(e)) {
                if let Some(d) = datum_from_value(&v) {
                    candidates.push(d);
                }
            }
            if let Ok(t) = kernel.query(&GeometryQuery::ShapeLocalTolerance(e)) {
                if let Some(tol) = t.as_f64() {
                    lin_tol = dedup_tolerance(lin_tol, tol);
                }
            }
        }
    }
    // (2) Construction-history datum-traits (Revolve → Axis, Extrude → Direction).
    if let Some(kind) = history {
        candidates.extend(swept_kind_datum_traits(kind));
    }

    // (3) Dedup (kind-partitioned by `dedup_datums`), then group by target.
    let mut bundle = FeatureDatumBundle::default();
    for d in dedup_datums(candidates, lin_tol) {
        match d {
            Datum::Axis { .. } => bundle.axes.push(d),
            Datum::Plane { .. } => bundle.planes.push(d),
            Datum::Point { .. } => bundle.points.push(d),
            Datum::Direction { .. } => bundle.directions.push(d),
        }
    }
    bundle
}

/// Project a realized feature's deduplicated [`FeatureDatumBundle`] to the single
/// datum named by `member` — the resolve-time refinement the `feature.<proj>`
/// eval performs (geometric-relations ε, design §7.2). `member` is the projection
/// name the compiler lowered (`"axis"` / `"plane"` / `"point"` / `"dir"`).
///
/// Each bundle group is already canonicalized by geometric equivalence
/// ([`dedup_datums`]), so the group's length is the refinement's discriminant:
///
///   * **exactly one** candidate ⇒ the feature's unambiguous datum for that
///     projection, returned as its runtime [`Value`] carrier (`Value::Axis` /
///     `Plane` / `Point` / `Direction`) with NO diagnostic — the unambiguous arm
///     of the `Axis | Axis?` refinement; or
///   * **zero or many** ⇒ the ambiguous arm: a
///     [`DiagnosticCode::FeatureDatumAmbiguous`] `Severity::Error`
///     select-a-subfeature diagnostic is pushed and the projection evaluates to
///     [`Value::Undef`] (the runtime analogue of β's compile-time poison literal).
///
/// An unrecognised `member` also yields [`Value::Undef`] (no diagnostic): the
/// typing pass (`datum_projection_result_type`) rejects unknown members at
/// compile time with [`DiagnosticCode::DatumProjectionUnavailable`], so this arm
/// is unreachable in a well-typed program and is purely defensive.
pub fn feature_datum_projection(
    bundle: &FeatureDatumBundle,
    member: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    let group: &[Datum] = match member {
        "axis" => &bundle.axes,
        "plane" => &bundle.planes,
        "point" => &bundle.points,
        "dir" => &bundle.directions,
        // Unrecognised member: rejected at compile time; defensive Undef here.
        _ => return Value::Undef,
    };

    match group {
        [only] => datum_to_value(only),
        _ => {
            let detail = if group.is_empty() {
                format!("no {member} datum")
            } else {
                format!("{} candidate {member} datums", group.len())
            };
            diagnostics.push(
                Diagnostic::error(format!(
                    "ambiguous feature datum projection '.{member}': the feature carries \
                     {detail} — select a sub-feature to disambiguate"
                ))
                .with_code(DiagnosticCode::FeatureDatumAmbiguous),
            );
            Value::Undef
        }
    }
}

/// Convert a bundle [`Datum`] back into its runtime [`Value`] carrier — the
/// inverse of [`datum_from_value`], mirroring exactly the Value shapes the OCCT
/// `FaceAnalyticDatum` / `EdgeAnalyticDatum` dispatch composes (`Value::Axis`
/// origin = `Value::Point` of three `Length`s, direction = `Value::Direction`).
///
/// The projected scalar metadata (`radius`) is not carried by the datum `Value`
/// variants and is intentionally dropped here — the `.axis` / `.plane` /
/// `.point` / `.dir` projections consume only the geometry (see the `Datum` type
/// note); scalar projections (`cylinder.radius`) are a future task.
fn datum_to_value(d: &Datum) -> Value {
    match d {
        Datum::Axis {
            origin, direction, ..
        } => Value::Axis {
            origin: Box::new(point_value(*origin)),
            direction: Box::new(direction_value(*direction)),
        },
        Datum::Plane { origin, normal } => Value::Plane {
            origin: Box::new(point_value(*origin)),
            normal: Box::new(direction_value(*normal)),
        },
        Datum::Point { position } => point_value(*position),
        Datum::Direction { direction } => direction_value(*direction),
    }
}

/// A 3-component `Value::Point` of `Length`-dimensioned scalars (SI metres).
fn point_value(p: [f64; 3]) -> Value {
    Value::Point(vec![
        Value::length(p[0]),
        Value::length(p[1]),
        Value::length(p[2]),
    ])
}

/// A `Value::Direction` from inline `[f64; 3]` components.
fn direction_value(d: [f64; 3]) -> Value {
    Value::Direction {
        x: d[0],
        y: d[1],
        z: d[2],
    }
}

/// Convert a kernel-returned analytic-datum [`Value`] (as composed by the OCCT
/// `FaceAnalyticDatum` / `EdgeAnalyticDatum` dispatch) into a [`Datum`]. Returns
/// `None` for any value that is not a datum carrier or whose coordinates are not
/// numeric — defensive, since the analytic dispatch always produces well-formed
/// Axis / Plane / Point / Direction values.
fn datum_from_value(v: &Value) -> Option<Datum> {
    match v {
        Value::Axis { origin, direction } => Some(Datum::Axis {
            origin: point3_from_value(origin)?,
            direction: vec3_from_value(direction)?,
            radius: None,
        }),
        Value::Plane { origin, normal } => Some(Datum::Plane {
            origin: point3_from_value(origin)?,
            normal: vec3_from_value(normal)?,
        }),
        Value::Point(_) => Some(Datum::Point {
            position: point3_from_value(v)?,
        }),
        Value::Direction { x, y, z } => Some(Datum::Direction {
            direction: [*x, *y, *z],
        }),
        _ => None,
    }
}

/// Extract `[f64; 3]` SI-metre coordinates from a 3-component `Value::Point`
/// (length-dimensioned scalars) or `Value::Vector`.
fn point3_from_value(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => {
            Some([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?])
        }
        _ => None,
    }
}

/// Extract `[f64; 3]` components from a `Value::Direction` (inline floats) or,
/// defensively, a 3-component `Value::Vector` / `Value::Point`.
fn vec3_from_value(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Direction { x, y, z } => Some([*x, *y, *z]),
        Value::Vector(c) | Value::Point(c) if c.len() == 3 => {
            Some([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?])
        }
        _ => None,
    }
}

/// The construction-history datum-traits a recovered [`SweptKind`] contributes
/// to the bundle: a `Revolve` yields its revolution **Axis**; an `Extrude`
/// yields its extrusion **Direction**. A linear sweep (and any future
/// `#[non_exhaustive]` variant) contributes no first-class datum trait in ε.
fn swept_kind_datum_traits(kind: &SweptKind) -> Vec<Datum> {
    match kind {
        SweptKind::Revolve {
            axis_origin,
            axis_dir,
            ..
        } => vec![Datum::Axis {
            origin: *axis_origin,
            direction: *axis_dir,
            radius: None,
        }],
        SweptKind::Extrude { axis, .. } => vec![Datum::Direction { direction: *axis }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod equivalence_tests {
    use super::*;

    /// Linear dedup tolerance used across the equivalence tests (1 µm) — wide
    /// enough that the `1e-9` within-tolerance perturbations merge and the
    /// macroscopic (`>= 1`) offsets do not.
    const TOL: f64 = 1e-6;

    fn axis(origin: [f64; 3], direction: [f64; 3]) -> AxisGeom {
        AxisGeom { origin, direction }
    }

    fn plane(origin: [f64; 3], normal: [f64; 3]) -> PlaneGeom {
        PlaneGeom { origin, normal }
    }

    // ----- axes_coaxial -----------------------------------------------------

    #[test]
    fn coaxial_same_line_identical_sense() {
        // Both rays lie on the world Z axis, same direction sense, offset only
        // *along* the shared line — the canonical "same axis" case.
        assert!(axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn coaxial_same_line_opposite_sense() {
        // B8's load-bearing case: a revolved rectangle's two end-arc circles
        // share the revolution axis but with OPPOSITE direction sense. A
        // sign-sensitive test would leave two axes and fail B8.
        assert!(axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]),
            TOL,
        ));
    }

    #[test]
    fn coaxial_within_tolerance_offset_merges() {
        // Reference point off the shared line by < tol still counts as coaxial.
        assert!(axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([1e-9, 0.0, 5.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coaxial_parallel_but_offset() {
        // Identical direction, but the lines are 1 unit apart — distinct
        // parallel axes, must NOT merge.
        assert!(!axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coaxial_skew() {
        // Different direction AND offset origin — skew lines.
        assert!(!axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coaxial_crossing_at_origin_different_direction() {
        // Both lines pass through the origin (so a point-on-line check alone
        // would pass) but meet at 90° — the angular gate must reject them.
        assert!(!axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            TOL,
        ));
    }

    // ----- planes_coplanar --------------------------------------------------

    #[test]
    fn coplanar_same_normal_sense() {
        // Both the z = 0 plane, anchored at different in-plane points.
        assert!(planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([3.0, 4.0, 0.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn coplanar_opposite_normal_sense() {
        // Same z = 0 plane, opposite normal sense — coplanarity is unsigned.
        assert!(planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([3.0, 4.0, 0.0], [0.0, 0.0, -1.0]),
            TOL,
        ));
    }

    #[test]
    fn coplanar_within_tolerance_offset() {
        assert!(planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([3.0, 4.0, 1e-9], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coplanar_parallel_offset() {
        // Parallel planes z = 0 and z = 2 — equal normal, distinct offset.
        assert!(!planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coplanar_tilted() {
        // z = 0 plane vs y = 0 plane through the same point.
        assert!(!planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            TOL,
        ));
    }

    // ----- points_coincident ------------------------------------------------

    #[test]
    fn points_coincident_exact_and_within_tolerance() {
        assert!(points_coincident([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], TOL));
        assert!(points_coincident(
            [1.0, 2.0, 3.0],
            [1.0, 2.0, 3.0 + 1e-9],
            TOL,
        ));
    }

    #[test]
    fn points_not_coincident_beyond_tolerance() {
        assert!(!points_coincident([1.0, 2.0, 3.0], [1.0, 2.0, 4.0], TOL));
    }
}

#[cfg(test)]
mod dedup_tests {
    use super::*;

    const TOL: f64 = 1e-6;

    fn ax(origin: [f64; 3], direction: [f64; 3]) -> Datum {
        Datum::Axis {
            origin,
            direction,
            radius: None,
        }
    }

    fn pl(origin: [f64; 3], normal: [f64; 3]) -> Datum {
        Datum::Plane { origin, normal }
    }

    fn count_axes(ds: &[Datum]) -> usize {
        ds.iter().filter(|d| matches!(d, Datum::Axis { .. })).count()
    }

    fn count_planes(ds: &[Datum]) -> usize {
        ds.iter()
            .filter(|d| matches!(d, Datum::Plane { .. }))
            .count()
    }

    #[test]
    fn three_coaxial_axes_collapse_to_one() {
        // Three rays on the world Z axis — identical sense, opposite sense, and
        // offset-along-the-line — are one axis. This is B8's dedup in miniature
        // (the opposite-sense member is the revolved rectangle's far end-arc).
        let ds = dedup_datums(
            vec![
                ax([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                ax([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
                ax([0.0, 0.0, -3.0], [0.0, 0.0, 1.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 1, "three coaxial axes must collapse to one");
        assert_eq!(count_axes(&ds), 1);
    }

    #[test]
    fn distinct_axes_do_not_merge() {
        // A genuine disagreement: the Z axis vs an offset, perpendicular X axis.
        // Dedup must not paper over a real conflict by over-merging.
        let ds = dedup_datums(
            vec![
                ax([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                ax([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 2, "non-coaxial axes must stay distinct");
    }

    #[test]
    fn duplicate_coplanar_planes_collapse_to_one() {
        // Same z = 0 plane, opposite normal sense + different in-plane anchor.
        let ds = dedup_datums(
            vec![
                pl([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                pl([5.0, 7.0, 0.0], [0.0, 0.0, -1.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 1, "coplanar planes must collapse to one");
        assert_eq!(count_planes(&ds), 1);
    }

    #[test]
    fn axes_and_planes_never_cross_merge() {
        // An axis and a plane sharing origin + Z geometry are different kinds:
        // dedup is kind-partitioned, so both survive.
        let ds = dedup_datums(
            vec![
                ax([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                pl([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 2, "axis and plane must not cross-merge");
        assert_eq!(count_axes(&ds), 1);
        assert_eq!(count_planes(&ds), 1);
    }
}

#[cfg(test)]
mod dedup_tolerance_tests {
    use super::*;

    /// The confusion floor the dedup tolerance can never fall below: OCCT's
    /// `Precision::Confusion()` (~0.1 µm), reused via the kernel-pinned
    /// `reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` constant rather than a
    /// fresh magic number (design §2.3 coherence — the floor is the same value
    /// the kernel's `point_on_shape` / `contains` defaults already pin to OCCT).
    const FLOOR: f64 = reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M;

    #[test]
    fn clean_model_collapses_to_the_confusion_floor() {
        // Both sub-shapes carry sub-floor local tolerances (a clean analytic
        // primitive whose `BRep_Tool::Tolerance` sits at/under the confusion
        // floor): the dedup tolerance is pinned AT the floor, never below it.
        assert_eq!(dedup_tolerance(1e-12, 1e-12), FLOOR);
        assert_eq!(dedup_tolerance(0.0, 0.0), FLOOR);
    }

    #[test]
    fn one_large_local_tolerance_dominates() {
        // A coarse / imprecise sub-shape (large `BRep_Tool::Tolerance`) widens
        // the dedup tolerance above the floor: `max(floor, a, b)` = the large
        // one, regardless of argument order.
        assert_eq!(dedup_tolerance(1e-3, 1e-12), 1e-3);
        assert_eq!(dedup_tolerance(1e-12, 1e-3), 1e-3);
    }

    #[test]
    fn larger_of_two_above_floor_locals_wins() {
        // When both locals exceed the floor the larger dominates — the formula
        // is a plain three-way max, not a sum or an average.
        assert_eq!(dedup_tolerance(5e-4, 2e-3), 2e-3);
    }
}
