use super::*;

const SENTINEL: &str = crate::Engine::DEFAULT_KERNEL_NAME;

fn kernel_map(names: &[&str]) -> BTreeMap<String, ()> {
    names.iter().map(|n| (n.to_string(), ())).collect()
}

#[test]
fn a_source_present_in_the_map_is_served_under_its_registry_name() {
    let kernels = kernel_map(&[SENTINEL, "openvdb"]);
    assert_eq!(
        conversion_source_key(&kernels, KernelId::OpenVdb, SENTINEL),
        Some("openvdb")
    );
}

#[test]
fn the_default_kernel_identity_is_served_by_the_single_pick_sentinel() {
    let kernels = kernel_map(&[SENTINEL, "openvdb"]);
    assert_eq!(
        conversion_source_key(&kernels, KernelId::Occt, SENTINEL),
        Some(SENTINEL)
    );
}

#[test]
fn an_absent_non_default_source_is_not_served_by_the_sentinel() {
    let kernels = kernel_map(&[SENTINEL, "openvdb"]);
    assert_eq!(
        conversion_source_key(&kernels, KernelId::Manifold, SENTINEL),
        None,
        "a handle id is kernel-local, so a manifold-sourced stage must not run on \
         the default kernel"
    );
}

#[test]
fn an_absent_non_default_source_is_not_served_by_a_named_default_kernel() {
    let kernels = kernel_map(&["occt", "openvdb"]);
    assert_eq!(
        conversion_source_key(&kernels, KernelId::Manifold, "occt"),
        None
    );
}
