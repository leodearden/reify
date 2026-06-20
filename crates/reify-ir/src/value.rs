use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use reify_core::diagnostics::{DiagnosticCode, SourceSpan};
use reify_core::dimension::DimensionVector;
use crate::expr::CompiledExpr;
use reify_core::hash::ContentHash;
use reify_core::identity::ValueCellId;
use crate::persistent::PersistentMap;
use crate::structure_registry::StructureTypeId;
use reify_core::ty::SelectorKind;
use crate::geometry::Role;

// ── Float ordering strategy ───────────────────────────────────────────────────
//
// `f64::to_bits().cmp()` and `f64::total_cmp()` produce *different* total orders:
//
//   to_bits().cmp()  — treats the f64 bit pattern as a u64. The sign bit is the
//     MSB, so all negative floats (including -0.0) sort *above* all positive
//     values. Among negatives, larger magnitude → larger exponent → larger u64,
//     so more-negative values sort *higher*, not lower. NaN canonical bits
//     (0x7FF8_0000_0000_0000) sort after +Infinity (0x7FF0_0000_0000_0000).
//
//   total_cmp()      — implements IEEE 754 totalOrder. Negative floats sort
//     *below* positive values. -0.0 sorts just below +0.0. NaN still sorts
//     after +Infinity. This matches mathematical intuition.
//
// `Value::Real`, `Value::Scalar`, `Value::Complex`, and `Value::Orientation`
// all use `total_cmp()` in their `Ord` impls (see `impl Ord for Value`).
//
// Migration note: any persisted or long-lived `BTreeSet<Value>` or
// `BTreeMap<Value, _>` containing NaN or negative-float keys created under the
// old `to_bits()` ordering would have stale tree invariants and must be fully
// rebuilt before use.
// ─────────────────────────────────────────────────────────────────────────────

/// Spatial-grid shape stored on a [`SampledField`].
///
/// Determines how many axes are active and how the runtime extracts query
/// coordinates from a `sample(field, point)` argument:
/// - `Regular1D`: scalar coordinate (`Real` or `Scalar`).
/// - `Regular2D`: 2-component `Point` / `Vector` (or 2-element list).
/// - `Regular3D`: 3-component `Point` / `Vector` (or 3-element list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampledGridKind {
    Regular1D,
    Regular2D,
    Regular3D,
}

/// Language-level interpolation method declared in `interpolation = …` config.
///
/// This is a parallel enum to `reify_expr::interp::InterpolationMethod`; it
/// lives in `reify-types` because `Value::SampledField` carries it directly,
/// and `reify-types` cannot depend on `reify-expr` (would form a cycle).
/// The `From<InterpolationKind> for InterpolationMethod` mapping is defined
/// in `reify-expr/src/sampled.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterpolationKind {
    Linear,
    NearestNeighbor,
    Cubic,
    /// Deferred to post-v0.1; falls back to Linear at runtime with
    /// `W_INTERPOLATION_DEFERRED` warning emitted by `interp::resolve_method`.
    Rbf,
    /// Deferred to post-v0.1; falls back to Linear at runtime with
    /// `W_INTERPOLATION_DEFERRED` warning emitted by `interp::resolve_method`.
    Kriging,
}

/// Runtime payload for a v0.2 sampled field — the data behind
/// `field def F { source = sampled { … } }`.
///
/// Stored as `Arc<Value::SampledField>` under the `lambda` slot of a
/// `Value::Field { source: FieldSourceKind::Sampled, .. }`. Sample dispatch
/// in `crates/reify-expr/src/sampled.rs::sample_at_point` extracts query
/// coordinates from the argument point, detects out-of-bounds, and
/// delegates to `interp::interpolate_1d/2d/3d`.
///
/// # Once-per-field-per-session OOB warning
///
/// `oob_emitted` is an `AtomicBool` that backs the once-per-field
/// `W_FIELD_OUT_OF_BOUNDS` semantics (see
/// `crates/reify-types/src/diagnostics.rs::DiagnosticCode::FieldOutOfBounds`).
/// It is intentionally **excluded** from `PartialEq`, `Ord`, `Hash`, and
/// `content_hash`: it is a runtime-only state slot and not part of the
/// semantic content. The flag is reset whenever
/// `engine_eval::elaborate_field` constructs a fresh `SampledField`,
/// matching the spec's "per session" lifetime.
#[derive(Debug)]
pub struct SampledField {
    /// Field name (used in the `W_FIELD_OUT_OF_BOUNDS` diagnostic message).
    pub name: String,
    /// Spatial-grid shape — selects 1D / 2D / 3D dispatch in `sample_at_point`.
    pub kind: SampledGridKind,
    /// Per-axis lower bound (in SI units).  Length matches axis count
    /// (1 / 2 / 3 for `Regular1D` / `Regular2D` / `Regular3D`).
    pub bounds_min: Vec<f64>,
    /// Per-axis upper bound (in SI units).
    pub bounds_max: Vec<f64>,
    /// Per-axis grid spacing (in SI units).
    pub spacing: Vec<f64>,
    /// Per-axis grid coordinates (`linspace(bounds_min[i], bounds_max[i], spacing[i])`).
    /// Pre-computed at elaboration time so `sample_at_point` can pass slices
    /// directly to `interp::interpolate_Nd`.
    pub axis_grids: Vec<Vec<f64>>,
    /// Interpolation method declared in `interpolation = …` config.
    pub interpolation: InterpolationKind,
    /// Flat row-major data values (in SI units).  Length must equal the
    /// product `axis_grids[0].len() * axis_grids[1].len() * …`.
    pub data: Vec<f64>,
    /// Once-per-session OOB warning suppression flag.  Atomic so concurrent
    /// `sample()` calls (e.g. from a parallel snapshot evaluation) all see
    /// at-most-one warning.  Excluded from `PartialEq`/`Ord`/`Hash`/content_hash.
    ///
    /// **clippy::mutable_key_type note:** because `AtomicBool` has interior
    /// mutability, every `BTreeMap<Value, _>` site (notably `Value::Map`) is
    /// flagged by the `mutable_key_type` lint.  The flag is a runtime-only
    /// observability slot — it never participates in equality/ordering/hash
    /// — so the lint is suppressed at the crate level for crates that hold
    /// `Value`-keyed maps (see `#![allow(clippy::mutable_key_type)]` in
    /// `reify-types`, `reify-stdlib`, `reify-eval`, `reify-expr`,
    /// `reify-compiler`, `reify-constraints`, `reify-lsp`,
    /// `reify-test-support`).  Wrapping in `Arc` does NOT silence the
    /// lint (clippy traverses `Arc<T>` to inspect `T`).
    pub oob_emitted: std::sync::atomic::AtomicBool,
}

impl SampledField {
    /// Returns `true` if all spatial-geometry fields of `self` and `other` are
    /// bit-identical, i.e. the two fields sample the same physical grid.
    ///
    /// ## Relationship to `PartialEq`
    ///
    /// This is a strict subset of `SampledField::PartialEq`: it compares every
    /// field that `PartialEq` compares **except** `data` (the value payload,
    /// compared element-wise with tolerance by callers) and `oob_emitted` (a
    /// runtime-mutability slot deliberately excluded from all equality/ordering
    /// impls).  When only grid geometry matters — regardless of what data values
    /// happen to be stored at those coordinates — use this method.
    ///
    /// ## `#[doc(hidden)]` rationale
    ///
    /// This method is `pub` because its primary caller, `reify-eval`'s
    /// significance filter, lives in a downstream crate.  It is `#[doc(hidden)]`
    /// because it is an internal contract between the two crates and is not part
    /// of `SampledField`'s stable public API.
    ///
    /// ## Bit-equality rationale
    ///
    /// Float fields (`bounds_min`, `bounds_max`, `spacing`, `axis_grids`) are
    /// compared with `to_bits()` to match the behaviour of `PartialEq`.  This
    /// means `+0.0` and `-0.0` compare as **different** (a grid spec that
    /// switches sign on a spacing entry is a physically distinct grid even
    /// though the two values are numerically equal under `f64::PartialEq`).
    /// Same-bit-pattern NaN values compare as equal.
    #[doc(hidden)]
    pub fn grid_metadata_eq(&self, other: &Self) -> bool {
        // Destructure `self` so that adding a new field to `SampledField`
        // without updating this method produces a compile error.  `data` and
        // `oob_emitted` are bound to `_` because they are intentionally
        // excluded from the geometry-only comparison.
        let Self {
            name,
            kind,
            bounds_min,
            bounds_max,
            spacing,
            axis_grids,
            interpolation,
            data: _,
            oob_emitted: _,
        } = self;
        if name != &other.name || kind != &other.kind || interpolation != &other.interpolation {
            return false;
        }
        let vecs_bit_eq = |xs: &[f64], ys: &[f64]| -> bool {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .zip(ys.iter())
                    .all(|(x, y)| x.to_bits() == y.to_bits())
        };
        if !vecs_bit_eq(bounds_min, &other.bounds_min)
            || !vecs_bit_eq(bounds_max, &other.bounds_max)
            || !vecs_bit_eq(spacing, &other.spacing)
        {
            return false;
        }
        if axis_grids.len() != other.axis_grids.len() {
            return false;
        }
        axis_grids
            .iter()
            .zip(other.axis_grids.iter())
            .all(|(ag, bg)| vecs_bit_eq(ag, bg))
    }
}

impl Clone for SampledField {
    /// Cloning a `SampledField` produces a fresh `oob_emitted = false`.
    /// Cloning is rare in normal operation (the `Arc<Value::SampledField>`
    /// is shared); this impl exists primarily because `Value` derives
    /// `Clone` and propagates through every variant.
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            kind: self.kind,
            bounds_min: self.bounds_min.clone(),
            bounds_max: self.bounds_max.clone(),
            spacing: self.spacing.clone(),
            axis_grids: self.axis_grids.clone(),
            interpolation: self.interpolation,
            data: self.data.clone(),
            // Fresh runtime flag — clones get their own warning slot.
            oob_emitted: std::sync::atomic::AtomicBool::new(
                self.oob_emitted.load(std::sync::atomic::Ordering::Acquire),
            ),
        }
    }
}

impl PartialEq for SampledField {
    /// Compares all semantic content fields. Excludes `oob_emitted` because
    /// it is a runtime-mutability slot, NOT semantic content.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.kind == other.kind
            && self.bounds_min.len() == other.bounds_min.len()
            && self
                .bounds_min
                .iter()
                .zip(other.bounds_min.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits())
            && self.bounds_max.len() == other.bounds_max.len()
            && self
                .bounds_max
                .iter()
                .zip(other.bounds_max.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits())
            && self.spacing.len() == other.spacing.len()
            && self
                .spacing
                .iter()
                .zip(other.spacing.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits())
            && self.axis_grids.len() == other.axis_grids.len()
            && self
                .axis_grids
                .iter()
                .zip(other.axis_grids.iter())
                .all(|(a, b)| {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b.iter())
                            .all(|(x, y)| x.to_bits() == y.to_bits())
                })
            && self.interpolation == other.interpolation
            && self.data.len() == other.data.len()
            && self
                .data
                .iter()
                .zip(other.data.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

impl Eq for SampledField {}

impl PartialOrd for SampledField {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SampledField {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lexicographic over the semantic content fields, excluding oob_emitted.
        // Float comparisons use IEEE 754 total_cmp() — consistent with how
        // Value::Real / Value::Scalar are ordered elsewhere in this module.
        fn cmp_floats(a: &[f64], b: &[f64]) -> std::cmp::Ordering {
            a.len().cmp(&b.len()).then_with(|| {
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| x.total_cmp(y))
                    .find(|o| !o.is_eq())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        }
        self.name
            .cmp(&other.name)
            .then_with(|| format!("{:?}", self.kind).cmp(&format!("{:?}", other.kind)))
            .then_with(|| cmp_floats(&self.bounds_min, &other.bounds_min))
            .then_with(|| cmp_floats(&self.bounds_max, &other.bounds_max))
            .then_with(|| cmp_floats(&self.spacing, &other.spacing))
            .then_with(|| {
                self.axis_grids
                    .len()
                    .cmp(&other.axis_grids.len())
                    .then_with(|| {
                        self.axis_grids
                            .iter()
                            .zip(other.axis_grids.iter())
                            .map(|(a, b)| cmp_floats(a, b))
                            .find(|o| !o.is_eq())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            })
            .then_with(|| {
                format!("{:?}", self.interpolation).cmp(&format!("{:?}", other.interpolation))
            })
            .then_with(|| cmp_floats(&self.data, &other.data))
    }
}

/// The source kind of a field value at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldSourceKind {
    Analytical,
    Sampled,
    Composed,
    Imported,
    /// A field produced by the language-level `gradient()` operator, yielding
    /// the gradient of a parent scalar (or vector) field.  The stored `lambda`
    /// in the associated `Value::Field` holds the original source field; see
    /// `Value::Field.lambda` for the storage-layout contract.
    Gradient,
    /// A field produced by the language-level `divergence()` operator, yielding
    /// the divergence of a parent vector field.  The stored `lambda` in the
    /// associated `Value::Field` holds the original source field; see
    /// `Value::Field.lambda` for the storage-layout contract.
    Divergence,
    /// A field produced by the language-level `curl()` operator, yielding
    /// the curl of a parent vector field.  The stored `lambda` in the
    /// associated `Value::Field` holds the original source field; see
    /// `Value::Field.lambda` for the storage-layout contract.
    Curl,
    /// A field produced by the language-level `laplacian()` operator, yielding
    /// the Laplacian of a parent scalar field.  The stored `lambda` in the
    /// associated `Value::Field` holds the original source field; see
    /// `Value::Field.lambda` for the storage-layout contract.
    Laplacian,
    /// A field produced by the language-level `von_mises()` operator, yielding
    /// the von Mises stress of a parent tensor field.  The stored `lambda` in
    /// the associated `Value::Field` holds the original source field; see
    /// `Value::Field.lambda` for the storage-layout contract.
    VonMises,
    /// A field produced by the language-level `principal_stresses()` operator,
    /// yielding the principal stresses of a parent tensor field.  The stored
    /// `lambda` in the associated `Value::Field` holds the original source
    /// field; see `Value::Field.lambda` for the storage-layout contract.
    PrincipalStresses,
    /// A field produced by the language-level `max_shear()` operator, yielding
    /// the maximum shear stress of a parent tensor field.  The stored `lambda`
    /// in the associated `Value::Field` holds the original source field; see
    /// `Value::Field.lambda` for the storage-layout contract.
    MaxShear,
    /// A field produced by the language-level `safety_factor()` operator,
    /// yielding the safety factor of a parent tensor field with respect to a
    /// yield value.  The stored `lambda` in the associated `Value::Field` is a
    /// `Value::List` containing `[original_field, yield_val]`; see
    /// `Value::Field.lambda` for the storage-layout contract.
    SafetyFactor,
    /// A field produced by the language-level `restrict()` operator, confining
    /// sampling to a geometric region.  The stored `lambda` in the associated
    /// `Value::Field` is a `Value::List` containing `[inner_field, region]`
    /// where `inner_field` is sampled for points inside `region` and
    /// `Value::Undef` is returned for points outside.
    ///
    /// **α scaffold (task 4219)**: the `sample` dispatch returns `Value::Undef`
    /// unconditionally pending the OCCT point-in-region containment hook.
    /// Task δ implements `contains(region, point)` and changes the behaviour to
    /// `inside → sample_field_at(inner_field, at)` / `outside → Value::Undef`.
    Restricted,
    /// A heterogeneous FDM as-printed material field
    /// (`Field<Point3<Length>, AnisotropicMaterial>`) produced by the R-fast
    /// `fdm::as_printed_material_r_fast` ComputeNode (task δ).
    ///
    /// A structure codomain (`AnisotropicMaterial`) cannot be produced from a
    /// DSL-authored field lambda (those evaluate in a restricted scalar-only
    /// scope) nor stored in a `Value::SampledField` (its `data` is `Vec<f64>`,
    /// numeric-only), so this field is Rust-constructed with custom sample
    /// dispatch in `reify-expr::sample_field_at` — exactly mirroring the
    /// `Restricted`/`SafetyFactor`/`Composed` "data-in-lambda-slot + Rust
    /// dispatch" precedent.
    ///
    /// The stored `lambda` in the associated `Value::Field` is a `Value::List`
    /// holding the per-zone data needed to classify+select at sample time:
    /// `[aabb_min, aabb_max, params, cos_threshold, mat_wall, mat_skin,
    /// mat_infill]` (see `Value::Field.lambda` for the storage-layout
    /// contract). Sampling reconstructs the γ `AxisAlignedBox`/
    /// `ZoneProcessParams`, classifies the query point into Wall/Skin/Infill,
    /// and returns the matching precomputed `AnisotropicMaterial` value.
    ///
    /// Opaque to differential / reduction operators (gradient, divergence,
    /// extremum, …): like `Sampled`/`Restricted`, those degrade to
    /// `Value::Undef` for this kind.
    AsPrintedZones,
}

// ── Topology-Selector substrate (task 4116 / α) ────────────────────────────

/// Named reference to a realized geometry handle, carrying only the
/// stable identity fields (realization_ref, upstream_values_hash) plus the
/// ephemeral kernel_handle for convenience.
///
/// `kernel_handle` is **excluded** from [`SelectorValue::content_hash`] and
/// from Value-level equality/ordering (GHR-β §DD: same geometry rebuilt in a
/// new session must compare equal and hash identically).
///
/// ## Equality semantics
///
/// `PartialEq` deliberately excludes `kernel_handle`, matching the
/// content-hash contract.  Use field access (`a.kernel_handle`) if you need
/// to compare ephemeral handles directly.
#[derive(Clone, Debug)]
pub struct GeometryHandleRef {
    pub realization_ref: reify_core::identity::RealizationNodeId,
    pub upstream_values_hash: [u8; 32],
    /// `None` = symbolic/unrealized (eval-path mint, task #4652).
    /// `Some(id)` = live session-scoped kernel handle (build/realize path).
    pub kernel_handle: Option<crate::geometry::GeometryHandleId>,
}

impl PartialEq for GeometryHandleRef {
    /// Equality excludes `kernel_handle` (ephemeral, GHR-β §DD).
    fn eq(&self, other: &Self) -> bool {
        self.realization_ref == other.realization_ref
            && self.upstream_values_hash == other.upstream_values_hash
    }
}

impl Eq for GeometryHandleRef {}

impl GeometryHandleRef {
    /// Extract a `GeometryHandleRef` from a `Value::GeometryHandle`.
    /// Returns `None` for any other `Value` variant.
    pub fn from_geometry_handle(v: &Value) -> Option<Self> {
        match v {
            Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => Some(Self {
                realization_ref: realization_ref.clone(),
                upstream_values_hash: *upstream_values_hash,
                kernel_handle: *kernel_handle,
            }),
            _ => None,
        }
    }
}

/// Query that selects geometry elements from a realized handle (§4.2).
///
/// `Named` / `All` accept any [`SelectorKind`].
/// `ByNormal` / `ByArea` require [`SelectorKind::Face`].
/// `ByLength` / `ByHeight` / `ByParallel` require [`SelectorKind::Edge`].
/// `ByRole` requires the kind implied by its [`Role`] (MidSurfaceFace → Face,
/// MidSurfaceEdge → Edge; any other role accepts any kind).
#[derive(Clone, Debug, PartialEq)]
pub enum LeafQuery {
    /// Select by user-assigned label.  Accepts any kind.
    Named(String),
    /// Select all elements of the target kind.
    All,
    /// Select faces whose outward normal is within `tol_rad` of `dir`.
    ByNormal { dir: [f64; 3], tol_rad: f64 },
    /// Select faces whose area is in `[min_m2, max_m2]` (m²).
    ByArea { min_m2: f64, max_m2: f64 },
    /// Select edges whose length is in `[min_m, max_m]` (m).
    ByLength { min_m: f64, max_m: f64 },
    /// Select edges whose centroid z-coordinate is within `tol_m` of `z_m`.
    ByHeight { z_m: f64, tol_m: f64 },
    /// Select edges parallel to `axis` within `tol_rad`.
    ByParallel { axis: [f64; 3], tol_rad: f64 },
    /// Select elements carrying a derived-geometry [`Role`] attribute in the
    /// realized body's `TopologyAttributeTable` (task 4536). The motivating
    /// case is `mid_surface(body)` → `ByRole(Role::MidSurfaceFace)`, which
    /// resolves to the shell-extract mid-surface faces. Unlike the kernel-query
    /// leaves, resolution reads the attribute table (no kernel call), since the
    /// synthetic mid-surface ids are not enumerable via `extract_faces`.
    ByRole(Role),
}

impl LeafQuery {
    /// The [`SelectorKind`] this query requires, or `None` if it accepts any kind.
    pub fn required_kind(&self) -> Option<SelectorKind> {
        match self {
            LeafQuery::ByNormal { .. } | LeafQuery::ByArea { .. } => Some(SelectorKind::Face),
            LeafQuery::ByLength { .. }
            | LeafQuery::ByHeight { .. }
            | LeafQuery::ByParallel { .. } => Some(SelectorKind::Edge),
            // Attribute-role leaf (task 4536): the role implies the kind so
            // K1 kind-closure rejects e.g. an Edge selector carrying a
            // MidSurfaceFace leaf. Roles without a surfaced selector kind map
            // to None (accept any kind) — keeps the match total without
            // presuming a kind for roles not yet wired to a selector.
            LeafQuery::ByRole(Role::MidSurfaceFace) => Some(SelectorKind::Face),
            LeafQuery::ByRole(Role::MidSurfaceEdge) => Some(SelectorKind::Edge),
            LeafQuery::ByRole(_) => None,
            LeafQuery::Named(_) | LeafQuery::All => None,
        }
    }
}

/// Tree-shaped selector node composing geometry queries.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectorNode {
    Leaf {
        target: GeometryHandleRef,
        query: LeafQuery,
    },
    Union(Vec<SelectorValue>),
    Intersect(Vec<SelectorValue>),
    Difference(Box<SelectorValue>, Box<SelectorValue>),
}

/// A first-class topology-selector value pairing a [`SelectorKind`] with a
/// [`SelectorNode`] tree.  All constructors enforce kind-closure (K1).
///
/// ## Equality semantics
///
/// `PartialEq` delegates to [`content_hash`](Self::content_hash) so that
/// `sv_a == sv_b` is always consistent with
/// `Value::Selector(sv_a) == Value::Selector(sv_b)`.
/// This means:
/// * `kernel_handle` is excluded (ephemeral, GHR-β §DD).
/// * `Union`/`Intersect` children are order-independent (commutative sets;
///   hashed after sorting — see `compute_content_hash`).
/// * NaN-bearing float fields compare reflexively (NaN-canonicalized).
///
/// The content hash is computed once at construction time and cached, giving
/// O(1) equality and ordering comparisons.
#[derive(Clone)]
pub struct SelectorValue {
    pub kind: SelectorKind,
    pub node: SelectorNode,
    /// Cached content hash, computed eagerly in every constructor.
    /// Never construct this field manually — always go through the public
    /// constructors (`leaf`, `union`, `intersect`, `difference`) so the hash
    /// stays in sync with `kind` and `node`.
    hash: ContentHash,
}

impl std::fmt::Debug for SelectorValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Omit `hash` from Debug output — it's a derived cache field, not
        // part of the logical identity.
        f.debug_struct("SelectorValue")
            .field("kind", &self.kind)
            .field("node", &self.node)
            .finish()
    }
}

impl PartialEq for SelectorValue {
    /// Equality goes through `content_hash` so kernel_handle, NaN, and
    /// child ordering are all handled consistently with `Value::Selector`
    /// equality and `SelectorValue::content_hash`.
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for SelectorValue {}

impl SelectorValue {
    /// Construct a leaf selector.
    ///
    /// Fails with [`SelectorError::KindMismatch`] when
    /// `query.required_kind() == Some(k) && k != kind`.
    pub fn leaf(
        kind: SelectorKind,
        target: GeometryHandleRef,
        query: LeafQuery,
    ) -> Result<Self, SelectorError> {
        if let Some(required) = query.required_kind()
            && required != kind
        {
            return Err(SelectorError::KindMismatch {
                expected: required,
                found: kind,
            });
        }
        let node = SelectorNode::Leaf { target, query };
        let hash = Self::compute_content_hash(kind, &node);
        Ok(Self { kind, node, hash })
    }

    /// Construct a union of same-kind selectors.
    ///
    /// Fails with [`SelectorError::EmptyComposition`] if `children` is empty,
    /// or [`SelectorError::KindMismatch`] if any child has a different kind.
    pub fn union(children: Vec<SelectorValue>) -> Result<Self, SelectorError> {
        let kind = Self::check_composition(&children)?;
        let node = SelectorNode::Union(children);
        let hash = Self::compute_content_hash(kind, &node);
        Ok(Self { kind, node, hash })
    }

    /// Construct an intersection of same-kind selectors.
    ///
    /// Fails with [`SelectorError::EmptyComposition`] if `children` is empty,
    /// or [`SelectorError::KindMismatch`] if any child has a different kind.
    pub fn intersect(children: Vec<SelectorValue>) -> Result<Self, SelectorError> {
        let kind = Self::check_composition(&children)?;
        let node = SelectorNode::Intersect(children);
        let hash = Self::compute_content_hash(kind, &node);
        Ok(Self { kind, node, hash })
    }

    /// Construct a difference of two same-kind selectors.
    ///
    /// Fails with [`SelectorError::KindMismatch`] if `a.kind != b.kind`.
    pub fn difference(a: SelectorValue, b: SelectorValue) -> Result<Self, SelectorError> {
        if a.kind != b.kind {
            return Err(SelectorError::KindMismatch {
                expected: a.kind,
                found: b.kind,
            });
        }
        let kind = a.kind;
        let node = SelectorNode::Difference(Box::new(a), Box::new(b));
        let hash = Self::compute_content_hash(kind, &node);
        Ok(Self { kind, node, hash })
    }

    /// Validate that `children` is non-empty and all share the same kind.
    fn check_composition(children: &[SelectorValue]) -> Result<SelectorKind, SelectorError> {
        let first = children.first().ok_or(SelectorError::EmptyComposition)?;
        let kind = first.kind;
        for child in children.iter().skip(1) {
            if child.kind != kind {
                return Err(SelectorError::KindMismatch {
                    expected: kind,
                    found: child.kind,
                });
            }
        }
        Ok(kind)
    }

    /// Return the cached content hash for this selector.
    ///
    /// Computed once at construction time (O(direct children)); O(1) after
    /// that, making equality and ordering comparisons on `Value::Selector`
    /// values O(1) as well.
    ///
    /// Tag 30 (after AffineMap=29).  All f64 fields are NaN-canonicalized.
    /// `kernel_handle` inside leaf targets is excluded (GHR-β §DD).
    /// `Union`/`Intersect` child hashes are sorted before combining so
    /// commutative compositions always produce the same hash regardless of
    /// child order.
    pub fn content_hash(&self) -> ContentHash {
        self.hash
    }

    /// Compute the content hash from a `(kind, node)` pair.
    ///
    /// Called once from each constructor to populate `Self::hash`.
    /// `Union`/`Intersect` children are sorted by their own hash values so
    /// that the two operations are recognised as commutative set operations.
    fn compute_content_hash(kind: SelectorKind, node: &SelectorNode) -> ContentHash {
        fn nan_bits(v: f64) -> u64 {
            if v.is_nan() { f64::NAN.to_bits() } else { v.to_bits() }
        }

        fn hash_query(q: &LeafQuery) -> ContentHash {
            match q {
                LeafQuery::Named(s) => ContentHash::of(&[0u8]).combine(ContentHash::of_str(s)),
                LeafQuery::All => ContentHash::of(&[1u8]),
                LeafQuery::ByNormal { dir, tol_rad } => {
                    let mut buf = [0u8; 33]; // 1 + 4×8
                    buf[0] = 2;
                    buf[1..9].copy_from_slice(&nan_bits(dir[0]).to_le_bytes());
                    buf[9..17].copy_from_slice(&nan_bits(dir[1]).to_le_bytes());
                    buf[17..25].copy_from_slice(&nan_bits(dir[2]).to_le_bytes());
                    buf[25..33].copy_from_slice(&nan_bits(*tol_rad).to_le_bytes());
                    ContentHash::of(&buf)
                }
                LeafQuery::ByArea { min_m2, max_m2 } => {
                    let mut buf = [0u8; 17]; // 1 + 2×8
                    buf[0] = 3;
                    buf[1..9].copy_from_slice(&nan_bits(*min_m2).to_le_bytes());
                    buf[9..17].copy_from_slice(&nan_bits(*max_m2).to_le_bytes());
                    ContentHash::of(&buf)
                }
                LeafQuery::ByLength { min_m, max_m } => {
                    let mut buf = [0u8; 17]; // 1 + 2×8
                    buf[0] = 4;
                    buf[1..9].copy_from_slice(&nan_bits(*min_m).to_le_bytes());
                    buf[9..17].copy_from_slice(&nan_bits(*max_m).to_le_bytes());
                    ContentHash::of(&buf)
                }
                LeafQuery::ByHeight { z_m, tol_m } => {
                    let mut buf = [0u8; 17]; // 1 + 2×8
                    buf[0] = 5;
                    buf[1..9].copy_from_slice(&nan_bits(*z_m).to_le_bytes());
                    buf[9..17].copy_from_slice(&nan_bits(*tol_m).to_le_bytes());
                    ContentHash::of(&buf)
                }
                LeafQuery::ByParallel { axis, tol_rad } => {
                    let mut buf = [0u8; 33]; // 1 + 4×8
                    buf[0] = 6;
                    buf[1..9].copy_from_slice(&nan_bits(axis[0]).to_le_bytes());
                    buf[9..17].copy_from_slice(&nan_bits(axis[1]).to_le_bytes());
                    buf[17..25].copy_from_slice(&nan_bits(axis[2]).to_le_bytes());
                    buf[25..33].copy_from_slice(&nan_bits(*tol_rad).to_le_bytes());
                    ContentHash::of(&buf)
                }
                // Task 4536: fresh tag byte 7 (0–6 already taken by the leaves
                // above). The role is encoded via `Role::content_hash_bytes()`
                // — an explicit, frozen per-variant byte discriminant (NOT the
                // derived `Debug` string), so renaming a `Role` variant cannot
                // silently change a cached selector's content hash. See the
                // INVARIANT on `Role::content_hash_bytes` (reviewer suggestion 4).
                LeafQuery::ByRole(role) => {
                    ContentHash::of(&[7u8]).combine(ContentHash::of(&role.content_hash_bytes()))
                }
            }
        }

        fn hash_ghr(ghr: &GeometryHandleRef) -> ContentHash {
            // kernel_handle excluded (ephemeral, GHR-β §DD).
            ContentHash::of_str(&ghr.realization_ref.entity)
                .combine(ContentHash::of(&ghr.realization_ref.index.to_le_bytes()))
                .combine(ContentHash::of(&ghr.upstream_values_hash))
        }

        fn hash_node(node: &SelectorNode) -> ContentHash {
            match node {
                SelectorNode::Leaf { target, query } => ContentHash::of(&[0u8])
                    .combine(hash_ghr(target))
                    .combine(hash_query(query)),
                SelectorNode::Union(children) => {
                    // Sort child hashes before combining: Union is a commutative
                    // set operation, so union(vec![a,b]) == union(vec![b,a]).
                    let mut child_hashes: Vec<u128> =
                        children.iter().map(|c| c.content_hash().0).collect();
                    child_hashes.sort_unstable();
                    let mut h = ContentHash::of(&[1u8]);
                    h = h.combine(ContentHash::of(&(children.len() as u64).to_le_bytes()));
                    for ch in child_hashes {
                        h = h.combine(ContentHash(ch));
                    }
                    h
                }
                SelectorNode::Intersect(children) => {
                    // Same canonicalization as Union: Intersect is also commutative.
                    let mut child_hashes: Vec<u128> =
                        children.iter().map(|c| c.content_hash().0).collect();
                    child_hashes.sort_unstable();
                    let mut h = ContentHash::of(&[2u8]);
                    h = h.combine(ContentHash::of(&(children.len() as u64).to_le_bytes()));
                    for ch in child_hashes {
                        h = h.combine(ContentHash(ch));
                    }
                    h
                }
                SelectorNode::Difference(a, b) => ContentHash::of(&[3u8])
                    .combine(a.content_hash())
                    .combine(b.content_hash()),
            }
        }

        let kind_byte: u8 = match kind {
            SelectorKind::Face => 0,
            SelectorKind::Edge => 1,
            SelectorKind::Body => 2,
        };
        // tag=30
        ContentHash::of(&[30, kind_byte]).combine(hash_node(node))
    }
}

/// Errors from K1 (kind-closure) constructor validation.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectorError {
    /// The query requires `expected` but the selector has kind `found`.
    KindMismatch { expected: SelectorKind, found: SelectorKind },
    /// Union/Intersect was called with an empty children list.
    EmptyComposition,
}

impl std::fmt::Display for SelectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectorError::KindMismatch { expected, found } => {
                write!(f, "selector kind mismatch: expected {expected}, found {found}")
            }
            SelectorError::EmptyComposition => {
                write!(f, "union/intersect requires at least one child selector")
            }
        }
    }
}

impl std::error::Error for SelectorError {}

/// Runtime values in Reify (M1 subset).
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    /// Dimensioned scalar: value in SI base units, with dimension.
    Scalar {
        si_value: f64,
        dimension: DimensionVector,
    },
    /// Enum variant value: type_name::variant.
    Enum {
        type_name: String,
        variant: String,
    },
    /// Ordered list of values.
    List(Vec<Value>),
    /// Ordered set of unique values.
    ///
    /// Iteration order is governed by `impl Ord for Value`. Float-bearing variants
    /// (`Value::Real`, `Value::Scalar`, `Value::Complex`, `Value::Orientation`)
    /// use [`f64::total_cmp`], which places NaN strictly after `+∞`, `-0.0` before
    /// `+0.0`, and negatives mathematically below positives — the full IEEE 754
    /// totalOrder. See the *Float ordering strategy* comment block at the top of
    /// this module for the rationale (total_cmp vs. to_bits) and migration guidance.
    ///
    /// **Breaking-change warning:** any modification to `impl Ord for Value`
    /// invalidates the iteration order of every persisted or cached
    /// `BTreeSet<Value>` containing float-bearing elements. Consult the *Float
    /// ordering strategy* migration note before changing `Ord`.
    ///
    /// Because `content_hash()` folds over the `BTreeSet` iteration order,
    /// iteration-order stability is also a content-addressing invariant: any `Ord`
    /// change silently shifts `content_hash` for sets containing floats.
    ///
    /// # Example: round-trip preserves iteration order and content hash
    ///
    /// ```rust
    /// use std::collections::BTreeSet;
    /// use reify_ir::Value;
    ///
    /// // Construct a Value::Set containing all float boundary values.
    /// let boundary = [f64::NEG_INFINITY, -1.0_f64, -0.0_f64, 0.0_f64, 1.0_f64, f64::INFINITY, f64::NAN];
    /// let inner: BTreeSet<Value> = boundary.iter().map(|&v| Value::Real(v)).collect();
    /// let original = Value::Set(inner);
    /// let original_hash = original.content_hash();
    ///
    /// // Collect the iteration sequence, rebuild from it — order and hash must be stable.
    /// let seq: Vec<Value> = if let Value::Set(ref s) = original {
    ///     s.iter().cloned().collect()
    /// } else { unreachable!() };
    /// let rebuilt = Value::Set(seq.into_iter().collect());
    ///
    /// assert_eq!(rebuilt, original);
    /// assert_eq!(rebuilt.content_hash(), original_hash);
    /// ```
    Set(BTreeSet<Value>),
    /// Ordered map from values to values.
    ///
    /// Key iteration order follows `impl Ord for Value`. Float-bearing key variants
    /// (`Value::Real`, `Value::Scalar`, `Value::Complex`, `Value::Orientation`)
    /// use [`f64::total_cmp`], so NaN sorts strictly after `+∞`, `-0.0` before
    /// `+0.0`, and negatives mathematically below positives — IEEE 754 totalOrder.
    /// See the *Float ordering strategy* comment block at the top of this module
    /// for the rationale and migration guidance.
    ///
    /// **Breaking-change warning:** any modification to `impl Ord for Value`
    /// invalidates the key iteration order of every persisted or cached
    /// `BTreeMap<Value, _>` containing float-bearing keys. Consult the *Float
    /// ordering strategy* migration note before changing `Ord`.
    ///
    /// Because `content_hash()` folds over the `BTreeMap` key iteration order,
    /// key-ordering stability is also a content-addressing invariant: any `Ord`
    /// change silently shifts `content_hash` for maps with float-bearing keys.
    ///
    /// # Example: round-trip preserves key iteration order and content hash
    ///
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use reify_ir::Value;
    ///
    /// // Construct a Value::Map keyed by all float boundary values.
    /// let boundary = [f64::NEG_INFINITY, -1.0_f64, -0.0_f64, 0.0_f64, 1.0_f64, f64::INFINITY, f64::NAN];
    /// let inner: BTreeMap<Value, Value> = boundary
    ///     .iter()
    ///     .enumerate()
    ///     .map(|(i, &v)| (Value::Real(v), Value::Int(i as i64)))
    ///     .collect();
    /// let original = Value::Map(inner);
    /// let original_hash = original.content_hash();
    ///
    /// // Collect (key, value) pairs, rebuild from them — key order and hash must be stable.
    /// let pairs: Vec<(Value, Value)> = if let Value::Map(ref m) = original {
    ///     m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    /// } else { unreachable!() };
    /// let rebuilt = Value::Map(pairs.into_iter().collect());
    ///
    /// assert_eq!(rebuilt, original);
    /// assert_eq!(rebuilt.content_hash(), original_hash);
    /// ```
    Map(BTreeMap<Value, Value>),
    /// Optional value: Some(value) or None.
    Option(Option<Box<Value>>),
    /// Field value: a typed domain->codomain mapping with stored lambda/data.
    ///
    /// The `lambda` field stores one of four value kinds depending on `source`;
    /// see the `lambda` field doc for the full mapping.
    ///
    /// # Calling convention for `lambda`
    ///
    /// When `lambda` is a `Value::Lambda`, callers may invoke it with either:
    ///
    /// * **(i) `n` scalar arguments** of the domain's component type, where
    ///   `n` is the domain dimensionality derived from `domain_type`
    ///   (e.g. `Point { n, scalar }` → `n`; `Real` / `Scalar { .. }` → 1), or
    /// * **(ii) a single `Value::Point` argument** holding all `n` coordinates.
    ///
    /// Convention (ii) — the `single_point_param` path — applies when and only
    /// when `lambda.params.len() == 1 && n > 1`; otherwise convention (i)
    /// applies.  The source of `n` is `domain_type`.
    Field {
        domain_type: reify_core::ty::Type,
        codomain_type: reify_core::ty::Type,
        source: FieldSourceKind,
        /// The value stored for this field; valid contents depend on `source`:
        ///
        /// | `source` variant(s)                                                         | stored value         |
        /// |-----------------------------------------------------------------------------|----------------------|
        /// | `Analytical`                                                                | `Value::Lambda`      |
        /// | `Composed` (inline-compose form)                                            | `Value::Lambda`      |
        /// | `Composed` (callable-compose list form, task 4219 §5.2)                    | `Value::List[f, g]` where `f` is the outer field and `g` the inner field; `sample(composed, p) == f(g(p))` |
        /// | `Sampled`                                                                   | `Value::SampledField` (v0.2) |
        /// | `Imported`                                                                  | `Value::Undef`       |
        /// | `Gradient`, `Divergence`, `Curl`, `Laplacian`, `VonMises`, `PrincipalStresses`, `MaxShear` | `Value::Field` (the original source field) |
        /// | `SafetyFactor`                                                              | `Value::List` containing `[original_field, yield_val]` |
        /// | `Restricted` (task 4219 §5.3, scaffold)                                    | `Value::List[inner_field, region]`; task δ adds OCCT containment |
        /// | `AsPrintedZones` (task 3786 / FDM δ)                                       | `Value::List[aabb_min, aabb_max, params, cos_threshold, mat_wall, mat_skin, mat_infill]` — body AABB corners (Point3), the FDMProcess-derived zone params, the top/bottom-normal cosine threshold, and the 3 precomputed `AnisotropicMaterial` zone values |
        lambda: Arc<Value>,
    },
    /// Lambda closure: captures environment values and body expression.
    Lambda {
        params: Vec<(String, ValueCellId)>,
        body: Box<CompiledExpr>,
        captures: ValueMap,
    },
    /// Rank-r tensor: recursive nesting of Vec<Value> (innermost elements are scalars).
    Tensor(Vec<Value>),
    /// Geometric point with n components (all sharing the same dimension).
    Point(Vec<Value>),
    /// Geometric vector with n components (all sharing the same dimension).
    Vector(Vec<Value>),
    /// Complex number: re and im share one dimension (e.g., complex impedance in ohms).
    Complex {
        re: f64,
        im: f64,
        dimension: DimensionVector,
    },
    /// Orientation as a unit quaternion (w + xi + yj + zk).
    Orientation {
        w: f64,
        x: f64,
        y: f64,
        z: f64,
    },
    /// Coordinate frame: an origin point and a basis orientation.
    Frame {
        origin: Box<Value>,
        basis: Box<Value>,
    },
    /// Rigid-body transformation: a rotation (Orientation) and a translation (Vector).
    Transform {
        rotation: Box<Value>,
        translation: Box<Value>,
    },
    /// 3D plane: an origin Point3 and a unit normal Vector3 (dimensionless).
    Plane {
        origin: Box<Value>,
        normal: Box<Value>,
    },
    /// 3D axis (ray): an origin Point3 and a unit direction Vector3 (dimensionless).
    Axis {
        origin: Box<Value>,
        direction: Box<Value>,
    },
    /// Dimensionless 3D unit vector; distinct from Vector3<Length> and Orientation.
    ///
    /// Stores three inline dimensionless components (assumed unit-normalized),
    /// mirroring [`Value::Orientation`]'s inline-float layout. Produced by datum
    /// projections (`axis.dir`, `plane.normal`, `frame.x/.y/.z`).
    Direction { x: f64, y: f64, z: f64 },
    /// 3D axis-aligned bounding box: min and max corner Point3 values.
    BoundingBox {
        min: Box<Value>,
        max: Box<Value>,
    },
    /// Range with optional inclusive/exclusive bounds.
    Range {
        lower: Option<Box<Value>>,
        upper: Option<Box<Value>>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    },
    /// User-facing matrix literal (m rows × n cols).
    ///
    /// Before arithmetic evaluation, canonicalized to nested [`Value::Tensor`] (rank-2
    /// Tensor where each element is a Tensor row) via [`Value::canonicalize_matrix()`].
    /// The evaluator in `reify-expr` operates exclusively on the nested-Tensor
    /// representation for matrix arithmetic.
    Matrix(Vec<Vec<Value>>),
    /// Runtime payload for a v0.2 sampled field — see [`SampledField`].
    ///
    /// Stored under the `lambda` slot of a `Value::Field { source: FieldSourceKind::Sampled, .. }`.
    /// `engine_eval::elaborate_field` constructs a fresh `SampledField` per cold-start,
    /// so the per-field-per-session `oob_emitted` AtomicBool resets naturally.
    SampledField(SampledField),
    /// Instance of a `structure def` (e.g. `Steel_AISI_1045()`).
    ///
    /// `type_id` is an opaque per-Engine handle into the
    /// [`StructureRegistry`](crate::StructureRegistry) side-table (declared
    /// trait bounds, source span, field layout). `type_name` and `version`
    /// are carried inline so that [`content_hash`](Value::content_hash) — a
    /// pure function with no registry access — can compose a cache key that
    /// is stable across Engine restarts (PRD §5: key on *name*, never the
    /// ephemeral id; `@version(N)` bumps must invalidate). `fields` is the
    /// constructed parameter map (declaration-order-independent: hashing and
    /// equality sort by key).
    StructureInstance(Box<StructureInstanceData>),
    /// A realized geometry object produced by the kernel.
    ///
    /// `realization_ref` identifies the realization node in the topology graph
    /// (entity + slot index). `upstream_values_hash` is a content-hash of the
    /// parameter values that produced the geometry — it participates in equality
    /// and ordering so that cache-key stability is preserved across Engine
    /// restarts (PRD §5). `kernel_handle` is an ephemeral session-scoped id;
    /// it is intentionally excluded from `==` / `Ord` / `content_hash()` (GHR-β
    /// design decision: same geometry rebuilt in a new session must still compare
    /// equal and hash identically).
    GeometryHandle {
        realization_ref: reify_core::identity::RealizationNodeId,
        upstream_values_hash: [u8; 32],
        kernel_handle: Option<crate::geometry::GeometryHandleId>,
    },
    /// General 3D affine map x ↦ linear·x + translation.
    ///
    /// `linear` is dimensionless row-major 3×3; `translation` carries Length (meters).
    /// Stored inline (no `Box<Value>`) because the shape is fixed at 9+3 f64s.
    AffineMap {
        linear: [[f64; 3]; 3],
        translation: [f64; 3],
    },
    /// A first-class topology selector — see [`SelectorValue`] (task 4116 / α).
    Selector(SelectorValue),
    /// Undefined — not yet determined or computation failed.
    Undef,
}

/// Boxed payload of [`Value::StructureInstance`].
///
/// Boxed so the `Value` enum stays compact — inlining four fields (plus the
/// `PersistentMap` header) widened every `Value`-typed stack slot enough to
/// regress `reify-expr`'s 256-deep `eval_user_fn_recursion_depth_exceeded`
/// safety test into a real debug-mode stack overflow (the runtime guard at
/// `MAX_RECURSION_DEPTH` is sized for the pre-SIR-α frame). Boxing costs one
/// heap allocation per ctor call (rare path) and restores the lean frame.
#[derive(Debug, Clone)]
pub struct StructureInstanceData {
    pub type_id: StructureTypeId,
    pub type_name: String,
    pub version: u32,
    pub fields: PersistentMap<String, Value>,
}

/// Normalize range inclusivity flags: force `inclusive=false` when the
/// corresponding bound is `None` (unbounded endpoint cannot be inclusive).
fn normalize_range_flags<T>(
    lower: &Option<T>,
    upper: &Option<T>,
    lower_inclusive: bool,
    upper_inclusive: bool,
) -> (bool, bool) {
    (
        lower_inclusive && lower.is_some(),
        upper_inclusive && upper.is_some(),
    )
}

impl Value {
    /// Create a scalar with LENGTH dimension from a value in meters.
    pub fn length(meters: f64) -> Self {
        Value::Scalar {
            si_value: meters,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// Create a scalar with ANGLE dimension from a value in radians.
    pub fn angle(radians: f64) -> Self {
        Value::Scalar {
            si_value: radians,
            dimension: DimensionVector::ANGLE,
        }
    }

    /// Create a `Real` or `Scalar` from a raw f64 component and a dimension.
    ///
    /// Returns `Real(value)` when the dimension is dimensionless, or
    /// `Scalar { si_value: value, dimension }` otherwise.  This is the
    /// shared pattern used by complex component extraction (re, im) and
    /// magnitude computation.
    ///
    /// **NaN/Inf safety:** This function does NOT sanitize NaN/Inf inputs —
    /// callers should wrap the result in `sanitize_value()` if the input is
    /// arithmetically derived. The function preserves the caller's `f64`
    /// bit-exactly, which is the desired behaviour for accessor-style callers
    /// (e.g. `re(Complex{NaN, ...})` intentionally surfaces NaN so a later
    /// `sanitize_value` can convert it to `Undef`).
    pub fn from_real_scalar(value: f64, dimension: DimensionVector) -> Self {
        if dimension.is_dimensionless() {
            Value::Real(value)
        } else {
            Value::Scalar {
                si_value: value,
                dimension,
            }
        }
    }

    /// Create a Range value with normalized inclusivity flags.
    ///
    /// When a bound is `None` (unbounded), the corresponding inclusive flag is forced to
    /// `false` — infinity is never "included". This ensures that two logically identical
    /// ranges compare as equal and produce the same content hash regardless of which
    /// inclusive flag the caller passed.
    pub fn range(
        lower: Option<Value>,
        upper: Option<Value>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> Value {
        let (lower_inclusive, upper_inclusive) =
            normalize_range_flags(&lower, &upper, lower_inclusive, upper_inclusive);
        Value::Range {
            lower: lower.map(Box::new),
            upper: upper.map(Box::new),
            lower_inclusive,
            upper_inclusive,
        }
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Value::Undef)
    }

    /// Convert a `Value::Matrix` to nested `Value::Tensor` (rank-2 Tensor where each
    /// element is a Tensor row).  All other variants are returned unchanged.
    ///
    /// This is used by the evaluator in `reify-expr` to canonicalize matrix literals
    /// before dispatching to the arithmetic engine, which operates exclusively on the
    /// nested-Tensor representation.
    pub fn canonicalize_matrix(self) -> Self {
        match self {
            Value::Matrix(rows) => Value::Tensor(rows.into_iter().map(Value::Tensor).collect()),
            other => other,
        }
    }

    /// Convert a rank-2 nested `Value::Tensor` back to a `Value::Matrix`.
    ///
    /// Returns `Some(Matrix(...))` if `self` is a `Tensor` with at least one element
    /// and every element is itself a `Tensor`.  Returns `None` otherwise.
    pub fn try_into_matrix(self) -> Option<Self> {
        match self {
            // NB: Only Value::Tensor elements qualify as matrix rows. Point and Vector
            // are geometrically-typed Vec<Value> wrappers and are intentionally excluded —
            // a Tensor-of-Points is a point collection, not a matrix.
            Value::Tensor(rows)
                if !rows.is_empty() && rows.iter().all(|r| matches!(r, Value::Tensor(_))) =>
            {
                let matrix_rows: Vec<Vec<Value>> = rows
                    .into_iter()
                    .map(|r| match r {
                        Value::Tensor(elems) => elems,
                        _ => unreachable!("checked above"),
                    })
                    .collect();
                Some(Value::Matrix(matrix_rows))
            }
            _ => None,
        }
    }

    /// Get the f64 value if this is a numeric type.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Scalar { si_value, .. } => Some(*si_value),
            _ => None,
        }
    }

    /// Negate each component, returning Undef if any component negation fails.
    fn neg_components(components: Vec<Value>, wrap: fn(Vec<Value>) -> Value) -> Value {
        let results: Vec<Value> = components.into_iter().map(|c| -c).collect();
        if results.iter().any(|v| v.is_undef()) {
            Value::Undef
        } else {
            wrap(results)
        }
    }

    /// Get the dimension of this value (DIMENSIONLESS for non-scalar types).
    pub fn dimension(&self) -> DimensionVector {
        match self {
            Value::Scalar { dimension, .. } => *dimension,
            Value::Complex { dimension, .. } => *dimension,
            // Point/Vector: dimension derives from the first component (all components share one dimension).
            Value::Point(items) | Value::Vector(items) => items
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS),
            Value::Frame { .. } => DimensionVector::DIMENSIONLESS,
            // Direction is a dimensionless unit vector.
            Value::Direction { .. } => DimensionVector::DIMENSIONLESS,
            _ => DimensionVector::DIMENSIONLESS,
        }
    }

    /// Compute a content hash for incremental caching.
    ///
    /// # NaN canonicalization and hash-equality invariant exception
    ///
    /// All float-bearing variants (`Real`, `Scalar`, `Complex`, `Orientation`)
    /// canonicalize every NaN bit pattern to `f64::NAN.to_bits()` before hashing.
    /// This means two NaN values that carry different IEEE 754 payloads — and are
    /// therefore **not equal** under `PartialEq` (which uses `to_bits()`) — will
    /// produce **identical** content hashes.  In short:
    ///
    /// ```text
    /// a != b  (PartialEq)   yet   content_hash(a) == content_hash(b)   (NaN payloads)
    /// ```
    ///
    /// This is a **deliberate exception** to the usual hash-equality invariant
    /// (`a == b  ⟹  content_hash(a) == content_hash(b)`).  The rationale is
    /// content-addressed deduplication: NaN values that differ only in payload
    /// are semantically equivalent for caching purposes, so collapsing them avoids
    /// spurious cache misses.
    ///
    /// **Contrast with `-0.0`/`+0.0`**: those *do* maintain the invariant.
    /// `-0.0 != +0.0` under `PartialEq` AND their hashes differ (the `-0.0` bit
    /// pattern is preserved).  See `real_neg_zero_hash_differs_from_pos_zero`,
    /// `scalar_neg_zero_hash_differs_from_pos_zero`, and
    /// `hash_equality_invariant_real` for those tests.
    ///
    /// **Caller guidance**: when performing a content-addressed lookup for a
    /// NaN-bearing `Value`, treat a hash-hit as "possibly equal" and re-check
    /// `PartialEq` if exact bit-pattern identity of the NaN payload matters.
    /// See `nan_payload_hash_equality_invariant_exception` for the invariant
    /// exception test.
    ///
    /// **Known intentional exception — incremental cache**: the incremental
    /// evaluation cache (`CacheStore::record_evaluation` in
    /// `crates/reify-eval/src/cache.rs`) performs hash-only comparison for its
    /// early-cutoff check and does *not* follow the "re-check `PartialEq`"
    /// guidance above.  This is deliberate: two results that differ only in NaN
    /// payload are considered equivalent for the purposes of invalidating
    /// downstream nodes, so collapsing them via the canonical hash is the
    /// correct behaviour there, not a bug.
    pub fn content_hash(&self) -> ContentHash {
        // Content-hash tag registry (first byte of every ContentHash payload):
        // 0=Bool, 1=Int, 2=Real, 3=String, 4=Scalar, 5=Undef, 6=Enum, 7=List,
        // 8=Set, 9=Map, 10=Satisfaction(reserved), 11=Option, 12=Lambda, 13=Field,
        // 14=Tensor, 15=Complex, 16=Orientation, 17=Range, 18=Point, 19=Vector,
        // 20=Frame, 21=Transform, 22=Plane, 23=Axis, 24=BoundingBox, 25=Matrix,
        // 26=SampledField, 27=StructureInstance, 28=GeometryHandle, 29=AffineMap,
        // 30=Selector (task 4116 / α)
        match self {
            Value::Bool(b) => ContentHash::of(&[0, *b as u8]),
            Value::Int(i) => {
                let mut buf = [0u8; 9];
                buf[0] = 1;
                buf[1..].copy_from_slice(&i.to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::Real(r) => {
                let mut buf = [0u8; 9];
                buf[0] = 2;
                // Canonicalize NaN → collapses payload differences (see method doc for
                // invariant exception). Preserve -0.0 (PartialEq uses to_bits).
                let bits = if r.is_nan() {
                    f64::NAN.to_bits() // canonical NaN
                } else {
                    r.to_bits()
                };
                buf[1..].copy_from_slice(&bits.to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::String(s) => ContentHash::of(&[3]).combine(ContentHash::of_str(s)),
            Value::Scalar {
                si_value,
                dimension,
            } => {
                // Canonicalize NaN → collapses payload differences (see method doc for
                // invariant exception). Preserve -0.0 (PartialEq uses to_bits).
                let bits = if si_value.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    si_value.to_bits()
                };
                let mut buf = [0u8; 9];
                buf[0] = 4;
                buf[1..].copy_from_slice(&bits.to_le_bytes());
                ContentHash::of(&buf).combine(dimension.content_hash())
            }
            Value::Enum { type_name, variant } => ContentHash::of(&[6])
                .combine(ContentHash::of_str(type_name))
                .combine(ContentHash::of_str(variant)),
            Value::List(items) => {
                let mut h = ContentHash::of(&[7]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Set(items) => {
                let mut h = ContentHash::of(&[8]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Map(entries) => {
                let mut h = ContentHash::of(&[9]);
                h = h.combine(ContentHash::of(&(entries.len() as u64).to_le_bytes()));
                for (k, v) in entries {
                    h = h.combine(k.content_hash()).combine(v.content_hash());
                }
                h
            }
            Value::Option(inner) => match inner {
                // Tag [11] — tag [10] is exclusively reserved for Satisfaction
                None => ContentHash::of(&[11, 0]),
                Some(v) => ContentHash::of(&[11, 1]).combine(v.content_hash()),
            },
            Value::Field {
                domain_type,
                codomain_type,
                source,
                lambda,
            } => {
                let mut h = ContentHash::of(&[13]);
                h = h.combine(ContentHash::of_str(&format!("{}", domain_type)));
                h = h.combine(ContentHash::of_str(&format!("{}", codomain_type)));
                h = h.combine(ContentHash::of_str(&format!("{:?}", source)));
                h = h.combine(lambda.content_hash());
                h
            }
            Value::Lambda {
                params,
                body,
                captures,
            } => {
                let mut h = ContentHash::of(&[12]);
                h = h.combine(ContentHash::of(&(params.len() as u64).to_le_bytes()));
                for (name, id) in params {
                    h = h.combine(ContentHash::of_str(name));
                    h = h.combine(ContentHash::of_str(&format!("{}", id)));
                }
                h = h.combine(body.content_hash);
                for (id, val) in sorted_captures(captures) {
                    h = h.combine(ContentHash::of_str(&format!("{}", id)));
                    h = h.combine(val.content_hash());
                }
                h
            }
            Value::Tensor(items) => {
                let mut h = ContentHash::of(&[14]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Point(items) => {
                let mut h = ContentHash::of(&[18]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Vector(items) => {
                let mut h = ContentHash::of(&[19]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Complex { re, im, dimension } => {
                // tag=15; NaN canonicalization for both re and im → collapses payload differences
                // (see method doc for invariant exception); combine with dimension hash
                let re_bits = if re.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    re.to_bits()
                };
                let im_bits = if im.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    im.to_bits()
                };
                let mut buf = [0u8; 17];
                buf[0] = 15;
                buf[1..9].copy_from_slice(&re_bits.to_le_bytes());
                buf[9..17].copy_from_slice(&im_bits.to_le_bytes());
                ContentHash::of(&buf).combine(dimension.content_hash())
            }
            Value::Orientation { w, x, y, z } => {
                // tag=16; NaN canonicalization for all 4 components → collapses payload
                // differences (see method doc for invariant exception)
                let canon = |v: &f64| -> u64 {
                    if v.is_nan() {
                        f64::NAN.to_bits()
                    } else {
                        v.to_bits()
                    }
                };
                let mut buf = [0u8; 33];
                buf[0] = 16;
                buf[1..9].copy_from_slice(&canon(w).to_le_bytes());
                buf[9..17].copy_from_slice(&canon(x).to_le_bytes());
                buf[17..25].copy_from_slice(&canon(y).to_le_bytes());
                buf[25..33].copy_from_slice(&canon(z).to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::Frame { origin, basis } => {
                // tag=20; combine origin and basis content hashes
                ContentHash::of(&[20])
                    .combine(origin.content_hash())
                    .combine(basis.content_hash())
            }
            Value::Transform {
                rotation,
                translation,
            } => {
                // tag=21; combine rotation and translation content hashes
                ContentHash::of(&[21])
                    .combine(rotation.content_hash())
                    .combine(translation.content_hash())
            }
            Value::Plane { origin, normal } => {
                // tag=22; combine origin and normal content hashes
                ContentHash::of(&[22])
                    .combine(origin.content_hash())
                    .combine(normal.content_hash())
            }
            Value::Axis { origin, direction } => {
                // tag=23; combine origin and direction content hashes
                ContentHash::of(&[23])
                    .combine(origin.content_hash())
                    .combine(direction.content_hash())
            }
            Value::Direction { x, y, z } => {
                // tag=30; NaN canonicalization for all 3 components (mirrors
                // Orientation) → collapses NaN payload differences (see method doc
                // invariant exception).
                let canon = |v: &f64| -> u64 {
                    if v.is_nan() {
                        f64::NAN.to_bits()
                    } else {
                        v.to_bits()
                    }
                };
                let mut buf = [0u8; 25];
                buf[0] = 30;
                buf[1..9].copy_from_slice(&canon(x).to_le_bytes());
                buf[9..17].copy_from_slice(&canon(y).to_le_bytes());
                buf[17..25].copy_from_slice(&canon(z).to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::BoundingBox { min, max } => {
                // tag=24; combine min and max content hashes
                ContentHash::of(&[24])
                    .combine(min.content_hash())
                    .combine(max.content_hash())
            }
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                // Defensive re-normalization: None bounds → inclusive=false
                let (lower_inclusive, upper_inclusive) =
                    normalize_range_flags(lower, upper, *lower_inclusive, *upper_inclusive);
                // tag=17; flags then optional bounds
                let mut h = ContentHash::of(&[17, lower_inclusive as u8, upper_inclusive as u8]);
                match lower {
                    None => h = h.combine(ContentHash::of(&[0])),
                    Some(v) => h = h.combine(ContentHash::of(&[1])).combine(v.content_hash()),
                }
                match upper {
                    None => h = h.combine(ContentHash::of(&[0])),
                    Some(v) => h = h.combine(ContentHash::of(&[1])).combine(v.content_hash()),
                }
                h
            }
            Value::Matrix(rows) => {
                // tag=25; hash row count, then per-row col count + element hashes
                let mut h = ContentHash::of(&[25]);
                h = h.combine(ContentHash::of(&(rows.len() as u64).to_le_bytes()));
                for row in rows {
                    h = h.combine(ContentHash::of(&(row.len() as u64).to_le_bytes()));
                    for elem in row {
                        h = h.combine(elem.content_hash());
                    }
                }
                h
            }
            Value::SampledField(sf) => {
                // tag=26; combine name + kind + bounds + spacing + interpolation + data.
                // oob_emitted is intentionally excluded — it's a runtime-mutability
                // slot, not semantic content (see SampledField doc).
                let mut h = ContentHash::of(&[26]);
                h = h.combine(ContentHash::of(sf.name.as_bytes()));
                h = h.combine(ContentHash::of(format!("{:?}", sf.kind).as_bytes()));
                let hash_floats = |slice: &[f64]| -> ContentHash {
                    let mut hh = ContentHash::of(&(slice.len() as u64).to_le_bytes());
                    for f in slice {
                        // Canonicalize NaN payloads — match Value::Real / Value::Scalar policy.
                        let bits = if f.is_nan() {
                            f64::NAN.to_bits()
                        } else {
                            f.to_bits()
                        };
                        hh = hh.combine(ContentHash::of(&bits.to_le_bytes()));
                    }
                    hh
                };
                h = h.combine(hash_floats(&sf.bounds_min));
                h = h.combine(hash_floats(&sf.bounds_max));
                h = h.combine(hash_floats(&sf.spacing));
                h = h.combine(ContentHash::of(&(sf.axis_grids.len() as u64).to_le_bytes()));
                for grid in &sf.axis_grids {
                    h = h.combine(hash_floats(grid));
                }
                h = h.combine(ContentHash::of(
                    format!("{:?}", sf.interpolation).as_bytes(),
                ));
                h = h.combine(hash_floats(&sf.data));
                h
            }
            Value::StructureInstance(data) => {
                // tag=27; cross-Engine-stable identity: name + version + the
                // sorted-by-key field hashes. `type_id` is intentionally
                // excluded — it is a per-Engine ephemeral handle, while the
                // persistent cache key must survive Engine restarts (PRD §5).
                let mut h = ContentHash::of(&[27]);
                h = h.combine(ContentHash::of_str(&data.type_name));
                h = h.combine(ContentHash::of(&data.version.to_le_bytes()));
                let mut entries: Vec<(&String, &Value)> = data.fields.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                h = h.combine(ContentHash::of(&(entries.len() as u64).to_le_bytes()));
                for (k, v) in entries {
                    h = h.combine(ContentHash::of_str(k));
                    h = h.combine(v.content_hash());
                }
                h
            }
            Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                ..
            } => {
                // tag=28; cross-Engine-stable identity: realization entity +
                // index + upstream_values_hash. `kernel_handle` is intentionally
                // excluded — it is an ephemeral session-scoped id (GHR-β §DD).
                ContentHash::of(&[28])
                    .combine(ContentHash::of_str(&realization_ref.entity))
                    .combine(ContentHash::of(&realization_ref.index.to_le_bytes()))
                    .combine(ContentHash::of(upstream_values_hash))
            }
            Value::AffineMap { linear, translation } => {
                // tag=29; 97-byte buffer: 1 tag + 9×8 linear (row-major) + 3×8 translation.
                // NaN payload canonicalized (collapses bit-pattern differences; see doc).
                // neg-zero preserved via to_bits() (PartialEq uses bit-identity).
                let mut buf = [0u8; 97];
                buf[0] = 29;
                let mut offset = 1usize;
                for row in linear.iter() {
                    for &v in row.iter() {
                        let bits = if v.is_nan() { f64::NAN.to_bits() } else { v.to_bits() };
                        buf[offset..offset + 8].copy_from_slice(&bits.to_le_bytes());
                        offset += 8;
                    }
                }
                for &v in translation.iter() {
                    let bits = if v.is_nan() { f64::NAN.to_bits() } else { v.to_bits() };
                    buf[offset..offset + 8].copy_from_slice(&bits.to_le_bytes());
                    offset += 8;
                }
                ContentHash::of(&buf)
            }
            Value::Selector(sv) => sv.content_hash(), // tag=30; see SelectorValue::content_hash
            Value::Undef => ContentHash::of(&[5]),
        }
    }

    // --- Type-spine consolidated methods ---
    //
    // These methods centralise logic that previously lived as match-on-Value
    // blocks in downstream crates (builders, classifier, LSP analysis).
    // Adding a new Value variant now only requires updating value.rs (and
    // ty.rs for the corresponding Type variant), instead of editing 4+
    // files across 4 crates.

    /// Infer the [`Type`] of a runtime [`Value`].
    ///
    /// Used by test builders to derive a type from a literal value.
    /// For variants whose type cannot be fully inferred (Tensor, Matrix, Frame, Transform),
    /// this method panics — use `CompiledExpr::literal(value, type)` directly.
    ///
    /// For empty collections the following defaults apply (matching the compiler):
    /// - empty `List` / `Set` → element type defaults to `Real`
    /// - empty `Map` → key defaults to `String`, value defaults to `Real`
    /// - `Option(None)` → inner type defaults to `Bool`
    /// - `Range` with no bounds → element type defaults to `Real`
    ///
    /// Empty `Point` and `Vector` are not valid inputs to `infer_type` —
    /// debug builds trip a `debug_assert!`; release builds panic via
    /// `.expect()` (the assertion is compiled out), with a message that
    /// points back to the `debug_assert!`.  Use [`try_infer_type()`] for
    /// ambiguity-aware inference that returns `None` without panicking.
    ///
    /// Use [`try_infer_type()`] when you need to distinguish "genuinely ambiguous"
    /// from "has a known fallback".
    pub fn infer_type(&self) -> reify_core::ty::Type {
        use reify_core::ty::Type;
        match self.try_infer_type() {
            Some(ty) => ty,
            None => match self {
                Value::List(items) => {
                    // G-allow: documented `infer_type()` with-defaults contract (function
                    // docstring above); `try_infer_type()` returns None for ambiguity
                    // (task 3639 review).
                    let elem_ty = items.first().map(|v| v.infer_type()).unwrap_or(Type::dimensionless_scalar());
                    Type::List(Box::new(elem_ty))
                }
                Value::Set(items) => {
                    let elem_ty = items
                        .iter()
                        .next()
                        .map(|v| v.infer_type())
                        // G-allow: documented `infer_type()` with-defaults contract (function
                        // docstring above); `try_infer_type()` returns None for ambiguity
                        // (task 3639 review).
                        .unwrap_or(Type::dimensionless_scalar());
                    Type::Set(Box::new(elem_ty))
                }
                Value::Map(m) => {
                    let (k_ty, v_ty) = m
                        .iter()
                        .next()
                        .map(|(k, v)| (k.infer_type(), v.infer_type()))
                        .unwrap_or((Type::String, Type::dimensionless_scalar()));
                    Type::Map(Box::new(k_ty), Box::new(v_ty))
                }
                Value::Option(Some(inner)) => Type::Option(Box::new(inner.infer_type())),
                Value::Option(None) => Type::Option(Box::new(Type::Bool)),
                Value::Point(components) => {
                    debug_assert!(
                        !components.is_empty(),
                        "infer_type() called on empty Point — nonsensical for engineering \
                         geometry; use try_infer_type() if ambiguity-aware inference is \
                         required (task 3749)"
                    );
                    let first = components
                        .first()
                        .expect("infer_type() on empty Point — see debug_assert above (task 3749)");
                    Type::Point {
                        n: components.len(),
                        quantity: Box::new(first.infer_type()),
                    }
                }
                Value::Vector(components) => {
                    debug_assert!(
                        !components.is_empty(),
                        "infer_type() called on empty Vector — nonsensical for engineering \
                         geometry; use try_infer_type() if ambiguity-aware inference is \
                         required (task 3749)"
                    );
                    let first = components
                        .first()
                        .expect("infer_type() on empty Vector — see debug_assert above (task 3749)");
                    Type::Vector {
                        n: components.len(),
                        quantity: Box::new(first.infer_type()),
                    }
                }
                Value::Range { lower, upper, .. } => {
                    let elem_ty = lower
                        .as_ref()
                        .or(upper.as_ref())
                        .map(|v| v.infer_type())
                        // G-allow: documented `infer_type()` with-defaults contract (function
                        // docstring above); `try_infer_type()` returns None for ambiguity
                        // (task 3639 review).
                        .unwrap_or(Type::dimensionless_scalar());
                    Type::Range(Box::new(elem_ty))
                }
                Value::Tensor(_) => panic!(
                    "infer_type() cannot infer Tensor type (rank/n/quantity). \
                     Use CompiledExpr::literal(value, type) directly."
                ),
                Value::Matrix(_) => panic!(
                    "infer_type() cannot infer Matrix type. \
                     Use CompiledExpr::literal(value, type) directly."
                ),
                Value::Frame { .. } => panic!(
                    "infer_type() cannot infer Frame dimensionality. \
                     Use CompiledExpr::literal(value, type) directly."
                ),
                Value::Transform { .. } => panic!(
                    "infer_type() cannot infer Transform dimensionality. \
                     Use CompiledExpr::literal(value, type) directly."
                ),
                // try_infer_type() only returns None for the variants above
                // (List, Set, Map, Option(None), Option(Some(ambiguous_inner)),
                // Point(empty/ambiguous), Vector(empty/ambiguous),
                // Range(unbounded/ambiguous), Tensor, Matrix, Frame, Transform);
                // all other variants return Some, so this arm is unreachable.
                _ => unreachable!("try_infer_type returned None for an unexpected variant"),
            },
        }
    }

    /// Returns the type of this value if it can be unambiguously inferred,
    /// or `None` for genuinely ambiguous cases.
    ///
    /// Returns `None` for:
    /// - Empty `List`, `Set`, `Map` — element/key/value types unknown
    /// - `Option(None)` — inner type unknown
    /// - Empty `Point`, `Vector` — quantity type unknown
    /// - Fully-unbounded `Range` (both bounds `None`) — element type unknown
    /// - `Tensor`, `Matrix`, `Frame`, `Transform` — structurally uninferrable
    ///
    /// Ambiguity propagates recursively through container variants: if a
    /// non-empty `List` contains an element that is itself ambiguous (e.g.,
    /// `List([Option(None)])`), this method returns `None` rather than guessing
    /// a default. Use [`infer_type()`] if you want compiler-aligned defaults
    /// applied.
    pub fn try_infer_type(&self) -> Option<reify_core::ty::Type> {
        use reify_core::ty::Type;
        match self {
            Value::Bool(_) => Some(Type::Bool),
            Value::Int(_) => Some(Type::Int),
            Value::Real(_) => Some(Type::dimensionless_scalar()),
            Value::String(_) => Some(Type::String),
            Value::Scalar { dimension, .. } => Some(Type::Scalar {
                dimension: *dimension,
            }),
            Value::Enum { type_name, .. } => Some(Type::Enum(type_name.clone())),
            Value::List(items) => {
                let first = items.first()?;
                let elem_ty = first.try_infer_type()?;
                Some(Type::List(Box::new(elem_ty)))
            }
            Value::Set(items) => {
                let first = items.iter().next()?;
                let elem_ty = first.try_infer_type()?;
                Some(Type::Set(Box::new(elem_ty)))
            }
            Value::Map(m) => {
                let (k, v) = m.iter().next()?;
                let k_ty = k.try_infer_type()?;
                let v_ty = v.try_infer_type()?;
                Some(Type::Map(Box::new(k_ty), Box::new(v_ty)))
            }
            Value::Option(Some(inner)) => {
                let inner_ty = inner.try_infer_type()?;
                Some(Type::Option(Box::new(inner_ty)))
            }
            Value::Option(None) => None,
            Value::Lambda { params, body, .. } => {
                let param_types = params.iter().map(|_| Type::dimensionless_scalar()).collect();
                Some(Type::Function {
                    params: param_types,
                    return_type: Box::new(body.result_type.clone()),
                })
            }
            Value::Field {
                domain_type,
                codomain_type,
                ..
            } => Some(Type::Field {
                domain: Box::new(domain_type.clone()),
                codomain: Box::new(codomain_type.clone()),
            }),
            Value::Tensor(_) => None,
            Value::Complex { dimension, .. } => Some(Type::complex(Type::Scalar {
                dimension: *dimension,
            })),
            Value::Matrix(_) => None,
            Value::Point(components) => {
                let q = components.first()?.try_infer_type()?;
                Some(Type::Point {
                    n: components.len(),
                    quantity: Box::new(q),
                })
            }
            Value::Vector(components) => {
                let q = components.first()?.try_infer_type()?;
                Some(Type::Vector {
                    n: components.len(),
                    quantity: Box::new(q),
                })
            }
            Value::Orientation { .. } => Some(Type::Orientation(3)),
            Value::Frame { .. } => None,
            Value::Transform { .. } => None,
            Value::Plane { .. } => Some(Type::Plane),
            Value::Axis { .. } => Some(Type::Axis),
            Value::Direction { .. } => Some(Type::Direction),
            Value::BoundingBox { .. } => Some(Type::BoundingBox),
            Value::Range { lower, upper, .. } => {
                let bound = lower.as_ref().or(upper.as_ref())?;
                let elem_ty = bound.try_infer_type()?;
                Some(Type::Range(Box::new(elem_ty)))
            }
            // SampledField is the runtime payload stored under Value::Field.lambda
            // for source = sampled fields; it has no standalone Type.
            Value::SampledField(_) => None,
            Value::StructureInstance(data) => Some(Type::StructureRef(data.type_name.clone())),
            Value::GeometryHandle { .. } => Some(Type::Geometry),
            // Dimension is structurally 3 (fixed-size arrays) — deterministic, unlike Frame/Transform.
            Value::AffineMap { .. } => Some(Type::AffineMap(3)),
            Value::Selector(sv) => Some(Type::Selector(sv.kind)), // task 4116 / α
            Value::Undef => Some(Type::Bool),
        }
    }

    /// Returns `true` if this value is a numeric leaf for constraint
    /// domain classification (Int, Real, or Scalar).
    pub fn is_domain_numeric_leaf(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Real(_) | Value::Scalar { .. })
    }

    /// Returns `true` if this value is a logical leaf for constraint
    /// domain classification (Bool).
    pub fn is_domain_logical_leaf(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    /// Format this value for user-friendly display (e.g., hover tooltips).
    ///
    /// Unlike the [`Display`](std::fmt::Display) impl which shows raw
    /// dimension vectors, this method uses human-readable SI unit labels.
    pub fn format_hover(&self) -> String {
        match self {
            Value::Bool(b) => format!("{b}"),
            Value::Int(i) => format!("{i}"),
            Value::Real(r) => format!("{r}"),
            Value::String(s) => format!("\"{s}\""),
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let unit = dimension_unit_label(dimension);
                if unit.is_empty() {
                    format!("{si_value}")
                } else {
                    format!("{si_value} {unit}")
                }
            }
            Value::Enum { type_name, variant } => format!("{type_name}::{variant}"),
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(Value::format_hover).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Set(items) => {
                let inner: Vec<String> = items.iter().map(Value::format_hover).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Map(entries) => {
                let inner: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.format_hover(), v.format_hover()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Option(inner) => match inner {
                None => "none".to_string(),
                Some(v) => format!("some({})", v.format_hover()),
            },
            Value::Tensor(items) => {
                let inner: Vec<String> = items.iter().map(Value::format_hover).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Lambda { .. } => "<lambda>".to_string(),
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                format!("Field<{}, {}>({:?})", domain_type, codomain_type, source)
            }
            Value::Complex { re, im, dimension } => {
                let unit = dimension_unit_label(dimension);
                let (sign, im_abs) = if *im < 0.0 {
                    ("-", im.abs())
                } else {
                    ("+", *im)
                };
                if unit.is_empty() {
                    format!("{re} {sign} {im_abs}i")
                } else {
                    format!("{re} {sign} {im_abs}i {unit}")
                }
            }
            Value::Matrix(rows) => {
                let inner: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let cols: Vec<String> = row.iter().map(Value::format_hover).collect();
                        format!("[{}]", cols.join(", "))
                    })
                    .collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Point(components) => {
                let inner: Vec<String> = components.iter().map(Value::format_hover).collect();
                format!("Point({})", inner.join(", "))
            }
            Value::Vector(components) => {
                let inner: Vec<String> = components.iter().map(Value::format_hover).collect();
                format!("Vector({})", inner.join(", "))
            }
            Value::Orientation { w, x, y, z } => {
                format!("Orientation(w={w}, x={x}, y={y}, z={z})")
            }
            Value::Frame { origin, basis } => {
                format!(
                    "Frame(origin={}, basis={})",
                    origin.format_hover(),
                    basis.format_hover()
                )
            }
            Value::Transform {
                rotation,
                translation,
            } => {
                format!(
                    "Transform(rotation={}, translation={})",
                    rotation.format_hover(),
                    translation.format_hover()
                )
            }
            Value::Plane { origin, normal } => {
                format!(
                    "Plane(origin={}, normal={})",
                    origin.format_hover(),
                    normal.format_hover()
                )
            }
            Value::Axis { origin, direction } => {
                format!(
                    "Axis(origin={}, direction={})",
                    origin.format_hover(),
                    direction.format_hover()
                )
            }
            Value::Direction { x, y, z } => {
                format!("Direction(x={x}, y={y}, z={z})")
            }
            Value::BoundingBox { min, max } => {
                format!(
                    "BoundingBox(min={}, max={})",
                    min.format_hover(),
                    max.format_hover()
                )
            }
            Value::Range { lower, upper, .. } => {
                let lo = lower
                    .as_ref()
                    .map(|v| v.format_hover())
                    .unwrap_or_else(|| "..".to_string());
                let hi = upper
                    .as_ref()
                    .map(|v| v.format_hover())
                    .unwrap_or_else(|| "..".to_string());
                format!("{lo}..{hi}")
            }
            Value::SampledField(sf) => format!(
                "SampledField('{}', {:?}, {} samples)",
                sf.name,
                sf.kind,
                sf.data.len()
            ),
            Value::StructureInstance(data) => {
                let mut entries: Vec<(&String, &Value)> = data.fields.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let type_name = &data.type_name;
                if entries.is_empty() {
                    format!("{type_name} {{ }}")
                } else {
                    let inner: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{k}: {}", v.format_hover()))
                        .collect();
                    format!("{type_name} {{ {} }}", inner.join(", "))
                }
            }
            Value::GeometryHandle { realization_ref, .. } => {
                format!("<Geometry: {realization_ref}>")
            }
            Value::AffineMap { linear, translation } => {
                format!(
                    "AffineMap(linear=[[{}, {}, {}], [{}, {}, {}], [{}, {}, {}]], translation=[{}, {}, {}])",
                    linear[0][0], linear[0][1], linear[0][2],
                    linear[1][0], linear[1][1], linear[1][2],
                    linear[2][0], linear[2][1], linear[2][2],
                    translation[0], translation[1], translation[2],
                )
            }
            Value::Selector(sv) => format!("Selector({})", sv.kind),
            Value::Undef => "(undefined)".to_string(),
        }
    }

    /// Format this value for GUI display, returning only the display string.
    ///
    /// For Scalar and Complex, the unit is discarded. Use [`format_display_pair`](Value::format_display_pair)
    /// directly when the unit must be preserved.
    ///
    /// This avoids the unnecessary `String::new()` allocations that `format_display_pair().0`
    /// would create on every recursive call inside composite types.
    pub fn format_display(&self) -> String {
        match self {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let (display_value, _unit) = dimension.to_display_units(*si_value);
                format_display_number(display_value)
            }
            Value::Int(i) => i.to_string(),
            Value::Real(r) => format_display_number(*r),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => s.clone(),
            Value::Enum { variant, .. } => variant.clone(),
            Value::List(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("[{}]", strs.join(", "))
            }
            Value::Set(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("set{{{}}}", strs.join(", "))
            }
            Value::Map(entries) => {
                let strs: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{} => {}", k.format_display(), v.format_display()))
                    .collect();
                format!("map{{{}}}", strs.join(", "))
            }
            Value::Option(opt) => match opt {
                Some(inner) => inner.format_display(),
                None => "none".to_string(),
            },
            Value::Lambda { .. } => "<lambda>".to_string(),
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => format!("Field<{}, {}>({:?})", domain_type, codomain_type, source),
            Value::Tensor(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("[{}]", strs.join(", "))
            }
            Value::Point(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("point({})", strs.join(", "))
            }
            Value::Vector(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("vec({})", strs.join(", "))
            }
            Value::Matrix(rows) => {
                let row_strs: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let inner: Vec<String> = row.iter().map(|v| v.format_display()).collect();
                        format!("[{}]", inner.join(", "))
                    })
                    .collect();
                format!("[{}]", row_strs.join(", "))
            }
            Value::Complex { re, im, dimension } => {
                let (display_re, _) = dimension.to_display_units(*re);
                let (display_im, _) = dimension.to_display_units(*im);
                format!(
                    "{} + {}i",
                    format_display_number(display_re),
                    format_display_number(display_im)
                )
            }
            Value::Orientation { w, x, y, z } => {
                format!("[{}, {}, {}, {}]q", w, x, y, z)
            }
            Value::Frame { origin, basis } => {
                format!(
                    "frame({}, {})",
                    origin.format_display(),
                    basis.format_display()
                )
            }
            Value::Transform {
                rotation,
                translation,
            } => format!(
                "transform({}, {})",
                rotation.format_display(),
                translation.format_display()
            ),
            Value::Plane { origin, normal } => {
                format!(
                    "plane({}, {})",
                    origin.format_display(),
                    normal.format_display()
                )
            }
            Value::Axis { origin, direction } => {
                format!(
                    "axis({}, {})",
                    origin.format_display(),
                    direction.format_display()
                )
            }
            Value::Direction { x, y, z } => {
                format!("direction({}, {}, {})", x, y, z)
            }
            Value::BoundingBox { min, max } => {
                format!("bbox({}, {})", min.format_display(), max.format_display())
            }
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                let (lower_inclusive, upper_inclusive) =
                    normalize_range_flags(lower, upper, *lower_inclusive, *upper_inclusive);
                let lower_bracket = if lower_inclusive { "[" } else { "(" };
                let upper_bracket = if upper_inclusive { "]" } else { ")" };
                let lower_str = lower
                    .as_ref()
                    .map(|v| v.format_display())
                    .unwrap_or_else(|| "-\u{221E}".to_string());
                let upper_str = upper
                    .as_ref()
                    .map(|v| v.format_display())
                    .unwrap_or_else(|| "+\u{221E}".to_string());
                format!(
                    "{}{}..{}{}",
                    lower_bracket, lower_str, upper_str, upper_bracket
                )
            }
            Value::SampledField(sf) => {
                format!("SampledField('{}', {} samples)", sf.name, sf.data.len())
            }
            Value::StructureInstance(data) => {
                let mut entries: Vec<(&String, &Value)> = data.fields.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let type_name = &data.type_name;
                if entries.is_empty() {
                    format!("{type_name} {{ }}")
                } else {
                    let inner: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{k}: {}", v.format_display()))
                        .collect();
                    format!("{type_name} {{ {} }}", inner.join(", "))
                }
            }
            Value::GeometryHandle { realization_ref, .. } => {
                format!("<Geometry: {realization_ref}>")
            }
            Value::AffineMap { linear, translation } => {
                format!(
                    "affine_map([[{}, {}, {}], [{}, {}, {}], [{}, {}, {}]], [{}, {}, {}])",
                    linear[0][0], linear[0][1], linear[0][2],
                    linear[1][0], linear[1][1], linear[1][2],
                    linear[2][0], linear[2][1], linear[2][2],
                    translation[0], translation[1], translation[2],
                )
            }
            Value::Selector(sv) => format!("Selector({})", sv.kind),
            Value::Undef => "undefined".to_string(),
        }
    }

    /// Format this value for GUI display, returning `(formatted_value, unit_string)`.
    ///
    /// Unlike [`format_hover`](Value::format_hover) which shows raw SI values,
    /// this method converts to standard engineering display units (mm, deg, mm², mm³)
    /// via [`DimensionVector::to_display_units`].
    pub fn format_display_pair(&self) -> (String, String) {
        match self {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let (display_value, unit) = dimension.to_display_units(*si_value);
                (format_display_number(display_value), unit.to_string())
            }
            Value::Complex { re, im, dimension } => {
                let (display_re, unit) = dimension.to_display_units(*re);
                let (display_im, _) = dimension.to_display_units(*im);
                let formatted = format!(
                    "{} + {}i",
                    format_display_number(display_re),
                    format_display_number(display_im)
                );
                (formatted, unit.to_string())
            }
            Value::Option(Some(inner)) => inner.format_display_pair(),
            _ => (self.format_display(), String::new()),
        }
    }

    /// Format this value for auto-resolve emit, returning
    /// `Some((display_value_f64, formatted_number_string, unit_string))`.
    ///
    /// Returns `None` for variants that are not physical scalars (i.e. anything
    /// other than `Value::Scalar` or `Value::Option(Some(Scalar))`).  The `None`
    /// case is the caller's signal to emit a non-scalar sentinel rather than a
    /// real value.
    ///
    /// For `Value::Scalar`, the `f64` component is the engineering-unit value
    /// (e.g. millimetres, degrees) and the strings are the formatted number
    /// and unit symbol respectively.
    ///
    /// For `Value::Option(Some(inner))`, this recurses into `inner`.
    // G-allow: task #3648 auto-resolve emit feature; consumer is the auto-resolve diagnostic Display in subsequent #3648 steps
    pub fn format_display_triple(&self) -> Option<(f64, String, String)> {
        match self {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let (display_value, unit) = dimension.to_display_units(*si_value);
                Some((
                    display_value,
                    format_display_number(display_value),
                    unit.to_string(),
                ))
            }
            Value::Option(Some(inner)) => inner.format_display_triple(),
            _ => None,
        }
    }
}

/// Return `true` iff all four quaternion components are finite (not NaN,
/// not ±∞).
///
/// This is the shared quaternion-finiteness predicate used by:
/// - `sanitize_value` in `reify-expr` and `reify-stdlib` (Orientation arm)
/// - The Transform * Vector, Transform * Point, and Transform * Transform
///   rotation guards in `reify-expr`
///
/// Callers write `!quaternion_is_finite(w, x, y, z)` to test for the
/// "return Undef" branch, preserving existing control-flow patterns.
#[inline]
pub fn quaternion_is_finite(w: f64, x: f64, y: f64, z: f64) -> bool {
    w.is_finite() && x.is_finite() && y.is_finite() && z.is_finite()
}

/// Format a floating-point number for display: whole numbers render without
/// decimal points (e.g. `80.0` → `"80"`).
pub fn format_display_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Map a DimensionVector to a human-readable SI unit label.
///
/// Used by [`Value::format_hover`] for user-facing display.
fn dimension_unit_label(dim: &DimensionVector) -> &'static str {
    if *dim == DimensionVector::LENGTH {
        "m"
    } else if *dim == DimensionVector::AREA {
        "m\u{00B2}"
    } else if *dim == DimensionVector::VOLUME {
        "m\u{00B3}"
    } else if *dim == DimensionVector::MASS {
        "kg"
    } else if *dim == DimensionVector::ANGLE {
        "rad"
    } else if *dim == DimensionVector::MONEY {
        "USD"
    } else if dim.is_dimensionless() {
        ""
    } else {
        "SI"
    }
}

/// Bit-identity equality for `Value`.
///
/// Float-bearing variants (`Real`, `Scalar`, `Complex`, `Orientation`) compare via
/// `to_bits()`, giving bit-pattern identity: `-0.0 != +0.0` and `NaN == NaN`
/// (for the same canonical NaN bit pattern).
///
/// **Float-sign and NaN payload behaviour — important caveat:**
///
/// - **`-0.0` vs `+0.0`**: `PartialEq` considers them **not equal** (different
///   `to_bits()`), and `content_hash()` also produces different hashes.  The
///   hash-equality invariant (`a == b ⟹ same hash`) is **maintained**.
///
/// - **NaN payloads**: `PartialEq` considers two NaN values with different
///   payloads **not equal** (different `to_bits()`).  However, `content_hash()`
///   canonicalizes all NaN bit patterns to `f64::NAN.to_bits()`, so they
///   **hash identically**.  This is a **deliberate exception** to the hash-equality
///   invariant — see `content_hash()` for the rationale and caller guidance.
///
/// The earlier claim that "two `Value`s that differ only in float sign or NaN
/// payload are distinct keys" is only true for `PartialEq`; it does **not** hold
/// for `content_hash()` in the NaN-payload case.
///
/// **Eq/Ord contract:** this impl and `impl Ord for Value` both define equality
/// as bit-pattern identity, preserving the invariant: `a == b` iff
/// `a.cmp(&b) == Ordering::Equal`. Any change to either impl must preserve this
/// contract — update both impls together.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a.to_bits() == b.to_bits(),
            (Value::String(a), Value::String(b)) => a == b,
            (
                Value::Scalar {
                    si_value: a,
                    dimension: ad,
                },
                Value::Scalar {
                    si_value: b,
                    dimension: bd,
                },
            ) => a.to_bits() == b.to_bits() && ad == bd,
            (
                Value::Enum {
                    type_name: a,
                    variant: av,
                },
                Value::Enum {
                    type_name: b,
                    variant: bv,
                },
            ) => a == b && av == bv,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Tensor(a), Value::Tensor(b)) => a == b,
            (Value::Point(a), Value::Point(b)) => a == b,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Option(a), Value::Option(b)) => a == b,
            (
                Value::Field {
                    domain_type: ad,
                    codomain_type: ac,
                    source: as_,
                    lambda: al,
                },
                Value::Field {
                    domain_type: bd,
                    codomain_type: bc,
                    source: bs,
                    lambda: bl,
                },
            ) => ad == bd && ac == bc && as_ == bs && al == bl,
            (
                Value::Lambda {
                    params: ap,
                    body: ab,
                    captures: ac,
                },
                Value::Lambda {
                    params: bp,
                    body: bb,
                    captures: bc,
                },
            ) => {
                ap == bp && ab.content_hash == bb.content_hash && {
                    let a_caps = sorted_captures(ac);
                    let b_caps = sorted_captures(bc);
                    a_caps.len() == b_caps.len()
                        && a_caps
                            .iter()
                            .zip(b_caps.iter())
                            .all(|((aid, av), (bid, bv))| aid == bid && av == bv)
                }
            }
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => ar.to_bits() == br.to_bits() && ai.to_bits() == bi.to_bits() && ad == bd,
            (
                Value::Orientation {
                    w: aw,
                    x: ax,
                    y: ay,
                    z: az,
                },
                Value::Orientation {
                    w: bw,
                    x: bx,
                    y: by,
                    z: bz,
                },
            ) => {
                aw.to_bits() == bw.to_bits()
                    && ax.to_bits() == bx.to_bits()
                    && ay.to_bits() == by.to_bits()
                    && az.to_bits() == bz.to_bits()
            }
            (
                Value::Frame {
                    origin: ao,
                    basis: ab,
                },
                Value::Frame {
                    origin: bo,
                    basis: bb,
                },
            ) => ao == bo && ab == bb,
            (
                Value::Transform {
                    rotation: ar,
                    translation: at,
                },
                Value::Transform {
                    rotation: br,
                    translation: bt,
                },
            ) => ar == br && at == bt,
            (
                Value::Plane {
                    origin: ao,
                    normal: an,
                },
                Value::Plane {
                    origin: bo,
                    normal: bn,
                },
            ) => ao == bo && an == bn,
            (
                Value::Axis {
                    origin: ao,
                    direction: ad,
                },
                Value::Axis {
                    origin: bo,
                    direction: bd,
                },
            ) => ao == bo && ad == bd,
            (
                Value::BoundingBox {
                    min: amin,
                    max: amax,
                },
                Value::BoundingBox {
                    min: bmin,
                    max: bmax,
                },
            ) => amin == bmin && amax == bmax,
            (
                Value::Range {
                    lower: al,
                    upper: au,
                    lower_inclusive: ali,
                    upper_inclusive: aui,
                },
                Value::Range {
                    lower: bl,
                    upper: bu,
                    lower_inclusive: bli,
                    upper_inclusive: bui,
                },
            ) => {
                // Defensive re-normalization: None bounds → inclusive=false
                let (ali, aui) = normalize_range_flags(al, au, *ali, *aui);
                let (bli, bui) = normalize_range_flags(bl, bu, *bli, *bui);
                al == bl && au == bu && ali == bli && aui == bui
            }
            (Value::Matrix(a), Value::Matrix(b)) => a == b,
            (Value::SampledField(a), Value::SampledField(b)) => a == b,
            (Value::StructureInstance(a), Value::StructureInstance(b)) => {
                a.type_name == b.type_name && a.version == b.version && a.fields == b.fields
            }
            (
                Value::GeometryHandle {
                    realization_ref: rr_a,
                    upstream_values_hash: h_a,
                    ..
                },
                Value::GeometryHandle {
                    realization_ref: rr_b,
                    upstream_values_hash: h_b,
                    ..
                },
            ) => rr_a == rr_b && h_a == h_b,
            (
                Value::AffineMap {
                    linear: la,
                    translation: ta,
                },
                Value::AffineMap {
                    linear: lb,
                    translation: tb,
                },
            ) => {
                // Bit-identity equality: +0.0 != -0.0, NaN == NaN (same canonical bits)
                la.iter().zip(lb.iter()).all(|(ra, rb)| {
                    ra.iter().zip(rb.iter()).all(|(a, b)| a.to_bits() == b.to_bits())
                }) && ta.iter().zip(tb.iter()).all(|(a, b)| a.to_bits() == b.to_bits())
            }
            // Value::Selector equality goes through content_hash (Lambda-body pattern):
            // LeafQuery carries f64, so derived PartialEq would make NaN-bearing selectors
            // non-reflexive — breaking Value's `impl Eq`. content_hash canonicalizes NaN.
            (Value::Selector(a), Value::Selector(b)) => {
                a.content_hash() == b.content_hash() // task 4116 / α
            }
            (
                Value::Direction {
                    x: ax,
                    y: ay,
                    z: az,
                },
                Value::Direction {
                    x: bx,
                    y: by,
                    z: bz,
                },
            ) => {
                // Bit-identity equality (mirrors Orientation): +0.0 != -0.0,
                // NaN == NaN for identical bit patterns. Agrees with the cmp arm's
                // total_cmp ordering. MANDATORY: without this arm equal Directions
                // fall to `_ => false` below and compare UNEQUAL.
                ax.to_bits() == bx.to_bits()
                    && ay.to_bits() == by.to_bits()
                    && az.to_bits() == bz.to_bits()
            }
            (Value::Undef, Value::Undef) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Total order for `Value`, consistent with `impl PartialEq for Value`.
///
/// Float-bearing variants use IEEE 754 `total_cmp()`, giving a deterministic
/// total order that agrees with bit-identity equality:
/// `-0.0` and `+0.0` sort differently (`-0.0 < +0.0` under `total_cmp()`),
/// and `NaN` occupies a fixed position after `+Infinity` in the order.
///
/// **Eq/Ord contract:** Both `PartialEq` and `Ord` define equality as
/// bit-pattern identity, so the contract `a == b` iff `a.cmp(&b) == Ordering::Equal`
/// is preserved.
impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Type-tag discriminant for cross-type ordering:
        // Undef=0, Bool=1, Int=2, Real=3, Scalar=4, String=5, Enum=6, List=7, Set=8, Map=9, Option=10, Field=11, Lambda=12, Tensor=13, Complex=14, Orientation=15, Range=16, Point=17, Vector=18, Matrix=19, Frame=20, Transform=21, Plane=22, Axis=23, BoundingBox=24, SampledField=25, StructureInstance=26, GeometryHandle=27, AffineMap=28, Selector=29
        fn type_tag(v: &Value) -> u8 {
            match v {
                Value::Undef => 0,
                Value::Bool(_) => 1,
                Value::Int(_) => 2,
                Value::Real(_) => 3,
                Value::Scalar { .. } => 4,
                Value::String(_) => 5,
                Value::Enum { .. } => 6,
                Value::List(_) => 7,
                Value::Set(_) => 8,
                Value::Map(_) => 9,
                Value::Option(_) => 10,
                Value::Field { .. } => 11,
                Value::Lambda { .. } => 12,
                Value::Tensor(_) => 13,
                Value::Complex { .. } => 14,
                Value::Orientation { .. } => 15,
                Value::Range { .. } => 16,
                Value::Point(_) => 17,
                Value::Vector(_) => 18,
                Value::Matrix(_) => 19,
                Value::Frame { .. } => 20,
                Value::Transform { .. } => 21,
                Value::Plane { .. } => 22,
                Value::Axis { .. } => 23,
                Value::BoundingBox { .. } => 24,
                Value::SampledField(_) => 25,
                Value::StructureInstance(_) => 26,
                Value::GeometryHandle { .. } => 27,
                Value::AffineMap { .. } => 28,
                Value::Selector(_) => 29, // task 4116 / α
                Value::Direction { .. } => 30, // β / task 4382
            }
        }

        let tag_a = type_tag(self);
        let tag_b = type_tag(other);

        if tag_a != tag_b {
            return tag_a.cmp(&tag_b);
        }

        // Same type — compare within type
        match (self, other) {
            (Value::Undef, Value::Undef) => Ordering::Equal,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Real(a), Value::Real(b)) => {
                // IEEE 754 total order — see module-level "Float ordering strategy"
                // doc for to_bits() vs total_cmp() rationale.
                a.total_cmp(b)
            }
            (
                Value::Scalar {
                    si_value: a,
                    dimension: ad,
                },
                Value::Scalar {
                    si_value: b,
                    dimension: bd,
                },
            ) => {
                // Compare by dimension first, then by value with IEEE 754 total order
                ad.cmp(bd).then_with(|| a.total_cmp(b))
            }
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (
                Value::Enum {
                    type_name: a,
                    variant: av,
                },
                Value::Enum {
                    type_name: b,
                    variant: bv,
                },
            ) => a.cmp(b).then_with(|| av.cmp(bv)),
            (Value::List(a), Value::List(b)) => a.cmp(b),
            (Value::Tensor(a), Value::Tensor(b)) => a.cmp(b),
            (Value::Point(a), Value::Point(b)) => a.cmp(b),
            (Value::Vector(a), Value::Vector(b)) => a.cmp(b),
            (Value::Set(a), Value::Set(b)) => a.cmp(b),
            (Value::Map(a), Value::Map(b)) => {
                // Lexicographic on (key, value) pairs in sorted key order
                a.iter().cmp(b.iter())
            }
            (Value::Option(a), Value::Option(b)) => a.cmp(b),
            (
                Value::Field {
                    domain_type: ad,
                    codomain_type: ac,
                    source: as_,
                    lambda: al,
                },
                Value::Field {
                    domain_type: bd,
                    codomain_type: bc,
                    source: bs,
                    lambda: bl,
                },
            ) => format!("{}", ad)
                .cmp(&format!("{}", bd))
                .then_with(|| format!("{}", ac).cmp(&format!("{}", bc)))
                .then_with(|| format!("{:?}", as_).cmp(&format!("{:?}", bs)))
                .then_with(|| al.cmp(bl)),
            (
                Value::Lambda {
                    params: ap,
                    body: ab,
                    captures: ac,
                },
                Value::Lambda {
                    params: bp,
                    body: bb,
                    captures: bc,
                },
            ) => ap
                .cmp(bp)
                .then_with(|| ab.content_hash.0.cmp(&bb.content_hash.0))
                .then_with(|| sorted_captures(ac).cmp(&sorted_captures(bc))),
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => ad
                .cmp(bd)
                .then_with(|| ar.total_cmp(br))
                .then_with(|| ai.total_cmp(bi)),
            (
                Value::Orientation {
                    w: aw,
                    x: ax,
                    y: ay,
                    z: az,
                },
                Value::Orientation {
                    w: bw,
                    x: bx,
                    y: by,
                    z: bz,
                },
            ) => {
                // Lexicographic: w → x → y → z (IEEE 754 total_cmp per component)
                aw.total_cmp(bw)
                    .then_with(|| ax.total_cmp(bx))
                    .then_with(|| ay.total_cmp(by))
                    .then_with(|| az.total_cmp(bz))
            }
            (
                Value::Range {
                    lower: al,
                    upper: au,
                    lower_inclusive: ali,
                    upper_inclusive: aui,
                },
                Value::Range {
                    lower: bl,
                    upper: bu,
                    lower_inclusive: bli,
                    upper_inclusive: bui,
                },
            ) => {
                // Defensive re-normalization: None bounds → inclusive=false
                let (ali, aui) = normalize_range_flags(al, au, *ali, *aui);
                let (bli, bui) = normalize_range_flags(bl, bu, *bli, *bui);
                ali.cmp(&bli)
                    .then_with(|| al.cmp(bl))
                    .then_with(|| aui.cmp(&bui))
                    .then_with(|| au.cmp(bu))
            }
            (Value::Matrix(a), Value::Matrix(b)) => a.cmp(b),
            (
                Value::Frame {
                    origin: ao,
                    basis: ab,
                },
                Value::Frame {
                    origin: bo,
                    basis: bb,
                },
            ) => ao.cmp(bo).then_with(|| ab.cmp(bb)),
            (
                Value::Transform {
                    rotation: ar,
                    translation: at,
                },
                Value::Transform {
                    rotation: br,
                    translation: bt,
                },
            ) => ar.cmp(br).then_with(|| at.cmp(bt)),
            (
                Value::Plane {
                    origin: ao,
                    normal: an,
                },
                Value::Plane {
                    origin: bo,
                    normal: bn,
                },
            ) => ao.cmp(bo).then_with(|| an.cmp(bn)),
            (
                Value::Axis {
                    origin: ao,
                    direction: ad,
                },
                Value::Axis {
                    origin: bo,
                    direction: bd,
                },
            ) => ao.cmp(bo).then_with(|| ad.cmp(bd)),
            (
                Value::BoundingBox {
                    min: amin,
                    max: amax,
                },
                Value::BoundingBox {
                    min: bmin,
                    max: bmax,
                },
            ) => amin.cmp(bmin).then_with(|| amax.cmp(bmax)),
            (Value::SampledField(a), Value::SampledField(b)) => a.cmp(b),
            (Value::StructureInstance(a), Value::StructureInstance(b)) => {
                // Ordering must agree with PartialEq (Eq/Ord contract) and be
                // field-insertion-order-independent: compare name, then
                // version, then the sorted-by-key (key, value) pairs.
                // `type_id` is excluded — it is per-Engine ephemeral.
                let mut ae: Vec<(&String, &Value)> = a.fields.iter().collect();
                ae.sort_by(|x, y| x.0.cmp(y.0));
                let mut be: Vec<(&String, &Value)> = b.fields.iter().collect();
                be.sort_by(|x, y| x.0.cmp(y.0));
                a.type_name
                    .cmp(&b.type_name)
                    .then_with(|| a.version.cmp(&b.version))
                    .then_with(|| ae.cmp(&be))
            }
            (
                Value::GeometryHandle {
                    realization_ref: rr_a,
                    upstream_values_hash: h_a,
                    ..
                },
                Value::GeometryHandle {
                    realization_ref: rr_b,
                    upstream_values_hash: h_b,
                    ..
                },
            ) => {
                // kernel_handle excluded (ephemeral); Eq/Ord contract: Equal iff ==
                rr_a.entity
                    .cmp(&rr_b.entity)
                    .then_with(|| rr_a.index.cmp(&rr_b.index))
                    .then_with(|| h_a.cmp(h_b))
            }
            (
                Value::AffineMap {
                    linear: la,
                    translation: ta,
                },
                Value::AffineMap {
                    linear: lb,
                    translation: tb,
                },
            ) => {
                // Lexicographic: row 0 → row 1 → row 2 (each element total_cmp),
                // then translation[0..2]. Agrees with bit-identity PartialEq.
                for (ra, rb) in la.iter().zip(lb.iter()) {
                    for (a, b) in ra.iter().zip(rb.iter()) {
                        let c = a.total_cmp(b);
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                }
                for (a, b) in ta.iter().zip(tb.iter()) {
                    let c = a.total_cmp(b);
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                Ordering::Equal
            }
            // Selector ordering: compare by content_hash bytes (same pattern as Lambda body).
            // Eq/Ord contract: Equal iff == (both delegate to content_hash).
            (Value::Selector(a), Value::Selector(b)) => {
                a.content_hash().0.cmp(&b.content_hash().0) // task 4116 / α
            }
            (
                Value::Direction {
                    x: ax,
                    y: ay,
                    z: az,
                },
                Value::Direction {
                    x: bx,
                    y: by,
                    z: bz,
                },
            ) => {
                // Lexicographic: x → y → z (IEEE 754 total_cmp per component).
                // Agrees with bit-identity PartialEq. MANDATORY: without this arm
                // two distinct Directions fall to `_ => unreachable!` below and PANIC.
                ax.total_cmp(bx)
                    .then_with(|| ay.total_cmp(by))
                    .then_with(|| az.total_cmp(bz))
            }
            _ => unreachable!("same type tag but different variants"),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Real(r) => {
                // Format cleanly: no trailing ".0" for whole numbers.
                // Use {:.0} instead of `as i64` to avoid silent saturation
                // for f64 values beyond i64 range (e.g., 1e20).
                if *r == r.trunc() && r.is_finite() {
                    write!(f, "{:.0}", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Scalar {
                si_value,
                dimension,
            } => {
                write!(f, "{} {}", si_value, dimension)
            }
            Value::Enum { type_name, variant } => write!(f, "{}::{}", type_name, variant),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Tensor(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Point(items) => {
                write!(f, "point(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Vector(items) => {
                write!(f, "vec(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Set(items) => {
                write!(f, "{{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "}}")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Option(None) => write!(f, "None"),
            Value::Option(Some(v)) => write!(f, "Some({})", v),
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                write!(f, "Field<{}, {}>({:?})", domain_type, codomain_type, source)
            }
            Value::Lambda { params, .. } => {
                write!(f, "|")?;
                for (i, (name, _)) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", name)?;
                }
                write!(f, "| <lambda>")
            }
            Value::Complex { re, im, dimension } => {
                // Format re and im using Real's whole-number convention (no trailing .0)
                let fmt_f64 = |v: f64| -> String {
                    if v == v.trunc() && v.is_finite() {
                        format!("{:.0}", v)
                    } else {
                        format!("{}", v)
                    }
                };
                let re_str = fmt_f64(*re);
                let im_abs_str = fmt_f64(im.abs());
                let sign = if im.is_sign_negative() { "-" } else { "+" };
                if dimension.is_dimensionless() {
                    write!(f, "{}{}{}", re_str, sign, im_abs_str)?;
                    write!(f, "i")
                } else {
                    write!(f, "({}{}{}i) {}", re_str, sign, im_abs_str, dimension)
                }
            }
            Value::Orientation { w, x, y, z } => {
                // Format quaternion components using same whole-number convention as Real
                let fmt_f64 = |v: f64| -> String {
                    if v == v.trunc() && v.is_finite() {
                        format!("{:.0}", v)
                    } else {
                        format!("{}", v)
                    }
                };
                write!(
                    f,
                    "[{}, {}, {}, {}]q",
                    fmt_f64(*w),
                    fmt_f64(*x),
                    fmt_f64(*y),
                    fmt_f64(*z)
                )
            }
            Value::Frame { origin, basis } => {
                write!(f, "frame({}, {})", origin, basis)
            }
            Value::Transform {
                rotation,
                translation,
            } => {
                write!(f, "transform({}, {})", rotation, translation)
            }
            Value::Plane { origin, normal } => {
                write!(f, "plane({}, {})", origin, normal)
            }
            Value::Axis { origin, direction } => {
                write!(f, "axis({}, {})", origin, direction)
            }
            Value::Direction { x, y, z } => {
                // Same whole-number convention as Real/Orientation (no trailing ".0").
                let fmt_f64 = |v: f64| -> String {
                    if v == v.trunc() && v.is_finite() {
                        format!("{:.0}", v)
                    } else {
                        format!("{}", v)
                    }
                };
                write!(f, "direction({}, {}, {})", fmt_f64(*x), fmt_f64(*y), fmt_f64(*z))
            }
            Value::BoundingBox { min, max } => {
                write!(f, "bbox({}, {})", min, max)
            }
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                // Defensive re-normalization: if someone bypassed Value::range(),
                // ensure None bounds never appear as inclusive.
                let (lower_inclusive, upper_inclusive) =
                    normalize_range_flags(lower, upper, *lower_inclusive, *upper_inclusive);
                let lb = if lower_inclusive { '[' } else { '(' };
                let ub = if upper_inclusive { ']' } else { ')' };
                let lower_str = match lower {
                    None => "-inf".to_string(),
                    Some(v) => format!("{}", v),
                };
                let upper_str = match upper {
                    None => "inf".to_string(),
                    Some(v) => format!("{}", v),
                };
                write!(f, "{}{}..{}{}", lb, lower_str, upper_str, ub)
            }
            Value::Matrix(rows) => {
                write!(f, "[")?;
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "[")?;
                    for (j, elem) in row.iter().enumerate() {
                        if j > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", elem)?;
                    }
                    write!(f, "]")?;
                }
                write!(f, "]")
            }
            Value::SampledField(sf) => {
                write!(
                    f,
                    "sampled_field({}, {:?}, {} samples, {:?})",
                    sf.name,
                    sf.kind,
                    sf.data.len(),
                    sf.interpolation
                )
            }
            Value::StructureInstance(data) => {
                // `TypeName { k1: v1, k2: v2 }` with keys sorted for
                // deterministic output; empty → `TypeName { }`.
                let mut entries: Vec<(&String, &Value)> = data.fields.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let type_name = &data.type_name;
                write!(f, "{type_name} {{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    write!(f, "{} {k}: {v}", if i > 0 { "," } else { "" })?;
                }
                write!(f, " }}")
            }
            Value::GeometryHandle { realization_ref, .. } => {
                write!(f, "<Geometry: {realization_ref}>")
            }
            Value::AffineMap { linear, translation } => {
                write!(
                    f,
                    "affine_map(linear=[[{}, {}, {}], [{}, {}, {}], [{}, {}, {}]], translation=[{}, {}, {}])",
                    linear[0][0], linear[0][1], linear[0][2],
                    linear[1][0], linear[1][1], linear[1][2],
                    linear[2][0], linear[2][1], linear[2][2],
                    translation[0], translation[1], translation[2],
                )
            }
            Value::Selector(sv) => {
                // Display: "Selector(<kind>:<hash>)" — non-empty, stable, deterministic.
                write!(f, "Selector({}:{:032x})", sv.kind, sv.content_hash().0)
            }
            Value::Undef => write!(f, "undef"),
        }
    }
}

impl std::ops::Neg for Value {
    type Output = Value;

    /// Negate this value. Returns `Value::Undef` for unsupported types or on
    /// overflow (e.g. `Int(i64::MIN)`).
    fn neg(self) -> Value {
        match self {
            Value::Int(i) => i.checked_neg().map(Value::Int).unwrap_or(Value::Undef),
            Value::Real(r) => Value::Real(-r),
            Value::Scalar {
                si_value,
                dimension,
            } => Value::Scalar {
                si_value: -si_value,
                dimension,
            },
            Value::Complex { re, im, dimension } => Value::Complex {
                re: -re,
                im: -im,
                dimension,
            },
            Value::Tensor(components) => Self::neg_components(components, Value::Tensor),
            Value::Vector(components) => Self::neg_components(components, Value::Vector),
            // Affine geometry: point negation is undefined (spec 3.3.1)
            Value::Point(_) => Value::Undef,
            _ => Value::Undef,
        }
    }
}

/// The determinacy state of a value cell in the evaluation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeterminacyState {
    /// Value is fully determined.
    Determined,
    /// Value cannot be determined (missing input, cycle, error).
    Undetermined,
    /// Value is provisionally determined (may change during solving).
    Provisional,
    /// Value is marked auto — to be resolved by the constraint solver.
    Auto,
}

/// WHY a value cell is `undef` — per-cell origin captured by the engine's
/// post-eval classification pass when `capture_undef_causes` is enabled.
///
/// # Task scope
/// This enum is defined in full up-front so task γ (op-sink) can add
/// `OpContractFailed` construction without any enum churn.  Task α
/// (this task) constructs the four Layer-1 variants only;
/// `OpContractFailed` is reserved for task γ.
///
/// # PRD references
/// - §9.2.9 "Tracing" substrate
/// - A1 TRANSPARENCY: recorded in a parallel side-map, never on `Value`
/// - PRD Q4: `Eq + Hash` enables β's dedup-by-(kind, originating-cell)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UndefCause {
    /// The cell is an unbound required param with no default expression and no
    /// caller-supplied override.  The cell is absent from `EvalResult.values`
    /// but present in `snapshot.values` as `(Undef, Undetermined)` via the
    /// pre-seed in `Snapshot::from_compiled_module`.
    Unbound {
        /// The `ValueCellId` of the unbound param.
        param: ValueCellId,
        /// Source span of the param declaration.
        span: SourceSpan,
    },
    /// The cell is an `auto` (or `Provisional`) param that has not yet been
    /// resolved by the constraint solver — either because no solver was
    /// attached to the engine, or because the template's constraint problem
    /// yielded no solution and the cell was not touched.
    ///
    /// Distinguished from `SolveFailed`: here the solver was either absent or
    /// returned `Solved` (updating other cells), but this cell stayed `Undef`.
    AwaitingSolve {
        /// The `ValueCellId` of the unsolved auto param.
        param: ValueCellId,
    },
    /// The cell is an `auto` (or `Provisional`) param whose template's
    /// constraint solve explicitly failed (`SolveResult::Infeasible` or
    /// `SolveResult::NoProgress`).
    ///
    /// `detail` is a coarse honest string sourced from the actual
    /// `SolveResult` (§8.3 — no fabricated detail):
    /// - `"infeasible"` for `SolveResult::Infeasible`
    /// - `"no progress: <reason>"` for `SolveResult::NoProgress`
    SolveFailed {
        /// Coarse description of the solve failure (never fabricated).
        detail: String,
    },
    /// The cell's default expression evaluated successfully (all inputs were
    /// determined) but the op returned `Undef` — typically a type/contract
    /// violation in a built-in or user-defined op.
    ///
    /// **Constructed by task γ (the op-sink) only.**  Task α records nothing
    /// for cells that fall into this branch; the variant exists now so γ can
    /// construct it without editing the enum.
    OpContractFailed {
        /// Diagnostic code identifying the violated contract.
        code: DiagnosticCode,
        /// Source span of the offending expression.
        span: SourceSpan,
    },
    /// The cell has an explicit `= undef` default expression (i.e. the author
    /// deliberately wrote `undef` as the param default).
    UserUndef {
        /// Source span of the `undef` literal in the default expression.
        span: SourceSpan,
    },
}

/// The satisfaction state of a constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Satisfaction {
    /// All constraint conditions are met.
    Satisfied,
    /// At least one constraint condition is violated.
    Violated,
    /// Satisfaction cannot be determined (undef inputs).
    Indeterminate,
}

impl Satisfaction {
    /// Compute a content hash for incremental caching.
    /// Domain-separated with tag byte [10], exclusively reserved for Satisfaction
    /// (Value tags use 0-9, 11+).
    pub fn content_hash(&self) -> ContentHash {
        match self {
            Satisfaction::Satisfied => ContentHash::of(&[10, 0]),
            Satisfaction::Violated => ContentHash::of(&[10, 1]),
            Satisfaction::Indeterminate => ContentHash::of(&[10, 2]),
        }
    }
}

/// An error produced during value evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError(pub String);

impl EvalError {
    /// Returns the error message string.
    ///
    /// Provides an accessor so wrappers (e.g. [`ErrorRef`]) can delegate
    /// without depending on the tuple-field representation.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EvalError {}

/// Opaque carrier for the last substantive result referenced by
/// [`Freshness::Pending`].
///
/// Wraps `Option<ContentHash>` with private inner field so callers cannot
/// pattern-match on `Some`/`None` directly, satisfying the "opaque type"
/// requirement of the spec.  The two-state semantics (no prior result vs.
/// prior result identified by hash) are exposed via `none()` / `of_hash()`
/// constructors and `has_hash()` / `content_hash()` accessors.
///
/// See arch §7.1 lines 716-728.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRef(Option<ContentHash>);

impl ResultRef {
    /// Construct a `ResultRef` that carries no prior substantive result.
    pub fn none() -> Self {
        ResultRef(None)
    }

    /// Construct a `ResultRef` that carries the given content hash.
    pub fn of_hash(hash: ContentHash) -> Self {
        ResultRef(Some(hash))
    }

    /// Returns `true` when a prior substantive result is available (identified
    /// by a content hash).  Returns `false` when no prior result exists.
    pub fn has_hash(&self) -> bool {
        self.0.is_some()
    }

    /// Returns the content hash of the last substantive result, if any.
    pub fn content_hash(&self) -> Option<ContentHash> {
        self.0
    }
}

/// Opaque nominal wrapper for an evaluation failure stored in [`Freshness::Failed`].
///
/// Wraps the existing [`EvalError`] with a private inner field.  The
/// public accessors are `message() -> &str` and `code() -> Option<DiagnosticCode>`;
/// `Display` is delegated to the inner `EvalError`.  Use
/// `ErrorRef::from(eval_error)` or `.into()` for ergonomic conversion from
/// any site that already holds an `EvalError`.
///
/// The optional `code` field carries an opaque [`DiagnosticCode`] so that
/// downstream consumers can match on a typed identifier rather than a
/// substring of the message text. See arch §9.2 lines 880-890 and
/// spec §9.6 lines 1799-1819.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorRef {
    error: EvalError,
    code: Option<DiagnosticCode>,
}

impl ErrorRef {
    /// Construct an `ErrorRef` from a plain message string. The diagnostic
    /// code defaults to `None`; use [`ErrorRef::with_code`] to attach one.
    pub fn new(message: impl Into<String>) -> Self {
        ErrorRef {
            error: EvalError(message.into()),
            code: None,
        }
    }

    /// Returns the error message string.
    ///
    /// Delegates to [`EvalError::message`] so the wrapper depends on
    /// `EvalError`'s API rather than its tuple-field representation.
    pub fn message(&self) -> &str {
        self.error.message()
    }

    /// Returns the optional diagnostic code attached to this error.
    pub fn code(&self) -> Option<DiagnosticCode> {
        self.code
    }

    /// Builder: attach a [`DiagnosticCode`] to this error, replacing any
    /// previously-set code.
    pub fn with_code(mut self, code: DiagnosticCode) -> Self {
        self.code = Some(code);
        self
    }
}

impl std::fmt::Display for ErrorRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for ErrorRef {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<EvalError> for ErrorRef {
    fn from(error: EvalError) -> Self {
        ErrorRef { error, code: None }
    }
}

/// Four-variant evaluation lifecycle tag for cached nodes.
///
/// The four variants model the full lifecycle of a node in the incremental
/// evaluation cache: `Final | Intermediate | Pending | Failed`.  All four
/// share the same cache infrastructure (see arch §7.1 line 728).
///
/// See arch §7.1 lines 716-728, arch §9.2 lines 880-890,
/// and spec §9.6 lines 1799-1819.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Committed, fully evaluated; the cached value is authoritative.
    ///
    /// See arch §7.1 lines 716-728.
    Final,
    /// Still refining; generation monotonically increases across passes.
    ///
    /// See arch §7.1 lines 716-728.
    Intermediate { generation: u64 },
    /// Current entry not authoritative; previous best on display (either
    /// gated on upstream, or recomputation in flight via a ComputeNode).
    ///
    /// `last_substantive` carries the opaque identity of the last
    /// known-good result (if any); use [`ResultRef::none()`] when no
    /// prior result exists.
    ///
    /// See arch §7.1 lines 716-728.
    Pending { last_substantive: ResultRef },
    /// Computation failure — see arch §9.2.
    ///
    /// See arch §9.2 lines 880-890 and spec §9.6 lines 1799-1819.
    Failed { error: ErrorRef },
}

impl Default for Freshness {
    /// `Default::default()` returns `Final`; this is the canonical fallback for
    /// cache reads on absent entries (see `CacheStore::freshness`) and pins
    /// task #2326's "default to Final on read" contract.
    fn default() -> Self {
        Freshness::Final
    }
}

impl Freshness {
    /// Returns `true` iff this freshness tag is `Final` — the only state in
    /// which a cached value is authoritative and safe for a downstream
    /// `OnlyRunOnFinalInputs` node to consume.
    ///
    /// Single audit point for the canonical "is final?" check.  Any future
    /// addition of a fifth `Freshness` variant (e.g. a `Provisional` state,
    /// see arch §7.1 lines 716–728) only needs to be handled here rather than
    /// at every `matches!(f, Freshness::Final)` call site.
    ///
    /// See arch §7.1 lines 716–728 and §7.3 lines 762–767.
    pub const fn is_final(&self) -> bool {
        matches!(self, Freshness::Final)
    }
}

/// Sort captures by ValueCellId for deterministic comparison/hashing.
fn sorted_captures(captures: &ValueMap) -> Vec<(&ValueCellId, &Value)> {
    let mut caps: Vec<_> = captures.iter().collect();
    caps.sort_by_key(|(id, _)| *id);
    caps
}

/// Map from ValueCellId to Value. Uses PersistentMap (im::HashMap) for
/// O(1) structural-sharing clones.
#[derive(Debug, Clone, Default)]
pub struct ValueMap {
    inner: PersistentMap<ValueCellId, Value>,
}

impl ValueMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &ValueCellId) -> Option<&Value> {
        self.inner.get(id)
    }

    pub fn insert(&mut self, id: ValueCellId, value: Value) {
        self.inner.insert(id, value);
    }

    pub fn remove(&mut self, id: &ValueCellId) {
        self.inner.remove(id);
    }

    pub fn contains(&self, id: &ValueCellId) -> bool {
        self.inner.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ValueCellId, &Value)> {
        self.inner.iter()
    }

    /// Get a value, returning Undef if not present.
    pub fn get_or_undef(&self, id: &ValueCellId) -> Value {
        self.inner.get(id).cloned().unwrap_or(Value::Undef)
    }
}

// ── Keyed-member identity (task 3930 β) ──────────────────────────────────────
//
// A `Keyed<T>` sub-collection addresses its members by an author-assigned
// String key rather than by position. `MemberKey` is the first-class key tag at
// the schema/eval-graph layer (PRD §2.4 — deliberately decoupled from
// geometry-topology / persistent-naming-v2), and `keyed_member_cell` stamps the
// key into the member's `ValueCellId` path so the resolved NodeId reads
// `Widget.vents["intake"]` — the stable, key-addressed replacement for the
// positional `[N]` member identity.

/// An author-assigned key tagging one member of a `Keyed<T>` sub-collection.
///
/// This is a schema/eval-graph-layer String tag (PRD §2.4), kept separate from
/// the geometry-topology persistent-naming subsystem.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberKey(pub String);

impl MemberKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The bracketed path segment for this key under `sub_name`.
    ///
    /// Rust's `{:?}` Debug-formats a plain `String` as a double-quoted literal,
    /// so the segment is provably `vents["intake"]` (and quote-containing keys
    /// are safely escaped) — matching the PRD's key-addressed NodeId rendering.
    pub fn path_segment(&self, sub_name: &str) -> String {
        format!("{sub_name}[{:?}]", self.0)
    }
}

/// Build the key-addressed [`ValueCellId`] for a keyed member.
///
/// The cell's `member` slot carries the bracketed key segment, so its Display
/// (`entity.member`) yields the full key-addressed NodeId path, e.g.
/// `Widget.vents["intake"]`.
pub fn keyed_member_cell(parent_entity: &str, sub_name: &str, key: &MemberKey) -> ValueCellId {
    ValueCellId::new(parent_entity, key.path_segment(sub_name))
}

/// A single member of a `Keyed<T>` sub-collection: its author-assigned key
/// together with the key-addressed value cell that identifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedMember {
    pub key: MemberKey,
    pub cell: ValueCellId,
}

impl KeyedMember {
    pub fn new(parent_entity: &str, sub_name: &str, key: MemberKey) -> Self {
        let cell = keyed_member_cell(parent_entity, sub_name, &key);
        Self { key, cell }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    // Boundary float values used by IEEE 754 totalOrder ordering tests.
    // All 7 values are bit-distinct; insertion order is intentionally scrambled
    // so that tests exercise the sort rather than relying on insertion sequence.
    const BOUNDARY_REALS: &[f64] = &[
        0.0,  // +0.0
        -0.0, // -0.0 (different bit pattern from +0.0)
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -1.0,
        1.0,
    ];

    // ── Keyed-member identity (step-5 RED / task 3930 β) ─────────────────────
    // `MemberKey` + `keyed_member_cell` + `KeyedMember` encode the key-addressed
    // NodeId path (`Widget.vents["intake"]`) — the stable replacement for the
    // positional `[N]` member identity.

    #[test]
    fn member_key_path_segment_is_bracketed_debug_quoted() {
        // Rust `{:?}` Debug-formats a plain String as a double-quoted literal,
        // so the segment is provably `vents["intake"]`.
        let key = MemberKey::new("intake");
        assert_eq!(key.path_segment("vents"), r#"vents["intake"]"#);
    }

    #[test]
    fn keyed_member_cell_carries_key_in_nodeid_path() {
        let cell = keyed_member_cell("Widget", "vents", &MemberKey::new("intake"));
        // The cell's member slot is the key-addressed segment …
        assert_eq!(cell.member, r#"vents["intake"]"#);
        // … and its Display (entity.member) is the full key-addressed NodeId path.
        assert_eq!(format!("{}", cell), r#"Widget.vents["intake"]"#);
    }

    #[test]
    fn keyed_member_bundles_key_and_cell() {
        let km = KeyedMember::new("Widget", "vents", MemberKey::new("intake"));
        assert_eq!(km.key, MemberKey::new("intake"));
        assert_eq!(
            km.cell,
            keyed_member_cell("Widget", "vents", &MemberKey::new("intake"))
        );
        assert_eq!(format!("{}", km.cell), r#"Widget.vents["intake"]"#);
    }

    // ── Value::StructureInstance variant (task 3540 / SIR-α) ─────────────────
    mod structure_instance {
        use super::*;
        use crate::structure_registry::StructureTypeId;

        /// Build a `Value::StructureInstance` with a fixed `type_id`/`version`
        /// and the given `(name, value)` field pairs.
        fn si(name: &str, version: u32, pairs: &[(&str, Value)]) -> Value {
            let fields: PersistentMap<String, Value> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(0),
                type_name: name.to_string(),
                version,
                fields,
            }))
        }

        #[test]
        fn construct_and_destructure() {
            let v = si("Steel_AISI_1045", 1, &[("youngs_modulus", Value::Real(205e9))]);
            match &v {
                Value::StructureInstance(data) => {
                    assert_eq!(data.type_name, "Steel_AISI_1045");
                    assert_eq!(data.version, 1);
                    assert_eq!(
                        data.fields.get(&"youngs_modulus".to_string()),
                        Some(&Value::Real(205e9))
                    );
                }
                other => panic!("expected StructureInstance, got {other:?}"),
            }
        }

        #[test]
        fn clone_preserves_nested_fields() {
            let v = si(
                "Beam",
                1,
                &[
                    ("length", Value::length(1.0)),
                    ("material", si("Steel_AISI_1045", 1, &[("e", Value::Real(2.0))])),
                ],
            );
            let c = v.clone();
            assert_eq!(v, c, "clone is structurally equal (PersistentMap sharing)");
        }

        #[test]
        fn partial_eq_is_field_order_insensitive() {
            let a = si("S", 1, &[("x", Value::Int(1)), ("y", Value::Int(2))]);
            let b = si("S", 1, &[("y", Value::Int(2)), ("x", Value::Int(1))]);
            assert_eq!(a, b, "PersistentMap content equality ignores insert order");
        }

        #[test]
        fn partial_eq_discriminates_name_version_and_fields() {
            let base = si("S", 1, &[("x", Value::Int(1))]);
            assert_ne!(base, si("T", 1, &[("x", Value::Int(1))]), "name differs");
            assert_ne!(base, si("S", 2, &[("x", Value::Int(1))]), "version differs");
            assert_ne!(
                base,
                si("S", 1, &[("x", Value::Int(2))]),
                "field value differs"
            );
            assert_ne!(
                base,
                si("S", 1, &[("x", Value::Int(1)), ("z", Value::Int(0))]),
                "extra field"
            );
        }

        #[test]
        fn content_hash_is_deterministic() {
            let v = si("S", 1, &[("a", Value::Int(1))]);
            assert_eq!(
                v.content_hash(),
                v.content_hash(),
                "content_hash is a deterministic pure function"
            );
        }

        #[test]
        fn content_hash_equal_for_equal_structures() {
            let a = si("S", 1, &[("a", Value::Int(1)), ("b", Value::Real(2.0))]);
            let b = si("S", 1, &[("a", Value::Int(1)), ("b", Value::Real(2.0))]);
            assert_eq!(a.content_hash(), b.content_hash());
        }

        #[test]
        fn content_hash_independent_of_field_insertion_order() {
            let a = si(
                "S",
                1,
                &[
                    ("alpha", Value::Int(1)),
                    ("beta", Value::Int(2)),
                    ("gamma", Value::Int(3)),
                ],
            );
            let b = si(
                "S",
                1,
                &[
                    ("gamma", Value::Int(3)),
                    ("alpha", Value::Int(1)),
                    ("beta", Value::Int(2)),
                ],
            );
            assert_eq!(
                a.content_hash(),
                b.content_hash(),
                "content_hash must sort fields by key before folding"
            );
        }

        #[test]
        fn content_hash_differs_on_name() {
            let a = si("Steel_AISI_1045", 1, &[("x", Value::Int(1))]);
            let b = si("Aluminium_6061_T6", 1, &[("x", Value::Int(1))]);
            assert_ne!(a.content_hash(), b.content_hash());
        }

        #[test]
        fn content_hash_differs_on_version() {
            let a = si("S", 1, &[("x", Value::Int(1))]);
            let b = si("S", 2, &[("x", Value::Int(1))]);
            assert_ne!(
                a.content_hash(),
                b.content_hash(),
                "@version(N) bump must invalidate the content hash"
            );
        }

        #[test]
        fn content_hash_differs_on_field_value() {
            let a = si("S", 1, &[("x", Value::Int(1))]);
            let b = si("S", 1, &[("x", Value::Int(2))]);
            assert_ne!(a.content_hash(), b.content_hash());
        }

        #[test]
        fn content_hash_distinct_from_map_lookalike() {
            // Linguistic Map-vs-Structure distinction: a Value::Map with the
            // same String→Value entry must not collide with a StructureInstance.
            let structure = si("S", 1, &[("x", Value::Int(1))]);
            let mut m = BTreeMap::new();
            m.insert(Value::String("x".to_string()), Value::Int(1));
            let map = Value::Map(m);
            assert_ne!(structure.content_hash(), map.content_hash());
        }
    }

    // ── Value::GeometryHandle variant (task 3604 / GHR-β) ────────────────────
    mod geometry_handle {
        use super::*;
        use reify_core::identity::RealizationNodeId;
        use crate::geometry::GeometryHandleId;

        /// Build a `Value::GeometryHandle` with the given realization_ref,
        /// upstream_values_hash, and kernel_handle (realized, Some-wrapped).
        fn gh(entity: &str, index: u32, hash: [u8; 32], kernel_id: u64) -> Value {
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new(entity, index),
                upstream_values_hash: hash,
                kernel_handle: Some(GeometryHandleId(kernel_id)),
            }
        }

        #[test]
        fn construct_and_destructure() {
            let v = gh("Bracket", 0, [7u8; 32], 42);
            match &v {
                Value::GeometryHandle {
                    realization_ref,
                    upstream_values_hash,
                    kernel_handle,
                } => {
                    assert_eq!(realization_ref.entity, "Bracket");
                    assert_eq!(realization_ref.index, 0);
                    assert_eq!(upstream_values_hash, &[7u8; 32]);
                    assert_eq!(*kernel_handle, Some(GeometryHandleId(42)));
                }
                other => panic!("expected GeometryHandle, got {other:?}"),
            }
        }

        #[test]
        fn clone_preserves_all_three_fields() {
            let v = gh("Bracket", 0, [7u8; 32], 42);
            let c = v.clone();
            match (&v, &c) {
                (
                    Value::GeometryHandle {
                        realization_ref: rr_a,
                        upstream_values_hash: h_a,
                        kernel_handle: kh_a,
                    },
                    Value::GeometryHandle {
                        realization_ref: rr_b,
                        upstream_values_hash: h_b,
                        kernel_handle: kh_b,
                    },
                ) => {
                    assert_eq!(rr_a, rr_b);
                    assert_eq!(h_a, h_b);
                    assert_eq!(kh_a, kh_b);
                }
                _ => panic!("clone changed variant"),
            }
        }

        #[test]
        fn partial_eq_excludes_kernel_handle() {
            // Same realization_ref + same hash but different kernel_handle ⇒ equal
            let a = gh("Bracket", 0, [7u8; 32], 42);
            let b = gh("Bracket", 0, [7u8; 32], 99);
            assert_eq!(a, b, "kernel_handle must not participate in PartialEq");

            // Differing upstream_values_hash ⇒ not equal
            let c = gh("Bracket", 0, [8u8; 32], 42);
            assert_ne!(a, c, "different upstream_values_hash must produce !=");

            // Differing realization_ref entity ⇒ not equal
            let d = gh("Hinge", 0, [7u8; 32], 42);
            assert_ne!(a, d, "different entity must produce !=");

            // Differing realization_ref index ⇒ not equal
            let e = gh("Bracket", 1, [7u8; 32], 42);
            assert_ne!(a, e, "different index must produce !=");
        }

        #[test]
        fn content_hash_mirrors_partial_eq() {
            // Equal-by-PartialEq handles ⇒ equal content_hash
            let a = gh("Bracket", 0, [7u8; 32], 42);
            let b = gh("Bracket", 0, [7u8; 32], 99); // kernel_handle differs
            assert_eq!(a, b, "precondition: a == b");
            assert_eq!(
                a.content_hash(),
                b.content_hash(),
                "kernel_handle-only difference must not affect content_hash"
            );

            // Different upstream_values_hash ⇒ different content_hash
            let c = gh("Bracket", 0, [8u8; 32], 42);
            assert_ne!(
                a.content_hash(),
                c.content_hash(),
                "different upstream_values_hash must produce different content_hash"
            );

            // Different realization_ref ⇒ different content_hash
            let d = gh("Hinge", 0, [7u8; 32], 42);
            assert_ne!(
                a.content_hash(),
                d.content_hash(),
                "different entity must produce different content_hash"
            );
        }

        #[test]
        fn ord_agrees_with_eq() {
            use std::cmp::Ordering;

            // Equal-by-PartialEq (kernel_handle-only difference) ⇒ cmp returns Equal
            let a = gh("Bracket", 0, [7u8; 32], 42);
            let b = gh("Bracket", 0, [7u8; 32], 99);
            assert_eq!(a, b, "precondition: a == b");
            assert_eq!(
                a.cmp(&b),
                Ordering::Equal,
                "Ord must return Equal for == handles"
            );

            // Different upstream_values_hash ⇒ not Equal
            let c = gh("Bracket", 0, [8u8; 32], 42);
            assert_ne!(a.cmp(&c), Ordering::Equal);

            // Different entity ⇒ not Equal
            let d = gh("Hinge", 0, [7u8; 32], 42);
            assert_ne!(a.cmp(&d), Ordering::Equal);

            // Directional asserts: pin the documented lexicographic order
            // (entity → index → upstream_values_hash); kernel_handle excluded.

            // entity: "A" < "B" regardless of other fields
            assert_eq!(
                gh("A", 0, [0u8; 32], 1).cmp(&gh("B", 0, [0u8; 32], 1)),
                Ordering::Less,
                "entity ordering: A < B"
            );
            // index: 0 < 1 when entity is equal
            assert_eq!(
                gh("A", 0, [0u8; 32], 1).cmp(&gh("A", 1, [0u8; 32], 1)),
                Ordering::Less,
                "index ordering: 0 < 1 when entity equal"
            );
            // upstream_values_hash: [0;32] < [1;32] when entity + index equal
            assert_eq!(
                gh("A", 0, [0u8; 32], 1).cmp(&gh("A", 0, [1u8; 32], 1)),
                Ordering::Less,
                "upstream_values_hash ordering: [0;32] < [1;32] when entity+index equal"
            );
        }

        #[test]
        fn display_format_is_realization_ref_only() {
            // §9 Q3: format must be `<Geometry: entity#realization[index]>`,
            // kernel_handle is omitted for golden-test stability.
            let v = gh("Bracket", 0, [7u8; 32], 42);
            assert_eq!(
                format!("{v}"),
                "<Geometry: Bracket#realization[0]>",
                "Display must use <Geometry: {{realization_ref}}> with no kernel_handle"
            );
        }

        // ── R2a: symbolic handle identity (task #4652) ────────────────────────
        //
        // These tests force `kernel_handle: Option<GeometryHandleId>` (DD-2).
        // They will FAIL TO COMPILE on main (the RED signal) because the field
        // is currently `GeometryHandleId`, not `Option<GeometryHandleId>`.
        // Step-2 changes the field type to make them compile and pass.

        #[test]
        fn symbolic_handle_none_and_realized_some_are_equal() {
            // (a) symbolic == realized: PartialEq excludes kernel_handle, so
            //     None vs Some(42) must not affect equality when realization_ref
            //     and upstream_values_hash are identical (GHR-β §DD).
            let rr = RealizationNodeId::new("Bracket", 0);
            let uvh = [7u8; 32];
            let symbolic = Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None, // ← forces Option; compile-error on main
            };
            let realized = Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: Some(GeometryHandleId(42)),
            };
            assert_eq!(
                symbolic, realized,
                "symbolic (None) and realized (Some(42)) must be equal when \
                 realization_ref + upstream_values_hash match (PartialEq excludes kernel_handle)"
            );
        }

        #[test]
        fn symbolic_and_realized_content_hash_equal() {
            // (b) content_hash excludes kernel_handle, so symbolic and realized
            //     handles with matching realization_ref+uvh must hash identically.
            let rr = RealizationNodeId::new("Bracket", 0);
            let uvh = [7u8; 32];
            let symbolic = Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            };
            let realized = Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: Some(GeometryHandleId(42)),
            };
            assert_eq!(
                symbolic.content_hash(),
                realized.content_hash(),
                "symbolic and realized handles must produce identical content_hash \
                 when realization_ref + upstream_values_hash match"
            );
        }

        #[test]
        fn geometry_handle_ref_threads_option_kernel_handle() {
            // (c) GeometryHandleRef::from_geometry_handle must propagate the
            //     Option: symbolic yields kernel_handle == None, realized yields
            //     kernel_handle == Some(GeometryHandleId(42)).
            let rr = RealizationNodeId::new("Bracket", 0);
            let uvh = [7u8; 32];
            let symbolic = Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            };
            let realized = Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: Some(GeometryHandleId(42)),
            };
            let sym_ref = GeometryHandleRef::from_geometry_handle(&symbolic)
                .expect("from_geometry_handle must return Some for Value::GeometryHandle");
            let real_ref = GeometryHandleRef::from_geometry_handle(&realized)
                .expect("from_geometry_handle must return Some for Value::GeometryHandle");
            assert_eq!(
                sym_ref.kernel_handle, None,
                "symbolic GeometryHandleRef must have kernel_handle == None"
            );
            assert_eq!(
                real_ref.kernel_handle,
                Some(GeometryHandleId(42)),
                "realized GeometryHandleRef must have kernel_handle == Some(GeometryHandleId(42))"
            );
        }
    }

    // ── normalize_range_flags unit tests ─────────────────────────────────────

    #[test]
    fn test_normalize_range_flags() {
        // Both bounds present → flags pass through unchanged
        assert_eq!(
            normalize_range_flags(&Some(1), &Some(2), true, true),
            (true, true)
        );

        // Lower is None → lower_inclusive forced false
        assert_eq!(
            normalize_range_flags::<i32>(&None, &Some(2), true, true),
            (false, true)
        );

        // Upper is None → upper_inclusive forced false
        assert_eq!(
            normalize_range_flags(&Some(1), &None::<i32>, true, true),
            (true, false)
        );

        // Both None → both forced false
        assert_eq!(
            normalize_range_flags::<i32>(&None, &None, true, true),
            (false, false)
        );

        // Both present but flags already false → stays false
        assert_eq!(
            normalize_range_flags(&Some(1), &Some(2), false, false),
            (false, false)
        );
    }

    // ── from_real_scalar unit tests (task 843) ───────────────────────────────

    #[test]
    fn from_real_scalar_dimensionless_returns_real() {
        // from_real_scalar(v, DIMENSIONLESS) should return Value::Real(v).
        // (Avoid 3.14-style literals — clippy::approx_constant flags them as PI.)
        let v = Value::from_real_scalar(2.5, DimensionVector::DIMENSIONLESS);
        assert_eq!(
            v,
            Value::Real(2.5),
            "from_real_scalar with DIMENSIONLESS must return Value::Real"
        );
    }

    #[test]
    fn from_real_scalar_length_returns_scalar() {
        // from_real_scalar(v, LENGTH) should return Value::Scalar { si_value: v, dimension: LENGTH }.
        let v = Value::from_real_scalar(1.0, DimensionVector::LENGTH);
        assert_eq!(
            v,
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            "from_real_scalar with LENGTH must return Value::Scalar"
        );
    }

    #[test]
    fn from_real_scalar_preserves_nan() {
        // NaN-safety contract: from_real_scalar does NOT sanitize — NaN is preserved,
        // NOT converted to Undef. Callers are responsible for wrapping in sanitize_value()
        // if the input is arithmetically derived. Value::PartialEq uses to_bits(),
        // so `Real(NaN) == Real(NaN)` holds in tests.
        let v = Value::from_real_scalar(f64::NAN, DimensionVector::DIMENSIONLESS);
        assert_eq!(
            v,
            Value::Real(f64::NAN),
            "from_real_scalar(NaN, DIMENSIONLESS) must preserve NaN (not convert to Undef)"
        );
    }

    #[test]
    fn value_content_hash_determinism() {
        let v1 = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        let v2 = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(v1.content_hash(), v2.content_hash());
    }

    #[test]
    fn real_neg_zero_not_normalized_in_hash() {
        // -0.0 and 0.0 are different via PartialEq (to_bits), so content_hash must differ
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn real_neg_zero_hash_differs_from_pos_zero() {
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        // PartialEq uses to_bits(), so -0.0 != 0.0
        assert_ne!(pos, neg);
        // Therefore content_hash must also differ (cache invariant)
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn scalar_neg_zero_hash_differs_from_pos_zero() {
        let pos = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let neg = Value::Scalar {
            si_value: -0.0,
            dimension: DimensionVector::LENGTH,
        };
        // PartialEq uses to_bits(), so -0.0 != 0.0
        assert_ne!(pos, neg);
        // Therefore content_hash must also differ (cache invariant)
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn hash_equality_invariant_real() {
        // Spot-check: for -0.0 and 0.0, if a != b then a.content_hash() != b.content_hash()
        let a = Value::Real(-0.0);
        let b = Value::Real(0.0);
        if a != b {
            assert_ne!(
                a.content_hash(),
                b.content_hash(),
                "hash-equality invariant violated: unequal values must have different hashes"
            );
        }
    }

    /// Documents the **deliberate exception** to the hash-equality invariant for
    /// NaN payloads.
    ///
    /// The standard invariant is: `a == b  ⟹  content_hash(a) == content_hash(b)`
    /// (equivalently: `content_hash(a) != content_hash(b)  ⟹  a != b`).
    ///
    /// For NaN-bearing variants, `content_hash()` intentionally **collapses** all
    /// NaN bit patterns to the canonical `f64::NAN` bit pattern (see the method
    /// doc on `content_hash()` for the rationale).  `PartialEq`, by contrast, uses
    /// `to_bits()` and therefore distinguishes NaN values that differ only in
    /// payload.  This creates the exception:
    ///
    ///   `a != b`  yet  `content_hash(a) == content_hash(b)`
    ///
    /// **Contrast with -0.0/+0.0**: those variants *do* maintain the invariant —
    /// `-0.0 != +0.0` (via `to_bits()`) AND their hashes differ.  See
    /// `real_neg_zero_hash_differs_from_pos_zero`, `scalar_neg_zero_hash_differs_from_pos_zero`,
    /// and `hash_equality_invariant_real` for those tests.
    ///
    /// **Caller guidance**: content-addressed lookups for NaN-bearing values should
    /// treat a hash-hit as "possibly equal" and re-check `PartialEq` when exact
    /// bit-pattern identity matters.
    #[test]
    fn nan_payload_hash_equality_invariant_exception() {
        // Build a non-canonical NaN: same NaN class, distinct low-mantissa bit.
        let non_canon_nan = f64::from_bits(f64::NAN.to_bits() ^ 1);
        assert!(non_canon_nan.is_nan(), "non_canon_nan must still be NaN");

        // (1) Value::Real
        {
            let a = Value::Real(f64::NAN);
            let b = Value::Real(non_canon_nan);
            // PartialEq uses to_bits() → they are NOT equal
            assert_ne!(
                a, b,
                "Real: NaN values with different payloads must be unequal via PartialEq"
            );
            // content_hash collapses both to canonical NaN → they DO hash equally
            assert_eq!(
                a.content_hash(),
                b.content_hash(),
                "Real: NaN values with different payloads must hash equally (invariant exception)"
            );
        }

        // (2) Value::Scalar
        {
            let a = Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::DIMENSIONLESS,
            };
            let b = Value::Scalar {
                si_value: non_canon_nan,
                dimension: DimensionVector::DIMENSIONLESS,
            };
            assert_ne!(
                a, b,
                "Scalar: NaN values with different payloads must be unequal via PartialEq"
            );
            assert_eq!(
                a.content_hash(),
                b.content_hash(),
                "Scalar: NaN values with different payloads must hash equally (invariant exception)"
            );
        }

        // (3) Value::Complex (re field)
        {
            let a = Value::Complex {
                re: f64::NAN,
                im: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            };
            let b = Value::Complex {
                re: non_canon_nan,
                im: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            };
            assert_ne!(
                a, b,
                "Complex re: NaN values with different payloads must be unequal via PartialEq"
            );
            assert_eq!(
                a.content_hash(),
                b.content_hash(),
                "Complex re: NaN values with different payloads must hash equally (invariant exception)"
            );
        }

        // (4) Value::Orientation (w field)
        {
            let a = orient(f64::NAN, 0.0, 0.0, 0.0);
            let b = orient(non_canon_nan, 0.0, 0.0, 0.0);
            assert_ne!(
                a, b,
                "Orientation: NaN values with different payloads must be unequal via PartialEq"
            );
            assert_eq!(
                a.content_hash(),
                b.content_hash(),
                "Orientation: NaN values with different payloads must hash equally (invariant exception)"
            );
        }
    }

    #[test]
    fn nan_normalized() {
        let nan1 = Value::Real(f64::NAN);
        let nan2 = Value::Real(f64::NAN);
        assert_eq!(nan1.content_hash(), nan2.content_hash());
    }

    #[test]
    fn nan_partialeq_bit_identity() {
        let nan1 = Value::Real(f64::NAN);
        let nan2 = Value::Real(f64::NAN);
        assert_eq!(
            nan1, nan2,
            "two separately constructed NaN values with identical bit patterns must compare equal"
        );
    }

    #[test]
    fn nan_partialeq_bit_identity_scalar() {
        let s1 = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let s2 = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(
            s1, s2,
            "two separately constructed Scalar NaN values with identical bit patterns must compare equal"
        );
        assert_eq!(
            s1,
            s1.clone(),
            "a Scalar NaN value must compare equal to its own clone"
        );
    }

    #[test]
    fn nan_partialeq_bit_identity_complex() {
        // (a) both re and im are NaN
        let c1 = Value::Complex {
            re: f64::NAN,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let c2 = Value::Complex {
            re: f64::NAN,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(
            c1, c2,
            "two separately constructed Complex values with NaN re and NaN im must compare equal"
        );
        assert_eq!(
            c1,
            c1.clone(),
            "a Complex value with NaN re and NaN im must compare equal to its own clone"
        );

        // (b) only re is NaN, im is finite
        let c3 = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let c4 = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(
            c3, c4,
            "two separately constructed Complex values with NaN re and finite im must compare equal"
        );
        assert_eq!(
            c3,
            c3.clone(),
            "a Complex value with NaN re and finite im must compare equal to its own clone"
        );

        // (c) only im is NaN, re is finite
        let c5 = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let c6 = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(
            c5, c6,
            "two separately constructed Complex values with finite re and NaN im must compare equal"
        );
        assert_eq!(
            c5,
            c5.clone(),
            "a Complex value with finite re and NaN im must compare equal to its own clone"
        );
    }

    #[test]
    fn nan_partialeq_bit_identity_orientation() {
        // (a) all four components are NaN
        let o1 = orient(f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        let o2 = orient(f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        assert_eq!(
            o1, o2,
            "two separately constructed Orientation values with all-NaN components must compare equal"
        );
        assert_eq!(
            o1,
            o1.clone(),
            "an Orientation value with all-NaN components must compare equal to its own clone"
        );

        // (b) only w is NaN
        let o3 = orient(f64::NAN, 0.0, 0.0, 0.0);
        let o4 = orient(f64::NAN, 0.0, 0.0, 0.0);
        assert_eq!(
            o3, o4,
            "two separately constructed Orientation values with NaN w must compare equal"
        );
        assert_eq!(
            o3,
            o3.clone(),
            "an Orientation value with NaN w must compare equal to its own clone"
        );

        // (c) only x is NaN
        let o5 = orient(0.0, f64::NAN, 0.0, 0.0);
        let o6 = orient(0.0, f64::NAN, 0.0, 0.0);
        assert_eq!(
            o5, o6,
            "two separately constructed Orientation values with NaN x must compare equal"
        );
        assert_eq!(
            o5,
            o5.clone(),
            "an Orientation value with NaN x must compare equal to its own clone"
        );

        // (d) only y is NaN
        let o7 = orient(0.0, 0.0, f64::NAN, 0.0);
        let o8 = orient(0.0, 0.0, f64::NAN, 0.0);
        assert_eq!(
            o7, o8,
            "two separately constructed Orientation values with NaN y must compare equal"
        );
        assert_eq!(
            o7,
            o7.clone(),
            "an Orientation value with NaN y must compare equal to its own clone"
        );

        // (e) only z is NaN
        let o9 = orient(0.0, 0.0, 0.0, f64::NAN);
        let o10 = orient(0.0, 0.0, 0.0, f64::NAN);
        assert_eq!(
            o9, o10,
            "two separately constructed Orientation values with NaN z must compare equal"
        );
        assert_eq!(
            o9,
            o9.clone(),
            "an Orientation value with NaN z must compare equal to its own clone"
        );
    }

    #[test]
    fn different_values_different_hashes() {
        let a = Value::length(0.08);
        let b = Value::length(0.10);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn different_dimensions_different_hashes() {
        let len = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let mass = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert_ne!(len.content_hash(), mass.content_hash());
    }

    #[test]
    fn test_freshness_final() {
        let f = Freshness::Final;
        let f2 = f.clone();
        assert_eq!(f, f2);
        assert_eq!(format!("{:?}", f), "Final");
    }

    #[test]
    fn test_freshness_intermediate() {
        let f = Freshness::Intermediate { generation: 42 };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Intermediate { generation } => assert_eq!(*generation, 42),
            _ => panic!("expected Intermediate"),
        }
    }

    #[test]
    fn test_freshness_pending_none() {
        let f = Freshness::Pending {
            last_substantive: ResultRef::none(),
        };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Pending { last_substantive } => assert!(!last_substantive.has_hash()),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn test_freshness_pending_some() {
        let hash = ContentHash::of(b"test");
        let f = Freshness::Pending {
            last_substantive: ResultRef::of_hash(hash),
        };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Pending { last_substantive } => {
                assert_eq!(last_substantive.content_hash(), Some(hash))
            }
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn test_freshness_failed() {
        let f = Freshness::Failed {
            error: ErrorRef::new("type mismatch"),
        };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Failed { error } => assert_eq!(error.message(), "type mismatch"),
            _ => panic!("expected Failed"),
        }
    }

    // ── ResultRef tests (step-1) ─────────────────────────────────────────────

    #[test]
    fn test_result_ref_none() {
        let r = ResultRef::none();
        assert!(!r.has_hash());
        assert_eq!(r.content_hash(), None);
    }

    #[test]
    fn test_result_ref_of_hash() {
        let hash = ContentHash::of(b"x");
        let r = ResultRef::of_hash(hash);
        assert!(r.has_hash());
        assert_eq!(r.content_hash(), Some(hash));
    }

    #[test]
    fn test_result_ref_clone_eq_debug() {
        let hash = ContentHash::of(b"y");
        let r = ResultRef::of_hash(hash);
        let r2 = r.clone();
        assert_eq!(r, r2);
        // Debug output must include the type name and the inner hash content.
        let debug_str = format!("{:?}", r);
        assert!(debug_str.contains("ResultRef"));
        assert!(debug_str.contains(&format!("{:?}", hash)));
    }

    // ── ErrorRef tests (step-3) ──────────────────────────────────────────────

    #[test]
    fn test_error_ref_new() {
        let err = ErrorRef::new("oops");
        assert_eq!(err.message(), "oops");
    }

    #[test]
    fn test_error_ref_from_eval_error() {
        let e = EvalError("boom".to_string());
        let via_from: ErrorRef = e.clone().into();
        assert_eq!(via_from.message(), "boom");
        // From and Into agree
        assert_eq!(ErrorRef::from(e), via_from);
    }

    #[test]
    fn test_error_ref_display_clone_eq() {
        let err = ErrorRef::new("boom");
        assert_eq!(format!("{}", err), "boom");
        let err2 = err.clone();
        assert_eq!(err, err2);
        // Debug output must include the message content
        assert!(format!("{:?}", err).contains("boom"));
    }

    // ── ErrorRef diagnostic code tests (task #2330 step-1) ──────────────────
    //
    // Pin the optional `code` field on `ErrorRef`: builders, accessors, and
    // backward-compatible defaults. See arch §9.2 lines 880-890.

    #[test]
    fn test_error_ref_new_has_no_code_by_default() {
        let err = ErrorRef::new("msg");
        assert_eq!(err.code(), None);
        // The message accessor is unaffected.
        assert_eq!(err.message(), "msg");
    }

    #[test]
    fn test_error_ref_with_code_sets_code() {
        use reify_core::diagnostics::DiagnosticCode;
        let err = ErrorRef::new("msg").with_code(DiagnosticCode::ConstraintViolated);
        assert_eq!(err.code(), Some(DiagnosticCode::ConstraintViolated));
        // The message accessor is unaffected by the builder.
        assert_eq!(err.message(), "msg");
    }

    #[test]
    fn test_error_ref_from_eval_error_defaults_code_to_none() {
        let e = EvalError("boom".to_string());
        let err: ErrorRef = e.into();
        assert_eq!(err.code(), None);
        assert_eq!(err.message(), "boom");
    }

    #[test]
    fn test_error_ref_with_code_preserves_clone_eq() {
        use reify_core::diagnostics::DiagnosticCode;
        let err = ErrorRef::new("boom").with_code(DiagnosticCode::ConstraintViolated);
        let err2 = err.clone();
        assert_eq!(err, err2);
        // Two ErrorRefs with the same message but different codes must NOT compare equal.
        let plain = ErrorRef::new("boom");
        assert_ne!(err, plain);
    }

    #[test]
    fn test_eval_error_display() {
        let err = EvalError("division by zero".to_string());
        assert_eq!(format!("{}", err), "division by zero");
        assert_eq!(err.0, "division by zero");

        // Verify Clone and PartialEq
        let err2 = err.clone();
        assert_eq!(err, err2);
    }

    #[test]
    fn value_map_get_or_undef() {
        let mut map = ValueMap::new();
        let id = ValueCellId::new("Bracket", "width");
        map.insert(id.clone(), Value::length(0.08));
        assert!(!map.get_or_undef(&id).is_undef());
        assert!(
            map.get_or_undef(&ValueCellId::new("Bracket", "missing"))
                .is_undef()
        );
    }

    #[test]
    fn value_map_clone_structural_sharing() {
        let mut original = ValueMap::new();
        let id_width = ValueCellId::new("Bracket", "width");
        let id_height = ValueCellId::new("Bracket", "height");
        let id_depth = ValueCellId::new("Bracket", "depth");

        original.insert(id_width.clone(), Value::length(0.08));
        original.insert(id_height.clone(), Value::length(0.10));

        // Clone the map (O(1) structural sharing via im::HashMap)
        let mut cloned = original.clone();

        // Insert into the clone
        cloned.insert(id_depth.clone(), Value::length(0.05));

        // Original is unmodified
        assert_eq!(original.len(), 2);
        assert!(!original.contains(&id_depth));
        assert!(original.contains(&id_width));

        // Clone has all three entries
        assert_eq!(cloned.len(), 3);
        assert!(cloned.contains(&id_depth));
        assert!(cloned.contains(&id_width));

        // Original values are still correct
        match original.get(&id_width) {
            Some(Value::Scalar { si_value, .. }) => assert!((si_value - 0.08).abs() < 1e-10),
            other => panic!("Expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn satisfaction_content_hash_deterministic() {
        // Same variant produces same hash on repeated calls
        let h1 = Satisfaction::Satisfied.content_hash();
        let h2 = Satisfaction::Satisfied.content_hash();
        assert_eq!(h1, h2);

        let h3 = Satisfaction::Violated.content_hash();
        let h4 = Satisfaction::Violated.content_hash();
        assert_eq!(h3, h4);

        let h5 = Satisfaction::Indeterminate.content_hash();
        let h6 = Satisfaction::Indeterminate.content_hash();
        assert_eq!(h5, h6);
    }

    #[test]
    fn determinacy_state_auto_exists_and_is_distinct() {
        // Auto variant should exist and be distinct from other variants
        let auto = DeterminacyState::Auto;
        let determined = DeterminacyState::Determined;
        let undetermined = DeterminacyState::Undetermined;
        let provisional = DeterminacyState::Provisional;

        assert_ne!(auto, determined);
        assert_ne!(auto, undetermined);
        assert_ne!(auto, provisional);
    }

    #[test]
    fn determinacy_state_auto_is_copy_clone_eq_hash() {
        let auto = DeterminacyState::Auto;
        let auto2 = auto; // Copy
        assert_eq!(auto, auto2); // PartialEq + Eq

        #[allow(clippy::clone_on_copy)]
        let auto3 = auto.clone(); // Clone
        assert_eq!(auto, auto3);

        // Hash: usable as HashMap key
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(auto, "auto");
        assert_eq!(map.get(&DeterminacyState::Auto), Some(&"auto"));
    }

    #[test]
    fn determinacy_state_auto_discriminant_is_3() {
        // Determined=0, Undetermined=1, Provisional=2, Auto=3
        assert_eq!(DeterminacyState::Determined as u8, 0);
        assert_eq!(DeterminacyState::Undetermined as u8, 1);
        assert_eq!(DeterminacyState::Provisional as u8, 2);
        assert_eq!(DeterminacyState::Auto as u8, 3);
    }

    // --- Ord tests (step-1) ---

    // NOTE: Several negative-float ordering tests below validate that our
    // `total_cmp()`-based Ord impl handles negative values correctly. The old
    // `to_bits().cmp()` approach gave wrong ordering for negatives because the
    // sign bit is the MSB of the u64 representation — see the module-level
    // "Float ordering strategy" doc at the top of this file for details.

    #[test]
    fn value_ord_cross_type_ordering() {
        // Undef < Bool < Int < Real < Scalar < String
        let undef = Value::Undef;
        let bool_f = Value::Bool(false);
        let bool_t = Value::Bool(true);
        let int0 = Value::Int(0);
        let real0 = Value::Real(0.0);
        let scalar = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let string = Value::String("z".into());

        assert!(undef < bool_f);
        assert!(bool_f < bool_t);
        assert!(bool_t < int0);
        assert!(int0 < real0);
        assert!(real0 < scalar);
        assert!(scalar < string);
    }

    #[test]
    fn value_ord_int_ordering() {
        assert!(Value::Int(1) < Value::Int(2));
        assert!(Value::Int(-10) < Value::Int(0));
        assert_eq!(Value::Int(5).cmp(&Value::Int(5)), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_ord_string_ordering() {
        assert!(Value::String("a".into()) < Value::String("b".into()));
        assert!(Value::String("abc".into()) < Value::String("abd".into()));
    }

    #[test]
    fn value_ord_real_nan_total_order() {
        // Normal ordering still holds
        assert!(Value::Real(1.0) < Value::Real(2.0));
        // Under total_cmp() (IEEE 754 totalOrder), NaN's canonical bits
        // (0x7FF8_0000_0000_0000) are numerically greater than +Infinity's bits
        // (0x7FF0_0000_0000_0000), so NaN sorts after +Infinity.
        let nan1 = Value::Real(f64::NAN);
        let nan2 = Value::Real(f64::NAN);
        let inf = Value::Real(f64::INFINITY);
        // Two independently constructed NaN values compare Equal under Ord
        // (same canonical bit pattern → total_cmp returns Equal).
        assert_eq!(nan1.cmp(&nan2), std::cmp::Ordering::Equal);
        // NaN sorts strictly after +Infinity
        assert_eq!(nan1.cmp(&inf), std::cmp::Ordering::Greater);
        assert_eq!(inf.cmp(&nan1), std::cmp::Ordering::Less);
    }

    #[test]
    fn value_ord_real_negative_zero() {
        // -0.0 and +0.0 have different bits and different Ord positions.
        // With total_cmp() (IEEE 754 totalOrder): -0.0 < +0.0.
        // PartialEq still distinguishes them (different bit patterns — content hash invariant).
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        // neg passes first: under IEEE 754 totalOrder, -0.0 < +0.0.
        assert_ord_consistent(&neg, &pos, false);
    }

    #[test]
    fn value_ord_real_nan_and_neg_zero_still_consistent() {
        // Ord-only consistency checks for NaN and negative zero.
        // PartialEq NaN coverage lives in nan_partialeq_bit_identity.

        // NaN: total_cmp() places NaN after +Infinity, giving it a defined position.
        let nan = Value::Real(f64::NAN);
        let inf = Value::Real(f64::INFINITY);
        let neg_inf = Value::Real(f64::NEG_INFINITY);
        // NaN > +Infinity (total_cmp: NaN is the maximum)
        assert!(nan > inf);
        // -Infinity < NaN
        assert!(neg_inf < nan);

        // -0.0 vs +0.0: with total_cmp(), -0.0 < +0.0 (IEEE 754 totalOrder).
        // This is a BEHAVIORAL CHANGE from to_bits() where +0.0 (0x0) < -0.0 (0x8000...).
        // The new ordering matches mathematical intuition: -0 < +0.
        let pos_zero = Value::Real(0.0_f64);
        let neg_zero = Value::Real(-0.0_f64);
        assert!(neg_zero < pos_zero);
    }

    #[test]
    fn value_btreeset_negative_real_iteration_order() {
        // End-to-end validation: inserting negative reals into a BTreeSet and
        // iterating must yield mathematical order [-2.0, -1.0, -0.5, 0.5, 1.0, 2.0].
        let mut set = BTreeSet::new();
        for v in &[-2.0_f64, 1.0, -0.5, 0.5, -1.0, 2.0] {
            set.insert(Value::Real(*v));
        }
        let sorted: Vec<f64> = set
            .iter()
            .map(|v| match v {
                Value::Real(f) => *f,
                _ => panic!("unexpected value"),
            })
            .collect();
        assert_eq!(sorted, vec![-2.0, -1.0, -0.5, 0.5, 1.0, 2.0]);
    }

    #[test]
    fn value_btreeset_boundary_real_iteration_order() {
        // End-to-end boundary coverage: BTreeSet iteration must yield the
        // IEEE 754 totalOrder sequence for all boundary cases.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        //
        // This subsumes value_ord_real_negative_vs_positive and
        // value_ord_real_negative_magnitude which test a subset of these pairings.
        let mut set = BTreeSet::new();
        for &v in BOUNDARY_REALS {
            set.insert(Value::Real(v));
        }
        let sorted: Vec<f64> = set
            .iter()
            .map(|v| match v {
                Value::Real(f) => *f,
                _ => panic!("unexpected value"),
            })
            .collect();

        assert_ieee754_total_order_real(&sorted);
    }

    #[test]
    fn value_ord_scalar_negative_ordering() {
        // Negative scalar values must order correctly.
        let neg1 = Value::Scalar {
            si_value: -1.0,
            dimension: DimensionVector::LENGTH,
        };
        let neg_half = Value::Scalar {
            si_value: -0.5,
            dimension: DimensionVector::LENGTH,
        };
        let pos_half = Value::Scalar {
            si_value: 0.5,
            dimension: DimensionVector::LENGTH,
        };
        // -1.0 < -0.5 < 0.5 within same dimension
        assert!(neg1 < neg_half);
        assert!(neg_half < pos_half);
    }

    #[test]
    fn value_ord_scalar_dimension_dominance_negative() {
        // Dimension ordering dominates even when si_value is negative.
        // LENGTH=[1,0,...] > MASS=[0,1,...] lexicographically, so
        // Scalar{-1.0, MASS} < Scalar{-1.0, LENGTH} regardless of the
        // total_cmp() result on si_value.
        let mass_neg = Value::Scalar {
            si_value: -1.0,
            dimension: DimensionVector::MASS,
        };
        let length_neg = Value::Scalar {
            si_value: -1.0,
            dimension: DimensionVector::LENGTH,
        };
        assert!(mass_neg < length_neg);
    }

    /// Asserts the PartialEq↔Ord two-sided contract for a pair of `Value`s.
    ///
    /// When `expect_equal` is `true`, asserts `a == b` and `a.cmp(b) == Equal`.
    /// When `expect_equal` is `false`, asserts `a != b`, `a.cmp(b) != Equal`,
    /// antisymmetry (`a.cmp(b) == b.cmp(a).reverse()`), and ordering direction
    /// (`a < b` — caller must pass the smaller value as `a`).
    fn assert_ord_consistent(a: &Value, b: &Value, expect_equal: bool) {
        if expect_equal {
            assert_eq!(a, b, "PartialEq↔Ord contract: expected a == b");
            assert_eq!(
                a.cmp(b),
                std::cmp::Ordering::Equal,
                "PartialEq↔Ord contract: expected a.cmp(b) == Equal when a == b"
            );
            assert_eq!(
                b.cmp(a),
                std::cmp::Ordering::Equal,
                "PartialEq↔Ord contract: expected b.cmp(a) == Equal when a == b"
            );
        } else {
            assert_ne!(a, b, "PartialEq↔Ord contract: expected a != b");
            assert_ne!(
                a.cmp(b),
                std::cmp::Ordering::Equal,
                "PartialEq↔Ord contract: expected a.cmp(b) != Equal when a != b"
            );
            assert_eq!(
                a.cmp(b),
                b.cmp(a).reverse(),
                "PartialEq↔Ord contract: antisymmetry violated"
            );
            assert!(
                a < b,
                "PartialEq↔Ord contract: expected a < b (caller must pass smaller value first)"
            );
        }
    }

    /// Asserts that `floats` contains exactly `BOUNDARY_REALS.len()` bit-distinct
    /// f64 values in the IEEE 754 totalOrder sequence:
    ///   NEG_INFINITY < -1.0 < -0.0 < +0.0 < 1.0 < INFINITY < NaN
    ///
    /// Positions are identified by property (sign, magnitude, is_nan, is_infinite)
    /// rather than by index so the helper is robust to future reorderings of the
    /// input array.
    fn assert_ieee754_total_order_real(floats: &[f64]) {
        assert_eq!(
            floats.len(),
            BOUNDARY_REALS.len(),
            "expected exactly {} bit-distinct boundary values, got {}",
            BOUNDARY_REALS.len(),
            floats.len()
        );

        let neg_inf_idx = floats
            .iter()
            .position(|f| f.is_infinite() && f.is_sign_negative())
            .expect("NEG_INFINITY must be present");
        let neg_one_idx = floats
            .iter()
            .position(|&f| f == -1.0_f64)
            .expect("-1.0 must be present");
        let neg_zero_idx = floats
            .iter()
            .position(|f| *f == 0.0 && f.is_sign_negative())
            .expect("-0.0 must be present");
        let pos_zero_idx = floats
            .iter()
            .position(|f| *f == 0.0 && f.is_sign_positive())
            .expect("+0.0 must be present");
        let pos_one_idx = floats
            .iter()
            .position(|&f| f == 1.0_f64)
            .expect("1.0 must be present");
        let pos_inf_idx = floats
            .iter()
            .position(|f| f.is_infinite() && f.is_sign_positive())
            .expect("INFINITY must be present");
        let nan_idx = floats
            .iter()
            .position(|f| f.is_nan())
            .expect("NaN must be present");

        // Full ordering: NEG_INFINITY < -1.0 < -0.0 < +0.0 < 1.0 < INFINITY < NaN
        assert!(
            neg_inf_idx < neg_one_idx,
            "NEG_INFINITY must come before -1.0"
        );
        assert!(neg_one_idx < neg_zero_idx, "-1.0 must come before -0.0");
        assert!(neg_zero_idx < pos_zero_idx, "-0.0 must come before +0.0");
        assert!(pos_zero_idx < pos_one_idx, "+0.0 must come before 1.0");
        assert!(pos_one_idx < pos_inf_idx, "1.0 must come before INFINITY");
        assert!(pos_inf_idx < nan_idx, "INFINITY must come before NaN");
    }

    /// Parameterized sibling of [`assert_ieee754_total_order_real`] for boundary-Set
    /// iteration-order checks.  Builds a `BTreeSet<Value>` by inserting `build(v)` for
    /// each value in `BOUNDARY_REALS`, wraps it in `Value::Set`, iterates it, extracts
    /// the discriminating `f64` field via `extract`, and delegates to
    /// `assert_ieee754_total_order_real` to assert the IEEE 754 totalOrder sequence.
    fn assert_boundary_set_iteration_order(build: fn(f64) -> Value, extract: fn(&Value) -> f64) {
        let mut inner = BTreeSet::new();
        for &v in BOUNDARY_REALS {
            inner.insert(build(v));
        }
        let set_val = Value::Set(inner);
        let sorted: Vec<f64> = if let Value::Set(ref s) = set_val {
            s.iter().map(extract).collect()
        } else {
            panic!("expected Set");
        };
        assert_ieee754_total_order_real(&sorted);
    }

    #[test]
    fn test_assert_ord_consistent_equal() {
        // Meta-test: verify assert_ord_consistent works for an equal pair.
        assert_ord_consistent(&Value::Int(5), &Value::Int(5), true);
    }

    #[test]
    fn test_assert_ord_consistent_not_equal() {
        // Meta-test: verify assert_ord_consistent works for a non-equal pair.
        // Value::Int(1) < Value::Int(2), so pass the smaller value first.
        assert_ord_consistent(&Value::Int(1), &Value::Int(2), false);
    }

    #[test]
    fn test_assert_ord_consistent_equal_real() {
        // Meta-test: exercises the to_bits()-based PartialEq and total_cmp()-based
        // Ord paths through the strengthened helper (including the b.cmp(a) check)
        // for an equal float pair.
        assert_ord_consistent(&Value::Real(3.125), &Value::Real(3.125), true);
    }

    #[test]
    fn test_assert_ord_consistent_not_equal_real() {
        // Meta-test: exercises the total_cmp() ordering and antisymmetry check
        // for a non-equal float pair. Value::Real(1.0) < Value::Real(2.0) under
        // total_cmp(), so pass the smaller value first.
        assert_ord_consistent(&Value::Real(1.0), &Value::Real(2.0), false);
    }

    #[test]
    fn test_assert_ord_consistent_real_neg_zero() {
        // Meta-test: exercises the to_bits()-based PartialEq path for bare
        // Value::Real with the -0.0 vs +0.0 edge case.
        // PartialEq uses to_bits(): -0.0 and +0.0 have different bit patterns → not equal.
        // Under f64::total_cmp(), -0.0 < +0.0, so pass -0.0 as the smaller value first.
        assert_ord_consistent(&Value::Real(-0.0), &Value::Real(0.0), false);
    }

    #[test]
    fn test_assert_ord_consistent_real_nan() {
        // Meta-test: exercises the to_bits()-based PartialEq path for bare
        // Value::Real with the NaN self-equality edge case.
        // PartialEq uses to_bits(): identical NaN bit patterns → equal.
        assert_ord_consistent(&Value::Real(f64::NAN), &Value::Real(f64::NAN), true);
    }

    #[test]
    fn test_assert_ord_consistent_real_nan_distinct_payloads() {
        // Meta-test: exercises the to_bits()-based PartialEq path for bare
        // Value::Real with two distinct NaN bit patterns.
        // Canonical NaN (0x7ff8_0000_0000_0000) vs payload NaN (0x7ff8_0000_0000_0001).
        // PartialEq uses to_bits(): distinct bit patterns → not equal.
        // Under f64::total_cmp(), canonical NaN < payload NaN (compared by bit representation),
        // so pass the canonical NaN as the smaller value first.
        let canonical_nan = Value::Real(f64::from_bits(0x7ff8_0000_0000_0000));
        let payload_nan = Value::Real(f64::from_bits(0x7ff8_0000_0000_0001));
        assert_ord_consistent(&canonical_nan, &payload_nan, false);
    }

    #[test]
    fn test_assert_ieee754_total_order_real_correct_order() {
        // Meta-test: assert_ieee754_total_order_real must not panic when given the
        // correct IEEE 754 totalOrder sequence.
        assert_ieee754_total_order_real(&[
            f64::NEG_INFINITY,
            -1.0_f64,
            -0.0_f64,
            0.0_f64,
            1.0_f64,
            f64::INFINITY,
            f64::NAN,
        ]);
    }

    #[test]
    #[should_panic(expected = "-0.0 must come before +0.0")]
    fn test_assert_ieee754_total_order_real_wrong_order() {
        // Meta-test: assert_ieee754_total_order_real must panic when -0.0 and +0.0
        // are swapped (violating the IEEE 754 totalOrder requirement that -0.0
        // precedes +0.0).
        assert_ieee754_total_order_real(&[
            f64::NEG_INFINITY,
            -1.0_f64,
            0.0_f64,  // +0.0 in the -0.0 position → wrong order
            -0.0_f64, // -0.0 in the +0.0 position → wrong order
            1.0_f64,
            f64::INFINITY,
            f64::NAN,
        ]);
    }

    #[test]
    #[should_panic(expected = "expected exactly")]
    fn test_assert_ieee754_total_order_real_wrong_count() {
        // Meta-test: assert_ieee754_total_order_real must panic when passed a slice
        // that does not contain exactly BOUNDARY_REALS.len() values.  Here we drop
        // NaN so the slice has only 6 elements; the length guard should fire with a
        // message containing "expected exactly".
        assert_ieee754_total_order_real(&[
            f64::NEG_INFINITY,
            -1.0_f64,
            -0.0_f64,
            0.0_f64,
            1.0_f64,
            f64::INFINITY,
            // NaN intentionally omitted → 6 elements instead of 7
        ]);
    }

    #[test]
    fn test_assert_boundary_set_iteration_order_using_real() {
        // Meta-test: assert_boundary_set_iteration_order must not panic when given
        // Value::Real as the build function and the corresponding Real extractor.
        // This documents the helper's contract using the simplest Value variant.
        assert_boundary_set_iteration_order(Value::Real, |v| match v {
            Value::Real(f) => *f,
            _ => panic!("unexpected value"),
        });
    }

    #[test]
    #[should_panic(expected = "unexpected value")]
    fn test_assert_boundary_set_iteration_order_variant_mismatch() {
        // Meta-test: mirrors test_assert_ieee754_total_order_real_wrong_order and
        // _wrong_count for assert_boundary_set_iteration_order, proving that the
        // helper propagates panics from the extract closure.
        //
        // build produces Value::Real entries, but extract only matches Value::Complex;
        // every element falls through to the `_` arm and panics with "unexpected value".
        assert_boundary_set_iteration_order(Value::Real, |v| match v {
            Value::Complex { im, .. } => *im,
            _ => panic!("unexpected value"),
        });
    }

    #[test]
    fn value_ord_real_negative_nan() {
        // Under f64::total_cmp() (IEEE 754 totalOrder), negative NaN bit patterns
        // sort before -Infinity: neg_qNaN < neg_sNaN < -Inf < ... < +Inf < pos_sNaN < pos_qNaN.
        //
        // Negative quiet NaN: sign bit set, exponent all-1s, quiet bit set (0xfff8_0000_0000_0000).
        let neg_qnan = Value::Real(f64::from_bits(0xfff8_0000_0000_0000));
        let neg_inf = Value::Real(f64::NEG_INFINITY);
        let pos_qnan = Value::Real(f64::from_bits(0x7ff8_0000_0000_0000));

        // neg_qnan < neg_inf under f64::total_cmp().
        assert_ord_consistent(&neg_qnan, &neg_inf, false);
        // neg_qnan < pos_qnan (cross-sign NaN pair).
        assert_ord_consistent(&neg_qnan, &pos_qnan, false);
    }

    #[test]
    fn value_ord_real_signaling_nan() {
        // Under f64::total_cmp() (IEEE 754 totalOrder), signaling NaN (quiet bit CLEAR)
        // sits between infinity and quiet NaN on each side:
        //   neg_qnan < neg_snan < -Inf < ... < +Inf < pos_snan < pos_qnan
        //
        // Positive sNaN: sign=0, exp=all-1s, quiet=0, non-zero mantissa.
        // Negative sNaN: sign=1, exp=all-1s, quiet=0, non-zero mantissa.
        let pos_snan = Value::Real(f64::from_bits(0x7ff0_0000_0000_0001));
        let neg_snan = Value::Real(f64::from_bits(0xfff0_0000_0000_0001));
        let pos_inf = Value::Real(f64::INFINITY);
        let neg_inf = Value::Real(f64::NEG_INFINITY);
        let pos_qnan = Value::Real(f64::from_bits(0x7ff8_0000_0000_0000));
        let neg_qnan = Value::Real(f64::from_bits(0xfff8_0000_0000_0000));

        // assert_ord_consistent for the pos_inf < pos_snan pair.
        assert_ord_consistent(&pos_inf, &pos_snan, false);
        // assert_ord_consistent for the neg_qnan < neg_snan pair.
        assert_ord_consistent(&neg_qnan, &neg_snan, false);
        // assert_ord_consistent for the neg_snan < neg_inf boundary (neg_snan is smaller).
        assert_ord_consistent(&neg_snan, &neg_inf, false);
        // assert_ord_consistent for the pos_snan < pos_qnan boundary (pos_snan is smaller).
        assert_ord_consistent(&pos_snan, &pos_qnan, false);
    }

    #[test]
    fn value_scalar_bit_identity_neg_zero_and_nan_consistent() {
        // Verifies the two-sided contract: a == b IFF a.cmp(&b) == Ordering::Equal,
        // for the Scalar variant's bit-identity edge cases.

        // --- neg-zero vs pos-zero ---
        let pos_zero = Value::Scalar {
            si_value: 0.0_f64,
            dimension: DimensionVector::LENGTH,
        };
        let neg_zero = Value::Scalar {
            si_value: -0.0_f64,
            dimension: DimensionVector::LENGTH,
        };
        // PartialEq uses to_bits(): -0.0 and +0.0 have different bit patterns → not equal.
        // IEEE 754 totalOrder: -0.0 < +0.0, so pass neg_zero as the smaller value.
        assert_ord_consistent(&neg_zero, &pos_zero, false);

        // --- NaN self-equality ---
        let nan_a = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        let nan_b = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        // PartialEq uses to_bits(): identical NaN bit patterns → equal.
        assert_ord_consistent(&nan_a, &nan_b, true);
        // IEEE 754 totalOrder: NaN sorts strictly after +Infinity.
        let inf = Value::Scalar {
            si_value: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        };
        assert!(nan_a > inf);

        // --- distinct NaN payloads: canonical NaN vs payload NaN ---
        // Strengthens the to_bits() contract: not all NaN values are equivalent.
        // 0x7ff8_0000_0000_0000 is canonical quiet NaN; 0x7ff8_0000_0000_0001 differs by 1 bit.
        let nan_canonical = Value::Scalar {
            si_value: f64::NAN, // 0x7ff8_0000_0000_0000
            dimension: DimensionVector::LENGTH,
        };
        let nan_payload = Value::Scalar {
            si_value: f64::from_bits(0x7ff8_0000_0000_0001),
            dimension: DimensionVector::LENGTH,
        };
        // PartialEq uses to_bits(): different bit patterns → not equal.
        assert_ne!(nan_canonical, nan_payload);
        // Ord must also distinguish them (different total_cmp ordering).
        assert_ne!(nan_canonical.cmp(&nan_payload), std::cmp::Ordering::Equal);
        // Antisymmetry.
        assert_eq!(
            nan_canonical.cmp(&nan_payload),
            nan_payload.cmp(&nan_canonical).reverse()
        );
    }

    // --- Option tests (step-11) ---

    #[test]
    fn value_option_some_and_none() {
        let some = Value::Option(Some(Box::new(Value::Int(42))));
        let none = Value::Option(None);
        assert_ne!(some, none);
    }

    #[test]
    fn value_option_equality() {
        let a = Value::Option(Some(Box::new(Value::Int(42))));
        let b = Value::Option(Some(Box::new(Value::Int(42))));
        let c = Value::Option(Some(Box::new(Value::Int(99))));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(Value::Option(None), Value::Option(None));
    }

    #[test]
    fn value_option_ordering() {
        // None < Some(anything)
        assert!(Value::Option(None) < Value::Option(Some(Box::new(Value::Int(0)))));
        // Some orders by inner value
        assert!(
            Value::Option(Some(Box::new(Value::Int(1))))
                < Value::Option(Some(Box::new(Value::Int(2))))
        );
        // Option sorts after Map
        let m = Value::Map(std::collections::BTreeMap::new());
        assert!(m < Value::Option(None));
    }

    #[test]
    fn value_option_content_hash() {
        let some1 = Value::Option(Some(Box::new(Value::Int(1))));
        let some2 = Value::Option(Some(Box::new(Value::Int(1))));
        let some3 = Value::Option(Some(Box::new(Value::Int(2))));
        let none = Value::Option(None);
        assert_eq!(some1.content_hash(), some2.content_hash());
        assert_ne!(some1.content_hash(), some3.content_hash());
        assert_ne!(some1.content_hash(), none.content_hash());
    }

    // --- Map tests (step-9) ---

    #[test]
    fn value_map_basic() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("a".into()), Value::Int(1));
        m.insert(Value::String("b".into()), Value::Int(2));
        let v = Value::Map(m);
        if let Value::Map(ref inner) = v {
            assert_eq!(inner.len(), 2);
            assert_eq!(inner.get(&Value::String("a".into())), Some(&Value::Int(1)));
        } else {
            panic!("expected Map");
        }
    }

    #[test]
    fn value_map_equality() {
        let mut m1 = BTreeMap::new();
        m1.insert(Value::String("a".into()), Value::Int(1));
        let mut m2 = BTreeMap::new();
        m2.insert(Value::String("a".into()), Value::Int(1));
        let mut m3 = BTreeMap::new();
        m3.insert(Value::String("a".into()), Value::Int(2));
        assert_eq!(Value::Map(m1), Value::Map(m2.clone()));
        assert_ne!(Value::Map(m2), Value::Map(m3));
    }

    #[test]
    fn value_map_ordering() {
        // Map sorts after Set
        let s = Value::Set(std::collections::BTreeSet::new());
        let m = Value::Map(BTreeMap::new());
        assert!(s < m);
    }

    #[test]
    fn value_map_content_hash() {
        let mut m1 = BTreeMap::new();
        m1.insert(Value::String("a".into()), Value::Int(1));
        let mut m2 = BTreeMap::new();
        m2.insert(Value::String("a".into()), Value::Int(1));
        let mut m3 = BTreeMap::new();
        m3.insert(Value::String("a".into()), Value::Int(2));
        assert_eq!(
            Value::Map(m1).content_hash(),
            Value::Map(m2.clone()).content_hash()
        );
        assert_ne!(Value::Map(m2).content_hash(), Value::Map(m3).content_hash());
    }

    // --- Set tests (step-7) ---

    #[test]
    fn value_set_basic() {
        let mut s = BTreeSet::new();
        s.insert(Value::Int(3));
        s.insert(Value::Int(1));
        s.insert(Value::Int(2));
        let v = Value::Set(s);
        // Verify it contains all elements
        if let Value::Set(ref inner) = v {
            assert_eq!(inner.len(), 3);
            assert!(inner.contains(&Value::Int(1)));
            assert!(inner.contains(&Value::Int(2)));
            assert!(inner.contains(&Value::Int(3)));
        } else {
            panic!("expected Set");
        }
    }

    #[test]
    fn value_set_equality() {
        let mut s1 = BTreeSet::new();
        s1.insert(Value::Int(1));
        s1.insert(Value::Int(2));
        let mut s2 = BTreeSet::new();
        s2.insert(Value::Int(2));
        s2.insert(Value::Int(1)); // same elements, different insertion order
        assert_eq!(Value::Set(s1), Value::Set(s2));
    }

    #[test]
    fn value_set_ordering() {
        let mut s1 = BTreeSet::new();
        s1.insert(Value::Int(1));
        let mut s2 = BTreeSet::new();
        s2.insert(Value::Int(2));
        // Set sorts after List
        assert!(Value::List(vec![]) < Value::Set(s1.clone()));
        // Between sets: lexicographic on sorted elements
        assert!(Value::Set(s1) < Value::Set(s2));
    }

    #[test]
    fn value_set_content_hash() {
        let mut s1 = BTreeSet::new();
        s1.insert(Value::Int(1));
        s1.insert(Value::Int(2));
        let mut s2 = BTreeSet::new();
        s2.insert(Value::Int(2));
        s2.insert(Value::Int(1));
        assert_eq!(Value::Set(s1).content_hash(), Value::Set(s2).content_hash());
    }

    // --- Set/Map float-boundary iteration-order regression guards (task-974) ---

    #[test]
    fn value_set_real_boundary_iteration_order_through_variant() {
        // Mirrors value_btreeset_boundary_real_iteration_order but exercises the
        // Value::Set wrapper rather than a bare BTreeSet<Value>.
        // Expected IEEE 754 totalOrder: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        let mut inner = BTreeSet::new();
        for &v in BOUNDARY_REALS {
            inner.insert(Value::Real(v));
        }
        let set_val = Value::Set(inner);

        let sorted: Vec<f64> = if let Value::Set(ref s) = set_val {
            s.iter()
                .map(|v| match v {
                    Value::Real(f) => *f,
                    _ => panic!("unexpected value"),
                })
                .collect()
        } else {
            panic!("expected Set");
        };

        assert_ieee754_total_order_real(&sorted);
    }

    #[test]
    fn value_map_real_boundary_key_iteration_order_through_variant() {
        // Mirrors value_set_real_boundary_iteration_order_through_variant but for
        // Value::Map: boundary floats are used as keys, each mapped to a distinct
        // sentinel Value::Int so we can verify key-iteration order.
        // Expected IEEE 754 totalOrder: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        let mut inner = BTreeMap::new();
        for (i, &v) in BOUNDARY_REALS.iter().enumerate() {
            inner.insert(Value::Real(v), Value::Int(i as i64));
        }
        let map_val = Value::Map(inner);

        let sorted_keys: Vec<f64> = if let Value::Map(ref m) = map_val {
            m.keys()
                .map(|v| match v {
                    Value::Real(f) => *f,
                    _ => panic!("unexpected key"),
                })
                .collect()
        } else {
            panic!("expected Map");
        };

        assert_ieee754_total_order_real(&sorted_keys);
    }

    #[test]
    fn value_set_round_trip_preserves_iteration_order() {
        // Round-trip guard: collect iteration order from a Value::Set, rebuild a
        // fresh BTreeSet from the collected sequence, and verify the golden ordering.
        //
        // Parts a-c (structural equality, content_hash identity, iteration-sequence
        // preservation) are tautological given BTreeSet stdlib guarantees: Ord alone
        // determines iteration order, not insertion order, so rebuilding from any
        // sequence produces the same BTreeSet. The real regression value is the golden
        // ordering assertion below.
        let mut original_inner = BTreeSet::new();
        for &v in BOUNDARY_REALS {
            original_inner.insert(Value::Real(v));
        }
        let original = Value::Set(original_inner);

        // Collect iteration order, rebuild, then assert the golden IEEE 754 totalOrder
        let collected: Vec<Value> = if let Value::Set(ref s) = original {
            s.iter().cloned().collect()
        } else {
            panic!("expected Set");
        };
        let floats: Vec<f64> = collected
            .iter()
            .map(|v| match v {
                Value::Real(f) => *f,
                _ => panic!("unexpected value"),
            })
            .collect();
        assert_ieee754_total_order_real(&floats);
    }

    #[test]
    fn value_map_round_trip_preserves_key_iteration_order() {
        // Round-trip guard for Value::Map: collect (key, value) pairs via iter(),
        // rebuild a fresh BTreeMap, and verify the golden key-ordering.
        //
        // Parts a-c (structural equality, content_hash identity, key-sequence
        // preservation) are tautological given BTreeMap stdlib guarantees: Ord alone
        // determines key iteration order, not insertion order, so rebuilding from any
        // sequence produces the same BTreeMap. The real regression value is the golden
        // ordering assertion below.
        let mut original_inner = BTreeMap::new();
        for (i, &v) in BOUNDARY_REALS.iter().enumerate() {
            original_inner.insert(Value::Real(v), Value::Int(i as i64));
        }
        let original = Value::Map(original_inner);

        // Collect iteration order, then assert the golden IEEE 754 totalOrder on keys
        let collected: Vec<(Value, Value)> = if let Value::Map(ref m) = original {
            m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        } else {
            panic!("expected Map");
        };
        let keys: Vec<f64> = collected
            .iter()
            .map(|(k, _)| match k {
                Value::Real(f) => *f,
                _ => panic!("unexpected key"),
            })
            .collect();
        assert_ieee754_total_order_real(&keys);
    }

    // --- Cross-variant float-boundary iteration-order tests (task-1434) ---

    #[test]
    fn value_set_scalar_boundary_iteration_order() {
        // Exercises the Scalar arm of the Value Ord impl: with identical dimensions,
        // ordering falls through to si_value.total_cmp. Inserts 7 boundary f64
        // values as Value::Scalar entries (all with dimension=LENGTH), then asserts
        // the IEEE 754 totalOrder sequence matches BTreeSet iteration order.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| Value::Scalar {
                si_value: v,
                dimension: DimensionVector::LENGTH,
            },
            |v| match v {
                Value::Scalar { si_value, .. } => *si_value,
                _ => panic!("unexpected value"),
            },
        );
    }

    #[test]
    fn value_set_complex_boundary_im_iteration_order() {
        // Exercises the im-component fallthrough in the Complex arm of Value::Ord.
        // All 7 entries share dimension=DIMENSIONLESS and re=0.0, so the ordering
        // falls through to im.total_cmp. Asserts the IEEE 754 totalOrder sequence
        // from BTreeSet iteration over the im components.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| Value::Complex {
                re: 0.0,
                im: v,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            |v| match v {
                Value::Complex { im, .. } => *im,
                _ => panic!("unexpected value"),
            },
        );
    }

    #[test]
    fn value_set_orientation_boundary_z_iteration_order() {
        // Exercises the z-component fallthrough in the Orientation arm of Value::Ord.
        // The Ord impl chains w → x → y → z via total_cmp. With w=0.0, x=0.0, y=0.0
        // for all entries, ordering falls through to z.total_cmp. Asserts the IEEE
        // 754 totalOrder sequence from BTreeSet iteration over the z components.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| orient(0.0, 0.0, 0.0, v),
            |v| match v {
                Value::Orientation { z, .. } => *z,
                _ => panic!("unexpected value"),
            },
        );
    }

    #[test]
    fn value_set_complex_boundary_re_iteration_order() {
        // Exercises the re-component fallthrough in the Complex arm of Value::Ord.
        // The Ord impl chains dimension → re → im via total_cmp. With
        // dimension=DIMENSIONLESS and im=0.0 for all entries, ordering falls
        // through to re.total_cmp. Asserts the IEEE 754 totalOrder sequence from
        // BTreeSet iteration over the re components.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| Value::Complex {
                re: v,
                im: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            |v| match v {
                Value::Complex { re, .. } => *re,
                _ => panic!("unexpected value"),
            },
        );
    }

    #[test]
    fn value_set_orientation_boundary_w_iteration_order() {
        // Exercises the w-component (first) in the Orientation arm of Value::Ord.
        // The Ord impl chains w → x → y → z via total_cmp. With x=0.0, y=0.0,
        // z=0.0 for all entries, ordering is determined entirely by w.total_cmp.
        // Asserts the IEEE 754 totalOrder sequence from BTreeSet iteration over
        // the w components.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| orient(v, 0.0, 0.0, 0.0),
            |v| match v {
                Value::Orientation { w, .. } => *w,
                _ => panic!("unexpected value"),
            },
        );
    }

    #[test]
    fn value_set_orientation_boundary_x_iteration_order() {
        // Exercises the x-component fallthrough in the Orientation arm of Value::Ord.
        // The Ord impl chains w → x → y → z via total_cmp. With w=0.0, y=0.0,
        // z=0.0 for all entries, ordering falls through to x.total_cmp. Asserts
        // the IEEE 754 totalOrder sequence from BTreeSet iteration over the x
        // components.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| orient(0.0, v, 0.0, 0.0),
            |v| match v {
                Value::Orientation { x, .. } => *x,
                _ => panic!("unexpected value"),
            },
        );
    }

    #[test]
    fn value_set_orientation_boundary_y_iteration_order() {
        // Exercises the y-component fallthrough in the Orientation arm of Value::Ord.
        // The Ord impl chains w → x → y → z via total_cmp. With w=0.0, x=0.0,
        // z=0.0 for all entries, ordering falls through to y.total_cmp. Asserts
        // the IEEE 754 totalOrder sequence from BTreeSet iteration over the y
        // components.
        // Expected order: [NEG_INFINITY, -1.0, -0.0, +0.0, 1.0, INFINITY, NaN]
        assert_boundary_set_iteration_order(
            |v| orient(0.0, 0.0, v, 0.0),
            |v| match v {
                Value::Orientation { y, .. } => *y,
                _ => panic!("unexpected value"),
            },
        );
    }

    // --- List tests (step-5) ---

    #[test]
    fn value_list_equality() {
        let a = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let c = Value::List(vec![Value::Int(1), Value::Int(3)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn value_list_empty() {
        let empty = Value::List(vec![]);
        let non_empty = Value::List(vec![Value::Int(1)]);
        assert_ne!(empty, non_empty);
        assert_eq!(empty, Value::List(vec![]));
    }

    #[test]
    fn value_list_nested() {
        let nested = Value::List(vec![
            Value::List(vec![Value::Int(1)]),
            Value::List(vec![Value::Int(2)]),
        ]);
        let nested2 = Value::List(vec![
            Value::List(vec![Value::Int(1)]),
            Value::List(vec![Value::Int(2)]),
        ]);
        assert_eq!(nested, nested2);
    }

    #[test]
    fn value_list_ordering() {
        // Lexicographic ordering
        let a = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(3)]);
        assert!(a < b);

        // Shorter list < longer list with same prefix
        let short = Value::List(vec![Value::Int(1)]);
        let long = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert!(short < long);

        // List sorts after Enum
        let enum_val = Value::Enum {
            type_name: "Z".into(),
            variant: "Z".into(),
        };
        assert!(enum_val < Value::List(vec![]));
    }

    #[test]
    fn value_list_content_hash() {
        let a = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let c = Value::List(vec![Value::Int(2), Value::Int(1)]);
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), c.content_hash());
    }

    // --- Enum tests (step-3) ---

    #[test]
    fn value_enum_debug() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let dbg = format!("{:?}", v);
        assert!(dbg.contains("Color"));
        assert!(dbg.contains("Red"));
    }

    #[test]
    fn value_enum_equality() {
        let a = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let b = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let c = Value::Enum {
            type_name: "Color".into(),
            variant: "Blue".into(),
        };
        let d = Value::Enum {
            type_name: "Shape".into(),
            variant: "Red".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn value_enum_ordering() {
        let enum_val = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let string_val = Value::String("zzz".into());
        // Enum sorts after String
        assert!(enum_val > string_val);

        // Within Enum: sort by type_name then variant
        let a = Value::Enum {
            type_name: "Color".into(),
            variant: "Blue".into(),
        };
        let b = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let c = Value::Enum {
            type_name: "Shape".into(),
            variant: "A".into(),
        };
        assert!(a < b); // same type_name, Blue < Red
        assert!(b < c); // Color < Shape
    }

    #[test]
    fn value_enum_content_hash() {
        let a = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let b = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let c = Value::Enum {
            type_name: "Color".into(),
            variant: "Blue".into(),
        };
        assert_eq!(a.content_hash(), b.content_hash()); // deterministic
        assert_ne!(a.content_hash(), c.content_hash()); // distinct
    }

    #[test]
    fn satisfaction_content_hash_distinct_variants() {
        let satisfied = Satisfaction::Satisfied.content_hash();
        let violated = Satisfaction::Violated.content_hash();
        let indeterminate = Satisfaction::Indeterminate.content_hash();

        assert_ne!(satisfied, violated);
        assert_ne!(satisfied, indeterminate);
        assert_ne!(violated, indeterminate);
    }

    // --- Display tests ---

    #[test]
    fn value_display_bool() {
        assert_eq!(format!("{}", Value::Bool(true)), "true");
        assert_eq!(format!("{}", Value::Bool(false)), "false");
    }

    #[test]
    fn value_display_int() {
        assert_eq!(format!("{}", Value::Int(42)), "42");
        assert_eq!(format!("{}", Value::Int(-7)), "-7");
        assert_eq!(format!("{}", Value::Int(0)), "0");
    }

    #[test]
    fn value_display_real() {
        assert_eq!(format!("{}", Value::Real(3.15)), "3.15");
        assert_eq!(format!("{}", Value::Real(0.0)), "0");
        assert_eq!(format!("{}", Value::Real(-2.5)), "-2.5");
    }

    #[test]
    fn value_display_string() {
        assert_eq!(format!("{}", Value::String("hello".into())), "\"hello\"");
        assert_eq!(format!("{}", Value::String("".into())), "\"\"");
    }

    #[test]
    fn value_display_scalar() {
        let v = Value::length(0.08);
        assert_eq!(format!("{}", v), "0.08 m");
    }

    #[test]
    fn value_display_undef() {
        assert_eq!(format!("{}", Value::Undef), "undef");
    }

    #[test]
    fn value_display_enum() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        assert_eq!(format!("{}", v), "Color::Red");
    }

    #[test]
    fn value_display_list() {
        let v = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(format!("{}", v), "[1, 2, 3]");
        assert_eq!(format!("{}", Value::List(vec![])), "[]");
    }

    #[test]
    fn value_display_set() {
        let mut s = BTreeSet::new();
        s.insert(Value::Int(1));
        s.insert(Value::Int(2));
        assert_eq!(format!("{}", Value::Set(s)), "{1, 2}");
        assert_eq!(format!("{}", Value::Set(BTreeSet::new())), "{}");
    }

    #[test]
    fn value_display_map() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("a".into()), Value::Int(1));
        assert_eq!(format!("{}", Value::Map(m)), "{\"a\": 1}");
        assert_eq!(format!("{}", Value::Map(BTreeMap::new())), "{}");
    }

    #[test]
    fn value_display_option() {
        assert_eq!(format!("{}", Value::Option(None)), "None");
        assert_eq!(
            format!("{}", Value::Option(Some(Box::new(Value::Int(42))))),
            "Some(42)"
        );
    }

    // --- Value::Real Display large float regression tests (step-14) ---

    #[test]
    fn value_display_real_large_positive() {
        // 1e20 is beyond i64::MAX (~9.2e18), so `*r as i64` saturates to i64::MAX.
        // Expected: the full float representation, not the saturated i64 value.
        assert_eq!(format!("{}", Value::Real(1e20)), "100000000000000000000");
    }

    #[test]
    fn value_display_real_large_negative() {
        assert_eq!(format!("{}", Value::Real(-1e20)), "-100000000000000000000");
    }

    #[test]
    fn value_display_real_max_safe_integer() {
        // 2^53 = 9007199254740992, the max integer exactly representable as f64
        assert_eq!(
            format!("{}", Value::Real(9.007199254740992e15)),
            "9007199254740992"
        );
    }

    // --- Cross-domain hash collision regression tests (step-12) ---

    #[test]
    fn value_option_none_hash_not_equal_satisfaction_satisfied() {
        // Value::Option(None) and Satisfaction::Satisfied must not collide.
        // Both use tag [10] currently, which produces identical hashes.
        let value_hash = Value::Option(None).content_hash();
        let satisfaction_hash = Satisfaction::Satisfied.content_hash();
        assert_ne!(
            value_hash, satisfaction_hash,
            "Value::Option(None) hash collides with Satisfaction::Satisfied hash"
        );
    }

    #[test]
    fn value_option_some_hash_not_equal_satisfaction_violated() {
        // Value::Option(Some(Bool(true))) and Satisfaction::Violated must not collide.
        let value_hash = Value::Option(Some(Box::new(Value::Bool(true)))).content_hash();
        let satisfaction_hash = Satisfaction::Violated.content_hash();
        assert_ne!(
            value_hash, satisfaction_hash,
            "Value::Option(Some(Bool(true))) hash collides with Satisfaction::Violated hash"
        );
    }

    // --- Comprehensive tag uniqueness regression test (step-16) ---

    #[test]
    fn value_and_satisfaction_content_hash_tags_no_cross_domain_collisions() {
        // Build representative Value for each variant
        let values: Vec<(&str, Value)> = vec![
            ("Bool(false)", Value::Bool(false)),
            ("Bool(true)", Value::Bool(true)),
            ("Int(0)", Value::Int(0)),
            ("Int(1)", Value::Int(1)),
            ("Real(0.0)", Value::Real(0.0)),
            ("Real(1.0)", Value::Real(1.0)),
            ("String(empty)", Value::String(String::new())),
            ("String(a)", Value::String("a".into())),
            (
                "Scalar(0,LENGTH)",
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
            ),
            ("Undef", Value::Undef),
            (
                "Enum",
                Value::Enum {
                    type_name: "T".into(),
                    variant: "V".into(),
                },
            ),
            ("List(empty)", Value::List(vec![])),
            ("List([0])", Value::List(vec![Value::Int(0)])),
            ("Set(empty)", Value::Set(BTreeSet::new())),
            ("Map(empty)", Value::Map(BTreeMap::new())),
            ("Option(None)", Value::Option(None)),
            (
                "Option(Some(Bool(false)))",
                Value::Option(Some(Box::new(Value::Bool(false)))),
            ),
            (
                "Option(Some(Bool(true)))",
                Value::Option(Some(Box::new(Value::Bool(true)))),
            ),
        ];

        let satisfactions: Vec<(&str, ContentHash)> = vec![
            ("Satisfied", Satisfaction::Satisfied.content_hash()),
            ("Violated", Satisfaction::Violated.content_hash()),
            ("Indeterminate", Satisfaction::Indeterminate.content_hash()),
        ];

        // Every Value hash must differ from every Satisfaction hash
        for (vname, val) in &values {
            let vh = val.content_hash();
            for (sname, sh) in &satisfactions {
                assert_ne!(
                    vh, *sh,
                    "Value::{} content_hash collides with Satisfaction::{}",
                    vname, sname
                );
            }
        }
    }

    #[test]
    fn scalar_neg_zero_hash_consistency() {
        // si_value -0.0 and 0.0 are different via PartialEq (to_bits), so content_hash must differ
        let pos = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let neg = Value::Scalar {
            si_value: -0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(pos, neg);
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    // --- Field tests (step-11) ---

    #[test]
    fn value_field_variant() {
        use reify_core::ty::Type;
        let field_val = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        // Display
        let display = format!("{}", field_val);
        assert!(
            display.contains("Field"),
            "expected display to contain 'Field', got: {}",
            display
        );
        // Content hash determinism
        let field_val2 = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        assert_eq!(field_val.content_hash(), field_val2.content_hash());
        // Not equal to Undef
        assert_ne!(field_val, Value::Undef);
    }

    // --- AsPrintedZones field-source cache-key distinctness (task δ, step-3) ---

    #[test]
    fn as_printed_zones_source_distinguishes_content_hash() {
        use reify_core::ty::Type;
        // Two fields identical in domain/codomain/lambda but differing ONLY in
        // `source`: the new FDM material-field kind vs Analytical. The δ field
        // stores its per-zone data in the lambda slot (a Value::List), so a
        // representative List payload is shared by both to isolate `source` as
        // the only varying input. content_hash folds `format!("{:?}", source)`,
        // so the derived Debug string "AsPrintedZones" must yield a DIFFERENT
        // cache key than "Analytical" — otherwise an as-printed field could
        // alias a same-shaped analytical field in the compute cache.
        let shared_lambda = Arc::new(Value::List(vec![Value::Undef, Value::Undef]));
        let as_printed = Value::Field {
            domain_type: Type::point3(Type::length()),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::AsPrintedZones,
            lambda: shared_lambda.clone(),
        };
        let analytical = Value::Field {
            domain_type: Type::point3(Type::length()),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: shared_lambda,
        };
        assert_ne!(
            as_printed.content_hash(),
            analytical.content_hash(),
            "AsPrintedZones and Analytical fields with identical lambda payloads \
             must NOT share a content hash"
        );
    }

    #[test]
    fn value_display_nested() {
        // List containing Option and Enum values
        let v = Value::List(vec![
            Value::Option(Some(Box::new(Value::Int(1)))),
            Value::Enum {
                type_name: "Color".into(),
                variant: "Red".into(),
            },
            Value::Option(None),
        ]);
        assert_eq!(format!("{}", v), "[Some(1), Color::Red, None]");
    }

    #[test]
    fn value_map_remove() {
        use reify_core::identity::ValueCellId;

        let id_a = ValueCellId::new("E", "a");
        let id_b = ValueCellId::new("E", "b");
        let id_c = ValueCellId::new("E", "c");

        let mut map = ValueMap::new();
        map.insert(id_a.clone(), Value::Int(1));
        map.insert(id_b.clone(), Value::Int(2));
        map.insert(id_c.clone(), Value::Int(3));
        assert_eq!(map.len(), 3);

        // Remove the middle entry
        map.remove(&id_b);

        assert_eq!(map.len(), 2);
        assert!(map.get(&id_b).is_none(), "removed entry should be gone");
        assert_eq!(
            map.get(&id_a),
            Some(&Value::Int(1)),
            "other entries should remain"
        );
        assert_eq!(
            map.get(&id_c),
            Some(&Value::Int(3)),
            "other entries should remain"
        );
    }

    // --- Value::Tensor tests ---

    #[test]
    fn value_tensor_construction_and_partial_eq() {
        // (a) rank-1 tensor with 3 length scalars equals itself rebuilt
        let t1 = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.12),
        ]);
        let t1b = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.12),
        ]);
        assert_eq!(t1, t1b);

        // (b) tensors with different elements are unequal
        let t1c = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.99),
        ]);
        assert_ne!(t1, t1c);

        // (c) rank-2 nested tensor (Tensor of Tensors) equals itself
        let inner_a = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        let inner_b = Value::Tensor(vec![Value::Int(3), Value::Int(4)]);
        let t2 = Value::Tensor(vec![inner_a.clone(), inner_b.clone()]);
        let t2_copy = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(t2, t2_copy);

        // (d) Tensor([Int(1), Int(2)]) != List([Int(1), Int(2)]) — distinct variants
        let tensor_ints = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        let list_ints = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(tensor_ints, list_ints);
    }

    #[test]
    fn value_tensor_display() {
        // rank-1 tensor of 3 length scalars
        let t1 = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.12),
        ]);
        assert_eq!(format!("{}", t1), "[0.08 m, 0.1 m, 0.12 m]");

        // rank-2 nested tensor of Ints
        let t2 = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(format!("{}", t2), "[[1, 2], [3, 4]]");
    }

    #[test]
    fn value_tensor_content_hash_determinism() {
        // (a) identical rank-1 tensors produce identical hashes
        let t1 = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let t1b = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(t1.content_hash(), t1b.content_hash());

        // (b) different elements produce different hashes
        let t1c = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(99)]);
        assert_ne!(t1.content_hash(), t1c.content_hash());

        // (c) Tensor hash differs from List hash with identical elements (tag [14] vs [7])
        let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_ne!(t1.content_hash(), list.content_hash());

        // (d) nested rank-2 tensor hash is deterministic
        let t2 = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        let t2b = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(t2.content_hash(), t2b.content_hash());
    }

    #[test]
    fn value_tensor_ord() {
        // (a) Tensor type_tag (13) > Lambda type_tag (12) — cross-type ordering
        // We can't easily construct a Lambda here, but we can compare with Field (tag 11)
        // and verify Tensor sorts after Lambda by inspecting the Ord contract.
        // Instead, use List (tag=7) as a reference: Tensor (13) > List (7).
        let tensor = Value::Tensor(vec![Value::Int(1)]);
        let list = Value::List(vec![Value::Int(99)]);
        assert!(
            tensor > list,
            "Tensor (tag 13) should order after List (tag 7)"
        );

        // (b) within-type lexicographic comparison of elements
        let ta = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        let tb = Value::Tensor(vec![Value::Int(1), Value::Int(3)]);
        assert!(ta < tb);

        // (c) shorter tensor < longer tensor with same prefix elements
        let short = Value::Tensor(vec![Value::Int(1)]);
        let long = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        assert!(short < long);
    }

    // ── Value::Complex Display tests (step-3) ─────────────────────────────────

    #[test]
    fn value_complex_display_positive_imaginary() {
        let v = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3+4i");
    }

    #[test]
    fn value_complex_display_negative_imaginary() {
        let v = Value::Complex {
            re: 3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3-4i");
    }

    #[test]
    fn value_complex_display_fractional() {
        let v = Value::Complex {
            re: 3.5,
            im: 4.2,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3.5+4.2i");
    }

    #[test]
    fn value_complex_display_dimensioned() {
        let v = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(format!("{}", v), "(3+4i) m");
    }

    #[test]
    fn value_complex_display_zero_imaginary() {
        let v = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3+0i");
    }

    #[test]
    fn value_complex_display_negative_real() {
        let v = Value::Complex {
            re: -3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "-3-4i");
    }

    // ── Value::Complex PartialEq tests (step-4) ───────────────────────────────

    #[test]
    fn value_complex_eq_same() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn value_complex_neq_different_re() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 5.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neq_different_im() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 5.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neq_different_dimension() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neg_zero_distinguished() {
        // -0.0 vs 0.0 distinguished via to_bits()
        let pos = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let neg_re = Value::Complex {
            re: -0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let neg_im = Value::Complex {
            re: 0.0,
            im: -0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(pos, neg_re);
        assert_ne!(pos, neg_im);
    }

    #[test]
    fn value_complex_bit_identity_nan_and_neg_zero_consistent() {
        // Verifies the two-sided contract: a == b IFF a.cmp(&b) == Ordering::Equal,
        // for the Complex variant's bit-identity edge cases.

        // --- NaN in `re` ---
        let nan_re_a = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let nan_re_b = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        // PartialEq uses to_bits(): identical NaN bit patterns → equal.
        assert_ord_consistent(&nan_re_a, &nan_re_b, true);

        // --- NaN in `im` ---
        let nan_im_a = Value::Complex {
            re: 0.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let nan_im_b = Value::Complex {
            re: 0.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ord_consistent(&nan_im_a, &nan_im_b, true);

        // --- neg-zero in `re`: Ord consistency (PartialEq already covered by value_complex_neg_zero_distinguished) ---
        let pos_re = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let neg_re = Value::Complex {
            re: -0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        // IEEE 754 totalOrder: -0.0 < +0.0, so pass neg_re as the smaller value.
        assert_ord_consistent(&neg_re, &pos_re, false);

        // --- neg-zero in `im` ---
        let pos_im = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let neg_im = Value::Complex {
            re: 0.0,
            im: -0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        // IEEE 754 totalOrder: -0.0 < +0.0, so pass neg_im as the smaller value.
        // Note: the ordering direction assertion (neg_im < pos_im) was previously missing here.
        assert_ord_consistent(&neg_im, &pos_im, false);

        // --- both-component: NaN in `re`, neg-zero in `im` ---
        // When re components are identical NaN bits (Equal via total_cmp), the Ord
        // comparison chains to im, where -0.0 < +0.0 (lexicographic fallthrough).
        let nan_re_neg_im = Value::Complex {
            re: f64::NAN,
            im: -0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let nan_re_pos_im = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        // Lexicographic fallthrough: -0.0 in im sorts before +0.0 (re compares Equal via NaN total_cmp).
        assert_ord_consistent(&nan_re_neg_im, &nan_re_pos_im, false);
    }

    #[test]
    fn value_complex_neq_real() {
        let c = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(c, Value::Real(3.0));
    }

    #[test]
    fn value_complex_neq_scalar() {
        let c = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let s = Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(c, s);
    }

    // ── Value::Complex Ord tests (step-5) ─────────────────────────────────────

    /// Construct a dimensionless `Value::Complex` for use in Ord tests.
    fn complex_with(re: f64, im: f64) -> Value {
        Value::Complex {
            re,
            im,
            dimension: DimensionVector::DIMENSIONLESS,
        }
    }

    #[test]
    fn value_complex_sorts_after_tensor() {
        // Complex type_tag=14 > Tensor type_tag=13
        let complex = complex_with(0.0, 0.0);
        let tensor = Value::Tensor(vec![Value::Int(99)]);
        assert!(
            complex > tensor,
            "Complex (tag 14) should order after Tensor (tag 13)"
        );
    }

    #[test]
    fn value_complex_sorts_before_undef() {
        // Undef tag=0, Complex tag=14 — Complex > Undef
        // (lower tag sorts first, so Undef=0 < Complex=14)
        // But also test vs something with tag > 14 doesn't exist yet,
        // so just verify cross-type ordering is consistent
        let complex = complex_with(0.0, 0.0);
        let undef = Value::Undef;
        assert!(
            complex > undef,
            "Complex (tag 14) should order after Undef (tag 0)"
        );
    }

    #[test]
    fn value_complex_ord_dimension_first() {
        // Same re/im, different dimension — dimension compared first
        // LENGTH > DIMENSIONLESS in DimensionVector ordering
        let a = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        // They should not be equal; whichever dimension ordering, they differ
        assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_complex_ord_re_second() {
        // Same dimension, different re — re bits compared second
        let a = complex_with(1.0, 0.0);
        let b = complex_with(2.0, 0.0);
        assert!(a < b);
    }

    #[test]
    fn value_complex_ord_im_third() {
        // Same dimension+re, different im — im bits compared third
        let a = complex_with(1.0, 1.0);
        let b = complex_with(1.0, 2.0);
        assert!(a < b);
    }

    #[test]
    fn value_complex_partial_ord_consistent() {
        let a = complex_with(1.0, 2.0);
        let b = complex_with(1.0, 3.0);
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));
        assert_eq!(b.partial_cmp(&a), Some(std::cmp::Ordering::Greater));
    }

    #[test]
    fn value_ord_complex_negative_re() {
        // Negative re components must order correctly.
        let a = complex_with(-1.0, 0.0);
        let b = complex_with(-0.5, 0.0);
        assert!(a < b);
    }

    #[test]
    fn value_ord_complex_negative_im() {
        // Negative im components must order correctly (re is tied).
        let a = complex_with(1.0, -1.0);
        let b = complex_with(1.0, -0.5);
        assert!(a < b);
    }

    // ── Value::Complex content_hash tests (step-6) ────────────────────────────

    #[test]
    fn value_complex_hash_determinism() {
        let a = complex_with(3.0, 4.0);
        let b = complex_with(3.0, 4.0);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_nan_re_canonicalized() {
        let a = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_hash_eq_implies_same_hash() {
        // Equal values produce equal hashes
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(a, b);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_different_re_different_hash() {
        let a = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 2.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_different_dimension_different_hash() {
        let a = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_hash_differs_from_scalar() {
        // Complex tag=15 vs Scalar tag=4 — hashes must differ even with same numeric value
        let c = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let s = Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(c.content_hash(), s.content_hash());
    }

    // ── Value::Complex dimension() test (step-7) ──────────────────────────────

    #[test]
    fn value_complex_dimension_returns_stored() {
        let v = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(v.dimension(), DimensionVector::LENGTH);
    }

    #[test]
    fn value_complex_dimensionless_returns_dimensionless() {
        let v = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(v.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::Orientation tests (step-3) ────────────────────────────────────

    #[test]
    fn value_orientation_construction() {
        let o = orient(1.0, 0.0, 0.0, 0.0);
        // Should not be undef
        assert!(!o.is_undef());
    }

    #[test]
    fn value_orientation_eq_same() {
        let a = orient(1.0, 0.0, 0.0, 0.0);
        let b = orient(1.0, 0.0, 0.0, 0.0);
        assert_eq!(a, b);
    }

    #[test]
    fn value_orientation_eq_different() {
        let a = orient(1.0, 0.0, 0.0, 0.0);
        let b = orient(0.0, 1.0, 0.0, 0.0);
        assert_ne!(a, b);
    }

    #[test]
    fn value_orientation_eq_nan_bitwise() {
        // NaN == NaN via to_bits (bitwise equality)
        let a = orient(f64::NAN, 0.0, 0.0, 0.0);
        let b = orient(f64::NAN, 0.0, 0.0, 0.0);
        assert_eq!(a, b);
    }

    #[test]
    fn value_orientation_eq_neg_zero() {
        // -0.0 != 0.0 via to_bits
        let a = orient(-0.0, 0.0, 0.0, 0.0);
        let b = orient(0.0, 0.0, 0.0, 0.0);
        assert_ne!(a, b);
    }

    #[test]
    fn value_orientation_bit_identity_nan_and_neg_zero_consistent() {
        // Verifies the two-sided contract: a == b IFF a.cmp(&b) == Ordering::Equal,
        // for the Orientation variant's bit-identity edge cases.

        // --- NaN in `w` ---
        let nan_w_a = orient(f64::NAN, 0.0, 0.0, 0.0);
        let nan_w_b = orient(f64::NAN, 0.0, 0.0, 0.0);
        // PartialEq uses to_bits(): identical NaN bit patterns → equal.
        assert_ord_consistent(&nan_w_a, &nan_w_b, true);

        // --- neg-zero in `w`: Ord consistency (PartialEq covered by value_orientation_eq_neg_zero) ---
        let pos_w = orient(0.0, 0.0, 0.0, 0.0);
        let neg_w = orient(-0.0, 0.0, 0.0, 0.0);
        // IEEE 754 totalOrder: -0.0 < +0.0, so pass neg_w as the smaller value.
        assert_ord_consistent(&neg_w, &pos_w, false);

        // --- Spot-check NaN in a non-w component (`z`) to exercise all component call sites ---
        let nan_z_a = orient(0.0, 0.0, 0.0, f64::NAN);
        let nan_z_b = orient(0.0, 0.0, 0.0, f64::NAN);
        assert_ord_consistent(&nan_z_a, &nan_z_b, true);

        // --- neg-zero in `z`: lexicographic fallthrough through w → x → y → z ---
        // w, x, y are all 0.0 (Equal), so comparison chains to z (-0.0 vs +0.0).
        let pos_z = orient(0.0, 0.0, 0.0, 0.0);
        let neg_z = orient(0.0, 0.0, 0.0, -0.0);
        // IEEE 754 totalOrder: -0.0 < +0.0, so pass neg_z as the smaller value.
        assert_ord_consistent(&neg_z, &pos_z, false);
    }

    #[test]
    fn value_orientation_ord_cross_type() {
        // Orientation should sort after Complex (tag 14), so Orientation tag = 15
        let complex = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(complex < orient(1.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn value_orientation_ord_within_type() {
        // Lexicographic on w, x, y, z via total_cmp() — component priority: w→x→y→z
        assert!(orient(0.0, 0.0, 0.0, 0.0) < orient(1.0, 0.0, 0.0, 0.0));

        // Same w, different x
        assert!(orient(1.0, 0.0, 0.0, 0.0) < orient(1.0, 1.0, 0.0, 0.0));
    }

    #[test]
    fn value_orientation_ord_equal_w_different_x() {
        // Equal w, different x with non-zero y — catches field-order swap regressions.
        // Correct Ord (w→x→y→z): w=0.5,x=1.0,y=0.5 > w=0.5,x=0.5,y=1.0 because x=1.0 > x=0.5 when w is tied.
        // A wrong impl comparing y before x would say the opposite (y=0.5 < y=1.0).
        let higher_x = orient(0.5, 1.0, 0.5, 0.0);
        let lower_x = orient(0.5, 0.5, 1.0, 0.0);
        assert!(higher_x > lower_x);
    }

    #[test]
    fn value_orientation_ord_equal_wx_different_y() {
        // Equal w and x, different y with non-zero z — catches y/z field-order swap regressions.
        // Correct Ord (w→x→y→z): w=0.5,x=0.5,y=1.0,z=0.5 > w=0.5,x=0.5,y=0.5,z=1.0
        // because y=1.0 > y=0.5 when w and x are tied.
        // A wrong impl comparing z before y would say the opposite (z=0.5 < z=1.0).
        let higher_y = orient(0.5, 0.5, 1.0, 0.5);
        let lower_y = orient(0.5, 0.5, 0.5, 1.0);
        assert!(higher_y > lower_y);
    }

    #[test]
    fn value_orientation_ord_equal_wxy_different_z() {
        // Equal w, x, and y, different z — catches implementations that drop the z comparison.
        // Correct Ord (w→x→y→z): w=0.5,x=0.5,y=0.5,z=1.0 > w=0.5,x=0.5,y=0.5,z=0.5
        // because z=1.0 > z=0.5 when w, x, and y are all tied.
        // A wrong impl that drops z comparison entirely would say greater_z == lesser_z, not greater_z > lesser_z.
        let greater_z = orient(0.5, 0.5, 0.5, 1.0);
        let lesser_z = orient(0.5, 0.5, 0.5, 0.5);
        assert!(greater_z > lesser_z);
    }

    #[test]
    fn value_orientation_ord_w_dominates_xyz() {
        // Pins w's precedence over x, y, AND z in the Orientation Ord chain.
        // Lower-priority components carry contradictory extreme totalOrder values:
        // NaN (totalOrder-largest) on the "smaller" side and NEG_INFINITY
        // (totalOrder-smallest) on the "larger" side. If any of x/y/z were
        // compared before w (e.g. a bogus `x.total_cmp().then(w)...` chain),
        // the NaN vs NEG_INFINITY signal would flip the ordering and this test
        // would fail. Under the correct w-first chain, w=1.0 < w=2.0 determines
        // the order regardless of the contradicting lower-priority values.
        let smaller = orient(1.0, f64::NAN, f64::NAN, f64::NAN);
        let larger = orient(2.0, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        assert_ord_consistent(&smaller, &larger, false);
    }

    #[test]
    fn value_orientation_ord_x_dominates_yz() {
        // With w held equal, pins x's precedence over y AND z in the Orientation
        // Ord chain. Lower-priority components (y, z) carry contradicting extreme
        // totalOrder values: NaN on the "smaller" side, NEG_INFINITY on the
        // "larger" side. If y or z were compared before x after w ties (e.g. a
        // bogus `w.then(y).then(x)...` chain), the NaN vs NEG_INFINITY signal
        // would flip the ordering and this test would fail. Under the correct
        // w → x chain, w ties at 0.0 and x=1.0 < x=2.0 determines the order.
        let smaller = orient(0.0, 1.0, f64::NAN, f64::NAN);
        let larger = orient(0.0, 2.0, f64::NEG_INFINITY, f64::NEG_INFINITY);
        assert_ord_consistent(&smaller, &larger, false);
    }

    #[test]
    fn value_orientation_ord_y_dominates_z() {
        // With w and x held equal, pins y's precedence over z in the Orientation
        // Ord chain. The lower-priority component (z) carries a contradicting
        // extreme totalOrder value: NaN on the "smaller" side, NEG_INFINITY on the
        // "larger" side. If z were compared before y after w/x tie (e.g. a bogus
        // `w.then(x).then(z).then(y)` chain), the NaN vs NEG_INFINITY signal
        // would flip the ordering and this test would fail. Under the correct
        // w → x → y chain, w and x tie at 0.0 and y=1.0 < y=2.0 determines the order.
        let smaller = orient(0.0, 0.0, 1.0, f64::NAN);
        let larger = orient(0.0, 0.0, 2.0, f64::NEG_INFINITY);
        assert_ord_consistent(&smaller, &larger, false);
    }

    #[test]
    fn value_ord_orientation_negative_components() {
        // Negative component values must order correctly via total_cmp().

        // Negative w: −1.0 < −0.5
        assert!(orient(-1.0, 0.0, 0.0, 0.0) < orient(-0.5, 0.0, 0.0, 0.0));

        // Negative x tiebreaker (w tied at 0.5): −1.0 < −0.5
        assert!(orient(0.5, -1.0, 0.0, 0.0) < orient(0.5, -0.5, 0.0, 0.0));

        // Negative y tiebreaker (w/x tied): −1.0 < −0.5
        assert!(orient(0.5, 0.0, -1.0, 0.0) < orient(0.5, 0.0, -0.5, 0.0));

        // Cross-sign in z (w/x/y all tied at 0.5/0.0/0.0): −0.5 < +0.5
        assert!(orient(0.5, 0.0, 0.0, -0.5) < orient(0.5, 0.0, 0.0, 0.5));
    }

    #[test]
    fn value_orientation_display() {
        let o = orient(1.0, 0.0, 0.0, 0.0);
        assert_eq!(format!("{}", o), "[1, 0, 0, 0]q");
    }

    #[test]
    fn value_orientation_display_fractional() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let o = orient(s, 0.0, 0.0, s);
        let display = format!("{}", o);
        assert!(display.starts_with('['));
        assert!(display.ends_with("]q"));
    }

    #[test]
    fn value_orientation_content_hash_deterministic() {
        let a = orient(1.0, 0.0, 0.0, 0.0);
        let b = orient(1.0, 0.0, 0.0, 0.0);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_nan_canonical() {
        let a = orient(f64::NAN, 0.0, 0.0, 0.0);
        let b = orient(f64::NAN, 0.0, 0.0, 0.0);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_distinct_from_complex() {
        // Tag 16 for Orientation vs tag 15 for Complex
        let o = orient(0.0, 0.0, 0.0, 0.0);
        let c = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(o.content_hash(), c.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_neg_zero() {
        let a = orient(-0.0, 0.0, 0.0, 0.0);
        let b = orient(0.0, 0.0, 0.0, 0.0);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_as_f64_none() {
        let o = orient(1.0, 0.0, 0.0, 0.0);
        assert_eq!(o.as_f64(), None);
    }

    #[test]
    fn value_orientation_dimension_dimensionless() {
        let o = orient(1.0, 0.0, 0.0, 0.0);
        assert_eq!(o.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Range Display tests (step-9) ─────────────────────────────────────────

    #[test]
    fn value_range_display_closed_inclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert_eq!(format!("{}", r), "[0..10]");
    }

    #[test]
    fn value_range_display_open_exclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, false);
        assert_eq!(format!("{}", r), "(0..10)");
    }

    #[test]
    fn value_range_display_half_open_lower_inclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(format!("{}", r), "[0..10)");
    }

    #[test]
    fn value_range_display_half_open_upper_inclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_eq!(format!("{}", r), "(0..10]");
    }

    #[test]
    fn value_range_display_unbounded_lower() {
        let r = make_range(None, Some(Value::Int(10)), false, true);
        assert_eq!(format!("{}", r), "(-inf..10]");
    }

    #[test]
    fn value_range_display_unbounded_upper() {
        let r = make_range(Some(Value::Int(0)), None, true, false);
        assert_eq!(format!("{}", r), "[0..inf)");
    }

    #[test]
    fn value_range_display_fully_unbounded() {
        let r = make_range(None, None, false, false);
        assert_eq!(format!("{}", r), "(-inf..inf)");
    }

    #[test]
    fn value_range_display_real_bounds() {
        let r = make_range(Some(Value::Real(1.5)), Some(Value::Real(3.5)), true, false);
        assert_eq!(format!("{}", r), "[1.5..3.5)");
    }

    // ── Range content_hash tests (step-7) ───────────────────────────────────

    #[test]
    fn value_range_content_hash_deterministic() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_content_hash_different_bounds_differ() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(1)), Some(Value::Int(10)), true, false);
        assert_ne!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_content_hash_none_vs_some_differ() {
        let r_none = make_range(None, Some(Value::Int(10)), false, true);
        let r_some = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_ne!(r_none.content_hash(), r_some.content_hash());
    }

    #[test]
    fn value_range_content_hash_inclusivity_differs() {
        let r_open = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, false);
        let r_half = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r_closed = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert_ne!(r_open.content_hash(), r_half.content_hash());
        assert_ne!(r_half.content_hash(), r_closed.content_hash());
        assert_ne!(r_open.content_hash(), r_closed.content_hash());
    }

    #[test]
    fn value_range_content_hash_no_collision_with_orientation() {
        // Range tag=17 should not collide with Orientation tag=16
        let range = make_range(None, None, false, false);
        let orient_v = orient(1.0, 0.0, 0.0, 0.0);
        assert_ne!(range.content_hash(), orient_v.content_hash());
    }

    #[test]
    fn value_range_content_hash_both_none_deterministic() {
        let r1 = make_range(None, None, false, false);
        let r2 = make_range(None, None, false, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    // ── Range Ord tests (step-5) ─────────────────────────────────────────────

    #[test]
    fn value_range_ord_cross_type_after_orientation() {
        // Range has type_tag=16, Orientation=15 → Range > Orientation
        let range = make_range(None, None, false, false);
        let orient_v = orient(1.0, 0.0, 0.0, 0.0);
        assert!(range > orient_v);
        assert!(orient_v < range);
    }

    #[test]
    fn value_range_ord_cross_type_before_undef() {
        // Range has type_tag=16, Undef=0 → Range > Undef
        let range = make_range(None, None, false, false);
        assert!(range > Value::Undef);
    }

    #[test]
    fn value_range_ord_within_type_lower_inclusive_first() {
        // lower_inclusive=false < lower_inclusive=true (false=0 < true=1)
        let r_open = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        let r_closed = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert!(r_open < r_closed);
    }

    #[test]
    fn value_range_ord_within_type_lower_bound_none_before_some() {
        // None lower < Some lower (Option ordering: None < Some)
        let r_unbounded = make_range(None, Some(Value::Int(10)), false, true);
        let r_bounded = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert!(r_unbounded < r_bounded);
    }

    #[test]
    fn value_range_ord_within_type_upper_inclusive_after_lower() {
        // When lower_inclusive and lower are equal, compare upper_inclusive
        let r_open_upper = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r_closed_upper = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert!(r_open_upper < r_closed_upper);
    }

    #[test]
    fn value_range_ord_within_type_upper_bound_none_before_some() {
        // None upper < Some upper
        let r_unbounded = make_range(Some(Value::Int(0)), None, true, false);
        let r_bounded = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert!(r_unbounded < r_bounded);
    }

    #[test]
    fn value_range_ord_equal_ranges() {
        use std::cmp::Ordering;
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1.cmp(&r2), Ordering::Equal);
    }

    #[test]
    fn value_ord_range_negative_bounds() {
        // Range delegates bound ordering to Value::cmp, which uses total_cmp() for
        // Real bounds. Negative-bound ranges must therefore sort correctly.

        // Real lower=-5.0, upper=5.0 (half-open [−5, 5))
        let r_neg_real = make_range(Some(Value::Real(-5.0)), Some(Value::Real(5.0)), true, false);
        // Real lower=0.0, upper=10.0 (half-open [0, 10))
        let r_pos_real = make_range(Some(Value::Real(0.0)), Some(Value::Real(10.0)), true, false);
        // [-5, 5) < [0, 10) because lower bounds: −5.0 < 0.0
        assert!(r_neg_real < r_pos_real);
        // Antisymmetry
        assert_eq!(
            r_neg_real.cmp(&r_pos_real),
            r_pos_real.cmp(&r_neg_real).reverse()
        );

        // Int lower=-10, upper=-1 (closed [−10, −1])
        let r_neg_int = make_range(Some(Value::Int(-10)), Some(Value::Int(-1)), true, true);
        // Int lower=-5, upper=-1 (closed [−5, −1])
        let r_less_neg_int = make_range(Some(Value::Int(-5)), Some(Value::Int(-1)), true, true);
        // [−10, −1] < [−5, −1] because lower bounds: −10 < −5
        assert!(r_neg_int < r_less_neg_int);
        assert_eq!(
            r_neg_int.cmp(&r_less_neg_int),
            r_less_neg_int.cmp(&r_neg_int).reverse()
        );
    }

    // ── Range PartialEq tests (step-3) ───────────────────────────────────────

    fn make_range(
        lower: Option<Value>,
        upper: Option<Value>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> Value {
        Value::range(lower, upper, lower_inclusive, upper_inclusive)
    }

    /// Construct a `Value::Orientation` from four f64 components.
    /// Placed near `make_range()` following the project convention of defining
    /// test helpers close to the tests that use them.
    fn orient(w: f64, x: f64, y: f64, z: f64) -> Value {
        Value::Orientation { w, x, y, z }
    }

    // ── quaternion_is_finite predicate tests ────────────────────────────────

    #[test]
    fn quaternion_is_finite_all_finite_returns_true() {
        assert!(
            quaternion_is_finite(1.0, 0.0, 0.0, 0.0),
            "all-finite quaternion should be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_nan_in_w_returns_false() {
        assert!(
            !quaternion_is_finite(f64::NAN, 0.0, 0.0, 0.0),
            "NaN in w should not be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_nan_in_x_returns_false() {
        assert!(
            !quaternion_is_finite(1.0, f64::NAN, 0.0, 0.0),
            "NaN in x should not be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_nan_in_y_returns_false() {
        assert!(
            !quaternion_is_finite(1.0, 0.0, f64::NAN, 0.0),
            "NaN in y should not be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_nan_in_z_returns_false() {
        assert!(
            !quaternion_is_finite(1.0, 0.0, 0.0, f64::NAN),
            "NaN in z should not be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_pos_inf_returns_false() {
        assert!(
            !quaternion_is_finite(f64::INFINITY, 0.0, 0.0, 0.0),
            "+Inf in w should not be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_neg_inf_returns_false() {
        assert!(
            !quaternion_is_finite(0.0, f64::NEG_INFINITY, 0.0, 0.0),
            "-Inf in x should not be finite"
        );
    }

    #[test]
    fn quaternion_is_finite_all_non_finite_returns_false() {
        assert!(
            !quaternion_is_finite(f64::NAN, f64::NAN, f64::INFINITY, f64::NEG_INFINITY),
            "all-non-finite quaternion should not be finite"
        );
    }

    #[test]
    fn value_range_equal_ranges_are_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_different_lower_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_different_upper_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(20)), true, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_different_lower_inclusive_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_different_upper_inclusive_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_none_vs_some_lower_not_equal() {
        let r1 = make_range(None, Some(Value::Int(10)), false, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_none_vs_some_upper_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), None, true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_both_none_equal() {
        let r1 = make_range(None, None, false, false);
        let r2 = make_range(None, None, false, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_not_equal_to_other_variants() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_ne!(r, Value::Int(0));
        assert_ne!(r, Value::Undef);
        assert_ne!(r, Value::Bool(true));
    }

    // ── Range inclusivity normalization tests (task-364) ─────────────────────

    #[test]
    fn value_range_normalize_lower_inclusive_when_none() {
        let r = Value::range(None, Some(Value::Int(10)), true, true);
        match r {
            Value::Range {
                lower_inclusive, ..
            } => assert!(!lower_inclusive),
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_normalize_upper_inclusive_when_none() {
        let r = Value::range(Some(Value::Int(0)), None, true, true);
        match r {
            Value::Range {
                upper_inclusive, ..
            } => assert!(!upper_inclusive),
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_normalize_both_when_none() {
        let r = Value::range(None, None, true, true);
        match r {
            Value::Range {
                lower_inclusive,
                upper_inclusive,
                ..
            } => {
                assert!(!lower_inclusive);
                assert!(!upper_inclusive);
            }
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_no_normalize_when_some() {
        let r = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        match r {
            Value::Range {
                lower_inclusive,
                upper_inclusive,
                ..
            } => {
                assert!(lower_inclusive);
                assert!(upper_inclusive);
            }
            _ => panic!("expected Range"),
        }
    }

    // ── Range equality/hash equivalence with differing flags (task-364 step-3) ─

    #[test]
    fn value_range_eq_none_lower_ignores_inclusive() {
        let r1 = Value::range(None, Some(Value::Int(10)), true, true);
        let r2 = Value::range(None, Some(Value::Int(10)), false, true);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_eq_none_upper_ignores_inclusive() {
        let r1 = Value::range(Some(Value::Int(0)), None, true, true);
        let r2 = Value::range(Some(Value::Int(0)), None, true, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_eq_both_none_ignores_inclusive() {
        let r1 = Value::range(None, None, true, true);
        let r2 = Value::range(None, None, false, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_hash_none_lower_ignores_inclusive() {
        let r1 = Value::range(None, Some(Value::Int(10)), true, true);
        let r2 = Value::range(None, Some(Value::Int(10)), false, true);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_hash_none_upper_ignores_inclusive() {
        let r1 = Value::range(Some(Value::Int(0)), None, true, true);
        let r2 = Value::range(Some(Value::Int(0)), None, true, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    // ── Range gap tests: both-None hash, both-bounds-present eq/hash (task-364 pre) ─

    #[test]
    fn value_range_hash_both_none_ignores_inclusive() {
        let r1 = Value::range(None, None, true, true);
        let r2 = Value::range(None, None, false, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_eq_both_bounds_present() {
        let r1 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1, r2);
        // Different upper bound → not equal
        let r3 = Value::range(Some(Value::Int(0)), Some(Value::Int(20)), true, false);
        assert_ne!(r1, r3);
    }

    #[test]
    fn value_range_hash_both_bounds_present() {
        let r1 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
        // Different bounds → different hash
        let r3 = Value::range(Some(Value::Int(0)), Some(Value::Int(20)), true, false);
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    // ── Range Display with inclusive+None edge cases (task-364 step-4) ─────────

    #[test]
    fn value_range_display_none_lower_with_inclusive_true() {
        let r = Value::range(None, Some(Value::Int(10)), true, true);
        assert_eq!(format!("{}", r), "(-inf..10]");
    }

    #[test]
    fn value_range_display_none_upper_with_inclusive_true() {
        let r = Value::range(Some(Value::Int(0)), None, true, true);
        assert_eq!(format!("{}", r), "[0..inf)");
    }

    #[test]
    fn value_range_display_both_none_with_inclusive_true() {
        let r = Value::range(None, None, true, true);
        assert_eq!(format!("{}", r), "(-inf..inf)");
    }

    // ── Range invariant re-normalization tests (step-9) ───────────────────────
    // These tests bypass Value::range() factory and directly construct Value::Range
    // with an invariant violation (lower/upper_inclusive=true when bound is None).
    // Each impl (content_hash, PartialEq, Ord, Display) silently re-normalizes
    // via normalize_range_flags.

    #[test]
    fn value_range_bypass_hash_renormalizes() {
        // Bypassed Range with lower=None+lower_inclusive=true should hash
        // identically to the correctly-constructed version.
        let bypassed = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let correct = Value::range(None, Some(Value::Int(10)), false, false);
        assert_eq!(bypassed.content_hash(), correct.content_hash());
    }

    #[test]
    fn value_range_bypass_eq_renormalizes() {
        // Two Range values: one with lower=None+lower_inclusive=true (bypassed),
        // one with lower=None+lower_inclusive=false. They are logically identical.
        let bypassed = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let correct = Value::range(None, Some(Value::Int(10)), false, false);
        assert_eq!(bypassed, correct);
    }

    #[test]
    fn value_range_bypass_cmp_renormalizes() {
        // Two Range values with lower=None and different lower_inclusive flags:
        // after normalization both should have lower_inclusive=false → Equal.
        let bypassed = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let correct = Value::range(None, Some(Value::Int(10)), false, false);
        assert_eq!(bypassed.cmp(&correct), std::cmp::Ordering::Equal);
    }

    // ── Bypass normalization-verifying tests (task-364) ─────────────────────
    // These construct Value::Range directly (bypassing Value::range()), setting
    // invariant-violating flags. Each impl must silently re-normalize so the
    // output is correct.

    #[test]
    fn value_range_bypass_display_renormalizes_lower() {
        // lower=None + lower_inclusive=true → Display must output '(' not '['
        let r = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let s = format!("{}", r);
        assert!(s.starts_with('('), "expected '(' but got: {}", s);
    }

    #[test]
    fn value_range_bypass_display_renormalizes_upper() {
        // upper=None + upper_inclusive=true → Display must output ')' not ']'
        let r = Value::Range {
            lower: Some(Box::new(Value::Int(0))),
            lower_inclusive: true,
            upper: None,
            upper_inclusive: true,
        };
        let s = format!("{}", r);
        assert!(s.ends_with(')'), "expected ')' but got: {}", s);
    }

    // ── Value::Matrix Ord tests (step-7) ─────────────────────────────────────

    #[test]
    fn value_matrix_ord_cross_type_after_range() {
        // (a) Matrix (tag 17) > Range (tag 16)
        let matrix = Value::Matrix(vec![vec![Value::Int(1)]]);
        let range = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert!(matrix > range);
    }

    #[test]
    fn value_matrix_ord_within_type_lexicographic() {
        // (b) lexicographic ordering on rows: [[1,2],[3,4]] < [[1,2],[3,5]]
        let m1 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let m2 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(5)],
        ]);
        assert!(m1 < m2);
        assert!(m2 > m1);
        // Equal matrices compare equal
        let m3 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        assert_eq!(m1.cmp(&m3), std::cmp::Ordering::Equal);
    }

    // ── Value::Matrix content_hash tests (step-5) ────────────────────────────

    #[test]
    fn value_matrix_content_hash_determinism() {
        // (a) same matrix produces same hash
        let m1 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let m2 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        assert_eq!(m1.content_hash(), m2.content_hash());
    }

    #[test]
    fn value_matrix_content_hash_transposed_differs() {
        // (b) transposed matrix has different hash
        let m_normal = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let m_transposed = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(3)],
            vec![Value::Int(2), Value::Int(4)],
        ]);
        assert_ne!(m_normal.content_hash(), m_transposed.content_hash());
    }

    #[test]
    fn value_matrix_content_hash_distinct_from_tensor() {
        // (c) same elements as Tensor produce different hash (different tag)
        let matrix = Value::Matrix(vec![vec![Value::Int(1), Value::Int(2)]]);
        let tensor = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(matrix.content_hash(), tensor.content_hash());
    }

    // ── Value::Matrix tests (step-3) ─────────────────────────────────────────

    #[test]
    fn value_matrix_construction_and_partial_eq() {
        // (a) same rows equal
        let m1 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(6)],
        ]);
        let m2 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(6)],
        ]);
        assert_eq!(m1, m2);

        // different element — not equal
        let m3 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(7)],
        ]);
        assert_ne!(m1, m3);

        // different shape — not equal
        let m4 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        assert_ne!(m1, m4);

        // cross-variant: Matrix != List
        let list = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(
            Value::Matrix(vec![vec![Value::Int(1), Value::Int(2)]]),
            list
        );

        // cross-variant: Matrix != Tensor
        let tensor = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(
            Value::Matrix(vec![vec![Value::Int(1), Value::Int(2)]]),
            tensor
        );
    }

    #[test]
    fn value_matrix_display_2x3() {
        // (b) 2x3 matrix: [[1, 2, 3], [4, 5, 6]]
        let m = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(6)],
        ]);
        assert_eq!(format!("{}", m), "[[1, 2, 3], [4, 5, 6]]");
    }

    #[test]
    fn value_matrix_display_1x1() {
        // (b) 1x1 matrix: [[1]]
        let m = Value::Matrix(vec![vec![Value::Int(1)]]);
        assert_eq!(format!("{}", m), "[[1]]");
    }

    // ── Value::Matrix canonicalize_matrix / try_into_matrix tests (step-11) ─

    #[test]
    fn canonicalize_matrix_converts_to_nested_tensor() {
        // (a) Matrix([[1,2],[3,4]]) → Tensor([Tensor([1,2]), Tensor([3,4])])
        let matrix = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let expected = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(matrix.canonicalize_matrix(), expected);
    }

    #[test]
    fn canonicalize_matrix_is_identity_for_non_matrix() {
        // (b) non-Matrix values pass through unchanged
        assert_eq!(Value::Int(42).canonicalize_matrix(), Value::Int(42));
        assert_eq!(
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]).canonicalize_matrix(),
            Value::Tensor(vec![Value::Int(1), Value::Int(2)])
        );
        assert_eq!(Value::Undef.canonicalize_matrix(), Value::Undef);
    }

    #[test]
    fn canonicalize_matrix_empty_rows() {
        // (c) Matrix([[],[]])  → Tensor([Tensor([]), Tensor([])])
        let matrix = Value::Matrix(vec![vec![], vec![]]);
        let expected = Value::Tensor(vec![Value::Tensor(vec![]), Value::Tensor(vec![])]);
        assert_eq!(matrix.canonicalize_matrix(), expected);
    }

    #[test]
    fn try_into_matrix_rank2_tensor_converts() {
        // (d) rank-2 Tensor([Tensor([1,2]), Tensor([3,4])]) → Some(Matrix([[1,2],[3,4]]))
        let tensor = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        let expected = Some(Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]));
        assert_eq!(tensor.try_into_matrix(), expected);
    }

    #[test]
    fn try_into_matrix_rank1_tensor_returns_none() {
        // (e) rank-1 Tensor([1,2,3]) → None (not all-Tensor elements)
        let tensor = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(tensor.try_into_matrix(), None);
    }

    #[test]
    fn try_into_matrix_non_tensor_returns_none() {
        // (f) non-Tensor values return None
        assert_eq!(Value::Int(42).try_into_matrix(), None);
        assert_eq!(
            Value::Matrix(vec![vec![Value::Int(1)]]).try_into_matrix(),
            None
        );
    }

    #[test]
    fn canonicalize_matrix_round_trip() {
        // (g) round-trip: matrix.clone().canonicalize_matrix().try_into_matrix() == Some(matrix)
        let matrix = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let round_tripped = matrix.clone().canonicalize_matrix().try_into_matrix();
        assert_eq!(round_tripped, Some(matrix));
    }

    // ── try_into_matrix Point/Vector exclusion tests ─────────────────────────
    // These document that Point and Vector elements inside a Tensor are
    // intentionally NOT treated as matrix rows (see exclusion comment on guard).

    #[test]
    fn try_into_matrix_tensor_of_points_returns_none() {
        // Tensor([Point([1,2,3]), Point([4,5,6])]) → None
        // A Tensor-of-Points is a point collection, not a matrix.
        let tensor = Value::Tensor(vec![
            Value::Point(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
            Value::Point(vec![Value::Int(4), Value::Int(5), Value::Int(6)]),
        ]);
        assert_eq!(tensor.try_into_matrix(), None);
    }

    #[test]
    fn try_into_matrix_tensor_of_vectors_returns_none() {
        // Tensor([Vector([1,2]), Vector([3,4])]) → None
        // A Tensor-of-Vectors is a vector collection, not a matrix.
        let tensor = Value::Tensor(vec![
            Value::Vector(vec![Value::Int(1), Value::Int(2)]),
            Value::Vector(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(tensor.try_into_matrix(), None);
    }

    #[test]
    fn try_into_matrix_mixed_tensor_point_returns_none() {
        // Tensor([Tensor([1,2]), Point([3,4])]) → None
        // The guard requires ALL elements to be Tensor; any non-Tensor element rejects.
        let tensor = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Point(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(tensor.try_into_matrix(), None);
    }

    // ── Value::Frame tests (step-3) ──────────────────────────────────────────

    fn make_point3_length() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_orientation_identity() -> Value {
        orient(1.0, 0.0, 0.0, 0.0)
    }

    fn make_frame(origin: Value, basis: Value) -> Value {
        Value::Frame {
            origin: Box::new(origin),
            basis: Box::new(basis),
        }
    }

    #[test]
    fn value_frame_construction() {
        let origin = make_point3_length();
        let basis = make_orientation_identity();
        let frame = make_frame(origin.clone(), basis.clone());
        match frame {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*b, basis);
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn value_frame_partial_eq_equal() {
        let f1 = make_frame(make_point3_length(), make_orientation_identity());
        let f2 = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(f1, f2);
    }

    #[test]
    fn value_frame_partial_eq_different_origin() {
        let origin_a = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let origin_b = Value::Point(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let basis = make_orientation_identity();
        let f1 = make_frame(origin_a, basis.clone());
        let f2 = make_frame(origin_b, basis);
        assert_ne!(f1, f2);
    }

    #[test]
    fn value_frame_partial_eq_different_basis() {
        let origin = make_point3_length();
        let basis_a = orient(1.0, 0.0, 0.0, 0.0);
        let basis_b = orient(0.0, 1.0, 0.0, 0.0);
        let f1 = make_frame(origin.clone(), basis_a);
        let f2 = make_frame(origin, basis_b);
        assert_ne!(f1, f2);
    }

    #[test]
    fn value_frame_display() {
        let origin = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let basis = orient(1.0, 0.0, 0.0, 0.0);
        let frame = make_frame(origin, basis);
        let s = format!("{}", frame);
        assert_eq!(s, "frame(point(0 m, 0 m, 0 m), [1, 0, 0, 0]q)");
    }

    #[test]
    fn value_frame_dimension_is_dimensionless() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(frame.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn value_frame_content_hash_determinism() {
        let f1 = make_frame(make_point3_length(), make_orientation_identity());
        let f2 = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(f1.content_hash(), f2.content_hash());
    }

    #[test]
    fn value_frame_content_hash_distinct_from_orientation() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let orientation = make_orientation_identity();
        assert_ne!(frame.content_hash(), orientation.content_hash());
    }

    #[test]
    fn value_frame_content_hash_distinct_from_point() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let point = make_point3_length();
        assert_ne!(frame.content_hash(), point.content_hash());
    }

    #[test]
    fn value_frame_ord_type_tag_gt_matrix() {
        // Frame type_tag=20 > Matrix type_tag=19
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let matrix = Value::Matrix(vec![vec![Value::Int(1)]]);
        assert!(frame > matrix);
    }

    #[test]
    fn value_frame_ord_same_type_compare_origin_first() {
        // Two frames with same basis but different origin should order by origin
        let origin_a = Value::Point(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let origin_b = Value::Point(vec![
            Value::length(2.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let basis = make_orientation_identity();
        let f1 = make_frame(origin_a, basis.clone());
        let f2 = make_frame(origin_b, basis);
        assert!(f1 < f2);
    }

    #[test]
    fn value_frame_ord_same_origin_compare_basis() {
        // Same origin, different basis: order by basis quaternion
        let origin = make_point3_length();
        // Valid 180° rotation around X-axis (unit quaternion: |q|=1).
        // w=0.0 < w=1.0 by to_bits ordering, so basis_a < basis_b.
        let basis_a = orient(0.0, 1.0, 0.0, 0.0);
        let basis_b = orient(1.0, 0.0, 0.0, 0.0);
        let f1 = make_frame(origin.clone(), basis_a);
        let f2 = make_frame(origin, basis_b);
        assert!(f1 < f2);
    }

    #[test]
    fn value_frame_content_hash_neg_zero_origin_differs() {
        // neg-zero and pos-zero in origin produce different hashes
        let origin_pos = Value::Point(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let origin_neg = Value::Point(vec![
            Value::Scalar {
                si_value: -0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let basis = make_orientation_identity();
        let f1 = make_frame(origin_pos, basis.clone());
        let f2 = make_frame(origin_neg, basis);
        assert_ne!(f1.content_hash(), f2.content_hash());
    }

    #[test]
    fn value_frame_dimension_explicit_arm() {
        // Ensures dimension() has an explicit Frame arm (not just the wildcard).
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(frame.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    #[should_panic(expected = "infer_type() cannot infer Frame")]
    fn value_frame_infer_type_panics() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let _ = frame.infer_type();
    }

    #[test]
    fn value_frame_ne_orientation() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let orientation = make_orientation_identity();
        assert_ne!(frame, orientation);
    }

    #[test]
    fn value_frame_ne_point() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let point = make_point3_length();
        assert_ne!(frame, point);
    }

    #[test]
    #[should_panic(expected = "infer_type() cannot infer Transform")]
    fn value_transform_infer_type_panics() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let _ = transform.infer_type();
    }

    // ── Value::Transform tests (step-3) ──────────────────────────────────────

    fn make_vector3_length() -> Value {
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_transform(rotation: Value, translation: Value) -> Value {
        Value::Transform {
            rotation: Box::new(rotation),
            translation: Box::new(translation),
        }
    }

    #[test]
    fn value_transform_construction() {
        let rotation = make_orientation_identity();
        let translation = make_vector3_length();
        let transform = make_transform(rotation.clone(), translation.clone());
        match transform {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation);
                assert_eq!(*t, translation);
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn value_transform_partial_eq_equal() {
        let t1 = make_transform(make_orientation_identity(), make_vector3_length());
        let t2 = make_transform(make_orientation_identity(), make_vector3_length());
        assert_eq!(t1, t2);
    }

    #[test]
    fn value_transform_partial_eq_different_rotation() {
        let rot_a = orient(1.0, 0.0, 0.0, 0.0);
        let rot_b = orient(0.0, 1.0, 0.0, 0.0);
        let translation = make_vector3_length();
        let t1 = make_transform(rot_a, translation.clone());
        let t2 = make_transform(rot_b, translation);
        assert_ne!(t1, t2);
    }

    #[test]
    fn value_transform_partial_eq_different_translation() {
        let rotation = make_orientation_identity();
        let trans_a = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let trans_b = Value::Vector(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let t1 = make_transform(rotation.clone(), trans_a);
        let t2 = make_transform(rotation, trans_b);
        assert_ne!(t1, t2);
    }

    #[test]
    fn value_transform_display() {
        let rotation = orient(1.0, 0.0, 0.0, 0.0);
        let translation = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let transform = make_transform(rotation, translation);
        let s = format!("{}", transform);
        // Expected: transform([1, 0, 0, 0]q, vec(0 m, 0 m, 0 m))
        assert!(
            s.starts_with("transform("),
            "display should start with 'transform(', got: {}",
            s
        );
        assert!(
            s.contains("[1, 0, 0, 0]q"),
            "display should contain rotation, got: {}",
            s
        );
    }

    #[test]
    fn value_transform_dimension_is_dimensionless() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert_eq!(transform.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn value_transform_content_hash_determinism() {
        let t1 = make_transform(make_orientation_identity(), make_vector3_length());
        let t2 = make_transform(make_orientation_identity(), make_vector3_length());
        assert_eq!(t1.content_hash(), t2.content_hash());
    }

    #[test]
    fn value_transform_content_hash_distinct_from_frame() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert_ne!(transform.content_hash(), frame.content_hash());
    }

    #[test]
    fn value_transform_content_hash_distinct_from_orientation() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let orientation = make_orientation_identity();
        assert_ne!(transform.content_hash(), orientation.content_hash());
    }

    #[test]
    fn value_transform_content_hash_distinct_from_vector() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let vector = make_vector3_length();
        assert_ne!(transform.content_hash(), vector.content_hash());
    }

    #[test]
    fn value_transform_ord_type_tag_gt_frame() {
        // Transform type_tag=21 > Frame type_tag=20
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert!(transform > frame);
    }

    #[test]
    fn value_transform_ord_same_type_compare_rotation_first() {
        // Two transforms with same translation but different rotation: order by rotation
        let rot_a = orient(0.0, 0.0, 0.0, 0.0);
        let rot_b = orient(1.0, 0.0, 0.0, 0.0);
        let translation = make_vector3_length();
        let t1 = make_transform(rot_a, translation.clone());
        let t2 = make_transform(rot_b, translation);
        assert!(t1 < t2);
    }

    #[test]
    fn value_transform_ord_same_rotation_compare_translation() {
        // Same rotation, different translation: order by translation
        let rotation = make_orientation_identity();
        let trans_a = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let trans_b = Value::Vector(vec![
            Value::length(2.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let t1 = make_transform(rotation.clone(), trans_a);
        let t2 = make_transform(rotation, trans_b);
        assert!(t1 < t2);
    }

    #[test]
    fn value_transform_content_hash_neg_zero_translation_differs() {
        // neg-zero and pos-zero in translation produce different hashes
        let trans_pos = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let trans_neg = Value::Vector(vec![
            Value::Scalar {
                si_value: -0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let rotation = make_orientation_identity();
        let t1 = make_transform(rotation.clone(), trans_pos);
        let t2 = make_transform(rotation, trans_neg);
        assert_ne!(t1.content_hash(), t2.content_hash());
    }

    // ── Value::AffineMap tests (step-3 RED / task 3958 α) ───────────────────

    fn make_affine_identity() -> Value {
        Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        }
    }

    fn make_affine_diag(sx: f64, sy: f64, sz: f64, tx: f64, ty: f64, tz: f64) -> Value {
        Value::AffineMap {
            linear: [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, sz]],
            translation: [tx, ty, tz],
        }
    }

    #[test]
    fn value_affine_map_construction() {
        let v = make_affine_diag(2.0, 3.0, 4.0, 0.1, 0.2, 0.3);
        match v {
            Value::AffineMap { linear, translation } => {
                assert_eq!(linear[0][0], 2.0);
                assert_eq!(linear[1][1], 3.0);
                assert_eq!(linear[2][2], 4.0);
                assert_eq!(linear[0][1], 0.0);
                assert_eq!(translation[0], 0.1);
                assert_eq!(translation[1], 0.2);
                assert_eq!(translation[2], 0.3);
            }
            other => panic!("expected Value::AffineMap, got {:?}", other),
        }
    }

    #[test]
    fn value_affine_map_partial_eq_equal() {
        let a1 = make_affine_identity();
        let a2 = make_affine_identity();
        assert_eq!(a1, a2);
    }

    #[test]
    fn value_affine_map_partial_eq_different_linear() {
        let a1 = make_affine_identity();
        let a2 = Value::AffineMap {
            linear: [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        assert_ne!(a1, a2);
    }

    #[test]
    fn value_affine_map_partial_eq_different_translation() {
        let a1 = make_affine_identity();
        let a2 = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [1.0, 0.0, 0.0],
        };
        assert_ne!(a1, a2);
    }

    #[test]
    fn value_affine_map_partial_eq_neg_zero_vs_pos_zero() {
        // bit-identity convention: +0.0 != -0.0
        let a_pos = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        let a_neg = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [-0.0, 0.0, 0.0],
        };
        assert_ne!(a_pos, a_neg);
    }

    #[test]
    fn value_affine_map_display() {
        let s = format!("{}", make_affine_identity());
        assert!(
            s.starts_with("affine_map("),
            "display should start with 'affine_map(', got: {}",
            s
        );
    }

    #[test]
    fn value_affine_map_dimension_is_dimensionless() {
        assert_eq!(make_affine_identity().dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn value_affine_map_content_hash_determinism() {
        let a1 = make_affine_identity();
        let a2 = make_affine_identity();
        assert_eq!(a1.content_hash(), a2.content_hash());
    }

    #[test]
    fn value_affine_map_content_hash_distinct_from_transform() {
        let affine = make_affine_identity();
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert_ne!(affine.content_hash(), transform.content_hash());
    }

    #[test]
    fn value_affine_map_content_hash_distinct_from_orientation() {
        let affine = make_affine_identity();
        let orientation = make_orientation_identity();
        assert_ne!(affine.content_hash(), orientation.content_hash());
    }

    #[test]
    fn value_affine_map_content_hash_neg_zero_translation_differs() {
        // neg-zero and pos-zero in translation produce different hashes (to_bits preserves sign)
        let a_pos = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        let a_neg = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [-0.0, 0.0, 0.0],
        };
        assert_ne!(a_pos.content_hash(), a_neg.content_hash());
    }

    #[test]
    fn value_affine_map_content_hash_nan_canonicalized() {
        // NaN payload is canonicalized: all NaN bit patterns hash the same
        let canonical_nan = f64::NAN;
        let noncanonical_nan = f64::from_bits(f64::NAN.to_bits() | 1);
        let a1 = Value::AffineMap {
            linear: [[canonical_nan, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        let a2 = Value::AffineMap {
            linear: [[noncanonical_nan, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        assert_eq!(a1.content_hash(), a2.content_hash());
    }

    #[test]
    fn value_affine_map_ord_type_tag_gt_geometry_handle() {
        // AffineMap type_tag=28 > GeometryHandle type_tag=27
        let affine = make_affine_identity();
        let ghandle = Value::GeometryHandle {
            realization_ref: reify_core::identity::RealizationNodeId::new("T", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(crate::geometry::GeometryHandleId(0)),
        };
        assert!(affine > ghandle);
    }

    #[test]
    fn value_affine_map_ord_same_type_compare_linear_first() {
        // Same translation, differing linear[0][0]: lower linear comes first
        let a1 = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        let a2 = Value::AffineMap {
            linear: [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        assert!(a1 < a2);
    }

    #[test]
    fn value_affine_map_ord_same_linear_compare_translation() {
        // Same linear, differing translation[0]: lower translation comes first
        let a1 = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        };
        let a2 = Value::AffineMap {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [1.0, 0.0, 0.0],
        };
        assert!(a1 < a2);
    }

    #[test]
    fn value_affine_map_try_infer_type_returns_affine_map_3() {
        // AffineMap has fixed-size arrays, so dimension is structurally pinned to 3
        assert_eq!(
            make_affine_identity().try_infer_type(),
            Some(reify_core::ty::Type::AffineMap(3))
        );
    }

    #[test]
    fn value_affine_map_infer_type_returns_affine_map_3() {
        // Does NOT panic — unlike Frame/Transform which panic on infer_type()
        assert_eq!(make_affine_identity().infer_type(), reify_core::ty::Type::AffineMap(3));
    }

    // ── Value::Plane tests (pre-2) ────────────────────────────────────────────

    fn make_plane(origin: Value, normal: Value) -> Value {
        Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(normal),
        }
    }

    fn make_point3_origin() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_normal_z() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    #[test]
    fn value_plane_construction() {
        let origin = make_point3_origin();
        let normal = make_normal_z();
        let plane = make_plane(origin.clone(), normal.clone());
        match plane {
            Value::Plane {
                origin: o,
                normal: n,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*n, normal);
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn value_plane_partial_eq_same() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let p2 = make_plane(make_point3_origin(), make_normal_z());
        assert_eq!(p1, p2);
    }

    #[test]
    fn value_plane_partial_eq_different() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let normal_x = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let p2 = make_plane(make_point3_origin(), normal_x);
        assert_ne!(p1, p2);
    }

    #[test]
    fn value_plane_partial_eq_different_origin() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let alt_origin = Value::Point(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let p2 = make_plane(alt_origin, make_normal_z());
        assert_ne!(p1, p2);
    }

    #[test]
    fn value_plane_display() {
        let origin = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let plane = make_plane(origin, normal);
        let s = format!("{}", plane);
        assert!(
            s.starts_with("plane("),
            "display should start with 'plane(', got: {}",
            s
        );
    }

    #[test]
    fn value_plane_content_hash_deterministic() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let p2 = make_plane(make_point3_origin(), make_normal_z());
        assert_eq!(p1.content_hash(), p2.content_hash());
    }

    #[test]
    fn value_plane_content_hash_no_collision_with_transform() {
        let plane = make_plane(make_point3_origin(), make_normal_z());
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert_ne!(plane.content_hash(), transform.content_hash());
    }

    #[test]
    fn value_plane_ord_cross_type() {
        // Plane type_tag=22 > Transform type_tag=21
        let plane = make_plane(make_point3_origin(), make_normal_z());
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert!(plane > transform);
    }

    #[test]
    fn value_plane_dimension_dimensionless() {
        let plane = make_plane(make_point3_origin(), make_normal_z());
        assert_eq!(plane.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::Axis tests (pre-3) ─────────────────────────────────────────────

    fn make_axis(origin: Value, direction: Value) -> Value {
        Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(direction),
        }
    }

    fn make_direction_z() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    #[test]
    fn value_axis_construction() {
        let origin = make_point3_origin();
        let direction = make_direction_z();
        let axis = make_axis(origin.clone(), direction.clone());
        match axis {
            Value::Axis {
                origin: o,
                direction: d,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*d, direction);
            }
            other => panic!("expected Value::Axis, got {:?}", other),
        }
    }

    #[test]
    fn value_axis_partial_eq_same() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let a2 = make_axis(make_point3_origin(), make_direction_z());
        assert_eq!(a1, a2);
    }

    #[test]
    fn value_axis_partial_eq_different() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let dir_x = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let a2 = make_axis(make_point3_origin(), dir_x);
        assert_ne!(a1, a2);
    }

    #[test]
    fn value_axis_partial_eq_different_origin() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let alt_origin = Value::Point(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let a2 = make_axis(alt_origin, make_direction_z());
        assert_ne!(a1, a2);
    }

    #[test]
    fn value_axis_display() {
        let origin = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let direction = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let axis = make_axis(origin, direction);
        let s = format!("{}", axis);
        assert!(
            s.starts_with("axis("),
            "display should start with 'axis(', got: {}",
            s
        );
    }

    #[test]
    fn value_axis_content_hash_deterministic() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let a2 = make_axis(make_point3_origin(), make_direction_z());
        assert_eq!(a1.content_hash(), a2.content_hash());
    }

    #[test]
    fn value_axis_content_hash_no_collision_with_plane() {
        let axis = make_axis(make_point3_origin(), make_direction_z());
        let plane = make_plane(make_point3_origin(), make_normal_z());
        // Plane tag=22, Axis tag=23 — distinct even if fields match
        assert_ne!(axis.content_hash(), plane.content_hash());
    }

    #[test]
    fn value_axis_ord_cross_type() {
        // Axis type_tag=23 > Plane type_tag=22
        let axis = make_axis(make_point3_origin(), make_direction_z());
        let plane = make_plane(make_point3_origin(), make_normal_z());
        assert!(axis > plane);
    }

    #[test]
    fn value_axis_dimension_dimensionless() {
        let axis = make_axis(make_point3_origin(), make_direction_z());
        assert_eq!(axis.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::Direction tests (step-3) ───────────────────────────────────────
    //
    // Value::Direction { x, y, z } is a dimensionless 3D unit vector (inline
    // floats, mirroring Value::Orientation's layout). These tests pin the
    // construction/round-trip, type inference, dimensionlessness, Display, and —
    // critically — the same-type `eq` and `cmp` arms. The `eq` impl ends in
    // `_ => false` and the same-type `cmp` match ends in
    // `_ => unreachable!("same type tag but different variants")`, so a missing
    // Direction arm would silently break equality / PANIC on ordering. (e)/(f)
    // therefore fail at runtime until step-4 adds those explicit arms (and the
    // whole block fails to compile until the variant exists).

    fn make_direction(x: f64, y: f64, z: f64) -> Value {
        Value::Direction { x, y, z }
    }

    #[test]
    fn value_direction_construction() {
        // (a) construction + field round-trip
        let d = make_direction(1.0, 0.0, 0.0);
        match d {
            Value::Direction { x, y, z } => {
                assert_eq!(x, 1.0);
                assert_eq!(y, 0.0);
                assert_eq!(z, 0.0);
            }
            other => panic!("expected Value::Direction, got {:?}", other),
        }
    }

    #[test]
    fn value_direction_infer_type() {
        // (b) try_infer_type() returns Some(Type::Direction)
        let d = make_direction(0.0, 0.0, 1.0);
        assert_eq!(d.try_infer_type(), Some(reify_core::ty::Type::Direction));
    }

    #[test]
    fn value_direction_dimension_dimensionless() {
        // (c) a Direction is dimensionless (dimensionless unit vector)
        let d = make_direction(0.0, 1.0, 0.0);
        assert_eq!(d.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn value_direction_display() {
        // (d) Display is stable and contains the components
        let d = make_direction(1.0, 0.0, 0.0);
        let s = format!("{}", d);
        assert!(
            s.starts_with("direction("),
            "display should start with 'direction(', got: {}",
            s
        );
        assert!(
            s.contains('1'),
            "display should contain the x component, got: {}",
            s
        );
    }

    #[test]
    fn value_direction_partial_eq_same() {
        // (e) equal-for-equal — pins the `eq` arm (the impl ends in `_ => false`,
        // so a missing Direction arm makes equal Directions compare UNEQUAL).
        assert_eq!(make_direction(1.0, 0.0, 0.0), make_direction(1.0, 0.0, 0.0));
    }

    #[test]
    fn value_direction_partial_eq_different() {
        // (e) two distinct Directions compare unequal
        assert_ne!(make_direction(1.0, 0.0, 0.0), make_direction(0.0, 1.0, 0.0));
    }

    #[test]
    fn value_direction_partial_eq_not_axis() {
        // (e) Direction is a distinct variant from Axis (cross-variant => not equal)
        let dir = make_direction(0.0, 0.0, 1.0);
        let axis = make_axis(make_point3_origin(), make_direction_z());
        assert_ne!(dir, axis);
    }

    #[test]
    fn value_direction_ord_within_type() {
        // (f) comparing/sorting two DISTINCT Directions is consistent and does NOT
        // panic. PINS the same-type `cmp` arm: the same-type match ends in
        // `_ => unreachable!("same type tag but different variants")`, so a missing
        // Direction arm would PANIC on any two distinct Directions. Lexicographic
        // x→y→z: (0,0,0) < (1,0,0) because x differs first.
        let a = make_direction(0.0, 0.0, 0.0);
        let b = make_direction(1.0, 0.0, 0.0);
        assert!(a < b);
        // Sorting must not panic and must be deterministic.
        let mut v = vec![b.clone(), a.clone()];
        v.sort();
        assert_eq!(v, vec![a, b]);
    }

    #[test]
    fn value_direction_ord_cross_type() {
        // (g) cross-type discriminant — a Direction orders distinctly (tag-based,
        // no overlap) from Plane/Axis/Orientation. Direction's tag is the current
        // max, so it sorts after all three.
        let dir = make_direction(1.0, 0.0, 0.0);
        let plane = make_plane(make_point3_origin(), make_normal_z());
        let axis = make_axis(make_point3_origin(), make_direction_z());
        let orientation = orient(1.0, 0.0, 0.0, 0.0);
        assert!(dir > plane);
        assert!(dir > axis);
        assert!(dir > orientation);
    }

    // ── Value::BoundingBox tests (pre-4) ──────────────────────────────────────

    fn make_bbox(min: Value, max: Value) -> Value {
        Value::BoundingBox {
            min: Box::new(min),
            max: Box::new(max),
        }
    }

    fn make_point3_min() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point3_max() -> Value {
        Value::Point(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ])
    }

    #[test]
    fn value_bbox_construction() {
        let min = make_point3_min();
        let max = make_point3_max();
        let bbox = make_bbox(min.clone(), max.clone());
        match bbox {
            Value::BoundingBox { min: mn, max: mx } => {
                assert_eq!(*mn, min);
                assert_eq!(*mx, max);
            }
            other => panic!("expected Value::BoundingBox, got {:?}", other),
        }
    }

    #[test]
    fn value_bbox_partial_eq_same() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let b2 = make_bbox(make_point3_min(), make_point3_max());
        assert_eq!(b1, b2);
    }

    #[test]
    fn value_bbox_partial_eq_different() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let max2 = Value::Point(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(9.0),
        ]);
        let b2 = make_bbox(make_point3_min(), max2);
        assert_ne!(b1, b2);
    }

    #[test]
    fn value_bbox_partial_eq_different_min() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let min2 = Value::Point(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let b2 = make_bbox(min2, make_point3_max());
        assert_ne!(b1, b2);
    }

    #[test]
    fn value_bbox_display() {
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        let s = format!("{}", bbox);
        assert!(
            s.starts_with("bbox("),
            "display should start with 'bbox(', got: {}",
            s
        );
    }

    #[test]
    fn value_bbox_content_hash_deterministic() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let b2 = make_bbox(make_point3_min(), make_point3_max());
        assert_eq!(b1.content_hash(), b2.content_hash());
    }

    #[test]
    fn value_bbox_content_hash_no_collision_with_axis() {
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        let axis = make_axis(make_point3_origin(), make_direction_z());
        // BoundingBox tag=24, Axis tag=23 — distinct
        assert_ne!(bbox.content_hash(), axis.content_hash());
    }

    #[test]
    fn value_bbox_ord_cross_type() {
        // BoundingBox type_tag=24 > Axis type_tag=23
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        let axis = make_axis(make_point3_origin(), make_direction_z());
        assert!(bbox > axis);
    }

    #[test]
    fn value_bbox_dimension_dimensionless() {
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        assert_eq!(bbox.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::neg() scalar tests ───────────────────────────────────────────

    #[test]
    fn neg_int_positive() {
        assert_eq!(-Value::Int(5), Value::Int(-5));
    }

    #[test]
    fn neg_real() {
        assert_eq!(-Value::Real(2.5), Value::Real(-2.5));
    }

    #[test]
    fn neg_scalar_length() {
        assert_eq!(
            -Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: -1.0,
                dimension: DimensionVector::LENGTH,
            }
        );
    }

    #[test]
    fn neg_complex() {
        assert_eq!(
            -Value::Complex {
                re: 1.0,
                im: 2.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Complex {
                re: -1.0,
                im: -2.0,
                dimension: DimensionVector::DIMENSIONLESS,
            }
        );
    }

    #[test]
    fn neg_int_min_overflow_returns_undef() {
        assert_eq!(-Value::Int(i64::MIN), Value::Undef);
    }

    #[test]
    fn neg_bool_returns_undef() {
        assert_eq!(-Value::Bool(true), Value::Undef);
    }

    #[test]
    fn neg_undef_returns_undef() {
        assert_eq!(-Value::Undef, Value::Undef);
    }

    // ── Value::neg() composite tests ────────────────────────────────────────

    #[test]
    fn neg_tensor_int_elements() {
        assert_eq!(
            -Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(-1), Value::Int(-2)])
        );
    }

    #[test]
    fn neg_tensor_with_overflow_returns_undef() {
        // One element overflows → entire result is Undef
        assert_eq!(
            -Value::Tensor(vec![Value::Int(i64::MIN), Value::Int(1)]),
            Value::Undef
        );
    }

    #[test]
    fn neg_vector_length_components() {
        assert_eq!(
            -Value::Vector(vec![Value::length(1.0), Value::length(2.0)]),
            Value::Vector(vec![Value::length(-1.0), Value::length(-2.0)])
        );
    }

    #[test]
    fn neg_point_returns_undef() {
        // Affine geometry: point negation is undefined (spec 3.3.1)
        assert_eq!(
            -Value::Point(vec![Value::length(1.0), Value::length(2.0)]),
            Value::Undef
        );
    }

    #[test]
    fn value_point_partial_eq() {
        // (a) two identically-constructed Points are equal
        let p1 = Value::Point(vec![Value::length(1.0), Value::length(2.0)]);
        let p2 = Value::Point(vec![Value::length(1.0), Value::length(2.0)]);
        assert_eq!(p1, p2);

        // (b) Points with a differing element are unequal
        let p3 = Value::Point(vec![Value::length(9.0), Value::length(2.0)]);
        assert_ne!(p1, p3);
    }

    #[test]
    fn value_vector_partial_eq() {
        // (a) two identically-constructed Vectors are equal
        let v1 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
        let v2 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
        assert_eq!(v1, v2);

        // (b) Vectors with a differing element are unequal
        let v3 = Value::Vector(vec![Value::length(1.0), Value::length(9.0)]);
        assert_ne!(v1, v3);
    }

    /// Regression sentinel: verifies that `content_hash()` normalizes every
    /// non-canonical NaN bit pattern to the canonical `f64::NAN` bit pattern.
    /// Uses `f64::from_bits(f64::NAN.to_bits() ^ 1)` — XOR toggles the low
    /// mantissa bit while keeping the exponent fully set, so the value remains
    /// NaN by IEEE 754 but has a distinct bit pattern.  Without the
    /// canonicalization branches in `content_hash()`, the hashes would differ.
    #[test]
    fn nan_payload_canonicalized_in_content_hash() {
        let non_canon_nan = f64::from_bits(f64::NAN.to_bits() ^ 1);
        assert!(non_canon_nan.is_nan(), "non_canon_nan must still be NaN");

        // (1) Value::Real
        assert_eq!(
            Value::Real(f64::NAN).content_hash(),
            Value::Real(non_canon_nan).content_hash(),
            "Real: non-canonical NaN must hash equal to canonical NaN"
        );

        // (2) Value::Scalar — si_value field
        assert_eq!(
            Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::DIMENSIONLESS,
            }
            .content_hash(),
            Value::Scalar {
                si_value: non_canon_nan,
                dimension: DimensionVector::DIMENSIONLESS,
            }
            .content_hash(),
            "Scalar: non-canonical NaN in si_value must hash equal to canonical NaN"
        );

        // (3) Value::Complex — re field
        assert_eq!(
            Value::Complex {
                re: f64::NAN,
                im: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            }
            .content_hash(),
            Value::Complex {
                re: non_canon_nan,
                im: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            }
            .content_hash(),
            "Complex re: non-canonical NaN must hash equal to canonical NaN"
        );

        // (4) Value::Complex — im field
        assert_eq!(
            Value::Complex {
                re: 0.0,
                im: f64::NAN,
                dimension: DimensionVector::DIMENSIONLESS,
            }
            .content_hash(),
            Value::Complex {
                re: 0.0,
                im: non_canon_nan,
                dimension: DimensionVector::DIMENSIONLESS,
            }
            .content_hash(),
            "Complex im: non-canonical NaN must hash equal to canonical NaN"
        );

        // (5) Value::Orientation — w field
        assert_eq!(
            orient(f64::NAN, 0.0, 0.0, 0.0).content_hash(),
            orient(non_canon_nan, 0.0, 0.0, 0.0).content_hash(),
            "Orientation w: non-canonical NaN must hash equal to canonical NaN"
        );

        // (6) Value::Orientation — x field
        assert_eq!(
            orient(1.0, f64::NAN, 0.0, 0.0).content_hash(),
            orient(1.0, non_canon_nan, 0.0, 0.0).content_hash(),
            "Orientation x: non-canonical NaN must hash equal to canonical NaN"
        );

        // (7) Value::Orientation — y field
        assert_eq!(
            orient(1.0, 0.0, f64::NAN, 0.0).content_hash(),
            orient(1.0, 0.0, non_canon_nan, 0.0).content_hash(),
            "Orientation y: non-canonical NaN must hash equal to canonical NaN"
        );

        // (8) Value::Orientation — z field
        assert_eq!(
            orient(1.0, 0.0, 0.0, f64::NAN).content_hash(),
            orient(1.0, 0.0, 0.0, non_canon_nan).content_hash(),
            "Orientation z: non-canonical NaN must hash equal to canonical NaN"
        );
    }

    /// Regression test: every `Value` variant must have a unique tag byte in
    /// `content_hash()`.  In particular, `Value::Point` and `Value::Matrix`
    /// previously both used tag `[18]`, which caused silent cache collisions.
    #[test]
    fn content_hash_tags_are_unique_across_variants() {
        use std::collections::HashMap;

        // Build one representative of every Value variant.
        let dim = DimensionVector::LENGTH;
        let variants: Vec<(&str, Value)> = vec![
            ("Bool", Value::Bool(true)),
            ("Int", Value::Int(42)),
            ("Real", Value::Real(1.0)),
            ("String", Value::String("x".into())),
            (
                "Scalar",
                Value::Scalar {
                    si_value: 1.0,
                    dimension: dim,
                },
            ),
            (
                "Enum",
                Value::Enum {
                    type_name: "T".into(),
                    variant: "V".into(),
                },
            ),
            ("List", Value::List(vec![])),
            ("Set", Value::Set(std::collections::BTreeSet::new())),
            ("Map", Value::Map(std::collections::BTreeMap::new())),
            ("Option_None", Value::Option(None)),
            ("Option_Some", Value::Option(Some(Box::new(Value::Int(0))))),
            (
                "Field",
                Value::Field {
                    domain_type: reify_core::ty::Type::dimensionless_scalar(),
                    codomain_type: reify_core::ty::Type::dimensionless_scalar(),
                    source: FieldSourceKind::Analytical,
                    lambda: Arc::new(Value::Undef),
                },
            ),
            (
                "Lambda",
                Value::Lambda {
                    params: vec![],
                    body: Box::new(CompiledExpr {
                        kind: crate::expr::CompiledExprKind::Literal(Value::Int(0)),
                        result_type: reify_core::ty::Type::dimensionless_scalar(),
                        content_hash: ContentHash::of(&[0]),
                    }),
                    captures: ValueMap::new(),
                },
            ),
            ("Tensor", Value::Tensor(vec![])),
            ("Point", Value::Point(vec![])),
            ("Vector", Value::Vector(vec![])),
            (
                "Complex",
                Value::Complex {
                    re: 0.0,
                    im: 0.0,
                    dimension: dim,
                },
            ),
            ("Orientation", orient(1.0, 0.0, 0.0, 0.0)),
            (
                "Frame",
                Value::Frame {
                    origin: Box::new(Value::Point(vec![])),
                    basis: Box::new(orient(1.0, 0.0, 0.0, 0.0)),
                },
            ),
            (
                "Transform",
                Value::Transform {
                    rotation: Box::new(orient(1.0, 0.0, 0.0, 0.0)),
                    translation: Box::new(Value::Vector(vec![])),
                },
            ),
            (
                "Plane",
                Value::Plane {
                    origin: Box::new(Value::Point(vec![])),
                    normal: Box::new(Value::Vector(vec![])),
                },
            ),
            (
                "Axis",
                Value::Axis {
                    origin: Box::new(Value::Point(vec![])),
                    direction: Box::new(Value::Vector(vec![])),
                },
            ),
            (
                "BoundingBox",
                Value::BoundingBox {
                    min: Box::new(Value::Point(vec![])),
                    max: Box::new(Value::Point(vec![])),
                },
            ),
            ("Range", Value::range(None, None, false, false)),
            ("Matrix", Value::Matrix(vec![])),
            (
                "StructureInstance",
                Value::StructureInstance(Box::new(StructureInstanceData {
                    type_id: crate::StructureTypeId(0),
                    type_name: "S".into(),
                    version: 1,
                    fields: crate::PersistentMap::new(),
                })),
            ),
            (
                "GeometryHandle",
                Value::GeometryHandle {
                    realization_ref: reify_core::identity::RealizationNodeId::new("T", 0),
                    upstream_values_hash: [0u8; 32],
                    kernel_handle: Some(crate::geometry::GeometryHandleId(0)),
                },
            ),
            // task 3958 / α: AffineMap tag=29
            ("AffineMap", make_affine_identity()),
            ("Undef", Value::Undef),
        ];

        let mut seen: HashMap<ContentHash, &str> = HashMap::new();
        // Pre-seed with Satisfaction variants (tag 10) so any Value that
        // accidentally reuses tag 10 is caught in this single-universe check.
        seen.insert(
            Satisfaction::Satisfied.content_hash(),
            "Satisfaction::Satisfied",
        );
        seen.insert(
            Satisfaction::Violated.content_hash(),
            "Satisfaction::Violated",
        );
        seen.insert(
            Satisfaction::Indeterminate.content_hash(),
            "Satisfaction::Indeterminate",
        );
        for (name, val) in &variants {
            let hash = val.content_hash();
            if let Some(previous_name) = seen.insert(hash, name) {
                panic!("content_hash collision: Value::{name} collides with {previous_name}");
            }
        }
    }

    // ── try_infer_type() tests: None for genuinely ambiguous cases ─────────

    #[test]
    fn try_infer_type_empty_list_returns_none() {
        let v = Value::List(vec![]);
        assert_eq!(
            v.try_infer_type(),
            None,
            "empty List has no inferable element type"
        );
    }

    #[test]
    fn try_infer_type_empty_set_returns_none() {
        let v = Value::Set(BTreeSet::new());
        assert_eq!(
            v.try_infer_type(),
            None,
            "empty Set has no inferable element type"
        );
    }

    #[test]
    fn try_infer_type_empty_map_returns_none() {
        let v = Value::Map(BTreeMap::new());
        assert_eq!(
            v.try_infer_type(),
            None,
            "empty Map has no inferable key/value types"
        );
    }

    #[test]
    fn try_infer_type_option_none_returns_none() {
        let v = Value::Option(None);
        assert_eq!(
            v.try_infer_type(),
            None,
            "Option(None) has no inferable inner type"
        );
    }

    // ── try_infer_type() tests: Some(correct_type) for populated values ────

    #[test]
    fn try_infer_type_nonempty_list_returns_some_list_int() {
        let v = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(
            v.try_infer_type(),
            Some(reify_core::ty::Type::List(Box::new(reify_core::ty::Type::Int))),
            "non-empty List(Int) should return Some(List(Int))"
        );
    }

    #[test]
    fn try_infer_type_nonempty_set_returns_some_set_int() {
        let mut s = BTreeSet::new();
        s.insert(Value::Int(42));
        let v = Value::Set(s);
        assert_eq!(
            v.try_infer_type(),
            Some(reify_core::ty::Type::Set(Box::new(reify_core::ty::Type::Int))),
            "non-empty Set(Int) should return Some(Set(Int))"
        );
    }

    #[test]
    fn try_infer_type_nonempty_map_returns_some_map_string_int() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("key".into()), Value::Int(1));
        let v = Value::Map(m);
        assert_eq!(
            v.try_infer_type(),
            Some(reify_core::ty::Type::Map(
                Box::new(reify_core::ty::Type::String),
                Box::new(reify_core::ty::Type::Int),
            )),
            "non-empty Map(String,Int) should return Some(Map(String,Int))"
        );
    }

    #[test]
    fn try_infer_type_option_some_int_returns_some_option_int() {
        let v = Value::Option(Some(Box::new(Value::Int(7))));
        assert_eq!(
            v.try_infer_type(),
            Some(reify_core::ty::Type::Option(Box::new(reify_core::ty::Type::Int))),
            "Option(Some(Int)) should return Some(Option(Int))"
        );
    }

    #[test]
    fn try_infer_type_scalar_values_return_some() {
        assert_eq!(
            Value::Bool(true).try_infer_type(),
            Some(reify_core::ty::Type::Bool)
        );
        assert_eq!(Value::Int(0).try_infer_type(), Some(reify_core::ty::Type::Int));
        assert_eq!(
            Value::Real(0.0).try_infer_type(),
            Some(reify_core::ty::Type::dimensionless_scalar())
        );
        assert_eq!(
            Value::String("".into()).try_infer_type(),
            Some(reify_core::ty::Type::String)
        );
    }

    // ── infer_type() defaults: compiler-aligned Real fallbacks ────────────

    #[test]
    fn infer_type_empty_list_uses_real_fallback() {
        use reify_core::ty::Type;
        let v = Value::List(vec![]);
        assert_eq!(
            v.infer_type(),
            Type::List(Box::new(Type::dimensionless_scalar())),
            "empty List should default element type to Real (matching compiler)"
        );
    }

    #[test]
    fn infer_type_empty_set_uses_real_fallback() {
        use reify_core::ty::Type;
        let v = Value::Set(BTreeSet::new());
        assert_eq!(
            v.infer_type(),
            Type::Set(Box::new(Type::dimensionless_scalar())),
            "empty Set should default element type to Real (matching compiler)"
        );
    }

    #[test]
    fn infer_type_empty_map_uses_string_real_fallback() {
        use reify_core::ty::Type;
        let v = Value::Map(BTreeMap::new());
        assert_eq!(
            v.infer_type(),
            Type::Map(Box::new(Type::String), Box::new(Type::dimensionless_scalar())),
            "empty Map should default value type to Real (key stays String, matching compiler)"
        );
    }

    #[test]
    fn infer_type_option_none_uses_bool_fallback() {
        assert_eq!(
            Value::Option(None).infer_type(),
            reify_core::ty::Type::Option(Box::new(reify_core::ty::Type::Bool)),
            "Option(None).infer_type() should default inner type to Bool"
        );
    }

    // ── infer_type() on Option(Some(ambiguous_inner)): regression tests ──────

    #[test]
    fn infer_type_option_some_empty_list() {
        use reify_core::ty::Type;
        // Option(Some([])) — inner is an empty list, so try_infer_type returns
        // None for the inner value and then None for the whole Option.
        // infer_type() must NOT panic; it should recurse into infer_type() on
        // the inner value (applying the Real fallback) to produce Option(List(Real)).
        let v = Value::Option(Some(Box::new(Value::List(vec![]))));
        assert_eq!(
            v.infer_type(),
            Type::Option(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
            "Option(Some(empty List)) should produce Option(List(Real)) via inner infer_type()"
        );
    }

    #[test]
    fn infer_type_option_some_option_none() {
        use reify_core::ty::Type;
        // Option(Some(Option(None))) — the inner Option(None) is ambiguous.
        // infer_type() on the inner value yields Option(Bool) via the Bool fallback,
        // so the outer result is Option(Option(Bool)).
        let v = Value::Option(Some(Box::new(Value::Option(None))));
        assert_eq!(
            v.infer_type(),
            Type::Option(Box::new(Type::Option(Box::new(Type::Bool)))),
            "Option(Some(Option(None))) should produce Option(Option(Bool)) via inner infer_type()"
        );
    }

    #[test]
    fn infer_type_option_some_empty_set() {
        use reify_core::ty::Type;
        // Option(Some(Set{})) — inner is an empty set, try_infer_type returns None.
        // infer_type() on the inner applies the Real fallback → Set(Real),
        // so outer result is Option(Set(Real)).
        let v = Value::Option(Some(Box::new(Value::Set(BTreeSet::new()))));
        assert_eq!(
            v.infer_type(),
            Type::Option(Box::new(Type::Set(Box::new(Type::dimensionless_scalar())))),
            "Option(Some(empty Set)) should produce Option(Set(Real)) via inner infer_type()"
        );
    }

    #[test]
    fn infer_type_nested_empty_list_preserves_structure() {
        use reify_core::ty::Type;
        // List(vec![List(vec![])]) — the inner list is empty so try_infer_type
        // returns None for both inner and outer. infer_type() should recurse
        // into the first element, producing List(List(Real)) not List(Real).
        let v = Value::List(vec![Value::List(vec![])]);
        assert_eq!(
            v.infer_type(),
            Type::List(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
            "List([List([])]) should produce List(List(Real)), not List(Real)"
        );
    }

    #[test]
    fn infer_type_nested_empty_set_preserves_structure() {
        use reify_core::ty::Type;
        let v = Value::Set([Value::Set(BTreeSet::new())].into_iter().collect());
        assert_eq!(
            v.infer_type(),
            Type::Set(Box::new(Type::Set(Box::new(Type::dimensionless_scalar())))),
            "Set({{Set({{}})}}) should produce Set(Set(Real)), not Set(Real)"
        );
    }

    #[test]
    fn infer_type_map_with_ambiguous_values_preserves_structure() {
        use reify_core::ty::Type;
        // Map with a string key and an empty list value — the value is ambiguous.
        let mut m = std::collections::BTreeMap::new();
        m.insert(Value::String("k".into()), Value::List(vec![]));
        let v = Value::Map(m);
        assert_eq!(
            v.infer_type(),
            Type::Map(
                Box::new(Type::String),
                Box::new(Type::List(Box::new(Type::dimensionless_scalar())))
            ),
            "Map with empty-list value should produce Map(String, List(Real))"
        );
    }

    #[test]
    fn try_infer_type_option_some_ambiguous_returns_none() {
        // try_infer_type() propagates None upward for nested ambiguous cases.
        // Option(Some(empty List)) — the inner try_infer_type() returns None
        // (empty list is ambiguous), and the ? propagates that None outward.
        let v = Value::Option(Some(Box::new(Value::List(vec![]))));
        assert_eq!(
            v.try_infer_type(),
            None,
            "try_infer_type() on Option(Some(empty List)) should return None (inner is ambiguous)"
        );
    }

    // ── Point/Vector infer_type / try_infer_type tests (task 3749) ──────────

    /// Empty Point has no inferable quantity type — try_infer_type() returns None.
    ///
    /// Mirrors `try_infer_type_empty_list_returns_none`: the `?` on
    /// `components.first()?` propagates None out of the Point arm.
    #[test]
    fn try_infer_type_empty_point_returns_none() {
        let v = Value::Point(vec![]);
        assert_eq!(
            v.try_infer_type(),
            None,
            "empty Point has no inferable quantity type"
        );
    }

    /// Empty Vector has no inferable quantity type — try_infer_type() returns None.
    #[test]
    fn try_infer_type_empty_vector_returns_none() {
        let v = Value::Vector(vec![]);
        assert_eq!(
            v.try_infer_type(),
            None,
            "empty Vector has no inferable quantity type"
        );
    }

    /// Non-empty Point — infer_type() returns the correct Type::Point.
    ///
    /// This exercises the happy path (n > 0), which is unaffected by step-04.
    #[test]
    fn infer_type_nonempty_point_returns_point_real() {
        use reify_core::ty::Type;
        let v = Value::Point(vec![Value::Real(1.0), Value::Real(2.0)]);
        assert_eq!(
            v.infer_type(),
            Type::Point {
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
            "non-empty Point(Real, Real) should infer as Type::Point {{ n: 2, quantity: Real }}"
        );
    }

    /// Non-empty Vector — infer_type() returns the correct Type::Vector.
    #[test]
    fn infer_type_nonempty_vector_returns_vector_real() {
        use reify_core::ty::Type;
        let v = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        assert_eq!(
            v.infer_type(),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
            "non-empty Vector(Real×3) should infer as Type::Vector {{ n: 3, quantity: Real }}"
        );
    }

    /// `infer_type()` on an empty `Value::Point` panics in debug builds (debug_assert).
    ///
    /// Gated by `#[cfg(debug_assertions)]` — in release builds the debug_assert is a
    /// no-op and the `unreachable!()` arm is not exercised (task 3749, step-04 tightening).
    /// Expected panic message matches the string written to the debug_assert in step-04.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "empty Point")]
    fn infer_type_empty_point_panics_in_debug() {
        let _ = Value::Point(vec![]).infer_type();
    }

    /// `infer_type()` on an empty `Value::Vector` panics in debug builds (debug_assert).
    ///
    /// Gated by `#[cfg(debug_assertions)]` — in release builds the debug_assert is a
    /// no-op and the `unreachable!()` arm is not exercised (task 3749, step-04 tightening).
    /// Expected panic message matches the string written to the debug_assert in step-04.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "empty Vector")]
    fn infer_type_empty_vector_panics_in_debug() {
        let _ = Value::Vector(vec![]).infer_type();
    }

    // --- Freshness::default() tests (task #2326) ---

    #[test]
    fn freshness_default_is_final() {
        // The canonical fallback for cache reads on absent entries must be Final.
        // This pins the type-level default so CacheStore::freshness() (task #2326)
        // can delegate to Freshness::default() rather than hard-coding Freshness::Final.
        assert_eq!(Freshness::default(), Freshness::Final);
    }

    // ── dimension_unit_label / format_hover Money tests ──────────────────────

    #[test]
    fn dimension_unit_label_money_returns_usd() {
        // dimension_unit_label must return "USD" for MONEY, not "SI".
        assert_eq!(
            dimension_unit_label(&DimensionVector::MONEY),
            "USD",
            "dimension_unit_label(MONEY) should return \"USD\" (source-form), not fall through to \"SI\""
        );
    }

    #[test]
    fn format_hover_money_scalar_renders_usd() {
        // Value::Scalar with MONEY dimension must format as "25 USD", not "25 SI".
        let v = Value::Scalar {
            si_value: 25.0,
            dimension: DimensionVector::MONEY,
        };
        assert_eq!(
            v.format_hover(),
            "25 USD",
            "format_hover() on a Money scalar should render \"25 USD\", not \"25 SI\""
        );
    }

    // --- Freshness::is_final tests (task #2356) ---

    #[test]
    fn freshness_is_final_returns_true_only_for_final() {
        // is_final() must return true ONLY for Freshness::Final.
        assert!(Freshness::Final.is_final(), "Final.is_final() must be true");
        assert!(
            !Freshness::Intermediate { generation: 1 }.is_final(),
            "Intermediate.is_final() must be false"
        );
        assert!(
            !Freshness::Pending {
                last_substantive: ResultRef::none()
            }
            .is_final(),
            "Pending.is_final() must be false"
        );
        assert!(
            !Freshness::Failed {
                error: ErrorRef::new("e")
            }
            .is_final(),
            "Failed.is_final() must be false"
        );
    }

    // --- SampledField / Value::SampledField tests (task 2341 step-3) ---

    /// Helper: build a 1D `SampledField` over `[0.0, 1.0]` with three samples.
    /// Used by the three round-trip tests below.
    fn sample_field_1d_fixture() -> SampledField {
        SampledField {
            name: "f".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![1.0],
            spacing: vec![0.5],
            axis_grids: vec![vec![0.0, 0.5, 1.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![0.0, 1.0, 2.0],
            oob_emitted: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Two `SampledField` values with identical semantic content compare equal,
    /// even if their `oob_emitted` AtomicBools have observed different writes.
    /// (AtomicBool is a runtime-mutability slot and is intentionally excluded from PartialEq.)
    #[test]
    fn sampled_field_value_partial_eq() {
        use std::sync::atomic::Ordering;
        let a = Value::SampledField(sample_field_1d_fixture());
        let b = Value::SampledField(sample_field_1d_fixture());
        assert_eq!(a, b);

        // After flipping b's oob_emitted, the values must STILL compare equal —
        // the runtime-only flag is not part of the PartialEq contract.
        if let Value::SampledField(sf) = &b {
            sf.oob_emitted.store(true, Ordering::Release);
        }
        assert_eq!(
            a, b,
            "AtomicBool oob_emitted must be excluded from PartialEq"
        );
    }

    /// `Value::SampledField`'s Ord type-tag (25) places it after `BoundingBox` (24).
    /// This pins the type-tag registry so a future reorganisation that drops
    /// `SampledField` from the tuple-match in `impl Ord for Value` is caught here.
    #[test]
    fn sampled_field_value_ord_type_tag_is_unique() {
        use std::cmp::Ordering;
        let sf = Value::SampledField(sample_field_1d_fixture());
        let bbox = Value::BoundingBox {
            min: Box::new(Value::Point(vec![Value::Real(0.0)])),
            max: Box::new(Value::Point(vec![Value::Real(1.0)])),
        };
        // SampledField (tag=25) sorts strictly AFTER BoundingBox (tag=24).
        assert_eq!(sf.cmp(&bbox), Ordering::Greater);
        assert_eq!(bbox.cmp(&sf), Ordering::Less);
        // SampledField sorts strictly AFTER Matrix (tag=19) and Field (tag=11).
        let matrix = Value::Matrix(vec![vec![Value::Real(0.0)]]);
        assert_eq!(sf.cmp(&matrix), Ordering::Greater);
    }

    /// `Value::SampledField` produces a non-zero content hash that is distinct
    /// from `Value::Undef`. Pins the new content-hash tag (26) so a regression
    /// that collapses the hash for SampledField to the Undef fallback is caught.
    #[test]
    fn sampled_field_value_content_hash_is_nonzero_and_distinct_from_undef() {
        let sf = Value::SampledField(sample_field_1d_fixture());
        let h = sf.content_hash();
        let undef_h = Value::Undef.content_hash();
        assert_ne!(h, undef_h);
        // Determinism: same inputs → same hash.
        let sf2 = Value::SampledField(sample_field_1d_fixture());
        assert_eq!(sf.content_hash(), sf2.content_hash());
    }

    // --- SampledField::grid_metadata_eq unit tests (task 3515) ---

    /// Identical fixtures compare as equal via `grid_metadata_eq`.
    #[test]
    fn sampled_field_grid_metadata_eq_identical_returns_true() {
        let a = sample_field_1d_fixture();
        let b = sample_field_1d_fixture();
        assert!(
            a.grid_metadata_eq(&b),
            "identical SampledFields must return true from grid_metadata_eq"
        );
    }

    /// Mutating `name` alone must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_name_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.name = "g".to_string();
        assert!(
            !a.grid_metadata_eq(&b),
            "different name must return false from grid_metadata_eq"
        );
    }

    /// Mutating `kind` alone must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_kind_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.kind = SampledGridKind::Regular2D;
        assert!(
            !a.grid_metadata_eq(&b),
            "different kind must return false from grid_metadata_eq"
        );
    }

    /// Mutating `bounds_min[0]` alone must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_bounds_min_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.bounds_min[0] = -1.0;
        assert!(
            !a.grid_metadata_eq(&b),
            "different bounds_min must return false from grid_metadata_eq"
        );
    }

    /// Mutating `bounds_max[0]` alone must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_bounds_max_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.bounds_max[0] = 2.0;
        assert!(
            !a.grid_metadata_eq(&b),
            "different bounds_max must return false from grid_metadata_eq"
        );
    }

    /// Mutating `spacing[0]` alone must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_spacing_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.spacing[0] = 0.25;
        assert!(
            !a.grid_metadata_eq(&b),
            "different spacing must return false from grid_metadata_eq"
        );
    }

    /// Mutating `axis_grids[0][1]` alone must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_axis_grids_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.axis_grids[0][1] = 0.75;
        assert!(
            !a.grid_metadata_eq(&b),
            "different axis_grids must return false from grid_metadata_eq"
        );
    }

    /// Changing `interpolation` (Linear → NearestNeighbor) must yield false.
    #[test]
    fn sampled_field_grid_metadata_eq_interpolation_change_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.interpolation = InterpolationKind::NearestNeighbor;
        assert!(
            !a.grid_metadata_eq(&b),
            "different interpolation must return false from grid_metadata_eq"
        );
    }

    /// Replacing `data` (same length, different values) must still return true —
    /// `grid_metadata_eq` deliberately skips the value payload.
    #[test]
    fn sampled_field_grid_metadata_eq_data_change_returns_true() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.data = vec![9.0, 8.0, 7.0];
        assert!(
            a.grid_metadata_eq(&b),
            "different data must still return true from grid_metadata_eq (data is skipped)"
        );
    }

    /// Flipping `oob_emitted` must still return true —
    /// `grid_metadata_eq` deliberately skips the runtime-mutability flag,
    /// mirroring the AtomicBool exclusion in PartialEq.
    #[test]
    fn sampled_field_grid_metadata_eq_oob_emitted_change_returns_true() {
        use std::sync::atomic::Ordering;
        let a = sample_field_1d_fixture();
        let b = sample_field_1d_fixture();
        b.oob_emitted.store(true, Ordering::Release);
        assert!(
            a.grid_metadata_eq(&b),
            "different oob_emitted must still return true from grid_metadata_eq (flag is skipped)"
        );
    }

    /// A length mismatch in a `Vec<f64>` geometry field must yield false.
    ///
    /// Pins the `xs.len() == ys.len()` length-prefix check inside the
    /// `vecs_bit_eq` closure.  A 1D vs 2D field (different number of dimension
    /// coordinates) must not compare as equal even if the shorter prefix matches.
    #[test]
    fn sampled_field_grid_metadata_eq_bounds_min_length_mismatch_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.bounds_min.push(0.0); // now length 2 vs. a's length 1
        assert!(
            !a.grid_metadata_eq(&b),
            "bounds_min length mismatch must return false from grid_metadata_eq"
        );
    }

    /// An outer-length mismatch in `axis_grids` must yield false.
    ///
    /// Pins the `self.axis_grids.len() != other.axis_grids.len()` early return.
    /// A field with one axis vs. two axes must not compare as equal.
    #[test]
    fn sampled_field_grid_metadata_eq_axis_grids_length_mismatch_returns_false() {
        let a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        b.axis_grids.push(vec![0.0, 1.0]); // outer length 2 vs. a's outer length 1
        assert!(
            !a.grid_metadata_eq(&b),
            "axis_grids outer length mismatch must return false from grid_metadata_eq"
        );
    }

    /// `+0.0` and `-0.0` must compare as **different** via `grid_metadata_eq`.
    ///
    /// Pins the bit-equality semantics documented on the method: `f64::to_bits()`
    /// distinguishes `+0.0` (bit pattern 0x0000…) from `-0.0` (bit pattern
    /// 0x8000…), so a spacing of `+0.0` is treated as a physically distinct grid
    /// from `-0.0`.  If a future contributor replaces `to_bits()` with plain `==`,
    /// this test will catch the regression.
    #[test]
    fn sampled_field_grid_metadata_eq_positive_zero_vs_negative_zero_returns_false() {
        let mut a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        a.spacing[0] = 0.0_f64; // +0.0
        b.spacing[0] = -0.0_f64; // -0.0  (different bit pattern)
        assert!(
            !a.grid_metadata_eq(&b),
            "+0.0 and -0.0 must compare as different (bit-equality semantics)"
        );
    }

    // --- SampledField::grid_metadata_eq comprehensive contract tests (task 3650) ---

    /// Two NaN values with **identical** bit patterns must compare as equal via
    /// `grid_metadata_eq`, while two NaN values with **different** bit patterns
    /// must compare as unequal.
    ///
    /// Pins the bidirectional NaN bit-equality contract documented on the method:
    /// the impl uses `f64::to_bits()` comparison, so same-bit-pattern NaNs are
    /// equal and distinct-bit-pattern NaNs are not.
    ///
    /// Both halves are needed:
    /// - Same-bits-equal: fails if a contributor replaces `to_bits()==to_bits()`
    ///   with plain `==` (NaN != NaN always, so the result would flip to false).
    /// - Different-bits-unequal: fails if a contributor introduces NaN
    ///   canonicalisation (all NaNs would be treated as equal).
    #[test]
    fn sampled_field_grid_metadata_eq_nan_bits_equal_returns_true() {
        // Half 1: same bit-pattern NaN → equal.
        let mut a = sample_field_1d_fixture();
        let mut b = sample_field_1d_fixture();
        a.spacing[0] = f64::NAN;
        b.spacing[0] = f64::from_bits(f64::NAN.to_bits()); // identical bit pattern
        assert!(
            a.grid_metadata_eq(&b),
            "same-bit-pattern NaN values must compare as equal (NaN bit-equality contract)"
        );

        // Half 2: distinct bit-pattern NaN → not equal.
        let mut c = sample_field_1d_fixture();
        let mut d = sample_field_1d_fixture();
        c.spacing[0] = f64::NAN;
        d.spacing[0] = f64::from_bits(f64::NAN.to_bits() | 1); // different payload bit
        assert!(
            !c.grid_metadata_eq(&d),
            "distinct-bit-pattern NaN values must compare as unequal (NaN bit-equality contract)"
        );
    }

    // ── format_display_triple unit tests (Task 3648) ─────────────────────────

    /// Scalar mm(4.2) → Some((4.2, "4.2", "mm")):
    /// display_value is the engineering-unit f64, formatted string has no unit,
    /// and unit_str is the engineering unit symbol.
    #[test]
    fn format_display_triple_scalar_fractional_returns_f64_formatted_unit() {
        // mm(4.2): si_value = 0.0042, dimension = LENGTH
        // to_display_units(0.0042) on LENGTH → (4.2, "mm")
        let v = Value::Scalar {
            si_value: 0.0042,
            dimension: DimensionVector::LENGTH,
        };
        let (display_value, formatted, unit) =
            v.format_display_triple().expect("Scalar must return Some");
        assert!(
            (display_value - 4.2).abs() < 1e-10,
            "display_value must be 4.2, got {}",
            display_value
        );
        assert_eq!(formatted, "4.2", "formatted must be '4.2'");
        assert_eq!(unit, "mm", "unit must be 'mm'");
    }

    /// Scalar mm(80.0) → Some((80.0, "80", "mm")):
    /// format_display_number trims trailing `.0` for whole numbers.
    #[test]
    fn format_display_triple_scalar_whole_number_trims_decimal() {
        // mm(80.0): si_value = 0.080, dimension = LENGTH
        // to_display_units(0.080) → (80.0, "mm")
        // format_display_number(80.0) → "80" (no trailing .0)
        let v = Value::Scalar {
            si_value: 0.080,
            dimension: DimensionVector::LENGTH,
        };
        let (display_value, formatted, unit) =
            v.format_display_triple().expect("Scalar must return Some");
        assert!(
            (display_value - 80.0).abs() < 1e-10,
            "display_value must be 80.0, got {}",
            display_value
        );
        assert_eq!(formatted, "80", "formatted must be '80' (no trailing .0)");
        assert_eq!(unit, "mm", "unit must be 'mm'");
    }

    /// Value::Option(Some(Scalar)) recurses to the inner Scalar, returns Some.
    #[test]
    fn format_display_triple_option_some_scalar_recurses_to_inner() {
        let inner = Value::Scalar {
            si_value: 0.0042,
            dimension: DimensionVector::LENGTH,
        };
        let v = Value::Option(Some(Box::new(inner)));
        let (display_value, formatted, unit) = v
            .format_display_triple()
            .expect("Option(Some(Scalar)) must recurse and return Some");
        assert!(
            (display_value - 4.2).abs() < 1e-10,
            "Option(Some(Scalar)) must recurse: display_value must be 4.2, got {}",
            display_value
        );
        assert_eq!(formatted, "4.2", "formatted must be '4.2'");
        assert_eq!(unit, "mm", "unit must be 'mm'");
    }

    /// Non-Scalar variant (Bool) → None:
    /// format_display_triple returns None for variants that are not physical
    /// scalars; callers handle the None case by emitting their own sentinel.
    #[test]
    fn format_display_triple_non_scalar_returns_none() {
        assert!(
            Value::Bool(true).format_display_triple().is_none(),
            "Bool must return None — not a physical scalar"
        );
        assert!(
            Value::Int(5).format_display_triple().is_none(),
            "Int must return None — not a physical scalar"
        );
    }

    /// Value::Option(None) → None: pins the `_ => None` fallthrough arm for the
    /// empty-Option case. Distinct from non-Scalar primitives like Bool/Int —
    /// Option(None) is an Option wrapper carrying nothing, not a different value
    /// family. If a future contributor changes the `_ => None` arm to something
    /// more permissive, this test will catch it.
    #[test]
    fn format_display_triple_option_none_returns_none() {
        assert!(
            Value::Option(None).format_display_triple().is_none(),
            "Option(None) must return None — falls through the `_ => None` arm"
        );
    }

    // ── Value::Selector substrate tests (step-3 RED / task 4116 α) ───────────
    //
    // These tests reference GeometryHandleRef, SelectorValue, LeafQuery, SelectorNode,
    // and Value::Selector which don't exist until step-4. They fail to compile
    // until the step-4 implementation lands.
    mod selector {
        use super::*;
        use reify_core::ty::{SelectorKind, Type};
        use reify_core::identity::RealizationNodeId;
        use crate::geometry::GeometryHandleId;
        use crate::value::{GeometryHandleRef, SelectorValue, LeafQuery};

        /// Build a GeometryHandleRef with the given fields (realized, Some-wrapped).
        fn ghr(entity: &str, index: u32, hash: [u8; 32], kernel_id: u64) -> GeometryHandleRef {
            GeometryHandleRef {
                realization_ref: RealizationNodeId::new(entity, index),
                upstream_values_hash: hash,
                kernel_handle: Some(GeometryHandleId(kernel_id)),
            }
        }

        /// Build a Value::GeometryHandle for use with from_geometry_handle (realized, Some-wrapped).
        fn gh_value(entity: &str, index: u32, hash: [u8; 32], kernel_id: u64) -> Value {
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new(entity, index),
                upstream_values_hash: hash,
                kernel_handle: Some(GeometryHandleId(kernel_id)),
            }
        }

        // (a) GeometryHandleRef: from_geometry_handle extracts fields correctly;
        //     kernel_handle excluded from Value-level equality.
        #[test]
        fn geometry_handle_ref_from_geometry_handle_extracts_fields() {
            let v = gh_value("Bracket", 0, [7u8; 32], 42);
            let r = GeometryHandleRef::from_geometry_handle(&v)
                .expect("from_geometry_handle must return Some for Value::GeometryHandle");
            assert_eq!(r.realization_ref.entity, "Bracket");
            assert_eq!(r.realization_ref.index, 0);
            assert_eq!(r.upstream_values_hash, [7u8; 32]);
            assert_eq!(r.kernel_handle, Some(GeometryHandleId(42)));
        }

        #[test]
        fn geometry_handle_ref_from_geometry_handle_non_gh_returns_none() {
            // from_geometry_handle on a non-GeometryHandle variant returns None
            assert!(GeometryHandleRef::from_geometry_handle(&Value::Real(1.0)).is_none());
            assert!(GeometryHandleRef::from_geometry_handle(&Value::Int(0)).is_none());
        }

        #[test]
        fn geometry_handle_ref_kernel_handle_excluded_from_selector_value_equality() {
            // Two selectors whose target refs differ only in kernel_handle must
            // compare equal at both the SelectorValue and Value::Selector levels
            // (kernel_handle is ephemeral — GHR-β §DD).
            let target_a = ghr("Bracket", 0, [7u8; 32], 42);
            let target_b = ghr("Bracket", 0, [7u8; 32], 99); // different kernel_handle
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let sv_a = SelectorValue::leaf(SelectorKind::Face, target_a, q.clone()).unwrap();
            let sv_b = SelectorValue::leaf(SelectorKind::Face, target_b, q).unwrap();
            // SelectorValue-level equality (impl PartialEq via content_hash):
            assert_eq!(sv_a, sv_b,
                "SelectorValue equality must exclude kernel_handle (GHR-β §DD)");
            // GeometryHandleRef-level equality also excludes kernel_handle:
            let ghr_a = ghr("Bracket", 0, [7u8; 32], 42);
            let ghr_b = ghr("Bracket", 0, [7u8; 32], 99);
            assert_eq!(ghr_a, ghr_b,
                "GeometryHandleRef equality must exclude kernel_handle (GHR-β §DD)");
            // Value-level equality:
            let va = Value::Selector(sv_a);
            let vb = Value::Selector(sv_b);
            assert_eq!(va, vb,
                "Value::Selector equality must exclude kernel_handle (GHR-β §DD)");
            assert_eq!(
                va.content_hash(),
                vb.content_hash(),
                "content_hash must exclude kernel_handle"
            );
        }

        #[test]
        fn selector_value_union_commutative_hash() {
            // union([a, b]) and union([b, a]) must produce the same hash and
            // compare equal, because Union is a commutative set operation.
            let qa = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let qb = LeafQuery::ByNormal { dir: [1., 0., 0.], tol_rad: 0.02 };
            let leaf_a =
                SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [1u8; 32], 1), qa)
                    .unwrap();
            let leaf_b =
                SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [2u8; 32], 1), qb)
                    .unwrap();
            let union_ab = SelectorValue::union(vec![leaf_a.clone(), leaf_b.clone()]).unwrap();
            let union_ba = SelectorValue::union(vec![leaf_b, leaf_a]).unwrap();
            assert_eq!(
                union_ab.content_hash(),
                union_ba.content_hash(),
                "union([a,b]) and union([b,a]) must hash identically (commutative)"
            );
            assert_eq!(union_ab, union_ba,
                "union([a,b]) and union([b,a]) must be equal (commutative)");
        }

        #[test]
        fn selector_value_intersect_commutative_hash() {
            // intersect([a, b]) == intersect([b, a]) for the same reason.
            let qa = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let qb = LeafQuery::ByNormal { dir: [1., 0., 0.], tol_rad: 0.02 };
            let leaf_a =
                SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [1u8; 32], 1), qa)
                    .unwrap();
            let leaf_b =
                SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [2u8; 32], 1), qb)
                    .unwrap();
            let i_ab = SelectorValue::intersect(vec![leaf_a.clone(), leaf_b.clone()]).unwrap();
            let i_ba = SelectorValue::intersect(vec![leaf_b, leaf_a]).unwrap();
            assert_eq!(
                i_ab.content_hash(),
                i_ba.content_hash(),
                "intersect([a,b]) and intersect([b,a]) must hash identically (commutative)"
            );
            assert_eq!(i_ab, i_ba,
                "intersect([a,b]) and intersect([b,a]) must be equal (commutative)");
        }

        #[test]
        fn selector_error_display_kind_mismatch() {
            use crate::value::SelectorError;
            let e = SelectorError::KindMismatch {
                expected: SelectorKind::Face,
                found: SelectorKind::Edge,
            };
            let s = e.to_string();
            assert!(s.contains("FaceSelector") && s.contains("EdgeSelector"),
                "KindMismatch Display must include both kind names; got: {s}");
        }

        #[test]
        fn selector_error_display_empty_composition() {
            use crate::value::SelectorError;
            let e = SelectorError::EmptyComposition;
            let s = e.to_string();
            assert!(!s.is_empty(),
                "EmptyComposition Display must be non-empty; got: {s}");
        }

        // (b) Type inference: Value::Selector carries SelectorKind; infer_type delegates.
        #[test]
        fn value_selector_infer_type_face() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let sv = SelectorValue::leaf(SelectorKind::Face, target, q).unwrap();
            let v = Value::Selector(sv);
            assert_eq!(v.try_infer_type(), Some(Type::Selector(SelectorKind::Face)));
            assert_eq!(v.infer_type(), Type::Selector(SelectorKind::Face));
        }

        #[test]
        fn value_selector_infer_type_edge() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByLength { min_m: 0.0, max_m: 1.0 };
            let sv = SelectorValue::leaf(SelectorKind::Edge, target, q).unwrap();
            let v = Value::Selector(sv);
            assert_eq!(v.try_infer_type(), Some(Type::Selector(SelectorKind::Edge)));
            assert_eq!(v.infer_type(), Type::Selector(SelectorKind::Edge));
        }

        #[test]
        fn value_selector_infer_type_body() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::All;
            let sv = SelectorValue::leaf(SelectorKind::Body, target, q).unwrap();
            let v = Value::Selector(sv);
            assert_eq!(v.try_infer_type(), Some(Type::Selector(SelectorKind::Body)));
        }

        // (c) content_hash determinism: same selector hashes equal; different hashes differ.
        #[test]
        fn selector_value_content_hash_deterministic() {
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let sv1 = SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), q.clone()).unwrap();
            let sv2 = SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), q).unwrap();
            assert_eq!(sv1.content_hash(), sv2.content_hash(),
                "same construction must produce identical content_hash");
        }

        #[test]
        fn selector_value_content_hash_differs_by_kind() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let qf = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let qe = LeafQuery::ByLength { min_m: 0.0, max_m: 1.0 };
            let sv_face = SelectorValue::leaf(SelectorKind::Face, target.clone(), qf).unwrap();
            let sv_edge = SelectorValue::leaf(SelectorKind::Edge, target, qe).unwrap();
            assert_ne!(sv_face.content_hash(), sv_edge.content_hash(),
                "different kind must produce different content_hash");
        }

        #[test]
        fn selector_value_content_hash_differs_by_query() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q1 = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let q2 = LeafQuery::ByNormal { dir: [1., 0., 0.], tol_rad: 0.01 };
            let sv1 = SelectorValue::leaf(SelectorKind::Face, target.clone(), q1).unwrap();
            let sv2 = SelectorValue::leaf(SelectorKind::Face, target, q2).unwrap();
            assert_ne!(sv1.content_hash(), sv2.content_hash(),
                "different query direction must produce different content_hash");
        }

        #[test]
        fn selector_value_content_hash_union_ne_intersect() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let leaf_a = SelectorValue::leaf(SelectorKind::Face, target.clone(), q.clone()).unwrap();
            let leaf_b = SelectorValue::leaf(SelectorKind::Face, target, q).unwrap();
            let union = SelectorValue::union(vec![leaf_a.clone(), leaf_b.clone()]).unwrap();
            let intersect = SelectorValue::intersect(vec![leaf_a, leaf_b]).unwrap();
            assert_ne!(union.content_hash(), intersect.content_hash(),
                "Union and Intersect of same children must hash differently");
        }

        // (d) Value-level equality/round-trip.
        #[test]
        fn value_selector_clone_eq() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let sv = SelectorValue::leaf(SelectorKind::Face, target, q).unwrap();
            let v = Value::Selector(sv);
            assert_eq!(v.clone(), v, "clone of Value::Selector must equal original");
        }

        #[test]
        fn value_selector_equal_same_structure() {
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let va = Value::Selector(
                SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), q.clone()).unwrap()
            );
            let vb = Value::Selector(
                SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), q).unwrap()
            );
            assert_eq!(va, vb, "identical selectors must compare equal");
        }

        #[test]
        fn value_selector_union_ne_intersect() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let leaf_a = SelectorValue::leaf(SelectorKind::Face, target.clone(), q.clone()).unwrap();
            let leaf_b = SelectorValue::leaf(SelectorKind::Face, target, q).unwrap();
            let vu = Value::Selector(SelectorValue::union(vec![leaf_a.clone(), leaf_b.clone()]).unwrap());
            let vi = Value::Selector(SelectorValue::intersect(vec![leaf_a, leaf_b]).unwrap());
            assert_ne!(vu, vi, "Union and Intersect of same children must be not equal");
        }

        // (e) Display for Value::Selector produces a non-empty stable string.
        #[test]
        fn value_selector_display_non_empty() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let sv = SelectorValue::leaf(SelectorKind::Face, target, q).unwrap();
            let v = Value::Selector(sv);
            let s = format!("{}", v);
            assert!(!s.is_empty(), "Display of Value::Selector must be non-empty");
            // Second construction produces the same Display string (stable)
            let target2 = ghr("B", 0, [0u8; 32], 1);
            let q2 = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let sv2 = SelectorValue::leaf(SelectorKind::Face, target2, q2).unwrap();
            let v2 = Value::Selector(sv2);
            assert_eq!(format!("{}", v), format!("{}", v2),
                "Display of equal selectors must produce identical strings");
        }

        // ── K1 (kind-closure) constructor tests (step-5 / RED) ─────────────────
        // NOTE: K1 validation was implemented in step-4 (plan allowed this as
        // "acceptable"), so these tests are immediately GREEN.

        use crate::value::SelectorError;

        // K1 leaf↔query: ByNormal requires Face; supplying Edge must Err.
        #[test]
        fn k1_leaf_bynormal_rejects_edge() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 };
            let result = SelectorValue::leaf(SelectorKind::Edge, target, q);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Face,
                    found: SelectorKind::Edge,
                }),
                "ByNormal requires Face; Edge must be rejected"
            );
        }

        // K1 leaf↔query: ByLength requires Edge; supplying Face must Err.
        #[test]
        fn k1_leaf_bylength_rejects_face() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByLength { min_m: 0.0, max_m: 1.0 };
            let result = SelectorValue::leaf(SelectorKind::Face, target, q);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Edge,
                    found: SelectorKind::Face,
                }),
                "ByLength requires Edge; Face must be rejected"
            );
        }

        // K1 leaf↔query: ByArea requires Face; supplying Edge must Err.
        #[test]
        fn k1_leaf_byarea_rejects_edge() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByArea { min_m2: 0.0, max_m2: 1.0 };
            let result = SelectorValue::leaf(SelectorKind::Edge, target, q);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Face,
                    found: SelectorKind::Edge,
                })
            );
        }

        // K1 leaf↔query: ByHeight requires Edge; supplying Face must Err.
        #[test]
        fn k1_leaf_byheight_rejects_face() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByHeight { z_m: 0.0, tol_m: 0.01 };
            let result = SelectorValue::leaf(SelectorKind::Face, target, q);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Edge,
                    found: SelectorKind::Face,
                })
            );
        }

        // K1 leaf↔query: ByParallel requires Edge; supplying Body must Err.
        #[test]
        fn k1_leaf_byparallel_rejects_body() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByParallel { axis: [0., 0., 1.], tol_rad: 0.01 };
            let result = SelectorValue::leaf(SelectorKind::Body, target, q);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Edge,
                    found: SelectorKind::Body,
                })
            );
        }

        // K1 leaf↔query: Named and All accept any kind.
        #[test]
        fn k1_leaf_named_and_all_accept_any_kind() {
            let t1 = ghr("B", 0, [0u8; 32], 1);
            let t2 = ghr("B", 0, [0u8; 32], 1);
            let t3 = ghr("B", 0, [0u8; 32], 1);
            assert!(SelectorValue::leaf(SelectorKind::Body, t1, LeafQuery::All).is_ok());
            assert!(SelectorValue::leaf(SelectorKind::Face, t2, LeafQuery::Named("top".into())).is_ok());
            assert!(SelectorValue::leaf(SelectorKind::Edge, t3, LeafQuery::Named("rim".into())).is_ok());
        }

        // ── Task 4536: ByRole(Role) attribute-role leaf query (step-5 RED) ─────
        // RED (compile-failure) until step-6 adds the LeafQuery::ByRole variant.

        // required_kind: ByRole(MidSurfaceFace) -> Face, ByRole(MidSurfaceEdge) -> Edge.
        #[test]
        fn byrole_required_kind_maps_role_to_kind() {
            use crate::geometry::Role;
            assert_eq!(
                LeafQuery::ByRole(Role::MidSurfaceFace).required_kind(),
                Some(SelectorKind::Face),
                "ByRole(MidSurfaceFace) must require a Face-kind selector"
            );
            assert_eq!(
                LeafQuery::ByRole(Role::MidSurfaceEdge).required_kind(),
                Some(SelectorKind::Edge),
                "ByRole(MidSurfaceEdge) must require an Edge-kind selector"
            );
        }

        // K1 leaf↔query: ByRole(MidSurfaceFace) accepts Face, rejects Edge.
        #[test]
        fn k1_leaf_byrole_mid_surface_face_kind_closure() {
            use crate::geometry::Role;
            let t_ok = ghr("B", 0, [0u8; 32], 1);
            assert!(
                SelectorValue::leaf(
                    SelectorKind::Face,
                    t_ok,
                    LeafQuery::ByRole(Role::MidSurfaceFace)
                )
                .is_ok(),
                "Face-kind selector must accept a ByRole(MidSurfaceFace) leaf"
            );
            let t_bad = ghr("B", 0, [0u8; 32], 1);
            let result = SelectorValue::leaf(
                SelectorKind::Edge,
                t_bad,
                LeafQuery::ByRole(Role::MidSurfaceFace),
            );
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Face,
                    found: SelectorKind::Edge,
                }),
                "Edge-kind selector must reject a ByRole(MidSurfaceFace) leaf (K1 kind-closure)"
            );
        }

        // content_hash: equal for equal ByRole leaves; distinct across the query
        // (vs an All leaf of the same kind) and across role (Face vs Edge leaf).
        #[test]
        fn byrole_content_hash_is_stable_and_distinct() {
            use crate::geometry::Role;
            let face_role_a = SelectorValue::leaf(
                SelectorKind::Face,
                ghr("B", 0, [0u8; 32], 1),
                LeafQuery::ByRole(Role::MidSurfaceFace),
            )
            .unwrap();
            let face_role_b = SelectorValue::leaf(
                SelectorKind::Face,
                ghr("B", 0, [0u8; 32], 1),
                LeafQuery::ByRole(Role::MidSurfaceFace),
            )
            .unwrap();
            // Equal ByRole leaves hash equal (deterministic).
            assert_eq!(
                face_role_a, face_role_b,
                "two identical ByRole(MidSurfaceFace) Face leaves must be equal"
            );
            // Same kind, different query (ByRole vs All) — must differ (isolates the
            // fresh tag byte 7 + role encoding from the All tag).
            let face_all = SelectorValue::leaf(
                SelectorKind::Face,
                ghr("B", 0, [0u8; 32], 1),
                LeafQuery::All,
            )
            .unwrap();
            assert_ne!(
                face_role_a, face_all,
                "ByRole(MidSurfaceFace) must hash distinct from an All leaf of the same kind"
            );
            // Different role (and kind) — must differ.
            let edge_role = SelectorValue::leaf(
                SelectorKind::Edge,
                ghr("B", 0, [0u8; 32], 1),
                LeafQuery::ByRole(Role::MidSurfaceEdge),
            )
            .unwrap();
            assert_ne!(
                face_role_a, edge_role,
                "ByRole(MidSurfaceFace) must hash distinct from ByRole(MidSurfaceEdge)"
            );
        }

        // K1 composition: union of face + edge selector must Err.
        #[test]
        fn k1_union_rejects_mixed_kinds() {
            let face_sel = SelectorValue::leaf(
                SelectorKind::Face, ghr("B", 0, [0u8; 32], 1),
                LeafQuery::All,
            ).unwrap();
            let edge_sel = SelectorValue::leaf(
                SelectorKind::Edge, ghr("B", 0, [0u8; 32], 1),
                LeafQuery::All,
            ).unwrap();
            let result = SelectorValue::union(vec![face_sel, edge_sel]);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Face,
                    found: SelectorKind::Edge,
                }),
                "union of Face+Edge must be rejected"
            );
        }

        // K1 composition: union of same-kind selectors must Ok with correct kind.
        #[test]
        fn k1_union_same_kind_ok() {
            let fa = SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let fb = SelectorValue::leaf(SelectorKind::Face, ghr("C", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let u = SelectorValue::union(vec![fa, fb]).unwrap();
            assert_eq!(u.kind, SelectorKind::Face);
        }

        // K1 composition: intersect of mixed kinds must Err.
        #[test]
        fn k1_intersect_rejects_mixed_kinds() {
            let face_sel = SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let edge_sel = SelectorValue::leaf(SelectorKind::Edge, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            assert!(
                SelectorValue::intersect(vec![face_sel, edge_sel]).is_err(),
                "intersect of Face+Edge must be rejected"
            );
        }

        // K1 composition: intersect of same-kind must Ok.
        #[test]
        fn k1_intersect_same_kind_ok() {
            let ea = SelectorValue::leaf(SelectorKind::Edge, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let eb = SelectorValue::leaf(SelectorKind::Edge, ghr("C", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let i = SelectorValue::intersect(vec![ea, eb]).unwrap();
            assert_eq!(i.kind, SelectorKind::Edge);
        }

        // K1 composition: difference of mixed kinds must Err.
        #[test]
        fn k1_difference_rejects_mixed_kinds() {
            let face_sel = SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let edge_sel = SelectorValue::leaf(SelectorKind::Edge, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            assert!(
                SelectorValue::difference(face_sel, edge_sel).is_err(),
                "difference of Face-Edge must be rejected"
            );
        }

        // K1 composition: difference of same kind must Ok with correct kind.
        #[test]
        fn k1_difference_same_kind_ok() {
            let fa = SelectorValue::leaf(SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let fb = SelectorValue::leaf(SelectorKind::Face, ghr("C", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            let d = SelectorValue::difference(fa, fb).unwrap();
            assert_eq!(d.kind, SelectorKind::Face);
        }

        // K1 empty: union([]) => EmptyComposition.
        #[test]
        fn k1_empty_union_returns_empty_composition() {
            assert_eq!(
                SelectorValue::union(vec![]),
                Err(SelectorError::EmptyComposition)
            );
        }

        // K1 empty: intersect([]) => EmptyComposition.
        #[test]
        fn k1_empty_intersect_returns_empty_composition() {
            assert_eq!(
                SelectorValue::intersect(vec![]),
                Err(SelectorError::EmptyComposition)
            );
        }

        // ── SelectorKind::Vertex round-trip + K1 tests (step-3 RED / task 4368) ──
        // Mirrors the Face/Edge/Body cases above; all fail to compile until
        // step-4 adds Vertex => 3 to compute_content_hash's kind_byte match.

        // (a) Value::Selector(Vertex leaf) infer_type == Selector(Vertex)
        #[test]
        fn value_selector_infer_type_vertex() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let sv = SelectorValue::leaf(SelectorKind::Vertex, target, LeafQuery::All).unwrap();
            let v = Value::Selector(sv);
            assert_eq!(v.try_infer_type(), Some(Type::Selector(SelectorKind::Vertex)));
            assert_eq!(v.infer_type(), Type::Selector(SelectorKind::Vertex));
        }

        // (b) content_hash of a Vertex leaf differs from Face/Edge/Body leaves
        //     (kind_byte discrimination — kind_byte match is exhaustive and will
        //     fail to compile until Vertex => 3 is added in step-4).
        #[test]
        fn selector_value_vertex_content_hash_differs_from_face_edge_body() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let sv_vertex = SelectorValue::leaf(SelectorKind::Vertex, target.clone(), LeafQuery::All).unwrap();
            let sv_face   = SelectorValue::leaf(SelectorKind::Face,   target.clone(), LeafQuery::All).unwrap();
            let sv_edge   = SelectorValue::leaf(SelectorKind::Edge,   target.clone(), LeafQuery::All).unwrap();
            let sv_body   = SelectorValue::leaf(SelectorKind::Body,   target,          LeafQuery::All).unwrap();
            assert_ne!(sv_vertex.content_hash(), sv_face.content_hash(),
                "Vertex and Face All-leaves must hash differently");
            assert_ne!(sv_vertex.content_hash(), sv_edge.content_hash(),
                "Vertex and Edge All-leaves must hash differently");
            assert_ne!(sv_vertex.content_hash(), sv_body.content_hash(),
                "Vertex and Body All-leaves must hash differently");
        }

        // (c) Value::Selector(Vertex) eq round-trips
        #[test]
        fn value_selector_vertex_eq_ne() {
            let t1 = ghr("B", 0, [0u8; 32], 1);
            let t2 = ghr("B", 0, [0u8; 32], 1);
            let sv1 = SelectorValue::leaf(SelectorKind::Vertex, t1, LeafQuery::All).unwrap();
            let sv2 = SelectorValue::leaf(SelectorKind::Vertex, t2, LeafQuery::All).unwrap();
            let sf  = SelectorValue::leaf(SelectorKind::Face,   ghr("B", 0, [0u8; 32], 1), LeafQuery::All).unwrap();
            assert_eq!(Value::Selector(sv1.clone()), Value::Selector(sv2),
                "identical Vertex leaves must compare equal");
            assert_ne!(Value::Selector(sv1), Value::Selector(sf),
                "Vertex leaf must not equal Face leaf");
        }

        // (d1) K1: leaf(Vertex, t, All).is_ok()
        #[test]
        fn k1_vertex_all_leaf_ok() {
            let target = ghr("B", 0, [0u8; 32], 1);
            assert!(SelectorValue::leaf(SelectorKind::Vertex, target, LeafQuery::All).is_ok());
        }

        // (d2) K1: leaf(Vertex, t, Named("tip")).is_ok()
        #[test]
        fn k1_vertex_named_leaf_ok() {
            let target = ghr("B", 0, [0u8; 32], 1);
            assert!(
                SelectorValue::leaf(SelectorKind::Vertex, target, LeafQuery::Named("tip".into())).is_ok()
            );
        }

        // (e) K1 wrong-kind: leaf(Vertex, t, ByLength{..}) ==
        //     Err(KindMismatch{expected:Edge, found:Vertex})
        #[test]
        fn k1_vertex_leaf_bylength_rejects_vertex() {
            let target = ghr("B", 0, [0u8; 32], 1);
            let q = LeafQuery::ByLength { min_m: 0.0, max_m: 1.0 };
            let result = SelectorValue::leaf(SelectorKind::Vertex, target, q);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Edge,
                    found: SelectorKind::Vertex,
                }),
                "ByLength requires Edge; Vertex must be rejected"
            );
        }

        // (f) K1 cross-kind composition: union(vertex_all_leaf, face_all_leaf) == Err
        #[test]
        fn k1_union_vertex_face_rejects_mixed_kinds() {
            let vertex_sel = SelectorValue::leaf(
                SelectorKind::Vertex, ghr("B", 0, [0u8; 32], 1), LeafQuery::All,
            ).unwrap();
            let face_sel = SelectorValue::leaf(
                SelectorKind::Face, ghr("B", 0, [0u8; 32], 1), LeafQuery::All,
            ).unwrap();
            let result = SelectorValue::union(vec![vertex_sel, face_sel]);
            assert_eq!(
                result,
                Err(SelectorError::KindMismatch {
                    expected: SelectorKind::Vertex,
                    found: SelectorKind::Face,
                }),
                "union of Vertex+Face must be rejected"
            );
        }

        // K2: constructing a full nested tree (Difference(Union(leaf,leaf), leaf))
        // requires no kernel, proving construction is kernel-free.
        #[test]
        fn k2_nested_construction_is_kernel_free() {
            // All of this runs without any GeometryKernel in scope — reify-ir
            // has no kernel dependency, so constructors are pure by crate layering.
            let leaf_a = SelectorValue::leaf(
                SelectorKind::Face, ghr("Body", 0, [1u8; 32], 1),
                LeafQuery::ByNormal { dir: [0., 0., 1.], tol_rad: 0.01 },
            ).unwrap();
            let leaf_b = SelectorValue::leaf(
                SelectorKind::Face, ghr("Body", 0, [2u8; 32], 1),
                LeafQuery::ByArea { min_m2: 0.01, max_m2: 0.1 },
            ).unwrap();
            let leaf_c = SelectorValue::leaf(
                SelectorKind::Face, ghr("Body", 0, [3u8; 32], 1),
                LeafQuery::Named("top".into()),
            ).unwrap();
            let union = SelectorValue::union(vec![leaf_a, leaf_b]).unwrap();
            let result = SelectorValue::difference(union, leaf_c).unwrap();
            assert_eq!(result.kind, SelectorKind::Face);
        }
    }
}

// ── UndefCause unit tests (task 4321 / undef-self-describing α) ──────────────
//
// Tests Eq / Hash / Clone / Debug runtime behaviour on `UndefCause`.
#[cfg(test)]
mod undef_cause_tests {
    use std::collections::HashSet;

    use reify_core::diagnostics::{DiagnosticCode, SourceSpan};
    use reify_core::identity::ValueCellId;

    use super::UndefCause;

    fn span(start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(start, end)
    }

    fn cell(entity: &str, member: &str) -> ValueCellId {
        ValueCellId::new(entity, member)
    }

    // ── Construct all 5 variants and assert inter-variant inequality ──────────

    #[test]
    fn distinct_variant_kinds_are_unequal() {
        let unbound = UndefCause::Unbound {
            param: cell("S", "a"),
            span: span(0, 5),
        };
        let awaiting = UndefCause::AwaitingSolve {
            param: cell("S", "k"),
        };
        let failed = UndefCause::SolveFailed {
            detail: "infeasible".to_string(),
        };
        let contract = UndefCause::OpContractFailed {
            code: DiagnosticCode::ConstraintViolated,
            span: span(10, 20),
        };
        let user = UndefCause::UserUndef {
            span: span(30, 35),
        };

        // Every pair of distinct variants must be unequal.
        assert_ne!(unbound, awaiting);
        assert_ne!(unbound, failed);
        assert_ne!(unbound, contract);
        assert_ne!(unbound, user);
        assert_ne!(awaiting, failed);
        assert_ne!(awaiting, contract);
        assert_ne!(awaiting, user);
        assert_ne!(failed, contract);
        assert_ne!(failed, user);
        assert_ne!(contract, user);

        // Debug round-trip: every variant produces a non-empty string.
        for v in [&unbound, &awaiting, &failed, &contract, &user] {
            assert!(!format!("{v:?}").is_empty());
        }
    }

    // ── Eq / Hash: dedup-by-(kind,cell) contract that β depends on (PRD Q4) ──

    #[test]
    fn same_unbound_same_span_equal_and_hash_dedup() {
        let a = UndefCause::Unbound { param: cell("S", "x"), span: span(0, 3) };
        let b = UndefCause::Unbound { param: cell("S", "x"), span: span(0, 3) };
        assert_eq!(a, b);
        // Both inserted into a HashSet → len == 1.
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 1, "two identical Unbound entries must dedup to 1");
    }

    #[test]
    fn different_param_unbound_not_equal() {
        let a = UndefCause::Unbound { param: cell("S", "x"), span: span(0, 3) };
        let b = UndefCause::Unbound { param: cell("S", "y"), span: span(0, 3) };
        assert_ne!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 2, "Unbound with different params must be distinct");
    }

    #[test]
    fn unbound_vs_user_undef_not_equal() {
        let a = UndefCause::Unbound { param: cell("S", "x"), span: span(0, 3) };
        let b = UndefCause::UserUndef { span: span(0, 3) };
        assert_ne!(a, b);
    }

    // ── Clone + Debug round-trip ──────────────────────────────────────────────

    #[test]
    fn clone_and_debug_work() {
        let orig = UndefCause::SolveFailed { detail: "no progress: dim".to_string() };
        let cloned = orig.clone();
        assert_eq!(orig, cloned);
        // Debug must produce a non-empty string without panicking.
        let debug_str = format!("{:?}", cloned);
        assert!(!debug_str.is_empty());
    }
}
