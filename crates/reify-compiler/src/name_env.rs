//! The one name-environment policy point (NS-P1/P3), shared by the compiler's
//! registries and the evaluator's resolution environment: per kind, a bare
//! name binds the FIRST definition in tier order — local, then user imports in
//! slice order, then std modules in slice order. Contract:
//! `docs/prds/v0_6/stdlib-namespace.md` §3.1.
//!
//! [`bind`]/[`resolve`] serve one module's compile: its own definitions over
//! the ambient prelude. [`bind_scopes`]/[`resolve_in_scopes`] add a middle
//! scope: local definitions, then each direct import's `pub` surface in order,
//! then the ambient prelude (resolution-unification I6, D-5).

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ModulePath, SourceSpan, Type};
use reify_ir::{CompiledFunction, EnumDef};

use crate::types::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, EntityKind, TopologyTemplate, Visibility,
};

/// Precedence tiers, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Local,
    Import,
    Stdlib,
}

/// Where a candidate definition comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin<'a> {
    Local,
    Module(&'a ModulePath),
}

impl<'a> Origin<'a> {
    pub fn tier(&self) -> Tier {
        match self {
            Origin::Local => Tier::Local,
            Origin::Module(path) => module_tier(path),
        }
    }

    /// The declaring prelude module; `None` for the local tier.
    pub fn module(&self) -> Option<&'a ModulePath> {
        match self {
            Origin::Local => None,
            Origin::Module(path) => Some(path),
        }
    }
}

const STDLIB_ROOT_SEGMENT: &str = "std";

fn module_tier(path: &ModulePath) -> Tier {
    if path.0.first().is_some_and(|s| s == STDLIB_ROOT_SEGMENT) {
        Tier::Stdlib
    } else {
        Tier::Import
    }
}

/// A kind of named definition. Each kind is its own namespace.
pub trait Definition: Debug + 'static {
    /// What two definitions must share to collide.
    type Key<'k>: Eq + Hash + Clone + Debug;

    fn key(&self) -> Self::Key<'_>;

    fn name(&self) -> &str;

    /// How diagnostics name this kind, e.g. "enum" or "type alias".
    fn kind_word(&self) -> &'static str;

    /// Whether the declaration is `pub`: a direct import exposes only these.
    fn is_pub(&self) -> bool;

    /// The definitions `module` makes visible to modules that have it in
    /// their prelude.
    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self>;
}

/// The collision key of a function: its name and parameter TYPES. Parameter
/// names and the return type do not participate.
#[derive(Debug, Clone, Copy)]
pub struct FunctionSignature<'a> {
    name: &'a str,
    params: &'a [(String, Type)],
}

impl<'a> FunctionSignature<'a> {
    pub fn new(name: &'a str, params: &'a [(String, Type)]) -> Self {
        Self { name, params }
    }

    fn param_types(&self) -> impl Iterator<Item = &'a Type> {
        self.params.iter().map(|(_, ty)| ty)
    }
}

impl PartialEq for FunctionSignature<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.params.len() == other.params.len()
            && self.param_types().eq(other.param_types())
    }
}

impl Eq for FunctionSignature<'_> {}

impl Hash for FunctionSignature<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.params.len().hash(state);
        self.param_types().for_each(|ty| ty.hash(state));
    }
}

/// Templates share ONE entity namespace across structures and occurrences.
impl Definition for TopologyTemplate {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        self.entity_kind.as_label()
    }

    fn is_pub(&self) -> bool {
        self.visibility == Visibility::Public
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.templates.iter()
    }
}

impl Definition for EnumDef {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "enum"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.enum_defs.iter()
    }
}

impl Definition for CompiledTrait {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "trait"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.trait_defs.iter()
    }
}

impl Definition for CompiledFunction {
    type Key<'k> = FunctionSignature<'k>;

    fn key(&self) -> FunctionSignature<'_> {
        FunctionSignature::new(&self.name, &self.params)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "function"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.functions.iter()
    }
}

impl Definition for CompiledTypeAlias {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "type alias"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.type_aliases.iter().filter(|a| a.is_pub)
    }
}

impl Definition for CompiledConstraintDef {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "constraint def"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.constraint_defs.iter().filter(|c| c.is_pub)
    }
}

impl Definition for CompiledPurpose {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "purpose"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        own_purposes(module).filter(|p| p.is_pub)
    }
}

/// `module`'s purposes minus the copies `crate::merge_prelude_purposes` merged
/// in from its prelude, which it marks with a prelude-sentinel declaration span.
/// A module does not re-export what it merged; the originals stay reachable
/// from their home modules.
pub(crate) fn own_purposes(module: &CompiledModule) -> impl Iterator<Item = &CompiledPurpose> {
    module
        .compiled_purposes
        .iter()
        .filter(|p| !p.declaration_span.is_prelude())
}

impl Definition for CompiledUnit {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind_word(&self) -> &'static str {
        "unit"
    }

    fn is_pub(&self) -> bool {
        self.is_pub
    }

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.units.iter().filter(|u| u.is_pub)
    }
}

/// One definition together with where it comes from.
#[derive(Debug)]
pub struct Candidate<'a, D> {
    pub origin: Origin<'a>,
    pub definition: &'a D,
}

impl<D> Clone for Candidate<'_, D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D> Copy for Candidate<'_, D> {}

/// A definition that lost its key to a higher-precedence one.
#[derive(Debug)]
pub struct Shadowing<'a, D: Definition> {
    pub key: D::Key<'a>,
    pub winner: Candidate<'a, D>,
    pub shadowed: Candidate<'a, D>,
}

/// The one wording of a first-wins collision: the kind, the name, the
/// winner's origin first, then the shadowed one. Each origin is a dotted
/// module path; [`Origin::Local`] is `local_module`, the module whose bind
/// produced `shadowing`, which is `None` for a bind with no local scope.
pub fn collision_warning<D: Definition>(
    shadowing: &Shadowing<'_, D>,
    local_module: Option<&ModulePath>,
) -> Diagnostic {
    let label = |origin: Origin<'_>| {
        origin
            .module()
            .or(local_module)
            .expect("a bind with no local scope has no local candidate")
            .to_dotted()
    };
    let definition = shadowing.winner.definition;
    let winner = label(shadowing.winner.origin);
    Diagnostic::warning(format!(
        "{kind} '{name}' declared in both '{winner}' and '{shadowed}'; \
         first-wins: '{winner}' binds",
        kind = definition.kind_word(),
        name = definition.name(),
        shadowed = label(shadowing.shadowed.origin),
    ))
    .with_code(DiagnosticCode::DefinitionCollision)
    .with_label(DiagnosticLabel::new(
        SourceSpan::prelude(),
        "definition collision",
    ))
}

/// What a module offers the candidate stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// A prelude module: everything it exports.
    Ambient,
    /// A direct import: only its `pub` exports (resolution-unification D-5).
    DirectImport,
}

/// The definitions `module` offers through `surface`.
pub fn surface_of<D: Definition>(
    module: &CompiledModule,
    surface: Surface,
) -> impl Iterator<Item = &D> {
    D::exported_by(module)
        .filter(move |definition| surface == Surface::Ambient || definition.is_pub())
}

/// Every candidate of one kind, in precedence order: `local` definitions, then
/// the surface of each import-tier module in order, then the surface of each
/// std module in order. The tier of a module is decided by its path;
/// `modules` streams every non-local module with its surface, once per tier.
fn in_precedence_order<'a, D, L, M, I>(
    local: L,
    modules: M,
) -> impl Iterator<Item = Candidate<'a, D>>
where
    D: Definition,
    L: IntoIterator<Item = &'a D>,
    M: Fn() -> I,
    I: Iterator<Item = (&'a CompiledModule, Surface)>,
{
    let surfaces_of_tier = |tier: Tier| {
        modules()
            .filter(move |(module, _)| module_tier(&module.path) == tier)
            .flat_map(|(module, surface)| {
                surface_of(module, surface).map(move |definition| Candidate {
                    origin: Origin::Module(&module.path),
                    definition,
                })
            })
    };
    local
        .into_iter()
        .map(|definition| Candidate {
            origin: Origin::Local,
            definition,
        })
        .chain(surfaces_of_tier(Tier::Import))
        .chain(surfaces_of_tier(Tier::Stdlib))
}

/// Candidates across three scopes: `local`, each direct import's `pub`
/// surface in order, then each ambient `prelude` module's exports.
fn scope_candidates<'a, D, L, I, P>(
    local: L,
    direct_imports: I,
    prelude: P,
) -> impl Iterator<Item = Candidate<'a, D>>
where
    D: Definition,
    L: IntoIterator<Item = &'a D>,
    I: IntoIterator<Item = &'a CompiledModule> + Clone,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    in_precedence_order(local, move || {
        let direct = direct_imports
            .clone()
            .into_iter()
            .map(|module| (module, Surface::DirectImport));
        let ambient = prelude
            .clone()
            .into_iter()
            .map(|module| (module, Surface::Ambient));
        direct.chain(ambient)
    })
}

/// Every key's first candidate, plus every later candidate it shadows.
fn bind_candidates<'a, D: Definition>(
    candidates: impl Iterator<Item = Candidate<'a, D>>,
) -> Bindings<'a, D> {
    let mut winners: Vec<Candidate<'a, D>> = Vec::new();
    let mut index: HashMap<D::Key<'a>, usize> = HashMap::new();
    let mut shadowings: Vec<Shadowing<'a, D>> = Vec::new();
    for candidate in candidates {
        match index.entry(candidate.definition.key()) {
            Entry::Vacant(slot) => {
                slot.insert(winners.len());
                winners.push(candidate);
            }
            Entry::Occupied(slot) => shadowings.push(Shadowing {
                key: slot.key().clone(),
                winner: winners[*slot.get()],
                shadowed: candidate,
            }),
        }
    }
    Bindings {
        winners,
        index,
        shadowings,
    }
}

/// The definition `key` binds to, if any. Lazy and allocation-free.
pub fn resolve<'a, D, P, Q>(local: &'a [D], prelude: P, key: &Q) -> Option<&'a D>
where
    D: Definition,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
    D::Key<'a>: Borrow<Q>,
    Q: Eq + ?Sized,
{
    resolve_in_scopes(local, std::iter::empty(), prelude, key)
}

/// Every key's binding plus every shadowing, by the same rule as [`resolve`].
pub fn bind<'a, D, P>(local: &'a [D], prelude: P) -> Bindings<'a, D>
where
    D: Definition,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    bind_scopes(local, std::iter::empty(), prelude)
}

/// The definition `key` binds to over `local`, the `pub` surface of each of
/// `direct_imports`, then the ambient `prelude`. Lazy and allocation-free.
pub fn resolve_in_scopes<'a, D, L, I, P, Q>(
    local: L,
    direct_imports: I,
    prelude: P,
    key: &Q,
) -> Option<&'a D>
where
    D: Definition,
    L: IntoIterator<Item = &'a D>,
    I: IntoIterator<Item = &'a CompiledModule> + Clone,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
    D::Key<'a>: Borrow<Q>,
    Q: Eq + ?Sized,
{
    resolve_in_scopes_by(local, direct_imports, prelude, |definition| {
        D::key(definition).borrow() == key
    })
}

/// The first definition `matches` accepts, in [`resolve_in_scopes`] order.
/// When `matches` accepts exactly the definitions of one key, that is the
/// key's binding, so this resolves a key the caller cannot build. Lazy and
/// allocation-free.
pub fn resolve_in_scopes_by<'a, D, L, I, P>(
    local: L,
    direct_imports: I,
    prelude: P,
    mut matches: impl FnMut(&'a D) -> bool,
) -> Option<&'a D>
where
    D: Definition,
    L: IntoIterator<Item = &'a D>,
    I: IntoIterator<Item = &'a CompiledModule> + Clone,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    scope_candidates(local, direct_imports, prelude)
        .map(|candidate| candidate.definition)
        .find(|definition| matches(definition))
}

/// Every key's binding plus every shadowing, by the same rule as
/// [`resolve_in_scopes`].
pub fn bind_scopes<'a, D, L, I, P>(local: L, direct_imports: I, prelude: P) -> Bindings<'a, D>
where
    D: Definition,
    L: IntoIterator<Item = &'a D>,
    I: IntoIterator<Item = &'a CompiledModule> + Clone,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    bind_scopes_where(local, direct_imports, prelude, |_| true)
}

/// [`bind_scopes`] over only the definitions `keep` accepts. When `keep`
/// accepts or rejects each key's definitions together, every kept key binds
/// and shadows exactly as under [`bind_scopes`], and only the kept
/// candidates are indexed.
pub fn bind_scopes_where<'a, D, L, I, P>(
    local: L,
    direct_imports: I,
    prelude: P,
    mut keep: impl FnMut(&'a D) -> bool,
) -> Bindings<'a, D>
where
    D: Definition,
    L: IntoIterator<Item = &'a D>,
    I: IntoIterator<Item = &'a CompiledModule> + Clone,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    bind_candidates(
        scope_candidates(local, direct_imports, prelude)
            .filter(|candidate| keep(candidate.definition)),
    )
}

/// The outcome of [`bind`] for one kind.
pub struct Bindings<'a, D: Definition> {
    winners: Vec<Candidate<'a, D>>,
    index: HashMap<D::Key<'a>, usize>,
    shadowings: Vec<Shadowing<'a, D>>,
}

impl<'a, D: Definition> Bindings<'a, D> {
    pub fn get<Q>(&self, key: &Q) -> Option<&'a D>
    where
        D::Key<'a>: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.index.get(key).map(|&i| self.winners[i].definition)
    }

    /// The bound definitions, in precedence order.
    pub fn winners(&self) -> impl Iterator<Item = &'a D> {
        self.winners.iter().map(|candidate| candidate.definition)
    }

    /// The bound definitions with their origins, in precedence order.
    pub fn winning_candidates(&self) -> &[Candidate<'a, D>] {
        &self.winners
    }

    pub fn shadowings(&self) -> &[Shadowing<'a, D>] {
        &self.shadowings
    }

    pub fn into_map(self) -> HashMap<D::Key<'a>, &'a D> {
        let winners = self.winners;
        self.index
            .into_iter()
            .map(|(key, i)| (key, winners[i].definition))
            .collect()
    }
}

/// The constructor view of the entity namespace: local templates of every
/// kind, plus the prelude STRUCTURES that win their name. Templates are keyed
/// by name alone, so this is the unified binding's ctor view.
pub(crate) fn ctor_template_registry<'a>(
    local: &'a [TopologyTemplate],
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a TopologyTemplate> {
    overlay_local_templates(&prelude_ctor_view(prelude), local)
}

/// The prelude STRUCTURES that win their name in the entity namespace, so a
/// winning prelude occurrence hides a lower-precedence structure of its name.
pub(crate) fn prelude_ctor_view<'a>(
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a TopologyTemplate> {
    bind::<TopologyTemplate, _>(&[], prelude.iter().copied())
        .winners()
        .filter(|template| template.entity_kind == EntityKind::Structure)
        .map(|template| (template.name.clone(), template))
        .collect()
}

/// `prelude_view` with every `local` template, of any kind, in place of the
/// prelude entry of its name; the first local of a name wins.
pub(crate) fn overlay_local_templates<'a>(
    prelude_view: &HashMap<String, &'a TopologyTemplate>,
    local: &'a [TopologyTemplate],
) -> HashMap<String, &'a TopologyTemplate> {
    let mut view = prelude_view.clone();
    for template in local.iter().rev() {
        view.insert(template.name.clone(), template);
    }
    view
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn compile_at(path: ModulePath, src: &str) -> CompiledModule {
        let parsed = reify_syntax::parse(src, path);
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse: {:?}",
            parsed.errors
        );
        crate::compile(&parsed)
    }

    /// `hi` (import tier) outranks `lo` (std tier); both declare `X`, `hi` as
    /// an occurrence and `lo` as a structure. `local` declares `L` twice and an
    /// OCCURRENCE `Q` over `hi`'s structure `Q`.
    struct Fixture {
        hi: CompiledModule,
        lo: CompiledModule,
        local: Vec<TopologyTemplate>,
    }

    impl Fixture {
        fn new() -> Self {
            let hi = compile_at(
                ModulePath::single("hi"),
                "occurrence X { let d = 5.0 }\nstructure def Q { param q : Real = 1.0 }\n",
            );
            let lo = compile_at(
                ModulePath(vec!["std".into(), "lo".into()]),
                "structure def X { param a : Real = 1.0 }\n\
                 structure def P { param a : Real = 1.0 }\n\
                 structure def S { param a : Real = 1.0 }\n",
            );
            let mut local = compile_at(
                ModulePath::single("local_one"),
                "structure def S { param b : Real = 2.0 }\n\
                 occurrence Q { let d = 1.0 }\n\
                 structure def L { param c : Real = 3.0 }\n",
            )
            .templates;
            local.extend(
                compile_at(
                    ModulePath::single("local_two"),
                    "structure def L { param c : Real = 4.0 }\n",
                )
                .templates,
            );
            Fixture { hi, lo, local }
        }

        /// Both orders of the two-module prelude: `lo`-first catches a view
        /// that keeps the first slice entry for `X`, `hi`-first catches one
        /// that keeps the last.
        fn slice_orders(&self) -> [[&CompiledModule; 2]; 2] {
            [[&self.lo, &self.hi], [&self.hi, &self.lo]]
        }

        /// The bindings of the prelude-only ctor view.
        fn expected_prelude_view(&self) -> Vec<(&str, &TopologyTemplate)> {
            vec![
                ("Q", first_named(&self.hi.templates, "Q")),
                ("P", first_named(&self.lo.templates, "P")),
                ("S", first_named(&self.lo.templates, "S")),
            ]
        }

        /// The bindings of the prelude view overlaid by `local`.
        fn expected_overlaid_view(&self) -> Vec<(&str, &TopologyTemplate)> {
            vec![
                ("Q", first_named(&self.local, "Q")),
                ("P", first_named(&self.lo.templates, "P")),
                ("S", first_named(&self.local, "S")),
                ("L", first_named(&self.local, "L")),
            ]
        }
    }

    fn first_named<'a>(templates: &'a [TopologyTemplate], name: &str) -> &'a TopologyTemplate {
        templates
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("fixture declares {name}"))
    }

    /// `prelude` is the slice order `view` was built under; failures name it.
    fn assert_binds(
        view: &HashMap<String, &TopologyTemplate>,
        expected: &[(&str, &TopologyTemplate)],
        prelude: &[&CompiledModule],
    ) {
        let order = prelude
            .iter()
            .map(|module| module.path.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let keys: BTreeSet<&str> = view.keys().map(String::as_str).collect();
        let expected_keys: BTreeSet<&str> = expected.iter().map(|(name, _)| *name).collect();
        assert_eq!(keys, expected_keys, "under prelude slice order {order}");
        for (name, template) in expected {
            assert!(
                std::ptr::eq(view[*name], *template),
                "{name} binds the wrong {} template under prelude slice order {order}",
                view[*name].entity_kind
            );
        }
    }

    #[test]
    fn prelude_ctor_view_keeps_winning_prelude_structures_only() {
        let fixture = Fixture::new();
        for prelude in fixture.slice_orders() {
            assert_binds(
                &prelude_ctor_view(&prelude),
                &fixture.expected_prelude_view(),
                &prelude,
            );
        }
    }

    #[test]
    fn overlaying_local_templates_shadows_the_prelude_by_name() {
        let fixture = Fixture::new();
        for prelude in fixture.slice_orders() {
            let prelude_view = prelude_ctor_view(&prelude);
            assert_binds(
                &overlay_local_templates(&prelude_view, &fixture.local),
                &fixture.expected_overlaid_view(),
                &prelude,
            );
        }
    }

    #[test]
    fn ctor_template_registry_is_the_prelude_view_overlaid_by_the_locals() {
        let fixture = Fixture::new();
        for prelude in fixture.slice_orders() {
            assert_binds(
                &ctor_template_registry(&fixture.local, &prelude),
                &fixture.expected_overlaid_view(),
                &prelude,
            );
        }
    }
}
