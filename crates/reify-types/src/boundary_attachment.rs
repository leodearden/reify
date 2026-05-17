//! B-rep node-attachment vocabulary shared between mesh producers (kernel
//! adapters) and consumers (mesh-morph projection).
//!
//! Lives in `reify-types` so adapter crates (e.g. `reify-kernel-gmsh`) can emit
//! a [`BoundaryAssociation`] without taking a transitive dependency on
//! `reify-mesh-morph` → `reify-eval`, which would form a Cargo cycle through
//! `reify-eval` → `reify-solver-elastic` → `reify-kernel-gmsh`.

use std::collections::BTreeMap;

use crate::GeometryHandleId;

/// Which B-rep entity a surface node was emitted onto by the upstream surface
/// mesher.
///
/// Populated by kernel adapters that know the OCCT sub-shape attribution of
/// each surface-mesh vertex; consumed by `reify-mesh-morph`'s projection step
/// (`compute_dirichlet_bcs`) to look up the mapped counterpart entity via a
/// `CorrespondenceMap` — without any globally-closest fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeAttachment {
    /// Node lies on the interior of a B-rep face.
    OnFace(GeometryHandleId),
    /// Node lies on a B-rep edge (shared between two faces).
    OnEdge(GeometryHandleId),
    /// Node coincides with a B-rep vertex.
    OnVertex(GeometryHandleId),
}

/// Map from mesh node index to B-rep attachment, used to drive the
/// Dirichlet-BC projection step.
///
/// `node_index` keys match `VolumeMesh::vertices[index*3..index*3+3]`.
///
/// [`BTreeMap`] is chosen for deterministic iteration order so the resulting
/// Dirichlet-BC list is stable across runs. This is load-bearing for:
/// - FEA warm-start (BC order must be bit-stable between a morphed-mesh
///   rebuild and the warm-start cache lookup).
/// - Reproducible morphed-mesh caching (non-deterministic iteration would
///   force every consumer to re-sort).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundaryAssociation {
    nodes: BTreeMap<u32, NodeAttachment>,
}

impl BoundaryAssociation {
    /// Record that mesh node `idx` was emitted onto the given B-rep entity.
    ///
    /// Returns the previously-recorded attachment if the index was already
    /// present (delegates to [`BTreeMap::insert`]).
    pub fn associate(&mut self, idx: u32, attach: NodeAttachment) -> Option<NodeAttachment> {
        self.nodes.insert(idx, attach)
    }

    /// Return the B-rep attachment for the given node index, or `None` if
    /// that index has no recorded attachment.
    pub fn get(&self, idx: u32) -> Option<NodeAttachment> {
        self.nodes.get(&idx).copied()
    }

    /// Iterate over all `(node_index, attachment)` pairs in ascending
    /// node-index order (BTreeMap iteration discipline).
    pub fn iter(&self) -> impl Iterator<Item = (u32, NodeAttachment)> + '_ {
        self.nodes.iter().map(|(&k, &v)| (k, v))
    }

    /// Number of recorded node attachments.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if no node attachments have been recorded.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
