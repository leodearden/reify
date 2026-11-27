//! What a request may spend, given by the host with the request (contract
//! 7.6): a machine or session budget, never part of a snapshot.

/// How much work the core may do for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    paths: Option<u64>,
}

impl Budget {
    /// No bound on the work done for the request.
    pub fn unlimited() -> Self {
        Self { paths: None }
    }
}
