//! Tests for the `volume_mesh_cache_key` derivation.
//!
//! The cache key is the identity that the realization-cache layer uses to
//! decide whether two volume-mesh requests can share a result. Per the v0.3
//! FEA PRD (`docs/prds/v0_3/structural-analysis-fea.md` "Parallelism
//! strategy"), the cacheable identity is `(surface_hash, mesh_size,
//! element_order)` — `threads` and `deterministic` are deliberately excluded
//! so thread-count flips and `#deterministic` toggles don't defeat the cache
//! for engineering-equivalent inputs.
//!
//! These tests pin the six key behaviours:
//!   (a) determinism — same inputs ⇒ same key
//!   (b) sensitivity to surface_hash
//!   (c) thread count is excluded from the key
//!   (d) element_order changes the key
//!   (e) `deterministic` flag is excluded from the key (mirrors threads)
//!   (f) `mesh_size: None` and `mesh_size: Some(0.0)` produce DIFFERENT keys
//!       (the presence-flag byte must disambiguate the two — both produce eight
//!       zero payload bytes, so without the flag they would collide silently)

use reify_kernel_gmsh::cache_key::volume_mesh_cache_key;
use reify_kernel_gmsh::options::MeshingOptions;
use reify_core::ContentHash;
use reify_ir::ElementOrderTag;

/// Two calls with byte-identical `(surface_hash, options, element_order)`
/// must return equal `ContentHash`. Without this, the cache never hits.
#[test]
fn same_inputs_produce_same_key() {
    let surface_hash = ContentHash::of_str("surface-mesh-blob-A");
    let options = MeshingOptions {
        mesh_size: Some(0.5),
        threads: Some(4),
        deterministic: false,
    };
    let k1 = volume_mesh_cache_key(surface_hash, &options, ElementOrderTag::P1);
    let k2 = volume_mesh_cache_key(surface_hash, &options, ElementOrderTag::P1);
    assert_eq!(
        k1, k2,
        "identical inputs must produce identical cache keys (deterministic)"
    );
}

/// Changing the surface-mesh content hash must change the cache key — a
/// different surface mesh is a different mesh request.
#[test]
fn different_surface_hash_changes_key() {
    let options = MeshingOptions::default();
    let k_a = volume_mesh_cache_key(
        ContentHash::of_str("surface-mesh-blob-A"),
        &options,
        ElementOrderTag::P1,
    );
    let k_b = volume_mesh_cache_key(
        ContentHash::of_str("surface-mesh-blob-B"),
        &options,
        ElementOrderTag::P1,
    );
    assert_ne!(
        k_a, k_b,
        "different surface meshes must produce different cache keys"
    );
}

/// Thread count is NOT in the cache key. PRD: "Thread count is *not* in the
/// cache key. The result is the same up to floating-point tolerance regardless
/// of thread count; treating bit-different-but-engineering-equivalent solves
/// as cache misses would defeat the cache for no real benefit."
#[test]
fn thread_count_is_excluded_from_key() {
    let surface_hash = ContentHash::of_str("surface-mesh-blob-A");
    let single_threaded = MeshingOptions {
        mesh_size: Some(0.5),
        threads: Some(1),
        deterministic: false,
    };
    let multi_threaded = MeshingOptions {
        mesh_size: Some(0.5),
        threads: Some(8),
        deterministic: false,
    };
    let k_single = volume_mesh_cache_key(surface_hash, &single_threaded, ElementOrderTag::P1);
    let k_multi = volume_mesh_cache_key(surface_hash, &multi_threaded, ElementOrderTag::P1);
    assert_eq!(
        k_single, k_multi,
        "options differing only in thread count must produce identical keys \
         (PRD: same answer to tolerance regardless of thread count)"
    );
}

/// P1 vs P2 element order must change the key — the produced volume mesh
/// has a different per-element node count and is structurally distinct.
#[test]
fn element_order_changes_key() {
    let surface_hash = ContentHash::of_str("surface-mesh-blob-A");
    let options = MeshingOptions::default();
    let k_p1 = volume_mesh_cache_key(surface_hash, &options, ElementOrderTag::P1);
    let k_p2 = volume_mesh_cache_key(surface_hash, &options, ElementOrderTag::P2);
    assert_ne!(
        k_p1, k_p2,
        "P1 and P2 produce structurally distinct meshes; cache keys must differ"
    );
}

/// `deterministic` is NOT in the cache key. Mirrors the thread-count exclusion
/// — flipping `#deterministic` does not change the engineering identity of the
/// mesh request, so the cache must still hit. A regression that started routing
/// `deterministic` through the byte buffer would silently invalidate caches on
/// every flag flip; this test pins the documented PRD invariant against that.
#[test]
fn deterministic_flag_is_excluded_from_key() {
    let surface_hash = ContentHash::of_str("surface-mesh-blob-A");
    let nondeterministic = MeshingOptions {
        mesh_size: Some(0.5),
        threads: Some(4),
        deterministic: false,
    };
    let deterministic = MeshingOptions {
        mesh_size: Some(0.5),
        threads: Some(4),
        deterministic: true,
    };
    let k_nd = volume_mesh_cache_key(surface_hash, &nondeterministic, ElementOrderTag::P1);
    let k_d = volume_mesh_cache_key(surface_hash, &deterministic, ElementOrderTag::P1);
    assert_eq!(
        k_nd, k_d,
        "options differing only in `deterministic` must produce identical keys \
         (PRD: same answer to tolerance regardless of determinism flag)"
    );
}

/// `mesh_size: None` and `mesh_size: Some(0.0)` must produce DIFFERENT keys.
/// Both encode eight zero payload bytes in the value slot, so without the
/// dedicated presence-flag byte (cache_key.rs byte 16) they would collide.
/// A regression that dropped the flag byte would silently produce key
/// equality between auto-sized and explicit-zero requests — this test pins
/// the disambiguation against that.
#[test]
fn mesh_size_none_and_some_zero_produce_different_keys() {
    let surface_hash = ContentHash::of_str("surface-mesh-blob-A");
    let none_size = MeshingOptions {
        mesh_size: None,
        threads: None,
        deterministic: false,
    };
    let some_zero = MeshingOptions {
        mesh_size: Some(0.0),
        threads: None,
        deterministic: false,
    };
    let k_none = volume_mesh_cache_key(surface_hash, &none_size, ElementOrderTag::P1);
    let k_some = volume_mesh_cache_key(surface_hash, &some_zero, ElementOrderTag::P1);
    assert_ne!(
        k_none, k_some,
        "mesh_size: None and Some(0.0) must produce distinct keys; the \
         presence-flag byte (cache_key.rs byte 16) disambiguates the two \
         identical eight-byte payloads"
    );
}
