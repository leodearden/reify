//! How many resolutions a correct core charges to a request's paths budget
//! whatever the order of its work (contract 7.6): every path a fresh
//! reference resolves answering the demand, but for one the run of a call
//! may reach first once nothing waits on the call.
//!
//! A resolution is charged to a request while one of its demanded paths
//! that is not yet answered waits on the node resolving it, directly or
//! through other waits. A wait ends when what it waits on is answered, or
//! when a call declares the child whose path was waiting on it, and from
//! then the call waits on its child. So once a call's first child is
//! answered, and the last child a demanded path waits through, the call
//! may run on with nothing waiting on it, and what its later work reaches
//! first is charged to no request. That work cannot reach first what is
//! surely resolved before it: the call and the paths above it, and each
//! place the call read before its first child, all answered, with what
//! their work reached and the paths above each.
//!
//! A call is waited on until it is answered where a demanded path is the
//! call, or waits on it through a step its run does not declare, or where
//! nothing reads a place below it and no demanded path is below it: then
//! whatever reached the call reads its value.
//!
//! A trial of a solve is charged to the request the solve's resolution
//! is, by the same rule, its demand being the places the solve read
//! through its view: a call inside a trial may run on there too.

use std::collections::BTreeSet;
use std::iter::successors;

use super::design::ModelPath;
use super::reference::{Reached, Reference};

/// How many of the paths `reference` and its trials resolved answering
/// `demanded` every order of a correct core's work charges to the request.
pub fn surely_charged(reference: &Reference, demanded: &[ModelPath]) -> usize {
    reference.paths_resolved() - uncharged(reference, demanded)
}

/// How many of the paths `reference` and its trials resolved answering
/// `demanded` some order of a correct core's work charges to no request:
/// those a call reaches first once it runs on with nothing waiting on it.
fn uncharged(reference: &Reference, demanded: &[ModelPath]) -> usize {
    let demand = Demand::of(reference, demanded);
    let mut reached_running_on = BTreeSet::new();
    for (call, reached) in reference.reaches() {
        let Some(after) = demand.runs_on_from(call, reached) else {
            continue;
        };
        let before = reached
            .iter()
            .take_while(|reached| matches!(reached, Reached::Read(_)));
        let settled = settled_before(reference, call, before);
        reached_running_on.extend(reach_from(reference, &reached[after..], &settled));
    }
    let here = reached_running_on
        .iter()
        .filter(|path| reference.resolved(path) && !demand.chains.contains(*path))
        .count();
    let in_trials: usize = reference
        .every_trial()
        .map(|trial| uncharged(trial.reference(), trial.demand()))
        .sum();
    here + in_trials
}

/// A demand's paths, each with the paths above it, which it waits through
/// from its start; and every place a read in the reference named.
struct Demand<'a> {
    demanded: &'a [ModelPath],
    chains: BTreeSet<ModelPath>,
    read: BTreeSet<&'a ModelPath>,
}

impl<'a> Demand<'a> {
    fn of(reference: &'a Reference, demanded: &'a [ModelPath]) -> Self {
        let read = reference
            .reaches()
            .flat_map(|(_, reached)| reached)
            .filter_map(|reached| match reached {
                Reached::Read(path) => Some(path),
                Reached::Child(_) => None,
            })
            .collect();
        Self {
            demanded,
            chains: demanded.iter().flat_map(ancestors_or_self).collect(),
            read,
        }
    }

    /// Where the work of the node at `node`, which reached `reached`, may
    /// go on with nothing waiting on it: past its first child and past the
    /// last child a demanded path waits through. None where it declares no
    /// child, or is waited on until it is answered.
    fn runs_on_from(&self, node: &ModelPath, reached: &[Reached]) -> Option<usize> {
        let is_child = |reached: &Reached| matches!(reached, Reached::Child(_));
        let first_child = reached.iter().position(is_child)?;
        if self.waits_to_the_end(node, reached) {
            return None;
        }
        let on_a_chain = |reached: &Reached| match reached {
            Reached::Child(child) => self.chains.contains(child),
            Reached::Read(_) => false,
        };
        let last_on_a_chain = reached.iter().rposition(on_a_chain);
        Some(last_on_a_chain.map_or(first_child, |last| last.max(first_child)) + 1)
    }

    /// Whether something waits on the call at `call`, whose work reached
    /// `reached`, until it is answered.
    fn waits_to_the_end(&self, call: &ModelPath, reached: &[Reached]) -> bool {
        let below = |path: &ModelPath| is_below(path, call);
        let undeclared_step = |path: &ModelPath| {
            path.parent().is_some_and(|(parent, _)| parent == *call)
                && !reached.contains(&Reached::Child(path.clone()))
        };
        self.demanded.contains(call)
            || self.chains.iter().any(undeclared_step)
            || !(self.chains.iter().any(below) || self.read.iter().any(|path| below(path)))
    }
}

/// What is surely resolved before the work of the call at `call` goes past
/// `before`, the reads it made before its first child.
fn settled_before<'a>(
    reference: &Reference,
    call: &ModelPath,
    before: impl Iterator<Item = &'a Reached>,
) -> BTreeSet<ModelPath> {
    let mut settled: BTreeSet<ModelPath> = ancestors_or_self(call).collect();
    let mut answered = BTreeSet::new();
    let mut todo: Vec<ModelPath> = before.map(|reached| reached.path().clone()).collect();
    while let Some(path) = todo.pop() {
        if !answered.insert(path.clone()) {
            continue;
        }
        settled.extend(ancestors_or_self(&path));
        let its_work = reference.reached_by(&path).iter();
        todo.extend(its_work.map(|reached| reached.path().clone()));
    }
    settled
}

/// Every place work that reaches `from` may reach first: each place in
/// `from` not in `settled`, the path above it, and what its own work
/// reached, in turn.
fn reach_from(
    reference: &Reference,
    from: &[Reached],
    settled: &BTreeSet<ModelPath>,
) -> BTreeSet<ModelPath> {
    let mut reached = BTreeSet::new();
    let mut todo: Vec<ModelPath> = from.iter().map(|from| from.path().clone()).collect();
    while let Some(path) = todo.pop() {
        if settled.contains(&path) || !reached.insert(path.clone()) {
            continue;
        }
        todo.extend(path.parent().map(|(parent, _)| parent));
        let its_work = reference.reached_by(&path).iter();
        todo.extend(its_work.map(|reached| reached.path().clone()));
    }
    reached
}

fn ancestors_or_self(path: &ModelPath) -> impl Iterator<Item = ModelPath> {
    successors(Some(path.clone()), |path| {
        path.parent().map(|(parent, _)| parent)
    })
}

/// Whether `path` is strictly below `above`.
fn is_below(path: &ModelPath, above: &ModelPath) -> bool {
    path.root == above.root
        && path.steps.len() > above.steps.len()
        && path.steps.starts_with(&above.steps)
}

#[cfg(test)]
mod tests {
    use reify_eval_core::Step;

    use super::super::design::*;
    use super::super::reference::Reference;
    use super::surely_charged;

    /// Root scope "r": a call "m0" whose term reads its sibling cell "m1",
    /// with two sites of one child each; a cell "m2" reading the call; a
    /// cell "m3" reading the call's first child.
    fn design() -> Inputs {
        let sites = calls_value([
            site(&[1], false, 0, cell([])),
            site(&[2], false, 0, cell([])),
        ]);
        inputs([
            (root("r"), declaration(scope("r.m"))),
            (
                named("r.m"),
                members([
                    (step("m0"), call([declarer([step("m1")])], input("k"))),
                    (step("m1"), cell([input("x")])),
                    (step("m2"), cell([declarer([step("m0")])])),
                    (
                        step("m3"),
                        cell([declarer([step("m0"), call_step(&[1], None)])]),
                    ),
                ]),
            ),
            (named("k"), sites),
            (named("x"), int(1)),
        ])
    }

    /// The paths a fresh reference resolves answering `demanded`, and how
    /// many of them are surely charged.
    fn charged(demanded: &[ModelPath]) -> (usize, usize) {
        let mut reference = Reference::new(design());
        for path in demanded {
            reference.answer(path).expect("the design is valid");
        }
        (
            reference.paths_resolved(),
            surely_charged(&reference, demanded),
        )
    }

    fn at(steps: impl IntoIterator<Item = Step>) -> ModelPath {
        path("r", steps)
    }

    #[test]
    fn what_a_call_reads_before_its_demanded_child_is_charged_and_its_later_child_is_not() {
        let child = at([step("m0"), call_step(&[1], None)]);
        assert_eq!(
            charged(&[child]),
            (5, 4),
            "r, m0, m1 and the child; not the second child"
        );
    }

    #[test]
    fn a_call_whose_value_is_read_is_charged_with_its_whole_run() {
        assert_eq!(charged(&[at([step("m2")])]), (6, 6));
    }

    #[test]
    fn a_cell_reading_a_child_of_a_call_that_runs_on_is_not_charged_with_the_later_child() {
        assert_eq!(charged(&[at([step("m3")])]), (6, 5));
    }

    /// Root scope "r": the solve "s" of the int unknown "n", whose one
    /// constraint is the first child of the call "c" over n, which has two
    /// sites of one child each.
    fn solve_over_a_call_s_child() -> Inputs {
        let sites = calls_value([
            site(&[1], false, 0, cell([])),
            site(&[2], false, 0, cell([])),
        ]);
        let first_child = declarer([step("c"), call_step(&[1], None)]);
        let held = members([
            (
                step("s"),
                solve(input("problem"), [declarer([step("n")])], [first_child]),
            ),
            (step("n"), solved_int(declarer([step("s")]), 0)),
            (step("c"), call([declarer([step("n")])], input("k"))),
        ]);
        inputs([
            (root("r"), declaration(scope("r.m"))),
            (named("r.m"), held),
            (named("k"), sites),
            (named("problem"), problem([vec![Part::Int(1)]], 1)),
        ])
    }

    #[test]
    fn a_call_in_a_trial_that_runs_on_is_not_charged_with_its_later_child() {
        let s = at([step("s")]);
        let mut reference = Reference::new(solve_over_a_call_s_child());
        reference.answer(&s).expect("the design is valid");
        assert_eq!(
            (reference.paths_resolved(), surely_charged(&reference, &[s])),
            (7, 6),
            "r and s; in the trial r, c, n, the child read and not the second child"
        );
    }

    #[test]
    fn a_demanded_child_declared_last_leaves_the_call_nothing_to_run_on() {
        let second = at([step("m0"), call_step(&[2], None)]);
        assert_eq!(charged(&[second]), (5, 5));
    }
}
