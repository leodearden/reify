//! NS-V2 fix-its (`docs/prds/v0_6/stdlib-namespace.md` §3.2): under strict
//! visibility a module sees only the std modules it imports, so a std name it
//! writes without importing a defining module draws one coded Error naming
//! the imports that would bring the name in.
//!
//! [`name_uses`](super::name_uses) finds the written names and
//! [`attribution`](super::attribution) the std modules that can bind each.
//! Visibility is the compile's own: a module is visible iff it is in the
//! import tier the module was compiled against (whole-module, entity imports
//! included, `pub import` re-exports folded in), so the fix-it and the
//! compile cannot disagree about what the module imports.
//! [`coverage`](super::coverage) only supplies the gated-off note.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use reify_ast::ParsedModule;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ModulePath, SourceSpan};

use super::attribution::{ExportIndex, defining_std_modules};
use super::coverage::{ImportCoverage, gated_off, in_effect};
use super::name_uses::collect_name_uses;
use crate::cfg::CfgSet;
use crate::re_exports::ReExports;
use crate::types::CompiledModule;

const PRELUDE: &str = "std.prelude";

/// One NS-V2 Error per name `parsed` writes that only std modules define
/// when none of those modules is in `imports`, the tier `local` was compiled
/// against.
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
    let uses = collect_name_uses(parsed);
    let index = ExportIndex::new(local, &universe, &uses);
    let gated_off = gated_off_coverage(parsed, cfg, &universe);
    let in_tier = |module: &ModulePath| imports.iter().any(|compiled| &compiled.path == module);

    let mut unimported: BTreeMap<&str, Unimported> = BTreeMap::new();
    let distinct: BTreeSet<_> = uses.uses.iter().collect();
    for name_use in distinct {
        let name = name_use.name.as_str();
        let modules = defining_std_modules(name_use, &uses, &index);
        if modules.is_empty() || modules.iter().any(|module| in_tier(module)) {
            continue;
        }
        let entry = unimported.entry(name).or_default();
        entry
            .modules
            .extend(modules.iter().map(|module| module.to_dotted()));
        entry.through_prelude |= modules
            .iter()
            .any(|module| prelude_closure().contains(module));
        entry.gated_off |= gated_off
            .as_ref()
            .is_some_and(|coverage| modules.iter().any(|module| coverage.covers(module, name)));
        if let Some(&span) = uses.first_spans.get(name_use) {
            entry.first_span = Some(match entry.first_span {
                Some(first) if first.start <= span.start => first,
                _ => span,
            });
        }
    }

    let mut fixits: Vec<(Option<SourceSpan>, Diagnostic)> = unimported
        .into_iter()
        .map(|(name, found)| (found.first_span, found.into_fixit(name)))
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
        crate::name_env::module_tier(&module) != crate::name_env::Tier::Stdlib
            && !imports.iter().any(|compiled| compiled.path == module)
    })
}

/// What the imports `parsed` declares that `cfg` leaves out would make
/// visible, along the re-export edges of `universe`; `None` when `cfg` leaves
/// none out.
fn gated_off_coverage<'p>(
    parsed: &'p ParsedModule,
    cfg: &CfgSet,
    universe: &[&CompiledModule],
) -> Option<ImportCoverage<'p>> {
    gated_off(parsed, cfg).next()?;
    let re_exports = ReExports::of_compiled(universe.iter().copied());
    Some(ImportCoverage::of_gated_off(parsed, cfg, &re_exports))
}

/// `std.prelude` and every module it re-exports: a property of the stdlib
/// build, since a std module re-exports only std modules.
fn prelude_closure() -> &'static [ModulePath] {
    static CLOSURE: OnceLock<Vec<ModulePath>> = OnceLock::new();
    CLOSURE.get_or_init(|| {
        let prelude = ModulePath::from_dotted(PRELUDE).expect("`std.prelude` is a module path");
        ReExports::of_stdlib().closure(&prelude)
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
    /// Whether one of those modules is `std.prelude` or one it re-exports.
    through_prelude: bool,
    /// Whether one of the module's `#cfg`-gated imports the active cfg leaves
    /// out would make it visible.
    gated_off: bool,
    first_span: Option<SourceSpan>,
}

impl Unimported {
    fn into_fixit(self, name: &str) -> Diagnostic {
        let offers_prelude = self.through_prelude && !self.modules.contains(PRELUDE);
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
