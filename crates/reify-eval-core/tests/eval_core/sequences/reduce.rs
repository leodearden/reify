//! How a case shrinks: the candidates one step smaller than it, largest
//! reductions first, and a size each candidate strictly lowers. Every
//! candidate still names only snapshots made before each event.

use super::case::{Case, HostEvent, Then};
use super::design::{Bind, Decl, ModelPath, ModelRef, ToyKind, Val, decl};

/// The cases one step smaller than `case`, largest reductions first: an
/// event dropped, a demanded path dropped or shortened, a Drive made an
/// Evaluate, a request ended rather than left, an Evaluate's budget
/// dropped, an edit's change dropped, an initial input dropped, then one
/// value made smaller.
pub fn candidates(case: &Case) -> Vec<Case> {
    let mut all: Vec<_> = (0..case.events.len())
        .map(|at| without_event(case, at))
        .collect();
    all.extend(each_event(case, |event| {
        let Some(paths) = demanded(event) else {
            return Vec::new();
        };
        let dropped = (0..paths.len()).filter(|_| paths.len() > 1).map(|at| {
            let mut fewer = paths.clone();
            fewer.remove(at);
            fewer
        });
        dropped.map(|paths| with_paths(event, paths)).collect()
    }));
    all.extend(each_event(case, |event| {
        let Some(paths) = demanded(event) else {
            return Vec::new();
        };
        let shortened = paths.iter().enumerate().filter_map(|(at, path)| {
            let (parent, _) = path.parent()?;
            let mut shorter = paths.clone();
            shorter[at] = parent;
            Some(shorter)
        });
        shortened.map(|paths| with_paths(event, paths)).collect()
    }));
    all.extend(each_event(case, |event| match event {
        HostEvent::Drive {
            on, paths, then, ..
        } => vec![HostEvent::Evaluate {
            on: *on,
            paths: paths.clone(),
            budget: None,
            then: *then,
        }],
        _ => Vec::new(),
    }));
    all.extend(each_event(case, |event| match event {
        HostEvent::Evaluate {
            on,
            paths,
            budget,
            then: Then::Leave,
        } => vec![HostEvent::Evaluate {
            on: *on,
            paths: paths.clone(),
            budget: *budget,
            then: Then::End,
        }],
        HostEvent::Drive {
            on,
            paths,
            turns,
            then: Then::Leave,
        } => vec![HostEvent::Drive {
            on: *on,
            paths: paths.clone(),
            turns: *turns,
            then: Then::End,
        }],
        _ => Vec::new(),
    }));
    all.extend(each_event(case, |event| match event {
        HostEvent::Evaluate {
            on,
            paths,
            budget: Some(_),
            then,
        } => vec![HostEvent::Evaluate {
            on: *on,
            paths: paths.clone(),
            budget: None,
            then: *then,
        }],
        _ => Vec::new(),
    }));
    all.extend(each_event(case, |event| match event {
        HostEvent::Edit { from, changes } => (0..changes.len())
            .map(|at| {
                let mut fewer = changes.clone();
                fewer.remove(at);
                HostEvent::Edit {
                    from: *from,
                    changes: fewer,
                }
            })
            .collect(),
        _ => Vec::new(),
    }));
    for key in case.inputs.keys() {
        let mut fewer = case.clone();
        fewer.inputs.remove(key);
        all.push(fewer);
    }
    for (key, value) in &case.inputs {
        for smaller in smaller_values(value) {
            let mut candidate = case.clone();
            candidate.inputs.insert(key.clone(), smaller);
            all.push(candidate);
        }
    }
    all.extend(each_event(case, |event| match event {
        HostEvent::Edit { from, changes } => (0..changes.len())
            .flat_map(|at| {
                let value = changes[at].1.iter();
                value.flat_map(smaller_values).map(move |smaller| {
                    let mut changes = changes.clone();
                    changes[at].1 = Some(smaller);
                    HostEvent::Edit {
                        from: *from,
                        changes,
                    }
                })
            })
            .collect(),
        _ => Vec::new(),
    }));
    all
}

/// A measure each candidate of a case strictly lowers: a count of its
/// events, paths, steps, inputs, changes, budgets and bindings, a Drive
/// above an Evaluate, a request left above one ended, any other kind above
/// a cell, and an int by its distance from zero.
pub fn size(case: &Case) -> u64 {
    let inputs: u64 = case
        .inputs
        .values()
        .map(|value| 1 + value_size(value))
        .sum();
    inputs + case.events.iter().map(event_size).sum::<u64>()
}

fn event_size(event: &HostEvent) -> u64 {
    let request = |paths: &[ModelPath], then: &Then| {
        let paths: u64 = paths.iter().map(|path| 1 + path.steps.len() as u64).sum();
        paths + u64::from(*then == Then::Leave)
    };
    match event {
        HostEvent::Edit { changes, .. } => {
            let change = |value: &Option<Val>| 1 + value.as_ref().map_or(0, value_size);
            1 + changes.iter().map(|(_, value)| change(value)).sum::<u64>()
        }
        HostEvent::Evaluate {
            paths,
            budget,
            then,
            ..
        } => 2 + u64::from(budget.is_some()) + request(paths, then),
        HostEvent::Drive { paths, then, .. } => 3 + request(paths, then),
    }
}

fn value_size(value: &Val) -> u64 {
    match value {
        Val::Int(value) => value.unsigned_abs(),
        Val::Bool(value) => u64::from(*value),
        Val::Members(members) => members.values().map(|decl| 1 + decl_size(decl)).sum(),
        Val::Decl(decl) => decl_size(decl),
        Val::Bound(bound) => *bound,
    }
}

fn decl_size(decl: &Decl) -> u64 {
    let bindings: u64 = decl
        .bindings
        .values()
        .map(|bind| 1 + bind_steps(bind))
        .sum();
    u64::from(decl.kind != ToyKind::Cell) + bindings
}

fn bind_steps(bind: &Bind) -> u64 {
    match bind {
        Bind::Declarer(place) => place.len() as u64,
        Bind::Passed(r) => r.steps.len() as u64,
        Bind::Input(_) => 0,
    }
}

/// `case` without event `at`. Dropping an Edit drops the snapshot it
/// made: later events on that snapshot act on the one it came from, and
/// later snapshots move down one.
fn without_event(case: &Case, at: usize) -> Case {
    let mut events = case.events.clone();
    let dropped = events.remove(at);
    if let HostEvent::Edit { from, .. } = dropped {
        let made = 1 + case.events[..at]
            .iter()
            .filter(|event| matches!(event, HostEvent::Edit { .. }))
            .count();
        for event in &mut events[at..] {
            let named = match event {
                HostEvent::Edit { from, .. } => from,
                HostEvent::Evaluate { on, .. } | HostEvent::Drive { on, .. } => on,
            };
            if *named == made {
                *named = from;
            } else if *named > made {
                *named -= 1;
            }
        }
    }
    Case {
        inputs: case.inputs.clone(),
        events,
    }
}

/// `case` with one event replaced by each smaller form `smaller` gives.
fn each_event(case: &Case, smaller: impl Fn(&HostEvent) -> Vec<HostEvent>) -> Vec<Case> {
    let mut all = Vec::new();
    for (at, event) in case.events.iter().enumerate() {
        for replaced in smaller(event) {
            let mut candidate = case.clone();
            candidate.events[at] = replaced;
            all.push(candidate);
        }
    }
    all
}

fn demanded(event: &HostEvent) -> Option<&Vec<ModelPath>> {
    match event {
        HostEvent::Evaluate { paths, .. } | HostEvent::Drive { paths, .. } => Some(paths),
        HostEvent::Edit { .. } => None,
    }
}

fn with_paths(event: &HostEvent, paths: Vec<ModelPath>) -> HostEvent {
    match event {
        HostEvent::Evaluate {
            on, budget, then, ..
        } => HostEvent::Evaluate {
            on: *on,
            paths,
            budget: *budget,
            then: *then,
        },
        HostEvent::Drive {
            on, turns, then, ..
        } => HostEvent::Drive {
            on: *on,
            paths,
            turns: *turns,
            then: *then,
        },
        HostEvent::Edit { .. } => unreachable!("only a request demands paths"),
    }
}

/// The values one step smaller than `value`: an int or a bound toward
/// zero, true made false, a member dropped or made smaller, a declaration
/// made smaller.
fn smaller_values(value: &Val) -> Vec<Val> {
    match value {
        Val::Int(value) => {
            let mut smaller = Vec::new();
            for toward in [0, value / 2, value - value.signum()] {
                if toward != *value && !smaller.contains(&toward) {
                    smaller.push(toward);
                }
            }
            smaller.into_iter().map(Val::Int).collect()
        }
        Val::Bool(true) => vec![Val::Bool(false)],
        Val::Bool(false) => Vec::new(),
        Val::Members(members) => {
            let dropped = members.keys().map(|step| {
                let mut fewer = members.clone();
                fewer.remove(step);
                fewer
            });
            let shrunk = members.iter().flat_map(|(step, decl)| {
                smaller_decls(decl).into_iter().map(move |smaller| {
                    let mut members = members.clone();
                    members.insert(step.clone(), smaller);
                    members
                })
            });
            dropped.chain(shrunk).map(Val::Members).collect()
        }
        Val::Decl(decl) => smaller_decls(decl).into_iter().map(Val::Decl).collect(),
        Val::Bound(bound) => {
            let mut smaller = Vec::new();
            for toward in [0, bound / 2, bound.saturating_sub(1)] {
                if toward != *bound && !smaller.contains(&toward) {
                    smaller.push(toward);
                }
            }
            smaller.into_iter().map(Val::Bound).collect()
        }
    }
}

/// A declaration of any other kind made a cell with no terms, a binding
/// dropped, or a binding's steps shortened by one.
fn smaller_decls(of: &Decl) -> Vec<Decl> {
    let mut smaller = Vec::new();
    if of.kind != ToyKind::Cell {
        smaller.push(decl(ToyKind::Cell, []));
    }
    for name in of.bindings.keys() {
        let mut fewer = of.clone();
        fewer.bindings.remove(name);
        smaller.push(fewer);
    }
    for (name, bind) in &of.bindings {
        let shorter = match bind {
            Bind::Declarer(place) if !place.is_empty() => {
                Bind::Declarer(place[..place.len() - 1].to_vec())
            }
            Bind::Passed(r) if !r.steps.is_empty() => Bind::Passed(ModelRef {
                binding: r.binding.clone(),
                steps: r.steps[..r.steps.len() - 1].to_vec(),
            }),
            _ => continue,
        };
        let mut shortened = of.clone();
        shortened.bindings.insert(name.clone(), shorter);
        smaller.push(shortened);
    }
    smaller
}

mod tests {
    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::{Inputs, ModelPath, ToyKind, int, named, path};
    use super::super::generator::gate_cases;
    use super::super::reference::{Reference, first_invalid};
    use super::super::shrink::shrink;
    use super::{candidates, size};

    #[test]
    fn dropping_an_edit_moves_later_events_onto_the_snapshot_it_came_from() {
        let edit = |from| HostEvent::Edit {
            from,
            changes: vec![(named("x"), Some(int(1)))],
        };
        let evaluate = |on| HostEvent::Evaluate {
            on,
            paths: vec![path("c", [])],
            budget: None,
            then: Then::End,
        };
        let case = Case {
            inputs: Inputs::new(),
            events: vec![
                edit(0),
                edit(1),
                evaluate(2),
                evaluate(1),
                edit(2),
                evaluate(3),
            ],
        };
        let without_second_edit = Case {
            inputs: Inputs::new(),
            events: vec![edit(0), evaluate(1), evaluate(1), edit(1), evaluate(2)],
        };
        assert!(candidates(&case).contains(&without_second_edit));
        assert!(without_second_edit.names_made_snapshots());
    }

    #[test]
    fn every_candidate_is_smaller_and_well_formed() {
        for (seed, case) in gate_cases() {
            let smaller = candidates(&case);
            assert!(!smaller.is_empty(), "seed {seed} has no candidate");
            for candidate in smaller {
                assert!(size(&candidate) < size(&case), "seed {seed}: {candidate:?}");
                assert!(
                    candidate.names_made_snapshots(),
                    "seed {seed}: {candidate:?}"
                );
                assert_eq!(
                    first_invalid(&candidate),
                    None,
                    "seed {seed}: {candidate:?}"
                );
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
        assert!(
            still.is_empty(),
            "seed {seed}: {shrunk:?} shrinks further to {still:?}"
        );
        let [HostEvent::Evaluate { paths, .. }] = shrunk.events.as_slice() else {
            panic!("seed {seed}: one Evaluate is all the predicate needs: {shrunk:?}");
        };
        assert_eq!(paths.len(), 1, "seed {seed}: {shrunk:?}");
    }
}
