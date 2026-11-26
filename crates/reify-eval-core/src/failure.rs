//! The two sorts of failure (contract 9). A `Failure` is an outcome: it
//! follows from the inputs. An `OutsideFailure` does not, so it is never an
//! outcome and nothing stores it.

/// A failure that follows from the inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A read asked for one type and the value read is of another.
    WrongType { expected: &'static str, found: &'static str },
}

/// A failure outside the inputs: a fault of the core's setup or of a kind's
/// own work, reported to the host and stored nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutsideFailure {
    /// A declaration names a kind the core was not built with.
    UnknownKind { kind: &'static str },
}
