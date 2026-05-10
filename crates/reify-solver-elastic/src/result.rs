//! Per-element stress and nodal-stress gradient recovery for tetrahedral
//! P1 FEA.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #13.
//!
//! # Scope
//!
//! P1-only stress recovery for v0.3. The engine integration layer
//! (PRD §16) wraps the recovered nodal field as
//! `Field<Point3<Length>, Tensor<2,3,Pressure>>`; this crate ships the
//! Rust math primitives in plain `f64` types, mirroring the pattern in
//! `shell_result.rs` for shells.
//!
//! # Public surface
//!
//! - [`element_stress_p1`] — per-element constant Cauchy stress
//!   `σ_e = D · B · u_e` returned as a 3×3 symmetric tensor (Voigt is
//!   internal to the multiplication).
//! - [`tet_volume_p1`] — `|det J| / 6` from the affine map.
//! - [`recover_nodal_stress_p1`] + [`StressElement`] — volume-weighted
//!   averaging across incident elements, producing a continuous nodal
//!   stress field interpolatable via the same P1 shape functions.
