//! D-2 (stdlib-namespace §3.1 NS-P2): a module's own definition that shadows
//! a same-kind std definition draws ONE [`DiagnosticCode::StdlibNameShadowed`],
//! at the local declaration.
//!
//! It is an Error when the shadowed std module is in scope because the module
//! chose it ([`StdlibScope::Declared`]) and the module is a user module (D-2
//! stage 2, κ #5503). It stays a Warning under the ambient whole-stdlib
//! compile ([`StdlibScope::Ambient`]), where no std name was chosen, and for a
//! std module, whose std-vs-std collisions the stdlib build checks (NS-P2).
//!
//! [`name_env`] decides who wins; this pass only reports local wins over the
//! stdlib tier. Definitions with no source declaration (monomorphs, registered
//! associated functions) are synthesized, not written, and are not reported.
//! Units keep their own duplicate-declaration Error; only their cross-prelude
//! seeding binds by `name_env` (introduced by #5522).

use std::collections::HashSet;

use reify_ast::{Declaration, FnDef, ParsedModule};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ModulePath, Severity, SourceSpan};
use reify_ir::{CompiledFunction, EnumDef};

use crate::compile_builder::ctx::CompilationCtx;
use crate::name_env::{self, Definition, Origin, Tier};
use crate::prelude_context::StdlibScope;
use crate::type_resolution::TypeAliasEntry;
use crate::types::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    TopologyTemplate,
};

/// Every stdlib shadowing in the module, ordered by declaration position.
/// `local_purposes` are the module's own purposes, before the prelude's are
/// merged in; `stdlib_scope` is how the prelude's std modules came into scope.
pub(crate) fn lint(
    parsed: &ParsedModule,
    ctx: &CompilationCtx,
    local_purposes: &[CompiledPurpose],
    prelude: &[&CompiledModule],
    stdlib_scope: StdlibScope,
) -> Vec<Diagnostic> {
    let severity = severity(&parsed.path, stdlib_scope);
    let local_aliases: Vec<CompiledTypeAlias> = ctx
        .alias_registry
        .iter()
        .cloned()
        .map(TypeAliasEntry::into_compiled)
        .collect();
    let mut shadowings = [
        report(parsed, &ctx.templates, prelude, severity),
        report(parsed, &ctx.enum_defs, prelude, severity),
        report(parsed, &ctx.trait_defs, prelude, severity),
        report(parsed, &ctx.functions, prelude, severity),
        report(parsed, &local_aliases, prelude, severity),
        report(parsed, &ctx.constraint_defs, prelude, severity),
        report(parsed, local_purposes, prelude, severity),
    ]
    .concat();
    shadowings.sort_by_key(|shadowing| shadowing.labels.first().map(|label| label.span.start));
    shadowings
}

/// Error iff the module chose its std modules and is not itself one.
fn severity(module: &ModulePath, stdlib_scope: StdlibScope) -> Severity {
    match stdlib_scope {
        StdlibScope::Declared if name_env::module_tier(module) != Tier::Stdlib => Severity::Error,
        StdlibScope::Declared | StdlibScope::Ambient => Severity::Warning,
    }
}

/// One diagnostic per key whose local winner shadows a std-tier definition.
fn report<'a, D: Reportable>(
    parsed: &ParsedModule,
    local: &'a [D],
    prelude: &[&'a CompiledModule],
    severity: Severity,
) -> Vec<Diagnostic> {
    if local.is_empty() {
        return Vec::new();
    }
    let bindings = name_env::bind(local, prelude.iter().copied());
    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();
    for shadowing in bindings.shadowings() {
        if shadowing.winner.origin != Origin::Local
            || shadowing.shadowed.origin.tier() != Tier::Stdlib
        {
            continue;
        }
        let (Some(std_module), Some(declaration)) = (
            shadowing.shadowed.origin.module(),
            shadowing.winner.definition.declaration(parsed),
        ) else {
            continue;
        };
        if reported.insert(shadowing.key.clone()) {
            diagnostics.push(diagnostic(
                &parsed.path,
                shadowing.winner.definition,
                std_module,
                declaration,
                severity,
            ));
        }
    }
    diagnostics
}

fn diagnostic<D: Reportable>(
    local_module: &ModulePath,
    definition: &D,
    std_module: &ModulePath,
    declaration: SourceSpan,
    severity: Severity,
) -> Diagnostic {
    let name = definition.name();
    let std_module = std_module.0.join(".");
    let message = format!(
        "{kind} '{name}' in module '{local}' shadows '{name}' from std module '{std_module}'; \
         the local definition wins",
        kind = definition.kind_word(),
        local = local_module.0.join("."),
    );
    let mut diagnostic = Diagnostic::warning(message)
        .with_code(DiagnosticCode::StdlibNameShadowed)
        .with_label(DiagnosticLabel::new(
            declaration,
            "shadows a stdlib definition",
        ))
        .with_label(DiagnosticLabel::new(
            SourceSpan::prelude(),
            format!("defined in std module '{std_module}'"),
        ));
    diagnostic.severity = severity;
    diagnostic
}

/// Where one kind of definition was written.
trait Reportable: Definition {
    /// The span of this definition's top-level source declaration, if any.
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan>;
}

fn first_declaration(
    parsed: &ParsedModule,
    span_of: impl FnMut(&Declaration) -> Option<SourceSpan>,
) -> Option<SourceSpan> {
    parsed.declarations.iter().find_map(span_of)
}

impl Reportable for TopologyTemplate {
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Structure(d) if d.name == self.name => Some(d.span),
            Declaration::Occurrence(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for EnumDef {
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Enum(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledTrait {
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Trait(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledFunction {
    /// Overloads share a name: prefer the declaration whose parameter names
    /// match this signature's, else the first same-named one.
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        let same_named = || {
            parsed.declarations.iter().filter_map(|decl| match decl {
                Declaration::Function(f) if f.name == self.name => Some(f),
                _ => None,
            })
        };
        let params_match = |f: &&FnDef| {
            f.params.len() == self.params.len()
                && f.params
                    .iter()
                    .zip(&self.params)
                    .all(|(declared, (compiled, _))| declared.name == *compiled)
        };
        same_named()
            .find(params_match)
            .or_else(|| same_named().next())
            .map(|f| f.span)
    }
}

impl Reportable for CompiledTypeAlias {
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::TypeAlias(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledConstraintDef {
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Constraint(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledPurpose {
    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Purpose(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}
