//! Exclusive sections on a resource's own thread (contract 2.3, K9): other
//! bodies run while a section does, and a body's handle on the core never
//! leaves the core's thread.

use std::any::type_name;
use std::thread;

use reify_eval_core::{Answer, Outcome, OutsideFailure, Path, Root, Turn};

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

    let first = core.evaluate(&snapshot, [Root::named("box")]);
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    let second = core.evaluate(&snapshot, [Root::named("total")]);
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    let events = core.take_events();
    let total = toy::the_answer(&events, &second, "total");
    assert_eq!(total.downcast::<i64>(), Answer::Final(Outcome::of(3)));
    assert!(toy::answers(&events, first.id()).is_empty(), "the shape still waits on its section");

    keeper.open();
    core.run();
    let shape = toy::the_value::<BoxShape>(&core.take_events(), &first, "box");
    assert_eq!(shape.volume, 8);
    assert_eq!(*shape.handle, BoxHandle(1));
    assert_ne!(shape.built_on, thread::current().id());
    assert_eq!(core.counters().sections(), 1);
}

#[test]
fn a_section_for_a_resource_nobody_added_is_pending_with_that_failure() {
    let mut core = toy::with_toys().build();
    let snapshot = core.snapshot([toy::named("side", 2_i64), toy::root("box", toy::shape("side"))]);

    let request = core.evaluate(&snapshot, [Root::named("box")]);
    core.run();

    let answer = toy::the_answer(&core.take_events(), &request, "box");
    let missing = OutsideFailure::NoResource { resource: type_name::<ToyKernel>() };
    assert_eq!(toy::pending(answer), missing);
}

#[test]
fn the_core_can_be_moved_to_another_thread_mid_evaluation() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([toy::named("side", 3_i64), toy::root("box", toy::shape("side"))]);
    let request = core.evaluate(&snapshot, [Root::named("box")]);
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    let driver = thread::spawn(move || {
        core.run();
        let events = core.take_events();
        (core, events)
    });
    keeper.let_one_through();
    let (_core, events) = driver.join().expect("the driving thread finishes");

    let shape = toy::the_value::<BoxShape>(&events, &request, "box");
    assert_eq!(shape.volume, 27);
}

#[test]
fn a_section_s_progress_notes_reach_the_host_while_it_still_runs() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated_after_progress(toy::with_toys(), &watch);
    let snapshot = core.snapshot([toy::named("side", 2_i64), toy::root("box", toy::shape("side"))]);
    let first = core.evaluate(&snapshot, [Root::named("box")]);
    let second = core.evaluate(&snapshot, [Root::named("box")]);
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    keeper.let_one_through();
    keeper.progressed();
    assert_eq!(core.turn(), Turn::SectionsRunning(1));

    let events = core.take_events();
    for request in [first.id(), second.id()] {
        let notes = toy::notes(&events, request);
        assert_eq!(notes.len(), 1, "one note for each request that wants the shape");
        assert_eq!(notes[0].0, Path::root(Root::named("box")));
        assert_eq!(notes[0].1.downcast_ref::<i64>(), Some(&2));
        assert!(toy::answers(&events, request).is_empty(), "the section still runs");
    }

    keeper.open_after_progress();
    core.run();
}

#[test]
fn a_section_sees_cancelled_when_its_request_is_ended_without_waiting() {
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([toy::named("side", 2_i64), toy::root("box", toy::shape("side"))]);
    let request = core.evaluate(&snapshot, [Root::named("box")]);
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    keeper.entered();

    core.end(request);
    keeper.open();
    core.drain();

    assert!(watch.saw_cancelled());
    assert_eq!(watch.live_boxes(), 0);
}
