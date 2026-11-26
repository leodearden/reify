//! What the core tells the host, each tagged with the request it belongs to
//! and the snapshot that request was made on (contract 2.4).

use std::any::Any;
use std::sync::Arc;

use crate::answer::Answer;
use crate::core_id::CoreId;
use crate::outcome::Erased;
use crate::path::Path;
use crate::snapshot::SnapshotId;

/// Which request: the core that opened it, and its place among that
/// core's requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId {
    pub(crate) core: CoreId,
    pub(crate) serial: u64,
}

#[derive(Debug)]
pub enum Event {
    Answered {
        request: RequestId,
        snapshot: SnapshotId,
        path: Path,
        answer: Answer<Erased>,
    },
    /// A note a section sent while it ran, for a path the request needs.
    Progress {
        request: RequestId,
        snapshot: SnapshotId,
        path: Path,
        note: Arc<dyn Any + Send + Sync>,
    },
    /// The core ended the request: it is no longer live, and nothing more
    /// is told to it.
    Ended {
        request: RequestId,
        snapshot: SnapshotId,
        error: RequestError,
    },
}

/// Why the core ended a request. The core ends none yet: a ring of waits,
/// which would leave a request waiting for ever, answers "cycle" at every
/// node of it instead (contract 7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {}
