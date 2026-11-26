//! The order a structure ctor evaluates its members in.

use std::collections::BTreeSet;

use reify_core::ValueCellId;
use reify_ir::{CompiledExpr, CtorGeometryLet};

/// One member a structure ctor evaluates after its ordered args.
#[derive(Clone, Copy)]
pub(crate) enum CtorMember<'e> {
    /// An uncovered default or a let: becomes an instance field.
    Field(&'e (String, CompiledExpr)),
    /// A template geometry let: a symbolic handle in member scope only.
    Geometry(&'e CtorGeometryLet),
}

impl<'e> CtorMember<'e> {
    pub(crate) fn name(self) -> &'e str {
        match self {
            CtorMember::Field((name, _)) => name,
            CtorMember::Geometry(geometry) => &geometry.member,
        }
    }

    /// Every expression the member evaluates: a field's own expr, or a
    /// geometry member's upstream args.
    fn exprs(self) -> impl Iterator<Item = &'e CompiledExpr> {
        let (field, upstream): (Option<&'e CompiledExpr>, &'e [(String, CompiledExpr)]) =
            match self {
                CtorMember::Field((_, expr)) => (Some(expr), &[]),
                CtorMember::Geometry(geometry) => (None, &geometry.upstream_args),
            };
        field.into_iter().chain(upstream.iter().map(|(_, expr)| expr))
    }
}

/// `members` (a ctor's uncovered defaults, its geometry lets and its lets, in
/// stored order) permuted so each member follows every sibling member it reads.
///
/// - What a member reads is `CompiledExpr::collect_value_refs` over its
///   exprs, the dependency semantics template scope orders by, so the two
///   scopes agree on every edge. Only a read of
///   `ValueCellId::new(type_name, m)` for a member `m` of `members` is an
///   edge: arg-supplied params and outer cells are already in scope before
///   any member runs.
/// - When no member reads a sibling at or after its own position, stored order
///   already is a dependency order and `members` is returned as given, with
///   no allocation: the common ctor pays one walk of its members and nothing
///   more.
/// - Otherwise ties break by lowest stored index, so the order is
///   deterministic. Members in or downstream of a cycle are appended in stored
///   order and nothing is reported: template scope already reports the cycle.
pub(crate) fn ctor_member_order<'e>(
    type_name: &str,
    members: Vec<CtorMember<'e>>,
) -> Vec<CtorMember<'e>> {
    if members.len() < 2 {
        return members;
    }
    let sibling = |id: &ValueCellId| -> Option<usize> {
        if id.entity == type_name {
            members
                .iter()
                .position(|member| member.name() == id.member.as_str())
        } else {
            None
        }
    };

    let reads_forward = members.iter().enumerate().any(|(reader, member)| {
        member.exprs().any(|expr| {
            expr.contains_value_ref_matching(|id| sibling(id).is_some_and(|read| read >= reader))
        })
    });
    if !reads_forward {
        return members;
    }

    let mut unmet_reads = vec![0usize; members.len()];
    let mut readers_of: Vec<Vec<usize>> = vec![Vec::new(); members.len()];
    for (reader, member) in members.iter().enumerate() {
        let reads: BTreeSet<usize> = member
            .exprs()
            .flat_map(CompiledExpr::collect_value_refs)
            .filter_map(|id| sibling(&id))
            .collect();
        unmet_reads[reader] = reads.len();
        for read in reads {
            readers_of[read].push(reader);
        }
    }

    let mut ready: BTreeSet<usize> = (0..members.len())
        .filter(|i| unmet_reads[*i] == 0)
        .collect();
    let mut placed = vec![false; members.len()];
    let mut ordered = Vec::with_capacity(members.len());
    while let Some(next) = ready.pop_first() {
        placed[next] = true;
        ordered.push(members[next]);
        for &reader in &readers_of[next] {
            unmet_reads[reader] -= 1;
            if unmet_reads[reader] == 0 {
                ready.insert(reader);
            }
        }
    }
    ordered.extend(
        (0..members.len())
            .filter(|i| !placed[*i])
            .map(|i| members[i]),
    );
    ordered
}

#[cfg(test)]
mod tests {
    use reify_core::{Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, Value};

    use super::{CtorMember, ctor_member_order};

    const TYPE: &str = "B";

    fn lit(n: i64) -> CompiledExpr {
        CompiledExpr::literal(Value::Int(n), Type::Int)
    }

    fn read_of(entity: &str, member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new(entity, member), Type::Int)
    }

    fn read(member: &str) -> CompiledExpr {
        read_of(TYPE, member)
    }

    fn add(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(BinOp::Add, left, right, Type::Int)
    }

    fn members(stored: Vec<(&str, CompiledExpr)>) -> Vec<(String, CompiledExpr)> {
        stored
            .into_iter()
            .map(|(name, expr)| (name.to_string(), expr))
            .collect()
    }

    fn order(stored: &[(String, CompiledExpr)]) -> Vec<&str> {
        ctor_member_order(TYPE, stored.iter().map(CtorMember::Field).collect())
            .into_iter()
            .map(CtorMember::name)
            .collect()
    }

    #[test]
    fn members_reading_only_earlier_siblings_come_back_as_given() {
        let stored = members(vec![
            ("a", lit(1)),
            ("b", add(read("a"), lit(1))),
            ("c", add(read("a"), read("b"))),
        ]);
        let given: Vec<_> = stored.iter().map(CtorMember::Field).collect();
        let buffer = given.as_ptr();

        let ordered = ctor_member_order(TYPE, given);

        assert_eq!(
            ordered.as_ptr(),
            buffer,
            "an already-ordered member list must be returned as given, not copied"
        );
        let names: Vec<&str> = ordered.iter().map(|member| member.name()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn a_forward_read_moves_the_reader_after_what_it_reads() {
        let stored = members(vec![("n", read("k")), ("k", lit(3))]);
        assert_eq!(order(&stored), ["k", "n"]);
    }

    #[test]
    fn ties_break_by_lowest_stored_index() {
        let stored = members(vec![
            ("d", add(read("b"), read("c"))),
            ("c", read("a")),
            ("b", read("a")),
            ("a", lit(1)),
        ]);
        assert_eq!(
            order(&stored),
            ["a", "c", "b", "d"],
            "`c` and `b` are both ready once `a` is placed; `c` is stored first"
        );
    }

    #[test]
    fn a_cycle_and_its_downstream_readers_are_appended_in_stored_order() {
        let stored = members(vec![
            ("r", add(read("p"), lit(1))),
            ("free", lit(1)),
            ("p", read("q")),
            ("q", read("p")),
        ]);
        assert_eq!(
            order(&stored),
            ["free", "r", "p", "q"],
            "every member the cycle blocks keeps its stored order, after the \
             members that could be placed"
        );
    }

    #[test]
    fn a_self_read_is_a_cycle() {
        let stored = members(vec![("s", add(read("s"), lit(1))), ("t", lit(1))]);
        assert_eq!(order(&stored), ["t", "s"]);
    }

    #[test]
    fn a_read_of_another_entity_is_no_edge() {
        let stored = members(vec![("a", read_of("Other", "b")), ("b", lit(1))]);
        assert_eq!(
            order(&stored),
            ["a", "b"],
            "`Other.b` is not this ctor's `b`, so `a` need not follow it"
        );
    }

    #[test]
    fn a_read_of_a_non_member_is_no_edge() {
        let stored = members(vec![
            ("b", read("c")),
            ("a", read("supplied")),
            ("c", lit(1)),
        ]);
        assert_eq!(
            order(&stored),
            ["a", "c", "b"],
            "`supplied` (an arg-supplied param or an outer cell) is already in \
             scope, so `a` is ready at once and keeps its stored precedence"
        );
    }
}
