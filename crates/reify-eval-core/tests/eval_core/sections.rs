//! Exclusive sections on a resource's own thread (contract 2.3, K9): other
//! bodies run while a section does, and a body's handle on the core never
//! leaves the core's thread.

use std::any::type_name;
use std::thread;

use reify_eval_core::{
    Answer, Builder, Cx, Declaration, Event, Held, Kind, KindId, Outcome, OutsideFailure, Path,
    Produced, Release, ResourceDeath, Root, Turn,
};

use crate::toy::{self, BoxHandle, BoxShape, ToyKernel, Watch};

#[test]
fn a_second_body_runs_to_completion_while_a_first_waits_on_a_section_still_running() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::named("a", 1_i64),
        toy::named("b", 2_i64),
        toy::root("box", toy::shape("side")),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);

    let first = core.evaluate(&snapshot, toy::roots(&["box"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    let second = core.evaluate(&snapshot, toy::roots(&["total"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    let events = core.take_events();
    let total = toy::the_answer(&events, &second, &toy::at("total"));
    assert_eq!(total.downcast::<i64>(), Answer::Final(Outcome::of(3)));
    assert!(
        toy::answers(&events, first.id()).is_empty(),
        "the shape still waits on its section"
    );

    keeper.open();
    core.run();
    let shape = toy::the_value::<BoxShape>(&core.take_events(), &first, &toy::at("box"));
    assert_eq!(shape.volume, 8);
    assert_eq!(*shape.handle, BoxHandle(1));
    assert_ne!(shape.built_on, thread::current().id());
    assert_eq!(core.counters().sections(), 1);
}

#[test]
fn a_section_for_a_resource_nobody_added_is_pending_with_that_failure() {
    let mut core = toy::with_toys().build();
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);

    let request = core.evaluate(&snapshot, toy::roots(&["box"]));
    core.run();

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("box"));
    let missing = OutsideFailure::NoResource {
        resource: type_name::<ToyKernel>(),
    };
    assert_eq!(toy::pending(answer), missing);
}

#[test]
fn the_core_can_be_moved_to_another_thread_mid_evaluation() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 3_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let request = core.evaluate(&snapshot, toy::roots(&["box"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    let driver = thread::spawn(move || {
        core.run();
        let events = core.take_events();
        (core, events)
    });
    keeper.let_one_through();
    let (_core, events) = driver.join().expect("the driving thread finishes");

    let shape = toy::the_value::<BoxShape>(&events, &request, &toy::at("box"));
    assert_eq!(shape.volume, 27);
}

#[test]
fn a_section_s_progress_notes_reach_the_host_while_it_still_runs() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated_after_progress(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let first = core.evaluate(&snapshot, toy::roots(&["box"]));
    let second = core.evaluate(&snapshot, toy::roots(&["box"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    keeper.let_one_through();
    keeper.progressed();
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    let events = core.take_events();
    for event in &events {
        if let Event::Progress { snapshot: told, .. } = event {
            assert_eq!(*told, snapshot.id(), "a note is tagged with its snapshot");
        }
    }
    for request in [first.id(), second.id()] {
        let notes = toy::notes(&events, request);
        assert_eq!(
            notes.len(),
            1,
            "one note for each request that wants the shape"
        );
        assert_eq!(notes[0].0, Path::root(Root::named("box")));
        assert_eq!(notes[0].1.downcast_ref::<i64>(), Some(&2));
        assert!(
            toy::answers(&events, request).is_empty(),
            "the section still runs"
        );
    }

    keeper.open_after_progress();
    core.run();
}

#[test]
fn a_section_sees_cancelled_when_its_request_is_ended_without_waiting() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let request = core.evaluate(&snapshot, toy::roots(&["box"]));
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    core.end(request);
    keeper.open();
    core.drain();

    assert!(watch.saw_cancelled());
    assert_eq!(watch.live_boxes(), 0);
}

/// Runs a section that panics on the toy kernel.
struct Fragile;

impl Kind for Fragile {
    type Out = i64;
    const ID: KindId = KindId("toy.fragile");

    async fn run(cx: &Cx) -> Produced<i64> {
        let made = cx
            .exclusive(|_: &mut ToyKernel, _| -> i64 { panic!("toy kernel broke") })
            .await;
        Produced::Outcome(Outcome::of(made))
    }
}

fn with_fragile() -> Builder {
    toy::with_toys().kind::<Fragile>().expect("distinct kinds")
}

fn assert_section_panicked(failure: OutsideFailure) {
    match failure {
        OutsideFailure::SectionPanicked { resource, message } => {
            assert_eq!(resource, type_name::<ToyKernel>());
            assert!(message.contains("toy kernel broke"), "{message}");
        }
        other => panic!("expected a section panic, got {other:?}"),
    }
}

#[test]
fn a_section_that_panics_ends_its_node_pending_with_a_failure_outside_the_inputs_and_no_later_call_hangs()
 {
    let watch = Watch::default();
    let mut core = toy::ungated(with_fragile(), &watch);
    let snapshot = core.snapshot([
        toy::named("a", 1_i64),
        toy::named("b", 2_i64),
        toy::root("fragile", Declaration::new(Fragile::ID)),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);

    let request = core.evaluate(&snapshot, toy::roots(&["fragile"]));
    core.run();
    assert_section_panicked(toy::pending(toy::the_answer(
        &core.take_events(),
        &request,
        &toy::at("fragile"),
    )));

    assert_eq!(core.turn(), Turn::Idle);
    assert_eq!(core.wait(), Turn::Idle);
    core.run();
    core.drain();

    let later = core.evaluate(&snapshot, toy::roots(&["total"]));
    core.run();
    let total = toy::the_answer(&core.take_events(), &later, &toy::at("total"));
    assert_eq!(total.downcast::<i64>(), Answer::Final(Outcome::of(3)));
}

#[test]
fn after_a_section_panics_its_resource_is_dead_and_later_sections_for_it_fail_the_same_way() {
    let watch = Watch::default();
    let mut core = toy::ungated(with_fragile(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("fragile", Declaration::new(Fragile::ID)),
        toy::root("shape", toy::shape("side")),
    ]);
    let dead = OutsideFailure::ResourceDead {
        resource: type_name::<ToyKernel>(),
        death: ResourceDeath::SectionPanicked {
            message: String::from("toy kernel broke"),
        },
    };

    let request = core.evaluate(&snapshot, toy::roots(&["fragile", "shape"]));
    assert_eq!(
        core.turn(),
        Turn::SectionsRunning(2),
        "both sections reach the kernel, the panicking one first"
    );
    core.run();
    let events = core.take_events();
    assert_section_panicked(toy::pending(toy::answer_at(&events, &request, &toy::at("fragile"))));
    assert_eq!(
        toy::pending(toy::answer_at(&events, &request, &toy::at("shape"))),
        dead
    );

    let later = core.evaluate(&snapshot, toy::roots(&["shape"]));
    core.run();
    assert_eq!(
        toy::pending(toy::the_answer(&core.take_events(), &later, &toy::at("shape"))),
        dead
    );
    core.drain();
}

#[test]
fn a_release_for_a_dead_resource_does_not_hang_the_drain() {
    let watch = Watch::default();
    let mut core = toy::ungated(with_fragile(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("fragile", Declaration::new(Fragile::ID)),
        toy::root("shape", toy::shape("side")),
    ]);
    let made = core.evaluate(&snapshot, toy::roots(&["shape"]));
    core.run();
    let kept = core.take_events();
    assert_eq!(toy::the_value::<BoxShape>(&kept, &made, &toy::at("shape")).volume, 8);

    let broken = core.evaluate(&snapshot, toy::roots(&["fragile"]));
    core.run();
    assert_section_panicked(toy::pending(toy::the_answer(
        &core.take_events(),
        &broken,
        &toy::at("fragile"),
    )));

    core.end(made);
    core.end(broken);
    drop((snapshot, kept));
    core.drain();
}

/// A resource whose making panics.
struct Unmade;

/// Runs a section on `Unmade`.
struct OnUnmade;

impl Kind for OnUnmade {
    type Out = i64;
    const ID: KindId = KindId("toy.on_unmade");

    async fn run(cx: &Cx) -> Produced<i64> {
        let made = cx.exclusive(|_: &mut Unmade, _| 1_i64).await;
        Produced::Outcome(Outcome::of(made))
    }
}

#[test]
fn a_section_on_a_resource_whose_making_panicked_is_pending_with_that_panic() {
    let mut core = Builder::new().kind::<OnUnmade>().expect("one kind").build();
    core.add_resource(|| -> Unmade { panic!("the resource could not be made") });
    let snapshot = core.snapshot([toy::root("doomed", Declaration::new(OnUnmade::ID))]);

    let request = core.evaluate(&snapshot, toy::roots(&["doomed"]));
    core.run();

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("doomed"));
    let dead = OutsideFailure::ResourceDead {
        resource: type_name::<Unmade>(),
        death: ResourceDeath::MakePanicked {
            message: String::from("the resource could not be made"),
        },
    };
    assert_eq!(toy::pending(answer), dead);
    core.drain();
}

/// A resource whose shards panic when they are given back.
struct Brittle;

#[derive(Debug)]
struct Shard;

impl Release<Brittle> for Shard {
    fn release(self, _: &mut Brittle) {
        panic!("giving back a shard broke the resource")
    }
}

/// Makes a shard in `Brittle` and holds it.
struct MakeShard;

impl Kind for MakeShard {
    type Out = Held<Brittle, Shard>;
    const ID: KindId = KindId("toy.make_shard");

    async fn run(cx: &Cx) -> Produced<Self::Out> {
        let shard = cx
            .exclusive(|_: &mut Brittle, section| section.hold(Shard))
            .await;
        Produced::Outcome(Outcome::of(shard))
    }
}

#[test]
fn after_a_release_panics_its_resource_is_dead_and_later_sections_name_that_panic() {
    let mut core = Builder::new()
        .kind::<MakeShard>()
        .expect("one kind")
        .build();
    core.add_resource(|| Brittle);
    let inputs = || [toy::root("shard", Declaration::new(MakeShard::ID))];
    let snapshot = core.snapshot(inputs());

    let first = core.evaluate(&snapshot, toy::roots(&["shard"]));
    core.run();
    let events = core.take_events();
    let made = toy::the_answer(&events, &first, &toy::at("shard"));
    assert!(matches!(made, Answer::Final(Outcome::Value(_))), "{made:?}");
    core.end(first);
    drop((made, events, snapshot));
    core.drain();

    let snapshot = core.snapshot(inputs());
    let second = core.evaluate(&snapshot, toy::roots(&["shard"]));
    core.run();
    let answer = toy::the_answer(&core.take_events(), &second, &toy::at("shard"));
    let dead = OutsideFailure::ResourceDead {
        resource: type_name::<Brittle>(),
        death: ResourceDeath::ReleasePanicked {
            message: String::from("giving back a shard broke the resource"),
        },
    };
    assert_eq!(toy::pending(answer), dead);
    core.drain();
}
