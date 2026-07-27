//! Panic-safe temporary directories for tests, with forensically attributable
//! names.
//!
//! Test code that builds a directory under [`std::env::temp_dir`] and tears it
//! down with a trailing `fs::remove_dir_all(...)` leaks that directory whenever
//! an assertion fails and the test unwinds — i.e. precisely on RED CI runs.
//! This module supplies the RAII replacement.

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// THE property this module exists to provide: the directory is removed
    /// when the scope holding the guard unwinds, not merely when it returns.
    ///
    /// The path is captured in an outer cell so it outlives the closure the
    /// guard is dropped inside of.
    #[test]
    fn prefixed_tempdir_is_removed_on_panic_unwind() {
        let seen: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let seen_inner = Rc::clone(&seen);

        // Silence the default panic hook across the deliberate `panic!` below so
        // its backtrace does not drown the RED/GREEN signal.  NOTE: the hook is
        // process-global, so a *concurrent* test panicking inside this narrow
        // window would also be silenced — accepted, the window is two syscalls
        // wide and the concurrent test still fails with its own assertion text.
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let guard = prefixed_tempdir("reify-test-support-unwind-");
            *seen_inner.borrow_mut() = Some(guard.path().to_path_buf());
            assert!(
                guard.path().exists(),
                "the guard's directory must exist before we force the panic"
            );
            panic!("forced unwind");
        }));
        std::panic::set_hook(prev_hook);

        assert!(
            result.is_err(),
            "the closure must have unwound — otherwise this test proves nothing"
        );
        let path = seen
            .borrow()
            .clone()
            .expect("the closure recorded the path before panicking");
        assert!(
            !path.exists(),
            "unwinding out of the guard's scope must remove {path:?}; \
             a surviving directory is the leak this module exists to prevent"
        );
    }

    /// The ordinary path: the directory is removed when the guard's scope ends
    /// normally.  Pins that the RAII teardown is not somehow panic-only.
    #[test]
    fn prefixed_tempdir_is_removed_on_normal_scope_exit() {
        let path: PathBuf = {
            let guard = prefixed_tempdir("reify-test-support-scope-");
            let path = guard.path().to_path_buf();
            assert!(path.exists(), "the guard's directory must exist in scope");
            path
        };
        assert!(
            !path.exists(),
            "leaving the guard's scope must remove {path:?}"
        );
    }

    /// Forensic attribution.  `Drop` cannot run on SIGKILL, OOM-kill or power
    /// loss, so residual debris stays possible and must stay ATTRIBUTABLE to the
    /// test that produced it: `find /tmp -maxdepth 1 -name '<prefix>*'` is the
    /// triage tool, and it only works if (a) the directory is a direct child of
    /// [`std::env::temp_dir`] and (b) its name carries the caller's prefix.
    ///
    /// Anonymous `.tmpXXXXXX` names — what plain `tempfile::tempdir()` produces
    /// — would leave the next operator strictly worse off than the manual
    /// teardown this module replaces.
    #[test]
    fn prefixed_tempdir_name_is_attributable_to_its_producer() {
        let prefix = "reify-test-support-attrib-";
        let guard = prefixed_tempdir(prefix);
        let path = guard.path();

        assert_eq!(
            path.parent(),
            Some(std::env::temp_dir().as_path()),
            "the directory must be a DIRECT child of the temp dir so the \
             `find /tmp -maxdepth 1` triage glob reaches it; got {path:?}"
        );

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("temp dir name is valid UTF-8");
        assert!(
            name.starts_with(prefix),
            "the directory name {name:?} must start with the caller's prefix \
             {prefix:?} so surviving debris names the test that made it"
        );
    }
}
