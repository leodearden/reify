//! The reference interpreter: what each path of a snapshot answers, worked
//! out directly from the design model and the meaning of the toy kinds,
//! with no store, index or executor and no code shared with the core or the
//! toy kinds' bodies. It remembers each path's answer within one snapshot,
//! so a design whose nodes read one another many times stays linear, and it
//! reports a design no evaluation can answer (a cycle, or a sum past i64)
//! as invalid rather than as an outcome.

use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use reify_eval_core::{Declaration, Failure, Step};

use super::case::Case;
use super::design::{Bind, Decl, Inputs, Key, ModelPath, ModelRef, ToyKind, Val};
use crate::toy;

/// What a path answers: the kind of the node declared there, if any, and
/// its outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub node: Option<ToyKind>,
    pub outcome: Expect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    Value(Out),
    Failure(Failure),
    Absent,
}

/// A value an answer or a snapshot input holds, in the model's form.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Int(i64),
    Bool(bool),
    Members(BTreeMap<Step, Decl>),
    Elements { count: u64, element: Decl },
    Decl(Decl),
}

/// Why a path has no answer in this snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// Answering `path` needs its own answer.
    Cycle { path: ModelPath },
    /// The sum the cell at `path` adds is past i64.
    Overflow { path: ModelPath },
}

/// The first path of `case` the reference has no answer for, trying every
/// path the case demands on every snapshot it makes.
pub fn first_invalid(case: &Case) -> Option<Invalid> {
    let demanded: BTreeSet<_> = case.demanded().collect();
    case.snapshots().into_iter().find_map(|inputs| {
        let mut reference = Reference::new(inputs);
        demanded
            .iter()
            .find_map(|path| reference.answer(path).err())
    })
}

/// What a path is before any body runs: a node with its declaration, or
/// an outcome reached without one.
#[derive(Clone)]
enum Declared {
    Node(Rc<Decl>),
    Settled(Expect),
}

/// A read typed as the Rust type the core reads it as.
enum Typed<T> {
    Value(T),
    Absent,
    Failure(Failure),
}

/// The answers of one snapshot.
pub struct Reference {
    inputs: Inputs,
    answers: BTreeMap<ModelPath, Result<Rc<Expected>, Invalid>>,
    declared: BTreeMap<ModelPath, Declared>,
    in_progress: BTreeSet<ModelPath>,
    work_outs: usize,
}

impl Reference {
    pub fn new(inputs: Inputs) -> Self {
        Self {
            inputs,
            answers: BTreeMap::new(),
            declared: BTreeMap::new(),
            in_progress: BTreeSet::new(),
            work_outs: 0,
        }
    }

    pub fn answer(&mut self, path: &ModelPath) -> Result<Expected, Invalid> {
        self.shared(path).map(|answer| Expected::clone(&answer))
    }

    /// The answer at `path`, shared with the memo.
    fn shared(&mut self, path: &ModelPath) -> Result<Rc<Expected>, Invalid> {
        if let Some(known) = self.answers.get(path) {
            return known.clone();
        }
        if !self.in_progress.insert(path.clone()) {
            return Err(Invalid::Cycle { path: path.clone() });
        }
        let answer = self.work_out(path).map(Rc::new);
        self.in_progress.remove(path);
        self.answers.insert(path.clone(), answer.clone());
        answer
    }

    /// How many times a path's answer has been worked out, memo aside.
    pub fn work_outs(&self) -> usize {
        self.work_outs
    }

    /// The declaration of the node at `path`, if one is declared there.
    pub fn declaration_at(&mut self, path: &ModelPath) -> Result<Option<Decl>, Invalid> {
        Ok(match self.declaration(path)? {
            Declared::Node(decl) => Some(Decl::clone(&decl)),
            Declared::Settled(_) => None,
        })
    }

    /// What the node at `reader` reads at `r`; absent where no node is
    /// declared there.
    pub fn read_at(&mut self, reader: &ModelPath, r: &ModelRef) -> Result<Expect, Invalid> {
        match self.declaration(reader)? {
            Declared::Node(decl) => self.read(reader, &decl, r),
            Declared::Settled(_) => Ok(Expect::Absent),
        }
    }

    fn work_out(&mut self, path: &ModelPath) -> Result<Expected, Invalid> {
        self.work_outs += 1;
        let decl = match self.declaration(path)? {
            Declared::Node(decl) => decl,
            Declared::Settled(outcome) => {
                return Ok(Expected {
                    node: None,
                    outcome,
                });
            }
        };
        let outcome = match decl.kind {
            ToyKind::Cell => self.cell(path, &decl)?,
            ToyKind::Scope => self.members(path, &decl)?,
            ToyKind::Guard => self.guard(path, &decl)?,
            ToyKind::Collection => self.collection(path, &decl)?,
        };
        Ok(Expected {
            node: Some(decl.kind),
            outcome,
        })
    }

    /// A root from its snapshot input, read as a declaration; any other
    /// path from what its parent's answer declares at its step.
    fn declaration(&mut self, path: &ModelPath) -> Result<Declared, Invalid> {
        if let Some(known) = self.declared.get(path) {
            return Ok(known.clone());
        }
        let declared = match path.parent() {
            None => match typed_decl(self.input(&Key::Root(path.root.clone()))) {
                Typed::Value(decl) => Declared::Node(Rc::new(decl)),
                Typed::Absent => Declared::Settled(Expect::Absent),
                Typed::Failure(failure) => Declared::Settled(Expect::Failure(failure)),
            },
            Some((parent, step)) => {
                let above = self.shared(&parent)?;
                match (above.node, &above.outcome) {
                    (_, Expect::Failure(failure)) => {
                        Declared::Settled(Expect::Failure(failure.clone()))
                    }
                    (Some(kind), Expect::Value(out)) => declares(kind, out, step)
                        .map_or(Declared::Settled(Expect::Absent), |decl| {
                            Declared::Node(Rc::new(decl))
                        }),
                    _ => Declared::Settled(Expect::Absent),
                }
            }
        };
        self.declared.insert(path.clone(), declared.clone());
        Ok(declared)
    }

    fn input(&self, key: &Key) -> Expect {
        self.inputs
            .get(key)
            .map_or(Expect::Absent, |value| Expect::Value(out_of(value)))
    }

    /// What the node at `reader`, declared `decl`, reads at `r`: an unset
    /// binding names nothing; an input names its value, and nothing with
    /// steps below it; a binding relative to the declarer names nothing on
    /// a root; a passed binding is the declarer's own binding, the steps
    /// still to take carried along (contract 2.1).
    fn read(
        &mut self,
        reader: &ModelPath,
        decl: &Rc<Decl>,
        r: &ModelRef,
    ) -> Result<Expect, Invalid> {
        let (mut node, mut decl) = (reader.clone(), Rc::clone(decl));
        let (mut name, mut steps) = (r.binding.clone(), r.steps.clone());
        loop {
            let declarer = node.parent().map(|(declarer, _)| declarer);
            match (decl.bindings.get(&name).cloned(), declarer) {
                (Some(Bind::Input(input)), _) if steps.is_empty() => {
                    return Ok(self.input(&Key::Named(input)));
                }
                (Some(Bind::Attached(attached)), _) if steps.is_empty() => {
                    return Ok(self.input(&Key::Attached(node, attached)));
                }
                (Some(Bind::Declarer(place)), Some(declarer)) => {
                    let target = place
                        .iter()
                        .chain(&steps)
                        .fold(declarer, |at, step| at.child(step.clone()));
                    return Ok(self.shared(&target)?.outcome.clone());
                }
                (Some(Bind::Passed(passed)), Some(declarer)) => {
                    let Declared::Node(declarer_decl) = self.declaration(&declarer)? else {
                        unreachable!("a node's declarer is a node: its value declared it")
                    };
                    steps = passed.steps.iter().chain(&steps).cloned().collect();
                    name = passed.binding;
                    decl = declarer_decl;
                    node = declarer;
                }
                _ => return Ok(Expect::Absent),
            }
        }
    }

    fn read_binding(
        &mut self,
        at: &ModelPath,
        decl: &Rc<Decl>,
        name: &str,
    ) -> Result<Expect, Invalid> {
        let r = ModelRef {
            binding: name.to_owned(),
            steps: Vec::new(),
        };
        self.read(at, decl, &r)
    }

    /// The i64s at "0", "1", … until one reads absent, a failure ending it;
    /// then, if "pick" reads i, the i64 at "from"/[i] added, absent at a
    /// negative i.
    fn cell(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Invalid> {
        let mut sum = 0_i64;
        for term in 0_u32.. {
            match typed_int(self.read_binding(at, decl, &term.to_string())?) {
                Typed::Value(value) => sum = add(sum, value, at)?,
                Typed::Absent => break,
                Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
            }
        }
        let pick = match typed_int(self.read_binding(at, decl, "pick")?) {
            Typed::Value(pick) => pick,
            Typed::Absent => return Ok(Expect::Value(Out::Int(sum))),
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        };
        let Ok(pick) = u64::try_from(pick) else {
            return Ok(Expect::Absent);
        };
        let from = ModelRef {
            binding: String::from("from"),
            steps: vec![Step::index(pick)],
        };
        Ok(match typed_int(self.read(at, decl, &from)?) {
            Typed::Value(value) => Expect::Value(Out::Int(add(sum, value, at)?)),
            Typed::Absent => Expect::Absent,
            Typed::Failure(failure) => Expect::Failure(failure),
        })
    }

    /// The members at "members" with those at "extra" over them.
    fn members(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Invalid> {
        let members = typed_members(self.read_binding(at, decl, "members")?);
        let extra = typed_members(self.read_binding(at, decl, "extra")?);
        Ok(match (members, extra) {
            (Typed::Value(mut members), Typed::Value(extra)) => {
                members.extend(extra);
                Expect::Value(Out::Members(members))
            }
            (Typed::Value(members), Typed::Absent) => Expect::Value(Out::Members(members)),
            (Typed::Value(_), Typed::Failure(failure)) | (Typed::Failure(failure), _) => {
                Expect::Failure(failure)
            }
            (Typed::Absent, _) => Expect::Absent,
        })
    }

    /// Its members while "condition" reads true; absent while it reads
    /// false or nothing.
    fn guard(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Invalid> {
        match typed_bool(self.read_binding(at, decl, "condition")?) {
            Typed::Value(true) => self.members(at, decl),
            Typed::Value(false) | Typed::Absent => Ok(Expect::Absent),
            Typed::Failure(failure) => Ok(Expect::Failure(failure)),
        }
    }

    /// The count at "count", none below zero, and the declaration at
    /// "element".
    fn collection(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Invalid> {
        let count = match typed_int(self.read_binding(at, decl, "count")?) {
            Typed::Value(count) => u64::try_from(count).unwrap_or(0),
            Typed::Absent => return Ok(Expect::Absent),
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        };
        Ok(match typed_decl(self.read_binding(at, decl, "element")?) {
            Typed::Value(element) => Expect::Value(Out::Elements { count, element }),
            Typed::Absent => Expect::Absent,
            Typed::Failure(failure) => Expect::Failure(failure),
        })
    }
}

/// The declaration a node of `kind` whose value is `out` gives the child
/// at `step`.
fn declares(kind: ToyKind, out: &Out, step: &Step) -> Option<Decl> {
    match (kind, out) {
        (ToyKind::Scope | ToyKind::Guard, Out::Members(members)) => members.get(step).cloned(),
        (ToyKind::Collection, Out::Elements { count, element }) => match step {
            Step::Index(index) if index < count => Some(element.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn add(sum: i64, value: i64, at: &ModelPath) -> Result<i64, Invalid> {
    sum.checked_add(value)
        .ok_or_else(|| Invalid::Overflow { path: at.clone() })
}

fn out_of(value: &Val) -> Out {
    match value {
        Val::Int(value) => Out::Int(*value),
        Val::Bool(value) => Out::Bool(*value),
        Val::Members(members) => Out::Members(members.clone()),
        Val::Decl(decl) => Out::Decl(decl.clone()),
    }
}

impl Out {
    /// The name of the Rust type the core holds this value as.
    pub fn type_name(&self) -> &'static str {
        match self {
            Out::Int(_) => type_name::<i64>(),
            Out::Bool(_) => type_name::<bool>(),
            Out::Members(_) => type_name::<toy::Members>(),
            Out::Elements { .. } => type_name::<toy::Elements>(),
            Out::Decl(_) => type_name::<Declaration>(),
        }
    }
}

/// `read` typed as `expected`: `take` gives the value of that type, and a
/// value of another type is a failure naming both.
fn typed<T>(read: Expect, expected: &'static str, take: impl FnOnce(Out) -> Option<T>) -> Typed<T> {
    match read {
        Expect::Value(out) => {
            let found = out.type_name();
            take(out).map_or(
                Typed::Failure(Failure::WrongType { expected, found }),
                Typed::Value,
            )
        }
        Expect::Absent => Typed::Absent,
        Expect::Failure(failure) => Typed::Failure(failure),
    }
}

fn typed_int(read: Expect) -> Typed<i64> {
    typed(read, type_name::<i64>(), |out| match out {
        Out::Int(value) => Some(value),
        _ => None,
    })
}

fn typed_bool(read: Expect) -> Typed<bool> {
    typed(read, type_name::<bool>(), |out| match out {
        Out::Bool(value) => Some(value),
        _ => None,
    })
}

fn typed_members(read: Expect) -> Typed<BTreeMap<Step, Decl>> {
    typed(read, type_name::<toy::Members>(), |out| match out {
        Out::Members(members) => Some(members),
        _ => None,
    })
}

fn typed_decl(read: Expect) -> Typed<Decl> {
    typed(read, type_name::<Declaration>(), |out| match out {
        Out::Decl(decl) => Some(decl),
        _ => None,
    })
}

mod tests {
    use std::any::type_name;

    use reify_eval_core::{Declaration, Failure};

    use super::super::case::{HostEvent, Then};
    use super::super::design::*;
    use super::*;
    use crate::toy;

    fn answer(inputs: &Inputs, at: &ModelPath) -> Result<Expected, Invalid> {
        Reference::new(inputs.clone()).answer(at)
    }

    fn value(node: ToyKind, out: Out) -> Result<Expected, Invalid> {
        Ok(Expected {
            node: Some(node),
            outcome: Expect::Value(out),
        })
    }

    fn int_of(node: ToyKind, sum: i64) -> Result<Expected, Invalid> {
        value(node, Out::Int(sum))
    }

    fn absent(node: Option<ToyKind>) -> Result<Expected, Invalid> {
        Ok(Expected {
            node,
            outcome: Expect::Absent,
        })
    }

    fn failed(
        node: Option<ToyKind>,
        expected: &'static str,
        found: &'static str,
    ) -> Result<Expected, Invalid> {
        Ok(Expected {
            node,
            outcome: Expect::Failure(Failure::WrongType { expected, found }),
        })
    }

    fn at(root: &str, steps: &[&str]) -> ModelPath {
        path(root, steps.iter().map(|name| step(name)))
    }

    /// Root scope `r` over the members input "r.m" holding `held`, with the
    /// named ints `ints`.
    fn scope_r(held: Val, ints: &[(&str, i64)]) -> Inputs {
        let mut all = inputs([(root("r"), declaration(scope("r.m"))), (named("r.m"), held)]);
        all.extend(ints.iter().map(|(name, value)| (named(name), int(*value))));
        all
    }

    #[test]
    fn a_root_cell_over_two_named_ints_sums_them() {
        let design = inputs([
            (root("c"), declaration(cell([input("a"), input("b")]))),
            (named("a"), int(2)),
            (named("b"), int(3)),
        ]);
        assert_eq!(answer(&design, &at("c", &[])), int_of(ToyKind::Cell, 5));
    }

    #[test]
    fn a_missing_root_is_absent() {
        let design = inputs([(named("a"), int(2))]);
        assert_eq!(answer(&design, &at("nothing", &[])), absent(None));
    }

    #[test]
    fn a_root_whose_input_is_not_a_declaration_fails_naming_both_types() {
        let design = inputs([(root("c"), int(1))]);
        assert_eq!(
            answer(&design, &at("c", &[])),
            failed(None, type_name::<Declaration>(), type_name::<i64>())
        );
    }

    #[test]
    fn a_member_cell_reads_an_earlier_sibling_through_its_declarer() {
        let held = members([
            (step("a"), cell([input("x")])),
            (step("b"), cell([declarer([step("a")]), input("y")])),
        ]);
        let design = scope_r(held.clone(), &[("x", 4), ("y", 1)]);
        assert_eq!(answer(&design, &at("r", &["b"])), int_of(ToyKind::Cell, 5));
        let Val::Members(held) = held else {
            unreachable!("members() makes members")
        };
        assert_eq!(
            answer(&design, &at("r", &[])),
            value(ToyKind::Scope, Out::Members(held))
        );
    }

    #[test]
    fn a_path_below_a_cell_is_absent() {
        let design = inputs([
            (root("c"), declaration(cell([input("a")]))),
            (named("a"), int(2)),
        ]);
        assert_eq!(answer(&design, &at("c", &["x"])), absent(None));
    }

    #[test]
    fn a_step_below_a_snapshot_input_is_absent() {
        let held = members([
            (step("a"), cell([input("x")])),
            (
                step("b"),
                cell([input("y"), passed("members", [step("a")]), input("y")]),
            ),
            (
                step("c"),
                cell([input("y"), passed("extra", [step("a")]), input("y")]),
            ),
        ]);
        let mut design = scope_r(held.clone(), &[("x", 4), ("y", 1)]);
        design.insert(attached_to(at("r", &[]), "extra"), held);
        assert_eq!(answer(&design, &at("r", &["b"])), int_of(ToyKind::Cell, 1));
        assert_eq!(answer(&design, &at("r", &["c"])), int_of(ToyKind::Cell, 1));
    }

    #[test]
    fn a_declarer_and_a_passed_binding_on_a_root_read_absent() {
        let design = inputs([
            (
                root("d"),
                declaration(cell([input("a"), declarer([]), input("b")])),
            ),
            (
                root("e"),
                declaration(cell([input("a"), passed("0", []), input("b")])),
            ),
            (root("p"), declaration(picker(input("zero"), declarer([])))),
            (named("a"), int(2)),
            (named("b"), int(3)),
            (named("zero"), int(0)),
        ]);
        assert_eq!(answer(&design, &at("d", &[])), int_of(ToyKind::Cell, 2));
        assert_eq!(answer(&design, &at("e", &[])), int_of(ToyKind::Cell, 2));
        assert_eq!(answer(&design, &at("p", &[])), absent(Some(ToyKind::Cell)));
    }

    #[test]
    fn a_guard_on_a_false_flag_is_absent_and_so_is_a_path_under_it() {
        let held = members([(step("a"), cell([input("x")]))]);
        let design = inputs([
            (root("g"), declaration(guard(input("off"), "g.m"))),
            (root("h"), declaration(guard(input("on"), "g.m"))),
            (named("g.m"), held.clone()),
            (named("off"), flag(false)),
            (named("on"), flag(true)),
            (named("x"), int(7)),
        ]);
        assert_eq!(answer(&design, &at("g", &[])), absent(Some(ToyKind::Guard)));
        assert_eq!(answer(&design, &at("g", &["a"])), absent(None));
        let Val::Members(held) = held else {
            unreachable!("members() makes members")
        };
        assert_eq!(
            answer(&design, &at("h", &[])),
            value(ToyKind::Guard, Out::Members(held))
        );
        assert_eq!(answer(&design, &at("h", &["a"])), int_of(ToyKind::Cell, 7));
    }

    #[test]
    fn a_guard_on_an_absent_condition_is_absent() {
        let design = inputs([
            (root("g"), declaration(guard(input("missing"), "g.m"))),
            (named("g.m"), members([(step("a"), cell([]))])),
        ]);
        assert_eq!(answer(&design, &at("g", &[])), absent(Some(ToyKind::Guard)));
    }

    #[test]
    fn a_collection_declares_its_element_below_its_count_and_nothing_else() {
        let element = cell([passed("count", []), input("x")]);
        let design = inputs([
            (
                root("col"),
                declaration(collection(input("n"), input("el"))),
            ),
            (
                root("none"),
                declaration(collection(input("neg"), input("el"))),
            ),
            (named("el"), declaration(element.clone())),
            (named("n"), int(3)),
            (named("neg"), int(-1)),
            (named("x"), int(10)),
        ]);
        let elements = |count| Out::Elements {
            count,
            element: element.clone(),
        };
        assert_eq!(
            answer(&design, &at("col", &[])),
            value(ToyKind::Collection, elements(3))
        );
        let col = |i| path("col", [index(i)]);
        assert_eq!(answer(&design, &col(0)), int_of(ToyKind::Cell, 13));
        assert_eq!(answer(&design, &col(2)), int_of(ToyKind::Cell, 13));
        assert_eq!(answer(&design, &col(3)), absent(None));
        assert_eq!(
            answer(&design, &at("none", &[])),
            value(ToyKind::Collection, elements(0))
        );
        assert_eq!(answer(&design, &path("none", [index(0)])), absent(None));
    }

    #[test]
    fn a_picker_reads_the_picked_element_of_a_sibling_collection() {
        let held = members([
            (step("col"), collection(input("n"), input("el"))),
            (step("p"), picker(input("one"), declarer([step("col")]))),
            (step("q"), picker(input("five"), declarer([step("col")]))),
            (step("s"), picker(input("neg"), declarer([step("col")]))),
        ]);
        let mut design = scope_r(
            held,
            &[("n", 3), ("x", 10), ("one", 1), ("five", 5), ("neg", -1)],
        );
        design.insert(named("el"), declaration(cell([input("x")])));
        assert_eq!(answer(&design, &at("r", &["p"])), int_of(ToyKind::Cell, 10));
        assert_eq!(
            answer(&design, &at("r", &["q"])),
            absent(Some(ToyKind::Cell))
        );
        assert_eq!(
            answer(&design, &at("r", &["s"])),
            absent(Some(ToyKind::Cell))
        );
    }

    #[test]
    fn a_cell_reading_a_scope_fails_naming_both_types() {
        let design = scope_r(members([(step("a"), cell([declarer([])]))]), &[]);
        let (int, scope) = (type_name::<i64>(), type_name::<toy::Members>());
        assert_eq!(
            answer(&design, &at("r", &["a"])),
            failed(Some(ToyKind::Cell), int, scope)
        );
    }

    #[test]
    fn a_child_of_a_failed_node_answers_that_failure() {
        let mut design = scope_r(members([(step("a"), cell([declarer([])]))]), &[]);
        design.insert(root("bad"), int(1));
        let (int, scope) = (type_name::<i64>(), type_name::<toy::Members>());
        assert_eq!(
            answer(&design, &at("r", &["a", "z"])),
            failed(None, int, scope)
        );
        let declaration = type_name::<Declaration>();
        assert_eq!(
            answer(&design, &at("bad", &["x"])),
            failed(None, declaration, int)
        );
    }

    #[test]
    fn an_attached_extra_input_adds_a_member_and_overrides_one_at_the_same_step() {
        let held = members([
            (step("a"), cell([input("x")])),
            (step("b"), cell([input("y")])),
        ]);
        let mut design = scope_r(held, &[("x", 1), ("y", 5)]);
        let extra = members([
            (step("b"), cell([input("x"), input("x")])),
            (step("c"), cell([input("y")])),
        ]);
        design.insert(attached_to(at("r", &[]), "extra"), extra);
        assert_eq!(answer(&design, &at("r", &["a"])), int_of(ToyKind::Cell, 1));
        assert_eq!(answer(&design, &at("r", &["b"])), int_of(ToyKind::Cell, 2));
        assert_eq!(answer(&design, &at("r", &["c"])), int_of(ToyKind::Cell, 5));
    }

    #[test]
    fn a_cell_whose_term_is_below_its_own_path_is_a_cycle() {
        let design = scope_r(
            members([(step("a"), cell([declarer([step("a"), step("x")])]))]),
            &[],
        );
        assert_eq!(
            answer(&design, &at("r", &["a"])),
            Err(Invalid::Cycle {
                path: at("r", &["a"])
            })
        );
    }

    #[test]
    fn first_invalid_finds_a_cycle_an_edit_makes_at_a_demanded_path() {
        let a_over = |term| members([(step("a"), cell([term]))]);
        let mut case = Case {
            inputs: scope_r(a_over(input("x")), &[("x", 1)]),
            events: vec![HostEvent::Evaluate {
                on: 0,
                paths: vec![at("r", &["a"])],
                then: Then::End,
            }],
        };
        assert_eq!(first_invalid(&case), None);
        let below_itself = a_over(declarer([step("a"), step("x")]));
        case.events.push(HostEvent::Edit {
            from: 0,
            changes: vec![(named("r.m"), Some(below_itself))],
        });
        assert_eq!(
            first_invalid(&case),
            Some(Invalid::Cycle {
                path: at("r", &["a"])
            })
        );
    }

    #[test]
    fn each_path_is_worked_out_once_however_often_it_is_read() {
        let name = |k: usize| format!("c{k}");
        let chain = (0..20).map(|k| {
            let terms = if k == 0 {
                vec![input("unit")]
            } else {
                vec![declarer([step(&name(k - 1))]); 2]
            };
            (step(&name(k)), cell(terms))
        });
        let design = scope_r(members(chain), &[("unit", 3)]);
        let mut reference = Reference::new(design);
        let last = reference.answer(&at("r", &["c19"]));
        assert_eq!(last, int_of(ToyKind::Cell, 3 << 19));
        assert_eq!(reference.work_outs(), 21, "the root and its twenty members");
    }

    #[test]
    fn a_sum_past_i64_is_an_overflow() {
        let design = inputs([
            (root("c"), declaration(cell([input("big"), input("big")]))),
            (named("big"), int(i64::MAX)),
        ]);
        assert_eq!(
            answer(&design, &at("c", &[])),
            Err(Invalid::Overflow { path: at("c", &[]) })
        );
    }
}
