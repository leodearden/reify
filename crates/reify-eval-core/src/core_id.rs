//! Which core made a snapshot or a request, so that a core can refuse one
//! another core made.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CoreId(u64);

impl CoreId {
    /// An id no other core in this process has.
    pub fn fresh() -> Self {
        static MADE: AtomicU64 = AtomicU64::new(0);
        Self(MADE.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Panics unless `made_by` is this core: a snapshot or a request is
    /// good only on the core that made it.
    pub fn refuse_foreign(self, made_by: CoreId, what: &str) {
        if made_by != self {
            panic!("{self} was given {what} made by {made_by}");
        }
    }
}

impl fmt::Display for CoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "core {}", self.0)
    }
}
