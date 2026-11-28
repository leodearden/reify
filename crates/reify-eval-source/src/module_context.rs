//! What one module's definitions and expressions resolve against: its own
//! compile and the modules that compile bound its names over, read through
//! the existing name binding (`reify_compiler::name_env`).

use std::collections::HashMap;

use reify_ast::{Declaration, ParsedModule, TraitDecl};
use reify_compiler::name_env::{self, Origin, Tier};
use reify_compiler::{CompiledModule, CompiledTrait, EntityKind, TopologyTemplate};
use reify_core::{DefinitionName, ModuleName, ModulePath, PathName};
use reify_ir::{CompiledFunction, EnumDef};

/// One module as the second output lowers it: its parse, its compile, and the
/// modules its compile bound names against.
pub(crate) struct ModuleContext<'m> {
    name: Option<ModuleName>,
    parsed: &'m ParsedModule,
    compiled: &'m CompiledModule,
    /// What a sub's written target can bind to, any kind: the module's own
    /// templates, each import's `pub` ones, then every stdlib one.
    sub_targets: HashMap<&'m str, (&'m TopologyTemplate, Origin<'m>)>,
    /// The trait each name binds to, and where it is declared.
    traits: HashMap<&'m str, (&'m CompiledTrait, Origin<'m>)>,
    /// The templates an expression's constructor call or sub read can name:
    /// the module's own, and every prelude structure that wins its name.
    templates: HashMap<String, &'m TopologyTemplate>,
    enums: Vec<EnumDef>,
    functions: Vec<CompiledFunction>,
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
        let (imports, stdlib): (Vec<&CompiledModule>, Vec<&CompiledModule>) = prelude
            .iter()
            .copied()
            .partition(|module| Origin::Module(&module.path).tier() == Tier::Import);
        let sub_targets = name_env::bind_scopes::<TopologyTemplate, _, _, _>(
            &compiled.templates,
            imports.iter().copied(),
            stdlib.iter().copied(),
        )
        .winning_candidates()
        .iter()
        .map(|candidate| {
            let target = candidate.definition;
            (target.name.as_str(), (target, candidate.origin))
        })
        .collect();
        let bound =
            name_env::bind::<TopologyTemplate, _>(&compiled.templates, prelude.iter().copied());
        let templates = bound
            .winning_candidates()
            .iter()
            .filter(|candidate| {
                candidate.origin == Origin::Local
                    || candidate.definition.entity_kind == EntityKind::Structure
            })
            .map(|candidate| (candidate.definition.name.clone(), candidate.definition))
            .collect();
        let traits =
            name_env::bind::<CompiledTrait, _>(&compiled.trait_defs, prelude.iter().copied())
                .winning_candidates()
                .iter()
                .map(|candidate| {
                    let declared = candidate.definition;
                    (declared.name.as_str(), (declared, candidate.origin))
                })
                .collect();
        let enums = name_env::bind::<EnumDef, _>(&compiled.enum_defs, prelude.iter().copied())
            .winners()
            .cloned()
            .collect();
        let functions =
            name_env::bind::<CompiledFunction, _>(&compiled.functions, prelude.iter().copied())
                .winners()
                .cloned()
                .collect();
        Self {
            name,
            parsed,
            compiled,
            sub_targets,
            traits,
            templates,
            enums,
            functions,
        }
    }

    /// The templates the module's expressions can name, for a definition's
    /// compile scope.
    pub(crate) fn templates(&self) -> &HashMap<String, &'m TopologyTemplate> {
        &self.templates
    }

    pub(crate) fn enums(&self) -> &[EnumDef] {
        &self.enums
    }

    pub(crate) fn functions(&self) -> &[CompiledFunction] {
        &self.functions
    }

    pub(crate) fn parsed(&self) -> &'m ParsedModule {
        self.parsed
    }

    pub(crate) fn path(&self) -> &'m ModulePath {
        &self.parsed.path
    }

    /// The qualified name of a definition this module declares.
    pub(crate) fn own_definition(&self, name: &str) -> DefinitionName {
        DefinitionName::new(self.name.clone(), definition_segment(name))
    }

    /// The existing compile's template of a definition this module declares.
    pub(crate) fn own_template(&self, name: &str) -> Option<&'m TopologyTemplate> {
        self.compiled.templates.iter().find(|t| t.name == name)
    }

    /// The definition a sub's written target binds to, as the existing
    /// compile checks it: this module's own, then each import's `pub` ones,
    /// then the standard library's.
    pub(crate) fn sub_target(&self, written: &str) -> Option<SubTarget<'m>> {
        let (template, origin) = *self.sub_targets.get(written)?;
        Some(SubTarget {
            name: self.qualified(origin, written),
            template,
        })
    }

    /// The qualified name of the definition `name` declared at `origin`, as
    /// this module's bindings report it.
    fn qualified(&self, origin: Origin<'_>, name: &str) -> DefinitionName {
        let module = match origin.module() {
            None => self.name.clone(),
            Some(path) => Some(module_name(path)),
        };
        DefinitionName::new(module, definition_segment(name))
    }

    /// The declaration of the trait `name` this module declares.
    fn own_trait_decl(&self, name: &str) -> Option<&'m TraitDecl> {
        self.parsed.declarations.iter().find_map(|decl| match decl {
            Declaration::Trait(declared) if declared.name == name => Some(declared),
            _ => None,
        })
    }
}

/// The definition a sub's written target binds to, and its existing-compile
/// template.
pub(crate) struct SubTarget<'m> {
    pub(crate) name: DefinitionName,
    pub(crate) template: &'m TopologyTemplate,
}

/// Every module a program's names can bind into: its user modules and the
/// standard library.
pub(crate) struct ModuleSet<'p, 'm> {
    modules: Vec<&'p ModuleContext<'m>>,
}

/// A trait a written name binds to: its qualified name, its declaration and
/// compile, and the module that declares it, against which its own
/// refinements resolve.
pub(crate) struct ResolvedTrait<'p, 'm> {
    pub(crate) name: DefinitionName,
    pub(crate) decl: &'m TraitDecl,
    pub(crate) compiled: &'m CompiledTrait,
    pub(crate) module: &'p ModuleContext<'m>,
}

impl<'p, 'm> ModuleSet<'p, 'm> {
    pub(crate) fn new(modules: Vec<&'p ModuleContext<'m>>) -> Self {
        Self { modules }
    }

    /// The trait `written` names in `module`, bound as the existing compile
    /// binds it, with the declaration it was compiled from; `None` when the
    /// name binds to no trait.
    pub(crate) fn trait_named(
        &self,
        module: &'p ModuleContext<'m>,
        written: &str,
    ) -> Option<ResolvedTrait<'p, 'm>> {
        let (compiled, origin) = *module.traits.get(written)?;
        let declaring = match origin.module() {
            None => module,
            Some(path) => self
                .modules
                .iter()
                .copied()
                .find(|m| m.path() == path)
                .unwrap_or_else(|| {
                    panic!("the trait `{written}` binds into `{path}`, a module of the program")
                }),
        };
        let decl = declaring.own_trait_decl(written).unwrap_or_else(|| {
            panic!(
                "the compiled trait `{written}` of `{}` has a declaration there",
                declaring.path()
            )
        });
        Some(ResolvedTrait {
            name: module.qualified(origin, written),
            decl,
            compiled,
            module: declaring,
        })
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
