//! The order a structure ctor evaluates its members in.

use std::collections::{BTreeSet, HashMap};

use reify_core::ValueCellId;
use reify_ir::CompiledExpr;

/// `members` (a ctor's uncovered defaults followed by its lets, in stored
/// order) permuted so each member follows every sibling member it reads.
///
/// - What a member reads is `CompiledExpr::collect_value_refs`, the dependency
///   semantics template scope orders by, so the two scopes agree on every
///   edge. Only a read of `ValueCellId::new(type_name, m)` for a member `m` of
///   `members` is an edge: arg-supplied params and outer cells are already in
///   scope before any member runs.
/// - When no member reads a sibling at or after its own position, stored order
///   already is a dependency order and is returned unsorted.
/// - Otherwise ties break by lowest stored index, so the order is
///   deterministic. Members in or downstream of a cycle are appended in stored
///   order and nothing is reported: template scope already reports the cycle.
pub(crate) fn ctor_member_order<'e>(
    type_name: &str,
    members: &[&'e (String, CompiledExpr)],
) -> Vec<&'e (String, CompiledExpr)> {
    if members.len() < 2 {
        return members.to_vec();
    }
    let mut index_of: HashMap<&str, usize> = HashMap::with_capacity(members.len());
    for (i, (name, _)) in members.iter().enumerate() {
        index_of.entry(name.as_str()).or_insert(i);
    }
    let sibling = |id: &ValueCellId| -> Option<usize> {
        if id.entity == type_name {
            index_of.get(id.member.as_str()).copied()
        } else {
            None
        }
    };

    let reads_forward = members.iter().enumerate().any(|(reader, (_, expr))| {
        expr.contains_value_ref_matching(|id| sibling(id).is_some_and(|read| read >= reader))
    });
    if !reads_forward {
        return members.to_vec();
    }

    let mut unmet_reads = vec![0usize; members.len()];
    let mut readers_of: Vec<Vec<usize>> = vec![Vec::new(); members.len()];
    for (reader, (_, expr)) in members.iter().enumerate() {
        let reads: BTreeSet<usize> = expr
            .collect_value_refs()
            .iter()
            .filter_map(&sibling)
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
