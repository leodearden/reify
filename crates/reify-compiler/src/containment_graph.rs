#[cfg(test)]
mod tests {
    use crate::{EntityKind, GuardState, SubComponentDecl, TopologyTemplate, Visibility};
    use reify_core::{ContentHash, SourceSpan};
    use std::collections::{HashMap, HashSet};

    fn minimal_template(name: &str) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: vec![],
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    fn sub_ref(name: &str, target: &str) -> SubComponentDecl {
        SubComponentDecl {
            name: name.to_string(),
            structure_name: target.to_string(),
            visibility: Visibility::Public,
            args: vec![],
            type_args: vec![],
            is_collection: false,
            keyed_members: Vec::new(),
            count_cell: None,
            guard_state: GuardState::None,
            pose: None,
            auto_pose: None,
            is_aux: false,
            span: SourceSpan::new(0, 0),
            content_hash: ContentHash(0),
        }
    }

    /// A references B, C, a duplicate B, and an undefined "Ghost".
    /// The adjacency row for A must be [idx_B, idx_C] — Ghost skipped,
    /// duplicate B collapsed.  Leaf rows for B and C must be empty.
    #[test]
    fn forward_adjacency_dedupes_and_skips_unknown() {
        let mut a = minimal_template("A");
        a.sub_components = vec![
            sub_ref("b1", "B"),
            sub_ref("c1", "C"),
            sub_ref("b2", "B"),        // duplicate edge to B
            sub_ref("ghost", "Ghost"), // unresolved name
        ];
        let b = minimal_template("B");
        let c = minimal_template("C");
        // A=0, B=1, C=2; "Ghost" absent from name_to_idx.
        let templates = vec![a, b, c];

        let name_to_idx: HashMap<&str, usize> = templates
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.as_str(), i))
            .collect();

        let adj = super::sub_component_forward_adjacency(&templates, &name_to_idx);

        assert_eq!(adj.len(), 3);
        // A → [B(1), C(2)]: sorted, deduplicated, Ghost skipped.
        assert_eq!(adj[0], vec![1usize, 2]);
        // B and C are leaves: no outgoing edges.
        assert_eq!(adj[1], Vec::<usize>::new());
        assert_eq!(adj[2], Vec::<usize>::new());
    }
}
