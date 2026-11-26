//! The live requests: each is on one snapshot and stays live until the host
//! ends it.

use std::collections::BTreeMap;

use crate::event::RequestId;
use crate::snapshot::Snapshot;

/// The host's handle on a request, given back to end it.
#[derive(Debug)]
#[must_use = "a request stays live until it is passed to `Core::end`"]
pub struct Request {
    id: RequestId,
}

impl Request {
    pub fn id(&self) -> RequestId {
        self.id
    }
}

#[derive(Default)]
pub(crate) struct Requests {
    live: BTreeMap<RequestId, Snapshot>,
    opened: u64,
}

impl Requests {
    pub fn open(&mut self, snapshot: Snapshot) -> Request {
        self.opened += 1;
        let id = RequestId(self.opened);
        self.live.insert(id, snapshot);
        Request { id }
    }

    pub fn end(&mut self, request: Request) {
        self.live.remove(&request.id);
    }
}
