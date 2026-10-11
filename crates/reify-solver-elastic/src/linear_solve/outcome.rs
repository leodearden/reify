//! How a solve ends when it does not produce a solution.

use std::fmt;

/// A solve that cannot proceed on this system under this policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolveFailure {
    /// An explicit Direct request whose predicted factor exceeds
    /// `SolvePolicy::direct_ceiling`; raised before any numeric allocation.
    FactorExceedsCeiling { predicted: u64, ceiling: u64 },
}

impl fmt::Display for SolveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FactorExceedsCeiling { predicted, ceiling } => write!(
                f,
                "direct factor needs {predicted} bytes, over the {ceiling}-byte direct ceiling"
            ),
        }
    }
}

impl std::error::Error for SolveFailure {}
