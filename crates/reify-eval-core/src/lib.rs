//! The evaluation core: runs node bodies against snapshots of inputs, as
//! Part A of docs/evaluation-core-contract.md specifies.

mod answer;
mod builder;
mod counters;
mod declaration;
mod event;
mod executor;
mod failure;
mod hash;
mod host;
mod input;
mod outcome;
mod path;
mod requests;
mod snapshot;

pub use answer::Answer;
pub use builder::Builder;
pub use counters::Counters;
pub use declaration::{Binding, Declaration, KindId};
pub use event::{Event, RequestId};
pub use executor::Turn;
pub use failure::{Failure, OutsideFailure};
pub use hash::Hash128;
pub use host::Core;
pub use input::{Input, InputKey};
pub use outcome::{Erased, Outcome, Value};
pub use path::{Path, Root};
pub use requests::Request;
pub use snapshot::Snapshot;
