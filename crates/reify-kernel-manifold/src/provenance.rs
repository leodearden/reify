//! MeshGL64 provenance walk for Manifold boolean results.
//!
//! After a Manifold boolean, `to_meshgl64()` exposes per-run provenance
//! (`run_original_id`, `run_index`) and per-triangle face identity
//! (`face_id`) that links each surviving triangle back to a parent input.
//! This module walks those vectors to produce a [`FacetProvenance`] entry
//! for every triangle, correlating each with its source [`TopologyAttribute`]
//! from the parent table.
//!
//! # Design decisions
//!
//! - Output is `Vec<FacetProvenance>` keyed by a stable `FacetDescriptor`
//!   rather than minted `GeometryHandleId`s (which are non-deterministic).
//!   Task 4262 will add the descriptor-keyed store; this module's output is
//!   forward-compatible with that interface.
//! - `correlate_from_vectors` is a pure function testable with synthetic
//!   vectors, beneath `correlate_facets` which extracts vectors from the FFI.
//! - Unmapped `run_original_id` values yield `source: None` — a
//!   boolean result may legitimately contain runs from a parent that carried
//!   no attribute (lossy-but-valid). This is not a contract violation.
//! - The merge vectors are consumed only for structural pairing validation
//!   (`merge_from_vert.len() == merge_to_vert.len()`). Per-vertex merge
//!   resolution and per-planar-face identity are task 4262's scope.

use std::collections::HashMap;

use reify_ir::TopologyAttribute;

/// Stable facet descriptor that identifies a result triangle by its Manifold
/// provenance coordinates.
///
/// Forward-compatible with task 4262's descriptor-keyed attribute store.
/// `run_original_id` matches the `Manifold::original_id()` of one of the
/// parent inputs; `face_id` is the per-triangle face identifier from
/// `MeshGL64::face_id()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FacetDescriptor {
    /// The `run_original_id` of the run containing this triangle — links
    /// back to a specific parent `Manifold` input via its `original_id()`.
    pub run_original_id: u32,
    /// Per-triangle face identifier from `MeshGL64::face_id()`.
    pub face_id: u64,
}

/// Provenance record for one surviving triangle in a Manifold boolean result.
///
/// Produced by [`correlate_facets`] (one entry per triangle in the result
/// mesh). The `source` field resolves to `None` when the run's
/// `run_original_id` has no entry in the parent attribute map — a valid
/// outcome when a parent carried no `TopologyAttribute`.
#[derive(Debug, Clone)]
pub struct FacetProvenance {
    /// Zero-based triangle index in the result mesh.
    pub triangle: usize,
    /// Stable descriptor (run provenance + face id) for this triangle.
    pub descriptor: FacetDescriptor,
    /// The topology attribute from the parent input that contributed this
    /// triangle, or `None` if the parent was untracked.
    pub source: Option<TopologyAttribute>,
}

/// Walk the `MeshGL64` provenance vectors to correlate each surviving
/// triangle with its source attribute.
///
/// Extracts `num_tri`, `run_index`, `run_original_id`, `face_id`,
/// `merge_from_vert`, and `merge_to_vert` from `meshgl`, then delegates
/// to [`correlate_from_vectors`].
///
/// Returns an `Err(String)` if the provenance vectors fail structural
/// validation (see [`correlate_from_vectors`] for the contract).
pub fn correlate_facets(
    meshgl: &manifold3d::MeshGL64,
    parent: &HashMap<u32, TopologyAttribute>,
) -> Result<Vec<FacetProvenance>, String> {
    let num_tri = meshgl.num_tri();
    let run_index = meshgl.run_index();
    let run_original_id = meshgl.run_original_id();
    let face_id = meshgl.face_id();
    let merge_from_vert = meshgl.merge_from_vert();
    let merge_to_vert = meshgl.merge_to_vert();
    correlate_from_vectors(
        num_tri,
        &run_index,
        &run_original_id,
        &face_id,
        &merge_from_vert,
        &merge_to_vert,
        parent,
    )
}

/// Core provenance walk over raw MeshGL64 vectors.
///
/// Validates the structural contract of the provenance vectors, then
/// for each run `r` maps triangles `run_index[r]/3 .. run_index[r+1]/3`
/// to a [`FacetProvenance`] carrying the run's `run_original_id`, the
/// triangle's `face_id`, and the source attribute resolved from `parent`.
///
/// # Contract (all must hold; violators return `Err`)
///
/// - `run_index.len() == run_original_id.len() + 1`
/// - `face_id.len() == num_tri`
/// - Every `run_index` entry is divisible by 3
/// - `run_index` is non-decreasing with `run_index[last] == num_tri * 3`
/// - `merge_from_vert.len() == merge_to_vert.len()`
fn correlate_from_vectors(
    _num_tri: usize,
    _run_index: &[u64],
    _run_original_id: &[u32],
    _face_id: &[u64],
    _merge_from_vert: &[u64],
    _merge_to_vert: &[u64],
    _parent: &HashMap<u32, TopologyAttribute>,
) -> Result<Vec<FacetProvenance>, String> {
    // Placeholder: walk not yet implemented.
    Err("provenance walk unimplemented".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{FeatureId, Role};

    fn make_attr(feature_name: &str) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: FeatureId::new(feature_name),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: vec![],
        }
    }

    /// Verifies that `correlate_from_vectors` partitions triangles by run and
    /// resolves source attributes from the parent map (or yields `None` for
    /// unmapped original ids).
    ///
    /// Synthetic well-formed vectors:
    ///   num_tri = 4
    ///   run_index = [0, 6, 12]   (flat tri-vert units: ×3 per triangle)
    ///     → run 0 covers tris 0..2 (run_index[0]/3=0 .. run_index[1]/3=2)
    ///     → run 1 covers tris 2..4 (run_index[1]/3=2 .. run_index[2]/3=4)
    ///   run_original_id = [10, 20]
    ///   face_id = [100, 100, 200, 200]
    ///   merge_from_vert = [] (empty — structurally valid)
    ///   merge_to_vert   = [] (empty — structurally valid)
    ///   parent = {10 → attrA} — id 20 deliberately unmapped
    ///
    /// Expected: Ok with 4 FacetProvenance entries.
    ///   facets[0],[1]: descriptor{run_original_id:10, face_id:100}, source==Some(attrA)
    ///   facets[2],[3]: descriptor{run_original_id:20, face_id:200}, source==None
    ///   triangle indices 0, 1, 2, 3 in order
    #[test]
    fn correlate_from_vectors_partitions_runs_and_resolves_sources() {
        let attr_a = make_attr("A");
        let mut parent: HashMap<u32, TopologyAttribute> = HashMap::new();
        parent.insert(10u32, attr_a.clone());
        // id 20 is deliberately NOT inserted

        let num_tri: usize = 4;
        // run_index in flat tri-vert units: 0, 6 (=2*3), 12 (=4*3)
        let run_index: &[u64] = &[0, 6, 12];
        let run_original_id: &[u32] = &[10, 20];
        let face_id: &[u64] = &[100, 100, 200, 200];
        let merge_from_vert: &[u64] = &[];
        let merge_to_vert: &[u64] = &[];

        let result = correlate_from_vectors(
            num_tri,
            run_index,
            run_original_id,
            face_id,
            merge_from_vert,
            merge_to_vert,
            &parent,
        );

        let facets = result.expect("well-formed vectors must produce Ok");
        assert_eq!(facets.len(), 4, "must produce one FacetProvenance per triangle");

        // Facets 0 and 1: run 0 (original_id 10, mapped → Some(attrA))
        assert_eq!(facets[0].triangle, 0);
        assert_eq!(
            facets[0].descriptor,
            FacetDescriptor { run_original_id: 10, face_id: 100 }
        );
        assert_eq!(
            facets[0].source.as_ref(),
            Some(&attr_a),
            "facet 0 source must be Some(attrA) — id 10 is in parent map"
        );

        assert_eq!(facets[1].triangle, 1);
        assert_eq!(
            facets[1].descriptor,
            FacetDescriptor { run_original_id: 10, face_id: 100 }
        );
        assert_eq!(
            facets[1].source.as_ref(),
            Some(&attr_a),
            "facet 1 source must be Some(attrA)"
        );

        // Facets 2 and 3: run 1 (original_id 20, not in map → None)
        assert_eq!(facets[2].triangle, 2);
        assert_eq!(
            facets[2].descriptor,
            FacetDescriptor { run_original_id: 20, face_id: 200 }
        );
        assert!(
            facets[2].source.is_none(),
            "facet 2 source must be None — id 20 is not in parent map"
        );

        assert_eq!(facets[3].triangle, 3);
        assert_eq!(
            facets[3].descriptor,
            FacetDescriptor { run_original_id: 20, face_id: 200 }
        );
        assert!(
            facets[3].source.is_none(),
            "facet 3 source must be None — id 20 is not in parent map"
        );
    }
}
