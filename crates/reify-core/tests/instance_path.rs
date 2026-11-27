//! The typed instance path's construction invariants, through its public
//! seam only: every value is built by a validating constructor, so a path
//! that breaks an invariant cannot be held.

use reify_core::{
    AutoAnchor, AutoSite, AutoSiteSegment, CallChain, DefinitionName, InstancePath,
    InstancePathError, ModuleName, PathElement, PathKey, PathName, PathStep,
};

fn name(text: &str) -> PathName {
    PathName::new(text).unwrap_or_else(|e| panic!("`{text}` should be a name: {e}"))
}

fn chain(indices: &[u64]) -> CallChain {
    CallChain::new(indices.to_vec()).expect("a non-empty chain")
}

fn entry(definition: &str) -> DefinitionName {
    DefinitionName::new(None, name(definition))
}

fn key(text: &str) -> PathKey {
    PathKey::new(text)
}

#[test]
fn a_path_name_is_a_reify_identifier() {
    for text in ["Asm", "h_body", "_x", "f1", "summary", "at", "connect"] {
        assert_eq!(name(text).as_str(), text);
    }
    for text in ["", "1a", "a.b", "a/b", "a b", "a-b", "a\"b", "é"] {
        let err = PathName::new(text).expect_err(text);
        assert_eq!(
            err,
            InstancePathError::NotAnIdentifier {
                text: text.to_string()
            }
        );
        let message = err.to_string();
        assert!(
            message.contains("a Reify identifier, [A-Za-z_][A-Za-z0-9_]*"),
            "{message}"
        );
        assert!(message.contains(&format!("`{text}`")), "{message}");
    }
}

#[test]
fn a_module_name_has_at_least_one_segment() {
    assert_eq!(
        ModuleName::new(vec![]),
        Err(InstancePathError::EmptyModule)
    );
    let module = ModuleName::new(vec![name("std"), name("units")]).expect("two segments");
    assert_eq!(module.segments(), &[name("std"), name("units")]);
}

#[test]
fn a_call_chain_has_at_least_one_index() {
    assert_eq!(CallChain::new(vec![]), Err(InstancePathError::EmptyChain));
    assert_eq!(chain(&[0, 2]).indices(), &[0, 2]);
}

#[test]
fn an_auto_site_has_segments_and_every_chain_names_an_argument() {
    let a = || AutoAnchor::Member {
        name: name("a"),
        key: None,
    };

    assert_eq!(AutoSite::new(a(), vec![]), Err(InstancePathError::EmptyAutoSite));
    assert_eq!(
        AutoSite::new(a(), vec![AutoSiteSegment::Chain(chain(&[0]))]),
        Err(InstancePathError::ChainWithoutArgument {
            chain: chain(&[0]),
            followed_by: None,
        })
    );
    assert_eq!(
        AutoSite::new(
            a(),
            vec![
                AutoSiteSegment::Chain(chain(&[0])),
                AutoSiteSegment::Chain(chain(&[1])),
                AutoSiteSegment::Argument(name("t")),
            ]
        ),
        Err(InstancePathError::ChainWithoutArgument {
            chain: chain(&[0]),
            followed_by: Some(AutoSiteSegment::Chain(chain(&[1]))),
        })
    );
    assert_eq!(
        AutoSite::new(
            a(),
            vec![AutoSiteSegment::Chain(chain(&[0])), AutoSiteSegment::Placement]
        ),
        Err(InstancePathError::ChainWithoutArgument {
            chain: chain(&[0]),
            followed_by: Some(AutoSiteSegment::Placement),
        })
    );

    let accepted = [
        (a(), vec![AutoSiteSegment::Argument(name("len"))]),
        (
            AutoAnchor::Member {
                name: name("d"),
                key: None,
            },
            vec![
                AutoSiteSegment::Argument(name("p")),
                AutoSiteSegment::Chain(chain(&[0])),
                AutoSiteSegment::Argument(name("t")),
            ],
        ),
        (
            AutoAnchor::Member {
                name: name("both"),
                key: None,
            },
            vec![
                AutoSiteSegment::Chain(chain(&[1])),
                AutoSiteSegment::Argument(name("t")),
            ],
        ),
        (
            AutoAnchor::Member {
                name: name("vents"),
                key: Some(key("a")),
            },
            vec![AutoSiteSegment::Argument(name("w"))],
        ),
        (
            AutoAnchor::Connect(0),
            vec![AutoSiteSegment::Argument(name("p"))],
        ),
        (a(), vec![AutoSiteSegment::Placement]),
    ];
    for (anchor, segments) in accepted {
        let site = AutoSite::new(anchor.clone(), segments.clone())
            .unwrap_or_else(|e| panic!("{anchor:?} {segments:?}: {e}"));
        assert_eq!(site.anchor(), &anchor);
        assert_eq!(site.segments(), segments.as_slice());
    }
}

#[test]
fn then_returns_a_new_path_and_leaves_the_receiver_alone() {
    let root = InstancePath::new(entry("Asm"));
    let thin = root
        .then(PathStep::Member(name("thin")))
        .expect("a member after the root");
    assert!(root.steps().is_empty());
    assert_eq!(thin.steps(), &[PathStep::Member(name("thin"))]);
    assert_eq!(thin.root(), root.root());
}

#[test]
fn an_element_may_follow_a_member_a_constraint_or_a_made_instance() {
    let accepted = [
        vec![
            PathStep::Member(name("vents")),
            PathStep::Element(PathElement::Index(2)),
        ],
        vec![
            PathStep::Constraint(0),
            PathStep::Element(PathElement::Key(key("intake"))),
        ],
        vec![
            PathStep::Made(chain(&[0])),
            PathStep::Element(PathElement::Index(1)),
        ],
        vec![PathStep::Braces, PathStep::Member(name("b"))],
    ];
    for steps in accepted {
        let mut path = InstancePath::new(entry("Asm"));
        for step in &steps {
            path = path
                .then(step.clone())
                .unwrap_or_else(|e| panic!("{steps:?}: {e}"));
        }
        assert_eq!(path.steps(), steps.as_slice());
        assert_eq!(
            InstancePath::from_steps(entry("Asm"), steps.clone()),
            Ok(path)
        );
    }
}

#[test]
fn an_element_cannot_follow_a_bare_made_or_open() {
    for bare in [PathStep::MadeList, PathStep::Open(None)] {
        let element = PathElement::Index(0);
        let expected = InstancePathError::ElementAfterBareStep {
            previous: bare.clone(),
            element: element.clone(),
        };

        let after_bare = InstancePath::new(entry("Asm"))
            .then(PathStep::Member(name("c")))
            .and_then(|p| p.then(bare.clone()))
            .expect("a bare made or open after a member");
        let err = after_bare
            .then(PathStep::Element(element.clone()))
            .expect_err("an element after a bare step");
        assert_eq!(err, expected);
        let message = err.to_string();
        assert!(
            message.contains(
                "an element step cannot follow a bare `made` or `open`: \
                 their instances are named by a chain"
            ),
            "{message}"
        );
        assert!(message.contains(&format!("{bare:?}")), "{message}");

        assert_eq!(
            InstancePath::from_steps(
                entry("Asm"),
                vec![
                    PathStep::Member(name("c")),
                    bare.clone(),
                    PathStep::Element(element.clone()),
                ]
            ),
            Err(expected)
        );
    }
}

#[test]
fn an_opened_instance_named_by_its_chain_may_be_followed_by_an_element() {
    let path = InstancePath::from_steps(
        entry("Asm"),
        vec![
            PathStep::Open(Some(chain(&[1]))),
            PathStep::Element(PathElement::Index(0)),
        ],
    );
    assert!(path.is_ok(), "{path:?}");
}

#[test]
fn accessors_read_back_what_was_built() {
    let module = ModuleName::new(vec![name("std"), name("units")]).expect("two segments");
    let plate = DefinitionName::new(Some(module.clone()), name("Plate"));
    assert_eq!(plate.module(), Some(&module));
    assert_eq!(plate.name(), &name("Plate"));
    assert_eq!(entry("Asm").module(), None);

    let path = InstancePath::from_steps(plate.clone(), vec![PathStep::Summary])
        .expect("a summary after the root");
    assert_eq!(path.root(), &plate);
    assert_eq!(path.steps(), &[PathStep::Summary]);

    assert_eq!(key("a.b/c]\"").as_str(), "a.b/c]\"");
    assert_eq!(name("h_body").as_str(), "h_body");
}
