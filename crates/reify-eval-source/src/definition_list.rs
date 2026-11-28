//! The definition list: every structure and occurrence definition of the
//! program, each with its member table, keyed by its qualified name.

use reify_ast::{Declaration, MemberDecl, TraitBoundRef};
use reify_core::{DefinitionName, Diagnostic};

use crate::member_table::{self, DefinitionBody, MemberTable};
use crate::module_context::{ModuleContext, ModuleSet};
use crate::refusal::SourceRefusal;

/// One structure or occurrence definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: DefinitionName,
    pub members: MemberTable,
}

/// The program's definitions: the entry module's, then the user modules',
/// then the standard library's, each module's in source order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DefinitionList {
    definitions: Vec<Definition>,
}

impl DefinitionList {
    pub(crate) fn new(definitions: Vec<Definition>) -> Self {
        Self { definitions }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Definition> {
        self.definitions.iter()
    }

    pub fn get(&self, name: &DefinitionName) -> Option<&Definition> {
        self.definitions.iter().find(|d| &d.name == name)
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

impl<'l> IntoIterator for &'l DefinitionList {
    type Item = &'l Definition;
    type IntoIter = std::slice::Iter<'l, Definition>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// What the lowering reads of a structure or occurrence definition: the two
/// are written alike and lowered alike.
struct DefinitionSource<'a> {
    name: &'a str,
    bounds: &'a [TraitBoundRef],
    members: &'a [MemberDecl],
}

impl<'a> DefinitionSource<'a> {
    fn of(declaration: &'a Declaration) -> Option<Self> {
        match declaration {
            Declaration::Structure(def) => Some(Self {
                name: &def.name,
                bounds: &def.trait_bounds,
                members: &def.members,
            }),
            Declaration::Occurrence(def) => Some(Self {
                name: &def.name,
                bounds: &def.trait_bounds,
                members: &def.members,
            }),
            _ => None,
        }
    }
}

/// The definitions `module` declares, in source order; the traits they
/// conform to may be declared in any of `modules`.
pub(crate) fn lower_module<'p, 'm>(
    module: &'p ModuleContext<'m>,
    modules: &ModuleSet<'p, 'm>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Definition>, SourceRefusal> {
    module
        .parsed()
        .declarations
        .iter()
        .filter_map(DefinitionSource::of)
        .map(|source| {
            let name = module.own_definition(source.name);
            let Some(template) = module.own_template(source.name) else {
                return Err(SourceRefusal::TemplateMissing { definition: name });
            };
            let body = DefinitionBody {
                name: &name,
                bounds: source.bounds,
                members: source.members,
            };
            let members =
                member_table::lower_definition(&body, template, module, modules, diagnostics);
            Ok(Definition { name, members })
        })
        .collect()
}
