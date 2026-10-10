//! Feature-activation liveness gate: a registered (provider, feature) pair may
//! be activated only by a crate that names one of the items the provider gates
//! on that feature.

mod fixture;
