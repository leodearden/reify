//! The evaluation core driven through its public interface, as a host and
//! as a kind author see it (docs/evaluation-core-contract.md, Part A).
//! Layout: toy.rs holds what several tests share (the toy kinds, the toy
//! kernel, host-side helpers); each other module holds the tests of one
//! subject, with any kind only one of its tests uses.

mod belonging;
mod bodies;
mod building;
mod ending;
mod hash;
mod reading;
mod roots;
mod sections;
mod toy;
