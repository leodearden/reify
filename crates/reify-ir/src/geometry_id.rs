//! Identity and naming vocabulary for kernel geometry: handle ids, feature
//! ids, topology roles and surface/curve kinds. Depends only on `reify_core`,
//! so `value` can name these types without importing `geometry` (INV-ARCH-1;
//! `docs/prds/v0_6/kernel-interface-typed-seam.md` §8 α₁).

use std::fmt;

use reify_core::hash::ContentHash;

/// Unique identifier for a geometry handle within a kernel session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeometryHandleId(pub u64);

impl GeometryHandleId {
    /// Sentinel value representing a failed geometry operation.
    ///
    /// Pushed into `step_handles` when `compile_geometry_op` returns `None`
    /// to maintain step index alignment so independent subsequent ops can
    /// still be attempted. No real geometry kernel allocates handle ID
    /// `u64::MAX` (kernels start from 1 and increment).
    pub const INVALID: GeometryHandleId = GeometryHandleId(u64::MAX);

    /// Compute a content hash for incremental caching.
    /// Domain-separated with tag byte [11] followed by the id as le_bytes.
    /// This serves as a proxy hash since OCCT shapes can't be hashed directly.
    pub fn content_hash(&self) -> ContentHash {
        debug_assert_ne!(self.0, u64::MAX, "INVALID handle must not be hashed");
        let mut buf = [0u8; 9];
        buf[0] = 11;
        buf[1..].copy_from_slice(&self.0.to_le_bytes());
        ContentHash::of(&buf)
    }
}

/// Typed kernel discriminator — re-exported from `reify_core`, which is the
/// single authoritative definition.
///
/// The canonical type lives at `reify_core::kernel::KernelId` and is re-exported
/// here so that existing paths (`reify_ir::KernelId`,
/// `reify_ir::geometry::KernelId`) continue to resolve, while reify-eval's
/// dispatcher and engine_build compile unchanged.
///
/// See `reify_core::kernel::KernelId` for full documentation, including the
/// determinism contract (derived `Ord` == dispatcher `BTreeMap` registry-name
/// iteration order) and the `#[non_exhaustive]` / `ALL` extensibility contract.
pub use reify_core::KernelId;

/// A geometry handle tagged with the kernel that produced it.
///
/// Pairs an opaque [`GeometryHandleId`] with the [`KernelId`] of the kernel
/// session that allocated it, so the executor can route a handle back to the
/// right kernel for follow-on operations. `Copy + Eq + Hash`, so it serves as
/// a `reify_eval::RealizationCache` value or a `HashMap` key/value directly.
/// Per-kernel trait methods keep operating on the bare
/// [`GeometryHandleId`]; the executor projects [`KernelHandle::id`] before a
/// trait call and re-wraps the result with that kernel's [`KernelId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelHandle {
    /// The kernel that produced (and owns) `id`.
    pub kernel: KernelId,
    /// The kernel-local geometry handle.
    pub id: GeometryHandleId,
}

#[cfg(test)]
mod kernel_id_tests {
    use super::*;

    /// `KernelHandle` pairs a `KernelId` with a `GeometryHandleId` and
    /// is a `Copy + Eq + Hash` value (usable as a `HashMap` key).
    #[test]
    fn kernel_handle_pairs_kernel_and_id() {
        use std::collections::HashMap;

        let h = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(5),
        };

        // Field access projects both halves of the pair.
        assert_eq!(h.kernel, KernelId::Manifold);
        assert_eq!(h.id, GeometryHandleId(5));

        // Copy semantics: `h` stays usable after being copied into `copy`.
        let copy = h;
        assert_eq!(copy, h);

        // Two identical handles are equal.
        let same = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(5),
        };
        assert_eq!(h, same);

        // Differing kernel OR id breaks equality.
        let diff_kernel = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(5),
        };
        let diff_id = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(6),
        };
        assert_ne!(h, diff_kernel);
        assert_ne!(h, diff_id);

        // Usable as a HashMap key (Eq + Hash): lookup by an equal handle hits,
        // a differing handle misses.
        let mut map: HashMap<KernelHandle, &str> = HashMap::new();
        map.insert(h, "bracket");
        assert_eq!(map.get(&same), Some(&"bracket"));
        assert_eq!(map.get(&diff_kernel), None);
        assert_eq!(map.get(&diff_id), None);
    }
}

/// Geometric kind of a face's underlying surface, matching OCCT's
/// `GeomAbs_*` taxonomy via `BRepAdaptor_Surface::GetType()`.
///
/// Returned (or implied via the `Value::String` wire format) by
/// [`GeometryQuery::FaceSurfaceKind`](crate::geometry::GeometryQuery::FaceSurfaceKind). Consumed by
/// `selector_vocabulary_v2::faces_by_surface_kind` to implement PRD
/// line 78's `%Plane`/`%Cylinder`/`%Cone`/`%Sphere`/`%Torus` slots.
///
/// The `BezierSurface`/`BSplineSurface` arms are kept distinct (rather
/// than collapsed under a generic `Spline`) because OCCT's classification
/// distinguishes them at the type level; the `OffsetSurface` arm is
/// preserved for completeness against the OCCT taxonomy. `Other` is the
/// safety-net arm for forward compatibility — a future OCCT version that
/// adds a new GeomAbs variant will surface as `Other` here, not silently
/// classify as one of the existing arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FaceSurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    BezierSurface,
    BSplineSurface,
    OffsetSurface,
    Other,
}

impl FaceSurfaceKind {
    /// Decode a canonical surface-kind name into a [`FaceSurfaceKind`].
    ///
    /// Mirrors the wire format produced by
    /// [`GeometryQuery::FaceSurfaceKind`](crate::geometry::GeometryQuery::FaceSurfaceKind) and consumed by
    /// `selector_vocabulary_v2::faces_by_surface_kind`. Returns the
    /// originating string (so callers can embed it in error diagnostics)
    /// when the name is not one of the documented canonical strings.
    pub fn try_from_str(s: &str) -> Result<Self, &str> {
        match s {
            "Plane" => Ok(FaceSurfaceKind::Plane),
            "Cylinder" => Ok(FaceSurfaceKind::Cylinder),
            "Cone" => Ok(FaceSurfaceKind::Cone),
            "Sphere" => Ok(FaceSurfaceKind::Sphere),
            "Torus" => Ok(FaceSurfaceKind::Torus),
            "BezierSurface" => Ok(FaceSurfaceKind::BezierSurface),
            "BSplineSurface" => Ok(FaceSurfaceKind::BSplineSurface),
            "OffsetSurface" => Ok(FaceSurfaceKind::OffsetSurface),
            "Other" => Ok(FaceSurfaceKind::Other),
            _ => Err(s),
        }
    }
}

/// Geometric kind of an edge's underlying curve, matching OCCT's
/// `GeomAbs_*` taxonomy via `BRepAdaptor_Curve::GetType()`.
///
/// Returned (or implied via the `Value::String` wire format) by
/// [`GeometryQuery::EdgeCurveKind`](crate::geometry::GeometryQuery::EdgeCurveKind). Consumed by
/// `selector_vocabulary_v2::edges_by_curve_kind` to implement PRD line
/// 78's `%Line`/`%Circle`/`%Ellipse`/etc. slots.
///
/// `Other` is the safety-net arm for forward compatibility against new
/// OCCT GeomAbs variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeCurveKind {
    Line,
    Circle,
    Ellipse,
    Hyperbola,
    Parabola,
    BezierCurve,
    BSplineCurve,
    OffsetCurve,
    Other,
}

impl EdgeCurveKind {
    /// Decode a canonical curve-kind name into an [`EdgeCurveKind`].
    ///
    /// Mirrors the wire format produced by
    /// [`GeometryQuery::EdgeCurveKind`](crate::geometry::GeometryQuery::EdgeCurveKind) and consumed by
    /// `selector_vocabulary_v2::edges_by_curve_kind`. Returns the
    /// originating string (so callers can embed it in error diagnostics)
    /// when the name is not one of the documented canonical strings.
    pub fn try_from_str(s: &str) -> Result<Self, &str> {
        match s {
            "Line" => Ok(EdgeCurveKind::Line),
            "Circle" => Ok(EdgeCurveKind::Circle),
            "Ellipse" => Ok(EdgeCurveKind::Ellipse),
            "Hyperbola" => Ok(EdgeCurveKind::Hyperbola),
            "Parabola" => Ok(EdgeCurveKind::Parabola),
            "BezierCurve" => Ok(EdgeCurveKind::BezierCurve),
            "BSplineCurve" => Ok(EdgeCurveKind::BSplineCurve),
            "OffsetCurve" => Ok(EdgeCurveKind::OffsetCurve),
            "Other" => Ok(EdgeCurveKind::Other),
            _ => Err(s),
        }
    }
}

/// Structured feature identifier for v0.2 persistent naming.
///
/// A `FeatureId` is either a *realization root* (a [`RealizationNodeId`], the
/// geometry output of an entity feature) or a *derived* feature built on top of
/// another `FeatureId` by a closed, append-only set of derivation kinds. The
/// recursion bottoms out at `Realization`; [`entity()`](FeatureId::entity) and
/// [`index()`](FeatureId::index) walk to that root. `Box` breaks the otherwise
/// infinite size of the recursive variant.
///
/// Replaces the former stringly-typed `FeatureId(String)` (task 4806 / P1 α,
/// PRD `docs/prds/naming-convergence/P1-structured-featureid-feature-value.md`).
/// `PartialEq`/`Eq`/`Hash` are `#[derive]`d — structural, recursing through the
/// `Box` — so identity is NEVER computed via the `Display` string.
///
/// [`RealizationNodeId`]: reify_core::identity::RealizationNodeId
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FeatureId {
    /// A realization root: the geometry output of an entity feature.
    Realization(reify_core::identity::RealizationNodeId),
    /// A feature derived from `base` by `kind` (e.g. a mid-surface).
    Derived {
        base: Box<FeatureId>,
        kind: DerivedKind,
    },
}

/// Closed, append-only set of [`FeatureId`] derivation kinds.
///
/// Intentionally has no `Other(String)` escape hatch — adding a derived kind is
/// a deliberate, compile-checked extension (mirroring the [`Role`] enum's
/// no-wildcard discipline). Today the only kind is `MidSurface`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DerivedKind {
    /// The mid-surface derived from a shell-extract parent.
    MidSurface,
}

impl FeatureId {
    /// Construct a realization-root `FeatureId` for `entity`'s realization `index`.
    ///
    /// Replaces the former stringly-typed `FeatureId::new(String)` constructor.
    pub fn realization(entity: impl Into<String>, index: u32) -> FeatureId {
        FeatureId::Realization(reify_core::identity::RealizationNodeId::new(entity, index))
    }

    /// The entity name of the realization root this feature derives from.
    ///
    /// Walks through any `Derived` wrappers to the `Realization` root.
    pub fn entity(&self) -> &str {
        match self {
            FeatureId::Realization(node) => &node.entity,
            FeatureId::Derived { base, .. } => base.entity(),
        }
    }

    /// The realization index of the realization root this feature derives from.
    ///
    /// Walks through any `Derived` wrappers to the `Realization` root.
    pub fn index(&self) -> u32 {
        match self {
            FeatureId::Realization(node) => node.index,
            FeatureId::Derived { base, .. } => base.index(),
        }
    }

    /// Derive the `FeatureId` for the mid-surface of `parent`.
    ///
    /// Wraps `parent` in `Derived { kind: MidSurface }`; the [`fmt::Display`]
    /// output is `<parent>/mid_surface`. Composition is well-defined: nesting
    /// yields a doubly-`Derived` value displaying as
    /// `<parent>/mid_surface/mid_surface`.
    ///
    /// Implements the derived-geometry naming sub-vocabulary from PRD
    /// `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20), built on
    /// top of the realization-based feature identity established by PRD
    /// `docs/prds/v0_2/persistent-naming-v2.md` line 33.
    pub fn derived_mid_surface(parent: &FeatureId) -> FeatureId {
        FeatureId::Derived {
            base: Box::new(parent.clone()),
            kind: DerivedKind::MidSurface,
        }
    }

    /// Stable byte encoding of this feature id for content-hashing.
    ///
    /// Byte 0 is an explicit per-variant discriminant: `Realization` appends a
    /// `u32`-LE length prefix + the entity bytes + the index `u32`-LE; `Derived`
    /// appends a `DerivedKind` tag byte then recurses through `base` (hence a
    /// variable-length `Vec<u8>`, unlike [`Role::content_hash_bytes`]'s fixed
    /// `[u8; 4]`). This is a deliberate replacement for a
    /// `format!("{self:?}")`/`Display`-derived hash input: neither the `Debug`
    /// string nor the `Display` grammar is a hashing contract, so a future
    /// rename (or a Display-grammar change) would silently change the hash and
    /// invalidate every cached selector resolution. The length prefix keeps the
    /// encoding unambiguous (e.g. entity `"Fo"` + index can never collide with a
    /// different entity/index split). Both matches are intentionally
    /// wildcard-free, so adding a `FeatureId` variant or a `DerivedKind` is a
    /// compile error here until a fresh discriminant is assigned.
    ///
    /// INVARIANT: never renumber an existing discriminant — only append new
    /// ones. The numbers below are a frozen serialization contract, not an
    /// enum-layout detail.
    pub(crate) fn content_hash_bytes(&self) -> Vec<u8> {
        fn derived_kind_tag(kind: DerivedKind) -> u8 {
            match kind {
                DerivedKind::MidSurface => 0x00,
            }
        }
        match self {
            FeatureId::Realization(node) => {
                let entity = node.entity.as_bytes();
                let mut bytes = Vec::with_capacity(1 + 4 + entity.len() + 4);
                bytes.push(0x00);
                bytes.extend_from_slice(&(entity.len() as u32).to_le_bytes());
                bytes.extend_from_slice(entity);
                bytes.extend_from_slice(&node.index.to_le_bytes());
                bytes
            }
            FeatureId::Derived { base, kind } => {
                let mut bytes = vec![0x01, derived_kind_tag(*kind)];
                bytes.extend_from_slice(&base.content_hash_bytes());
                bytes
            }
        }
    }
}

impl fmt::Display for FeatureId {
    /// Pinned OWNED grammar: the `Realization` root reuses
    /// [`RealizationNodeId`](reify_core::identity::RealizationNodeId)'s Display
    /// (`"<entity>#realization[<index>]"`) and each `Derived` step appends its
    /// per-kind suffix (`"/mid_surface"`). The `FromStr` round-trip test pins
    /// this grammar; changing it requires a codec `FORMAT_VERSION` bump.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureId::Realization(node) => write!(f, "{node}"),
            FeatureId::Derived { base, kind } => match kind {
                DerivedKind::MidSurface => write!(f, "{base}/mid_surface"),
            },
        }
    }
}

impl From<&reify_core::identity::RealizationNodeId> for FeatureId {
    /// Lossless: the realization node becomes the `Realization` root.
    fn from(id: &reify_core::identity::RealizationNodeId) -> Self {
        FeatureId::Realization(id.clone())
    }
}

/// Error returned by [`FeatureId`]'s [`FromStr`](std::str::FromStr) /
/// [`TryFrom<&str>`] impls when the input does not match the pinned
/// [`fmt::Display`](FeatureId) grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureIdParseError {
    /// The input was empty.
    Empty,
    /// The realization root did not match `"<entity>#realization[<index>]"`.
    BadRealization,
    /// A `"/<step>"` suffix named a derivation kind that is not recognized.
    UnknownDerivedKind,
    /// A derived step had no base feature to derive from (e.g. a bare
    /// `"/mid_surface"` with an empty prefix).
    Trailing,
}

impl fmt::Display for FeatureIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureIdParseError::Empty => write!(f, "feature id must not be empty"),
            FeatureIdParseError::BadRealization => {
                write!(f, "feature id root must be '<entity>#realization[<index>]'")
            }
            FeatureIdParseError::UnknownDerivedKind => {
                write!(f, "feature id has an unrecognized derived step")
            }
            FeatureIdParseError::Trailing => {
                write!(f, "feature id derived step has no base feature")
            }
        }
    }
}

impl std::error::Error for FeatureIdParseError {}

impl std::str::FromStr for FeatureId {
    type Err = FeatureIdParseError;

    /// Parse the pinned [`fmt::Display`](FeatureId) grammar back into a
    /// structured `FeatureId`. Right-peels a trailing `"/mid_surface"` step
    /// (wrapping the parsed prefix in `Derived { kind: MidSurface }`) and
    /// otherwise parses the residual root via [`RealizationNodeId`]'s `FromStr`.
    /// This is the exact inverse of `Display` (round-trip pinned by test).
    ///
    /// [`RealizationNodeId`]: reify_core::identity::RealizationNodeId
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(FeatureIdParseError::Empty);
        }
        if let Some(prefix) = s.strip_suffix("/mid_surface") {
            if prefix.is_empty() {
                return Err(FeatureIdParseError::Trailing);
            }
            let base = prefix.parse::<FeatureId>()?;
            return Ok(FeatureId::Derived {
                base: Box::new(base),
                kind: DerivedKind::MidSurface,
            });
        }
        // Base case: a bare realization root. A residual '/' here means an
        // unrecognized derived step (a realization entity never contains '/').
        if s.contains('/') {
            return Err(FeatureIdParseError::UnknownDerivedKind);
        }
        let node = s
            .parse::<reify_core::identity::RealizationNodeId>()
            .map_err(|_| FeatureIdParseError::BadRealization)?;
        Ok(FeatureId::Realization(node))
    }
}

impl TryFrom<&str> for FeatureId {
    type Error = FeatureIdParseError;

    /// Delegates to [`FromStr`](std::str::FromStr).
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

#[cfg(test)]
mod feature_id_tests {
    use super::*;

    // --- task 4806 (P1 α): FeatureId FromStr / TryFrom (B3 round-trip, B4 reject) ---

    #[test]
    fn feature_id_from_str_round_trips_display() {
        use std::str::FromStr;
        let cases = [
            FeatureId::realization("Foo", 3),
            FeatureId::derived_mid_surface(&FeatureId::realization("Foo", 3)),
            FeatureId::derived_mid_surface(&FeatureId::derived_mid_surface(
                &FeatureId::realization("Foo", 3),
            )),
        ];
        for x in cases {
            let s = x.to_string();
            // I2/B3: from_str is the exact inverse of Display.
            assert_eq!(FeatureId::from_str(&s), Ok(x.clone()), "from_str({s:?})");
            // TryFrom<&str> agrees with from_str.
            assert_eq!(
                FeatureId::try_from(s.as_str()),
                Ok(x.clone()),
                "try_from({s:?})"
            );
            // and .parse() (the FromStr entry point) too.
            assert_eq!(s.parse::<FeatureId>(), Ok(x));
        }
    }

    #[test]
    fn feature_id_from_str_rejects_malformed() {
        use std::str::FromStr;
        for bad in [
            "",                       // empty
            "Foo",                    // root not realization-shaped
            "Foo#realization[]",      // empty index
            "Foo/mid_surface",        // root not realization-shaped
            "x#realization[0]/bogus", // unknown derived step
        ] {
            assert!(FeatureId::from_str(bad).is_err(), "expected Err for {bad:?}");
            assert!(FeatureId::try_from(bad).is_err(), "expected Err for {bad:?}");
        }
    }

    #[test]
    fn feature_id_from_str_error_variants() {
        use std::str::FromStr;
        // Empty input.
        assert_eq!(FeatureId::from_str(""), Err(FeatureIdParseError::Empty));
        // A non-"/mid_surface" derived suffix is an unknown derived kind.
        assert_eq!(
            FeatureId::from_str("Foo#realization[0]/bogus"),
            Err(FeatureIdParseError::UnknownDerivedKind)
        );
        // A root that is not realization-shaped is a bad realization.
        assert_eq!(
            FeatureId::from_str("Foo"),
            Err(FeatureIdParseError::BadRealization)
        );
    }

    // --- task 4806 (P1 α): FeatureId::content_hash_bytes golden (B5) ---
    // A frozen, append-only serialization contract (mirrors Role::content_hash_bytes).
    // The expected bytes are derived directly from the pinned spec, not guessed.

    #[test]
    fn feature_id_content_hash_bytes_golden() {
        // Realization("Foo", 3): tag 0x00, u32-LE len=3, b"Foo", u32-LE index=3.
        let realization = FeatureId::realization("Foo", 3);
        let expected_realization: Vec<u8> = vec![
            0x00, // Realization variant tag
            0x03, 0x00, 0x00, 0x00, // entity length (u32-LE)
            b'F', b'o', b'o', // entity bytes
            0x03, 0x00, 0x00, 0x00, // realization index (u32-LE)
        ];
        assert_eq!(realization.content_hash_bytes(), expected_realization);

        // Derived(MidSurface) of the above: tag 0x01, DerivedKind tag 0x00,
        // then the Realization encoding recursively.
        let derived = FeatureId::derived_mid_surface(&realization);
        let mut expected_derived: Vec<u8> = vec![0x01, 0x00];
        expected_derived.extend_from_slice(&expected_realization);
        assert_eq!(derived.content_hash_bytes(), expected_derived);

        // Identical structures hash identically.
        assert_eq!(
            FeatureId::realization("Foo", 3).content_hash_bytes(),
            realization.content_hash_bytes()
        );

        // Changing the entity, the index, or wrapping in Derived changes the bytes.
        assert_ne!(
            FeatureId::realization("Bar", 3).content_hash_bytes(),
            realization.content_hash_bytes()
        );
        assert_ne!(
            FeatureId::realization("Foo", 4).content_hash_bytes(),
            realization.content_hash_bytes()
        );
        assert_ne!(derived.content_hash_bytes(), realization.content_hash_bytes());
    }
}

/// Per-axis sign discriminator for box-primitive corner vertices.
///
/// `Pos` selects the positive face along an axis, `Neg` the negative face.
/// Used for all three axes in `Role::CornerVertex { x, y, z }` to uniquely
/// name each of a box's 8 corners as a sign triple.
/// PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.1 (task α).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisSign {
    Pos,
    Neg,
}

/// Cap orientation for the `Role::Cap` variant.
///
/// Two semantic flavours of cap exist:
///   - `Top` / `Bottom`: gravitational orientation, used by extrude
///     (where `Top` is the swept-end face / `LastShape()` and `Bottom`
///     is the profile-as-placed / `FirstShape()`).
///   - `Start` / `End`: parametric sequence along a sweep parameter,
///     used by revolve (where `Start` is the profile at angle 0 /
///     `FirstShape()` and `End` is the profile at the angle endpoint /
///     `LastShape()`). For full-2π revolutions both caps collapse and
///     no `Cap` entries are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapKind {
    Top,
    Bottom,
    Start,
    End,
}

/// One entry in a topology entity's mod-history postfix.
///
/// Recorded each time a feature splits a parent topology entity into
/// children — `splitting_feature_id` is the FeatureId whose op caused
/// the split, and `split_index` distinguishes the resulting children
/// (PRD lines 60, 64).
///
/// Populated by `reify_eval::propagate_attributes_via_brepalgoapi_history`
/// when a parent topology entity is split into multiple result sub-shapes
/// (count > 1 across same-kind Modified ∪ Generated). Tasks 5/7/8 add
/// per-op coverage for sweeps/local-features/booleans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModEntry {
    pub splitting_feature_id: FeatureId,
    pub split_index: u32,
}

/// Role of a topology entity within its originating feature.
///
/// The minimal initial set per PRD line 56. Tasks 5-8 (sweeps, primitives,
/// local features, booleans) will add per-op variants here as a closed
/// extension — there is intentionally no `Other(String)` escape hatch so
/// that selector-resolution exhaustive matching remains auditable.
///
/// Per-op vocabulary:
///   - **Extrude** (`GeometryOp::Extrude`): `Cap(Top)` / `Cap(Bottom)` for
///     end faces, `Side` for lateral faces, `NewEdge` for cap-to-side edges.
///   - **Revolve** (`GeometryOp::Revolve`): `Cap(Start)` / `Cap(End)` for
///     profile faces of partial revolutions (omitted for full-2π),
///     `RevolvedFace` for lateral revolved faces, `AxisFace` reserved for
///     faces that touch the revolve axis (declared but not yet detected
///     by `populate_revolve_attributes`; see task 5a design decisions).
///   - **Sweep** (`GeometryOp::Sweep`): `Cap(Start)` / `Cap(End)` for
///     profile-as-placed and swept-end caps (single profile follows a
///     spine; parametric sequence along the spine), `SweptFace` for
///     lateral faces generated by sweeping a profile edge along the
///     spine (sweep convention; distinct from `Side` and `RevolvedFace`
///     so selectors can match per-op).
///   - **Loft** (`GeometryOp::Loft`): `Cap(Start)` / `Cap(End)` for the
///     first / last profile-section caps under `is_solid=true` (closed
///     ends of the multi-section solid), `LoftedFace` for lateral faces
///     generated between consecutive section profiles (loft convention;
///     distinct from `Side`, `RevolvedFace`, and `SweptFace` so selectors
///     can match per-op).
///   - **Mid-surface (derived geometry)**: `MidSurfaceFace` for per-region
///     mid-surface patches (`local_index = region.label`), `MidSurfaceEdge`
///     for inter-region adjacency curves (`local_index` = canonical sort
///     position of the `(min, max)` region pair). Emitted by
///     `reify_shell_extract::populate_mid_surface_attributes`; PRD
///     `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A cap face (`top`/`bottom` for extrude, `start`/`end` for revolve)
    /// of the feature.
    Cap(CapKind),
    /// A lateral side face of the feature (extrude convention).
    Side,
    /// An edge created by the feature's construction (e.g. cap-to-side
    /// boundary edges of an extrude).
    NewEdge,
    /// A revolved lateral face — a face generated by sweeping a profile
    /// edge around the revolve axis (revolve convention; distinct from
    /// `Side` so selectors can match per-op).
    RevolvedFace,
    /// A face that touches the revolve axis (e.g. zero-area axis-coincident
    /// face, or a face whose surface contains the axis). Reserved for
    /// detection in a follow-up task; not currently emitted by
    /// `populate_revolve_attributes` but declared here so the variant
    /// space is stable for selector vocabulary v2 (PRD line 102).
    AxisFace,
    /// A swept lateral face — a face generated by sweeping a profile
    /// edge along the sweep spine (sweep convention; distinct from
    /// `Side` and `RevolvedFace` so selectors can match per-op).
    /// Emitted by `populate_sweep_attributes` for `GeometryOp::Sweep`
    /// (task 5b, #2619).
    SweptFace,
    /// A lofted lateral face — a face generated between consecutive
    /// profile sections of a loft (loft convention; distinct from
    /// `Side`, `RevolvedFace`, and `SweptFace` so selectors can match
    /// per-op). Emitted by `populate_loft_attributes` for
    /// `GeometryOp::Loft` (task 5b, #2619).
    LoftedFace,
    /// A mid-surface patch corresponding to one segmentation region of
    /// a body's derived mid-surface (derived-geometry naming, PRD
    /// `docs/prds/v0_4/structural-analysis-shells.md` line 81).
    /// `local_index = region.label` (BFS-discovery order from
    /// `reify_shell_extract::segmentation`). Emitted by
    /// `reify_shell_extract::populate_mid_surface_attributes`.
    MidSurfaceFace,
    /// An inter-region adjacency edge of a body's derived mid-surface.
    /// `local_index` is the canonical sort position of the `(min, max)`
    /// region pair (ascending tuple order). Emitted by
    /// `reify_shell_extract::populate_mid_surface_attributes` from PRD
    /// `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20).
    MidSurfaceEdge,
    /// A corner vertex of a box primitive — uniquely identified by the
    /// three face-signs (±X, ±Y, ±Z) that meet at it. Produces 8 distinct
    /// values per box. Emitted by per-primitive vertex seeders (task C).
    ///
    /// PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.1 (task α).
    CornerVertex {
        x: AxisSign,
        y: AxisSign,
        z: AxisSign,
    },
    /// A corner vertex of a swept solid where a cap face meets the lateral
    /// envelope. Emitted by per-op vertex seeders for extrude / revolve /
    /// sweep / loft (task C). `face` records which cap (top/bottom for
    /// gravitational sweeps; start/end for parametric sweeps).
    ///
    /// PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.1 (task α).
    CapCornerVertex { face: CapKind },
    /// A face generated by a local-feature op — fillet/chamfer blend
    /// surface; distinct so provenance selectors can match
    /// created_by/split_by per creating feature.
    LocalFeatureFace,
}

impl Role {
    /// Stable byte encoding of this role for content-hashing (task 4536
    /// amendment, reviewer suggestion 4).
    ///
    /// Byte 0 is an explicit per-variant discriminant; bytes 1..4 encode the
    /// payload (`CapKind`/`AxisSign`), 0-padded for unit variants. This is a
    /// deliberate replacement for a `format!("{self:?}")`-derived hash input:
    /// the derived `Debug` string is NOT a serialization contract, so a future
    /// rename of a `Role` variant (or one of its `CapKind`/`AxisSign` payload
    /// variants) would silently change the hash and invalidate every cached
    /// selector resolution. The `match` is intentionally wildcard-free, so
    /// adding a `Role` variant is a compile error here until a fresh
    /// discriminant is assigned.
    ///
    /// INVARIANT: never renumber an existing discriminant — only append new
    /// ones. The numbers below are a frozen serialization contract, not an
    /// enum-layout detail.
    pub(crate) fn content_hash_bytes(&self) -> [u8; 4] {
        fn cap(k: CapKind) -> u8 {
            match k {
                CapKind::Top => 1,
                CapKind::Bottom => 2,
                CapKind::Start => 3,
                CapKind::End => 4,
            }
        }
        fn sign(s: AxisSign) -> u8 {
            match s {
                AxisSign::Pos => 1,
                AxisSign::Neg => 2,
            }
        }
        match self {
            Role::Cap(k) => [0, cap(*k), 0, 0],
            Role::Side => [1, 0, 0, 0],
            Role::NewEdge => [2, 0, 0, 0],
            Role::RevolvedFace => [3, 0, 0, 0],
            Role::AxisFace => [4, 0, 0, 0],
            Role::SweptFace => [5, 0, 0, 0],
            Role::LoftedFace => [6, 0, 0, 0],
            Role::MidSurfaceFace => [7, 0, 0, 0],
            Role::MidSurfaceEdge => [8, 0, 0, 0],
            Role::CornerVertex { x, y, z } => [9, sign(*x), sign(*y), sign(*z)],
            Role::CapCornerVertex { face } => [10, cap(*face), 0, 0],
            Role::LocalFeatureFace => [11, 0, 0, 0],
        }
    }
}

#[cfg(test)]
mod role_tests {
    use super::*;

    // --- task 4536 amendment: Role::content_hash_bytes stable encoding ---

    #[test]
    fn role_content_hash_bytes_are_distinct_deterministic_and_frozen() {
        use std::collections::HashSet;

        // One representative of every variant (every payload value enumerated).
        // This list is the proof that `content_hash_bytes` is injective across
        // the whole `Role` space — the property the selector content hash
        // relies on.
        let all = [
            Role::Cap(CapKind::Top),
            Role::Cap(CapKind::Bottom),
            Role::Cap(CapKind::Start),
            Role::Cap(CapKind::End),
            Role::Side,
            Role::NewEdge,
            Role::RevolvedFace,
            Role::AxisFace,
            Role::SweptFace,
            Role::LoftedFace,
            Role::MidSurfaceFace,
            Role::MidSurfaceEdge,
            Role::CornerVertex {
                x: AxisSign::Pos,
                y: AxisSign::Neg,
                z: AxisSign::Pos,
            },
            Role::CornerVertex {
                x: AxisSign::Neg,
                y: AxisSign::Pos,
                z: AxisSign::Neg,
            },
            Role::CapCornerVertex { face: CapKind::Top },
            Role::CapCornerVertex { face: CapKind::End },
            Role::LocalFeatureFace,
        ];

        // (1) Deterministic: the encoding does not depend on call-site state.
        for r in &all {
            assert_eq!(r.content_hash_bytes(), r.content_hash_bytes());
        }

        // (2) Injective: no two distinct Role values collide on their bytes.
        let mut seen: HashSet<[u8; 4]> = HashSet::new();
        for r in &all {
            assert!(
                seen.insert(r.content_hash_bytes()),
                "content_hash_bytes collision for {r:?}: {:?}",
                r.content_hash_bytes()
            );
        }

        // (3) Frozen contract for the roles this task's selectors address. These
        // exact bytes are a serialization contract — a deliberate edit, never an
        // incidental rename, may change them (see the INVARIANT doc).
        assert_eq!(Role::MidSurfaceFace.content_hash_bytes(), [7, 0, 0, 0]);
        assert_eq!(Role::MidSurfaceEdge.content_hash_bytes(), [8, 0, 0, 0]);
    }
}
