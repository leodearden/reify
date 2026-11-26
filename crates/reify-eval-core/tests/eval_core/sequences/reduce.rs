//! How a case shrinks: the candidates one step smaller than it, largest
//! reductions first, and a size each candidate strictly lowers. Every
//! candidate still names only snapshots made before each event.

use super::case::Case;

/// The cases one step smaller than `case`.
pub fn candidates(case: &Case) -> Vec<Case> {
    let _ = case;
    Vec::new()
}

/// A measure each candidate of a case strictly lowers.
pub fn size(case: &Case) -> u64 {
    let _ = case;
    0
}

mod tests {
    use super::super::case::{Case, HostEvent};
    use super::super::design::{ModelPath, ToyKind};
    use super::super::generator::{Features, GATE_EVENTS, GATE_SEEDS, generate};
    use super::super::reference::{Reference, first_invalid};
    use super::super::shrink::shrink;
    use super::{candidates, size};

    fn gate_cases() -> impl Iterator<Item = (u64, Case)> {
        GATE_SEEDS.map(|seed| (seed, generate(seed, Features::structure(), GATE_EVENTS)))
    }

    #[test]
    fn every_candidate_is_smaller_well_formed_and_acyclic() {
        for (seed, case) in gate_cases() {
            let smaller = candidates(&case);
            assert!(!smaller.is_empty(), "seed {seed} has no candidate");
            for candidate in smaller {
                assert!(size(&candidate) < size(&case), "seed {seed}: {candidate:?}");
                assert!(candidate.names_made_snapshots(), "seed {seed}: {candidate:?}");
                assert_eq!(first_invalid(&candidate), None, "seed {seed}: {candidate:?}");
            }
        }
    }

    /// Whether some strict prefix of `path` is declared a guard.
    fn below_a_guard(reference: &mut Reference, path: &ModelPath) -> bool {
        let mut at = path.clone();
        while let Some((parent, _)) = at.parent() {
            let declared = reference.declaration_at(&parent).ok().flatten();
            if declared.is_some_and(|decl| decl.kind == ToyKind::Guard) {
                return true;
            }
            at = parent;
        }
        false
    }

    /// Whether an Evaluate demands a path below a guard on its snapshot.
    fn evaluates_below_a_guard(case: &Case) -> bool {
        let snapshots = case.snapshots();
        case.events.iter().any(|event| match event {
            HostEvent::Evaluate { on, paths, .. } => {
                let mut reference = Reference::new(snapshots[*on].clone());
                paths.iter().any(|path| below_a_guard(&mut reference, path))
            }
            HostEvent::Edit { .. } | HostEvent::Drive { .. } => false,
        })
    }

    #[test]
    fn a_case_shrinks_to_what_its_predicate_needs() {
        let (seed, case) = gate_cases()
            .find(|(_, case)| evaluates_below_a_guard(case))
            .expect("a gate case evaluates a path below a guard");
        let shrunk = shrink(case, candidates, size, evaluates_below_a_guard);
        assert!(evaluates_below_a_guard(&shrunk), "seed {seed}");
        let still: Vec<_> = candidates(&shrunk)
            .into_iter()
            .filter(evaluates_below_a_guard)
            .collect();
        assert!(still.is_empty(), "seed {seed}: {shrunk:?} shrinks further to {still:?}");
        let [HostEvent::Evaluate { paths, .. }] = shrunk.events.as_slice() else {
            panic!("seed {seed}: one Evaluate is all the predicate needs: {shrunk:?}");
        };
        assert_eq!(paths.len(), 1, "seed {seed}: {shrunk:?}");
    }
}
