//! The generator's solve feature (leaf ι): a case's one solve group, its
//! problems and the demands and edits that reach it, all drawn from a
//! stream of their own and named apart from the generator's other inputs,
//! so that turning the feature on leaves every other feature's draws as
//! they were.
//!
//! A group is a scope, at a step of a space of its own, holding a solve of
//! a problem input, its int and flag unknowns, which are solved parts of
//! it, and the nodes its trials read: a guard on the flag unknown, a
//! collection counting the int unknown, a cell over the int unknown and,
//! with calls on, a call over it. Those nodes read only the group's
//! unknowns, inputs and constants, so no trial reaches another solve: one
//! view deep. The solve reads them only through its view, where its
//! unknowns are supposed, so the group forms no ring of waits. With cycles
//! on, at times a constraint reads the solve itself.

use std::collections::{BTreeMap, BTreeSet};

use reify_eval_core::{Space, Step, StepKey};

use super::design::{
    Bind, Decl, Key, ModelPath, Part, Val, call, calls_value, cell, collection, constant,
    declaration, declarer, guard, index, input, int, members, named, passed, problem, scope, site,
    solve, solved_flag, solved_int, step,
};
use super::rng::XorShift64Star;

/// What the solve stream's seed is the case seed mixed with.
const SOLVE_STREAM: u64 = 0x5017_E5EE_D5EE_D501;

/// The space of the step a solve group is at, apart from the ordinals of a
/// level's other members.
const SOLVE_SPACE: Space = Space::Named("toy.solve");

/// Whether `at` is the step a solve group is drawn at.
pub fn is_group_step(at: &Step) -> bool {
    at.space() == SOLVE_SPACE
}

/// The draws of the solve feature for one case. It draws nothing until its
/// group is drawn.
pub struct SolveDraws {
    rng: XorShift64Star,
    /// Whether a constraint may read the solve itself.
    cycles: bool,
    /// Whether the group holds a call.
    call: bool,
    named: usize,
    /// The inputs it made, which the generator's other edits leave.
    inputs: BTreeSet<String>,
    /// Each problem input, with the number of constraints its solve binds.
    problems: Vec<(String, u64)>,
    /// The path of the group's scope, once drawn.
    scope: Option<ModelPath>,
}

impl SolveDraws {
    /// The solve feature's draws for the case `seed` gives, with rings
    /// through the solve if `cycles` and a call in the group if `call`.
    pub fn new(seed: u64, cycles: bool, call: bool) -> Self {
        Self {
            rng: XorShift64Star::new(seed ^ SOLVE_STREAM),
            cycles,
            call,
            named: 0,
            inputs: BTreeSet::new(),
            problems: Vec::new(),
            scope: None,
        }
    }

    /// Whether the case's group is drawn.
    pub fn drawn(&self) -> bool {
        self.scope.is_some()
    }

    /// Whether `name` is an input these draws made.
    pub fn owns(&self, name: &str) -> bool {
        self.inputs.contains(name)
    }

    /// The case's solve group, as a member of the level at `at`: its step
    /// and the scope declared there. Pushes the inputs it needs to
    /// `needed`.
    pub fn group(&mut self, at: &ModelPath, needed: &mut Vec<(Key, Val)>) -> (Step, Decl) {
        let (solved, n, f) = (step("solve"), step("n"), step("f"));
        let read = |step: &Step| declarer([step.clone()]);
        let mut group = BTreeMap::new();
        let (mut constraints, last) = self.trial_nodes(&n, &f, &mut group, needed);
        if self.rng.chance(1, 2) {
            constraints.reverse();
        }
        if self.cycles && self.rng.chance(1, 4) {
            let place = self.rng.below(constraints.len() + 1);
            constraints.insert(place, read(&solved));
        }
        constraints.push(last);
        let bound = u64::try_from(constraints.len()).expect("a count of constraints fits u64");
        let value = self.problem_value(bound);
        let name = self.input("p", value, needed);
        self.problems.push((name.clone(), bound));
        let unknowns = [read(&n), read(&f)];
        group.insert(solved.clone(), solve(input(&name), unknowns, constraints));
        group.insert(n, solved_int(read(&solved), 0));
        group.insert(f, solved_flag(read(&solved), 1));
        let group_input = self.input("pq", members(group), needed);
        let at_group = Step::new(SOLVE_SPACE, StepKey::Name("group".into()));
        self.scope = Some(at.child(at_group.clone()));
        (at_group, scope(&group_input))
    }

    /// Once the group is drawn, at times a path to add to a demand: the
    /// solve, or a solved part.
    pub fn demand(&mut self) -> Option<ModelPath> {
        let group = self.scope.as_ref()?;
        if !self.rng.chance(3, 4) {
            return None;
        }
        let member = *self.rng.choose(&["solve", "solve", "n", "f"]);
        Some(group.child(step(member)))
    }

    /// Once the group is drawn, at times a change to add to an edit: a new
    /// value for its problem.
    pub fn edit(&mut self) -> Option<(Key, Option<Val>)> {
        if self.problems.is_empty() || !self.rng.chance(1, 3) {
            return None;
        }
        let (name, constraints) = self.rng.choose(&self.problems).clone();
        Some((named(&name), Some(self.problem_value(constraints))))
    }

    /// Puts into `group` the nodes a solve group's trials read, over its int
    /// unknown at `n` and its flag unknown at `f`. Returns the reads of
    /// them the constraints before the last make (the member of a guard on
    /// f, an element of a collection counting n and, with calls on, a cell
    /// over n that most candidates hold), and the last constraint's: a node
    /// over n that the smallest candidates fail (a call, or, with calls
    /// off, the cell), so that a trial that fails there has read the
    /// others, and so has the next.
    fn trial_nodes(
        &mut self,
        n: &Step,
        f: &Step,
        group: &mut BTreeMap<Step, Decl>,
        needed: &mut Vec<(Key, Val)>,
    ) -> (Vec<Bind>, Bind) {
        let (guarded, list) = (step("guard"), step("list"));
        let member = cell([constant(int(self.small_from(-1)))]);
        let guard_members = self.input("pg", members([(step("m0"), member)]), needed);
        group.insert(
            guarded.clone(),
            guard(declarer([f.clone()]), &guard_members),
        );
        let below_count = -1 - self.rng.below(2) as i64;
        let element = cell([passed("count", []), constant(int(below_count))]);
        let element = self.input("pe", declaration(element), needed);
        group.insert(
            list.clone(),
            collection(declarer([n.clone()]), input(&element)),
        );
        let at_index = self.rng.below(4) as u64;
        let mut reads = vec![
            declarer([guarded, step("m0")]),
            declarer([list, index(at_index)]),
        ];
        let (over_n, cell_over_n) = (step("cell"), declarer([step("cell")]));
        let over = |offset| cell([declarer([n.clone()]), constant(int(offset))]);
        if !self.call {
            group.insert(over_n, over(self.small_from(-4)));
            return (reads, cell_over_n);
        }
        group.insert(over_n, over(self.small_from(-1)));
        reads.push(cell_over_n);
        let offset = self.small_from(-4);
        let sites = calls_value([site(&[1], false, offset, cell([]))]);
        let sites = self.input("pk", sites, needed);
        group.insert(step("call"), call([declarer([n.clone()])], input(&sites)));
        (reads, declarer([step("call")]))
    }

    /// A problem of two to four candidates for a group's int and flag
    /// unknowns, against `constraints` constraints: each candidate's flag is
    /// the other of the one before, and its int differs from it, so that
    /// trials differ in a guard and in a count. The ints start at zero or
    /// one and mostly rise, and at times fall, so that some problems hold
    /// no candidate.
    fn problem_value(&mut self, constraints: u64) -> Val {
        let falling = self.rng.chance(1, 4);
        let mut int = self.rng.below(2) as i64;
        let mut flag = self.rng.chance(1, 2);
        let count = 2 + self.rng.below(3);
        let mut candidates = Vec::new();
        for _ in 0..count {
            candidates.push(vec![Part::Int(int), Part::Flag(flag)]);
            let step = 1 + self.rng.below(2) as i64;
            int += if falling { -step } else { step };
            flag = !flag;
        }
        problem(candidates, constraints)
    }

    /// One of the four ints from `least` up.
    fn small_from(&mut self, least: i64) -> i64 {
        least + self.rng.below(4) as i64
    }

    /// A new input of the solve feature's own naming, set to `value`.
    fn input(&mut self, prefix: &str, value: Val, needed: &mut Vec<(Key, Val)>) -> String {
        self.named += 1;
        let name = format!("{prefix}{}", self.named);
        needed.push((named(&name), value));
        self.inputs.insert(name.clone());
        name
    }
}
