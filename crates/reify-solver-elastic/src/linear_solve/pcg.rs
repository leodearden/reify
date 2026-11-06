//! The interim iterative tier: Jacobi-preconditioned CG on the crate's one
//! CG loop (`solver::jacobi_pcg_seq`), each iteration reported through the
//! control, every ending typed.

use super::control::{SolveControl, SolvePhase};
use super::outcome::{NpdSite, SolveFailure, SolveLimits, SolveStop, reported_count};
use super::system::SpdSystem;
use crate::solver::{CgEnd, CgIterationControl, jacobi_pcg_seq, norm2_squared};

/// A PCG run that produced an iterate.
pub(crate) enum PcgOutcome {
    Converged {
        u: Vec<f64>,
        iterations: u32,
    },
    /// The iteration cap `cap` ran out; `u` is the last iterate.
    CapReached {
        u: Vec<f64>,
        cap: u32,
    },
}

/// PCG on `system` from `warm` (or zero) to `limits`. Requires a non-zero
/// `rhs` (the session owns the zero-RHS rule).
pub(crate) fn solve(
    system: &SpdSystem,
    rhs: &[f64],
    limits: SolveLimits,
    warm: Option<&[f64]>,
    ctl: &mut SolveControl<'_>,
) -> Result<PcgOutcome, SolveStop> {
    ctl.enter(SolvePhase::Iterate)?;
    let rhs_norm = norm2_squared(rhs).sqrt();
    let mut report = |iter: usize, residual_norm: f64| match ctl
        .iteration(reported_count(iter), residual_norm / rhs_norm)
    {
        Ok(()) => CgIterationControl::Continue,
        Err(_) => CgIterationControl::Cancel,
    };
    let run = jacobi_pcg_seq(
        system.matrix(),
        rhs,
        warm,
        limits.tolerance,
        limits.max_iter,
        &mut report,
    );
    let iterations = reported_count(run.iterations);
    match run.end {
        CgEnd::Converged => Ok(PcgOutcome::Converged {
            u: run.u,
            iterations,
        }),
        CgEnd::CapReached => Ok(PcgOutcome::CapReached {
            u: run.u,
            cap: iterations,
        }),
        CgEnd::Cancelled => Err(SolveStop::Cancelled {
            phase: SolvePhase::Iterate,
        }),
        CgEnd::NonPositiveCurvature { iteration, .. } => Err(SolveFailure::NotPositiveDefinite {
            site: NpdSite::Curvature {
                iteration: reported_count(iteration),
            },
        }
        .into()),
    }
}
