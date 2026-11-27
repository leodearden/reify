//! The evaluation core driven through its public interface, as a host and
//! as a kind author see it (docs/evaluation-core-contract.md, Part A).
//! Layout: toy.rs holds what several tests share (the toy kinds, the toy
//! kernel, host-side helpers); each other module holds the tests of one
//! subject, with any kind only one of its tests uses.

mod belonging;
mod bindings;
mod bodies;
mod budgets;
mod building;
mod contents;
mod cycles;
mod ending;
mod failures;
mod growth;
mod hash;
mod limits;
mod must_not_compile;
mod outside;
mod outside_kinds;
mod reading;
mod roots;
mod sections;
mod seeding;
mod sequences;
mod snapshots;
mod store;
mod structure;
mod t_model;
mod toy;
mod two_places;
