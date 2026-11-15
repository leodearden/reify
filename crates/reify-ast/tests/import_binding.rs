//! Behaviour of `reify_ast::ImportKind::binding`: what each written `import`
//! form binds in the importing module — a namespace qualifier, or entities of
//! the imported module, each under its local name.

use reify_ast::{EntityBinding, ImportBinding, ImportKind};

#[test]
fn module_import_binds_an_unaliased_namespace() {
    let kind = ImportKind::Module;
    assert_eq!(kind.binding(), ImportBinding::Namespace { alias: None });
}

#[test]
fn aliased_module_import_binds_a_namespace_under_its_alias() {
    let kind = ImportKind::Aliased { alias: "m".into() };
    assert_eq!(
        kind.binding(),
        ImportBinding::Namespace { alias: Some("m") }
    );
}

#[test]
fn entity_import_binds_the_entity_under_its_own_name() {
    let kind = ImportKind::Entity("Bolt".into());
    let binding = EntityBinding {
        entity: "Bolt",
        alias: None,
    };
    assert_eq!(kind.binding(), ImportBinding::Entities(vec![binding]));
    assert_eq!(binding.local_name(), "Bolt");
}

#[test]
fn destructured_import_binds_each_entity_in_written_order() {
    let kind = ImportKind::Destructured(vec!["Bolt".into(), "Nut".into()]);
    assert_eq!(
        kind.binding(),
        ImportBinding::Entities(vec![
            EntityBinding {
                entity: "Bolt",
                alias: None,
            },
            EntityBinding {
                entity: "Nut",
                alias: None,
            },
        ])
    );
}

#[test]
fn empty_destructured_import_binds_no_entities() {
    let kind = ImportKind::Destructured(vec![]);
    assert_eq!(kind.binding(), ImportBinding::Entities(vec![]));
}

#[test]
fn aliased_entity_import_binds_the_home_entity_under_its_alias() {
    let kind = ImportKind::EntityAliased {
        entity: "Bolt".into(),
        alias: "StdBolt".into(),
    };
    let binding = EntityBinding {
        entity: "Bolt",
        alias: Some("StdBolt"),
    };
    assert_eq!(kind.binding(), ImportBinding::Entities(vec![binding]));
    assert_eq!(binding.local_name(), "StdBolt");
}
