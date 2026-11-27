//! The reference interpreter: what each path of a snapshot answers, worked
//! out directly from the design model and the meaning of the toy kinds,
//! with no store, index or executor and no code shared with the core or the
//! toy kinds' bodies. It remembers each path's answer within one snapshot,
//! so a design whose nodes read one another many times stays linear. A
//! path whose answer needs its own answer is on a ring and answers "cycle"
//! (contract 7.7); arithmetic past i64, which no evaluation can answer, it
//! reports as invalid rather than as an outcome.
//!
//! A call declares its children as it runs (contract 7.1), so each child is
//! answered before its call, with its own outcome. A child path asked
//! before its call ran is resolving, waiting on its call's answer, when
//! the call declares it: the call takes it over and works it out in its own
//! frame, as the core takes a waiting path over, and a ring the child then
//! closes through a frame outside the resolution is finished from the
//! child's own frame once its call has read the child's "cycle".
//!
//! A solve tries each candidate in a trial of its own (contract 6): a
//! reference over the same inputs whose memo is its own, as the core's
//! view has an index of its own, in which a node at a place the solve's
//! unknowns name answers the candidate's part and every other path is
//! worked out afresh. The solve resolves each constraint relative to
//! itself and reads it there. The solve's own path is in progress outside
//! the trial, so a trial that reads it halts as a ring that the solve's
//! own frame answers "cycle" (contract 7.7). A solve met inside a trial
//! would open a view inside a view, which the core does not evaluate until
//! views nest, so the case is invalid.

use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use reify_eval_core::{Failure, Kind, Limit, Space, Step, StepKey};

use super::case::Case;
use super::design::{
    Bind, Decl, Inputs, Key, ModelPath, ModelRef, Part, PartType, Site, ToyKind, Val, call_step,
    constraint, unknown,
};
use super::expected::{
    Expect, Expected, Invalid, Out, Typed, out_of, part_out, typed_bool, typed_calls, typed_decl,
    typed_int, typed_members, typed_problem, typed_shape, typed_solution,
};
use crate::toy;

/// Why working out a path stopped short of its answer: the case is
/// invalid, or the recursion re-entered `entered`, which is in progress, so
/// every path worked out from there up to the latest is on one ring. With
/// one ref per read in every generated kind, each node waits on one other,
/// and these are the paths whose waits the core closes into a ring at rest.
/// In a trial, the recursion may instead have reached the node of the
/// solve that opened it, which is in progress outside the trial: every
/// path of the trial worked out from there up is on a ring through that
/// solve, and the trial hands the solve's own frame that ring.
enum Halt {
    Invalid(Invalid),
    Ring { entered: ModelPath },
    Reopened,
}

impl From<Invalid> for Halt {
    fn from(invalid: Invalid) -> Self {
        Halt::Invalid(invalid)
    }
}

impl Halt {
    /// What a public call reports.
    ///
    /// # Panics
    ///
    /// On a ring: the path it re-entered was entered within the same call,
    /// whose own frame answers it, and a trial, whose halt at its opener is
    /// a ring of the opener's, is called by no public call.
    fn invalid(self) -> Invalid {
        match self {
            Halt::Invalid(invalid) => invalid,
            Halt::Ring { entered } => {
                unreachable!("the ring entered at {entered:?} is answered by that path's frame")
            }
            Halt::Reopened => unreachable!("a trial's halt at its opener is the opener's ring"),
        }
    }
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

/// A place a node's work reached: one a read named, or a child its run
/// declared and read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reached {
    Read(ModelPath),
    Child(ModelPath),
}

impl Reached {
    pub fn path(&self) -> &ModelPath {
        match self {
            Reached::Read(path) | Reached::Child(path) => path,
        }
    }
}

/// What a ref names: an outcome settled without a node (nothing, an input
/// or a constant), or a place.
enum Target {
    Settled(Expect),
    Path(ModelPath),
}

/// What a trial supposes: the outcome at each place its view substitutes,
/// and the path of the solve that opened the view.
struct Supposition {
    opener: ModelPath,
    supposed: BTreeMap<ModelPath, Expect>,
}

/// How trying one candidate of a solve ended.
enum Tried {
    /// Every constraint held.
    Held,
    /// A constraint did not hold.
    Failed,
    /// The substitution named no place of its own: the solve's failure.
    Ended(Failure),
}

/// One view a solve opened: the reference that answered the reads made
/// through it, and the places those reads named, in order.
pub struct Trial {
    reference: Reference,
    demand: Vec<ModelPath>,
}

impl Trial {
    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    /// The places the solve's reads through the view named, in order: what
    /// the trial was demanded.
    pub fn demand(&self) -> &[ModelPath] {
        &self.demand
    }

    /// Each path the trial answered, with its answer.
    pub fn answers(&self) -> impl Iterator<Item = (&ModelPath, &Expected)> {
        self.reference
            .answers
            .iter()
            .filter_map(|(path, answer)| Some((path, answer.as_deref().ok()?)))
    }

    /// How many distinct paths the trial worked out the declaration of.
    pub fn paths_resolved(&self) -> usize {
        self.reference.paths_resolved()
    }
}

/// The answers of one snapshot, or of one trial under it.
pub struct Reference {
    /// The snapshot's inputs, shared with each trial under it.
    inputs: Rc<Inputs>,
    /// The snapshot's depth setting.
    depth: Option<u64>,
    /// The snapshot's children-per-node setting.
    children: Option<u64>,
    answers: BTreeMap<ModelPath, Result<Rc<Expected>, Invalid>>,
    declared: BTreeMap<ModelPath, Declared>,
    /// Each path whose answer is being worked out, at its height in the
    /// recursion: the number of such paths when it was entered.
    in_progress: BTreeMap<ModelPath, usize>,
    /// Each path whose declaration waits on its parent's answer, at the
    /// height its resolution began.
    resolving: BTreeMap<ModelPath, usize>,
    /// Each child taken over whose ring is still to be finished from its
    /// own frame, with the path the ring entered.
    deferred: BTreeMap<ModelPath, ModelPath>,
    /// The distinct steps each call's run declared a child at, in the
    /// order it first declared them.
    run_children: BTreeMap<ModelPath, Vec<Step>>,
    /// What each node's work reached, in order.
    reached: BTreeMap<ModelPath, Vec<Reached>>,
    work_outs: usize,
    /// What this reference supposes, if it is a trial.
    supposition: Option<Supposition>,
    /// The trials each solve ran, in order.
    trials: BTreeMap<ModelPath, Vec<Trial>>,
}

impl Reference {
    pub fn new(inputs: Inputs) -> Self {
        Self::supposing(Rc::new(inputs), None)
    }

    fn supposing(inputs: Rc<Inputs>, supposition: Option<Supposition>) -> Self {
        Self {
            depth: setting(&inputs, Limit::Depth),
            children: setting(&inputs, Limit::ChildrenPerNode),
            inputs,
            answers: BTreeMap::new(),
            declared: BTreeMap::new(),
            in_progress: BTreeMap::new(),
            resolving: BTreeMap::new(),
            deferred: BTreeMap::new(),
            run_children: BTreeMap::new(),
            reached: BTreeMap::new(),
            work_outs: 0,
            supposition,
            trials: BTreeMap::new(),
        }
    }

    pub fn answer(&mut self, path: &ModelPath) -> Result<Expected, Invalid> {
        self.shared(path)
            .map(|answer| Expected::clone(&answer))
            .map_err(Halt::invalid)
    }

    /// The answer at `path`, shared with the memo. A path re-entered while
    /// it is in progress halts the recursion; each path the halt passes
    /// back through is on the ring and answers "cycle", and the re-entered
    /// path's own frame returns that answer to its reader. In a trial, the
    /// solve that opened it is in progress outside it: a halt there passes
    /// back through every frame of the trial, each but the solve's own path
    /// answering "cycle". A child its call took over with a ring still to
    /// finish passes the halt on from here.
    fn shared(&mut self, path: &ModelPath) -> Result<Rc<Expected>, Halt> {
        if let Some(known) = self.answers.get(path) {
            return Ok(Rc::clone(known.as_ref().map_err(Invalid::clone)?));
        }
        if self.in_progress.contains_key(path) {
            return Err(Halt::Ring {
                entered: path.clone(),
            });
        }
        self.in_progress
            .insert(path.clone(), self.in_progress.len());
        let worked = self.work_out(path);
        self.in_progress.remove(path);
        let answer = match worked {
            Ok(answer) => Ok(Rc::new(answer)),
            Err(Halt::Invalid(invalid)) => Err(invalid),
            Err(Halt::Ring { entered }) => {
                let on_ring = self.on_ring(path);
                return if entered == *path {
                    Ok(on_ring)
                } else {
                    Err(Halt::Ring { entered })
                };
            }
            Err(Halt::Reopened) => return Err(self.reopened(path)),
        };
        self.answers.insert(path.clone(), answer.clone());
        if let Some(entered) = self.deferred.remove(path) {
            return Err(Halt::Ring { entered });
        }
        answer.map_err(Halt::from)
    }

    /// Passes on, from the frame of `path`, a trial's halt at the solve
    /// that opened it: `path` is on that solve's ring, and answers "cycle"
    /// unless it is the solve's own path, which is the solve's node outside
    /// the trial.
    fn reopened(&mut self, path: &ModelPath) -> Halt {
        if !self.opened_by(path) {
            self.on_ring(path);
        }
        Halt::Reopened
    }

    /// Answers `path` "cycle", as a node on a ring.
    fn on_ring(&mut self, path: &ModelPath) -> Rc<Expected> {
        let on_ring = Rc::new(Expected {
            node: self.node_declared(path),
            outcome: Expect::Failure(Failure::Cycle),
        });
        self.answers.insert(path.clone(), Ok(Rc::clone(&on_ring)));
        on_ring
    }

    /// Whether this reference is a trial of the solve at `path`.
    fn opened_by(&self, path: &ModelPath) -> bool {
        self.supposition
            .as_ref()
            .is_some_and(|supposition| supposition.opener == *path)
    }

    /// The outcome this reference supposes at `path`, if it is a trial
    /// whose view substitutes there.
    fn supposed(&self, path: &ModelPath) -> Option<&Expect> {
        self.supposition.as_ref()?.supposed.get(path)
    }

    /// The trials the solve at `opener` ran, in order.
    pub fn trials(&self, opener: &ModelPath) -> &[Trial] {
        self.trials.get(opener).map_or(&[], Vec::as_slice)
    }

    /// Every trial any solve ran.
    pub fn every_trial(&self) -> impl Iterator<Item = &Trial> {
        self.trials.values().flatten()
    }

    /// The kind of the node declared at `path`, if its declaration has
    /// been worked out and is a node.
    fn node_declared(&self, path: &ModelPath) -> Option<ToyKind> {
        match self.declared.get(path)? {
            Declared::Node(decl) => Some(decl.kind),
            Declared::Settled(_) => None,
        }
    }

    /// How many times a path's answer has been worked out, memo aside.
    pub fn work_outs(&self) -> usize {
        self.work_outs
    }

    /// How many distinct paths the reference and each of its trials have
    /// worked out the declaration of: the paths the core resolves for the
    /// same reads, in the snapshot and in each view. A path waiting on a
    /// ring halts before it is worked out, as the core answers it "cycle"
    /// without resolving it.
    pub fn paths_resolved(&self) -> usize {
        let trials: usize = self.every_trial().map(Trial::paths_resolved).sum();
        self.declared.len() + trials
    }

    /// Whether a call's run declared a child at `path`, as answering it
    /// found.
    pub fn declared_by_run(&self, path: &ModelPath) -> bool {
        path.parent()
            .is_some_and(|(call, step)| self.steps_declared_by(&call).contains(step))
    }

    /// The distinct steps the run of the call at `call` declared a child
    /// at, in the order it first declared them; none if no run did.
    pub fn steps_declared_by(&self, call: &ModelPath) -> &[Step] {
        self.run_children.get(call).map_or(&[], Vec::as_slice)
    }

    /// Each node whose work reached a place, with what it reached in order.
    pub fn reaches(&self) -> impl Iterator<Item = (&ModelPath, &[Reached])> {
        self.reached
            .iter()
            .map(|(node, reached)| (node, reached.as_slice()))
    }

    /// What the work of the node at `node` reached, in order.
    pub fn reached_by(&self, node: &ModelPath) -> &[Reached] {
        self.reached.get(node).map_or(&[], Vec::as_slice)
    }

    fn reach(&mut self, node: &ModelPath, reached: Reached) {
        self.reached.entry(node.clone()).or_default().push(reached);
    }

    /// Whether the reference has worked out the declaration of `path`,
    /// resolving it.
    pub fn resolved(&self, path: &ModelPath) -> bool {
        self.declared.contains_key(path)
    }

    /// The declaration of the node at `path`, if one is declared there.
    pub fn declaration_at(&mut self, path: &ModelPath) -> Result<Option<Decl>, Invalid> {
        Ok(match self.declaration(path).map_err(Halt::invalid)? {
            Declared::Node(decl) => Some(Decl::clone(&decl)),
            Declared::Settled(_) => None,
        })
    }

    /// What the node at `reader` reads at `r`; absent where no node is
    /// declared there.
    pub fn read_at(&mut self, reader: &ModelPath, r: &ModelRef) -> Result<Expect, Invalid> {
        let read = match self.declaration(reader) {
            Ok(Declared::Node(decl)) => self.read(reader, &decl, r),
            Ok(Declared::Settled(_)) => Ok(Expect::Absent),
            Err(halt) => Err(halt),
        };
        read.map_err(Halt::invalid)
    }

    /// The answer at `path`: what its declaration works out to, or, where
    /// its call took it over while it was resolving, the answer given then.
    fn work_out(&mut self, path: &ModelPath) -> Result<Expected, Halt> {
        self.work_outs += 1;
        let declared = self.declaration(path)?;
        if let Some(taken) = self.answers.get(path) {
            return Ok(Expected::clone(taken.as_ref().map_err(Invalid::clone)?));
        }
        self.run(path, &declared)
    }

    /// What the node `declared` at `path` works out to: in a trial whose
    /// view substitutes there, the outcome supposed. In a trial, a node at
    /// the path of the solve that opened it is that solve, in progress
    /// outside the trial; only once the trial's structure declares a node
    /// there, so that where it declares none, the trial answers that.
    fn run(&mut self, path: &ModelPath, declared: &Declared) -> Result<Expected, Halt> {
        let decl = match declared {
            Declared::Node(decl) => Rc::clone(decl),
            Declared::Settled(outcome) => {
                return Ok(Expected {
                    node: None,
                    outcome: outcome.clone(),
                });
            }
        };
        if self.opened_by(path) {
            return Err(Halt::Reopened);
        }
        if let Some(supposed) = self.supposed(path) {
            return Ok(Expected {
                node: Some(decl.kind),
                outcome: self.within_children(path, decl.kind, supposed.clone()),
            });
        }
        let outcome = match decl.kind {
            ToyKind::Cell => self.cell(path, &decl)?,
            ToyKind::Scope => self.members(path, &decl)?,
            ToyKind::Guard => self.guard(path, &decl)?,
            ToyKind::Collection => self.collection(path, &decl)?,
            ToyKind::Shape => self.shape(path, &decl)?,
            ToyKind::Measure => self.measure(path, &decl)?,
            ToyKind::Call => self.call(path, &decl)?,
            ToyKind::Solve => self.solve(path, &decl)?,
            ToyKind::Solved(part_type) => self.solved(path, &decl, part_type)?,
        };
        Ok(Expected {
            node: Some(decl.kind),
            outcome: self.within_children(path, decl.kind, outcome),
        })
    }

    /// A root from its snapshot input, read as a declaration; a child its
    /// parent's run declared as that run declared it; any other path from
    /// what its parent's answer declares at its step. A node deeper than
    /// the depth setting is that failure instead.
    fn declaration(&mut self, path: &ModelPath) -> Result<Declared, Halt> {
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
                self.resolving.insert(path.clone(), self.in_progress.len());
                let above = self.shared(&parent);
                self.resolving.remove(path);
                let above = above?;
                if let Some(by_run) = self.declared.get(path) {
                    return Ok(by_run.clone());
                }
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
        let declared = match (declared, self.too_deep(path)) {
            (Declared::Node(_), Some(failure)) => Declared::Settled(Expect::Failure(failure)),
            (declared, _) => declared,
        };
        self.declared.insert(path.clone(), declared.clone());
        Ok(declared)
    }

    /// The failure of a node at `path` if it is more steps below its root
    /// than the depth setting allows.
    fn too_deep(&self, path: &ModelPath) -> Option<Failure> {
        let limit = self.depth?;
        let depth = u64::try_from(path.steps.len()).expect("a path's depth fits in a u64");
        (depth > limit).then_some(Failure::TooDeep { limit })
    }

    /// `outcome`, what a node of `kind` at `path` works out, or the failure
    /// of a node that declares more children than the children setting
    /// allows: by its value, or, for a call, by its run.
    fn within_children(&self, path: &ModelPath, kind: ToyKind, outcome: Expect) -> Expect {
        let (Some(limit), Expect::Value(out)) = (self.children, &outcome) else {
            return outcome;
        };
        let count = |count: usize| u64::try_from(count).expect("a count of children fits in a u64");
        let children = match (kind, out) {
            (ToyKind::Scope | ToyKind::Guard, Out::Members(members)) => count(members.len()),
            (ToyKind::Collection, Out::Elements { count, .. }) => *count,
            (ToyKind::Call, _) => count(self.steps_declared_by(path).len()),
            _ => 0,
        };
        if children > limit {
            Expect::Failure(Failure::TooManyChildren { limit })
        } else {
            outcome
        }
    }

    fn input(&self, key: &Key) -> Expect {
        self.inputs
            .get(key)
            .map_or(Expect::Absent, |value| Expect::Value(out_of(value)))
    }

    /// What the node at `reader`, declared `decl`, reads at `r`.
    fn read(&mut self, reader: &ModelPath, decl: &Rc<Decl>, r: &ModelRef) -> Result<Expect, Halt> {
        match self.target(reader, decl, r)? {
            Target::Settled(outcome) => Ok(outcome),
            Target::Path(target) => {
                self.reach(reader, Reached::Read(target.clone()));
                Ok(self.shared(&target)?.outcome.clone())
            }
        }
    }

    /// What the node at `reader`, declared `decl`, names at `r`: an unset
    /// binding names nothing; an input or a constant names its value, and
    /// nothing with steps below it; a binding relative to the declarer names
    /// nothing on a root; a passed binding is the declarer's own binding,
    /// the steps still to take carried along (contract 2.1).
    fn target(
        &mut self,
        reader: &ModelPath,
        decl: &Rc<Decl>,
        r: &ModelRef,
    ) -> Result<Target, Halt> {
        let (mut node, mut decl) = (reader.clone(), Rc::clone(decl));
        let (mut name, mut steps) = (r.binding.clone(), r.steps.clone());
        loop {
            let declarer = node.parent().map(|(declarer, _)| declarer);
            match (decl.bindings.get(&name).cloned(), declarer) {
                (Some(Bind::Input(input)), _) if steps.is_empty() => {
                    return Ok(Target::Settled(self.input(&Key::Named(input))));
                }
                (Some(Bind::Constant(value)), _) if steps.is_empty() => {
                    return Ok(Target::Settled(Expect::Value(out_of(&value))));
                }
                (Some(Bind::Declarer(place)), Some(declarer)) => {
                    let target = place
                        .iter()
                        .chain(&steps)
                        .fold(declarer, |at, step| at.child(step.clone()));
                    return Ok(Target::Path(target));
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
                _ => return Ok(Target::Settled(Expect::Absent)),
            }
        }
    }

    fn read_binding(
        &mut self,
        at: &ModelPath,
        decl: &Rc<Decl>,
        name: &str,
    ) -> Result<Expect, Halt> {
        self.read(at, decl, &bare(name))
    }

    /// The i64s at "0", "1", … until one reads absent; then, if "pick"
    /// reads i, the i64 at "from"/[i] added, absent at a negative i; then,
    /// if "divisor" reads an i64, the sum divided by it, truncating, and
    /// zero the cell's own failure.
    fn cell(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        let sum = match self.cell_sum(at, decl)? {
            Typed::Value(sum) => sum,
            Typed::Absent => return Ok(Expect::Absent),
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        };
        let divisor = ModelRef {
            binding: String::from("divisor"),
            steps: Vec::new(),
        };
        Ok(match self.term(at, decl, &divisor)? {
            Typed::Absent => Expect::Value(Out::Int(sum)),
            Typed::Value(0) => Expect::Failure(Failure::OfKind {
                kind: toy::Cell::ID,
                code: toy::DIVISION_BY_ZERO,
            }),
            Typed::Value(divisor) => {
                let quotient = sum
                    .checked_div(divisor)
                    .ok_or_else(|| Invalid::Overflow { path: at.clone() })?;
                Expect::Value(Out::Int(quotient))
            }
            Typed::Failure(failure) => Expect::Failure(failure),
        })
    }

    /// The cell's terms added, with the picked element; absent where the
    /// cell is.
    fn cell_sum(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Typed<i64>, Halt> {
        let mut sum = 0_i64;
        for term in 0_u32.. {
            let r = ModelRef {
                binding: term.to_string(),
                steps: Vec::new(),
            };
            match self.term(at, decl, &r)? {
                Typed::Value(value) => sum = add(sum, value, at)?,
                Typed::Absent => break,
                failed @ Typed::Failure(_) => return Ok(failed),
            }
        }
        let pick = ModelRef {
            binding: String::from("pick"),
            steps: Vec::new(),
        };
        let pick = match self.term(at, decl, &pick)? {
            Typed::Value(pick) => pick,
            Typed::Absent => return Ok(Typed::Value(sum)),
            failed @ Typed::Failure(_) => return Ok(failed),
        };
        let Ok(pick) = u64::try_from(pick) else {
            return Ok(Typed::Absent);
        };
        let from = ModelRef {
            binding: String::from("from"),
            steps: vec![Step::index(pick)],
        };
        Ok(match self.term(at, decl, &from)? {
            Typed::Value(value) => Typed::Value(add(sum, value, at)?),
            other => other,
        })
    }

    /// What the cell at `at` reads at `r` as an i64. A failure ends the
    /// cell: a wrong type is its answer, whether found by this read or by
    /// the node read, and any other makes it blocked by `r`.
    fn term(&mut self, at: &ModelPath, decl: &Rc<Decl>, r: &ModelRef) -> Result<Typed<i64>, Halt> {
        Ok(match typed_int(self.read(at, decl, r)?) {
            Typed::Failure(wrong @ Failure::WrongType { .. }) => Typed::Failure(wrong),
            Typed::Failure(_) => Typed::Failure(Failure::BlockedBy { at: r.to_core() }),
            read => read,
        })
    }

    /// The members at "members".
    fn members(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        Ok(
            match typed_members(self.read_binding(at, decl, "members")?) {
                Typed::Value(members) => Expect::Value(Out::Members(members)),
                Typed::Absent => Expect::Absent,
                Typed::Failure(failure) => Expect::Failure(failure),
            },
        )
    }

    /// Its members while "condition" reads true; absent while it reads
    /// false or nothing.
    fn guard(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        match typed_bool(self.read_binding(at, decl, "condition")?) {
            Typed::Value(true) => self.members(at, decl),
            Typed::Value(false) | Typed::Absent => Ok(Expect::Absent),
            Typed::Failure(failure) => Ok(Expect::Failure(failure)),
        }
    }

    /// The count at "count", none below zero, and the declaration at
    /// "element".
    fn collection(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
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

    /// A box whose volume is the cube of the i64 at "side".
    fn shape(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        Ok(match typed_int(self.read_binding(at, decl, "side")?) {
            Typed::Value(side) => {
                let volume = side
                    .checked_mul(side)
                    .and_then(|square| square.checked_mul(side))
                    .ok_or_else(|| Invalid::Overflow { path: at.clone() })?;
                Expect::Value(Out::Shape { volume })
            }
            Typed::Absent => Expect::Absent,
            Typed::Failure(failure) => Expect::Failure(failure),
        })
    }

    /// The volume of the box at "shape".
    fn measure(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        Ok(match typed_shape(self.read_binding(at, decl, "shape")?) {
            Typed::Value(volume) => Expect::Value(Out::Int(volume)),
            Typed::Absent => Expect::Absent,
            Typed::Failure(failure) => Expect::Failure(failure),
        })
    }

    /// The i64s at "0", "1", … until one reads absent, a failure ending it
    /// as it ends a cell. Then "when": false declares nothing and answers
    /// the sum, absent counts as true, a failure is its answer. Then
    /// "calls": absent declares nothing and answers the sum, a failure or a
    /// value of another type is its answer. Then each site's children, in
    /// order, are declared and read one at a time, each the callee with the
    /// site's argument bound to a constant: one at the chain of the site's
    /// numbers given the sum plus the offset, or, for a site that loops,
    /// one per term at that chain then the term's index, given the term.
    /// It answers the sum plus every child's i64: a child absent adds
    /// nothing, a child's wrong type is its answer, any other failure of a
    /// child blocks it by that child, and a second, different declaration
    /// at one step is a conflict.
    fn call(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        let mut terms = Vec::new();
        for term in 0_u32.. {
            let r = ModelRef {
                binding: term.to_string(),
                steps: Vec::new(),
            };
            match self.term(at, decl, &r)? {
                Typed::Value(value) => terms.push(value),
                Typed::Absent => break,
                Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
            }
        }
        let sum = terms.iter().try_fold(0, |sum, term| add(sum, *term, at))?;
        match typed_bool(self.read_binding(at, decl, "when")?) {
            Typed::Value(false) => return Ok(Expect::Value(Out::Int(sum))),
            Typed::Value(true) | Typed::Absent => {}
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        }
        let sites = match typed_calls(self.read_binding(at, decl, "calls")?) {
            Typed::Value(sites) => sites,
            Typed::Absent => return Ok(Expect::Value(Out::Int(sum))),
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        };
        let mut run = Vec::new();
        let called = self.call_children(at, &sites, &terms, sum, &mut run);
        let steps = run.into_iter().map(|(step, _)| step).collect();
        self.run_children.insert(at.clone(), steps);
        called
    }

    /// The call at `at`'s sum plus what the children of `sites` add, each
    /// declared and read in turn; `run` holds the children its run has
    /// declared, with their steps, in order.
    fn call_children(
        &mut self,
        at: &ModelPath,
        sites: &[Site],
        terms: &[i64],
        sum: i64,
        run: &mut Vec<(Step, Decl)>,
    ) -> Result<Expect, Halt> {
        let mut total = sum;
        for site in sites {
            for (step, argument) in site_children(site, terms, sum, at)? {
                let child = at.child(step.clone());
                let callee = given(site, argument);
                match run.iter().find(|(declared, _)| *declared == step) {
                    Some((_, earlier)) if *earlier != callee => {
                        return Ok(Expect::Failure(Failure::ConflictingChild { step }));
                    }
                    Some(_) => {}
                    None => {
                        run.push((step.clone(), callee.clone()));
                        self.declare_child(&child, callee);
                    }
                }
                self.reach(at, Reached::Child(child.clone()));
                match typed_int(self.child_answer(&child)?.outcome.clone()) {
                    Typed::Value(value) => total = add(total, value, at)?,
                    Typed::Absent => {}
                    Typed::Failure(wrong @ Failure::WrongType { .. }) => {
                        return Ok(Expect::Failure(wrong));
                    }
                    Typed::Failure(_) => return Ok(Expect::Failure(toy::blocked_by_child(step))),
                }
            }
        }
        Ok(Expect::Value(Out::Int(total)))
    }

    /// The problem at "problem": absent or a failure is its answer. Then
    /// each candidate in order, tried in a trial of its own; the first
    /// whose constraints all hold is its answer, and if none holds, its own
    /// failure. A solve inside a trial is invalid.
    fn solve(&mut self, at: &ModelPath, decl: &Rc<Decl>) -> Result<Expect, Halt> {
        if self.supposition.is_some() {
            return Err(Invalid::ViewInsideView { path: at.clone() }.into());
        }
        let problem = match typed_problem(self.read_binding(at, decl, "problem")?) {
            Typed::Value(problem) => problem,
            Typed::Absent => return Ok(Expect::Absent),
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        };
        for candidate in &problem.candidates {
            match self.try_candidate(at, decl, candidate, problem.constraints)? {
                Tried::Held => return Ok(Expect::Value(Out::Solution(candidate.clone()))),
                Tried::Failed => {}
                Tried::Ended(failure) => return Ok(Expect::Failure(failure)),
            }
        }
        Ok(Expect::Failure(Failure::OfKind {
            kind: toy::Solve::ID,
            code: toy::NO_CANDIDATE_HOLDS,
        }))
    }

    /// Tries `candidate` as the solve at `at` does: its `constraints` are
    /// read one at a time in a trial supposing its unknowns to be the
    /// candidate's parts, which opens on the first read, and the trial
    /// stops at the first that does not hold. A constraint holds if it
    /// reads absent or an i64 of zero or more.
    fn try_candidate(
        &mut self,
        at: &ModelPath,
        decl: &Rc<Decl>,
        candidate: &[Part],
        constraints: u64,
    ) -> Result<Tried, Halt> {
        if constraints == 0 {
            return Ok(Tried::Held);
        }
        let supposed = match self.substitution(at, decl, candidate)? {
            Ok(supposed) => supposed,
            Err(failure) => return Ok(Tried::Ended(failure)),
        };
        let supposition = Supposition {
            opener: at.clone(),
            supposed,
        };
        let mut trial = Trial {
            reference: Reference::supposing(Rc::clone(&self.inputs), Some(supposition)),
            demand: Vec::new(),
        };
        let held = self.constraints_hold(at, decl, &mut trial, constraints);
        self.trials.entry(at.clone()).or_default().push(trial);
        Ok(if held? { Tried::Held } else { Tried::Failed })
    }

    /// The outcome a trial of the solve at `at` supposes at each place its
    /// unknowns name: `candidate`'s parts, one each. An unknown naming no
    /// node's place, or a place an earlier one names, is the solve's
    /// failure (contract 6: what a view substitutes has exactly one path).
    fn substitution(
        &mut self,
        at: &ModelPath,
        decl: &Rc<Decl>,
        candidate: &[Part],
    ) -> Result<Result<BTreeMap<ModelPath, Expect>, Failure>, Halt> {
        let mut supposed = BTreeMap::new();
        for (index, part) in candidate.iter().enumerate() {
            let r = bare(&unknown(index));
            let not_supposable = Failure::NotSupposable { at: r.to_core() };
            let Target::Path(place) = self.target(at, decl, &r)? else {
                return Ok(Err(not_supposable));
            };
            if supposed.contains_key(&place) {
                return Ok(Err(not_supposable));
            }
            supposed.insert(place, Expect::Value(part_out(*part)));
        }
        Ok(Ok(supposed))
    }

    /// Whether each of the `constraints` of the solve at `at` holds, each
    /// resolved here and read in `trial`, stopping at the first that does
    /// not. A read that reached the solve's own node under the trial is a
    /// ring entered at the solve.
    fn constraints_hold(
        &mut self,
        at: &ModelPath,
        decl: &Rc<Decl>,
        trial: &mut Trial,
        constraints: u64,
    ) -> Result<bool, Halt> {
        for index in 0..constraints {
            let read = match self.target(at, decl, &bare(&constraint(index)))? {
                Target::Settled(outcome) => outcome,
                Target::Path(path) => {
                    trial.demand.push(path.clone());
                    let read = trial.reference.shared(&path).map_err(|halt| match halt {
                        Halt::Reopened => Halt::Ring {
                            entered: at.clone(),
                        },
                        other => other,
                    });
                    read?.outcome.clone()
                }
            };
            let held = match typed_int(read) {
                Typed::Value(value) => value >= 0,
                Typed::Absent => true,
                Typed::Failure(_) => false,
            };
            if !held {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The solution at "solve": absent is its answer, and a failure makes
    /// it blocked by the solve. Then its part at the i64 "part" as
    /// `part_type`: past the end it is absent, and a part of the other type
    /// is a wrong type naming both.
    fn solved(
        &mut self,
        at: &ModelPath,
        decl: &Rc<Decl>,
        part_type: PartType,
    ) -> Result<Expect, Halt> {
        let solve = bare("solve");
        let parts = match typed_solution(self.read(at, decl, &solve)?) {
            Typed::Value(parts) => parts,
            Typed::Absent => return Ok(Expect::Absent),
            Typed::Failure(_) => {
                return Ok(Expect::Failure(Failure::BlockedBy {
                    at: solve.to_core(),
                }));
            }
        };
        let index = match typed_int(self.read_binding(at, decl, "part")?) {
            Typed::Value(index) => index,
            Typed::Absent => return Ok(Expect::Absent),
            Typed::Failure(failure) => return Ok(Expect::Failure(failure)),
        };
        let Some(part) = usize::try_from(index).ok().and_then(|at| parts.get(at)) else {
            return Ok(Expect::Absent);
        };
        let expected = match part_type {
            PartType::Int => type_name::<i64>(),
            PartType::Flag => type_name::<bool>(),
        };
        Ok(match (part_type, part_out(*part)) {
            (PartType::Int, value @ Out::Int(_)) | (PartType::Flag, value @ Out::Bool(_)) => {
                Expect::Value(value)
            }
            (_, found) => Expect::Failure(Failure::WrongType {
                expected,
                found: found.type_name(),
            }),
        })
    }

    /// Enters the child a run declared `decl` at `path` in the memo of
    /// declarations, as a path resolved: a node, or the depth failure of a
    /// node too deep.
    fn declare_child(&mut self, path: &ModelPath, decl: Decl) {
        let declared = match self.too_deep(path) {
            Some(failure) => Declared::Settled(Expect::Failure(failure)),
            None => Declared::Node(Rc::new(decl)),
        };
        self.declared.insert(path.clone(), declared);
    }

    /// The answer of the child its call declared at `path`: taken over if
    /// the path is resolving, otherwise shared as any path's.
    fn child_answer(&mut self, path: &ModelPath) -> Result<Rc<Expected>, Halt> {
        match self.resolving.get(path) {
            Some(&began) if !self.answers.contains_key(path) => self.take_over(path, began),
            _ => self.shared(path),
        }
    }

    /// Works out the child at `path`, whose resolution began at height
    /// `began` and waits on its call, in the call's frame. On a ring the
    /// child answers "cycle": a ring it closes at itself ends here; one
    /// through its call halts on; one through a frame outside the
    /// resolution is left for the child's own frame to finish, once its
    /// call has read the cycle.
    fn take_over(&mut self, path: &ModelPath, began: usize) -> Result<Rc<Expected>, Halt> {
        let declared = self
            .declared
            .get(path)
            .cloned()
            .expect("a child taken over was declared by its call's run");
        let framed = self.in_progress.contains_key(path);
        if !framed {
            self.in_progress
                .insert(path.clone(), self.in_progress.len());
        }
        let worked = self.run(path, &declared);
        if !framed {
            self.in_progress.remove(path);
        }
        let answer = match worked {
            Ok(answer) => Ok(Rc::new(answer)),
            Err(Halt::Invalid(invalid)) => Err(invalid),
            Err(Halt::Reopened) => return Err(self.reopened(path)),
            Err(Halt::Ring { entered }) => {
                let on_ring = self.on_ring(path);
                if entered == *path {
                    return Ok(on_ring);
                }
                let outside = self.in_progress.get(&entered);
                if outside.is_none_or(|&height| height < began) {
                    self.deferred.insert(path.clone(), entered);
                    return Ok(on_ring);
                }
                return Err(Halt::Ring { entered });
            }
        };
        self.answers.insert(path.clone(), answer.clone());
        answer.map_err(Halt::from)
    }
}

/// A ref to the binding `name`, with no steps below it.
fn bare(name: &str) -> ModelRef {
    ModelRef {
        binding: name.to_owned(),
        steps: Vec::new(),
    }
}

/// The step and the argument of each child `site` declares, for the call
/// at `at` whose terms are `terms` and whose sum is `sum`.
fn site_children(
    site: &Site,
    terms: &[i64],
    sum: i64,
    at: &ModelPath,
) -> Result<Vec<(Step, i64)>, Invalid> {
    if !site.each {
        let argument = add(sum, site.offset, at)?;
        return Ok(vec![(call_step(&site.sites, None), argument)]);
    }
    Ok((0_u64..)
        .zip(terms)
        .map(|(index, term)| (call_step(&site.sites, Some(index)), *term))
        .collect())
}

/// `site`'s callee with `argument` bound at the site's argument, as a
/// constant.
fn given(site: &Site, argument: i64) -> Decl {
    let mut callee = site.callee.clone();
    let constant = Bind::Constant(Val::Int(argument));
    callee.bindings.insert(site.argument.clone(), constant);
    callee
}

/// The declaration a node of `kind` whose value is `out` gives the child
/// at `step`.
fn declares(kind: ToyKind, out: &Out, step: &Step) -> Option<Decl> {
    match (kind, out) {
        (ToyKind::Scope | ToyKind::Guard, Out::Members(members)) => members.get(step).cloned(),
        (ToyKind::Collection, Out::Elements { count, element }) => {
            match (step.space(), step.key()) {
                (Space::Default, StepKey::Index(index)) if index < count => Some(element.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn add(sum: i64, value: i64, at: &ModelPath) -> Result<i64, Invalid> {
    sum.checked_add(value)
        .ok_or_else(|| Invalid::Overflow { path: at.clone() })
}

/// The snapshot's setting of `limit`, if any.
///
/// # Panics
///
/// If the setting is not a bound, as the core refuses it.
fn setting(inputs: &Inputs, limit: Limit) -> Option<u64> {
    match inputs.get(&Key::Limit(limit))? {
        Val::Bound(bound) => Some(*bound),
        other => panic!("the limit setting {limit:?} must be a bound, and is {other:?}"),
    }
}

mod tests {
    use std::any::type_name;

    use reify_eval_core::{Declaration, Failure, Kind, Ref};

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
        ]);
        let design = scope_r(held, &[("x", 4), ("y", 1)]);
        assert_eq!(answer(&design, &at("r", &["b"])), int_of(ToyKind::Cell, 1));
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

    fn failure(failure: Failure) -> Result<Expected, Invalid> {
        Ok(Expected {
            node: Some(ToyKind::Cell),
            outcome: Expect::Failure(failure),
        })
    }

    fn blocked_at(binding: &str) -> Result<Expected, Invalid> {
        failure(Failure::BlockedBy {
            at: Ref::binding(binding),
        })
    }

    #[test]
    fn a_dividing_cell_truncates_toward_zero() {
        for (sum, quotient) in [(7, 3), (-7, -3)] {
            let design = inputs([
                (
                    root("q"),
                    declaration(divided(cell([input("a")]), input("d"))),
                ),
                (named("a"), int(sum)),
                (named("d"), int(2)),
            ]);
            assert_eq!(
                answer(&design, &at("q", &[])),
                int_of(ToyKind::Cell, quotient)
            );
        }
    }

    #[test]
    fn a_cell_dividing_by_zero_fails_in_the_cell_s_own_terms() {
        let design = inputs([
            (
                root("q"),
                declaration(divided(cell([input("a")]), input("d"))),
            ),
            (named("a"), int(7)),
            (named("d"), int(0)),
        ]);
        let by_zero = Failure::OfKind {
            kind: toy::Cell::ID,
            code: toy::DIVISION_BY_ZERO,
        };
        assert_eq!(answer(&design, &at("q", &[])), failure(by_zero));
    }

    #[test]
    fn a_cell_reading_a_failed_node_is_blocked_by_the_ref_it_read() {
        let held = members([
            (step("z"), divided(cell([input("a")]), input("zero"))),
            (step("r"), cell([input("a"), declarer([step("z")])])),
            (
                step("s"),
                divided(cell([input("a")]), declarer([step("z")])),
            ),
            (step("t"), cell([declarer([step("r")])])),
        ]);
        let design = scope_r(held, &[("a", 7), ("zero", 0)]);
        assert_eq!(answer(&design, &at("r", &["r"])), blocked_at("1"));
        assert_eq!(answer(&design, &at("r", &["s"])), blocked_at("divisor"));
        assert_eq!(answer(&design, &at("r", &["t"])), blocked_at("0"));
    }

    /// Root scope `r` whose members are a shape "s" over the side "side",
    /// a cell "z" dividing by zero and `others`.
    fn kernel_r(others: impl IntoIterator<Item = (Step, Decl)>, side: Val) -> Inputs {
        let held = [
            (step("s"), shape(input("side"))),
            (step("z"), divided(cell([input("a")]), input("zero"))),
        ];
        let mut design = scope_r(
            members(held.into_iter().chain(others)),
            &[("a", 7), ("zero", 0)],
        );
        design.insert(named("side"), side);
        design.insert(named("on"), flag(true));
        design
    }

    #[test]
    fn a_shape_is_a_box_of_its_side_cubed_passing_absent_and_a_failure_through() {
        let held = [
            (step("n"), shape(input("missing"))),
            (step("w"), shape(input("on"))),
            (step("f"), shape(declarer([step("z")]))),
        ];
        let design = kernel_r(held, int(3));
        assert_eq!(
            answer(&design, &at("r", &["s"])),
            value(ToyKind::Shape, Out::Shape { volume: 27 })
        );
        assert_eq!(
            answer(&kernel_r([], int(-2)), &at("r", &["s"])),
            value(ToyKind::Shape, Out::Shape { volume: -8 })
        );
        assert_eq!(
            answer(&design, &at("r", &["n"])),
            absent(Some(ToyKind::Shape))
        );
        let (int, bool) = (type_name::<i64>(), type_name::<bool>());
        assert_eq!(
            answer(&design, &at("r", &["w"])),
            failed(Some(ToyKind::Shape), int, bool)
        );
        let by_zero = Failure::OfKind {
            kind: toy::Cell::ID,
            code: toy::DIVISION_BY_ZERO,
        };
        assert_eq!(
            answer(&design, &at("r", &["f"])),
            Ok(Expected {
                node: Some(ToyKind::Shape),
                outcome: Expect::Failure(by_zero),
            })
        );
    }

    #[test]
    fn a_measure_of_a_sibling_shape_is_its_volume_and_a_cell_reads_it_as_an_int() {
        let held = [
            (step("m"), measure(declarer([step("s")]))),
            (step("c"), cell([declarer([step("m")]), input("a")])),
            (step("n"), measure(declarer([step("none")]))),
        ];
        let design = kernel_r(held, int(2));
        assert_eq!(
            answer(&design, &at("r", &["m"])),
            int_of(ToyKind::Measure, 8)
        );
        assert_eq!(answer(&design, &at("r", &["c"])), int_of(ToyKind::Cell, 15));
        assert_eq!(
            answer(&design, &at("r", &["n"])),
            absent(Some(ToyKind::Measure))
        );
    }

    #[test]
    fn a_measure_of_what_is_not_a_shape_fails_naming_both_types() {
        let held = [
            (step("i"), measure(input("side"))),
            (step("f"), measure(declarer([step("z")]))),
            (step("k"), cell([declarer([step("s")])])),
        ];
        let design = kernel_r(held, int(2));
        let (int, shape) = (type_name::<i64>(), type_name::<toy::BoxShape>());
        assert_eq!(
            answer(&design, &at("r", &["i"])),
            failed(Some(ToyKind::Measure), shape, int)
        );
        let by_zero = Failure::OfKind {
            kind: toy::Cell::ID,
            code: toy::DIVISION_BY_ZERO,
        };
        assert_eq!(
            answer(&design, &at("r", &["f"])),
            Ok(Expected {
                node: Some(ToyKind::Measure),
                outcome: Expect::Failure(by_zero),
            })
        );
        assert_eq!(
            answer(&design, &at("r", &["k"])),
            failed(Some(ToyKind::Cell), int, shape)
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

    /// The answer of a node on a ring of waits, declared a `node`, if any.
    fn cycle(node: Option<ToyKind>) -> Result<Expected, Invalid> {
        Ok(Expected {
            node,
            outcome: Expect::Failure(Failure::Cycle),
        })
    }

    #[test]
    fn a_cell_whose_term_is_below_its_own_path_is_a_cycle() {
        let design = scope_r(
            members([(step("a"), cell([declarer([step("a"), step("x")])]))]),
            &[],
        );
        assert_eq!(
            answer(&design, &at("r", &["a"])),
            cycle(Some(ToyKind::Cell))
        );
        assert_eq!(answer(&design, &at("r", &["a", "x"])), cycle(None));
    }

    /// Root scope `r` with the cells a, over its sibling b, and b, over a,
    /// and `more` beside them.
    fn two_cell_ring(more: impl IntoIterator<Item = (Step, Decl)>) -> Inputs {
        let over = |other: &str| cell([declarer([step(other)])]);
        let mut held = vec![(step("a"), over("b")), (step("b"), over("a"))];
        held.extend(more);
        scope_r(members(held), &[])
    }

    #[test]
    fn a_two_cell_ring_answers_cycle_whichever_member_is_asked_first() {
        let design = two_cell_ring([]);
        for order in [["a", "b"], ["b", "a"]] {
            let mut reference = Reference::new(design.clone());
            for name in order {
                assert_eq!(
                    reference.answer(&at("r", &[name])),
                    cycle(Some(ToyKind::Cell)),
                    "{name} in order {order:?}"
                );
            }
        }
    }

    #[test]
    fn a_reader_of_a_ring_is_blocked_by_it() {
        let design = two_cell_ring([(step("c"), cell([declarer([step("a")])]))]);
        for order in [["c", "a"], ["a", "c"]] {
            let mut reference = Reference::new(design.clone());
            for name in order {
                let expected = match name {
                    "c" => blocked_at("0"),
                    _ => cycle(Some(ToyKind::Cell)),
                };
                let found = reference.answer(&at("r", &[name]));
                assert_eq!(found, expected, "{name} in order {order:?}");
            }
        }
    }

    #[test]
    fn a_ring_through_a_collection_s_element_answers_cycle_on_each_path_of_it() {
        let held = members([
            (step("m0"), cell([declarer([step("m1"), index(0)])])),
            (
                step("m1"),
                collection(declarer([step("m0")]), input("element")),
            ),
        ]);
        let mut design = scope_r(held, &[]);
        design.insert(named("element"), declaration(cell([])));
        assert_eq!(
            answer(&design, &at("r", &["m0"])),
            cycle(Some(ToyKind::Cell))
        );
        assert_eq!(
            answer(&design, &at("r", &["m1"])),
            cycle(Some(ToyKind::Collection))
        );
        let element = at("r", &["m1"]).child(index(0));
        assert_eq!(answer(&design, &element), cycle(None));
    }

    #[test]
    fn first_invalid_finds_an_overflow_an_edit_makes_at_a_demanded_path() {
        let a_over = |terms: Vec<Bind>| members([(step("a"), cell(terms))]);
        let ints = [("x", 1), ("big", i64::MAX)];
        let mut case = Case {
            inputs: scope_r(a_over(vec![input("x")]), &ints),
            events: vec![HostEvent::Evaluate {
                on: 0,
                paths: vec![at("r", &["a"])],
                budget: None,
                then: Then::End,
            }],
        };
        assert_eq!(first_invalid(&case), None);
        let edit = |held| HostEvent::Edit {
            from: 0,
            changes: vec![(named("r.m"), Some(held))],
        };
        let below_itself = a_over(vec![declarer([step("a"), step("x")])]);
        case.events.push(edit(below_itself));
        assert_eq!(first_invalid(&case), None, "a cycle is an answer");
        let past_i64 = a_over(vec![input("big"), input("big")]);
        case.events.push(edit(past_i64));
        assert_eq!(
            first_invalid(&case),
            Some(Invalid::Overflow {
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

    /// Root scope `r` whose member `inner` is a scope over "inner.m"
    /// holding the cell c over "x", beside the cell `reader` over inner/c;
    /// under the depth setting `depth`, if any.
    fn nested_r(depth: Option<u64>) -> Inputs {
        let mut design = scope_r(
            members([
                (step("inner"), scope("inner.m")),
                (step("reader"), cell([declarer([step("inner"), step("c")])])),
            ]),
            &[("x", 4)],
        );
        design.insert(named("inner.m"), members([(step("c"), cell([input("x")]))]));
        design.extend(depth.map(|depth| (depth_limit(), bound(depth))));
        design
    }

    fn settled(failure: Failure) -> Result<Expected, Invalid> {
        Ok(Expected {
            node: None,
            outcome: Expect::Failure(failure),
        })
    }

    #[test]
    fn under_a_depth_setting_a_member_answers_and_a_member_of_its_scope_is_too_deep() {
        let design = nested_r(Some(1));
        let inner = answer(&design, &at("r", &["inner"])).map(|expected| expected.outcome);
        assert!(
            matches!(inner, Ok(Expect::Value(Out::Members(_)))),
            "{inner:?}"
        );
        assert_eq!(
            answer(&design, &at("r", &["inner", "c"])),
            settled(Failure::TooDeep { limit: 1 })
        );
    }

    #[test]
    fn a_path_past_the_depth_setting_that_nothing_declares_is_absent() {
        let design = nested_r(Some(1));
        assert_eq!(
            answer(&design, &at("r", &["inner", "missing"])),
            absent(None)
        );
    }

    #[test]
    fn a_path_below_a_too_deep_node_answers_its_failure() {
        let design = nested_r(Some(1));
        assert_eq!(
            answer(&design, &at("r", &["inner", "c", "x"])),
            settled(Failure::TooDeep { limit: 1 })
        );
    }

    #[test]
    fn a_cell_reading_a_too_deep_member_is_blocked_by_it() {
        let design = nested_r(Some(1));
        assert_eq!(answer(&design, &at("r", &["reader"])), blocked_at("0"));
    }

    #[test]
    fn with_no_limit_setting_nothing_is_bounded() {
        let design = nested_r(None);
        assert_eq!(
            answer(&design, &at("r", &["inner", "c"])),
            int_of(ToyKind::Cell, 4)
        );
    }

    #[test]
    fn a_children_setting_fails_a_scope_holding_more_members_and_each_member() {
        let held = members([
            (step("a"), cell([input("x")])),
            (step("b"), cell([input("x")])),
        ]);
        let mut design = scope_r(held, &[("x", 4)]);
        design.insert(children_limit(), bound(1));
        let too_many = Failure::TooManyChildren { limit: 1 };
        assert_eq!(
            answer(&design, &at("r", &[])),
            Ok(Expected {
                node: Some(ToyKind::Scope),
                outcome: Expect::Failure(too_many.clone()),
            })
        );
        assert_eq!(answer(&design, &at("r", &["a"])), settled(too_many));
    }

    #[test]
    fn a_children_setting_fails_a_collection_past_it_and_answers_one_at_it() {
        for (count, fails) in [(2, true), (1, false)] {
            let design = inputs([
                (
                    root("list"),
                    declaration(collection(input("n"), input("e"))),
                ),
                (named("n"), int(count)),
                (named("e"), declaration(cell([input("x")]))),
                (children_limit(), bound(1)),
            ]);
            let found = answer(&design, &at("list", &[])).map(|expected| expected.outcome);
            if fails {
                assert_eq!(
                    found,
                    Ok(Expect::Failure(Failure::TooManyChildren { limit: 1 }))
                );
            } else {
                assert!(
                    matches!(found, Ok(Expect::Value(Out::Elements { count: 1, .. }))),
                    "{found:?}"
                );
            }
        }
    }

    /// How many paths a fresh reference works out the declaration of to
    /// answer `at`.
    fn paths_resolved_for(design: &Inputs, at: &ModelPath) -> usize {
        let mut reference = Reference::new(design.clone());
        reference.answer(at).expect("the design is valid");
        reference.paths_resolved()
    }

    #[test]
    fn the_paths_resolved_are_the_ancestors_and_the_read_targets_of_a_demand() {
        let held = members([
            (step("a"), cell([input("x")])),
            (step("b"), cell([declarer([step("a")])])),
        ]);
        let design = scope_r(held, &[("x", 4)]);
        assert_eq!(paths_resolved_for(&design, &at("r", &["b"])), 3);
    }

    #[test]
    fn a_path_waiting_on_a_ring_is_answered_without_being_resolved() {
        let design = scope_r(
            members([(step("c"), cell([declarer([step("c"), step("x")])]))]),
            &[],
        );
        assert_eq!(paths_resolved_for(&design, &at("r", &["c"])), 2);
    }

    #[test]
    fn a_path_off_a_ring_below_it_is_resolved_to_the_ring_s_cycle() {
        let design = two_cell_ring([]);
        assert_eq!(paths_resolved_for(&design, &at("r", &["a", "x"])), 4);
    }

    #[test]
    fn a_read_through_a_constant_answers_its_value_and_a_step_below_it_absent() {
        let design = inputs([
            (root("c"), declaration(cell([constant(int(5)), input("a")]))),
            (
                root("p"),
                declaration(picker(input("zero"), constant(int(9)))),
            ),
            (named("a"), int(2)),
            (named("zero"), int(0)),
        ]);
        assert_eq!(answer(&design, &at("c", &[])), int_of(ToyKind::Cell, 7));
        assert_eq!(answer(&design, &at("p", &[])), absent(Some(ToyKind::Cell)));
    }

    /// Root scope `r` whose member p is `call`, beside the input "calls"
    /// holding `calls` and the named ints `ints`.
    fn call_r(call: Decl, calls: Val, ints: &[(&str, i64)]) -> Inputs {
        let mut design = scope_r(members([(step("p"), call)]), ints);
        design.insert(named("calls"), calls);
        design
    }

    /// A call over the inputs "a" and "b" whose sites are at "calls".
    fn over_a_and_b() -> Decl {
        call([input("a"), input("b")], input("calls"))
    }

    fn p_child(step: Step) -> ModelPath {
        at("r", &["p"]).child(step)
    }

    fn of_call(outcome: Expect) -> Result<Expected, Invalid> {
        Ok(Expected {
            node: Some(ToyKind::Call),
            outcome,
        })
    }

    #[test]
    fn a_call_answers_its_sum_plus_its_child_and_the_child_s_path_answers_the_child() {
        let calls = calls_value([site(&[1], false, 1, cell([]))]);
        let design = call_r(over_a_and_b(), calls, &[("a", 2), ("b", 3)]);
        let child = p_child(call_step(&[1], None));
        assert_eq!(answer(&design, &at("r", &["p"])), int_of(ToyKind::Call, 11));
        assert_eq!(answer(&design, &child), int_of(ToyKind::Cell, 6));
    }

    #[test]
    fn a_looping_site_s_children_answer_at_their_iteration_paths_and_one_past_is_absent() {
        let plus_a = decl(ToyKind::Cell, [("1", input("a"))]);
        let calls = calls_value([site(&[1], true, 0, plus_a)]);
        let design = call_r(over_a_and_b(), calls, &[("a", 2), ("b", 3)]);
        let child = |index| p_child(call_step(&[1], Some(index)));
        assert_eq!(answer(&design, &at("r", &["p"])), int_of(ToyKind::Call, 14));
        assert_eq!(answer(&design, &child(0)), int_of(ToyKind::Cell, 4));
        assert_eq!(answer(&design, &child(1)), int_of(ToyKind::Cell, 5));
        assert_eq!(answer(&design, &child(2)), absent(None));
    }

    #[test]
    fn a_false_when_declares_nothing_and_the_child_s_path_is_absent() {
        let calls = calls_value([site(&[1], false, 1, cell([]))]);
        let mut when = over_a_and_b();
        when.bindings.insert(String::from("when"), input("off"));
        let mut design = call_r(when, calls, &[("a", 2), ("b", 3)]);
        design.insert(named("off"), flag(false));
        assert_eq!(answer(&design, &at("r", &["p"])), int_of(ToyKind::Call, 5));
        assert_eq!(
            answer(&design, &p_child(call_step(&[1], None))),
            absent(None)
        );
    }

    #[test]
    fn a_child_path_asked_before_its_parent_answers_the_child_and_is_resolved_once() {
        let calls = calls_value([site(&[1], false, 1, cell([]))]);
        let design = call_r(over_a_and_b(), calls, &[("a", 2), ("b", 3)]);
        let child = p_child(call_step(&[1], None));
        let mut reference = Reference::new(design);
        assert_eq!(reference.answer(&child), int_of(ToyKind::Cell, 6));
        assert_eq!(reference.paths_resolved(), 3, "r, r/p and the child");
        assert_eq!(
            reference.answer(&at("r", &["p"])),
            int_of(ToyKind::Call, 11)
        );
    }

    /// A site [1] declaring `callee`, its argument bound at "given", which
    /// the cell reads nowhere.
    fn given(callee: Decl) -> Site {
        let mut given = site(&[1], false, 0, callee);
        given.argument = String::from("given");
        given
    }

    #[test]
    fn a_child_reading_its_declarer_answers_cycle_and_so_does_the_declarer() {
        let calls = calls_value([given(cell([declarer([])]))]);
        let design = call_r(over_a_and_b(), calls, &[("a", 2), ("b", 3)]);
        let (p, child) = (at("r", &["p"]), p_child(call_step(&[1], None)));
        for first in [&p, &child] {
            let mut reference = Reference::new(design.clone());
            reference.answer(first).expect("the design is valid");
            assert_eq!(
                reference.answer(&p),
                cycle(Some(ToyKind::Call)),
                "{first:?}"
            );
            assert_eq!(
                reference.answer(&child),
                cycle(Some(ToyKind::Cell)),
                "{first:?}"
            );
        }
    }

    #[test]
    fn a_ring_through_a_child_and_its_declarer_s_sibling_leaves_the_declarer_off_it() {
        let s = call_step(&[1], None);
        let mut p = call([], input("calls"));
        p.bindings.insert(String::from("x"), declarer([step("x")]));
        let held = members([
            (step("x"), cell([declarer([step("p"), s.clone()])])),
            (step("p"), p),
        ]);
        let mut design = scope_r(held, &[]);
        design.insert(
            named("calls"),
            calls_value([given(cell([passed("x", [])]))]),
        );
        let (x, p, child) = (at("r", &["x"]), at("r", &["p"]), p_child(s.clone()));
        for first in [&x, &p, &child] {
            let mut reference = Reference::new(design.clone());
            reference.answer(first).expect("the design is valid");
            assert_eq!(
                reference.answer(&x),
                cycle(Some(ToyKind::Cell)),
                "{first:?}"
            );
            assert_eq!(
                reference.answer(&child),
                cycle(Some(ToyKind::Cell)),
                "{first:?}"
            );
            assert_eq!(
                reference.answer(&p),
                of_call(Expect::Failure(toy::blocked_by_child(s.clone()))),
                "{first:?}"
            );
        }
    }

    #[test]
    fn a_child_past_the_depth_setting_is_too_deep_and_its_call_blocked_by_it() {
        let s = call_step(&[1], None);
        let calls = calls_value([site(&[1], false, 1, cell([]))]);
        let mut design = call_r(over_a_and_b(), calls, &[("a", 2), ("b", 3)]);
        design.insert(depth_limit(), bound(1));
        assert_eq!(
            answer(&design, &p_child(s.clone())),
            settled(Failure::TooDeep { limit: 1 })
        );
        assert_eq!(
            answer(&design, &at("r", &["p"])),
            of_call(Expect::Failure(toy::blocked_by_child(s)))
        );
    }

    #[test]
    fn two_declarations_at_one_step_in_one_run_fail_the_call() {
        let s = call_step(&[1], None);
        let calls = calls_value([
            site(&[1], false, 0, cell([])),
            site(&[1], false, 1, cell([])),
        ]);
        let design = call_r(over_a_and_b(), calls, &[("a", 2), ("b", 3)]);
        assert_eq!(
            answer(&design, &at("r", &["p"])),
            of_call(Expect::Failure(Failure::ConflictingChild {
                step: s.clone()
            }))
        );
        assert_eq!(answer(&design, &p_child(s)), int_of(ToyKind::Cell, 5));
    }

    #[test]
    fn a_call_declaring_more_children_than_the_setting_fails_and_its_children_answer() {
        let calls = calls_value([site(&[1], true, 0, cell([]))]);
        let looping = call([input("a"), input("b"), input("c")], input("calls"));
        let mut design = call_r(looping, calls, &[("a", 2), ("b", 3), ("c", 4)]);
        design.insert(children_limit(), bound(2));
        assert_eq!(
            answer(&design, &at("r", &["p"])),
            of_call(Expect::Failure(Failure::TooManyChildren { limit: 2 }))
        );
        for (index, value) in [(0, 2), (1, 3), (2, 4)] {
            let child = p_child(call_step(&[1], Some(index)));
            assert_eq!(answer(&design, &child), int_of(ToyKind::Cell, value));
        }
        design.insert(children_limit(), bound(3));
        assert_eq!(answer(&design, &at("r", &["p"])), int_of(ToyKind::Call, 18));
    }

    /// Root scope `r` whose members are `held`, beside the input "problem"
    /// holding `problem` and the named ints `ints`.
    fn solve_r(held: Val, problem: Val, ints: &[(&str, i64)]) -> Inputs {
        let mut design = scope_r(held, ints);
        design.insert(named("problem"), problem);
        design
    }

    /// A solve of the problem at "problem" whose unknowns and constraints
    /// are the siblings named.
    fn solve_over(unknowns: &[&str], constraints: &[&str]) -> Decl {
        let sibling = |name: &&str| declarer([step(name)]);
        solve(
            input("problem"),
            unknowns.iter().map(sibling),
            constraints.iter().map(sibling),
        )
    }

    /// The sibling solve s.
    fn sibling_s() -> Bind {
        declarer([step("s")])
    }

    /// Root scope `r` holding the solve s of the int unknown n, whose
    /// constraint k is n - 3, over the candidates n = 1, 4 and 7, beside
    /// the parts `wrong`, part 0 read as a flag, and `past`, part 1.
    fn first_holding_r() -> Inputs {
        let held = members([
            (step("s"), solve_over(&["n"], &["k"])),
            (step("n"), solved_int(sibling_s(), 0)),
            (step("k"), cell([declarer([step("n")]), input("minus")])),
            (step("wrong"), solved_flag(sibling_s(), 0)),
            (step("past"), solved_int(sibling_s(), 1)),
        ]);
        let candidates = [1, 4, 7].map(|n| vec![Part::Int(n)]);
        solve_r(held, problem(candidates, 1), &[("minus", -3)])
    }

    const SOLVED_INT: ToyKind = ToyKind::Solved(PartType::Int);

    #[test]
    fn a_solve_answers_its_first_holding_candidate_and_each_solved_part_reads_it() {
        let design = first_holding_r();
        assert_eq!(
            answer(&design, &at("r", &["s"])),
            value(ToyKind::Solve, Out::Solution(vec![Part::Int(4)]))
        );
        assert_eq!(answer(&design, &at("r", &["n"])), int_of(SOLVED_INT, 4));
        assert_eq!(answer(&design, &at("r", &["k"])), int_of(ToyKind::Cell, 1));
        assert_eq!(
            answer(&design, &at("r", &["wrong"])),
            failed(
                Some(ToyKind::Solved(PartType::Flag)),
                type_name::<bool>(),
                type_name::<i64>()
            )
        );
        assert_eq!(
            answer(&design, &at("r", &["past"])),
            absent(Some(SOLVED_INT))
        );
    }

    /// What `trial` answered at `path`, if it answered there.
    fn in_trial(trial: &Trial, path: &ModelPath) -> Option<Expected> {
        trial
            .answers()
            .find(|(at, _)| *at == path)
            .map(|(_, expected)| expected.clone())
    }

    #[test]
    fn a_trial_sees_its_substitution_and_structure_follows_it() {
        let m = [step("g"), step("m")];
        let unknowns = [
            declarer([step("n")]),
            declarer([step("f")]),
            declarer(m.clone()),
        ];
        let constraints = [
            declarer(m.clone()),
            declarer([step("col"), index(2)]),
            declarer([step("k")]),
        ];
        let held = members([
            (step("s"), solve(input("problem"), unknowns, constraints)),
            (step("n"), solved_int(sibling_s(), 0)),
            (step("f"), solved_flag(sibling_s(), 1)),
            (step("g"), guard(declarer([step("f")]), "g.m")),
            (step("col"), collection(declarer([step("n")]), input("el"))),
            (step("k"), cell([declarer([step("n")]), input("minus")])),
        ]);
        let candidates =
            [(2, false), (3, true)].map(|(n, f)| vec![Part::Int(n), Part::Flag(f), Part::Int(9)]);
        let mut design = solve_r(held, problem(candidates.clone(), 3), &[("minus", -3)]);
        design.insert(named("g.m"), members([(step("m"), cell([input("minus")]))]));
        design.insert(named("el"), declaration(cell([])));
        let s = at("r", &["s"]);
        let mut reference = Reference::new(design);
        assert_eq!(
            reference.answer(&s),
            value(ToyKind::Solve, Out::Solution(candidates[1].clone()))
        );
        let [first, second] = reference.trials(&s) else {
            panic!("the first candidate fails its last constraint, the second holds")
        };
        let (g, col) = (at("r", &["g"]), at("r", &["col"]));
        let (m, element) = (g.child(step("m")), col.child(index(2)));
        assert_eq!(in_trial(first, &g), absent(Some(ToyKind::Guard)).ok());
        assert_eq!(
            in_trial(first, &m),
            absent(None).ok(),
            "under an absent guard"
        );
        let present = in_trial(second, &g).map(|expected| expected.outcome);
        assert!(
            matches!(present, Some(Expect::Value(Out::Members(_)))),
            "{present:?}"
        );
        assert_eq!(in_trial(second, &m), int_of(ToyKind::Cell, 9).ok());
        let count = |trial| match in_trial(trial, &col).map(|expected| expected.outcome) {
            Some(Expect::Value(Out::Elements { count, .. })) => count,
            other => panic!("the collection answered {other:?}"),
        };
        assert_eq!((count(first), count(second)), (2, 3));
        assert_eq!(in_trial(first, &element), absent(None).ok());
        assert_eq!(in_trial(second, &element), int_of(ToyKind::Cell, 0).ok());
    }

    #[test]
    fn a_constraint_reading_its_own_solve_makes_the_solve_a_cycle_and_is_blocked_by_it() {
        let held = members([
            (step("s"), solve_over(&["n"], &["k"])),
            (step("n"), solved_int(sibling_s(), 0)),
            (step("k"), cell([sibling_s()])),
        ]);
        let design = solve_r(held, problem([vec![Part::Int(1)]], 1), &[]);
        for first in ["s", "k"] {
            let mut reference = Reference::new(design.clone());
            reference
                .answer(&at("r", &[first]))
                .expect("the design is valid");
            assert_eq!(
                reference.answer(&at("r", &["s"])),
                cycle(Some(ToyKind::Solve)),
                "{first} first"
            );
            assert_eq!(
                reference.answer(&at("r", &["k"])),
                blocked_at("0"),
                "{first} first"
            );
        }
    }

    #[test]
    fn a_solve_s_own_path_in_a_trial_follows_the_structure_its_unknowns_give() {
        let mut gate = guard(declarer([step("f")]), "g.m");
        gate.bindings
            .insert(String::from("c"), declarer([step("c")]));
        let held = members([
            (step("t"), solve(input("pt"), [], [])),
            (step("f"), solved_flag(declarer([step("t")]), 0)),
            (step("g"), gate),
            (step("c"), cell([declarer([step("g"), step("s")])])),
        ]);
        let s = solve(
            input("problem"),
            [passed("condition", [])],
            [passed("c", [])],
        );
        let mut design = solve_r(held, problem([vec![Part::Flag(false)]], 1), &[]);
        design.insert(named("pt"), problem([vec![Part::Flag(true)]], 0));
        design.insert(named("g.m"), members([(step("s"), s)]));
        let s = at("r", &["g", "s"]);
        let mut reference = Reference::new(design);
        assert_eq!(
            reference.answer(&s),
            value(ToyKind::Solve, Out::Solution(vec![Part::Flag(false)])),
            "f, which g reads, is the solve's unknown"
        );
        let [trial] = reference.trials(&s) else {
            panic!("one candidate, one trial")
        };
        let g = at("r", &["g"]);
        assert_eq!(in_trial(trial, &g), absent(Some(ToyKind::Guard)).ok());
        assert_eq!(
            in_trial(trial, &s),
            absent(None).ok(),
            "the solve's own path is under the trial's absent guard"
        );
    }

    /// A solve's one constraint reads its call, or its own path below the
    /// call, so that its trial resolves the path only once the call's run
    /// declares it, or first, the call taking it over.
    #[test]
    fn a_call_in_a_trial_declaring_the_trial_s_solve_again_reaches_the_solve() {
        let site_step = call_step(&[1], None);
        for (constraint, reads) in [
            (declarer([]), "the call"),
            (declarer([site_step.clone()]), "itself"),
        ] {
            let solve = solve(input("problem"), [passed("0", [])], [constraint]);
            let held = members([
                (step("x"), cell([input("x0")])),
                (step("call"), call([declarer([step("x")])], input("calls"))),
            ]);
            let mut design = solve_r(held, problem([vec![Part::Int(2)]], 1), &[("x0", 1)]);
            design.insert(named("calls"), calls_value([site(&[1], false, 0, solve)]));
            let s = at("r", &["call"]).child(site_step.clone());
            let mut reference = Reference::new(design);
            assert_eq!(
                reference.answer(&s),
                cycle(Some(ToyKind::Solve)),
                "reading {reads}"
            );
            assert!(
                reference
                    .every_trial()
                    .all(|trial| in_trial(trial, &s).is_none()),
                "reading {reads}: the solve's own path in its trial is the solve, outside it"
            );
        }
    }

    #[test]
    fn a_solve_none_of_whose_candidates_holds_fails_and_its_parts_are_blocked_by_it() {
        let no_candidate = Ok(Expected {
            node: Some(ToyKind::Solve),
            outcome: Expect::Failure(Failure::OfKind {
                kind: toy::Solve::ID,
                code: toy::NO_CANDIDATE_HOLDS,
            }),
        });
        let blocked = Ok(Expected {
            node: Some(SOLVED_INT),
            outcome: Expect::Failure(Failure::BlockedBy {
                at: Ref::binding("solve"),
            }),
        });
        let mut design = first_holding_r();
        for candidates in [vec![1, 2], vec![]] {
            let candidates = candidates.into_iter().map(|n| vec![Part::Int(n)]);
            design.insert(named("problem"), problem(candidates, 1));
            assert_eq!(answer(&design, &at("r", &["s"])), no_candidate);
            assert_eq!(answer(&design, &at("r", &["n"])), blocked);
        }
    }

    #[test]
    fn a_substitution_naming_no_place_of_its_own_fails_the_solve_once_a_trial_reads() {
        let n = || declarer([step("n")]);
        let k = || declarer([step("k")]);
        let cases = [
            (vec![input("x")], "u0"),
            (vec![n(), n()], "u1"),
            (vec![n()], "u1"),
        ];
        for (unknowns, at_ref) in cases {
            let held = members([
                (step("s"), solve(input("problem"), unknowns, [k()])),
                (step("n"), solved_int(sibling_s(), 0)),
                (step("k"), cell([n()])),
            ]);
            let candidate = vec![Part::Int(1), Part::Int(2)];
            let mut design = solve_r(held, problem([candidate.clone()], 1), &[("x", 0)]);
            assert_eq!(
                answer(&design, &at("r", &["s"])),
                Ok(Expected {
                    node: Some(ToyKind::Solve),
                    outcome: Expect::Failure(Failure::NotSupposable {
                        at: Ref::binding(at_ref),
                    }),
                }),
                "{at_ref}"
            );
            design.insert(named("problem"), problem([candidate.clone()], 0));
            assert_eq!(
                answer(&design, &at("r", &["s"])),
                value(ToyKind::Solve, Out::Solution(candidate)),
                "with no constraint no trial reads, and no view opens"
            );
        }
    }

    #[test]
    fn a_trial_that_reaches_another_solve_is_invalid_and_first_invalid_finds_it() {
        let held = members([
            (step("s"), solve_over(&["n"], &["k"])),
            (step("n"), solved_int(sibling_s(), 0)),
            (step("t"), solve_over(&["m"], &[])),
            (step("m"), solved_int(declarer([step("t")]), 0)),
            (step("k"), cell([declarer([step("m")])])),
        ]);
        let design = solve_r(held, problem([vec![Part::Int(1)]], 1), &[]);
        let inside = Invalid::ViewInsideView {
            path: at("r", &["t"]),
        };
        assert_eq!(answer(&design, &at("r", &["s"])), Err(inside.clone()));
        let evaluate = |path: ModelPath| Case {
            inputs: design.clone(),
            events: vec![HostEvent::Evaluate {
                on: 0,
                paths: vec![path],
                budget: None,
                then: Then::End,
            }],
        };
        assert_eq!(first_invalid(&evaluate(at("r", &["s"]))), Some(inside));
        assert_eq!(
            first_invalid(&evaluate(at("r", &["k"]))),
            None,
            "t opens its view in the snapshot"
        );
        assert_eq!(answer(&design, &at("r", &["k"])), int_of(ToyKind::Cell, 1));
    }

    #[test]
    fn the_paths_resolved_count_each_trial_s_resolutions_beside_the_snapshot_s() {
        let s = at("r", &["s"]);
        let mut reference = Reference::new(first_holding_r());
        reference.answer(&s).expect("the design is valid");
        let trials = reference.trials(&s);
        assert_eq!(trials.len(), 2, "n = 1 fails, n = 4 holds");
        for trial in trials {
            assert_eq!(trial.paths_resolved(), 3, "r, r/k and the supposed r/n");
        }
        assert_eq!(
            reference.paths_resolved(),
            2 + 3 + 3,
            "r and r/s, then the trials'"
        );
    }
}
