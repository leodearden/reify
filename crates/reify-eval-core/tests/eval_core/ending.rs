//! Ending requests, and giving back what sections made (contract 2.5 and
//! 5.5, K11). Every count of live boxes is read after a drain.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reify_eval_core::{
    Binding, Cx, Declaration, Held, InputKey, Kind, KindId, Outcome, Produced, Ref, Root, Turn,
};

use crate::toy::{self, BoxHandle, BoxShape, Shape, ToyKernel, Watch};

#[test]
fn a_request_ended_while_its_body_s_section_runs_leaves_the_kernel_holding_nothing_after_a_drain() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let request = core.evaluate(&snapshot, [Root::named("box")]);
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
    seen: Arc<Arc<AtomicBool>>,
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
    let request = core.evaluate(&snapshot, [Root::named("witnessed")]);
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
    let request = core.evaluate(&snapshot, [Root::named("pair")]);
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
    let request = core.evaluate(&snapshot, [Root::named("box")]);
    core.run();

    let events = core.take_events();
    assert_eq!(
        toy::the_value::<BoxShape>(&events, &request, "box").volume,
        8
    );
    assert_eq!(watch.live_boxes(), 1, "the answer holds the box");

    drop(events);
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
    let first = core.evaluate(&snapshot, [Root::named("x"), Root::named("y")]);
    let ended = first.id();
    let second = core.evaluate(&snapshot, [Root::named("y")]);
    assert_eq!(core.turn(), Turn::SectionsRunning(2));
    keeper.entered();

    core.end(first);
    keeper.open();
    keeper.let_one_through();
    core.drain();

    let events = core.take_events();
    assert!(toy::answers(&events, ended).is_empty());
    assert_eq!(toy::the_value::<BoxShape>(&events, &second, "y").volume, 8);
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
        ["large", "small", "middle"].map(|name| core.evaluate(&snapshot, [Root::named(name)]));
    core.run();

    assert_eq!(watch.sides_in_order(), [3, 1, 2]);
}
