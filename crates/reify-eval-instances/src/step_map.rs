#[cfg(test)]
mod tests {
    use reify_core::{DefinitionName, InstancePath, ModuleName, PathName, PathStep};

    use super::{core_steps, definition, definition_name, defs, member, member_name, root};

    fn name(text: &str) -> PathName {
        PathName::new(text).expect("a test name is an identifier")
    }

    fn in_entry(text: &str) -> DefinitionName {
        DefinitionName::new(None, name(text))
    }

    fn qualified() -> DefinitionName {
        let module = ModuleName::new(vec![name("std"), name("units")])
            .expect("a module of two segments");
        DefinitionName::new(Some(module), name("Plate"))
    }

    /// Member names, including those that read like a fixed child's word.
    fn member_names() -> Vec<PathName> {
        ["t", "body", "summary", "defs", "as_value", "_x9"]
            .into_iter()
            .map(name)
            .collect()
    }

    fn definitions() -> Vec<DefinitionName> {
        vec![in_entry("Plate"), qualified()]
    }

    #[test]
    fn a_member_step_reads_back_as_its_name() {
        for n in member_names() {
            assert_eq!(member_name(&member(&n)), Some(n.clone()), "{n:?}");
        }
    }

    #[test]
    fn a_definition_step_reads_back_as_its_definition() {
        for d in definitions() {
            assert_eq!(definition_name(&definition(&d)), Some(d.clone()), "{d:?}");
        }
    }

    #[test]
    fn the_spaces_are_disjoint() {
        for n in member_names() {
            assert_eq!(definition_name(&member(&n)), None, "{n:?}");
        }
        for d in definitions() {
            assert_eq!(member_name(&definition(&d)), None, "{d:?}");
        }
        assert_eq!(member_name(&defs()), None);
        assert_eq!(definition_name(&defs()), None);
    }

    #[test]
    fn a_typed_step_maps_to_the_core_steps_its_kinds_declare() {
        for n in member_names() {
            assert_eq!(
                core_steps(&PathStep::Member(n.clone())),
                Some(vec![member(&n)])
            );
        }
        for d in definitions() {
            assert_eq!(
                core_steps(&PathStep::Defs(d.clone())),
                Some(vec![defs(), definition(&d)])
            );
        }
    }

    #[test]
    fn a_root_is_named_by_its_definitions_path_text() {
        for d in definitions() {
            assert_eq!(
                root(&d).name(),
                InstancePath::new(d.clone()).to_string(),
                "{d:?}"
            );
        }
    }
}
