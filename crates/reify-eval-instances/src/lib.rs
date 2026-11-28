//! The instance node kinds of the new evaluation core, and the corpus they
//! are built under: docs/prds/instances-on-the-core.md section 5.1.

mod content;
mod failure;
mod kind_ids;
mod member_kinds;
mod step_map;

pub use failure::InstanceFailure;
pub use member_kinds::MemberKinds;
