//! What this crate's unit tests share: a source compiled by the second
//! output, and the kind contract's rule for `steps`, that a result's steps
//! are exactly those `declares` gives a declaration for, each once.

use std::collections::BTreeSet;
use std::path::Path;

use reify_core::{DefinitionName, PathName};
use reify_eval_core::{Kind, Step};
use reify_eval_source::{DefinitionList, compile_entry};

/// The definition list of `source`, compiled as an entry file.
pub(crate) fn compiled(source: &str) -> DefinitionList {
    compile_entry(Path::new("/nonexistent/unit-test.ri"), source)
        .unwrap_or_else(|refusal| panic!("the second output lowers {source:?}: {refusal:?}"))
        .definitions
}

/// The definition `name` of the entry module.
pub(crate) fn in_entry(name: &str) -> DefinitionName {
    DefinitionName::new(None, PathName::new(name).expect("an identifier"))
}

/// The steps of `out`, each asserted to have a declaration and to occur
/// once.
pub(crate) fn declared_steps<K: Kind>(out: &K::Out) -> Vec<Step> {
    let steps: Vec<Step> = K::steps(out).collect();
    for step in &steps {
        assert!(
            K::declares(out, step).is_some(),
            "{step:?} is a step with no declaration"
        );
    }
    let distinct: BTreeSet<&Step> = steps.iter().collect();
    assert_eq!(distinct.len(), steps.len(), "a step repeats: {steps:?}");
    steps
}
