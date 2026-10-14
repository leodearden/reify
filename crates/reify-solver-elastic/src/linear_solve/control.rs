//! Progress and cooperative cancel for a solve (PRD §5 C5): the caller's
//! callback sees every event and may cancel at any of them.

use core::ops::ControlFlow;

use super::outcome::SolveStop;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SolvePhase {
    /// The symbolic factor analysis.
    Analyse,
    /// The numeric factorisation.
    Factor,
    /// PCG iterations.
    Iterate,
    /// The forward/backward substitution of one right-hand side.
    Substitute,
    /// The true-residual check.
    Verify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolveEvent {
    /// Fires once on entering a phase.
    Phase(SolvePhase),
    /// After PCG iteration `iter` (1-based); `residual` is the relative
    /// recurrence residual `‖r_iter‖/‖f‖`.
    Iteration { iter: u32, residual: f64 },
}

pub struct SolveControl<'a> {
    /// Progress and cooperative cancel poll; `ControlFlow::Break(())` cancels.
    pub on_event: &'a mut dyn FnMut(&SolveEvent) -> ControlFlow<()>,
}

impl<'a> SolveControl<'a> {
    pub fn new(on_event: &'a mut dyn FnMut(&SolveEvent) -> ControlFlow<()>) -> Self {
        Self { on_event }
    }

    /// Announces `phase`; `Cancelled { phase }` if the callback breaks.
    pub(crate) fn enter(&mut self, phase: SolvePhase) -> Result<(), SolveStop> {
        self.poll(&SolveEvent::Phase(phase), phase)
    }

    /// Reports a PCG iteration; `Cancelled { Iterate }` if the callback breaks.
    pub(crate) fn iteration(&mut self, iter: u32, residual: f64) -> Result<(), SolveStop> {
        self.poll(
            &SolveEvent::Iteration { iter, residual },
            SolvePhase::Iterate,
        )
    }

    fn poll(&mut self, event: &SolveEvent, phase: SolvePhase) -> Result<(), SolveStop> {
        match (self.on_event)(event) {
            ControlFlow::Continue(()) => Ok(()),
            ControlFlow::Break(()) => Err(SolveStop::Cancelled { phase }),
        }
    }
}
