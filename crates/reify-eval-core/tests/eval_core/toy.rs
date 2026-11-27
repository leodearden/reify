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
    Answer, Binding, Budget, Builder, Core, Cx, Declaration, Demand, DuplicateKind, Erased, Event,
    Failure, Hash128, Held, Input, InputKey, Kind, KindId, Limit, Outcome, OutsideFailure, Path,
    Produced, Ref, Release, Request, RequestId, Root, Section, Snapshot, Step, Turn, Value,
};

/// The content a test gives the core for one of its values: a hash of
/// everything a reader can see of it (O7). The core tags it with the
/// value's type, so two types may hash the same bytes.
pub trait Contented {
    fn content(&self) -> Hash128;
}

impl Contented for i64 {
    fn content(&self) -> Hash128 {
        Hash128::of_bytes(&self.to_le_bytes())
    }
}

impl Contented for bool {
    fn content(&self) -> Hash128 {
        Hash128::of_bytes(&[u8::from(*self)])
    }
}

impl Contented for u64 {
    fn content(&self) -> Hash128 {
        Hash128::of_bytes(&self.to_le_bytes())
    }
}

impl Contented for String {
    fn content(&self) -> Hash128 {
        Hash128::of_bytes(self.as_bytes())
    }
}

impl Contented for Declaration {
    fn content(&self) -> Hash128 {
        Declaration::content(self)
    }
}

/// A test's own encoding of a step: a tag, then the name or the index.
fn step_bytes(step: &Step) -> Vec<u8> {
    match step {
        Step::Named(name) => [b"n", &name.len().to_le_bytes()[..], name.as_bytes()].concat(),
        Step::Index(index) => [b"i", &index.to_le_bytes()[..]].concat(),
    }
}

impl Contented for Members {
    fn content(&self) -> Hash128 {
        let bytes: Vec<u8> = self
            .0
            .iter()
            .flat_map(|(step, declaration)| {
                [
                    step_bytes(step),
                    declaration.content().as_u128().to_le_bytes().to_vec(),
                ]
                .concat()
            })
            .collect();
        Hash128::of_bytes(&bytes)
    }
}

impl Contented for Elements {
    fn content(&self) -> Hash128 {
        let count = self.count.to_le_bytes();
        let element = self.element.content().as_u128().to_le_bytes();
        Hash128::of_bytes(&[&count[..], &element[..]].concat())
    }
}

/// `value` as a snapshot input, with its content.
pub fn input_of<T: Value + Contented>(value: T) -> Input {
    let content = value.content();
    Input::of(value, content)
}

/// Adds the i64s bound at "0", "1", … until a binding reads absent. Then,
/// if "pick" reads an i64 i, adds the i64 at "from"/[i], a ref computed
/// from an answer; absent there, or at a negative i, the cell is absent.
/// Then, if "divisor" reads an i64, divides the sum by it, truncating; zero
/// is the cell's own failure, `DIVISION_BY_ZERO`. A read that fails ends
/// the body: a wrong type is its answer, whether its own read found the
/// wrong type or what it read had (a body cannot tell the two apart), and
/// any other failure makes it blocked by the ref it read.
pub struct Cell;

/// What a cell's read of an i64 at `r` gives it: the value, or none where
/// it reads absent; a failure is the cell's answer.
async fn read_term(cx: &Cx, r: &Ref) -> Result<Option<i64>, Failure> {
    match cx.read::<i64>(r).await {
        Outcome::Value(value) => Ok(Some(*value)),
        Outcome::Absent => Ok(None),
        Outcome::Failure(wrong @ Failure::WrongType { .. }) => Err(wrong),
        Outcome::Failure(_) => Err(Failure::BlockedBy { at: r.clone() }),
    }
}

/// A cell's sum, with the picked element; none where the cell is absent.
async fn sum(cx: &Cx) -> Result<Option<i64>, Failure> {
    let mut sum = 0;
    for index in 0_u32.. {
        match read_term(cx, &Ref::binding(index.to_string())).await? {
            Some(value) => sum += value,
            None => break,
        }
    }
    let Some(pick) = read_term(cx, &Ref::binding("pick")).await? else {
        return Ok(Some(sum));
    };
    let Ok(pick) = u64::try_from(pick) else {
        return Ok(None);
    };
    let from = Ref::binding("from").step(Step::index(pick));
    Ok(read_term(cx, &from).await?.map(|value| sum + value))
}

impl Kind for Cell {
    type Out = i64;
    const ID: KindId = KindId("toy.cell");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let sum = match sum(cx).await {
            Ok(Some(sum)) => sum,
            Ok(None) => return Produced::Outcome(Outcome::Absent),
            Err(failure) => return Produced::Outcome(Outcome::Failure(failure)),
        };
        Produced::Outcome(match read_term(cx, &Ref::binding("divisor")).await {
            Ok(None) => Outcome::of(sum),
            Ok(Some(0)) => Outcome::Failure(Failure::OfKind {
                kind: Self::ID,
                code: DIVISION_BY_ZERO,
            }),
            Ok(Some(divisor)) => Outcome::of(sum / divisor),
            Err(failure) => Outcome::Failure(failure),
        })
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(out.content())
    }
}

/// Adds the i64s at "0" and "1", read in one read.
pub struct Pair;

impl Kind for Pair {
    type Out = i64;
    const ID: KindId = KindId("toy.pair");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let refs = [Ref::binding("0"), Ref::binding("1")];
        let mut sum = 0;
        for read in cx.read_all::<i64>(&refs).await {
            match read {
                Outcome::Value(value) => sum += *value,
                other => return Produced::Outcome(other),
            }
        }
        Produced::Outcome(Outcome::of(sum))
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(out.content())
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

/// The code of the toy cell's failure when its divisor is zero.
pub const DIVISION_BY_ZERO: &str = "division by zero";

/// `cell`, its sum divided by what `divisor` names.
pub fn divided(cell: Declaration, divisor: Binding) -> Declaration {
    cell.bind("divisor", divisor)
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
#[derive(Debug, Clone, Default, PartialEq)]
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

/// The members at binding "members".
async fn read_members(cx: &Cx) -> Outcome<Members> {
    cx.read::<Members>(&Ref::binding("members")).await
}

/// Declares the members it reads at binding "members", and nothing else
/// (PRD 5.4); its value is those members.
pub struct Scope;

impl Kind for Scope {
    type Out = Members;
    const ID: KindId = KindId("toy.scope");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Members> {
        Produced::Outcome(read_members(cx).await)
    }

    fn content(out: &Members) -> Option<Hash128> {
        Some(out.content())
    }

    fn declares(out: &Members, step: &Step) -> Option<Declaration> {
        out.0.get(step).cloned()
    }

    fn steps(out: &Members) -> impl Iterator<Item = Step> {
        out.0.keys().cloned()
    }
}

/// A scope whose members are the host-named input `members`, and nothing
/// else.
pub fn scope(members: &str) -> Declaration {
    Declaration::new(Scope::ID).bind("members", input(members))
}

/// Reads the bool at "condition" first: false makes it absent, true makes
/// it declare as a scope does; any other outcome is its answer.
pub struct Guard;

impl Kind for Guard {
    type Out = Members;
    const ID: KindId = KindId("toy.guard");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Members> {
        match cx.read::<bool>(&Ref::binding("condition")).await {
            Outcome::Value(holds) if *holds => Produced::Outcome(read_members(cx).await),
            Outcome::Value(_) => Produced::Outcome(Outcome::Absent),
            Outcome::Absent => Produced::Outcome(Outcome::Absent),
            Outcome::Failure(failure) => Produced::Outcome(Outcome::Failure(failure)),
        }
    }

    fn content(out: &Members) -> Option<Hash128> {
        Some(out.content())
    }

    fn declares(out: &Members, step: &Step) -> Option<Declaration> {
        out.0.get(step).cloned()
    }

    fn steps(out: &Members) -> impl Iterator<Item = Step> {
        out.0.keys().cloned()
    }
}

/// A guard on `condition` over the host-named input `members`.
pub fn guard(condition: Binding, members: &str) -> Declaration {
    Declaration::new(Guard::ID)
        .bind("condition", condition)
        .bind("members", input(members))
}

/// How many elements a collection has, and what each is.
#[derive(Debug, PartialEq)]
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
    const VERSION: u32 = 1;

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

    fn content(out: &Elements) -> Option<Hash128> {
        Some(out.content())
    }

    fn declares(out: &Elements, step: &Step) -> Option<Declaration> {
        match step {
            Step::Index(index) if *index < out.count => Some(out.element.clone()),
            _ => None,
        }
    }

    fn steps(out: &Elements) -> impl Iterator<Item = Step> {
        (0..out.count).map(Step::index)
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
        (InputKey::named(input.as_str()), input_of(members)),
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
        kernel.marked.remove(&self.0);
        kernel.watch.live_boxes.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub struct BoxShape {
    pub handle: Held<ToyKernel, BoxHandle>,
    pub volume: i64,
    pub built_on: ThreadId,
}

/// A box is its description: the handle and the thread are where it
/// lives (contract 5.4).
impl PartialEq for BoxShape {
    fn eq(&self, other: &Self) -> bool {
        self.volume == other.volume
    }
}

/// Makes a box on the toy kernel from the side length bound at "side".
pub struct Shape;

impl Kind for Shape {
    type Out = BoxShape;
    const ID: KindId = KindId("toy.shape");
    const VERSION: u32 = 1;

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

/// Reads the box at "shape" and measures it in a section on the toy
/// kernel: its volume as the kernel keeps it.
pub struct Measure;

impl Kind for Measure {
    type Out = i64;
    const ID: KindId = KindId("toy.measure");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let shape = match cx.read::<BoxShape>(&Ref::binding("shape")).await {
            Outcome::Value(shape) => shape,
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            Outcome::Absent => return Produced::Outcome(Outcome::Absent),
        };
        let volume = cx
            .exclusive(move |kernel: &mut ToyKernel, _| kernel.measure(shape.handle.0))
            .await;
        Produced::Outcome(Outcome::of(volume))
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(out.content())
    }
}

/// A measure of the box `shape` names.
pub fn measure(shape: Binding) -> Declaration {
    Declaration::new(Measure::ID).bind("shape", shape)
}

pub fn with_toys() -> Builder {
    with_cell()
        .kind::<Shape>()
        .and_then(|builder| builder.kind::<Measure>())
        .expect("toy kind ids are distinct")
}

/// How a kind is registered on a builder.
pub type Register = fn(Builder) -> Result<Builder, DuplicateKind>;

/// Every PRD toy kind built so far (PRD 5.4), with its registration: the
/// one list `with_every_kind` registers and the two-places table must
/// cover. A kind added here needs a two-places entry.
pub const TOY_KINDS: [(KindId, Register); 6] = [
    (Cell::ID, Builder::kind::<Cell>),
    (Scope::ID, Builder::kind::<Scope>),
    (Guard::ID, Builder::kind::<Guard>),
    (Collection::ID, Builder::kind::<Collection>),
    (Shape::ID, Builder::kind::<Shape>),
    (Measure::ID, Builder::kind::<Measure>),
];

pub fn with_every_kind() -> Builder {
    TOY_KINDS
        .iter()
        .try_fold(Builder::new(), |builder, (_, register)| register(builder))
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
    /// The volume of each live box, by handle.
    boxes: BTreeMap<u32, i64>,
    marked: BTreeSet<u32>,
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
        self.boxes.insert(self.made, side * side * side);
        self.watch.live_boxes.fetch_add(1, Ordering::SeqCst);
        self.watch
            .sides_in_order
            .lock()
            .expect("no holder panics")
            .push(side);
        (section.hold(BoxHandle(self.made)), thread::current().id())
    }

    /// The volume of the live box `handle`.
    pub fn measure(&self, handle: u32) -> i64 {
        *self.boxes.get(&handle).expect("a held box is live")
    }

    pub fn mark(&mut self, handle: u32) {
        self.marked.insert(handle);
    }

    pub fn is_marked(&self, handle: u32) -> bool {
        self.marked.contains(&handle)
    }
}

fn with_kernel(builder: Builder, watch: &Watch, gate: Option<Gate>) -> Core {
    let mut core = builder.build();
    let watch = watch.clone();
    core.add_resource(move || ToyKernel {
        boxes: BTreeMap::new(),
        marked: BTreeSet::new(),
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
pub fn root<T: Value + Contented>(name: &str, declaration: T) -> (InputKey, Input) {
    (InputKey::root(name), input_of(declaration))
}

/// The host-named input `name`.
pub fn named<T: Value + Contented>(name: &str, value: T) -> (InputKey, Input) {
    (InputKey::named(name), input_of(value))
}

/// The snapshot's setting of `limit` (contract 7.6).
pub fn limit(limit: Limit, value: u64) -> (InputKey, Input) {
    (InputKey::Limit(limit), input_of(value))
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

/// A fresh request for `paths` on `snapshot`, and the events of the turn
/// it is given, which must leave nothing running.
pub fn evaluate_to_idle(
    core: &mut Core,
    snapshot: &Snapshot,
    paths: &[&Path],
) -> (Request, Vec<Event>) {
    evaluate_within(core, snapshot, paths, Budget::unlimited())
}

/// A fresh request for `paths` on `snapshot` under `budget`, run to idle,
/// and the events of that turn.
pub fn evaluate_within(
    core: &mut Core,
    snapshot: &Snapshot,
    paths: &[&Path],
    budget: Budget,
) -> (Request, Vec<Event>) {
    let request = core.evaluate(snapshot, demand(paths.iter().copied()), budget);
    assert_eq!(core.turn(), Turn::Idle);
    (request, core.take_events())
}

/// The answer at `at` to a fresh request for it alone on `snapshot`.
pub fn answer_alone(core: &mut Core, snapshot: &Snapshot, at: &Path) -> Answer<Erased> {
    let (request, events) = evaluate_to_idle(core, snapshot, &[at]);
    the_answer(&events, &request, at)
}

/// The answers to a fresh request for `paths` on `snapshot`, one for each
/// path in order.
pub fn answers_to(core: &mut Core, snapshot: &Snapshot, paths: &[&Path]) -> Vec<Answer<Erased>> {
    let (request, events) = evaluate_to_idle(core, snapshot, paths);
    paths
        .iter()
        .map(|path| answer_at(&events, &request, path))
        .collect()
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
