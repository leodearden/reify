//! D-2, N0 interim (stdlib-namespace §3.1 NS-P2): a module's own definition
//! that shadows a same-kind std definition draws ONE
//! [`DiagnosticCode::StdlibNameShadowed`] Warning, at the local declaration.
//!
//! [`name_env`] decides who wins; this pass only reports local wins over the
//! stdlib tier. Definitions with no source declaration (monomorphs, registered
//! associated functions) are synthesized, not written, and are not reported.
//! Units keep their own duplicate-declaration Error until resolution-unification
//! θ #5522 moves them onto `name_env`.

use std::collections::HashSet;

use reify_ast::{Declaration, FnDef, ParsedModule};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ModulePath, SourceSpan};
use reify_ir::{CompiledFunction, EnumDef};

use crate::compile_builder::ctx::CompilationCtx;
use crate::name_env::{self, Definition, Origin, Tier};
use crate::type_resolution::TypeAliasEntry;
use crate::types::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    TopologyTemplate,
};

/// Every stdlib shadowing in the module, ordered by declaration position.
/// `local_purposes` are the module's own purposes, before the prelude's are
/// merged in.
pub(crate) fn lint(
    parsed: &ParsedModule,
    ctx: &CompilationCtx,
    local_purposes: &[CompiledPurpose],
    prelude: &[&CompiledModule],
) -> Vec<Diagnostic> {
    let local_aliases: Vec<CompiledTypeAlias> = ctx
        .alias_registry
        .iter()
        .cloned()
        .map(TypeAliasEntry::into_compiled)
        .collect();
    let mut warnings = [
        report(parsed, &ctx.templates, prelude),
        report(parsed, &ctx.enum_defs, prelude),
        report(parsed, &ctx.trait_defs, prelude),
        report(parsed, &ctx.functions, prelude),
        report(parsed, &local_aliases, prelude),
        report(parsed, &ctx.constraint_defs, prelude),
        report(parsed, local_purposes, prelude),
    ]
    .concat();
    warnings.sort_by_key(|warning| warning.labels.first().map(|label| label.span.start));
    warnings
}

/// One Warning per key whose local winner shadows a std-tier definition.
fn report<'a, D: Reportable>(
    parsed: &ParsedModule,
    local: &'a [D],
    prelude: &[&'a CompiledModule],
) -> Vec<Diagnostic> {
    let bindings = name_env::bind(local, prelude.iter().copied());
    let mut reported = HashSet::new();
    let mut warnings = Vec::new();
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
            warnings.push(warning(
                &parsed.path,
                shadowing.winner.definition,
                std_module,
                declaration,
            ));
        }
    }
    warnings
}

fn warning<D: Reportable>(
    local_module: &ModulePath,
    definition: &D,
    std_module: &ModulePath,
    declaration: SourceSpan,
) -> Diagnostic {
    let name = definition.name();
    let std_module = std_module.0.join(".");
    Diagnostic::warning(format!(
        "{kind} '{name}' in module '{local}' shadows '{name}' from std module '{std_module}'; \
         the local definition wins",
        kind = definition.kind_word(),
        local = local_module.0.join("."),
    ))
    .with_code(DiagnosticCode::StdlibNameShadowed)
    .with_label(DiagnosticLabel::new(
        declaration,
        "shadows a stdlib definition",
    ))
    .with_label(DiagnosticLabel::new(
        SourceSpan::prelude(),
        format!("defined in std module '{std_module}'"),
    ))
}

/// What the lint says about one kind of definition, and where it was written.
trait Reportable: Definition {
    fn kind_word(&self) -> &'static str;

    fn name(&self) -> &str;

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
    fn kind_word(&self) -> &'static str {
        self.entity_kind.as_label()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Structure(d) if d.name == self.name => Some(d.span),
            Declaration::Occurrence(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for EnumDef {
    fn kind_word(&self) -> &'static str {
        "enum"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Enum(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledTrait {
    fn kind_word(&self) -> &'static str {
        "trait"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Trait(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledFunction {
    fn kind_word(&self) -> &'static str {
        "function"
    }

    fn name(&self) -> &str {
        &self.name
    }

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
    fn kind_word(&self) -> &'static str {
        "type alias"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::TypeAlias(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledConstraintDef {
    fn kind_word(&self) -> &'static str {
        "constraint def"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Constraint(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}

impl Reportable for CompiledPurpose {
    fn kind_word(&self) -> &'static str {
        "purpose"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn declaration(&self, parsed: &ParsedModule) -> Option<SourceSpan> {
        first_declaration(parsed, |decl| match decl {
            Declaration::Purpose(d) if d.name == self.name => Some(d.span),
            _ => None,
        })
    }
}
