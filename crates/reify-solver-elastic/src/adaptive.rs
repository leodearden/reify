//! A-posteriori adaptive refinement loop control + budget enforcement.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md`
//! (Task decomposition #2, task 2997).
//!
//! This module implements the v0.4 a-posteriori outer refinement loop —
//! `solve → estimate → mark → refine → re-solve` — with three budget knobs
//! ("any of these stops it"), Dörfler bulk marking (θ = 0.5), and a
//! `>10%`-stall-drop termination rule, plus the `ConvergenceStatus` /
//! `BudgetReason` termination-reason bookkeeping this task OWNS.
//!
//! # Distinct from `progressive`
//!
//! [`crate::progressive`] (v0.3 task #15) is a DIFFERENT refinement scheme — a
//! `mesh_tol`/`cg_tol` pass schedule with yield-proximity auto-refine, carrying
//! its own `TerminationReason`/`AdvanceDecision` vocabulary. This module is the
//! distinct v0.4 a-posteriori Dörfler + Z-Z + budget + stall model with its own
//! [`ConvergenceStatus`]/[`BudgetReason`] vocabulary (mirroring the DSL enum
//! from task 2998). The two termination models are NOT interchangeable.
//!
//! # Kernel-form primitives; eval threading deferred
//!
//! Following the crate convention ([`crate::error_estimator`],
//! [`crate::volume_refine`], [`crate::progressive`]): this module ships
//! plain-`f64` kernel-form primitives. The `reify_ir::Value::Enum` bridge that
//! maps a Rust [`ConvergenceStatus`] into the DSL enum, and running the loop
//! inside reify-eval's elastic-static compute target, are OUT OF SCOPE here
//! (mirroring the `progressive` → engine-integration split). The Rust enums
//! mirror the DSL variant/payload-field names exactly so the future bridge is
//! mechanical.
