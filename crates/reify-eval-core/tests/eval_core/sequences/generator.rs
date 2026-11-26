//! Seeded random cases: a toy design of cells and, as features allow,
//! scopes, guards and collections, and a sequence of host events on it.
//! Every design is acyclic by construction (the ordinal rule: member m_k
//! reaches through its declarer only the declarer itself or members m_j
//! with j < k), and a feature that is off draws nothing from the rng, so
//! a later leaf's feature leaves this feature set's stream unchanged.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use reify_eval_core::Step;

use super::case::{Case, HostEvent, Then, edited};
use super::design::{
    Decl, Inputs, Key, ModelPath, ToyKind, Val, attached_to, cell, collection, declaration,
    declarer, flag, guard, index, input, int, members, named, passed, path, picker, root, scope,
    step,
};
use super::rng::XorShift64Star;

/// The seeds the gate runs, each for `GATE_EVENTS` host events.
pub const GATE_SEEDS: RangeInclusive<u64> = 1..=64;
pub const GATE_EVENTS: usize = 16;

/// The case G0 runs for `seed`: every feature the gate switches on, and
/// `GATE_EVENTS` host events.
pub fn gate_case(seed: u64) -> Case {
    generate(seed, Features::structure(), GATE_EVENTS)
}

/// The gate's case for each of `GATE_SEEDS`, with its seed.
pub fn gate_cases() -> impl Iterator<Item = (u64, Case)> {
    GATE_SEEDS.map(|seed| (seed, gate_case(seed)))
}

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
    Extra,
    Remove,
    Root,
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
        needed.into_iter().collect()
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
            ToyKind::Cell => self.member_cell(&at, level.kind, earlier),
            ToyKind::Scope => self.scope_at(&at, depth, needed),
            ToyKind::Guard => self.guard_at(&at, depth, Some(level.kind), needed),
            ToyKind::Collection => self.collection_at(&at, earlier, needed),
        }
    }

    /// A picker of an earlier collection, or a cell of terms.
    fn member_cell(&mut self, at: &ModelPath, declarer_kind: ToyKind, earlier: &[Sibling]) -> Decl {
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
        self.terms(at, earlier, Some(declarer_kind))
    }

    /// A cell of up to three terms: mostly inputs, else earlier siblings,
    /// places below them, or the declarer itself.
    fn terms(
        &mut self,
        at: &ModelPath,
        earlier: &[Sibling],
        declarer_kind: Option<ToyKind>,
    ) -> Decl {
        if self.rng.chance(1, 4) {
            self.paths.push(at.child(step("x")));
        }
        let count = self.rng.below(4);
        let terms: Vec<_> = (0..count)
            .map(|_| match self.rng.below(10) {
                5..=7 if !earlier.is_empty() => declarer([member_step(self.rng.choose(earlier).0)]),
                8 if !earlier.is_empty() => {
                    declarer([member_step(self.rng.choose(earlier).0), step("x")])
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
        if self.rng.chance(1, 4) {
            let extra = self.extra(&level, &BTreeMap::new(), needed);
            needed.push((attached_to(at.clone(), "extra"), extra));
        }
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

    /// A new members input of one to four members, for the node at `at`.
    fn level(&mut self, at: &ModelPath, depth: usize, kind: ToyKind, needed: &mut Needed) -> Level {
        let count = 1 + self.rng.below(4);
        let level = Level {
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
        needed.push((named(&level.input), members(held)));
        level
    }

    /// Extra members for `level`, whose members are `held`: each at an
    /// ordinal up to one past the last, over a member or beside them.
    fn extra(&mut self, level: &Level, held: &BTreeMap<Step, Decl>, needed: &mut Needed) -> Val {
        let present = level.siblings(held);
        let mut extra = BTreeMap::new();
        for _ in 0..1 + self.rng.below(2) {
            let ordinal = self.rng.below(present.len() + 1);
            let earlier = before(&present, ordinal);
            let decl = self.member(level, ordinal, &earlier, needed);
            extra.insert(member_step(ordinal), decl);
        }
        Val::Members(extra)
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
        if self.levels.iter().any(|level| level.kind == ToyKind::Scope) {
            sorts.push(Change::Extra);
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
            Change::Extra => self.extra_change(current, &mut needed),
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
        };
        changes.extend(needed.into_iter().map(|(key, value)| (key, Some(value))));
        changes
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

    /// The extra members attached to some scope set anew, or removed.
    fn extra_change(&mut self, current: &Inputs, needed: &mut Needed) -> Vec<(Key, Option<Val>)> {
        let scopes: Vec<_> = (0..self.levels.len())
            .filter(|at| self.levels[*at].kind == ToyKind::Scope)
            .collect();
        let at = *self.rng.choose(&scopes);
        let level = self.levels.swap_remove(at);
        let key = attached_to(level.at.clone(), "extra");
        let change = if current.contains_key(&key) && self.rng.chance(1, 3) {
            None
        } else {
            let held = match current.get(&named(&level.input)) {
                Some(Val::Members(held)) => held.clone(),
                _ => BTreeMap::new(),
            };
            Some(self.extra(&level, &held, needed))
        };
        self.levels.push(level);
        let last = self.levels.len() - 1;
        self.levels.swap(at, last);
        vec![(key, change)]
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

    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::{Bind, Decl, Key, ToyKind, Val};
    use super::super::reference::first_invalid;
    use super::{Features, GATE_EVENTS, GATE_SEEDS, gate_case, gate_cases, generate};

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
        };
        assert_eq!(Features::every(), every);
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
    fn the_gate_cases_reach_every_kind_binding_and_event_shape() {
        let mut kinds = BTreeSet::new();
        let (mut pickers, mut passed, mut extras, mut divisors) = (0, 0, 0, 0);
        let (mut edits_off_latest, mut evaluations_off_latest) = (0, 0);
        let (mut zero_left, mut zero_ended) = (0, 0);
        for (_, case) in gate_cases() {
            for decl in decls(&case) {
                kinds.insert(decl.kind);
                pickers +=
                    usize::from(decl.kind == ToyKind::Cell && decl.bindings.contains_key("pick"));
                divisors += usize::from(
                    decl.kind == ToyKind::Cell && decl.bindings.contains_key("divisor"),
                );
                passed += decl
                    .bindings
                    .values()
                    .filter(|bind| matches!(bind, Bind::Passed(_)))
                    .count();
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
            ("divisors", divisors),
            ("passed bindings", passed),
            ("attached extras", extras),
            ("edits off the latest snapshot", edits_off_latest),
            (
                "evaluations off the latest snapshot",
                evaluations_off_latest,
            ),
            ("drives of no turns left live", zero_left),
            ("drives of no turns ended", zero_ended),
        ];
        for (shape, count) in counts {
            assert!(count > 0, "the gate cases hold no {shape}");
        }
    }
}
