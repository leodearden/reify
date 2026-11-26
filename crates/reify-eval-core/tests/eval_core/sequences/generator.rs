//! Seeded random cases: a toy design of cells and, as features allow,
//! scopes, guards and collections, and a sequence of host events on it.
//! Every design is acyclic by construction (the ordinal rule: member m_k
//! reaches through its declarer only the declarer itself or members m_j
//! with j < k), and a feature that is off draws nothing from the rng, so
//! a later leaf's feature leaves this feature set's stream unchanged.

use std::ops::RangeInclusive;

use super::case::Case;
use super::design::Inputs;

/// The seeds the gate runs, each for `GATE_EVENTS` host events.
pub const GATE_SEEDS: RangeInclusive<u64> = 1..=64;
pub const GATE_EVENTS: usize = 16;

/// Which toy kinds a design may use besides cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    pub scope: bool,
    pub guard: bool,
    pub collection: bool,
}

impl Features {
    /// Every kind of structure.
    pub fn structure() -> Self {
        Self {
            scope: true,
            guard: true,
            collection: true,
        }
    }
}

/// The case `seed` gives: a design using `features` and `events` host
/// events on it.
pub fn generate(seed: u64, features: Features, events: usize) -> Case {
    let _ = (seed, features, events);
    Case {
        inputs: Inputs::new(),
        events: Vec::new(),
    }
}

mod tests {
    use std::collections::BTreeSet;

    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::{Bind, Decl, Key, ToyKind, Val};
    use super::super::reference::first_invalid;
    use super::{Features, GATE_EVENTS, GATE_SEEDS, generate};

    fn gate_cases() -> impl Iterator<Item = (u64, Case)> {
        GATE_SEEDS.map(|seed| (seed, generate(seed, Features::structure(), GATE_EVENTS)))
    }

    /// Every input value a case sets, initially or by an edit.
    fn values(case: &Case) -> Vec<(&Key, &Val)> {
        let edited = case.events.iter().flat_map(|event| match event {
            HostEvent::Edit { changes, .. } => changes
                .iter()
                .filter_map(|(key, value)| value.as_ref().map(|value| (key, value)))
                .collect(),
            _ => Vec::new(),
        });
        case.inputs.iter().chain(edited).collect()
    }

    /// Every declaration a case holds, members' members included.
    fn decls(case: &Case) -> Vec<&Decl> {
        fn within(value: &Val) -> Vec<&Decl> {
            match value {
                Val::Decl(decl) => vec![decl],
                Val::Members(members) => members.values().collect(),
                Val::Int(_) | Val::Bool(_) => Vec::new(),
            }
        }
        values(case).into_iter().flat_map(|(_, value)| within(value)).collect()
    }

    #[test]
    fn a_seed_gives_one_case_and_another_seed_another() {
        let features = Features::structure();
        assert_eq!(generate(7, features, GATE_EVENTS), generate(7, features, GATE_EVENTS));
        assert_ne!(generate(7, features, GATE_EVENTS), generate(8, features, GATE_EVENTS));
    }

    #[test]
    fn gate_cases_name_made_snapshots_edit_after_each_drive_and_are_valid() {
        for (seed, case) in gate_cases() {
            assert_eq!(case.events.len(), GATE_EVENTS, "seed {seed}");
            assert!(case.names_made_snapshots(), "seed {seed}: {case:?}");
            let mut made = 1;
            for (at, event) in case.events.iter().enumerate() {
                if let HostEvent::Drive { on, .. } = event
                    && at + 1 < case.events.len()
                {
                    let next = &case.events[at + 1];
                    let edits_driven = matches!(next, HostEvent::Edit { from, .. } if from == on);
                    assert!(edits_driven, "seed {seed}: event {at} is a Drive on {on}, then {next:?}");
                }
                if let HostEvent::Edit { .. } = event {
                    made += 1;
                }
            }
            assert!(made > 1, "seed {seed} edits nothing");
            assert_eq!(first_invalid(&case), None, "seed {seed}");
        }
    }

    #[test]
    fn a_feature_that_is_off_puts_no_declaration_of_its_kind_in_a_case() {
        let cells_only = Features {
            scope: false,
            guard: false,
            collection: false,
        };
        for seed in GATE_SEEDS {
            let case = generate(seed, cells_only, GATE_EVENTS);
            assert!(!case.events.is_empty(), "seed {seed}");
            let kinds: BTreeSet<_> = decls(&case).iter().map(|decl| decl.kind).collect();
            assert!(kinds.iter().all(|kind| *kind == ToyKind::Cell), "seed {seed}: {kinds:?}");
        }
    }

    #[test]
    fn the_gate_cases_reach_every_kind_binding_and_event_shape() {
        let mut kinds = BTreeSet::new();
        let (mut pickers, mut passed, mut extras) = (0, 0, 0);
        let (mut edits_off_latest, mut evaluations_off_latest) = (0, 0);
        let (mut zero_left, mut zero_ended) = (0, 0);
        for (_, case) in gate_cases() {
            for decl in decls(&case) {
                kinds.insert(decl.kind);
                pickers += usize::from(decl.kind == ToyKind::Cell && decl.bindings.contains_key("pick"));
                passed += decl.bindings.values().filter(|bind| matches!(bind, Bind::Passed(_))).count();
            }
            extras += values(&case)
                .iter()
                .filter(|(key, _)| matches!(key, Key::Attached(_, name) if name == "extra"))
                .count();
            let mut latest = 0;
            for event in &case.events {
                match event {
                    HostEvent::Edit { from, .. } => {
                        edits_off_latest += usize::from(*from != latest);
                        latest += 1;
                    }
                    HostEvent::Evaluate { on, .. } => evaluations_off_latest += usize::from(*on != latest),
                    HostEvent::Drive { turns: 0, then: Then::Leave, .. } => zero_left += 1,
                    HostEvent::Drive { turns: 0, then: Then::End, .. } => zero_ended += 1,
                    HostEvent::Drive { .. } => {}
                }
            }
        }
        let all = [ToyKind::Cell, ToyKind::Scope, ToyKind::Guard, ToyKind::Collection];
        assert_eq!(kinds, BTreeSet::from(all));
        let counts = [
            ("pickers", pickers),
            ("passed bindings", passed),
            ("attached extras", extras),
            ("edits off the latest snapshot", edits_off_latest),
            ("evaluations off the latest snapshot", evaluations_off_latest),
            ("drives of no turns left live", zero_left),
            ("drives of no turns ended", zero_ended),
        ];
        for (shape, count) in counts {
            assert!(count > 0, "the gate cases hold no {shape}");
        }
    }
}
