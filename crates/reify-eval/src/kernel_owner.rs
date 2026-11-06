//! Map a geometry handle's owning [`KernelId`] to the engine kernel-map key
//! that serves it.
//!
//! The rule, stated once here: a handle is served by the map entry keyed by
//! its owner's registry name when that name is a key of the map, else by
//! `default_kernel_name`. The fallback exists because single-pick (sentinel)
//! mode keys the default kernel under `Engine::DEFAULT_KERNEL_NAME`
//! (`__reify_eval_default_kernel`), never under `"occt"`. It matters because
//! handle ids are kernel-local — every kernel counts from 1 — so a handle
//! resolved against any kernel other than its owner names an unrelated shape.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use reify_ir::KernelId;

/// The kernel-map key serving a handle owned by `owner`.
///
/// The result borrows only `default_kernel_name`, never `kernels`, so callers
/// may `get_mut` the map with it.
pub(crate) fn resolve_owning_kernel_name<'a, V>(
    kernels: &BTreeMap<String, V>,
    owner: KernelId,
    default_kernel_name: &'a str,
) -> &'a str {
    let registry_name = owner.as_registry_name();
    if kernels.contains_key(registry_name) {
        registry_name
    } else {
        default_kernel_name
    }
}

/// The one kernel-map key serving every handle in `owners`, for exporting
/// them together; `default_kernel_name` when `owners` is empty.
///
/// Owners are compared on their RESOLVED keys, so several owners that all fall
/// back to the default kernel are one owner, not a mix.
pub(crate) fn export_kernel_for_owners<'a, V>(
    kernels: &BTreeMap<String, V>,
    owners: impl IntoIterator<Item = KernelId>,
    default_kernel_name: &'a str,
) -> Result<&'a str, MixedExportOwners> {
    let resolved: BTreeSet<&'a str> = owners
        .into_iter()
        .map(|owner| resolve_owning_kernel_name(kernels, owner, default_kernel_name))
        .collect();
    if resolved.len() > 1 {
        return Err(MixedExportOwners {
            kernel_names: resolved.into_iter().map(str::to_string).collect(),
        });
    }
    Ok(resolved.into_iter().next().unwrap_or(default_kernel_name))
}

/// Product bodies that resolve to more than one serving kernel, so they
/// cannot be assembled into one compound: a compound takes ids in a single
/// kernel's id space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MixedExportOwners {
    /// The distinct resolved kernel-map keys, sorted.
    pub kernel_names: Vec<String>,
}

impl fmt::Display for MixedExportOwners {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot export product bodies owned by different kernels as one compound \
             (owners: {}); cross-kernel compound export is not supported",
            self.kernel_names.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
