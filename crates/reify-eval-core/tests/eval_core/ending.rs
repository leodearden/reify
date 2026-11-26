//! Ending requests, and giving back what sections made (contract 2.5, 5.5
//! and 10, K11): work nobody wants is dropped, and work a live request
//! waits on is not. Every count of live boxes is read after a drain.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reify_eval_core::{
    Answer, Binding, Core, Cx, Declaration, Held, InputKey, Kind, KindId, Outcome, Produced, Ref,
    Snapshot, Turn,
};

use crate::toy::{self, BoxHandle, BoxShape, Scope, Shape, ToyKernel, Watch};

#[test]
fn a_request_ended_while_its_body_s_section_runs_leaves_the_kernel_holding_nothing_after_a_drain() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let request = core.evaluate(&snapshot, toy::roots(&["box"]));
    let ended = request.id();
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    core.end(request);
    keeper.open();
    core.drain();

    assert_eq!(watch.live_boxes(), 0);
    assert!(toy::answers(&core.take_events(), ended).is_empty());
}

/// Stores what its body's `cancelled` reads at the moment it is dropped.
struct Witness<'a> {
    cx: &'a Cx,
    seen: Arc<AtomicBool>,
}

impl Drop for Witness<'_> {
    fn drop(&mut self) {
        self.seen.store(self.cx.cancelled(), Ordering::SeqCst);
    }
}

/// Holds a witness of its own `cancelled` while it waits on a section.
struct Witnessed;

impl Kind for Witnessed {
    type Out = i64;
    const ID: KindId = KindId("toy.witnessed");

    async fn run(cx: &Cx) -> Produced<i64> {
        let Outcome::Value(seen) = cx.read::<Arc<AtomicBool>>(&Ref::binding("seen")).await else {
            return Produced::Outcome(Outcome::Absent);
        };
        let seen = Arc::clone(&*seen);
        let _witness = Witness { cx, seen };
        let _ = cx
            .exclusive(|kernel: &mut ToyKernel, section| kernel.make_box(1, section))
            .await;
        Produced::Outcome(Outcome::of(1))
    }
}

#[test]
fn a_body_dropped_because_its_request_ended_reads_cancelled_as_it_is_dropped() {
    let watch = Watch::default();
    let builder = toy::with_toys()
        .kind::<Witnessed>()
        .expect("distinct kinds");
    let (mut core, keeper) = toy::gated(builder, &watch);
    let seen = Arc::new(AtomicBool::new(false));
    let witnessed =
        Declaration::new(Witnessed::ID).bind("seen", Binding::Input(InputKey::named("seen")));
    let snapshot = core.snapshot([
        toy::named("seen", Arc::clone(&seen)),
        toy::root("witnessed", witnessed),
    ]);
    let request = core.evaluate(&snapshot, toy::roots(&["witnessed"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    core.end(request);
    assert!(seen.load(Ordering::SeqCst));

    keeper.open();
    core.drain();
    assert_eq!(watch.live_boxes(), 0);
}

/// Makes a box in one section and keeps it while a second section makes
/// another.
struct TwoBoxes;

impl Kind for TwoBoxes {
    type Out = [Held<ToyKernel, BoxHandle>; 2];
    const ID: KindId = KindId("toy.two_boxes");

    async fn run(cx: &Cx) -> Produced<Self::Out> {
        let (first, _) = cx
            .exclusive(|kernel: &mut ToyKernel, section| kernel.make_box(1, section))
            .await;
        let (second, _) = cx
            .exclusive(|kernel: &mut ToyKernel, section| kernel.make_box(2, section))
            .await;
        Produced::Outcome(Outcome::of([first, second]))
    }
}

#[test]
fn a_request_ended_while_its_body_holds_what_a_section_made_leaves_the_kernel_holding_nothing_after_a_drain()
 {
    let watch = Watch::default();
    let builder = toy::with_toys().kind::<TwoBoxes>().expect("distinct kinds");
    let (mut core, keeper) = toy::gated(builder, &watch);
    let snapshot = core.snapshot([toy::root("pair", Declaration::new(TwoBoxes::ID))]);
    let request = core.evaluate(&snapshot, toy::roots(&["pair"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    keeper.let_one_through();
    assert_eq!(
        core.wait(),
        Turn::SectionsRunning(1),
        "the body holds its first box and waits on the second"
    );
    assert_eq!(watch.live_boxes(), 1);
    keeper.entered();

    core.end(request);
    keeper.open();
    core.drain();

    assert_eq!(watch.live_boxes(), 0);
}

#[test]
fn an_answer_the_host_drops_is_released_on_the_kernel_s_thread() {
    let watch = Watch::default();
    let mut core = toy::ungated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let request = core.evaluate(&snapshot, toy::roots(&["box"]));
    core.run();

    let events = core.take_events();
    assert_eq!(
        toy::the_value::<BoxShape>(&events, &request, &toy::at("box")).volume,
        8
    );
    assert_eq!(watch.live_boxes(), 1, "the answer holds the box");

    drop(events);
    core.drain();
    assert_eq!(
        watch.live_boxes(),
        1,
        "the index of the live snapshot holds the box"
    );

    core.end(request);
    drop(snapshot);
    core.drain();
    assert_eq!(watch.live_boxes(), 0);
}

#[test]
fn ending_a_request_drops_only_the_bodies_no_other_live_request_wants() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("one", 1_i64),
        toy::named("two", 2_i64),
        toy::root("x", toy::shape("one")),
        toy::root("y", toy::shape("two")),
    ]);
    let first = core.evaluate(&snapshot, toy::roots(&["x", "y"]));
    let ended = first.id();
    let second = core.evaluate(&snapshot, toy::roots(&["y"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(2));
    keeper.entered();

    core.end(first);
    keeper.open();
    keeper.let_one_through();
    core.drain();

    let events = core.take_events();
    assert!(toy::answers(&events, ended).is_empty());
    assert_eq!(
        toy::the_value::<BoxShape>(&events, &second, &toy::at("y")).volume,
        8
    );
    assert_eq!(
        watch.live_boxes(),
        1,
        "only the box in the live request's answer"
    );
    assert_eq!(core.counters().body_runs(Shape::ID), 2);
}

#[test]
fn live_requests_are_served_in_order_of_arrival() {
    let watch = Watch::default();
    let mut core = toy::ungated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("one", 1_i64),
        toy::named("two", 2_i64),
        toy::named("three", 3_i64),
        toy::root("small", toy::shape("one")),
        toy::root("middle", toy::shape("two")),
        toy::root("large", toy::shape("three")),
    ]);

    let _requests =
        ["large", "small", "middle"].map(|name| core.evaluate(&snapshot, toy::roots(&[name])));
    core.run();

    assert_eq!(watch.sides_in_order(), [3, 1, 2]);
}

/// The volume of the box it reads at "shape".
struct Volume;

impl Kind for Volume {
    type Out = i64;
    const ID: KindId = KindId("toy.volume");

    async fn run(cx: &Cx) -> Produced<i64> {
        match cx.read::<BoxShape>(&Ref::binding("shape")).await {
            Outcome::Value(shape) => Produced::Outcome(Outcome::of(shape.volume)),
            Outcome::Failure(failure) => Produced::Outcome(Outcome::Failure(failure)),
            Outcome::Absent => Produced::Outcome(Outcome::Absent),
        }
    }
}

/// A gated core with the toy kinds, scopes and `Volume`, and a snapshot of
/// root scope R with members `box` (a shape of side 2) and `vol` (the
/// volume of `box`).
fn box_and_volume(watch: &Watch) -> (Core, toy::GateKeeper, Snapshot) {
    let builder = toy::with_toys()
        .kind::<Scope>()
        .and_then(|builder| builder.kind::<Volume>())
        .expect("distinct kinds");
    let (mut core, keeper) = toy::gated(builder, watch);
    let volume = Declaration::new(Volume::ID).bind("shape", toy::sibling(&["box"]));
    let mut inputs = vec![toy::named("side", 2_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([("box", toy::shape("side")), ("vol", volume)]),
    ));
    let snapshot = core.snapshot(inputs);
    (core, keeper, snapshot)
}

#[test]
fn ending_a_request_keeps_a_node_a_body_of_another_live_request_is_reading() {
    let watch = Watch::default();
    let (mut core, keeper, snapshot) = box_and_volume(&watch);
    let (shape, volume) = (toy::below("R", &["box"]), toy::below("R", &["vol"]));
    let first = core.evaluate(&snapshot, toy::demand([&shape]));
    let second = core.evaluate(&snapshot, toy::demand([&volume]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    core.end(first);
    keeper.open();
    core.drain();

    let answer = toy::the_answer(&core.take_events(), &second, &volume);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(8)));
    assert!(!watch.saw_cancelled());
    assert_eq!(core.counters().body_runs(Shape::ID), 1);
}

#[test]
fn ending_a_request_drops_the_nodes_only_its_bodies_were_reading() {
    let watch = Watch::default();
    let (mut core, keeper, snapshot) = box_and_volume(&watch);
    let volume = toy::below("R", &["vol"]);
    let request = core.evaluate(&snapshot, toy::demand([&volume]));
    let ended = request.id();
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    core.end(request);
    keeper.open();
    core.drain();

    assert!(toy::answers(&core.take_events(), ended).is_empty());
    assert!(watch.saw_cancelled());
    assert_eq!(watch.live_boxes(), 0);
}

#[test]
fn a_body_queued_when_its_request_ended_and_demanded_again_waits_only_on_the_core() {
    let mut core = toy::with_scope().build();
    let mut inputs = vec![toy::named("two", 2_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("a", toy::cell_over([toy::sibling(&["b"])])),
            ("b", toy::cell(&["two"])),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    let _answered = core.evaluate(&snapshot, toy::roots(&["R"]));
    assert_eq!(core.turn(), Turn::Idle);
    let a = toy::below("R", &["a"]);

    let first = core.evaluate(&snapshot, toy::demand([&a]));
    core.end(first);
    let second = core.evaluate(&snapshot, toy::demand([&a]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &second, &a);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
}
