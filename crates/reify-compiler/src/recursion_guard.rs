//! Bounded compiler expression-recursion depth (task #5337).
//!
//! See the `#[cfg(test)] mod tests` below for the behavioural contract; the
//! implementation lands in step-2.

#[cfg(test)]
mod tests {
    use super::*;

    /// The RAII guard's thread-local counter reflects the number of live
    /// guards: nested `enter()` calls increment (1, 2, 3…), dropping a guard
    /// decrements, and once every guard has dropped the counter is back at 0
    /// (so a fresh `enter()` reports depth 1 again).
    #[test]
    fn recursion_depth_guard_counts_live_guards() {
        // (a) nested enters increment 1, 2, 3
        let g1 = RecursionDepthGuard::enter();
        assert_eq!(g1.depth(), 1, "first enter -> depth 1");
        let g2 = RecursionDepthGuard::enter();
        assert_eq!(g2.depth(), 2, "second enter -> depth 2");
        let g3 = RecursionDepthGuard::enter();
        assert_eq!(g3.depth(), 3, "third enter -> depth 3");

        // (b) dropping a guard decrements the thread-local counter: after
        // dropping the innermost guard a fresh enter reports depth 3 again.
        drop(g3);
        let g3b = RecursionDepthGuard::enter();
        assert_eq!(g3b.depth(), 3, "re-enter after one drop -> depth 3 again");
        drop(g3b);
        drop(g2);
        drop(g1);

        // (c) after all guards drop, the counter has returned to 0, so a fresh
        // enter reports depth 1.
        let g = RecursionDepthGuard::enter();
        assert_eq!(
            g.depth(),
            1,
            "counter returned to 0 after all guards dropped"
        );
    }

    /// (d) The depth cap is present and carries generous headroom over
    /// realistic nesting (> 128).
    #[test]
    fn max_compile_recursion_depth_has_headroom() {
        assert!(
            MAX_COMPILE_RECURSION_DEPTH > 128,
            "MAX_COMPILE_RECURSION_DEPTH must exceed realistic-nesting headroom (128), got {}",
            MAX_COMPILE_RECURSION_DEPTH
        );
    }
}
