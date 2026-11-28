//! The host's scenarios (docs/prds/instances-on-the-core.md section 10):
//! `Host` driven directly over sources the second output compiles.

use std::path::Path;

use reify_core::{DefinitionName, InstancePath, PathName};
use reify_eval_instances::{Host, HostAnswer, MemberKinds};
use reify_eval_source::{DefinitionList, compile_entry};

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
