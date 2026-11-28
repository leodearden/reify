//! NS-V2 fix-its (`docs/prds/v0_6/stdlib-namespace.md` §3.2): under strict
//! visibility a module sees only the std modules it imports, so a std name it
//! writes without importing a defining module draws one coded Error naming
//! the imports that would bring the name in.
//!
//! The same derivation `fix --imports` runs: [`name_uses`](super::name_uses)
//! finds the written names, [`attribution`](super::attribution) the std
//! modules that can bind each, and [`coverage`](super::coverage) what the
//! module's in-effect imports make visible, `pub import` re-exports included.

use std::collections::{BTreeMap, BTreeSet};

use reify_ast::ParsedModule;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ModulePath, SourceSpan};

use super::attribution::{ExportIndex, defining_std_modules};
use super::coverage::{ImportCoverage, in_effect};
use super::name_uses::collect_name_uses;
use crate::cfg::CfgSet;
use crate::re_exports::ReExports;
use crate::types::CompiledModule;

const PRELUDE: &str = "std.prelude";

/// One NS-V2 Error per name `parsed` writes that only std modules define
/// when `cfg`'s in-effect imports make none of those modules visible.
///
/// `local` is `parsed` compiled against its own import tier, `imports`;
/// attribution ranks its definitions and the tier's ahead of the whole
/// stdlib, so a name the module or a user import defines never draws one. A
/// name one of whose defining modules is visible draws none either: what
/// stays unresolved there is an overload or type question, not a missing
/// import.
///
/// A module with an in-effect user import absent from `imports` (missing,
/// failed, cyclic, or unfollowed because the entry is detached) draws none:
/// that import may define any name it writes, so no std attribution is sound.
pub fn std_import_fixits(
    parsed: &ParsedModule,
    local: &CompiledModule,
    imports: &[&CompiledModule],
    cfg: &CfgSet,
) -> Vec<Diagnostic> {
    if has_absent_user_import(parsed, imports, cfg) {
        return Vec::new();
    }
    let universe = tier_then_stdlib(local, imports);
    let index = ExportIndex::new(local, &universe);
    let uses = collect_name_uses(parsed);
    let re_exports = ReExports::of_compiled(universe.iter().copied());
    let in_effect = ImportCoverage::of(parsed, cfg, &re_exports);
    let gated_off = ImportCoverage::of_gated_off(parsed, cfg, &re_exports);

    let mut unimported: BTreeMap<&str, Unimported> = BTreeMap::new();
    let distinct: BTreeSet<_> = uses.uses.iter().collect();
    for name_use in distinct {
        let name = name_use.name.as_str();
        let modules = defining_std_modules(name_use, &uses, &index);
        if modules.is_empty() || modules.iter().any(|module| in_effect.covers(module, name)) {
            continue;
        }
        let entry = unimported.entry(name).or_default();
        entry
            .modules
            .extend(modules.iter().map(|module| module.to_dotted()));
        entry.gated_off |= modules.iter().any(|module| gated_off.covers(module, name));
        if let Some(&span) = uses.first_spans.get(name_use) {
            entry.first_span = Some(match entry.first_span {
                Some(first) if first.start <= span.start => first,
                _ => span,
            });
        }
    }

    let prelude_closure = re_exports.closure(PRELUDE);
    let mut fixits: Vec<(Option<SourceSpan>, Diagnostic)> = unimported
        .into_iter()
        .map(|(name, found)| {
            let offers_prelude = !found.modules.contains(PRELUDE)
                && found
                    .modules
                    .iter()
                    .any(|module| prelude_closure.contains(module));
            (found.first_span, found.into_fixit(name, offers_prelude))
        })
        .collect();
    fixits.sort_by_key(|(span, _)| span.map(|span| span.start));
    fixits.into_iter().map(|(_, fixit)| fixit).collect()
}

/// Whether `parsed` declares a non-std import in effect under `cfg` whose
/// module is not one of `imports`.
fn has_absent_user_import(
    parsed: &ParsedModule,
    imports: &[&CompiledModule],
    cfg: &CfgSet,
) -> bool {
    in_effect(parsed, cfg).any(|(module, _)| {
        !crate::module_dag::is_std_import_path(&module.to_dotted())
            && !imports.iter().any(|compiled| compiled.path == module)
    })
}

/// `imports`, then every stdlib module that is neither one of them nor
/// `local` itself.
fn tier_then_stdlib<'a>(
    local: &CompiledModule,
    imports: &[&'a CompiledModule],
) -> Vec<&'a CompiledModule> {
    let in_tier = |path: &ModulePath| imports.iter().any(|module| &module.path == path);
    let mut universe = imports.to_vec();
    universe.extend(
        crate::stdlib_loader::load_stdlib()
            .iter()
            .filter(|module| module.path != local.path && !in_tier(&module.path)),
    );
    universe
}

/// What one unimported name needs.
#[derive(Default)]
struct Unimported {
    /// The dotted paths of the std modules that define it.
    modules: BTreeSet<String>,
    /// Whether one of the module's `#cfg`-gated imports the active cfg leaves
    /// out would make it visible.
    gated_off: bool,
    first_span: Option<SourceSpan>,
}

impl Unimported {
    fn into_fixit(self, name: &str, offers_prelude: bool) -> Diagnostic {
        let modules: Vec<String> = self.modules.into_iter().collect();
        let mut candidates = modules.clone();
        if offers_prelude {
            candidates.push(PRELUDE.to_string());
        }
        let defined_in = match modules.as_slice() {
            [module] => format!("{module}, which this module does not import"),
            _ => format!("{}, none of which this module imports", and_list(&modules)),
        };
        let add: Vec<String> = candidates
            .iter()
            .map(|candidate| format!("`import {candidate}`"))
            .collect();
        let gated_note = if self.gated_off {
            "; a #cfg-gated import that would make it visible is not in effect under the \
             active cfg"
        } else {
            ""
        };
        let mut fixit = Diagnostic::error(format!(
            "'{name}' is defined in {defined_in}; add {}{gated_note}",
            or_list(&add)
        ))
        .with_code(DiagnosticCode::StdNameNotImported)
        .with_candidates(candidates);
        if let Some(span) = self.first_span {
            fixit = fixit.with_label(DiagnosticLabel::new(
                span,
                format!("'{name}' needs a std import"),
            ));
        }
        fixit
    }
}

fn and_list(items: &[String]) -> String {
    joined(items, "and")
}

fn or_list(items: &[String]) -> String {
    joined(items, "or")
}

fn joined(items: &[String], last: &str) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., tail] => format!("{} {last} {tail}", init.join(", ")),
    }
}
