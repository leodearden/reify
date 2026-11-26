//! What the tests share: the toy kinds, the toy kernel with the gate that
//! holds its sections, and host-side helpers for building a core, making
//! snapshot inputs and reading the answers a request received.

use std::any::{Any, type_name};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

use reify_eval_core::{
    Answer, Binding, Builder, Core, Cx, Declaration, Demand, Erased, Event, Failure, Held, Input,
    InputKey, Kind, KindId, Outcome, OutsideFailure, Path, Produced, Ref, Release, Request,
    RequestId, Root, Section, Step, Value,
};

/// Adds the i64s bound at "0", "1", … until a binding reads absent. Then,
/// if "pick" reads an i64 i, adds the i64 at "from"/[i], a ref computed
/// from an answer; absent there, or at a negative i, the cell is absent. A
/// read that fails ends the body with that failure as its outcome.
pub struct Cell;

impl Kind for Cell {
    type Out = i64;
    const ID: KindId = KindId("toy.cell");

    async fn run(cx: &Cx) -> Produced<i64> {
        let mut sum = 0;
        for index in 0_u32.. {
            match cx.read::<i64>(&Ref::binding(index.to_string())).await {
                Outcome::Value(value) => sum += *value,
                Outcome::Absent => break,
                Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            }
        }
        let pick = match cx.read::<i64>(&Ref::binding("pick")).await {
            Outcome::Value(pick) => u64::try_from(*pick).ok(),
            Outcome::Absent => return Produced::Outcome(Outcome::of(sum)),
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
        };
        let Some(pick) = pick else {
            return Produced::Outcome(Outcome::Absent);
        };
        match cx
            .read::<i64>(&Ref::binding("from").step(Step::index(pick)))
            .await
        {
            Outcome::Value(value) => Produced::Outcome(Outcome::of(sum + *value)),
            other => Produced::Outcome(other),
        }
    }
}

/// A cell that adds what `terms` name, bound at "0", "1", … in order.
pub fn cell_over(terms: impl IntoIterator<Item = Binding>) -> Declaration {
    terms
        .into_iter()
        .enumerate()
        .fold(Declaration::new(Cell::ID), |declaration, (index, term)| {
            declaration.bind(index.to_string(), term)
        })
}

/// A cell over the host-named inputs `names`, bound at "0", "1", … in order.
pub fn cell(names: &[&str]) -> Declaration {
    cell_over(names.iter().map(|name| input(name)))
}

/// A binding to the host-named input `name`.
pub fn input(name: &str) -> Binding {
    Binding::Input(InputKey::named(name))
}

/// A cell whose only term is the element at "from"/[the i64 at "pick"].
pub fn picker(pick: Binding, from: Binding) -> Declaration {
    Declaration::new(Cell::ID)
        .bind("pick", pick)
        .bind("from", from)
}

/// A binding to the place `steps` down from the bound node's declarer:
/// `sibling(&["b"])` names the declarer's member `b`.
pub fn sibling(steps: &[&str]) -> Binding {
    Binding::Declarer(steps.iter().map(|step| Step::named(*step)).collect())
}

pub fn with_cell() -> Builder {
    Builder::new()
        .kind::<Cell>()
        .expect("a fresh builder has no kinds")
}

/// What a scope declares: a declaration at each step.
#[derive(Debug, Clone, Default)]
pub struct Members(pub BTreeMap<Step, Declaration>);

/// Members, each at a named step.
pub fn members<'a>(named: impl IntoIterator<Item = (&'a str, Declaration)>) -> Members {
    Members(
        named
            .into_iter()
            .map(|(step, declaration)| (Step::named(step), declaration))
            .collect(),
    )
}

/// The members at binding "members", with those at binding "extra" added;
/// with no extra members, what "members" read, not a copy.
async fn read_members(cx: &Cx) -> Outcome<Members> {
    let refs = [Ref::binding("members"), Ref::binding("extra")];
    let [members, extra] = <[_; 2]>::try_from(cx.read_all::<Members>(&refs).await)
        .expect("a read gives an outcome per ref");
    match (members, extra) {
        (Outcome::Value(members), Outcome::Value(extra)) => {
            let mut all = (*members).clone();
            all.0.extend(extra.0.clone());
            Outcome::of(all)
        }
        (Outcome::Value(members), Outcome::Absent) => Outcome::Value(members),
        (Outcome::Value(_), failed) | (failed, _) => failed,
    }
}

/// Declares the members it reads at binding "members", and those of the
/// input "extra" attached to its own path; its value is those members.
pub struct Scope;

impl Kind for Scope {
    type Out = Members;
    const ID: KindId = KindId("toy.scope");

    async fn run(cx: &Cx) -> Produced<Members> {
        Produced::Outcome(read_members(cx).await)
    }

    fn declares(out: &Members, step: &Step) -> Option<Declaration> {
        out.0.get(step).cloned()
    }
}

/// A scope whose members are the host-named input `members`, with any
/// members in the input "extra" attached to its path.
pub fn scope(members: &str) -> Declaration {
    Declaration::new(Scope::ID)
        .bind("members", input(members))
        .bind("extra", Binding::Attached("extra".into()))
}

/// Reads the bool at "condition" first: false makes it absent, true makes
/// it declare as a scope does; any other outcome is its answer.
pub struct Guard;

impl Kind for Guard {
    type Out = Members;
    const ID: KindId = KindId("toy.guard");

    async fn run(cx: &Cx) -> Produced<Members> {
        match cx.read::<bool>(&Ref::binding("condition")).await {
            Outcome::Value(holds) if *holds => Produced::Outcome(read_members(cx).await),
            Outcome::Value(_) => Produced::Outcome(Outcome::Absent),
            Outcome::Absent => Produced::Outcome(Outcome::Absent),
            Outcome::Failure(failure) => Produced::Outcome(Outcome::Failure(failure)),
        }
    }

    fn declares(out: &Members, step: &Step) -> Option<Declaration> {
        out.0.get(step).cloned()
    }
}

/// A guard on `condition` over the host-named input `members`.
pub fn guard(condition: Binding, members: &str) -> Declaration {
    Declaration::new(Guard::ID)
        .bind("condition", condition)
        .bind("members", input(members))
}

/// How many elements a collection has, and what each is.
pub struct Elements {
    pub count: u64,
    pub element: Declaration,
}

/// Reads the i64 count at "count" and the declaration at "element", and
/// declares that element at each index below the count.
pub struct Collection;

impl Kind for Collection {
    type Out = Elements;
    const ID: KindId = KindId("toy.collection");

    async fn run(cx: &Cx) -> Produced<Elements> {
        let count = match cx.read::<i64>(&Ref::binding("count")).await {
            Outcome::Value(count) => u64::try_from(*count).unwrap_or(0),
            Outcome::Absent => return Produced::Outcome(Outcome::Absent),
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
        };
        match cx.read::<Declaration>(&Ref::binding("element")).await {
            Outcome::Value(element) => Produced::Outcome(Outcome::of(Elements {
                count,
                element: (*element).clone(),
            })),
            Outcome::Absent => Produced::Outcome(Outcome::Absent),
            Outcome::Failure(failure) => Produced::Outcome(Outcome::Failure(failure)),
        }
    }

    fn declares(out: &Elements, step: &Step) -> Option<Declaration> {
        match step {
            Step::Index(index) if *index < out.count => Some(out.element.clone()),
            _ => None,
        }
    }
}

/// A collection of `count` elements, each declared as `element` reads.
pub fn collection(count: Binding, element: Binding) -> Declaration {
    Declaration::new(Collection::ID)
        .bind("count", count)
        .bind("element", element)
}

/// The inputs of root scope `name` with `members`: the members under the
/// host-named input "`name`.members", and the root's declaration.
pub fn root_scope(name: &str, members: Members) -> [(InputKey, Input); 2] {
    let input = format!("{name}.members");
    [
        (InputKey::named(input.as_str()), Input::of(members)),
        root(name, scope(&input)),
    ]
}

pub fn with_scope() -> Builder {
    with_cell()
        .kind::<Scope>()
        .expect("toy kind ids are distinct")
}

/// A builder with every toy kind of structure: cells, scopes, guards and
/// collections.
pub fn with_structure() -> Builder {
    with_scope()
        .kind::<Guard>()
        .and_then(|builder| builder.kind::<Collection>())
        .expect("toy kind ids are distinct")
}

/// A box the toy kernel made, by the number it handed out.
#[derive(Debug, PartialEq)]
pub struct BoxHandle(pub u32);

impl Release<ToyKernel> for BoxHandle {
    fn release(self, kernel: &mut ToyKernel) {
        kernel.boxes.remove(&self.0);
        kernel.watch.live_boxes.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub struct BoxShape {
    pub handle: Held<ToyKernel, BoxHandle>,
    pub volume: i64,
    pub built_on: ThreadId,
}

/// Makes a box on the toy kernel from the side length bound at "side".
pub struct Shape;

impl Kind for Shape {
    type Out = BoxShape;
    const ID: KindId = KindId("toy.shape");

    async fn run(cx: &Cx) -> Produced<BoxShape> {
        let side = match cx.read::<i64>(&Ref::binding("side")).await {
            Outcome::Value(side) => *side,
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            Outcome::Absent => return Produced::Outcome(Outcome::Absent),
        };
        let (handle, built_on) = cx
            .exclusive(move |kernel: &mut ToyKernel, section| kernel.make_box(side, section))
            .await;
        Produced::Outcome(Outcome::of(BoxShape {
            handle,
            volume: side * side * side,
            built_on,
        }))
    }
}

/// A shape whose side is the host-named input `side`.
pub fn shape(side: &str) -> Declaration {
    Declaration::new(Shape::ID).bind("side", Binding::Input(InputKey::named(side)))
}

pub fn with_toys() -> Builder {
    with_cell()
        .kind::<Shape>()
        .expect("toy kind ids are distinct")
}

/// What a test can see of the toy kernel from outside its thread.
#[derive(Clone, Default)]
pub struct Watch {
    live_boxes: Arc<AtomicUsize>,
    sides_in_order: Arc<Mutex<Vec<i64>>>,
    saw_cancelled: Arc<AtomicBool>,
}

impl Watch {
    pub fn live_boxes(&self) -> usize {
        self.live_boxes.load(Ordering::SeqCst)
    }

    /// The side of each box made, in the order the kernel made them.
    pub fn sides_in_order(&self) -> Vec<i64> {
        self.sides_in_order
            .lock()
            .expect("no holder panics")
            .clone()
    }

    /// Whether a section read `cancelled` as true.
    pub fn saw_cancelled(&self) -> bool {
        self.saw_cancelled.load(Ordering::SeqCst)
    }
}

/// Where a section is held until the test lets it through. A closed
/// channel counts as open, so a test that fails and drops its keeper never
/// leaves a section blocked, and dropping the core never hangs.
struct Point {
    reached: Sender<()>,
    open: Receiver<()>,
}

impl Point {
    fn pass(&self) {
        let _ = self.reached.send(());
        let _ = self.open.recv();
    }
}

struct PointKeeper {
    reached: Receiver<()>,
    open: Sender<()>,
}

impl PointKeeper {
    fn reached(&self) {
        self.reached
            .recv()
            .expect("the kernel lives while a section is expected");
    }

    fn open(&self) {
        self.open
            .send(())
            .expect("the kernel lives while a section waits");
    }
}

fn point() -> (Point, PointKeeper) {
    let (reached_in, reached) = channel();
    let (open, open_out) = channel();
    (
        Point {
            reached: reached_in,
            open: open_out,
        },
        PointKeeper { reached, open },
    )
}

/// Holds each section on entry, and, if built so, again after it has sent
/// its progress note and before it reads `cancelled`.
pub struct Gate {
    entry: Point,
    after_progress: Option<Point>,
}

pub struct GateKeeper {
    entry: PointKeeper,
    after_progress: Option<PointKeeper>,
}

impl GateKeeper {
    /// Blocks until a section has reached the gate.
    pub fn entered(&self) {
        self.entry.reached();
    }

    /// Lets one section that reached the gate through.
    pub fn open(&self) {
        self.entry.open();
    }

    pub fn let_one_through(&self) {
        self.entered();
        self.open();
    }

    /// Blocks until a section has sent its progress note and is held after it.
    pub fn progressed(&self) {
        self.second_point().reached();
    }

    pub fn open_after_progress(&self) {
        self.second_point().open();
    }

    fn second_point(&self) -> &PointKeeper {
        self.after_progress
            .as_ref()
            .expect("the gate was built to hold sections after progress")
    }
}

pub struct ToyKernel {
    boxes: BTreeSet<u32>,
    made: u32,
    watch: Watch,
    gate: Option<Gate>,
}

impl ToyKernel {
    pub fn make_box(
        &mut self,
        side: i64,
        section: &Section<ToyKernel>,
    ) -> (Held<ToyKernel, BoxHandle>, ThreadId) {
        if let Some(gate) = &self.gate {
            gate.entry.pass();
        }
        section.progress(side);
        if let Some(point) = self
            .gate
            .as_ref()
            .and_then(|gate| gate.after_progress.as_ref())
        {
            point.pass();
        }
        if section.cancelled() {
            self.watch.saw_cancelled.store(true, Ordering::SeqCst);
        }
        self.made += 1;
        self.boxes.insert(self.made);
        self.watch.live_boxes.fetch_add(1, Ordering::SeqCst);
        self.watch
            .sides_in_order
            .lock()
            .expect("no holder panics")
            .push(side);
        (section.hold(BoxHandle(self.made)), thread::current().id())
    }
}

fn with_kernel(builder: Builder, watch: &Watch, gate: Option<Gate>) -> Core {
    let mut core = builder.build();
    let watch = watch.clone();
    core.add_resource(move || ToyKernel {
        boxes: BTreeSet::new(),
        made: 0,
        watch,
        gate,
    });
    core
}

/// A core whose toy kernel makes boxes without waiting.
pub fn ungated(builder: Builder, watch: &Watch) -> Core {
    with_kernel(builder, watch, None)
}

/// A core whose toy kernel holds each section on entry. The keeper comes
/// second so that, bound as `let (core, keeper)`, it is dropped first.
pub fn gated(builder: Builder, watch: &Watch) -> (Core, GateKeeper) {
    let (entry, entry_keeper) = point();
    let gate = Gate {
        entry,
        after_progress: None,
    };
    (
        with_kernel(builder, watch, Some(gate)),
        GateKeeper {
            entry: entry_keeper,
            after_progress: None,
        },
    )
}

/// As `gated`, and each section is held again after its progress note.
pub fn gated_after_progress(builder: Builder, watch: &Watch) -> (Core, GateKeeper) {
    let ((entry, entry_keeper), (after, after_keeper)) = (point(), point());
    let gate = Gate {
        entry,
        after_progress: Some(after),
    };
    let keeper = GateKeeper {
        entry: entry_keeper,
        after_progress: Some(after_keeper),
    };
    (with_kernel(builder, watch, Some(gate)), keeper)
}

pub fn core() -> Core {
    Builder::new().build()
}

/// The input that declares root `name`.
pub fn root<T: Value>(name: &str, declaration: T) -> (InputKey, Input) {
    (InputKey::root(name), Input::of(declaration))
}

/// The host-named input `name`.
pub fn named<T: Value>(name: &str, value: T) -> (InputKey, Input) {
    (InputKey::named(name), Input::of(value))
}

/// The path of root `name`.
pub fn at(name: &str) -> Path {
    Path::root(Root::named(name))
}

/// The path `steps` down from root `name`, each step named.
pub fn below(name: &str, steps: &[&str]) -> Path {
    steps
        .iter()
        .fold(at(name), |path, step| path.child(Step::named(*step)))
}

/// A demand for `paths`.
pub fn demand<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Demand {
    Demand::of(paths.into_iter().cloned())
}

/// A demand for the roots named `names`.
pub fn roots(names: &[&str]) -> Demand {
    Demand::of(names.iter().map(|name| at(name)))
}

/// Every answer among `events` that went to `request`, in the order given.
pub fn answers(events: &[Event], request: RequestId) -> Vec<(Path, Answer<Erased>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Answered {
                request: to,
                path,
                answer,
                ..
            } if *to == request => Some((path.clone(), answer.clone())),
            _ => None,
        })
        .collect()
}

/// Every progress note among `events` that went to `request`, in order.
pub fn notes(events: &[Event], request: RequestId) -> Vec<(Path, Arc<dyn Any + Send + Sync>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Progress {
                request: to,
                path,
                note,
                ..
            } if *to == request => Some((path.clone(), Arc::clone(note))),
            _ => None,
        })
        .collect()
}

/// The one answer `request` received, which must be for `at`.
pub fn the_answer(events: &[Event], request: &Request, at: &Path) -> Answer<Erased> {
    let mut answers = answers(events, request.id());
    assert_eq!(answers.len(), 1, "one answer to the request: {answers:?}");
    let (path, answer) = answers.remove(0);
    assert_eq!(path, *at);
    answer
}

/// The one answer `request` received for `at`, among others.
pub fn answer_at(events: &[Event], request: &Request, at: &Path) -> Answer<Erased> {
    let mut answers: Vec<_> = answers(events, request.id())
        .into_iter()
        .filter(|(path, _)| path == at)
        .collect();
    assert_eq!(answers.len(), 1, "one answer for {at:?}: {answers:?}");
    answers.remove(0).1
}

/// The value of the one answer `request` received, for `at`.
pub fn the_value<T: Value + Debug>(events: &[Event], request: &Request, at: &Path) -> Arc<T> {
    match the_answer(events, request, at).downcast::<T>() {
        Answer::Final(Outcome::Value(value)) => value,
        other => panic!("expected a value of {}, got {other:?}", type_name::<T>()),
    }
}

/// The failure a final answer holds.
pub fn failed(answer: Answer<Erased>) -> Failure {
    match answer {
        Answer::Final(Outcome::Failure(failure)) => failure,
        other => panic!("expected a final failure, got {other:?}"),
    }
}

/// The failure outside the inputs a pending answer carries.
pub fn pending(answer: Answer<Erased>) -> OutsideFailure {
    match answer {
        Answer::Pending(failure) => failure,
        other => panic!("expected a pending answer, got {other:?}"),
    }
}
