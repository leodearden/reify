//! What the core sends a resource's thread, and what every resource thread
//! reports back on the one channel they share.

use std::any::Any;
use std::sync::Arc;

/// What a section returned, erased on its way back to the body.
pub(crate) type Product = Box<dyn Any + Send>;

pub(crate) type SectionRun = Box<dyn FnOnce(&mut dyn Any) -> Product + Send>;

pub(crate) type ReleaseRun = Box<dyn FnOnce(&mut dyn Any) + Send>;

pub(crate) enum Job {
    Section { serial: u64, run: SectionRun },
    Release(ReleaseRun),
    Stop,
}

pub(crate) enum Report {
    Progress { serial: u64, note: Arc<dyn Any + Send + Sync> },
    Finished { serial: u64, product: Result<Product, SectionFault> },
    Released,
}

/// Why a section gave no product.
pub(crate) enum SectionFault {
    Panicked { message: String },
    /// Its resource is dead: an earlier section on it panicked.
    Dead,
}
