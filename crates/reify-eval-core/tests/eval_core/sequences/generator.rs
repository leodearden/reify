//! Seeded random cases: a toy design of cells and, as features allow,
//! scopes, guards, collections, dividing cells, shapes and measures on the
//! toy kernel, and limit settings, and a sequence of host events on it,
//! whose edits may set or remove a limit setting. With dividing
//! cells on, every members input the generator makes ends in a pair: a
//! cell dividing by an input held at zero, and a sibling cell blocked by
//! it. With limits on, a members input at times holds, before that pair,
//! the deep pair: a collection, and a sibling cell reading its first
//! element, a read a depth setting can cut.
//! With cycles off every design is acyclic by construction (the ordinal
//! rule: member m_k reaches through its declarer only the declarer itself
//! or members m_j with j < k). With cycles on, a member cell's term may
//! name a sibling at its own ordinal or a later one, so a design may hold
//! rings of waits, which answer "cycle". A feature that is off draws
//! nothing from the rng, so a later leaf's feature leaves this feature
//! set's stream unchanged.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use reify_eval_core::{Limit, Step};

use super::case::{Case, HostEvent, Then, edited};
use super::design::{
    Decl, Inputs, Key, ModelPath, ToyKind, Val, bound, cell, collection, declaration, declarer,
    divided, flag, guard, index, input, int, measure, members, named, passed, path, picker, root,
    scope, shape, step,
};
use super::rng::XorShift64Star;

/// The seeds the gate runs, each for `GATE_EVENTS` host events.
pub const GATE_SEEDS: RangeInclusive<u64> = 1..=64;
pub const GATE_EVENTS: usize = 16;

/// The case G0 runs for `seed`: every feature, and `GATE_EVENTS` host
/// events.
pub fn gate_case(seed: u64) -> Case {
    generate(seed, Features::every(), GATE_EVENTS)
}

/// The gate's case for each of `GATE_SEEDS`, with its seed.
pub fn gate_cases() -> impl Iterator<Item = (u64, Case)> {
    GATE_SEEDS.map(|seed| (seed, gate_case(seed)))
}

/// Which toy kinds a design may use besides cells, and whether its cells
/// may divide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    pub scope: bool,
    pub guard: bool,
    pub collection: bool,
    /// Member cells binding a divisor.
    pub divide: bool,
    /// Shapes and measures, on the toy kernel.
    pub kernel: bool,
    /// Member cells naming a sibling at their own ordinal or a later one.
    pub cycles: bool,
    /// Limit settings, in the initial design and set or removed by edits.
    pub limits: bool,
}

impl Features {
    /// Every kind of structure, and nothing else.
    pub fn structure() -> Self {
        Self {
            scope: true,
            guard: true,
            collection: true,
            divide: false,
            kernel: false,
            cycles: false,
            limits: false,
        }
    }

    /// Every feature: the set the gate runs.
    pub fn every() -> Self {
        Self {
            divide: true,
            kernel: true,
            cycles: true,
            limits: true,
            ..Self::structure()
        }
    }
}

/// The case `seed` gives: a design using `features` and `events` host
/// events on it.
pub fn generate(seed: u64, features: Features, events: usize) -> Case {
    let mut generator = Generator {
        rng: XorShift64Star::new(seed),
        features,
        ints: Vec::new(),
        flags: Vec::new(),
        counts: Vec::new(),
        levels: Vec::new(),
        roots: Vec::new(),
        paths: Vec::new(),
        named: 0,
    };
    let inputs = generator.design();
    let events = generator.events(&inputs, events);
    Case { inputs, events }
}

/// The prefix of the name of each blocked pair's input held at zero.
const BLOCKED_PAIR_ZERO: &str = "z";

/// The deepest a node that declares members may sit: a root is at depth
/// 0, so the design nests at most three levels of nodes.
const DEEPEST_DECLARER: usize = 1;

/// A members input and the node that binds it.
struct Level {
    input: String,
    at: ModelPath,
    depth: usize,
    kind: ToyKind,
    /// One past the highest ordinal the members input has ever held.
    ordinals: usize,
}

impl Level {
    /// The ordinal and kind of each member of `held`, the level's members.
    ///
    /// # Panics
    ///
    /// If `held` holds a member at a step no ordinal of the level gives.
    fn siblings(&self, held: &BTreeMap<Step, Decl>) -> Vec<Sibling> {
        let present: Vec<Sibling> = (0..self.ordinals)
            .filter_map(|ordinal| {
                let decl = held.get(&member_step(ordinal))?;
                Some((ordinal, decl.kind))
            })
            .collect();
        assert_eq!(
            present.len(),
            held.len(),
            "{} holds a member at a step no ordinal below {} gives",
            self.input,
            self.ordinals
        );
        present
    }
}

/// A sibling member: its ordinal and its kind.
type Sibling = (usize, ToyKind);

/// Inputs a new declaration needs, set alongside it.
type Needed = Vec<(Key, Val)>;

/// What an edit changes.
#[derive(Clone, Copy)]
enum Change {
    Int,
    Flag,
    Count,
    Member,
    Remove,
    Root,
    Limit,
}

struct Generator {
    rng: XorShift64Star,
    features: Features,
    ints: Vec<String>,
    flags: Vec<String>,
    counts: Vec<String>,
    levels: Vec<Level>,
    roots: Vec<String>,
    /// Every path the design has declared, and a few it never declares.
    paths: Vec<ModelPath>,
    named: usize,
}

impl Generator {
    fn fresh(&mut self, prefix: &str) -> String {
        self.named += 1;
        format!("{prefix}{}", self.named)
    }

    fn int_value(&mut self) -> i64 {
        self.rng.below(13) as i64 - 3
    }

    fn some_int(&mut self) -> String {
        self.rng.choose(&self.ints).clone()
    }

    fn design(&mut self) -> Inputs {
        let mut needed = Needed::new();
        for _ in 0..3 + self.rng.below(3) {
            let name = self.fresh("i");
            needed.push((named(&name), int(self.int_value())));
            self.ints.push(name);
        }
        if self.features.guard {
            for _ in 0..1 + self.rng.below(2) {
                let name = self.fresh("f");
                needed.push((named(&name), flag(self.rng.chance(1, 2))));
                self.flags.push(name);
            }
        }
        for at in 0..1 + self.rng.below(3) {
            let name = format!("r{at}");
            let decl = self.root_decl(&name, &mut needed);
            needed.push((root(&name), declaration(decl)));
            self.roots.push(name);
        }
        self.paths.push(path("nowhere", []));
        if self.features.limits {
            for limit in [Limit::Depth, Limit::ChildrenPerNode] {
                if self.rng.chance(1, 2) {
                    needed.push((Key::Limit(limit), bound(self.limit_value(limit))));
                }
            }
        }
        needed.into_iter().collect()
    }

    /// A setting of `limit` that cuts into the design's nesting without
    /// failing most of it. Members sit at depth 1, their members and
    /// elements at 2, and those members' elements at 3. A level holds up
    /// to four drawn members, the deep pair and the blocked pair; a count
    /// reads up to 4.
    fn limit_value(&mut self, limit: Limit) -> u64 {
        let least = match limit {
            Limit::Depth => 1,
            Limit::ChildrenPerNode => 5,
        };
        least + self.rng.below(3) as u64
    }

    /// A root's declaration: mostly a scope.
    fn root_decl(&mut self, name: &str, needed: &mut Needed) -> Decl {
        let at = path(name, []);
        self.paths.push(at.clone());
        let mut kinds = vec![ToyKind::Cell];
        if self.features.scope {
            kinds.extend([ToyKind::Scope; 3]);
        }
        if self.features.guard {
            kinds.push(ToyKind::Guard);
        }
        if self.features.collection {
            kinds.push(ToyKind::Collection);
        }
        match *self.rng.choose(&kinds) {
            ToyKind::Cell => self.terms(&at, &[], None),
            ToyKind::Scope => self.scope_at(&at, 0, needed),
            ToyKind::Guard => self.guard_at(&at, 0, None, needed),
            ToyKind::Collection => self.collection_at(&at, &[], needed),
            ToyKind::Shape | ToyKind::Measure => unreachable!("a root is never on the kernel"),
        }
    }

    /// The declaration of member m`ordinal` of `level`, reading through
    /// its declarer only the declarer itself or the `earlier` siblings.
    fn member(
        &mut self,
        level: &Level,
        ordinal: usize,
        earlier: &[Sibling],
        needed: &mut Needed,
    ) -> Decl {
        let at = level.at.child(member_step(ordinal));
        self.paths.push(at.clone());
        let depth = level.depth + 1;
        let mut kinds = vec![ToyKind::Cell; 3];
        if self.features.scope && depth <= DEEPEST_DECLARER {
            kinds.push(ToyKind::Scope);
        }
        if self.features.guard && depth <= DEEPEST_DECLARER {
            kinds.push(ToyKind::Guard);
        }
        if self.features.collection {
            kinds.push(ToyKind::Collection);
        }
        match *self.rng.choose(&kinds) {
            ToyKind::Cell if self.features.kernel && self.rng.chance(1, 3) => {
                self.on_kernel(earlier)
            }
            ToyKind::Cell => self.member_cell(&at, level, earlier, needed),
            ToyKind::Scope => self.scope_at(&at, depth, needed),
            ToyKind::Guard => self.guard_at(&at, depth, Some(level.kind), needed),
            ToyKind::Collection => self.collection_at(&at, earlier, needed),
            ToyKind::Shape | ToyKind::Measure => unreachable!("the kernel takes a cell's place"),
        }
    }

    /// A measure of an earlier shape, or a shape over an int input.
    fn on_kernel(&mut self, earlier: &[Sibling]) -> Decl {
        let shapes = of_kind(earlier, ToyKind::Shape);
        if !shapes.is_empty() && self.rng.chance(1, 2) {
            return measure(declarer([member_step(*self.rng.choose(&shapes))]));
        }
        shape(input(&self.some_int()))
    }

    /// A picker of an earlier collection, or a cell of terms, as a member
    /// of `level`.
    fn member_cell(
        &mut self,
        at: &ModelPath,
        level: &Level,
        earlier: &[Sibling],
        needed: &mut Needed,
    ) -> Decl {
        let collections = of_kind(earlier, ToyKind::Collection);
        if self.features.collection && !collections.is_empty() && self.rng.chance(1, 2) {
            let cells = of_kind(earlier, ToyKind::Cell);
            let pick = if !cells.is_empty() && self.rng.chance(1, 3) {
                declarer([member_step(*self.rng.choose(&cells))])
            } else {
                input(&self.some_int())
            };
            let from = declarer([member_step(*self.rng.choose(&collections))]);
            return picker(pick, from);
        }
        let siblings = self.nameable(level, earlier);
        let terms = self.terms(at, &siblings, Some(level.kind));
        self.maybe_divided(terms, earlier, needed)
    }

    /// The ordinals of the siblings a term of a member of `level` may
    /// name: the `earlier` ones; with cycles on, at times every ordinal of
    /// the level, the member's own and later ones included.
    fn nameable(&mut self, level: &Level, earlier: &[Sibling]) -> Vec<usize> {
        if self.features.cycles && self.rng.chance(1, 2) {
            return (0..level.ordinals).collect();
        }
        earlier.iter().map(|(ordinal, _)| *ordinal).collect()
    }

    /// `cell`, or `cell` divided by an earlier sibling cell or by a new int
    /// input, either often zero.
    fn maybe_divided(&mut self, cell: Decl, earlier: &[Sibling], needed: &mut Needed) -> Decl {
        if !(self.features.divide && self.rng.chance(1, 2)) {
            return cell;
        }
        let cells = of_kind(earlier, ToyKind::Cell);
        if !cells.is_empty() && self.rng.chance(1, 2) {
            return divided(cell, declarer([member_step(*self.rng.choose(&cells))]));
        }
        let name = self.fresh("d");
        needed.push((named(&name), int(self.rng.below(3) as i64)));
        self.ints.push(name.clone());
        divided(cell, input(&name))
    }

    /// A cell of up to three terms: mostly inputs, else the siblings at
    /// the ordinals `siblings`, places below them, or the declarer itself.
    fn terms(
        &mut self,
        at: &ModelPath,
        siblings: &[usize],
        declarer_kind: Option<ToyKind>,
    ) -> Decl {
        if self.rng.chance(1, 4) {
            self.paths.push(at.child(step("x")));
        }
        let count = self.rng.below(4);
        let terms: Vec<_> = (0..count)
            .map(|_| match self.rng.below(10) {
                5..=7 if !siblings.is_empty() => {
                    declarer([member_step(*self.rng.choose(siblings))])
                }
                8 if !siblings.is_empty() => {
                    declarer([member_step(*self.rng.choose(siblings)), step("x")])
                }
                9 if declarer_kind.is_some() => declarer([]),
                9 => passed("members", []),
                _ => input(&self.some_int()),
            })
            .collect();
        cell(terms)
    }

    fn scope_at(&mut self, at: &ModelPath, depth: usize, needed: &mut Needed) -> Decl {
        let level = self.level(at, depth, ToyKind::Scope, needed);
        let decl = scope(&level.input);
        self.levels.push(level);
        decl
    }

    /// A guard on a flag, or, below another guard, on its condition.
    fn guard_at(
        &mut self,
        at: &ModelPath,
        depth: usize,
        declarer_kind: Option<ToyKind>,
        needed: &mut Needed,
    ) -> Decl {
        let condition = if declarer_kind == Some(ToyKind::Guard) && self.rng.chance(1, 2) {
            passed("condition", [])
        } else {
            input(&self.rng.choose(&self.flags).clone())
        };
        let level = self.level(at, depth, ToyKind::Guard, needed);
        let decl = guard(condition, &level.input);
        self.levels.push(level);
        decl
    }

    /// A new members input for the node at `at`: one to four drawn members,
    /// then, with limits on and at times, the deep pair, then, with divide
    /// on, the blocked pair.
    fn level(&mut self, at: &ModelPath, depth: usize, kind: ToyKind, needed: &mut Needed) -> Level {
        let count = 1 + self.rng.below(4);
        let mut level = Level {
            input: self.fresh("s"),
            at: at.clone(),
            depth,
            kind,
            ordinals: count,
        };
        let mut held = BTreeMap::new();
        let mut earlier = Vec::new();
        for ordinal in 0..count {
            let decl = self.member(&level, ordinal, &earlier, needed);
            earlier.push((ordinal, decl.kind));
            held.insert(member_step(ordinal), decl);
        }
        if self.features.limits && self.rng.chance(1, 2) {
            self.deep_pair(&mut level, &mut held, needed);
        }
        if self.features.divide {
            self.blocked_pair(&mut level, &mut held, needed);
        }
        needed.push((named(&level.input), members(held)));
        level
    }

    /// Appends to `held`, the members of `level`, a collection of one to
    /// four elements and a sibling cell whose only term reads its first
    /// element, one level deeper than the cell: a read a depth setting can
    /// cut while leaving the cell itself within it.
    fn deep_pair(
        &mut self,
        level: &mut Level,
        held: &mut BTreeMap<Step, Decl>,
        needed: &mut Needed,
    ) {
        let counted = member_step(level.ordinals);
        let reader = member_step(level.ordinals + 1);
        level.ordinals += 2;
        let count = self.fresh("n");
        needed.push((named(&count), int(1 + self.rng.below(4) as i64)));
        self.counts.push(count.clone());
        let element = self.fresh("e");
        needed.push((named(&element), declaration(self.element())));
        held.insert(counted.clone(), collection(input(&count), input(&element)));
        held.insert(
            reader.clone(),
            cell([declarer([counted.clone(), index(0)])]),
        );
        let elements = level.at.child(counted);
        self.paths
            .extend([elements.child(index(0)), elements, level.at.child(reader)]);
    }

    /// Appends to `held`, the members of `level`, a cell dividing by a new
    /// input held at zero and a sibling cell whose only term reads it.
    fn blocked_pair(
        &mut self,
        level: &mut Level,
        held: &mut BTreeMap<Step, Decl>,
        needed: &mut Needed,
    ) {
        let failing = level.ordinals;
        let blocked = failing + 1;
        level.ordinals += 2;
        let zero = self.fresh(BLOCKED_PAIR_ZERO);
        needed.push((named(&zero), int(0)));
        held.insert(member_step(failing), divided(cell([]), input(&zero)));
        held.insert(
            member_step(blocked),
            cell([declarer([member_step(failing)])]),
        );
        for ordinal in [failing, blocked] {
            self.paths.push(level.at.child(member_step(ordinal)));
        }
    }

    /// A collection counting a new input, or an earlier sibling cell, of
    /// new element declarations.
    fn collection_at(&mut self, at: &ModelPath, earlier: &[Sibling], needed: &mut Needed) -> Decl {
        let cells = of_kind(earlier, ToyKind::Cell);
        let count = if !cells.is_empty() && self.rng.chance(1, 3) {
            declarer([member_step(*self.rng.choose(&cells))])
        } else {
            let name = self.fresh("n");
            needed.push((named(&name), int(self.rng.below(5) as i64)));
            self.counts.push(name.clone());
            input(&name)
        };
        let element = self.fresh("e");
        needed.push((named(&element), declaration(self.element())));
        self.paths.push(at.child(index(0)));
        self.paths.push(at.child(index(self.rng.below(6) as u64)));
        collection(count, input(&element))
    }

    /// An element: a cell over inputs, its collection's count, or the
    /// collection itself.
    fn element(&mut self) -> Decl {
        let count = 1 + self.rng.below(3);
        let terms: Vec<_> = (0..count)
            .map(|_| match self.rng.below(6) {
                3 | 4 => passed("count", []),
                5 => declarer([]),
                _ => input(&self.some_int()),
            })
            .collect();
        cell(terms)
    }

    fn events(&mut self, initial: &Inputs, count: usize) -> Vec<HostEvent> {
        let mut snapshots = vec![initial.clone()];
        let mut events = Vec::new();
        let mut edit_driven = None;
        while events.len() < count {
            let event = match edit_driven.take() {
                Some(from) => self.edit(from, &snapshots[from]),
                None => match self.rng.below(5) {
                    0 | 1 => {
                        let from = self.snapshot(snapshots.len());
                        self.edit(from, &snapshots[from])
                    }
                    2 | 3 => HostEvent::Evaluate {
                        on: self.snapshot(snapshots.len()),
                        paths: self.demand(),
                        budget: None,
                        then: *self.rng.choose(&[Then::Leave, Then::End]),
                    },
                    _ => {
                        let on = self.snapshot(snapshots.len());
                        if events.len() + 1 < count {
                            edit_driven = Some(on);
                        }
                        HostEvent::Drive {
                            on,
                            paths: self.demand(),
                            turns: self.rng.below(3) as u8,
                            then: *self.rng.choose(&[Then::Leave, Then::End]),
                        }
                    }
                },
            };
            if let HostEvent::Edit { from, changes } = &event {
                snapshots.push(edited(&snapshots[*from], changes));
            }
            events.push(event);
        }
        events
    }

    /// A snapshot index below `made`, mostly the latest.
    fn snapshot(&mut self, made: usize) -> usize {
        if self.rng.chance(2, 3) {
            made - 1
        } else {
            self.rng.below(made)
        }
    }

    fn demand(&mut self) -> Vec<ModelPath> {
        (0..1 + self.rng.below(4))
            .map(|_| self.rng.choose(&self.paths).clone())
            .collect()
    }

    fn edit(&mut self, from: usize, current: &Inputs) -> HostEvent {
        let mut changes = Vec::new();
        for _ in 0..1 + self.rng.below(2) {
            changes.extend(self.change(current));
        }
        HostEvent::Edit { from, changes }
    }

    /// One change to `current`, with any inputs a new declaration needs.
    fn change(&mut self, current: &Inputs) -> Vec<(Key, Option<Val>)> {
        let mut sorts = vec![Change::Int, Change::Int, Change::Remove, Change::Root];
        if !self.flags.is_empty() {
            sorts.push(Change::Flag);
        }
        if !self.counts.is_empty() {
            sorts.push(Change::Count);
        }
        if !self.levels.is_empty() {
            sorts.extend([Change::Member; 3]);
        }
        if self.features.limits {
            sorts.push(Change::Limit);
        }
        let mut needed = Needed::new();
        let mut changes = match *self.rng.choose(&sorts) {
            Change::Int => {
                let name = self.some_int();
                vec![(named(&name), Some(int(self.int_value())))]
            }
            Change::Flag => {
                let name = self.rng.choose(&self.flags).clone();
                vec![(named(&name), Some(flag(self.rng.chance(1, 2))))]
            }
            Change::Count => {
                let name = self.rng.choose(&self.counts).clone();
                vec![(named(&name), Some(int(self.rng.below(5) as i64)))]
            }
            Change::Member => self.member_change(current, &mut needed),
            Change::Remove => {
                let present: Vec<_> = current
                    .keys()
                    .filter(|key| matches!(key, Key::Named(_)))
                    .collect();
                match present.is_empty() {
                    true => Vec::new(),
                    false => vec![((*self.rng.choose(&present)).clone(), None)],
                }
            }
            Change::Root => {
                let name = self.rng.choose(&self.roots).clone();
                let decl = self.root_decl(&name, &mut needed);
                vec![(root(&name), Some(declaration(decl)))]
            }
            Change::Limit => vec![self.limit_change(current)],
        };
        changes.extend(needed.into_iter().map(|(key, value)| (key, Some(value))));
        changes
    }

    /// A limit setting set to a new value, or, at times where it is set,
    /// removed.
    fn limit_change(&mut self, current: &Inputs) -> (Key, Option<Val>) {
        let limit = *self.rng.choose(&[Limit::Depth, Limit::ChildrenPerNode]);
        let key = Key::Limit(limit);
        if current.contains_key(&key) && self.rng.chance(1, 2) {
            return (key, None);
        }
        (key, Some(bound(self.limit_value(limit))))
    }

    /// A member of some level added at the next ordinal, replaced, or
    /// removed.
    fn member_change(&mut self, current: &Inputs, needed: &mut Needed) -> Vec<(Key, Option<Val>)> {
        let at = self.rng.below(self.levels.len());
        let mut level = self.levels.swap_remove(at);
        let key = named(&level.input);
        let mut held = match current.get(&key) {
            Some(Val::Members(held)) => held.clone(),
            _ => BTreeMap::new(),
        };
        let present = level.siblings(&held);
        match (self.rng.below(3), present.last()) {
            (1, Some(_)) => {
                let ordinal = self.rng.choose(&present).0;
                let decl = self.member(&level, ordinal, &before(&present, ordinal), needed);
                held.insert(member_step(ordinal), decl);
            }
            (2, Some(_)) => {
                held.remove(&member_step(self.rng.choose(&present).0));
            }
            (_, last) => {
                let ordinal = last.map_or(0, |(ordinal, _)| ordinal + 1);
                level.ordinals = level.ordinals.max(ordinal + 1);
                let decl = self.member(&level, ordinal, &present, needed);
                held.insert(member_step(ordinal), decl);
            }
        }
        self.levels.push(level);
        let last = self.levels.len() - 1;
        self.levels.swap(at, last);
        vec![(key, Some(Val::Members(held)))]
    }
}

fn member_step(ordinal: usize) -> Step {
    step(&format!("m{ordinal}"))
}

/// The members of `present` below `ordinal`.
fn before(present: &[Sibling], ordinal: usize) -> Vec<Sibling> {
    present
        .iter()
        .filter(|(at, _)| *at < ordinal)
        .copied()
        .collect()
}

/// The ordinals of the members of `kind` among `siblings`.
fn of_kind(siblings: &[Sibling], kind: ToyKind) -> Vec<usize> {
    siblings
        .iter()
        .filter(|(_, of)| *of == kind)
        .map(|(ordinal, _)| *ordinal)
        .collect()
}

mod tests {
    use std::collections::BTreeSet;

    use reify_eval_core::Failure;

    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::{Bind, Decl, Key, ToyKind, Val};
    use super::super::reference::{Expect, Invalid, Reference, first_invalid};
    use super::{
        BLOCKED_PAIR_ZERO, Features, GATE_EVENTS, GATE_SEEDS, gate_case, gate_cases, generate,
    };

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
                Val::Int(_) | Val::Bool(_) | Val::Bound(_) => Vec::new(),
            }
        }
        values(case)
            .into_iter()
            .flat_map(|(_, value)| within(value))
            .collect()
    }

    #[test]
    fn a_seed_gives_one_case_and_another_seed_another() {
        let features = Features::structure();
        assert_eq!(
            generate(7, features, GATE_EVENTS),
            generate(7, features, GATE_EVENTS)
        );
        assert_ne!(
            generate(7, features, GATE_EVENTS),
            generate(8, features, GATE_EVENTS)
        );
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
                    assert!(
                        edits_driven,
                        "seed {seed}: event {at} is a Drive on {on}, then {next:?}"
                    );
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
    fn the_gate_runs_every_feature() {
        let every = Features {
            scope: true,
            guard: true,
            collection: true,
            divide: true,
            kernel: true,
            cycles: true,
            limits: true,
            budgets: true,
        };
        assert_eq!(Features::every(), every);
        assert!(!Features::structure().cycles);
        assert!(!Features::structure().limits);
        assert!(!Features::structure().budgets);
        for seed in GATE_SEEDS {
            let case = generate(seed, every, GATE_EVENTS);
            assert_eq!(gate_case(seed), case, "seed {seed}");
        }
    }

    /// Whether `features` lets a design hold a declaration of `kind`.
    fn allows(features: Features, kind: ToyKind) -> bool {
        match kind {
            ToyKind::Cell => true,
            ToyKind::Scope => features.scope,
            ToyKind::Guard => features.guard,
            ToyKind::Collection => features.collection,
            ToyKind::Shape | ToyKind::Measure => features.kernel,
        }
    }

    #[test]
    fn a_feature_that_is_off_puts_no_declaration_of_its_kind_in_a_case() {
        let cells_only = Features {
            scope: false,
            guard: false,
            collection: false,
            divide: false,
            kernel: false,
            cycles: false,
            limits: false,
            budgets: false,
        };
        let every = Features::every();
        let some_off = [
            cells_only,
            Features::structure(),
            Features {
                divide: false,
                ..every
            },
            Features {
                kernel: false,
                ..every
            },
        ];
        for features in some_off {
            for seed in GATE_SEEDS {
                let case = generate(seed, features, GATE_EVENTS);
                assert!(!case.events.is_empty(), "seed {seed}");
                for decl in decls(&case) {
                    assert!(
                        allows(features, decl.kind),
                        "seed {seed}, {features:?}: {decl:?}"
                    );
                    let divides = decl.bindings.contains_key("divisor");
                    assert!(
                        features.divide || !divides,
                        "seed {seed}, {features:?}: {decl:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn with_cycles_off_no_case_answers_cycle() {
        let cycles_off = [
            Features::structure(),
            Features {
                cycles: false,
                ..Features::every()
            },
        ];
        for features in cycles_off {
            for seed in GATE_SEEDS {
                let case = generate(seed, features, GATE_EVENTS);
                let demanded: BTreeSet<_> = case.demanded().collect();
                for inputs in case.snapshots() {
                    let mut reference = Reference::new(inputs);
                    for path in &demanded {
                        let outcome = reference.answer(path).map(|answer| answer.outcome);
                        assert_ne!(
                            outcome,
                            Ok(Expect::Failure(Failure::Cycle)),
                            "seed {seed}, {features:?}: {path:?}"
                        );
                    }
                }
            }
        }
    }

    /// Every key an edit of `case` sets or removes.
    fn edited_keys(case: &Case) -> impl Iterator<Item = &Key> {
        case.events.iter().flat_map(|event| match event {
            HostEvent::Edit { changes, .. } => changes.iter().map(|(key, _)| key).collect(),
            _ => Vec::new(),
        })
    }

    fn is_limit(key: &&Key) -> bool {
        matches!(key, Key::Limit(_))
    }

    fn is_limit_failure(outcome: &Result<Expect, Invalid>) -> bool {
        matches!(
            outcome,
            Ok(Expect::Failure(
                Failure::TooDeep { .. } | Failure::TooManyChildren { .. }
            ))
        )
    }

    #[test]
    fn with_limits_off_no_case_holds_a_limit_key_or_answers_a_limit_failure() {
        let limits_off = [
            Features::structure(),
            Features {
                limits: false,
                ..Features::every()
            },
        ];
        for features in limits_off {
            for seed in GATE_SEEDS {
                let case = generate(seed, features, GATE_EVENTS);
                let mut keys = case.inputs.keys().chain(edited_keys(&case));
                let limit = keys.find(is_limit);
                assert_eq!(limit, None, "seed {seed}, {features:?}");
                let demanded: BTreeSet<_> = case.demanded().collect();
                for inputs in case.snapshots() {
                    let mut reference = Reference::new(inputs);
                    for path in &demanded {
                        let outcome = reference.answer(path).map(|answer| answer.outcome);
                        assert!(
                            !is_limit_failure(&outcome),
                            "seed {seed}, {features:?}: {path:?} answers {outcome:?}"
                        );
                    }
                }
            }
        }
    }

    /// The budget of each Evaluate of `case`.
    fn budgets(case: &Case) -> impl Iterator<Item = Option<u64>> {
        case.events.iter().filter_map(|event| match event {
            HostEvent::Evaluate { budget, .. } => Some(*budget),
            HostEvent::Edit { .. } | HostEvent::Drive { .. } => None,
        })
    }

    #[test]
    fn with_budgets_off_no_evaluate_has_a_budget() {
        let budgets_off = [
            Features::structure(),
            Features {
                budgets: false,
                ..Features::every()
            },
        ];
        for features in budgets_off {
            for seed in GATE_SEEDS {
                let case = generate(seed, features, GATE_EVENTS);
                let budgeted = budgets(&case).find(Option::is_some);
                assert_eq!(budgeted, None, "seed {seed}, {features:?}");
            }
        }
    }

    #[test]
    fn the_gate_cases_set_limits_initially_and_by_edits() {
        let (mut initial, mut edited) = (0, 0);
        for (_, case) in gate_cases() {
            initial += case.inputs.keys().filter(is_limit).count();
            edited += edited_keys(&case).filter(is_limit).count();
        }
        assert!(initial > 0, "no gate case starts with a limit setting");
        assert!(edited > 0, "no gate case edits a limit setting");
    }

    #[test]
    fn the_gate_cases_reach_every_kind_binding_and_event_shape() {
        let mut kinds = BTreeSet::new();
        let (mut pickers, mut passed) = (0, 0);
        let (mut drawn_divisors, mut blocked_pairs) = (0, 0);
        let (mut edits_off_latest, mut evaluations_off_latest) = (0, 0);
        let (mut zero_left, mut zero_ended) = (0, 0);
        let mut budgeted = 0;
        for (_, case) in gate_cases() {
            budgeted += budgets(&case).filter(Option::is_some).count();
            for decl in decls(&case) {
                kinds.insert(decl.kind);
                pickers +=
                    usize::from(decl.kind == ToyKind::Cell && decl.bindings.contains_key("pick"));
                match (decl.kind, decl.bindings.get("divisor")) {
                    (ToyKind::Cell, Some(Bind::Input(name)))
                        if name.starts_with(BLOCKED_PAIR_ZERO) =>
                    {
                        blocked_pairs += 1
                    }
                    (ToyKind::Cell, Some(_)) => drawn_divisors += 1,
                    _ => {}
                }
                passed += decl
                    .bindings
                    .values()
                    .filter(|bind| matches!(bind, Bind::Passed(_)))
                    .count();
            }
            let mut latest = 0;
            for event in &case.events {
                match event {
                    HostEvent::Edit { from, .. } => {
                        edits_off_latest += usize::from(*from != latest);
                        latest += 1;
                    }
                    HostEvent::Evaluate { on, .. } => {
                        evaluations_off_latest += usize::from(*on != latest)
                    }
                    HostEvent::Drive {
                        turns: 0,
                        then: Then::Leave,
                        ..
                    } => zero_left += 1,
                    HostEvent::Drive {
                        turns: 0,
                        then: Then::End,
                        ..
                    } => zero_ended += 1,
                    HostEvent::Drive { .. } => {}
                }
            }
        }
        let all = [
            ToyKind::Cell,
            ToyKind::Scope,
            ToyKind::Guard,
            ToyKind::Collection,
            ToyKind::Shape,
            ToyKind::Measure,
        ];
        assert_eq!(kinds, BTreeSet::from(all));
        let counts = [
            ("pickers", pickers),
            ("drawn divisors", drawn_divisors),
            ("blocked pairs", blocked_pairs),
            ("passed bindings", passed),
            ("edits off the latest snapshot", edits_off_latest),
            (
                "evaluations off the latest snapshot",
                evaluations_off_latest,
            ),
            ("drives of no turns left live", zero_left),
            ("drives of no turns ended", zero_ended),
            ("evaluations with a budget", budgeted),
        ];
        for (shape, count) in counts {
            assert!(count > 0, "the gate cases hold no {shape}");
        }
    }
}
