//! Task 6667 (η′): attributable-Indeterminate status for GD&T `Conforms` and
//! DFM rules on the **GUI evaluation surface**.
//!
//! Sibling of task 6169 (ζ), which established the attribution channel for
//! `RepresentationWithin`: a `Severity::Info` [`reify_core::Diagnostic`] carrying
//! [`reify_core::DiagnosticCode::ConstraintIndeterminate`] and a reason clause
//! that names the SURFACE and a terminal REMEDY, rather than blaming the model.
//! This module extends that ruling by analogy to the two other kinds the GUI
//! surface leaves unmeasured — and it deliberately reuses ζ's channel rather
//! than inventing a parallel one.
//!
//! ## Why these pins live in the GUI crate
//!
//! The claim under test is about a SURFACE, not about an arbitrary engine, so
//! every test here boots the production GUI configuration —
//! [`crate::engine::EngineSession::with_registered_kernel`], the exact call
//! `main.rs:842` makes (`kernel_status_tests.rs:106` is the in-repo precedent
//! for driving it from a test). A hand-built `Engine::new` would prove nothing
//! about the GUI.
//!
//! Each test asserts `realization_handles_len() == 0` as an explicit SURFACE
//! PRECONDITION. The GUI's production geometry path is
//! `Engine::tessellate_snapshot`, never `build()`/`realize_for_check`, so the
//! handle map stays empty on every production GUI path. Pinning that fact makes
//! these tests fail loudly if a future change starts realizing (the separately
//! chartered on-demand-measurement work would do exactly that) instead of
//! silently passing for the wrong reason.
//!
//! The cheap RED/GREEN engine-level loop lives in
//! `crates/reify-eval/tests/unmeasured_surface_attribution.rs`; THIS module is
//! the contract.
