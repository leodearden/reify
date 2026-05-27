//! Stable cache-key derivation for volume-mesh realization.
//!
//! Computes a [`ContentHash`] from the engineering-equivalent identity of a
//! volume-mesh request: the surface-mesh content hash, the user-tunable
//! `mesh_size` override, and the `element_order` discriminator.
//!
//! # Excluded fields
//!
//! `MeshingOptions::threads` and `MeshingOptions::deterministic` are
//! deliberately NOT in the byte buffer — see `options.rs` for the rationale
//! and the v0.3 FEA PRD (`docs/prds/v0_3/structural-analysis-fea.md`,
//! "Parallelism strategy") for the source-of-truth statement: "Thread count
//! is *not* in the cache key. The result is the same up to floating-point
//! tolerance regardless of thread count; treating bit-different-but-
//! engineering-equivalent solves as cache misses would defeat the cache for
//! no real benefit."
//!
//! # Byte layout
//!
//! The serialised buffer is fixed-shape and version-implicit:
//!
//! ```text
//! [ surface_hash u128 LE         ] (16 bytes)
//! [ mesh_size present-flag u8    ] (1 byte: 0 = None, 1 = Some)
//! [ mesh_size value f64 LE       ] (8 bytes; zeros when None)
//! [ element_order u8             ] (1 byte: 0 = P1, 1 = P2)
//! ```
//!
//! Total: 26 bytes, fed to `ContentHash::of`. The layout is documented inline
//! in the implementation. Future extension: when a new field is added to the
//! cacheable identity, append it after the existing bytes — never re-order
//! existing fields, since persistent caches that key on this hash would
//! silently invalidate.

use reify_core::ContentHash;
use reify_ir::ElementOrderTag;

use crate::options::MeshingOptions;

/// Compute the stable cache key for a volume-mesh request.
///
/// `surface_hash` is the upstream surface-mesh content hash (already a
/// [`ContentHash`]); `options` carries the user-tunable knobs (only
/// `mesh_size` contributes to the key — see module docs); `element_order`
/// distinguishes P1 from P2 meshes.
pub fn volume_mesh_cache_key(
    surface_hash: ContentHash,
    options: &MeshingOptions,
    element_order: ElementOrderTag,
) -> ContentHash {
    let mut buf = [0u8; 26];

    // [0..16): surface_hash u128 LE
    buf[..16].copy_from_slice(&surface_hash.0.to_le_bytes());

    // [16]: mesh_size presence flag.
    // [17..25): mesh_size value f64 LE (zeros when None — the flag byte
    //          disambiguates Some(0.0) from None even though both produce
    //          the same eight payload bytes).
    if let Some(size) = options.mesh_size {
        buf[16] = 1;
        buf[17..25].copy_from_slice(&size.to_le_bytes());
    } else {
        buf[16] = 0;
        // bytes [17..25) stay zero — explicit for clarity.
    }

    // [25]: element_order discriminant (0 = P1, 1 = P2).
    buf[25] = match element_order {
        ElementOrderTag::P1 => 0,
        ElementOrderTag::P2 => 1,
    };

    ContentHash::of(&buf)
}
