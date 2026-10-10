//! The one name-environment policy point (NS-P1/P3), shared by the compiler's
//! registries and the evaluator's template lookup: per kind, a bare name binds
//! the FIRST definition in tier order — local, then user imports in slice
//! order, then std modules in slice order. Contract:
//! `docs/prds/v0_6/stdlib-namespace.md` §3.1.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use reify_core::{ModulePath, Type};
use reify_ir::{CompiledFunction, EnumDef};

use crate::types::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    EntityKind, TopologyTemplate,
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

impl Origin<'_> {
    pub fn tier(&self) -> Tier {
        match self {
            Origin::Local => Tier::Local,
            Origin::Module(path) => module_tier(path),
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

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.templates.iter()
    }
}

impl Definition for EnumDef {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
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

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.trait_defs.iter()
    }
}

impl Definition for CompiledFunction {
    type Key<'k> = FunctionSignature<'k>;

    fn key(&self) -> FunctionSignature<'_> {
        FunctionSignature::new(&self.name, &self.params)
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

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.type_aliases.iter().filter(|a| a.is_pub)
    }
}

impl Definition for CompiledConstraintDef {
    type Key<'k> = &'k str;

    fn key(&self) -> &str {
        &self.name
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

    fn exported_by(module: &CompiledModule) -> impl Iterator<Item = &Self> {
        module.compiled_purposes.iter().filter(|p| p.is_pub)
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

/// Every candidate of one kind, in precedence order: local definitions, then
/// the exports of each user-import module in slice order, then the exports of
/// each std module in slice order.
fn in_precedence_order<'a, D, P>(
    local: &'a [D],
    prelude: P,
) -> impl Iterator<Item = Candidate<'a, D>>
where
    D: Definition,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    let exports_of_tier = move |tier: Tier| {
        prelude
            .clone()
            .into_iter()
            .filter(move |module| module_tier(&module.path) == tier)
            .flat_map(|module| {
                D::exported_by(module).map(move |definition| Candidate {
                    origin: Origin::Module(&module.path),
                    definition,
                })
            })
    };
    local
        .iter()
        .map(|definition| Candidate {
            origin: Origin::Local,
            definition,
        })
        .chain(exports_of_tier(Tier::Import))
        .chain(exports_of_tier(Tier::Stdlib))
}

/// The definition `key` binds to, if any. Lazy and allocation-free.
pub fn resolve<'a, D, P, Q>(local: &'a [D], prelude: P, key: &Q) -> Option<&'a D>
where
    D: Definition,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
    D::Key<'a>: Borrow<Q>,
    Q: Eq + ?Sized,
{
    in_precedence_order(local, prelude)
        .map(|candidate| candidate.definition)
        .find(|definition| D::key(definition).borrow() == key)
}

/// Every key's binding plus every shadowing, by the same rule as [`resolve`].
pub fn bind<'a, D, P>(local: &'a [D], prelude: P) -> Bindings<'a, D>
where
    D: Definition,
    P: IntoIterator<Item = &'a CompiledModule> + Clone,
{
    let mut winners: Vec<Candidate<'a, D>> = Vec::new();
    let mut index: HashMap<D::Key<'a>, usize> = HashMap::new();
    let mut shadowings: Vec<Shadowing<'a, D>> = Vec::new();
    for candidate in in_precedence_order(local, prelude) {
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
/// kind, plus the prelude STRUCTURES that win their name. Derived from the
/// unified binding, so a prelude occurrence that wins a name never lets a
/// lower-precedence structure of that name through.
pub(crate) fn structure_registry<'a>(
    local: &'a [TopologyTemplate],
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a TopologyTemplate> {
    bind(local, prelude.iter().copied())
        .winning_candidates()
        .iter()
        .filter(|candidate| {
            candidate.origin == Origin::Local
                || candidate.definition.entity_kind == EntityKind::Structure
        })
        .map(|candidate| (candidate.definition.name.clone(), candidate.definition))
        .collect()
}
