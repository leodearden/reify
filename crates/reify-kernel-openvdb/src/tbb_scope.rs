//! `volumeToMesh` partitions its work by TBB concurrency, so run-twice
//! determinism tests (implicit-solids D-13 / B17) tessellate inside a
//! [`TbbSingleThreadScope`].

use crate::ffi::ffi as openvdb_ffi;

/// Holds TBB to a single thread until dropped. The limit is process-global,
/// so tests that observe it must serialize against each other.
pub struct TbbSingleThreadScope {
    _limit: cxx::UniquePtr<openvdb_ffi::TbbParallelismLimit>,
}

impl TbbSingleThreadScope {
    #[must_use]
    pub fn enter() -> Self {
        Self {
            _limit: openvdb_ffi::tbb_single_thread_limit_ffi(),
        }
    }
}

/// TBB's currently active `max_allowed_parallelism`.
pub fn tbb_max_allowed_parallelism() -> usize {
    openvdb_ffi::tbb_max_allowed_parallelism_ffi()
}
