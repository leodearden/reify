//! What a body does wrong, and what the host is told: a failure outcome
//! where it follows from the inputs, a pending answer where it does not
//! (contract 9, decision D12).

use std::future::{Future, pending, poll_fn};
use std::pin::pin;

use reify_eval_core::{
    Answer, Binding, Budget, Builder, Cx, Declaration, Failure, InputKey, Kind, KindId, Outcome,
    OutsideFailure, Produced, Ref, Turn,
};

use crate::toy;

/// Makes a second request of the core without waiting for the first.
async fn two_reads_at_once(cx: &Cx) -> Outcome<i64> {
    let (one, other) = (Ref::binding("0"), Ref::binding("1"));
    let mut first = pin!(cx.read::<i64>(&one));
    let mut second = pin!(cx.read::<i64>(&other));
    poll_fn(|context| {
        let _ = first.as_mut().poll(context);
        second.as_mut().poll(context)
    })
    .await
}

struct Impatient;

impl Kind for Impatient {
    type Out = i64;
    const ID: KindId = KindId("toy.impatient");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        Produced::Outcome(two_reads_at_once(cx).await)
    }
}

#[test]
fn a_body_that_makes_two_requests_at_once_ends_with_a_failure() {
    let mut core = Builder::new()
        .kind::<Impatient>()
        .expect("one kind")
        .build();
    let snapshot = core.snapshot([toy::root("hasty", Declaration::new(Impatient::ID))]);

    let request = core.evaluate(&snapshot, toy::roots(&["hasty"]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("hasty"));
    assert_eq!(toy::failed(answer), Failure::TwoRequestsAtOnce);
}

/// Panics when it is dropped.
struct Fuse;

impl Drop for Fuse {
    fn drop(&mut self) {
        panic!("a fuse the body held went off as it was dropped");
    }
}

/// Holds a fuse while it makes two requests at once.
struct ImpatientHolder;

impl Kind for ImpatientHolder {
    type Out = i64;
    const ID: KindId = KindId("toy.impatient_holder");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let _fuse = Fuse;
        Produced::Outcome(two_reads_at_once(cx).await)
    }
}

#[test]
fn a_panic_from_what_a_body_holds_as_its_failed_node_drops_it_does_not_reach_the_host() {
    let mut core = Builder::new()
        .kind::<ImpatientHolder>()
        .expect("one kind")
        .build();
    let snapshot = core.snapshot([toy::root("hasty", Declaration::new(ImpatientHolder::ID))]);

    let request = core.evaluate(&snapshot, toy::roots(&["hasty"]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("hasty"));
    assert_eq!(toy::failed(answer), Failure::TwoRequestsAtOnce);
}

/// Waits on something that is not the core.
struct Rogue;

impl Kind for Rogue {
    type Out = i64;
    const ID: KindId = KindId("toy.rogue");
    const VERSION: u32 = 1;

    async fn run(_: &Cx) -> Produced<i64> {
        pending().await
    }
}

#[test]
fn a_body_that_waits_on_something_other_than_the_core_is_pending_with_that_failure() {
    let mut core = Builder::new().kind::<Rogue>().expect("one kind").build();
    let snapshot = core.snapshot([toy::root("astray", Declaration::new(Rogue::ID))]);

    let request = core.evaluate(&snapshot, toy::roots(&["astray"]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("astray"));
    assert_eq!(toy::pending(answer), OutsideFailure::WaitedOutsideCore);
}

/// Reads its first binding, then panics.
struct Panicking;

impl Kind for Panicking {
    type Out = i64;
    const ID: KindId = KindId("toy.panicking");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let _ = cx.read::<i64>(&Ref::binding("0")).await;
        panic!("toy body broke")
    }
}

#[test]
fn a_body_that_panics_is_pending_with_a_failure_outside_the_inputs_and_the_next_turn_returns() {
    let mut core = toy::with_cell()
        .kind::<Panicking>()
        .expect("distinct kinds")
        .build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::root(
            "broken",
            Declaration::new(Panicking::ID).bind("0", Binding::Input(InputKey::named("a"))),
        ),
        toy::root("total", toy::cell(&["a"])),
    ]);

    let request = core.evaluate(&snapshot, toy::roots(&["broken"]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("broken"));
    match toy::pending(answer) {
        OutsideFailure::BodyPanicked { message } => {
            assert!(message.contains("toy body broke"), "{message}")
        }
        other => panic!("expected a body panic, got {other:?}"),
    }
    assert_eq!(core.turn(), Turn::Idle);

    let later = core.evaluate(&snapshot, toy::roots(&["total"]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &later, &toy::at("total"));
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
}

/// Fails in its own work, outside the inputs, as a section that ran out of
/// memory would.
struct Exhausted;

impl Kind for Exhausted {
    type Out = i64;
    const ID: KindId = KindId("toy.exhausted");
    const VERSION: u32 = 1;

    async fn run(_: &Cx) -> Produced<i64> {
        Produced::Outside(OutsideFailure::OutOfMemory { resource: "toy" })
    }
}

#[test]
fn a_body_whose_own_work_fails_outside_the_inputs_is_reported_pending() {
    let mut core = Builder::new()
        .kind::<Exhausted>()
        .expect("one kind")
        .build();
    let snapshot = core.snapshot([toy::root("spent", Declaration::new(Exhausted::ID))]);

    let request = core.evaluate(&snapshot, toy::roots(&["spent"]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("spent"));
    assert_eq!(
        toy::pending(answer),
        OutsideFailure::OutOfMemory { resource: "toy" }
    );
}
