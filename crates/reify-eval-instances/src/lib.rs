//! The instance node kinds of the new evaluation core, and the corpus they
//! are built under: docs/prds/instances-on-the-core.md section 5.1.

mod content;
mod core_seam;
mod declarations;
mod definition_table;
mod failure;
mod host;
mod instance;
mod kind_ids;
mod member_kinds;
mod member_table;
mod step_map;
#[cfg(test)]
mod test_support;
mod unsupplied;

pub use failure::InstanceFailure;
pub use host::{AnswerFailure, DemandError, Host, HostAnswer, Loaded};
pub use member_kinds::MemberKinds;
