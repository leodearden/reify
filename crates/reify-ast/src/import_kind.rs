//! The forms an `import` takes, and what each binds in the importing module.

/// The kind of import (determines how names are brought into scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// `import std.math` — import entire module
    Module,
    /// `import std.math.sqrt` — import a single entity
    Entity(String),
    /// `import std.mech.{Bolt, Nut}` — import multiple entities
    Destructured(Vec<String>),
    /// `import std.mech as m` — import module with alias
    Aliased { alias: String },
    /// `import std.mech.Bolt as StdBolt` — import entity with alias
    EntityAliased { entity: String, alias: String },
}

/// What one import binds in the importing module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportBinding<'a> {
    /// The imported module's whole pub surface, reached through one qualifier:
    /// `alias` when written, else the import path's final segment.
    Namespace { alias: Option<&'a str> },
    /// These entities of the imported module, each under its local name.
    Entities(Vec<EntityBinding<'a>>),
}

/// One entity an import binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityBinding<'a> {
    /// The entity's name in the imported (home) module.
    pub entity: &'a str,
    /// The `as` name, if written.
    pub alias: Option<&'a str>,
}

impl<'a> EntityBinding<'a> {
    /// The name the importing module refers to the entity by.
    pub fn local_name(&self) -> &'a str {
        self.alias.unwrap_or(self.entity)
    }
}

impl ImportKind {
    /// The one statement of what this import binds; consumers derive their
    /// answers from it rather than matching `ImportKind`'s variants.
    pub fn binding(&self) -> ImportBinding<'_> {
        match self {
            ImportKind::Module => ImportBinding::Namespace { alias: None },
            ImportKind::Aliased { alias } => ImportBinding::Namespace { alias: Some(alias) },
            ImportKind::Entity(entity) => ImportBinding::Entities(vec![EntityBinding {
                entity,
                alias: None,
            }]),
            ImportKind::Destructured(entities) => ImportBinding::Entities(
                entities
                    .iter()
                    .map(|entity| EntityBinding {
                        entity,
                        alias: None,
                    })
                    .collect(),
            ),
            ImportKind::EntityAliased { entity, alias } => {
                ImportBinding::Entities(vec![EntityBinding {
                    entity,
                    alias: Some(alias),
                }])
            }
        }
    }
}
