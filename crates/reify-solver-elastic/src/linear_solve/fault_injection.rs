//! Fault injection for the seam's Cholesky driver ([`super::direct`]).
//!
//! The driver calls the crate-internal hooks below at fixed points. Without
//! the `linear-solve-fault-injection` feature they are inert, stateless
//! twins. With it, a test installs a [`FaultProbe`] on its own thread; the
//! hooks then record what the driver does and inject the faults the probe is
//! armed with. A thread with no installed probe sees inert hooks either way,
//! so parallel tests never observe each other.

use faer::sparse::linalg::SupernodalThreshold;
use faer::sparse::linalg::cholesky::SymbolicCholeskyRaw;

#[cfg(feature = "linear-solve-fault-injection")]
pub use armed::{FaultProbe, ProbeGuard, Reservation, SymbolicVariant};
#[cfg(feature = "linear-solve-fault-injection")]
pub(crate) use armed::{before_numeric, record_symbolic, reserve, symbolic_threshold};
#[cfg(not(feature = "linear-solve-fault-injection"))]
pub(crate) use inert::{before_numeric, record_symbolic, reserve, symbolic_threshold};

/// The driver's two fallible numeric-step allocations, in the order made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationSite {
    /// `8·len_val` bytes for the factor's values.
    FactorValues,
    /// faer's numeric-factorisation workspace.
    NumericScratch,
}

/// A reservation the installed probe was armed to fail.
pub(crate) struct Injected;

#[cfg(not(feature = "linear-solve-fault-injection"))]
mod inert {
    use super::*;

    pub(crate) fn symbolic_threshold() -> SupernodalThreshold {
        SupernodalThreshold::AUTO
    }

    pub(crate) fn record_symbolic(_raw: &SymbolicCholeskyRaw<usize>) {}

    pub(crate) fn reserve(_site: ReservationSite, _bytes: u64) -> Result<(), Injected> {
        Ok(())
    }

    pub(crate) fn before_numeric() {}
}

#[cfg(feature = "linear-solve-fault-injection")]
mod armed {
    use std::cell::RefCell;
    use std::marker::PhantomData;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    use super::*;

    /// Which structure faer's symbolic analysis produced.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SymbolicVariant {
        Simplicial,
        Supernodal,
    }

    /// One attempted reservation, recorded before the allocation is tried.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Reservation {
        pub site: ReservationSite,
        pub bytes: u64,
    }

    #[derive(Default)]
    struct State {
        forced_variant: Option<SymbolicVariant>,
        failing_site: Option<ReservationSite>,
        symbolic_variants: Vec<SymbolicVariant>,
        reservations: Vec<Reservation>,
        numeric_calls: usize,
    }

    /// A shared record of the driver's activity on every thread it is
    /// installed on, plus the faults it injects there. Clones share state.
    #[derive(Clone, Default)]
    pub struct FaultProbe {
        state: Arc<Mutex<State>>,
    }

    /// Keeps a probe installed on the current thread; dropping it restores
    /// whatever was installed before.
    pub struct ProbeGuard {
        previous: Option<FaultProbe>,
        _thread_bound: PhantomData<*const ()>,
    }

    thread_local! {
        static INSTALLED: RefCell<Option<FaultProbe>> = const { RefCell::new(None) };
    }

    impl FaultProbe {
        pub fn new() -> Self {
            Self::default()
        }

        /// Routes the current thread's driver hooks to this probe.
        pub fn install(&self) -> ProbeGuard {
            ProbeGuard {
                previous: INSTALLED.replace(Some(self.clone())),
                _thread_bound: PhantomData,
            }
        }

        /// Makes every later symbolic analysis produce `variant`.
        pub fn force_symbolic_variant(&self, variant: SymbolicVariant) {
            self.state().forced_variant = Some(variant);
        }

        /// Makes every later reservation at `site` fail as out-of-memory.
        pub fn fail_reservation(&self, site: ReservationSite) {
            self.state().failing_site = Some(site);
        }

        /// Completed symbolic analyses.
        pub fn symbolic_calls(&self) -> usize {
            self.state().symbolic_variants.len()
        }

        /// The variant of each completed symbolic analysis, in order.
        pub fn symbolic_variants(&self) -> Vec<SymbolicVariant> {
            self.state().symbolic_variants.clone()
        }

        /// Every attempted reservation, in order, failed ones included.
        pub fn reservations(&self) -> Vec<Reservation> {
            self.state().reservations.clone()
        }

        /// Numeric factorisations started (both reservations succeeded).
        pub fn numeric_calls(&self) -> usize {
            self.state().numeric_calls
        }

        fn state(&self) -> MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    impl Drop for ProbeGuard {
        fn drop(&mut self) {
            INSTALLED.set(self.previous.take());
        }
    }

    /// `f` applied to the installed probe's state; `None` with no probe.
    fn with_installed<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
        INSTALLED.with_borrow(|probe| probe.as_ref().map(|probe| f(&mut probe.state())))
    }

    pub(crate) fn symbolic_threshold() -> SupernodalThreshold {
        match with_installed(|state| state.forced_variant).flatten() {
            None => SupernodalThreshold::AUTO,
            Some(SymbolicVariant::Simplicial) => SupernodalThreshold::FORCE_SIMPLICIAL,
            Some(SymbolicVariant::Supernodal) => SupernodalThreshold::FORCE_SUPERNODAL,
        }
    }

    pub(crate) fn record_symbolic(raw: &SymbolicCholeskyRaw<usize>) {
        let variant = match raw {
            SymbolicCholeskyRaw::Simplicial(_) => SymbolicVariant::Simplicial,
            SymbolicCholeskyRaw::Supernodal(_) => SymbolicVariant::Supernodal,
        };
        with_installed(|state| state.symbolic_variants.push(variant));
    }

    pub(crate) fn reserve(site: ReservationSite, bytes: u64) -> Result<(), Injected> {
        with_installed(|state| {
            state.reservations.push(Reservation { site, bytes });
            match state.failing_site {
                Some(failing) if failing == site => Err(Injected),
                _ => Ok(()),
            }
        })
        .unwrap_or(Ok(()))
    }

    pub(crate) fn before_numeric() {
        with_installed(|state| state.numeric_calls += 1);
    }
}
