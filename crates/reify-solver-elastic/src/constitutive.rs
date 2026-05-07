//! Constitutive laws for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the isotropic linear-elastic 6×6 D-matrix used by element-stiffness
//! assembly. The Voigt component order is `[εxx, εyy, εzz, γxy, γyz, γxz]`
//! with **engineering shear strain** (`γ = 2ε`); see [`IsotropicElastic`] for
//! the convention details.
