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

    /// At most `paths` paths resolved for the request.
    pub fn paths(paths: u64) -> Self {
        Self { paths: Some(paths) }
    }
}

/// What a live request has spent of its budget.
pub(crate) struct Allowance {
    budget: Budget,
    paths_resolved: u64,
}

impl Allowance {
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            paths_resolved: 0,
        }
    }

    /// Whether the budget bounds anything.
    pub fn is_bounded(&self) -> bool {
        self.budget.paths.is_some()
    }

    /// Spends one path resolved, or, if the budget allows no more, gives
    /// the budget back: a budget of n allows n resolutions.
    pub fn spend_path(&mut self) -> Result<(), u64> {
        match self.budget.paths {
            Some(budget) if self.paths_resolved >= budget => Err(budget),
            _ => {
                self.paths_resolved += 1;
                Ok(())
            }
        }
    }
}
