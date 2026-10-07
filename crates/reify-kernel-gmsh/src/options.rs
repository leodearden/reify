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
#[derive(Debug, Clone, PartialEq, Default)]
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

impl MeshingOptions {
    /// The `General.NumThreads` every 3D entry point hands gmsh: one worker
    /// when output must be deterministic, otherwise the caller's `threads`,
    /// otherwise the host's available parallelism. `threads` stays a pure
    /// performance hint, outside the cache key.
    pub fn resolved_num_threads(&self) -> u32 {
        if self.deterministic {
            return 1;
        }
        self.threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| u32::try_from(n.get()).unwrap_or(u32::MAX))
                .unwrap_or(1)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MeshingOptions;

    #[test]
    fn deterministic_resolves_to_one_thread_whatever_threads_says() {
        let options = MeshingOptions {
            threads: Some(8),
            deterministic: true,
            ..Default::default()
        };
        assert_eq!(
            options.resolved_num_threads(),
            1,
            "deterministic output requires a single gmsh worker, overriding `threads`",
        );
    }

    #[test]
    fn an_explicit_thread_count_is_honoured_when_not_deterministic() {
        for requested in [3, 0] {
            let options = MeshingOptions {
                threads: Some(requested),
                deterministic: false,
                ..Default::default()
            };
            assert_eq!(
                options.resolved_num_threads(),
                requested,
                "an explicit thread count reaches gmsh unchanged (0 included, which gmsh \
                 itself interprets) when output need not be deterministic",
            );
        }
    }

    #[test]
    fn no_thread_count_resolves_to_at_least_one_thread() {
        let resolved = MeshingOptions::default().resolved_num_threads();
        assert!(
            resolved >= 1,
            "with no explicit count the kernel picks one from the host, and never fewer \
             than one worker; got {resolved}",
        );
    }
}
