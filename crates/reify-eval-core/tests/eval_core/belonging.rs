//! What an event names, and what a snapshot and a request belong to: each
//! event is tagged with its request and that request's snapshot (contract
//! 2.4), and a snapshot or a request is good only on the core that made it.

use std::panic::{AssertUnwindSafe, catch_unwind};

use reify_eval_core::{Event, RequestId, SnapshotId};

use crate::toy;

/// The snapshot each event for `request` is tagged with.
fn snapshots_told(events: &[Event], request: RequestId) -> Vec<SnapshotId> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Answered {
                request: to,
                snapshot,
                ..
            } if *to == request => Some(*snapshot),
            _ => None,
        })
        .collect()
}

#[test]
fn each_answer_is_tagged_with_its_request_and_the_snapshot_that_request_was_made_on() {
    let mut core = toy::core();
    let (earlier, later) = (core.snapshot([]), core.snapshot([]));
    assert_ne!(earlier.id(), later.id());

    let on_earlier = core.evaluate(&earlier, toy::roots(&["missing"]));
    let on_later = core.evaluate(&later, toy::roots(&["missing"]));
    core.run();

    let events = core.take_events();
    assert_eq!(snapshots_told(&events, on_earlier.id()), [earlier.id()]);
    assert_eq!(snapshots_told(&events, on_later.id()), [later.id()]);
}

/// The text of the panic `refused` holds.
fn refusal(refused: std::thread::Result<()>) -> String {
    let payload = refused.expect_err("the call is refused");
    payload
        .downcast_ref::<String>()
        .expect("the refusal's message is formatted")
        .clone()
}

#[test]
fn a_core_refuses_a_snapshot_another_core_made() {
    let mut maker = toy::core();
    let mut other = toy::core();
    let snapshot = maker.snapshot([]);

    let refused = catch_unwind(AssertUnwindSafe(|| {
        let _ = other.evaluate(&snapshot, toy::roots(&["missing"]));
    }));
    let message = refusal(refused);
    assert!(message.contains("a snapshot made by core "), "{message}");
}

#[test]
fn a_core_refuses_to_end_a_request_another_core_opened() {
    let mut maker = toy::core();
    let mut other = toy::core();
    let snapshot = maker.snapshot([]);
    let request = maker.evaluate(&snapshot, toy::roots(&["missing"]));

    let refused = catch_unwind(AssertUnwindSafe(|| other.end(request)));
    let message = refusal(refused);
    assert!(message.contains("a request made by core "), "{message}");
}
