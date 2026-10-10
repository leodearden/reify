#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::KernelId;
    use std::collections::BTreeMap;

    const SENTINEL: &str = crate::Engine::DEFAULT_KERNEL_NAME;

    fn kernel_map(names: &[&str]) -> BTreeMap<String, ()> {
        names.iter().map(|n| (n.to_string(), ())).collect()
    }

    #[test]
    fn owner_present_in_map_resolves_to_its_registry_name() {
        let kernels = kernel_map(&["occt", "manifold"]);
        assert_eq!(
            resolve_owning_kernel_name(&kernels, KernelId::Manifold, "occt"),
            "manifold"
        );
    }

    #[test]
    fn owner_absent_from_map_falls_back_to_default() {
        let kernels = kernel_map(&[SENTINEL]);
        assert_eq!(
            resolve_owning_kernel_name(&kernels, KernelId::Occt, SENTINEL),
            SENTINEL
        );
    }

    #[test]
    fn single_owner_set_resolves_to_that_owner() {
        let kernels = kernel_map(&["occt", "openvdb"]);
        assert_eq!(
            export_kernel_for_owners(&kernels, [KernelId::OpenVdb, KernelId::OpenVdb], "occt"),
            Ok("openvdb")
        );
    }

    #[test]
    fn owners_that_resolve_to_the_same_key_are_not_mixed() {
        let kernels = kernel_map(&[SENTINEL, "manifold"]);
        assert_eq!(
            export_kernel_for_owners(
                &kernels,
                [KernelId::Occt, KernelId::Occt, KernelId::Gmsh],
                SENTINEL
            ),
            Ok(SENTINEL)
        );
    }

    #[test]
    fn distinct_resolved_owners_are_mixed() {
        let kernels = kernel_map(&[SENTINEL, "manifold"]);
        let mixed = export_kernel_for_owners(
            &kernels,
            [KernelId::Occt, KernelId::Manifold, KernelId::Occt],
            SENTINEL,
        )
        .expect_err("bodies on two distinct serving kernels must not resolve to one");
        assert_eq!(
            mixed.kernel_names,
            vec![SENTINEL.to_string(), "manifold".to_string()]
        );
        let message = mixed.to_string();
        assert!(
            message.contains(SENTINEL) && message.contains("manifold"),
            "the mixed-owner error must name every offending kernel, got: {message}"
        );
    }

    #[test]
    fn empty_owner_set_resolves_to_default() {
        let kernels = kernel_map(&["occt", "manifold"]);
        assert_eq!(
            export_kernel_for_owners(&kernels, std::iter::empty(), "occt"),
            Ok("occt")
        );
    }
}
