//! The evaluation core driven through its public interface, as a host and
//! as a kind author see it (docs/evaluation-core-contract.md, Part A).
//! Layout: toy.rs holds what several tests share (the toy kinds, the toy
//! kernel, host-side helpers); each other module holds the tests of one
//! subject, with any kind only one of its tests uses.

mod belonging;
mod bindings;
mod bodies;
mod building;
mod ending;
mod growth;
mod hash;
mod outside;
mod reading;
mod roots;
mod sections;
mod snapshots;
mod structure;
mod toy;
mod waiting;
