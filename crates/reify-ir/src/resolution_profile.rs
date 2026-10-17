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
