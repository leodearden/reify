//! What the first kinds outside the crate need of its data (contract 9,
//! 5.2, 7.3; PRD D14): a host takes a path and a ref apart and puts them
//! together again, a kind declares a child at a step in a space of its
//! own, and a kind's failure carries a description its readers and the
//! host read back.

use reify_eval_core::{Path, Ref, Root, Space, Step, StepKey};

/// A space of the presence kind's own, beside the default one.
const OWN: Space = Space::Named("test.presence.own");

#[test]
fn a_host_can_take_a_path_apart_and_put_it_together_again() {
    let path = Path::root(Root::named("R"))
        .child(Step::named("p"))
        .child(Step::index(2))
        .child(Step::new(OWN, StepKey::Name("note".into())));

    assert_eq!(path.root_name().name(), "R");
    let parts: Vec<(Space, StepKey)> = path
        .steps()
        .iter()
        .map(|step| (step.space(), step.key().clone()))
        .collect();
    assert_eq!(
        parts,
        [
            (Space::Default, StepKey::Name("p".into())),
            (Space::Default, StepKey::Index(2)),
            (OWN, StepKey::Name("note".into())),
        ]
    );
    let rebuilt = parts.into_iter().fold(
        Path::root(Root::named(path.root_name().name())),
        |rebuilt, (space, key)| rebuilt.child(Step::new(space, key)),
    );
    assert_eq!(rebuilt, path);

    let r = Ref::binding("subject")
        .step(Step::named("p"))
        .step(Step::new(OWN, StepKey::Index(0)));
    let rebuilt = r.steps().iter().fold(
        Ref::binding(r.binding_name()),
        |rebuilt, step| rebuilt.step(Step::new(step.space(), step.key().clone())),
    );
    assert_eq!(rebuilt, r);

    assert_eq!(
        Step::new(Space::Default, StepKey::Name("p".into())),
        Step::named("p")
    );
    assert_eq!(
        Step::new(Space::Default, StepKey::Index(2)),
        Step::index(2)
    );
}
