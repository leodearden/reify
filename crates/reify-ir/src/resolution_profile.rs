//! How a solving driver may spend and reuse resolution work.
//!
//! A [`ResolutionProfile`] varies along exactly two axes: the solver
//! [`IterationBudget`] and the [`StalenessPolicy`] for previously computed
//! results. It has no way to decline a stage. Rationale:
//! `docs/prds/v0_6/solver-driver-parity.md` §4.5 (I5).

/// How many iterations a solve may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IterationBudget {
    /// The solver's own built-in iteration limits, the only budget any driver
    /// uses today.
    SolverDefault,
}

/// Whether a driver may serve a previously computed result instead of
/// recomputing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StalenessPolicy {
    /// Every result is computed in this session.
    Fresh,
    /// Results persisted by an earlier session may be served.
    MayServePersisted,
}

impl StalenessPolicy {
    pub const fn permits_persisted_results(self) -> bool {
        matches!(self, Self::MayServePersisted)
    }
}

/// A solving driver's resolution profile: an iteration budget and a
/// staleness policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolutionProfile {
    iteration_budget: IterationBudget,
    staleness: StalenessPolicy,
}

impl ResolutionProfile {
    /// The solver's default budget, serving nothing persisted.
    pub const FRESH: Self = Self::new(IterationBudget::SolverDefault, StalenessPolicy::Fresh);

    pub const fn new(iteration_budget: IterationBudget, staleness: StalenessPolicy) -> Self {
        Self {
            iteration_budget,
            staleness,
        }
    }

    pub const fn iteration_budget(self) -> IterationBudget {
        self.iteration_budget
    }

    pub const fn staleness(self) -> StalenessPolicy {
        self.staleness
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGETS: [IterationBudget; 1] = [IterationBudget::SolverDefault];
    const STALENESS: [StalenessPolicy; 2] =
        [StalenessPolicy::Fresh, StalenessPolicy::MayServePersisted];

    #[test]
    fn only_may_serve_persisted_permits_persisted_results() {
        assert!(!StalenessPolicy::Fresh.permits_persisted_results());
        assert!(StalenessPolicy::MayServePersisted.permits_persisted_results());
    }

    #[test]
    fn fresh_preset_is_solver_default_budget_and_fresh_staleness() {
        assert_eq!(
            ResolutionProfile::FRESH.iteration_budget(),
            IterationBudget::SolverDefault
        );
        assert_eq!(ResolutionProfile::FRESH.staleness(), StalenessPolicy::Fresh);
    }

    #[test]
    fn a_profile_is_exactly_its_two_axes() {
        let pairs: Vec<(IterationBudget, StalenessPolicy)> = BUDGETS
            .iter()
            .flat_map(|&b| STALENESS.iter().map(move |&s| (b, s)))
            .collect();
        for &(b, s) in &pairs {
            let profile = ResolutionProfile::new(b, s);
            assert_eq!(profile.iteration_budget(), b);
            assert_eq!(profile.staleness(), s);
            for &(b2, s2) in &pairs {
                assert_eq!(
                    profile == ResolutionProfile::new(b2, s2),
                    b == b2 && s == s2,
                    "profiles must compare equal iff both axes match: ({b:?}, {s:?}) vs ({b2:?}, {s2:?})"
                );
            }
        }
    }
}
