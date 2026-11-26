//! What the tests share: the toy kinds, the toy kernel with the gate that
//! holds its sections, and host-side helpers for building a core, making
//! snapshot inputs and reading the answers a request received.

use std::any::{Any, type_name};
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

use reify_eval_core::{
    Answer, Binding, Builder, Core, Cx, Declaration, Erased, Event, Failure, Held, Input, InputKey,
    Kind, KindId, Outcome, OutsideFailure, Path, Produced, Ref, Release, Request, RequestId, Root,
    Section, Value,
};

/// Adds the i64s bound at "0", "1", … until a binding reads absent. A read
/// that fails ends the body with that failure as its outcome.
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
        Produced::Outcome(Outcome::of(sum))
    }
}

/// A cell over the host-named inputs `names`, bound at "0", "1", … in order.
pub fn cell(names: &[&str]) -> Declaration {
    names.iter().enumerate().fold(Declaration::new(Cell::ID), |declaration, (index, name)| {
        declaration.bind(index.to_string(), Binding::Input(InputKey::named(*name)))
    })
}

pub fn with_cell() -> Builder {
    Builder::new().kind::<Cell>().expect("a fresh builder has no kinds")
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
        Produced::Outcome(Outcome::of(BoxShape { handle, volume: side * side * side, built_on }))
    }
}

/// A shape whose side is the host-named input `side`.
pub fn shape(side: &str) -> Declaration {
    Declaration::new(Shape::ID).bind("side", Binding::Input(InputKey::named(side)))
}

pub fn with_toys() -> Builder {
    with_cell().kind::<Shape>().expect("toy kind ids are distinct")
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
        self.sides_in_order.lock().expect("no holder panics").clone()
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
        self.reached.recv().expect("the kernel lives while a section is expected");
    }

    fn open(&self) {
        self.open.send(()).expect("the kernel lives while a section waits");
    }
}

fn point() -> (Point, PointKeeper) {
    let (reached_in, reached) = channel();
    let (open, open_out) = channel();
    (Point { reached: reached_in, open: open_out }, PointKeeper { reached, open })
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
        self.after_progress.as_ref().expect("the gate was built to hold sections after progress")
    }
}

pub struct ToyKernel {
    boxes: BTreeSet<u32>,
    made: u32,
    watch: Watch,
    gate: Option<Gate>,
}

impl ToyKernel {
    pub fn make_box(&mut self, side: i64, section: &Section<ToyKernel>) -> (Held<ToyKernel, BoxHandle>, ThreadId) {
        if let Some(gate) = &self.gate {
            gate.entry.pass();
        }
        section.progress(side);
        if let Some(point) = self.gate.as_ref().and_then(|gate| gate.after_progress.as_ref()) {
            point.pass();
        }
        if section.cancelled() {
            self.watch.saw_cancelled.store(true, Ordering::SeqCst);
        }
        self.made += 1;
        self.boxes.insert(self.made);
        self.watch.live_boxes.fetch_add(1, Ordering::SeqCst);
        self.watch.sides_in_order.lock().expect("no holder panics").push(side);
        (section.hold(BoxHandle(self.made)), thread::current().id())
    }
}

fn with_kernel(builder: Builder, watch: &Watch, gate: Option<Gate>) -> Core {
    let mut core = builder.build();
    let watch = watch.clone();
    core.add_resource(move || ToyKernel { boxes: BTreeSet::new(), made: 0, watch, gate });
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
    let gate = Gate { entry, after_progress: None };
    (with_kernel(builder, watch, Some(gate)), GateKeeper { entry: entry_keeper, after_progress: None })
}

/// As `gated`, and each section is held again after its progress note.
pub fn gated_after_progress(builder: Builder, watch: &Watch) -> (Core, GateKeeper) {
    let ((entry, entry_keeper), (after, after_keeper)) = (point(), point());
    let gate = Gate { entry, after_progress: Some(after) };
    let keeper = GateKeeper { entry: entry_keeper, after_progress: Some(after_keeper) };
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

/// Every answer among `events` that went to `request`, in the order given.
pub fn answers(events: &[Event], request: RequestId) -> Vec<(Path, Answer<Erased>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Answered { request: to, path, answer } if *to == request => {
                Some((path.clone(), answer.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Every progress note among `events` that went to `request`, in order.
pub fn notes(events: &[Event], request: RequestId) -> Vec<(Path, Arc<dyn Any + Send + Sync>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Progress { request: to, path, note } if *to == request => {
                Some((path.clone(), Arc::clone(note)))
            }
            _ => None,
        })
        .collect()
}

/// The one answer `request` received, which must be for root `name`.
pub fn the_answer(events: &[Event], request: &Request, name: &str) -> Answer<Erased> {
    let mut answers = answers(events, request.id());
    assert_eq!(answers.len(), 1, "one answer to the request: {answers:?}");
    let (path, answer) = answers.remove(0);
    assert_eq!(path, Path::root(Root::named(name)));
    answer
}

/// The one answer `request` received for root `name`, among others.
pub fn answer_at(events: &[Event], request: &Request, name: &str) -> Answer<Erased> {
    let at = Path::root(Root::named(name));
    let mut answers: Vec<_> = answers(events, request.id()).into_iter().filter(|(path, _)| *path == at).collect();
    assert_eq!(answers.len(), 1, "one answer for {name}: {answers:?}");
    answers.remove(0).1
}

/// The value of the one answer `request` received, for root `name`.
pub fn the_value<T: Value + Debug>(events: &[Event], request: &Request, name: &str) -> Arc<T> {
    match the_answer(events, request, name).downcast::<T>() {
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
