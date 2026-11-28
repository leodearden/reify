//! The host's scenarios (docs/prds/instances-on-the-core.md section 10):
//! `Host` driven directly over sources the second output compiles.

use std::path::Path;

use reify_core::{DefinitionName, InstancePath, PathName};
use reify_eval_instances::{AnswerFailure, Host, HostAnswer, InstanceFailure, MemberKinds};
use reify_eval_source::{DefinitionList, MemberKind, compile_entry};

/// The definition list of `source`, compiled as an entry file.
fn compiled(source: &str) -> DefinitionList {
    compile_entry(Path::new("/nonexistent/host-scenario.ri"), source)
        .unwrap_or_else(|refusal| panic!("the second output lowers {source:?}: {refusal:?}"))
        .definitions
}

/// The definition `name` of the entry module.
fn in_entry(name: &str) -> DefinitionName {
    DefinitionName::new(None, PathName::new(name).expect("an identifier"))
}

fn path(text: &str) -> InstancePath {
    text.parse()
        .unwrap_or_else(|error| panic!("`{text}` is a path: {error}"))
}

/// Scenario H2: only a declared root is evaluated. A definition in the list
/// that is not a root has no instance, and nothing below it is declared.
#[test]
fn scenario_h2() {
    let definitions = compiled(
        "structure def A { param t : Length = 1mm }\nstructure def B { param u : Length = 2mm }\n",
    );
    let mut host = Host::new();
    let loaded = host.load(&definitions, &MemberKinds::built(), &[in_entry("A")]);

    assert_eq!(host.answer(&loaded, &path("B")), Ok(HostAnswer::Absent));
    assert_eq!(host.answer(&loaded, &path("B.u")), Ok(HostAnswer::Absent));
    assert_eq!(host.answer(&loaded, &path("A")), Ok(HostAnswer::Present));
}

const PLATE: &str = "structure def C { param x : Length = 1mm }\n\
                     structure def Plate { param t : Length = 2mm  sub c : C }\n";

const BLOCK: &str = "structure def Block { param w : Length = 10mm  let body = box(w, w, w)  \
                     sub p : Plate  constraint w > 5mm }\n";

/// The answer of a member no kind evaluates.
fn no_kind_supplied(member: MemberKind, under_guard: bool) -> HostAnswer {
    HostAnswer::Failed(AnswerFailure::Instance(InstanceFailure::NoKindSupplied {
        member,
        under_guard,
    }))
}

/// Scenario C6, second half: a member whose kind no row of the table of
/// member kinds evaluates answers no-kind-supplied, and the instance's
/// other members are still declared, a sub's sub included, which reads its
/// table through the definition table the root declares. The table is an
/// input, so emptying it changes what a sub answers.
#[test]
fn scenario_c6_no_kind_supplied() {
    let definitions = compiled(&format!("{PLATE}{BLOCK}"));
    let roots = [in_entry("Block")];
    let mut host = Host::new();
    let loaded = host.load(&definitions, &MemberKinds::built(), &roots);
    let mut answer = |at: &str| {
        host.answer(&loaded, &path(at))
            .unwrap_or_else(|error| panic!("`{at}` can be asked: {error}"))
    };

    assert_eq!(
        answer("Block.body"),
        no_kind_supplied(MemberKind::GeometryMember, false)
    );
    assert_eq!(answer("Block.p"), HostAnswer::Present);
    assert_ne!(answer("Block.w"), HostAnswer::Absent);
    assert_ne!(answer("Block.p.t"), HostAnswer::Absent);
    assert_eq!(answer("Block.p.c"), HostAnswer::Present);
    assert_ne!(answer("Block.p.c.x"), HostAnswer::Absent);
    assert_eq!(answer("Block.nothing"), HostAnswer::Absent);

    let emptied = host.load(&definitions, &MemberKinds::empty(), &roots);
    assert_eq!(
        host.answer(&emptied, &path("Block.p")),
        Ok(no_kind_supplied(MemberKind::Sub, false))
    );
    let refilled = host.load(&definitions, &MemberKinds::built(), &roots);
    assert_eq!(
        host.answer(&refilled, &path("Block.p")),
        Ok(HostAnswer::Present)
    );
}

/// A member declared under a guard answers no-kind-supplied, since no kind
/// reads a guard yet (PRD 5.2).
#[test]
fn guarded_member_answers_no_kind_supplied() {
    let definitions = compiled(&format!(
        "{PLATE}structure def Block {{ param w : Length = 10mm  sub q : Plate where w > 5mm }}\n"
    ));
    let mut host = Host::new();
    let loaded = host.load(&definitions, &MemberKinds::built(), &[in_entry("Block")]);

    assert_eq!(
        host.answer(&loaded, &path("Block.q")),
        Ok(no_kind_supplied(MemberKind::Sub, true))
    );
}

/// A sub whose definition did not resolve is in the table with no
/// definition, and no instance can be made of it, so it answers
/// no-kind-supplied: it is not dropped as absent (C6).
#[test]
fn a_sub_of_no_definition_answers_no_kind_supplied() {
    let definitions = compiled("structure def Block { sub p : Nope }\n");
    let mut host = Host::new();
    let loaded = host.load(&definitions, &MemberKinds::built(), &[in_entry("Block")]);

    assert_eq!(
        host.answer(&loaded, &path("Block.p")),
        Ok(no_kind_supplied(MemberKind::Sub, false))
    );
}
