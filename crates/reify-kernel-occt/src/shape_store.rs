//! The OCCT kernel's shape store: every resident native shape and its
//! [`BRepKind`], the per-kind sub-shape extraction caches, and the child →
//! parent provenance map. Handles are minted in one place ([`ShapeStore::insert`])
//! and read through one chokepoint ([`ShapeStore::get`]).

use std::collections::HashMap;

use cxx::UniquePtr;
use reify_ir::{BRepKind, GeometryError, GeometryHandle, GeometryHandleId, QueryError};

use crate::ffi;

/// The sub-shape kinds [`ShapeStore::sub_shapes`] enumerates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubShapeKind {
    Edge,
    Face,
    Vertex,
}

impl SubShapeKind {
    fn repr(self) -> BRepKind {
        match self {
            Self::Edge => BRepKind::Edge,
            Self::Face => BRepKind::Face,
            Self::Vertex => BRepKind::Vertex,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Edge => "edge",
            Self::Face => "face",
            Self::Vertex => "vertex",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Edge => "edges",
            Self::Face => "faces",
            Self::Vertex => "vertices",
        }
    }
}

/// How an op's result [`BRepKind`] is decided: stated, copied from an input,
/// or read off the built shape by [`brep_kind_of_shape`].
#[derive(Clone, Copy)]
pub(crate) enum ReprPolicy {
    Fixed(BRepKind),
    Inherit(GeometryHandleId),
    Classify,
}

/// Classify an `OcctShape` by its BODY type: the top-level TopAbs type after
/// the single-body unwrap, so a 1-solid COMPOUND wrapper is
/// [`BRepKind::Solid`]. A type with no [`BRepKind`] is an error, never Solid.
///
/// Only the kind is unwrapped, not the stored shape: a fillet, chamfer, draft
/// or thicken result is still OCCT's COMPOUND wrapper, so C++ code branching
/// on `ShapeType()` sees COMPOUND where `repr_of` says Solid.
pub(crate) fn brep_kind_of_shape(shape: &ffi::ffi::OcctShape) -> Result<BRepKind, GeometryError> {
    let name = ffi::ffi::body_type_name(shape)
        .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
    brep_kind_of_type_name(&name)
}

/// The OCCT TopAbs type name → [`BRepKind`] table behind [`brep_kind_of_shape`].
fn brep_kind_of_type_name(name: &str) -> Result<BRepKind, GeometryError> {
    Ok(match name {
        "Solid" => BRepKind::Solid,
        "CompSolid" | "Compound" => BRepKind::Compound,
        "Shell" => BRepKind::Shell,
        "Wire" => BRepKind::Wire,
        "Face" => BRepKind::Face,
        "Edge" => BRepKind::Edge,
        "Vertex" => BRepKind::Vertex,
        other => {
            return Err(GeometryError::OperationFailed(format!(
                "cannot classify OCCT shape type {other:?} as a BRepKind"
            )));
        }
    })
}

pub(crate) struct ShapeStore {
    /// Every resident native shape, by handle id.
    shapes: HashMap<u64, UniquePtr<ffi::ffi::OcctShape>>,
    /// Each resident shape's [`BRepKind`], written with it, so its keys are
    /// exactly `shapes`' keys.
    reprs: HashMap<u64, BRepKind>,
    /// Parent handle id → its edge handles, in canonical
    /// `TopExp::MapShapes(.., TopAbs_EDGE, ..)` order. Makes
    /// [`Self::sub_shapes`] idempotent per (parent, kind): a caller that
    /// re-extracts to recover a sub-shape's canonical index gets the same ids.
    extracted_edges: HashMap<u64, Vec<GeometryHandleId>>,
    /// Parent handle id → its face handles; as `extracted_edges`.
    extracted_faces: HashMap<u64, Vec<GeometryHandleId>>,
    /// Parent handle id → its vertex handles; as `extracted_edges`.
    extracted_vertices: HashMap<u64, Vec<GeometryHandleId>>,
    /// Extracted child handle id → the parent (body) handle it came from.
    /// A handle minted any other way has no entry.
    parent_handle: HashMap<u64, GeometryHandleId>,
    /// The id the next minted handle takes. Only [`Self::mint`] advances it,
    /// by fetch-and-increment, so no id is ever reused.
    next_id: u64,
}

impl ShapeStore {
    pub(crate) fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            reprs: HashMap::new(),
            extracted_edges: HashMap::new(),
            extracted_faces: HashMap::new(),
            extracted_vertices: HashMap::new(),
            parent_handle: HashMap::new(),
            next_id: 1,
        }
    }

    /// Store `shape` under the [`BRepKind`] its `policy` resolves to and mint
    /// its handle. Nothing is stored when the policy cannot name a kind.
    pub(crate) fn insert(
        &mut self,
        shape: UniquePtr<ffi::ffi::OcctShape>,
        policy: ReprPolicy,
    ) -> Result<GeometryHandle, GeometryError> {
        let repr = self.resolve_repr(&shape, policy)?;
        Ok(self.mint(shape, repr))
    }

    fn mint(&mut self, shape: UniquePtr<ffi::ffi::OcctShape>, repr: BRepKind) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, shape);
        self.reprs.insert(id, repr);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: Some(repr),
        }
    }

    /// The [`BRepKind`] `policy` assigns to `shape`.
    ///
    /// An `Inherit` source always has a recorded kind: every caller has
    /// already read the source's shape, and [`Self::mint`] records a shape and
    /// its kind together. A source with no kind is therefore a broken
    /// invariant and an error, never a silent re-classification.
    fn resolve_repr(
        &self,
        shape: &ffi::ffi::OcctShape,
        policy: ReprPolicy,
    ) -> Result<BRepKind, GeometryError> {
        match policy {
            ReprPolicy::Fixed(kind) => Ok(kind),
            ReprPolicy::Inherit(source) => self.repr_of(source).ok_or_else(|| {
                GeometryError::OperationFailed(format!(
                    "ReprPolicy::Inherit source {source:?} has no recorded BRepKind: \
                     the kernel's shapes and reprs are out of lock-step"
                ))
            }),
            ReprPolicy::Classify => brep_kind_of_shape(shape),
        }
    }

    /// Look up a shape by handle ID: the one read path, so every shape the
    /// kernel hands to OCCT has passed its null checks.
    ///
    /// Propagate with `?`; see reify-ir's `From<GeometryError>` impls for the
    /// mapping.
    pub(crate) fn get(&self, id: GeometryHandleId) -> Result<&ffi::ffi::OcctShape, GeometryError> {
        let ptr = self
            .shapes
            .get(&id.0)
            .ok_or(GeometryError::InvalidReference(id))?;
        let shape = ptr.as_ref().ok_or_else(|| {
            GeometryError::OperationFailed(format!("shape handle {id:?} is null"))
        })?;
        // Reject a NON-null wrapper around null (IsNull) topology. Such a shape
        // passes the null-UniquePtr check above yet dereferences to a null
        // TShape, which crashes the unguarded OCCT geometry-query FFI (e.g.
        // query_volume's ShapeType() fallback -> hardware SIGSEGV). Refuse
        // loudly — no silent zero/origin fallback.
        if ffi::ffi::shape_is_null(shape) {
            return Err(GeometryError::OperationFailed(format!(
                "shape handle {:?} wraps null/empty topology (IsNull)",
                id
            )));
        }
        Ok(shape)
    }

    /// The [`BRepKind`] recorded for `id`, or `None` if `id` is not resident.
    pub(crate) fn repr_of(&self, id: GeometryHandleId) -> Option<BRepKind> {
        self.reprs.get(&id.0).copied()
    }

    /// The parent `child` was extracted from, or `None` if `child` was not
    /// minted by [`Self::sub_shapes`].
    pub(crate) fn parent_of(&self, child: GeometryHandleId) -> Option<GeometryHandleId> {
        self.parent_handle.get(&child.0).copied()
    }

    /// Number of resident native shapes.
    pub(crate) fn len(&self) -> usize {
        self.shapes.len()
    }

    fn cache(&mut self, kind: SubShapeKind) -> &mut HashMap<u64, Vec<GeometryHandleId>> {
        match kind {
            SubShapeKind::Edge => &mut self.extracted_edges,
            SubShapeKind::Face => &mut self.extracted_faces,
            SubShapeKind::Vertex => &mut self.extracted_vertices,
        }
    }

    /// Every unique `kind` sub-shape of `parent` in canonical
    /// `TopExp::MapShapes` order, each stamped `kind.repr()` with `parent`
    /// recorded as its owner. Idempotent per (parent, kind): a repeat call
    /// returns the first call's ids.
    pub(crate) fn sub_shapes(
        &mut self,
        parent: GeometryHandleId,
        kind: SubShapeKind,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        if let Some(cached) = self.cache(kind).get(&parent.0) {
            return Ok(cached.clone());
        }
        // Collect the sub-shapes in a scope that ends the shared borrow of
        // `parent`'s shape before minting.
        let materialized: Vec<UniquePtr<ffi::ffi::OcctShape>> = {
            let shape = self.get(parent)?;
            let vec = match kind {
                SubShapeKind::Edge => ffi::ffi::get_edges(shape),
                SubShapeKind::Face => ffi::ffi::get_faces(shape),
                SubShapeKind::Vertex => ffi::ffi::get_vertices(shape),
            }
            .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
            let len = ffi::ffi::shape_vec_len(&vec);
            let mut buf = Vec::with_capacity(len);
            for i in 0..len {
                let sub = ffi::ffi::shape_vec_at(&vec, i)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                buf.push(sub);
            }
            buf
        };

        let mut ids = Vec::with_capacity(materialized.len());
        for sub in materialized {
            let h = self.mint(sub, kind.repr());
            self.parent_handle.insert(h.id.0, parent);
            ids.push(h.id);
        }
        self.cache(kind).insert(parent.0, ids.clone());
        Ok(ids)
    }

    /// The 0-based positions of `selection` in `parent`'s canonical `kind`
    /// enumeration, in selection order; `None` selects every sub-shape.
    ///
    /// [`Self::sub_shapes`], the `get_edges`/`get_faces` getters and the
    /// `OcctShape` edge/face maps share one `TopExp::MapShapes` order, so a
    /// position is exactly the index the kernel-side FFI expects.
    pub(crate) fn indices_of(
        &mut self,
        parent: GeometryHandleId,
        selection: Option<&[GeometryHandleId]>,
        kind: SubShapeKind,
        label: &str,
    ) -> Result<Vec<u32>, GeometryError> {
        let parent_subshapes = self.sub_shapes(parent, kind).map_err(|e| {
            GeometryError::OperationFailed(format!(
                "{label}: failed to enumerate parent {} of {parent:?}: {e:?}",
                kind.plural()
            ))
        })?;
        let Some(selection) = selection else {
            return Ok((0..parent_subshapes.len() as u32).collect());
        };
        let index_of: HashMap<GeometryHandleId, u32> = parent_subshapes
            .iter()
            .enumerate()
            .map(|(i, h)| (*h, i as u32))
            .collect();
        selection
            .iter()
            .map(|h| {
                index_of.get(h).copied().ok_or_else(|| {
                    GeometryError::OperationFailed(format!(
                        "{label}: {} {h:?} does not belong to solid {parent:?}",
                        kind.noun()
                    ))
                })
            })
            .collect()
    }

    /// Free every resident native shape and clear every table derived from
    /// them, keeping `next_id` monotonic.
    ///
    /// The kernel is reused across whole-file reloads, so this is the reload
    /// native-memory bound. `next_id` is never rewound: a handle that outlives
    /// a reload then resolves to a clean `InvalidReference` in [`Self::get`]
    /// instead of aliasing a freshly-minted shape — the read-side counterpart
    /// of INV-BUILD-1 (`engine_build.rs`).
    pub(crate) fn clear(&mut self) {
        // Exhaustive (no `..`): a new field is a compile error here until it
        // is classified as cleared or kept.
        let Self {
            shapes,
            reprs,
            extracted_edges,
            extracted_faces,
            extracted_vertices,
            parent_handle,
            next_id: _,
        } = self;
        shapes.clear();
        reprs.clear();
        extracted_edges.clear();
        extracted_faces.clear();
        extracted_vertices.clear();
        parent_handle.clear();
    }

    /// Inject a null `UniquePtr<OcctShape>` at `id`, bumping `next_id` past
    /// it. No insert path can produce a null `UniquePtr`, so this is the only
    /// way to reach [`Self::get`]'s null-pointer arm.
    #[cfg(test)]
    pub(crate) fn insert_null_for_test(&mut self, id: u64) {
        self.shapes.insert(id, UniquePtr::null());
        if id >= self.next_id {
            self.next_id = id + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_reports_a_null_pointer_as_an_error_not_a_panic() {
        let mut store = ShapeStore::new();
        let box_id = store
            .insert(
                ffi::ffi::make_box(0.01, 0.02, 0.03).expect("make_box"),
                ReprPolicy::Fixed(BRepKind::Solid),
            )
            .expect("a Fixed insert names its kind")
            .id;
        store.insert_null_for_test(42);

        assert!(store.get(box_id).is_ok());

        match store.get(GeometryHandleId(42)) {
            Err(GeometryError::OperationFailed(msg)) => assert!(
                msg.to_lowercase().contains("null"),
                "error should mention null, got: {msg}"
            ),
            Err(other) => panic!("expected OperationFailed, got {other:?}"),
            Ok(_) => panic!("expected error for null shape, got Ok"),
        }

        match store.get(GeometryHandleId(999)) {
            Err(GeometryError::InvalidReference(id)) => assert_eq!(id, GeometryHandleId(999)),
            Err(other) => panic!("expected InvalidReference, got {other:?}"),
            Ok(_) => panic!("expected error for missing shape, got Ok"),
        }
    }

    /// The one mint entry refuses, and stores nothing, when a policy cannot
    /// name a kind: a null shape under `Classify`, or an `Inherit` source with
    /// no recorded kind.
    #[test]
    fn insert_refuses_a_shape_it_cannot_name_a_kind_for() {
        let mut store = ShapeStore::new();
        let unknown_source = GeometryHandleId(999_999);
        let cases = [
            (
                "null shape under Classify",
                ffi::ffi::make_null_shape_for_test(),
                ReprPolicy::Classify,
                "null shape",
            ),
            (
                "Inherit from an unknown source",
                ffi::ffi::make_box(0.01, 0.01, 0.01).expect("make_box"),
                ReprPolicy::Inherit(unknown_source),
                "out of lock-step",
            ),
        ];
        for (case, shape, policy, cause) in cases {
            match store.insert(shape, policy) {
                Err(GeometryError::OperationFailed(msg)) => assert!(
                    msg.contains(cause),
                    "{case}: the error must name its cause {cause:?}, got: {msg}"
                ),
                other => panic!("{case}: expected OperationFailed, got {other:?}"),
            }
            assert_eq!(store.len(), 0, "{case}: nothing may be stored");
        }
    }

    #[test]
    fn brep_kind_decoder_rejects_an_unrecognised_shape_type_name() {
        let known = [
            ("Solid", BRepKind::Solid),
            ("CompSolid", BRepKind::Compound),
            ("Compound", BRepKind::Compound),
            ("Shell", BRepKind::Shell),
            ("Wire", BRepKind::Wire),
            ("Face", BRepKind::Face),
            ("Edge", BRepKind::Edge),
            ("Vertex", BRepKind::Vertex),
        ];
        for (name, kind) in known {
            assert_eq!(
                brep_kind_of_type_name(name).ok(),
                Some(kind),
                "known TopAbs name {name:?}"
            );
        }

        for unknown in ["Shape", "Bogus"] {
            match brep_kind_of_type_name(unknown) {
                Err(GeometryError::OperationFailed(msg)) => assert!(
                    msg.contains(unknown),
                    "error for {unknown:?} must name it, got: {msg}"
                ),
                other => panic!(
                    "unrecognised TopAbs name {unknown:?} must be an \
                     OperationFailed error, not silently classified: {other:?}"
                ),
            }
        }
    }
}
