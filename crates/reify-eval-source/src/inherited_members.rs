//! The members a definition inherits from the traits it conforms to, in the
//! order they come before its own: each listed trait in turn, a trait's
//! refinements before the trait, each trait once.

use reify_ast::{MemberDecl, TraitBoundRef};
use reify_compiler::CompiledTrait;
use reify_core::{DefinitionName, Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

use crate::module_context::{ModuleContext, ModuleSet};

/// A member declared in a trait's body, with the trait.
pub(crate) struct InheritedMember<'p, 'm> {
    pub(crate) decl: &'m MemberDecl,
    pub(crate) from: DefinitionName,
    pub(crate) declared_by: &'m CompiledTrait,
    /// The module that declares the trait, where the member's written names
    /// bind.
    pub(crate) declared_in: &'p ModuleContext<'m>,
}

/// The members `definition`, declared in `module` with `bounds`, inherits.
/// A bound naming no trait is reported and skipped.
pub(crate) fn inherited_members<'p, 'm>(
    definition: &DefinitionName,
    bounds: &[TraitBoundRef],
    module: &'p ModuleContext<'m>,
    modules: &ModuleSet<'p, 'm>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<InheritedMember<'p, 'm>> {
    let mut walk = Walk {
        definition,
        modules,
        visited: Vec::new(),
        members: Vec::new(),
        diagnostics,
    };
    for bound in bounds {
        walk.visit(module, &bound.name, bound.span);
    }
    walk.members
}

struct Walk<'w, 'p, 'm> {
    definition: &'w DefinitionName,
    modules: &'w ModuleSet<'p, 'm>,
    visited: Vec<DefinitionName>,
    members: Vec<InheritedMember<'p, 'm>>,
    diagnostics: &'w mut Vec<Diagnostic>,
}

impl<'p, 'm> Walk<'_, 'p, 'm> {
    /// Collect the members of the trait `written` names in `module`.
    fn visit(&mut self, module: &'p ModuleContext<'m>, written: &str, span: SourceSpan) {
        let Some(found) = self.modules.trait_named(module, written) else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "in `{}`: `{written}` is not a trait",
                    self.definition.name().as_str()
                ))
                .with_code(DiagnosticCode::UnresolvedTrait)
                .with_label(DiagnosticLabel::new(span, "this trait")),
            );
            return;
        };
        if self.visited.contains(&found.name) {
            return;
        }
        self.visited.push(found.name.clone());
        for refined in &found.decl.refinements {
            self.visit(found.module, &refined.name, refined.span);
        }
        self.members
            .extend(found.decl.members.iter().map(|decl| InheritedMember {
                decl,
                from: found.name.clone(),
                declared_by: found.compiled,
                declared_in: found.module,
            }));
    }
}
