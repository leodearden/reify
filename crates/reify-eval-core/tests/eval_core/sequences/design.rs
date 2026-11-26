//! A toy design as the tests own it: keys, values, declarations, paths and
//! refs of their own, built by the constructor functions below, which are
//! the vocabulary the generator and the printed tests share. The core's
//! values expose nothing to read back, so a design is interpreted, shrunk
//! and printed in this form and converted to the core's only through its
//! public constructors.

use std::any::type_name;
use std::collections::BTreeMap;

use reify_eval_core::{Binding, Declaration, Input, InputKey, Kind, KindId, Path, Ref, Root, Step};

use crate::toy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToyKind {
    Cell,
    Scope,
    Guard,
    Collection,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelPath {
    pub root: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelRef {
    pub binding: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Root(String),
    Named(String),
    Attached(ModelPath, String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bind {
    Input(String),
    Attached(String),
    Declarer(Vec<Step>),
    Passed(ModelRef),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decl {
    pub kind: ToyKind,
    pub bindings: BTreeMap<String, Bind>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Val {
    Int(i64),
    Bool(bool),
    Members(BTreeMap<Step, Decl>),
    Decl(Decl),
}

pub type Inputs = BTreeMap<Key, Val>;

impl ModelPath {
    /// The path one `step` below this one.
    pub fn child(&self, step: Step) -> ModelPath {
        let mut steps = self.steps.clone();
        steps.push(step);
        ModelPath {
            root: self.root.clone(),
            steps,
        }
    }

    /// The path this one is a child of, and the step down from it; none
    /// for a root.
    pub fn parent(&self) -> Option<(ModelPath, &Step)> {
        let (last, above) = self.steps.split_last()?;
        let parent = ModelPath {
            root: self.root.clone(),
            steps: above.to_vec(),
        };
        Some((parent, last))
    }

    pub fn to_core(&self) -> Path {
        let root = Path::root(Root::named(self.root.as_str()));
        self.steps
            .iter()
            .fold(root, |path, step| path.child(step.clone()))
    }
}

impl ModelRef {
    pub fn to_core(&self) -> Ref {
        let start = Ref::binding(self.binding.as_str());
        self.steps.iter().fold(start, |r, step| r.step(step.clone()))
    }
}

impl Key {
    pub fn to_core(&self) -> InputKey {
        match self {
            Key::Root(name) => InputKey::root(name.as_str()),
            Key::Named(name) => InputKey::named(name.as_str()),
            Key::Attached(path, name) => InputKey::attached(path.to_core(), name.as_str()),
        }
    }
}

impl Bind {
    pub fn to_core(&self) -> Binding {
        match self {
            Bind::Input(name) => Binding::Input(InputKey::named(name.as_str())),
            Bind::Attached(name) => Binding::Attached(name.as_str().into()),
            Bind::Declarer(place) => Binding::Declarer(place.clone()),
            Bind::Passed(r) => Binding::Passed(r.to_core()),
        }
    }
}

impl ToyKind {
    pub fn id(self) -> KindId {
        match self {
            ToyKind::Cell => toy::Cell::ID,
            ToyKind::Scope => toy::Scope::ID,
            ToyKind::Guard => toy::Guard::ID,
            ToyKind::Collection => toy::Collection::ID,
        }
    }
}

impl Decl {
    pub fn to_core(&self) -> Declaration {
        self.bindings
            .iter()
            .fold(Declaration::new(self.kind.id()), |declaration, (name, bind)| {
                declaration.bind(name.as_str(), bind.to_core())
            })
    }
}

/// The core's form of members: each declaration converted.
pub fn core_members(members: &BTreeMap<Step, Decl>) -> BTreeMap<Step, Declaration> {
    members
        .iter()
        .map(|(step, decl)| (step.clone(), decl.to_core()))
        .collect()
}

impl Val {
    pub fn to_core(&self) -> Input {
        match self {
            Val::Int(value) => Input::of(*value),
            Val::Bool(value) => Input::of(*value),
            Val::Members(members) => Input::of(toy::Members(core_members(members))),
            Val::Decl(decl) => Input::of(decl.to_core()),
        }
    }

    /// The name of the Rust type the core holds this value as.
    pub fn type_name(&self) -> &'static str {
        match self {
            Val::Int(_) => type_name::<i64>(),
            Val::Bool(_) => type_name::<bool>(),
            Val::Members(_) => type_name::<toy::Members>(),
            Val::Decl(_) => type_name::<Declaration>(),
        }
    }
}

/// The snapshot inputs `inputs` stand for.
pub fn to_core(inputs: &Inputs) -> Vec<(InputKey, Input)> {
    inputs
        .iter()
        .map(|(key, value)| (key.to_core(), value.to_core()))
        .collect()
}

pub fn step(name: &str) -> Step {
    Step::named(name)
}

pub fn index(index: u64) -> Step {
    Step::index(index)
}

pub fn path(root: &str, steps: impl IntoIterator<Item = Step>) -> ModelPath {
    ModelPath {
        root: root.to_owned(),
        steps: steps.into_iter().collect(),
    }
}

/// A binding to the host-named input `name`.
pub fn input(name: &str) -> Bind {
    Bind::Input(name.to_owned())
}

/// A binding to the input `name` attached to the bound node's own path.
pub fn attached(name: &str) -> Bind {
    Bind::Attached(name.to_owned())
}

/// A binding to the place `steps` down from the bound node's declarer.
pub fn declarer(steps: impl IntoIterator<Item = Step>) -> Bind {
    Bind::Declarer(steps.into_iter().collect())
}

/// A binding to the declarer's own binding `binding`, then `steps` down.
pub fn passed(binding: &str, steps: impl IntoIterator<Item = Step>) -> Bind {
    Bind::Passed(ModelRef {
        binding: binding.to_owned(),
        steps: steps.into_iter().collect(),
    })
}

/// A declaration of `kind` with `bindings`.
pub fn decl<'a>(kind: ToyKind, bindings: impl IntoIterator<Item = (&'a str, Bind)>) -> Decl {
    Decl {
        kind,
        bindings: bindings
            .into_iter()
            .map(|(name, bind)| (name.to_owned(), bind))
            .collect(),
    }
}

/// A cell adding what `terms` name, bound at "0", "1", … in order.
pub fn cell(terms: impl IntoIterator<Item = Bind>) -> Decl {
    Decl {
        kind: ToyKind::Cell,
        bindings: terms
            .into_iter()
            .enumerate()
            .map(|(at, term)| (at.to_string(), term))
            .collect(),
    }
}

/// A cell whose only term is the element at "from"/[the i64 at "pick"].
pub fn picker(pick: Bind, from: Bind) -> Decl {
    decl(ToyKind::Cell, [("pick", pick), ("from", from)])
}

/// A scope over the host-named members input `members`, with the members
/// of the input "extra" attached to its path.
pub fn scope(members: &str) -> Decl {
    decl(
        ToyKind::Scope,
        [("members", input(members)), ("extra", attached("extra"))],
    )
}

/// A guard on `condition` over the host-named members input `members`.
pub fn guard(condition: Bind, members: &str) -> Decl {
    decl(
        ToyKind::Guard,
        [("condition", condition), ("members", input(members))],
    )
}

/// A collection of `count` elements, each the declaration `element` reads.
pub fn collection(count: Bind, element: Bind) -> Decl {
    decl(ToyKind::Collection, [("count", count), ("element", element)])
}

pub fn int(value: i64) -> Val {
    Val::Int(value)
}

pub fn flag(value: bool) -> Val {
    Val::Bool(value)
}

pub fn members(members: impl IntoIterator<Item = (Step, Decl)>) -> Val {
    Val::Members(members.into_iter().collect())
}

pub fn declaration(decl: Decl) -> Val {
    Val::Decl(decl)
}

/// The key of the input declaring root `name`.
pub fn root(name: &str) -> Key {
    Key::Root(name.to_owned())
}

/// The key of the host-named input `name`.
pub fn named(name: &str) -> Key {
    Key::Named(name.to_owned())
}

/// The key of the input `name` attached to the node at `path`.
pub fn attached_to(path: ModelPath, name: &str) -> Key {
    Key::Attached(path, name.to_owned())
}

pub fn inputs(entries: impl IntoIterator<Item = (Key, Val)>) -> Inputs {
    entries.into_iter().collect()
}
