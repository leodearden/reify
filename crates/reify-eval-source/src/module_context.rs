//! What one module's definitions and expressions resolve against: its own
//! compile and the modules that compile bound its names over, read through
//! the existing name binding (`reify_compiler::name_env`).

use std::collections::HashMap;

use reify_ast::ParsedModule;
use reify_compiler::name_env::{self, Origin};
use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_core::{DefinitionName, ModuleName, ModulePath, PathName};

/// One module as the second output lowers it: its parse, its compile, and the
/// modules its compile bound names against.
pub(crate) struct ModuleContext<'m> {
    name: Option<ModuleName>,
    parsed: &'m ParsedModule,
    compiled: &'m CompiledModule,
    /// Where the definition each name binds to is declared.
    definitions: HashMap<&'m str, Origin<'m>>,
}

impl<'m> ModuleContext<'m> {
    /// `tier` is the modules the module's compile binds against when it does
    /// not opt out with `#no_prelude`; `name` is `None` for the entry file.
    pub(crate) fn new(
        name: Option<ModuleName>,
        parsed: &'m ParsedModule,
        compiled: &'m CompiledModule,
        tier: Vec<&'m CompiledModule>,
    ) -> Self {
        let prelude = if has_no_prelude(parsed) {
            Vec::new()
        } else {
            tier
        };
        let definitions =
            name_env::bind::<TopologyTemplate, _>(&compiled.templates, prelude.iter().copied())
                .winning_candidates()
                .iter()
                .map(|candidate| (candidate.definition.name.as_str(), candidate.origin))
                .collect();
        Self {
            name,
            parsed,
            compiled,
            definitions,
        }
    }

    pub(crate) fn parsed(&self) -> &'m ParsedModule {
        self.parsed
    }

    /// The qualified name of a definition this module declares.
    pub(crate) fn own_definition(&self, name: &str) -> DefinitionName {
        DefinitionName::new(self.name.clone(), definition_segment(name))
    }

    /// The existing compile's template of a definition this module declares.
    pub(crate) fn own_template(&self, name: &str) -> Option<&'m TopologyTemplate> {
        self.compiled.templates.iter().find(|t| t.name == name)
    }

    /// The structure or occurrence definition `written` names in this module,
    /// bound as the existing compile binds it: this module's own, then its
    /// imports', then the standard library's.
    pub(crate) fn definition_named(&self, written: &str) -> Option<DefinitionName> {
        let module = match self.definitions.get(written)?.module() {
            None => self.name.clone(),
            Some(path) => Some(module_name(path)),
        };
        Some(DefinitionName::new(module, definition_segment(written)))
    }
}

/// Whether a module opts out of every prelude, the test the existing compile
/// applies (`compile_builder::pre_pass::effective_prelude`).
pub(crate) fn has_no_prelude(parsed: &ParsedModule) -> bool {
    parsed.pragmas.iter().any(|p| p.name == "no_prelude")
}

/// A module's name as a definition name's module.
pub(crate) fn module_name(path: &ModulePath) -> ModuleName {
    ModuleName::new(path.0.iter().map(|s| definition_segment(s)).collect())
        .unwrap_or_else(|e| panic!("a compiled module's path `{path}` is a module name: {e:?}"))
}

/// A definition or module segment the parser accepted as an identifier.
fn definition_segment(text: &str) -> PathName {
    PathName::new(text)
        .unwrap_or_else(|e| panic!("the parser accepts `{text}` only as an identifier: {e:?}"))
}
