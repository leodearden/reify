//! Runs a case on a core, as a host would, and checks what the core tells
//! it: every final answer equals the reference interpreter's for its
//! snapshot and path (check 1 of PRD 5.5), every request the core leaves
//! idle holds one answer per demanded path, and a fresh core asked for the
//! same paths in another order answers the same (the order check). What
//! the run reached is counted, so a test can show it is not vacuous.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use reify_eval_core::{
    Answer, Core, Demand, Erased, Event, Failure, Kind, Path, Request, RequestError, RequestId,
    Snapshot, Step, Store,
};

use super::case::{Case, HostEvent, Then};
use super::design::{Inputs, ModelPath, ModelRef, ToyKind, Val, to_core};
use super::fresh::{Answered, FreshMismatch, Order, fresh_check};
use super::reference::{Expect, Expected, Invalid, Out, Reference};
use super::seen::Seen;
use crate::toy;

/// Declares `Shape` and `Shape::ALL` from one list, so every shape the
/// enum names is one the census looks for.
macro_rules! shapes {
    ($($(#[$doc:meta])* $shape:ident,)*) => {
        /// A shape of design or event a run reached, counted to show the run
        /// is not vacuous (PRD 12.1 rule 9).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Shape {
            $($(#[$doc])* $shape,)*
        }

        impl Shape {
            /// Every shape, in the order declared.
            pub const ALL: &'static [Shape] = &[$(Shape::$shape),*];
        }
    };
}

shapes! {
    /// A cell's value.
    CellValue,
    ScopeValue,
    GuardValue,
    /// A guard answering absent because its condition reads false.
    GuardAbsentOnFalse,
    /// A path below a guard that answers absent.
    UnderAbsentGuard,
    CollectionValue,
    /// A node a collection declares at an index.
    CollectionElement,
    /// An index at or past a collection's count, answering absent.
    IndexPastCount,
    /// The value of a cell that picks an element.
    PickerValue,
    /// A failure naming the type read and the type found.
    WrongType,
    /// A cell answering its own failure: its divisor reads zero.
    DivisionByZero,
    /// A cell blocked by the failure at a ref it read.
    BlockedBy,
    /// A box a shape made on the toy kernel.
    BoxValue,
    /// The volume a measure read from the toy kernel.
    MeasureValue,
    /// An answer checked for a request left live after a Drive of no
    /// turns, told after a later event.
    LeftLiveAnswer,
    /// A Drive of no turns whose request is then ended.
    DriveZeroEnded,
    /// An Edit from a snapshot other than the latest.
    EditFromNonLatest,
    /// An answer the order check compared.
    OrderComparison,
}

/// What a run reached.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    /// Final answers checked, by snapshot index.
    checked: BTreeMap<usize, u64>,
    shapes: BTreeMap<Shape, u64>,
    /// Distinct paths of a snapshot answered.
    answered: u64,
}

impl Reach {
    fn reached(&mut self, shape: Shape) {
        *self.shapes.entry(shape).or_default() += 1;
    }

    /// How many final answers were checked on snapshot `snapshot`.
    pub fn checked_on(&self, snapshot: usize) -> u64 {
        self.checked.get(&snapshot).copied().unwrap_or(0)
    }

    /// How many distinct paths of a snapshot were answered.
    pub fn answered(&self) -> u64 {
        self.answered
    }

    /// How many answers the order check compared.
    pub fn order_compared(&self) -> u64 {
        self.count(Shape::OrderComparison)
    }

    /// Adds what another run reached.
    pub fn add(&mut self, other: &Reach) {
        for (snapshot, checked) in &other.checked {
            *self.checked.entry(*snapshot).or_default() += checked;
        }
        for (shape, count) in &other.shapes {
            *self.shapes.entry(*shape).or_default() += count;
        }
        self.answered += other.answered;
    }

    /// The shapes never reached, in the order of `Shape::ALL`.
    pub fn missing(&self) -> Vec<Shape> {
        Shape::ALL
            .iter()
            .copied()
            .filter(|shape| self.count(*shape) == 0)
            .collect()
    }

    /// How often `shape` was reached.
    pub fn count(&self, shape: Shape) -> u64 {
        self.shapes.get(&shape).copied().unwrap_or(0)
    }
}

/// What a check found wrong. `event` is the index of the host event after
/// which it was seen, `snapshot` the index of the snapshot concerned.
#[derive(Debug, Clone, PartialEq)]
pub enum Mismatch {
    /// The core's answer is not the reference interpreter's.
    Reference {
        event: usize,
        snapshot: usize,
        path: ModelPath,
        core: Box<Seen>,
        reference: Box<Seen>,
    },
    /// The core is idle and a live request holds no answer at a path it
    /// demanded.
    Unanswered {
        event: usize,
        snapshot: usize,
        path: ModelPath,
    },
    /// An answer at a path its request did not demand.
    Unasked {
        event: usize,
        snapshot: usize,
        path: Path,
    },
    /// A second answer at one path of one request.
    Twice {
        event: usize,
        snapshot: usize,
        path: ModelPath,
    },
    /// The core ended a request.
    Ended {
        event: usize,
        snapshot: usize,
        error: RequestError,
    },
    /// A fresh core built on the session core's selection (check 2),
    /// asked in the session's order, answered otherwise.
    Fresh {
        snapshot: usize,
        path: ModelPath,
        session: Box<Seen>,
        fresh: Box<Seen>,
    },
    /// A fresh core built on the session core's selection told no answer
    /// at a path the session answered: it ended the request with `ended`,
    /// or left it with neither.
    FreshUnanswered {
        snapshot: usize,
        path: ModelPath,
        ended: Option<RequestError>,
    },
    /// A fresh core, asked in another order, answered otherwise.
    Order {
        snapshot: usize,
        path: ModelPath,
        session: Box<Seen>,
        fresh: Box<Seen>,
    },
    /// A fresh core, asked in another order, told no answer at a path the
    /// session answered: it ended the request with `ended`, or left it
    /// with neither.
    OrderUnanswered {
        snapshot: usize,
        path: ModelPath,
        ended: Option<RequestError>,
    },
    /// The reference interpreter has no answer: the case is not one the
    /// checks can judge.
    Invalid {
        event: usize,
        snapshot: usize,
        invalid: Invalid,
    },
}

/// Which check a mismatch failed, for keeping a shrunk case on the same
/// fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    Reference,
    Unanswered,
    Unasked,
    Twice,
    Ended,
    Fresh,
    FreshUnanswered,
    Order,
    OrderUnanswered,
    Invalid,
}

impl Mismatch {
    pub fn kind(&self) -> MismatchKind {
        match self {
            Mismatch::Reference { .. } => MismatchKind::Reference,
            Mismatch::Unanswered { .. } => MismatchKind::Unanswered,
            Mismatch::Unasked { .. } => MismatchKind::Unasked,
            Mismatch::Twice { .. } => MismatchKind::Twice,
            Mismatch::Ended { .. } => MismatchKind::Ended,
            Mismatch::Fresh { .. } => MismatchKind::Fresh,
            Mismatch::FreshUnanswered { .. } => MismatchKind::FreshUnanswered,
            Mismatch::Order { .. } => MismatchKind::Order,
            Mismatch::OrderUnanswered { .. } => MismatchKind::OrderUnanswered,
            Mismatch::Invalid { .. } => MismatchKind::Invalid,
        }
    }
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Reference {
                event,
                snapshot,
                path,
                core,
                reference,
            } => write!(
                f,
                "after event {event}, at {path:?} on snapshot {snapshot}: the core answered \
                 {core:?}, the reference interpreter {reference:?}"
            ),
            Mismatch::Unanswered {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, the core is idle with no answer at {path:?} on snapshot \
                 {snapshot}"
            ),
            Mismatch::Unasked {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, an answer at {path:?} on snapshot {snapshot}, which its \
                 request did not demand"
            ),
            Mismatch::Twice {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, a second answer at {path:?} on snapshot {snapshot} to one \
                 request"
            ),
            Mismatch::Ended {
                event,
                snapshot,
                error,
            } => write!(
                f,
                "after event {event}, the core ended a request on snapshot {snapshot}: {error:?}"
            ),
            Mismatch::Fresh {
                snapshot,
                path,
                session,
                fresh,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: the session answered {session:?}, a fresh \
                 core built on the session's selection {fresh:?}"
            ),
            Mismatch::FreshUnanswered {
                snapshot,
                path,
                ended,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: a fresh core built on the session's \
                 selection told no answer, the request ended with {ended:?}"
            ),
            Mismatch::Order {
                snapshot,
                path,
                session,
                fresh,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: the session answered {session:?}, a fresh \
                 core asked in reverse order {fresh:?}"
            ),
            Mismatch::OrderUnanswered {
                snapshot,
                path,
                ended,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: a fresh core asked in reverse order told no \
                 answer, the request ended with {ended:?}"
            ),
            Mismatch::Invalid {
                event,
                snapshot,
                invalid,
            } => write!(
                f,
                "after event {event}, snapshot {snapshot} has no reference answer: {invalid:?}"
            ),
        }
    }
}

/// A core with every honest toy kind and the toy kernel, built on
/// `store`.
pub fn toy_core(store: Store) -> Core {
    toy::ungated(
        toy::with_every_kind().on_store(store),
        &toy::Watch::default(),
    )
}

/// Which check asked a fresh core.
#[derive(Clone, Copy)]
enum FreshCheck {
    /// Check 2: on the session core's selection, in the session's order.
    Selection,
    /// The order check: on an empty store, latest first.
    Order,
}

/// Runs `case` on a core `make_core` builds on an empty store, checking
/// after every host event; then check 2 and the order check, each on a
/// core `make_core` builds on the store that check names.
pub fn check(case: &Case, make_core: &dyn Fn(Store) -> Core) -> Result<Reach, Mismatch> {
    let mut session = Session::new(case, make_core(Store::default()));
    for (at, event) in case.events.iter().enumerate() {
        session.host_event(at, event)?;
    }
    session.finish(case.events.len())?;
    session.fresh_check(FreshCheck::Selection, make_core)?;
    session.fresh_check(FreshCheck::Order, make_core)?;
    Ok(session.reach)
}

/// A request the session opened, live while it holds the handle.
struct Opened {
    handle: Option<Request>,
    snapshot: usize,
    /// The host event that opened it.
    event: usize,
    /// Whether a Drive left it live, unfinished.
    left_live: bool,
    demanded: BTreeMap<Path, ModelPath>,
    told: BTreeSet<Path>,
}

struct Session {
    core: Core,
    models: Vec<Inputs>,
    snapshots: Vec<Snapshot>,
    references: Vec<Option<Reference>>,
    requests: BTreeMap<RequestId, Opened>,
    /// The first final answer at each path of each snapshot, in order.
    first: Vec<Answered<Seen>>,
    answered: BTreeSet<(usize, Path)>,
    reach: Reach,
}

impl Session {
    fn new(case: &Case, mut core: Core) -> Self {
        let models = case.snapshots();
        let initial = core.snapshot(to_core(&models[0]));
        Self {
            references: models.iter().map(|_| None).collect(),
            models,
            snapshots: vec![initial],
            core,
            requests: BTreeMap::new(),
            first: Vec::new(),
            answered: BTreeSet::new(),
            reach: Reach::default(),
        }
    }

    fn host_event(&mut self, at: usize, event: &HostEvent) -> Result<(), Mismatch> {
        match event {
            HostEvent::Edit { from, changes } => {
                if *from + 1 != self.snapshots.len() {
                    self.reach.reached(Shape::EditFromNonLatest);
                }
                let changed = changes
                    .iter()
                    .map(|(key, value)| (key.to_core(), value.as_ref().map(Val::to_core)));
                let next = self.snapshot(at, *from).edit(changed);
                self.snapshots.push(next);
                self.take_events(at)
            }
            HostEvent::Evaluate { on, paths, then } => {
                let request = self.open(at, *on, paths, false);
                self.core.run();
                self.close(request, *then);
                self.take_events(at)?;
                self.complete(at, Some(request))
            }
            HostEvent::Drive {
                on,
                paths,
                turns,
                then,
            } => {
                let request = self.open(at, *on, paths, *then == Then::Leave);
                for _ in 0..*turns {
                    self.core.turn();
                }
                if *turns == 0 && *then == Then::End {
                    self.reach.reached(Shape::DriveZeroEnded);
                }
                self.close(request, *then);
                self.take_events(at)
            }
        }
    }

    /// After the last event: runs what is left, checks it, and ends every
    /// live request.
    fn finish(&mut self, at: usize) -> Result<(), Mismatch> {
        self.core.run();
        self.take_events(at)?;
        self.complete(at, None)?;
        let live: Vec<_> = self.requests.keys().copied().collect();
        for request in live {
            self.close(request, Then::End);
        }
        self.core.drain();
        Ok(())
    }

    /// A fresh core asked for every answered path gives the same answers.
    /// Check 2 builds it on the session core's selection and asks in the
    /// session's order. Every result is treated as deterministic until leaf
    /// ο, so none is selected; ο selects those that are not. The order
    /// check builds it on an empty store and asks latest first.
    fn fresh_check(
        &mut self,
        check: FreshCheck,
        make_core: &dyn Fn(Store) -> Core,
    ) -> Result<(), Mismatch> {
        let (core, order) = match check {
            FreshCheck::Selection => (
                make_core(self.core.selected_store(|_| false)),
                Order::Session,
            ),
            FreshCheck::Order => (make_core(Store::default()), Order::Reverse),
        };
        let snapshots: Vec<_> = self.models.iter().map(to_core).collect();
        let compared = fresh_check(core, order, &snapshots, &self.first, Seen::of_answer)
            .map_err(|mismatch| self.fresh_mismatch(check, mismatch))?;
        if let FreshCheck::Order = check {
            let compared = u64::try_from(compared).expect("a count fits u64");
            *self.reach.shapes.entry(Shape::OrderComparison).or_default() += compared;
        }
        Ok(())
    }

    /// `mismatch`, found by `check`, at the model path the session
    /// demanded.
    fn fresh_mismatch(&self, check: FreshCheck, mismatch: FreshMismatch<Seen>) -> Mismatch {
        match mismatch {
            FreshMismatch::Differs {
                snapshot,
                path,
                session,
                fresh,
            } => {
                let (path, session, fresh) =
                    (self.model_path(&path), Box::new(session), Box::new(fresh));
                match check {
                    FreshCheck::Selection => Mismatch::Fresh {
                        snapshot,
                        path,
                        session,
                        fresh,
                    },
                    FreshCheck::Order => Mismatch::Order {
                        snapshot,
                        path,
                        session,
                        fresh,
                    },
                }
            }
            FreshMismatch::Unanswered {
                snapshot,
                path,
                ended,
            } => {
                let path = self.model_path(&path);
                match check {
                    FreshCheck::Selection => Mismatch::FreshUnanswered {
                        snapshot,
                        path,
                        ended,
                    },
                    FreshCheck::Order => Mismatch::OrderUnanswered {
                        snapshot,
                        path,
                        ended,
                    },
                }
            }
        }
    }

    /// The model path a request demanded as `path`.
    fn model_path(&self, path: &Path) -> ModelPath {
        self.requests
            .values()
            .find_map(|opened| opened.demanded.get(path).cloned())
            .expect("an answered path was demanded")
    }

    fn snapshot(&self, at: usize, index: usize) -> &Snapshot {
        self.snapshots
            .get(index)
            .unwrap_or_else(|| panic!("event {at} names snapshot {index}, which is not made yet"))
    }

    fn open(&mut self, at: usize, on: usize, paths: &[ModelPath], left_live: bool) -> RequestId {
        let demanded: BTreeMap<_, _> = paths
            .iter()
            .map(|path| (path.to_core(), path.clone()))
            .collect();
        let snapshot = self.snapshot(at, on).clone();
        let handle = self
            .core
            .evaluate(&snapshot, Demand::of(demanded.keys().cloned()));
        let id = handle.id();
        let opened = Opened {
            handle: Some(handle),
            snapshot: on,
            event: at,
            left_live,
            demanded,
            told: BTreeSet::new(),
        };
        self.requests.insert(id, opened);
        id
    }

    fn close(&mut self, request: RequestId, then: Then) {
        if then == Then::End {
            let opened = self.requests.get_mut(&request);
            if let Some(handle) = opened.and_then(|opened| opened.handle.take()) {
                self.core.end(handle);
            }
        }
    }

    fn take_events(&mut self, at: usize) -> Result<(), Mismatch> {
        for event in self.core.take_events() {
            match event {
                Event::Answered {
                    request,
                    path,
                    answer,
                    ..
                } => self.answered(at, request, path, answer)?,
                Event::Ended { request, error, .. } => {
                    let snapshot = self.opened(request).snapshot;
                    return Err(Mismatch::Ended {
                        event: at,
                        snapshot,
                        error,
                    });
                }
                Event::Progress { .. } => {}
            }
        }
        Ok(())
    }

    fn opened(&mut self, request: RequestId) -> &mut Opened {
        self.requests
            .get_mut(&request)
            .expect("the core tells only requests this session opened")
    }

    /// Checks the answer `request` was told at `path` after event `at`.
    fn answered(
        &mut self,
        at: usize,
        request: RequestId,
        path: Path,
        answer: Answer<Erased>,
    ) -> Result<(), Mismatch> {
        let opened = self.opened(request);
        let snapshot = opened.snapshot;
        let late_for_left_live = opened.left_live && opened.event < at;
        let Some(model_path) = opened.demanded.get(&path).cloned() else {
            return Err(Mismatch::Unasked {
                event: at,
                snapshot,
                path,
            });
        };
        if !opened.told.insert(path.clone()) {
            return Err(Mismatch::Twice {
                event: at,
                snapshot,
                path: model_path,
            });
        }
        let expected = self
            .reference(snapshot)
            .answer(&model_path)
            .map_err(|invalid| Mismatch::Invalid {
                event: at,
                snapshot,
                invalid,
            })?;
        let (core, reference) = (
            Seen::of_answer(answer),
            Seen::of_expected(&expected.outcome),
        );
        if core != reference {
            return Err(Mismatch::Reference {
                event: at,
                snapshot,
                path: model_path,
                core: Box::new(core),
                reference: Box::new(reference),
            });
        }
        *self.reach.checked.entry(snapshot).or_default() += 1;
        self.census(snapshot, &model_path, &expected);
        if late_for_left_live {
            self.reach.reached(Shape::LeftLiveAnswer);
        }
        if self.answered.insert((snapshot, path.clone())) {
            self.reach.answered += 1;
            self.first.push(Answered {
                snapshot,
                path,
                seen: core,
            });
        }
        Ok(())
    }

    /// Counts the shapes the answer `expected` at `path` reached: from the
    /// node's kind and outcome, its declaration, and its parent's answer.
    fn census(&mut self, snapshot: usize, path: &ModelPath, expected: &Expected) {
        let reference = self.reference(snapshot);
        let mut reached = Vec::new();
        match (expected.node, &expected.outcome) {
            (Some(ToyKind::Cell), Expect::Value(_)) => {
                reached.push(Shape::CellValue);
                let decl = reference.declaration_at(path).ok().flatten();
                if decl.is_some_and(|decl| decl.bindings.contains_key("pick")) {
                    reached.push(Shape::PickerValue);
                }
            }
            (Some(ToyKind::Scope), Expect::Value(_)) => reached.push(Shape::ScopeValue),
            (Some(ToyKind::Guard), Expect::Value(_)) => reached.push(Shape::GuardValue),
            (Some(ToyKind::Guard), Expect::Absent) => {
                let condition = ModelRef {
                    binding: String::from("condition"),
                    steps: Vec::new(),
                };
                if reference.read_at(path, &condition) == Ok(Expect::Value(Out::Bool(false))) {
                    reached.push(Shape::GuardAbsentOnFalse);
                }
            }
            (Some(ToyKind::Collection), Expect::Value(_)) => reached.push(Shape::CollectionValue),
            (Some(ToyKind::Shape), Expect::Value(_)) => reached.push(Shape::BoxValue),
            (Some(ToyKind::Measure), Expect::Value(_)) => reached.push(Shape::MeasureValue),
            (_, Expect::Failure(Failure::WrongType { .. })) => reached.push(Shape::WrongType),
            (Some(ToyKind::Cell), Expect::Failure(Failure::OfKind { kind, code }))
                if *kind == toy::Cell::ID && *code == toy::DIVISION_BY_ZERO =>
            {
                reached.push(Shape::DivisionByZero)
            }
            (Some(ToyKind::Cell), Expect::Failure(Failure::BlockedBy { .. })) => {
                reached.push(Shape::BlockedBy)
            }
            _ => {}
        }
        if let Some((parent, step)) = path.parent()
            && let Ok(above) = reference.answer(&parent)
        {
            match (above.node, &above.outcome, step) {
                (Some(ToyKind::Guard), Expect::Absent, _) => reached.push(Shape::UnderAbsentGuard),
                (
                    Some(ToyKind::Collection),
                    Expect::Value(Out::Elements { count, .. }),
                    Step::Index(at),
                ) => {
                    if expected.node.is_some() {
                        reached.push(Shape::CollectionElement);
                    }
                    if at >= count {
                        reached.push(Shape::IndexPastCount);
                    }
                }
                _ => {}
            }
        }
        for shape in reached {
            self.reach.reached(shape);
        }
    }

    fn reference(&mut self, snapshot: usize) -> &mut Reference {
        let model = &self.models[snapshot];
        self.references[snapshot].get_or_insert_with(|| Reference::new(model.clone()))
    }

    /// Every live request, and `this` request, holds an answer at each
    /// path it demanded: the core is idle, so none is still coming.
    fn complete(&self, at: usize, this: Option<RequestId>) -> Result<(), Mismatch> {
        let owed = self
            .requests
            .iter()
            .filter(|(id, opened)| opened.handle.is_some() || Some(**id) == this);
        for (_, opened) in owed {
            let missing = opened
                .demanded
                .iter()
                .find(|(path, _)| !opened.told.contains(*path));
            if let Some((_, path)) = missing {
                return Err(Mismatch::Unanswered {
                    event: at,
                    snapshot: opened.snapshot,
                    path: path.clone(),
                });
            }
        }
        Ok(())
    }
}

mod tests {
    use reify_eval_core::{
        Builder, Core, Cx, Kind, KindId, Outcome, OutsideFailure, Produced, Ref, RequestError,
        Store,
    };

    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::*;
    use super::super::reference::Invalid;
    use super::super::seen::Seen;
    use super::{Mismatch, Shape, check, toy_core};
    use crate::toy;

    /// Root scope "r" with a cell "a" over "x" and a cell "b" adding "a"
    /// and "y".
    fn design() -> Inputs {
        inputs([
            (root("r"), declaration(scope("r.m"))),
            (
                named("r.m"),
                members([
                    (step("a"), cell([input("x")])),
                    (step("b"), cell([declarer([step("a")]), input("y")])),
                ]),
            ),
            (named("x"), int(1)),
            (named("y"), int(2)),
        ])
    }

    fn r(steps: &[&str]) -> ModelPath {
        path("r", steps.iter().map(|name| step(name)))
    }

    fn edit_x(from: usize, x: i64) -> HostEvent {
        HostEvent::Edit {
            from,
            changes: vec![(named("x"), Some(int(x)))],
        }
    }

    #[test]
    fn answers_on_an_edited_snapshot_and_on_the_one_it_came_from_are_checked() {
        let case = Case {
            inputs: design(),
            events: vec![
                HostEvent::Evaluate {
                    on: 0,
                    paths: vec![r(&["a"]), r(&["b"])],
                    then: Then::End,
                },
                edit_x(0, 5),
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![r(&["b"])],
                    then: Then::End,
                },
                HostEvent::Evaluate {
                    on: 0,
                    paths: vec![r(&["b"]), r(&[])],
                    then: Then::Leave,
                },
            ],
        };
        let reach = check(&case, &toy_core).unwrap_or_else(|mismatch| panic!("{mismatch}"));
        assert!(reach.checked_on(0) > 0, "{reach:?}");
        assert!(reach.checked_on(1) > 0, "{reach:?}");
        assert_eq!(reach.answered(), 4, "a, b and the root on 0, b on 1");
        assert_eq!(reach.order_compared(), reach.answered(), "{reach:?}");
    }

    fn driven_then_edited(then: Then) -> Case {
        Case {
            inputs: design(),
            events: vec![
                HostEvent::Drive {
                    on: 0,
                    paths: vec![r(&["b"])],
                    turns: 0,
                    then,
                },
                edit_x(0, 5),
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![r(&["b"])],
                    then: Then::End,
                },
            ],
        }
    }

    #[test]
    fn a_request_left_live_unfinished_is_answered_and_checked_after_later_events() {
        let case = driven_then_edited(Then::Leave);
        let reach = check(&case, &toy_core).unwrap_or_else(|mismatch| panic!("{mismatch}"));
        assert!(reach.count(Shape::LeftLiveAnswer) > 0, "{reach:?}");
    }

    #[test]
    fn a_request_ended_unfinished_is_owed_no_answer() {
        let case = driven_then_edited(Then::End);
        let reach = check(&case, &toy_core).unwrap_or_else(|mismatch| panic!("{mismatch}"));
        assert!(reach.count(Shape::DriveZeroEnded) > 0, "{reach:?}");
    }

    /// Registered in place of the toy cell: answers 0 whatever it binds.
    struct ZeroCell;

    impl Kind for ZeroCell {
        type Out = i64;
        const ID: KindId = toy::Cell::ID;
        const VERSION: u32 = toy::Cell::VERSION;

        async fn run(_cx: &Cx) -> Produced<i64> {
            Produced::Outcome(Outcome::of(0))
        }
    }

    /// Registered in place of the toy cell: fails outside the inputs.
    struct OutsideCell;

    impl Kind for OutsideCell {
        type Out = i64;
        const ID: KindId = toy::Cell::ID;
        const VERSION: u32 = toy::Cell::VERSION;

        async fn run(_cx: &Cx) -> Produced<i64> {
            Produced::Outside(OutsideFailure::OutOfMemory { resource: "test" })
        }
    }

    fn with_cell_kind<K: Kind>(store: Store) -> Core {
        Builder::new()
            .kind::<K>()
            .and_then(|builder| builder.kind::<toy::Scope>())
            .expect("the kind ids are distinct")
            .on_store(store)
            .build()
    }

    fn sum_of_two() -> Case {
        Case {
            inputs: inputs([
                (root("c"), declaration(cell([input("a"), input("b")]))),
                (named("a"), int(2)),
                (named("b"), int(3)),
            ]),
            events: vec![
                edit_x(0, 1),
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![path("c", [])],
                    then: Then::End,
                },
            ],
        }
    }

    #[test]
    fn a_cell_that_answers_zero_is_reported_against_the_reference() {
        let found = check(&sum_of_two(), &with_cell_kind::<ZeroCell>);
        let expected = Mismatch::Reference {
            event: 1,
            snapshot: 1,
            path: path("c", []),
            core: Box::new(Seen::Int(0)),
            reference: Box::new(Seen::Int(5)),
        };
        assert_eq!(found, Err(expected));
    }

    #[test]
    fn a_pending_answer_is_reported_against_the_reference() {
        let found = check(&sum_of_two(), &with_cell_kind::<OutsideCell>);
        let pending = OutsideFailure::OutOfMemory { resource: "test" };
        let expected = Mismatch::Reference {
            event: 1,
            snapshot: 1,
            path: path("c", []),
            core: Box::new(Seen::Pending(pending)),
            reference: Box::new(Seen::Int(5)),
        };
        assert_eq!(found, Err(expected));
    }

    /// Root scope "r" with the one member "a" declared `a`, and an
    /// Evaluate of "a".
    fn evaluate_a(a: Decl) -> Case {
        Case {
            inputs: inputs([
                (root("r"), declaration(scope("r.m"))),
                (named("r.m"), members([(step("a"), a)])),
            ]),
            events: vec![HostEvent::Evaluate {
                on: 0,
                paths: vec![r(&["a"])],
                then: Then::End,
            }],
        }
    }

    /// A place below member "a" itself.
    fn below_a() -> Bind {
        declarer([step("a"), step("x")])
    }

    #[test]
    fn a_request_the_core_ends_is_reported_with_its_error() {
        let found = check(&evaluate_a(cell([below_a()])), &toy_core);
        let expected = Mismatch::Ended {
            event: 0,
            snapshot: 0,
            error: RequestError::NothingCanRun {
                waiting: vec![r(&["a"]).to_core()],
            },
        };
        assert_eq!(found, Err(expected));
    }

    #[test]
    fn an_answer_on_a_case_the_reference_cannot_judge_is_reported_invalid() {
        let found = check(&evaluate_a(cell([below_a()])), &with_cell_kind::<ZeroCell>);
        let expected = Mismatch::Invalid {
            event: 0,
            snapshot: 0,
            invalid: Invalid::Cycle { path: r(&["a"]) },
        };
        assert_eq!(found, Err(expected));
    }

    /// A resource that counts the cells run on its core.
    struct Ran(i64);

    /// Registered in place of the toy cell: reads its term "0", so that
    /// cells over different terms are different results in the store, then
    /// answers how many cells ran on its core before it, so its answer
    /// depends on the order of requests.
    struct CountingCell;

    impl Kind for CountingCell {
        type Out = i64;
        const ID: KindId = toy::Cell::ID;
        const VERSION: u32 = toy::Cell::VERSION;

        async fn run(cx: &Cx) -> Produced<i64> {
            let _ = cx.read::<i64>(&Ref::binding("0")).await;
            let before = cx
                .exclusive(|ran: &mut Ran, _| {
                    ran.0 += 1;
                    ran.0 - 1
                })
                .await;
            Produced::Outcome(Outcome::of(before))
        }
    }

    fn counting_core(store: Store) -> Core {
        let mut core = with_cell_kind::<CountingCell>(store);
        core.add_resource(|| Ran(0));
        core
    }

    #[test]
    fn an_answer_that_depends_on_the_order_of_requests_is_reported_at_its_model_path() {
        let evaluate = |name| HostEvent::Evaluate {
            on: 0,
            paths: vec![path(name, [])],
            then: Then::End,
        };
        let case = Case {
            inputs: inputs([
                (root("c0"), declaration(cell([]))),
                (root("c1"), declaration(cell([input("one")]))),
                (named("one"), int(1)),
            ]),
            events: vec![evaluate("c0"), evaluate("c1")],
        };
        let expected = Mismatch::Order {
            snapshot: 0,
            path: path("c1", []),
            session: Box::new(Seen::Int(1)),
            fresh: Box::new(Seen::Int(0)),
        };
        assert_eq!(check(&case, &counting_core), Err(expected));
    }

    #[test]
    fn an_answer_that_depends_on_history_is_reported_by_the_fresh_core_check() {
        let case = Case {
            inputs: inputs([
                (root("c0"), declaration(cell([]))),
                (root("c1"), declaration(cell([input("one")]))),
                (named("one"), int(1)),
            ]),
            events: vec![
                HostEvent::Drive {
                    on: 0,
                    paths: vec![path("c0", [])],
                    turns: 1,
                    then: Then::End,
                },
                HostEvent::Evaluate {
                    on: 0,
                    paths: vec![path("c1", [])],
                    then: Then::End,
                },
            ],
        };
        let expected = Mismatch::Fresh {
            snapshot: 0,
            path: path("c1", []),
            session: Box::new(Seen::Int(1)),
            fresh: Box::new(Seen::Int(0)),
        };
        assert_eq!(check(&case, &counting_core), Err(expected));
    }

    /// Registered in place of the toy cell: reads its binding "loop", then
    /// answers 0.
    struct LoopCell;

    impl Kind for LoopCell {
        type Out = i64;
        const ID: KindId = toy::Cell::ID;
        const VERSION: u32 = toy::Cell::VERSION;

        async fn run(cx: &Cx) -> Produced<i64> {
            let _ = cx.read::<i64>(&Ref::binding("loop")).await;
            Produced::Outcome(Outcome::of(0))
        }
    }

    /// What `check` finds when the first `honest` cores it builds are
    /// honest and every later one is built with `LoopCell`.
    fn looping_after(honest: usize) -> Mismatch {
        let made = std::cell::Cell::new(0);
        let honest_then_looping = |store| {
            made.set(made.get() + 1);
            if made.get() <= honest {
                toy_core(store)
            } else {
                with_cell_kind::<LoopCell>(store)
            }
        };
        let looping = decl(ToyKind::Cell, [("loop", below_a())]);
        check(&evaluate_a(looping), &honest_then_looping).expect_err("a looping core ends it")
    }

    fn ended_waiting_on_a() -> Option<RequestError> {
        Some(RequestError::NothingCanRun {
            waiting: vec![r(&["a"]).to_core()],
        })
    }

    #[test]
    fn a_fresh_core_on_the_session_s_selection_that_ends_a_request_the_session_answered_is_reported()
     {
        let expected = Mismatch::FreshUnanswered {
            snapshot: 0,
            path: r(&["a"]),
            ended: ended_waiting_on_a(),
        };
        assert_eq!(looping_after(1), expected);
    }

    #[test]
    fn a_fresh_core_that_ends_a_request_the_session_answered_is_reported() {
        let expected = Mismatch::OrderUnanswered {
            snapshot: 0,
            path: r(&["a"]),
            ended: ended_waiting_on_a(),
        };
        assert_eq!(looping_after(2), expected);
    }
}
