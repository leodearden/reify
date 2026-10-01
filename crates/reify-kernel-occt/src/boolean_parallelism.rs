//! Whether OCCT boolean (BOP) algorithms run in OCCT's parallel mode (task 7439).
//!
//! One policy point governs it: `build_bop_algorithm` in `cpp/occt_wrapper.cpp`
//! is the only place a BOP algorithm is `Build()`-ed. That covers every binary
//! boolean and its `*_with_history` sibling, the single-pass `fuse_shape_list`
//! behind `fuse_all` and the pattern realizers, and the `BRepAlgoAPI_Splitter`
//! behind `split`. It calls `SetRunParallel` per algorithm from the calling
//! thread's mode, never through OCCT's process-global default, so the BOPs
//! internal to other OCCT algorithms (offset, fillet, defeaturing) keep OCCT's
//! own default.
//!
//! The mode is PER-THREAD, like the boolean pass counter: every thread starts at
//! the production default, so [`OcctKernelHandle`](crate::OcctKernelHandle)'s
//! dedicated worker always runs the default, and a scoped test override on one
//! thread never leaks into tests running concurrently on others.
//!
//! Serial and parallel builds produce bit-identical topology, sub-shape order
//! and history, which is what persistent naming consumes; proven by
//! `tests/harness_occt/boolean_parallel_determinism.rs`.
//!
//! In parallel mode OCCT runs the BOP loops on the process's single TBB runtime,
//! shared with gmsh's OCCT, OpenVDB and Manifold, so they share one worker pool.
//! Its face/face intersection additionally starts OCCT's own `OSD_ThreadPool`
//! nested inside those TBB jobs, so up to about twice the logical CPU count of
//! threads can be runnable during that phase. Capping that pool is follow-up
//! tkt_0RV9MGP4455Y9389R7EKCDA16C.
//!
//! The production default is stated once, as `kBooleanRunParallelByDefault` in
//! the C++, and production cannot change it. This Rust surface exists for tests
//! only, so `src/lib.rs` compiles it only with OCCT and `test-fixtures`.

/// How the calling thread's OCCT boolean algorithms are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanParallelism {
    /// On the calling thread alone.
    Serial,
    /// In OCCT's parallel mode (`SetRunParallel(true)`).
    Parallel,
}

/// The calling thread's [`BooleanParallelism`].
pub fn boolean_parallelism() -> BooleanParallelism {
    if crate::ffi::ffi::boolean_run_parallel() {
        BooleanParallelism::Parallel
    } else {
        BooleanParallelism::Serial
    }
}

/// Run `f` with the calling thread's booleans built in `mode`, then restore the
/// thread's prior mode, also when `f` unwinds.
pub fn with_boolean_parallelism<R>(mode: BooleanParallelism, f: impl FnOnce() -> R) -> R {
    fn set(mode: BooleanParallelism) {
        crate::ffi::ffi::set_boolean_run_parallel(mode == BooleanParallelism::Parallel);
    }
    struct RestoreOnDrop(BooleanParallelism);
    impl Drop for RestoreOnDrop {
        fn drop(&mut self) {
            set(self.0);
        }
    }

    let _restore = RestoreOnDrop(boolean_parallelism());
    set(mode);
    f()
}

/// How many BOP algorithms the calling thread has built that OCCT itself ran in
/// parallel mode, read back from each algorithm after `Build()` rather than from
/// the requested mode. Lets a test prove its parallel runs really were parallel.
pub fn parallel_bop_build_count() -> u64 {
    crate::ffi::ffi::parallel_bop_build_count()
}
