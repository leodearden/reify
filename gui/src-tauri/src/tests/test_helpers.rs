/// Process-global mutex for serialising tests that mutate
/// `std::env::set_current_dir`.
///
/// Cargo runs all lib tests of a crate in a SINGLE process with multiple
/// threads, so any test file that mutates CWD must lock the SAME mutex as
/// every other CWD-mutating file.  Putting the lock here in a shared module
/// guarantees that — every call site goes through `crate::tests::test_helpers::cwd_lock`,
/// which returns the same `&'static Mutex<()>` instance every time.
use std::sync::{Mutex, OnceLock};

static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Returns the process-global mutex used to serialise CWD-mutating tests.
///
/// Every call returns the SAME `&'static Mutex<()>` — enforced by
/// `OnceLock::get_or_init`.  Tests across different files in this crate all
/// share a single serialisation point when they call `cwd_lock().lock()`.
pub(crate) fn cwd_lock() -> &'static Mutex<()> {
    CWD_LOCK.get_or_init(|| Mutex::new(()))
}
