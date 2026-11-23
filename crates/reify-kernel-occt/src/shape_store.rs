//! The OCCT kernel's shape store: every resident native shape, stored together
//! with its [`BRepKind`] as one entry, the per-kind sub-shape extraction caches,
//! and the child → parent provenance map. Handles are minted in one place
//! ([`ShapeStore::insert`]) and read through one chokepoint ([`ShapeStore::get`]).

use std::collections::HashMap;

use cxx::UniquePtr;
use reify_ir::{BRepKind, GeometryError, GeometryHandle, GeometryHandleId, QueryError};

use crate::ffi;
use crate::ffi::ffi::FfiShapeKind;

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
/// [`BRepKind::Solid`].
///
/// Only the kind is unwrapped, not the stored shape: a fillet, chamfer, draft
/// or thicken result is still OCCT's COMPOUND wrapper, so C++ code branching
/// on `ShapeType()` sees COMPOUND where `repr_of` says Solid.
pub(crate) fn brep_kind_of_shape(shape: &ffi::ffi::OcctShape) -> Result<BRepKind, GeometryError> {
    let kind = ffi::ffi::body_shape_kind(shape)
        .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
    BRepKind::try_from(kind)
}

impl TryFrom<FfiShapeKind> for BRepKind {
    type Error = GeometryError;

    /// A CompSolid is a [`BRepKind::Compound`]. A repr outside the declared
    /// variants is an error, never a default kind: cxx shared enums are open
    /// `u8` newtypes, so the match cannot be exhaustive without that arm.
    fn try_from(kind: FfiShapeKind) -> Result<Self, Self::Error> {
        Ok(match kind {
            FfiShapeKind::Solid => BRepKind::Solid,
            FfiShapeKind::CompSolid | FfiShapeKind::Compound => BRepKind::Compound,
            FfiShapeKind::Shell => BRepKind::Shell,
            FfiShapeKind::Wire => BRepKind::Wire,
            FfiShapeKind::Face => BRepKind::Face,
            FfiShapeKind::Edge => BRepKind::Edge,
            FfiShapeKind::Vertex => BRepKind::Vertex,
            FfiShapeKind { repr } => {
                return Err(GeometryError::OperationFailed(format!(
                    "FfiShapeKind repr {repr} is not a declared FfiShapeKind variant"
                )));
            }
        })
    }
}

/// A resident native shape and the [`BRepKind`] it was stored under.
struct Entry {
    shape: UniquePtr<ffi::ffi::OcctShape>,
    repr: BRepKind,
}

pub(crate) struct ShapeStore {
    /// Every resident shape with its kind, by handle id: a shape cannot be
    /// resident without one.
    entries: HashMap<u64, Entry>,
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
            entries: HashMap::new(),
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
        self.entries.insert(id, Entry { shape, repr });
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: Some(repr),
        }
    }

    /// The [`BRepKind`] `policy` assigns to `shape`; an `Inherit` source that
    /// is not resident is `InvalidReference`.
    fn resolve_repr(
        &self,
        shape: &ffi::ffi::OcctShape,
        policy: ReprPolicy,
    ) -> Result<BRepKind, GeometryError> {
        match policy {
            ReprPolicy::Fixed(kind) => Ok(kind),
            ReprPolicy::Inherit(source) => self.repr_of(source),
            ReprPolicy::Classify => brep_kind_of_shape(shape),
        }
    }

    /// Look up a shape by handle ID: the one read path, so every shape the
    /// kernel hands to OCCT has passed its null checks.
    ///
    /// Propagate with `?`; see reify-ir's `From<GeometryError>` impls for the
    /// mapping.
    pub(crate) fn get(&self, id: GeometryHandleId) -> Result<&ffi::ffi::OcctShape, GeometryError> {
        let entry = self
            .entries
            .get(&id.0)
            .ok_or(GeometryError::InvalidReference(id))?;
        let shape = entry.shape.as_ref().ok_or_else(|| {
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

    /// The [`BRepKind`] stored with `id`; `InvalidReference` if `id` is not
    /// resident.
    pub(crate) fn repr_of(&self, id: GeometryHandleId) -> Result<BRepKind, GeometryError> {
        self.entries
            .get(&id.0)
            .map(|entry| entry.repr)
            .ok_or(GeometryError::InvalidReference(id))
    }

    /// The parent `child` was extracted from, or `None` if `child` was not
    /// minted by [`Self::sub_shapes`].
    pub(crate) fn parent_of(&self, child: GeometryHandleId) -> Option<GeometryHandleId> {
        self.parent_handle.get(&child.0).copied()
    }

    /// Number of resident native shapes.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
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
            entries,
            extracted_edges,
            extracted_faces,
            extracted_vertices,
            parent_handle,
            next_id: _,
        } = self;
        entries.clear();
        extracted_edges.clear();
        extracted_faces.clear();
        extracted_vertices.clear();
        parent_handle.clear();
    }

    /// Inject a null `UniquePtr<OcctShape>` stored as `repr` at `id`, bumping
    /// `next_id` past it. No insert path can produce a null `UniquePtr`, so
    /// this is the only way to reach [`Self::get`]'s null-pointer arm.
    #[cfg(test)]
    pub(crate) fn insert_null_for_test(&mut self, id: u64, repr: BRepKind) {
        self.entries.insert(
            id,
            Entry {
                shape: UniquePtr::null(),
                repr,
            },
        );
        if id >= self.next_id {
            self.next_id = id + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::ffi::FfiShapeKind;

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
        store.insert_null_for_test(42, BRepKind::Solid);

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
    /// name a kind: a null shape under `Classify`, or an `Inherit` source that
    /// is not resident.
    #[test]
    fn insert_refuses_a_shape_it_cannot_name_a_kind_for() {
        const UNKNOWN_SOURCE: GeometryHandleId = GeometryHandleId(999_999);
        type ErrorMatcher = fn(&GeometryError) -> bool;
        fn names_null_shape(err: &GeometryError) -> bool {
            matches!(err, GeometryError::OperationFailed(msg) if msg.contains("null shape"))
        }
        fn is_unknown_source(err: &GeometryError) -> bool {
            matches!(err, GeometryError::InvalidReference(id) if *id == UNKNOWN_SOURCE)
        }
        let mut store = ShapeStore::new();
        let cases: [(&str, _, _, ErrorMatcher); 2] = [
            (
                "null shape under Classify",
                ffi::ffi::make_null_shape_for_test(),
                ReprPolicy::Classify,
                names_null_shape,
            ),
            (
                "Inherit from an unknown source",
                ffi::ffi::make_box(0.01, 0.01, 0.01).expect("make_box"),
                ReprPolicy::Inherit(UNKNOWN_SOURCE),
                is_unknown_source,
            ),
        ];
        for (case, shape, policy, matches_expected) in cases {
            match store.insert(shape, policy) {
                Err(err) => assert!(matches_expected(&err), "{case}: unexpected error {err:?}"),
                Ok(handle) => panic!("{case}: expected an error, got {handle:?}"),
            }
            assert_eq!(store.len(), 0, "{case}: nothing may be stored");
        }
    }

    /// The source box is stored as `Shell`, a kind no box classifies as, so an
    /// `Inherit` that classifies instead of copying, or a `repr_of` that
    /// returns `Solid` without reading the entry, fails here.
    #[test]
    fn repr_of_reads_the_kind_stored_with_the_shape() {
        let mut store = ShapeStore::new();
        let first_id = store
            .insert(
                ffi::ffi::make_box(0.01, 0.02, 0.03).expect("make_box"),
                ReprPolicy::Fixed(BRepKind::Shell),
            )
            .expect("a Fixed insert names its kind")
            .id;
        let inherited_id = store
            .insert(
                ffi::ffi::make_box(0.01, 0.01, 0.01).expect("make_box"),
                ReprPolicy::Inherit(first_id),
            )
            .expect("a resident source names its kind")
            .id;
        for (case, id) in [("Fixed(Shell)", first_id), ("Inherit(first)", inherited_id)] {
            let got = store.repr_of(id);
            assert!(
                matches!(got, Ok(BRepKind::Shell)),
                "{case}: expected Ok(Shell), got {got:?}"
            );
        }

        let unknown = GeometryHandleId(999_999);
        let got = store.repr_of(unknown);
        assert!(
            matches!(got, Err(GeometryError::InvalidReference(id)) if id == unknown),
            "an unknown id: expected InvalidReference({unknown:?}), got {got:?}"
        );

        store.clear();
        let got = store.repr_of(first_id);
        assert!(
            matches!(got, Err(GeometryError::InvalidReference(id)) if id == first_id),
            "a cleared id: expected InvalidReference({first_id:?}), got {got:?}"
        );
    }

    /// Every `TopAbs_ShapeEnum` value, through the one C++ switch and the
    /// `TryFrom` decode: each concrete type names its kind, and the abstract
    /// `TopAbs_SHAPE` is refused as a contract violation, never a kind.
    #[test]
    fn every_topabs_value_decodes_to_its_brep_kind_and_the_abstract_one_is_refused() {
        // OCCT 7.8 TopAbs_ShapeEnum.hxx order.
        let concrete = [
            (0, "TopAbs_COMPOUND", BRepKind::Compound),
            (1, "TopAbs_COMPSOLID", BRepKind::Compound),
            (2, "TopAbs_SOLID", BRepKind::Solid),
            (3, "TopAbs_SHELL", BRepKind::Shell),
            (4, "TopAbs_FACE", BRepKind::Face),
            (5, "TopAbs_WIRE", BRepKind::Wire),
            (6, "TopAbs_EDGE", BRepKind::Edge),
            (7, "TopAbs_VERTEX", BRepKind::Vertex),
        ];
        let mut mismatches = Vec::new();
        for (ordinal, name, expected) in concrete {
            let decoded = ffi::ffi::shape_kind_of_topabs_for_test(ordinal)
                .map_err(|e| e.to_string())
                .and_then(|kind| BRepKind::try_from(kind).map_err(|e| format!("{e:?}")));
            if decoded.as_ref() != Ok(&expected) {
                mismatches.push(format!(
                    "{name} ({ordinal}): expected {expected:?}, got {decoded:?}"
                ));
            }
        }

        match ffi::ffi::shape_kind_of_topabs_for_test(8) {
            Err(e) => {
                let msg = e.to_string();
                if !(msg.starts_with("shape_kind_of_topabs_for_test: ")
                    && msg.contains("TopAbs_SHAPE"))
                {
                    mismatches.push(format!(
                        "TopAbs_SHAPE (8): expected a ContractViolation naming TopAbs_SHAPE, got: {msg}"
                    ));
                }
            }
            Ok(kind) => mismatches.push(format!(
                "TopAbs_SHAPE (8): expected an error, got kind {kind:?}"
            )),
        }

        if let Ok(kind) = ffi::ffi::shape_kind_of_topabs_for_test(9) {
            mismatches.push(format!(
                "ordinal 9 is no TopAbs value: expected an error, got kind {kind:?}"
            ));
        }

        assert!(
            mismatches.is_empty(),
            "TopAbs decode mismatches:\n{}",
            mismatches.join("\n")
        );
    }

    #[test]
    fn an_undeclared_ffi_shape_kind_is_an_error_naming_its_repr() {
        match BRepKind::try_from(FfiShapeKind { repr: u8::MAX }) {
            Err(GeometryError::OperationFailed(msg)) => assert!(
                msg.contains("255"),
                "the error must name the raw repr 255, got: {msg}"
            ),
            other => panic!("an undeclared FfiShapeKind must be OperationFailed, got {other:?}"),
        }
    }

    #[test]
    fn brep_kind_of_shape_classifies_the_unwrapped_body() {
        let compound_of = |shapes: &[&ffi::ffi::OcctShape]| {
            let mut vec = ffi::ffi::new_shape_vec();
            for shape in shapes {
                ffi::ffi::shape_vec_push(vec.pin_mut(), shape);
            }
            ffi::ffi::make_compound(&vec).expect("make_compound")
        };
        let first = ffi::ffi::make_box(0.01, 0.01, 0.01).expect("make_box");
        let second = ffi::ffi::translate_shape(&first, 0.1, 0.0, 0.0).expect("translate_shape");

        let one_box = compound_of(&[&first]);
        let two_boxes = compound_of(&[&first, &second]);
        let cases: [(&str, &ffi::ffi::OcctShape, BRepKind); 3] = [
            ("a bare box", &first, BRepKind::Solid),
            ("a compound of one box", &one_box, BRepKind::Solid),
            (
                "a compound of two disjoint boxes",
                &two_boxes,
                BRepKind::Compound,
            ),
        ];
        let mismatches: Vec<String> = cases
            .into_iter()
            .filter_map(|(case, shape, expected)| {
                let got = brep_kind_of_shape(shape);
                (got.as_ref().ok() != Some(&expected))
                    .then(|| format!("{case}: expected Ok({expected:?}), got {got:?}"))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "body classification mismatches:\n{}",
            mismatches.join("\n")
        );
    }
}
