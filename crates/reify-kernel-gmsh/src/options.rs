//! `MeshingOptions` — user-tunable knobs for the volume-mesh pipeline.
//!
//! Translated from the user-facing `ElasticOptions` fields (sibling task
//! #2911, see `crates/reify-compiler/stdlib/solver_elastic.ri`) into the
//! mesher's internal config. The fields here are the engineering-equivalent
//! identity inputs to a mesh request: a different `mesh_size` produces a
//! different mesh, a different `threads` count does NOT (see
//! `cache_key.rs` for the cache-key composition).
//!
//! `Hash` is intentionally NOT derived — `f64` doesn't impl `Hash`. The
//! cache-key derivation in `cache_key.rs` hashes via byte serialization
//! instead, so we can use a fixed deterministic byte layout.

/// User-tunable knobs for a single volume-mesh request.
///
/// All fields are optional except `deterministic`; the mesher fills defaults
/// from the auto-size and config layers when a field is `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshingOptions {
    /// Target characteristic mesh edge length (millimetres). When `None`,
    /// the mesher derives a default from the smallest geometric feature
    /// (see `auto_size.rs`).
    pub mesh_size: Option<f64>,
    /// Worker-thread count for parallel volume meshing (`gmshOptionSetNumber
    /// "General.NumThreads"`). `None` lets the kernel decide. **Not part of
    /// the cache key** — same answer to tolerance regardless of thread count.
    pub threads: Option<u32>,
    /// Whether the user requested bit-deterministic mesh output (`#deterministic`
    /// pragma, sibling task #2926). Plumbed through but **not part of the cache
    /// key** — under `#deterministic` the cache returns bit-identical bytes from
    /// a prior cold-start mesh regardless of how that mesh was originally
    /// produced; treating the flag as part of the key would force re-meshing
    /// on every flag flip and defeat the cross-machine reproducibility purpose.
    pub deterministic: bool,
}

impl Default for MeshingOptions {
    fn default() -> Self {
        Self {
            mesh_size: None,
            threads: None,
            deterministic: false,
        }
    }
}
