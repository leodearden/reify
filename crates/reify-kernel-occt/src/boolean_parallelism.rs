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

/// How the calling thread's OCCT boolean algorithms are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanParallelism {
    /// On the calling thread alone.
    Serial,
    /// In OCCT's parallel mode (`SetRunParallel(true)`).
    Parallel,
}

/// Every thread's mode until a scope overrides it. Mirrors
/// `kBooleanRunParallelByDefault` in `cpp/occt_wrapper.cpp`, which is what an
/// OCCT build actually reads; the unit test below keeps the two in step.
#[cfg(any(not(has_occt), test))]
const DEFAULT: BooleanParallelism = BooleanParallelism::Parallel;

/// The calling thread's [`BooleanParallelism`].
#[cfg(has_occt)]
pub fn boolean_parallelism() -> BooleanParallelism {
    if crate::ffi::ffi::boolean_run_parallel() {
        BooleanParallelism::Parallel
    } else {
        BooleanParallelism::Serial
    }
}

/// Stub (OCCT not available): always the default.
#[cfg(not(has_occt))]
pub fn boolean_parallelism() -> BooleanParallelism {
    DEFAULT
}

/// Run `f` with the calling thread's booleans built in `mode`, then restore the
/// thread's prior mode, also when `f` unwinds.
#[cfg(all(has_occt, feature = "test-fixtures"))]
#[doc(hidden)]
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

/// Stub (OCCT not available): there are no booleans to build, so just run `f`.
#[cfg(all(not(has_occt), feature = "test-fixtures"))]
#[doc(hidden)]
pub fn with_boolean_parallelism<R>(_mode: BooleanParallelism, f: impl FnOnce() -> R) -> R {
    f()
}

#[cfg(all(test, has_occt))]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_thread_starts_at_the_rust_side_default() {
        let fresh = std::thread::spawn(boolean_parallelism)
            .join()
            .expect("reader thread must not panic");
        assert_eq!(
            fresh, DEFAULT,
            "the C++ kBooleanRunParallelByDefault and the Rust DEFAULT must agree"
        );
    }
}
