//! What the core tells the host, each tagged with the request it belongs to.

use crate::answer::Answer;
use crate::outcome::Erased;
use crate::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(pub(crate) u64);

#[derive(Debug)]
pub enum Event {
    Answered { request: RequestId, path: Path, answer: Answer<Erased> },
}
