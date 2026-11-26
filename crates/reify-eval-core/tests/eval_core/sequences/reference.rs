//! The reference interpreter: what each path of a snapshot answers, worked
//! out directly from the design model and the meaning of the toy kinds,
//! with no store, index or executor and no code shared with the core or the
//! toy kinds' bodies. It remembers each path's answer within one snapshot,
//! so a design whose nodes read one another many times stays linear, and it
//! reports a design no evaluation can answer (a cycle, or a sum past i64)
//! as invalid rather than as an outcome.

use std::collections::BTreeMap;

use reify_eval_core::{Failure, Step};

use super::design::{Decl, Inputs, ModelPath, ToyKind};

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

/// The answers of one snapshot.
pub struct Reference {
    inputs: Inputs,
}

impl Reference {
    pub fn new(inputs: Inputs) -> Self {
        Self { inputs }
    }

    pub fn answer(&mut self, path: &ModelPath) -> Result<Expected, Invalid> {
        let _ = (&self.inputs, path);
        Ok(Expected {
            node: None,
            outcome: Expect::Absent,
        })
    }

    /// How many distinct paths have been answered.
    pub fn evaluations(&self) -> usize {
        0
    }
}

mod tests {
    use std::any::type_name;

    use reify_eval_core::{Declaration, Failure};

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

    fn failed(node: Option<ToyKind>, expected: &'static str, found: &'static str) -> Result<Expected, Invalid> {
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
        assert_eq!(answer(&design, &at("r", &[])), value(ToyKind::Scope, Out::Members(held)));
    }

    #[test]
    fn a_path_below_a_cell_is_absent() {
        let design = inputs([(root("c"), declaration(cell([input("a")]))), (named("a"), int(2))]);
        assert_eq!(answer(&design, &at("c", &["x"])), absent(None));
    }

    #[test]
    fn a_step_below_a_snapshot_input_is_absent() {
        let held = members([
            (step("a"), cell([input("x")])),
            (step("b"), cell([input("y"), passed("members", [step("a")]), input("y")])),
            (step("c"), cell([input("y"), passed("extra", [step("a")]), input("y")])),
        ]);
        let mut design = scope_r(held.clone(), &[("x", 4), ("y", 1)]);
        design.insert(attached_to(at("r", &[]), "extra"), held);
        assert_eq!(answer(&design, &at("r", &["b"])), int_of(ToyKind::Cell, 1));
        assert_eq!(answer(&design, &at("r", &["c"])), int_of(ToyKind::Cell, 1));
    }

    #[test]
    fn a_declarer_and_a_passed_binding_on_a_root_read_absent() {
        let design = inputs([
            (root("d"), declaration(cell([input("a"), declarer([]), input("b")]))),
            (root("e"), declaration(cell([input("a"), passed("0", []), input("b")]))),
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
        assert_eq!(answer(&design, &at("h", &[])), value(ToyKind::Guard, Out::Members(held)));
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
            (root("col"), declaration(collection(input("n"), input("el")))),
            (root("none"), declaration(collection(input("neg"), input("el")))),
            (named("el"), declaration(element.clone())),
            (named("n"), int(3)),
            (named("neg"), int(-1)),
            (named("x"), int(10)),
        ]);
        let elements = |count| Out::Elements {
            count,
            element: element.clone(),
        };
        assert_eq!(answer(&design, &at("col", &[])), value(ToyKind::Collection, elements(3)));
        let col = |i| path("col", [index(i)]);
        assert_eq!(answer(&design, &col(0)), int_of(ToyKind::Cell, 13));
        assert_eq!(answer(&design, &col(2)), int_of(ToyKind::Cell, 13));
        assert_eq!(answer(&design, &col(3)), absent(None));
        assert_eq!(answer(&design, &at("none", &[])), value(ToyKind::Collection, elements(0)));
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
        let mut design = scope_r(held, &[("n", 3), ("x", 10), ("one", 1), ("five", 5), ("neg", -1)]);
        design.insert(named("el"), declaration(cell([input("x")])));
        assert_eq!(answer(&design, &at("r", &["p"])), int_of(ToyKind::Cell, 10));
        assert_eq!(answer(&design, &at("r", &["q"])), absent(Some(ToyKind::Cell)));
        assert_eq!(answer(&design, &at("r", &["s"])), absent(Some(ToyKind::Cell)));
    }

    #[test]
    fn a_cell_reading_a_scope_fails_naming_both_types() {
        let design = scope_r(members([(step("a"), cell([declarer([])]))]), &[]);
        let (int, scope) = (type_name::<i64>(), type_name::<toy::Members>());
        assert_eq!(answer(&design, &at("r", &["a"])), failed(Some(ToyKind::Cell), int, scope));
    }

    #[test]
    fn a_child_of_a_failed_node_answers_that_failure() {
        let mut design = scope_r(members([(step("a"), cell([declarer([])]))]), &[]);
        design.insert(root("bad"), int(1));
        let (int, scope) = (type_name::<i64>(), type_name::<toy::Members>());
        assert_eq!(answer(&design, &at("r", &["a", "z"])), failed(None, int, scope));
        let declaration = type_name::<Declaration>();
        assert_eq!(answer(&design, &at("bad", &["x"])), failed(None, declaration, int));
    }

    #[test]
    fn an_attached_extra_input_adds_a_member_and_overrides_one_at_the_same_step() {
        let held = members([(step("a"), cell([input("x")])), (step("b"), cell([input("y")]))]);
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
        let design = scope_r(members([(step("a"), cell([declarer([step("a"), step("x")])]))]), &[]);
        assert_eq!(
            answer(&design, &at("r", &["a"])),
            Err(Invalid::Cycle { path: at("r", &["a"]) })
        );
    }

    #[test]
    fn each_path_is_evaluated_once_however_often_it_is_read() {
        let name = |k: usize| format!("c{k}");
        let chain = (0..40).map(|k| {
            let terms = if k == 0 {
                vec![input("unit")]
            } else {
                vec![declarer([step(&name(k - 1))]); 2]
            };
            (step(&name(k)), cell(terms))
        });
        let design = scope_r(members(chain), &[("unit", 3)]);
        let mut reference = Reference::new(design);
        let last = reference.answer(&at("r", &["c39"]));
        assert_eq!(last, int_of(ToyKind::Cell, 3 << 39));
        assert_eq!(reference.evaluations(), 41, "the root and its forty members");
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
