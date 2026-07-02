//! Realization-read types: `RealizedContent`, `RealizationReadHandle`.
//!
//! Moved from `reify-eval`'s `engine_compute.rs` (task A / #4934). See
//! `docs/prds/v0_6/realization-read-api.md` task α §3.1.

use std::sync::Arc;

use reify_core::{ContentHash, RealizationNodeId};
use reify_ir::{Mesh, SampledField, VolumeMesh};

/// The content of a realized geometry node.
///
/// Wraps the three concrete content kinds the realization pipeline can produce,
/// each held behind an `Arc` for cheap cloning and multi-consumer sharing.
/// `Arc<T>: Clone` is unconditional, so this enum derives `Clone` even though
/// `SampledField` itself is not `Clone` (PRD §3.1 realization-read-api.md).
///
/// `None` content on a [`RealizationReadHandle`] (BRep-only or honest degradation)
/// means every accessor returns `None` — no content is fabricated, no panic
/// occurs (invariants §3.2-5 of the PRD).
///
/// See `docs/prds/v0_6/realization-read-api.md` task α §3.1.
#[derive(Debug, Clone)]
pub enum RealizedContent {
    /// A signed-distance field (volumetric scalar field).
    Sdf(Arc<SampledField>),
    /// A tessellated surface mesh.
    SurfaceMesh(Arc<Mesh>),
    /// A tetrahedral volume mesh.
    VolumeMesh(Arc<VolumeMesh>),
}

/// Minimal read-only wrapper over a realization node identity and its optional content.
///
/// Passed to `ComputeFn` invocations that declare realization inputs.
/// `content_hash` identifies the content (matches the compute-cache key's
/// realization hash). The `content()` accessor returns the payload when content
/// is available; `None` when BRep-only or not yet hydrated (honest-degradation
/// invariant §3.2-5, realization-read-api.md §3.2).
///
/// The `content` field is private: `new()` is the sole construction path,
/// honouring PRD §3.1 "only the Engine-side constructor builds handles".
///
/// See `docs/prds/v0_6/realization-read-api.md` task α §3.1/§3.2.
#[derive(Debug, Clone)]
pub struct RealizationReadHandle {
    /// Identity of the realization node this handle references.
    pub node_id: RealizationNodeId,
    /// Content hash of the realization (mirrors the compute-cache key).
    pub content_hash: ContentHash,
    /// Optional content payload.  Private so `new()` is the sole construction
    /// path (PRD §3.1).
    content: Option<RealizedContent>,
}

impl RealizationReadHandle {
    /// Construct a handle from its three components.
    ///
    /// `pub` (not `pub(crate)`) because external integration tests and future
    /// η two-way boundary tests construct handles from outside this crate.
    pub fn new(
        node_id: RealizationNodeId,
        content_hash: ContentHash,
        content: Option<RealizedContent>,
    ) -> Self {
        Self {
            node_id,
            content_hash,
            content,
        }
    }

    /// Return a reference to the content payload, or `None` when absent.
    pub fn content(&self) -> Option<&RealizedContent> {
        self.content.as_ref()
    }

    /// Return a reference to the inner [`SampledField`] when the content is
    /// [`RealizedContent::Sdf`]; `None` otherwise.
    pub fn sdf(&self) -> Option<&SampledField> {
        match self.content.as_ref() {
            Some(RealizedContent::Sdf(a)) => Some(a),
            _ => None,
        }
    }

    /// Return a reference to the inner [`Mesh`] when the content is
    /// [`RealizedContent::SurfaceMesh`]; `None` otherwise.
    pub fn surface_mesh(&self) -> Option<&Mesh> {
        match self.content.as_ref() {
            Some(RealizedContent::SurfaceMesh(a)) => Some(a),
            _ => None,
        }
    }

    /// Return a reference to the inner [`VolumeMesh`] when the content is
    /// [`RealizedContent::VolumeMesh`]; `None` otherwise.
    pub fn volume_mesh(&self) -> Option<&VolumeMesh> {
        match self.content.as_ref() {
            Some(RealizedContent::VolumeMesh(a)) => Some(a),
            _ => None,
        }
    }

    /// Return a reference to the per-node B-rep `reify_ir::BoundaryAssociation`
    /// threaded onto the realized [`VolumeMesh`] (task 4092 — FEA face-selector
    /// boundary conditions), or `None` when no attribution is present.
    ///
    /// Boundary rides *inside* the `Arc<VolumeMesh>` (the
    /// `Option<BoundaryAssociation>` field added in step-2), so this is a
    /// one-line delegation through [`Self::volume_mesh`] — no new
    /// [`RealizedContent`] variant and no `realization_content` projection
    /// churn. Returns `None` for a `VolumeMesh` produced without attribution
    /// (`boundary: None`), for non-`VolumeMesh` content, and for a `None`-content
    /// handle. The FEA trampoline maps the resolved face handles to clamp/load
    /// node sets via this accessor + `boundary_node_set`.
    pub fn boundary(&self) -> Option<&reify_ir::BoundaryAssociation> {
        self.volume_mesh().and_then(|vm| vm.boundary.as_ref())
    }
}
