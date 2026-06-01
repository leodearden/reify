/// Canonical typed kernel discriminator for the Reify multi-kernel runtime.
///
/// This is the *single* definition of `KernelId` in the workspace.  Both
/// `reify-ir` and `reify-config` re-export it via `pub use reify_core::KernelId`
/// so the three public paths always name the same type — drift becomes
/// impossible by construction.
///
/// # Variant order
///
/// Variants are declared in **registry-name lexical order**
/// (`"fidget" < "gmsh" < "manifold" < "occt" < "openvdb"`), so the derived
/// [`Ord`] equals the dispatcher's `BTreeMap<String, _>` registry-name
/// iteration order.  This is the determinism contract in `reify_eval::dispatcher`
/// (pinned by the `kernel_id_ord_matches_registry_name_lexical_order` test).
///
/// # Extensibility
///
/// Marked `#[non_exhaustive]` so new kernel adapters can be added without
/// a breaking change to downstream `match` sites.  Exhaustive enumeration
/// for in-crate use is provided by [`KernelId::ALL`]; external crates must
/// use a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum KernelId {
    /// Fidget — pure-Rust SDF kernel (`"fidget"`).
    Fidget,
    /// Gmsh — surface→volume tetrahedral mesher (`"gmsh"`).
    Gmsh,
    /// Manifold — triangle-mesh Boolean kernel (`"manifold"`).
    Manifold,
    /// OCCT / OpenCASCADE — B-rep kernel (`"occt"`).
    Occt,
    /// OpenVDB — voxel-grid kernel (`"openvdb"`).
    OpenVdb,
}

impl KernelId {
    /// All `KernelId` variants in declaration (== registry-name lexical) order.
    ///
    /// Provides a stable enumeration handle for exhaustive in-crate tests and
    /// callers, since `#[non_exhaustive]` forbids external exhaustive `match`.
    pub const ALL: [KernelId; 5] = [
        KernelId::Fidget,
        KernelId::Gmsh,
        KernelId::Manifold,
        KernelId::Occt,
        KernelId::OpenVdb,
    ];

    /// Canonical lowercase registry name for this kernel.
    ///
    /// Equals the `*_KERNEL_NAME` const each kernel crate registers as its
    /// `KernelRegistration::name` (and the dispatcher's `BTreeMap` key), so
    /// `from_registry_name` is its exact inverse.  Exhaustive in-crate
    /// `match` — adding a variant forces updating this bridge at the same
    /// diff site.
    pub const fn as_registry_name(self) -> &'static str {
        match self {
            KernelId::Fidget => "fidget",
            KernelId::Gmsh => "gmsh",
            KernelId::Manifold => "manifold",
            KernelId::Occt => "occt",
            KernelId::OpenVdb => "openvdb",
        }
    }

    /// Inverse of [`as_registry_name`](KernelId::as_registry_name): resolve a
    /// canonical registry name back to its `KernelId`, or `None` if the string
    /// is not a registered kernel name.
    ///
    /// Exact inverse over the distinct canonical names, so
    /// `from_registry_name(k.as_registry_name()) == Some(k)` for every variant.
    /// Matching is case-sensitive — registry names are canonical lowercase.
    pub fn from_registry_name(name: &str) -> Option<KernelId> {
        match name {
            "fidget" => Some(KernelId::Fidget),
            "gmsh" => Some(KernelId::Gmsh),
            "manifold" => Some(KernelId::Manifold),
            "occt" => Some(KernelId::Occt),
            "openvdb" => Some(KernelId::OpenVdb),
            _ => None,
        }
    }
}

impl std::fmt::Display for KernelId {
    /// Delegates to [`as_registry_name`](KernelId::as_registry_name) — single
    /// source of truth for the canonical string representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_registry_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // (a) ALL has exactly 5 elements in the declared lexical order.
    #[test]
    fn all_has_five_variants_in_lexical_order() {
        assert_eq!(KernelId::ALL.len(), 5);
        assert_eq!(
            KernelId::ALL,
            [
                KernelId::Fidget,
                KernelId::Gmsh,
                KernelId::Manifold,
                KernelId::Occt,
                KernelId::OpenVdb,
            ]
        );
    }

    // (b) as_registry_name returns the five canonical lowercase strings.
    #[test]
    fn as_registry_name_returns_canonical_strings() {
        assert_eq!(KernelId::Fidget.as_registry_name(), "fidget");
        assert_eq!(KernelId::Gmsh.as_registry_name(), "gmsh");
        assert_eq!(KernelId::Manifold.as_registry_name(), "manifold");
        assert_eq!(KernelId::Occt.as_registry_name(), "occt");
        assert_eq!(KernelId::OpenVdb.as_registry_name(), "openvdb");
    }

    // (c) from_registry_name round-trips exhaustively and returns None for
    //     bogus/empty/wrong-case inputs.
    #[test]
    fn registry_name_round_trips_exhaustively() {
        for k in KernelId::ALL {
            assert_eq!(
                KernelId::from_registry_name(k.as_registry_name()),
                Some(k),
                "round-trip must recover {k:?} from its registry name {:?}",
                k.as_registry_name(),
            );
        }
        assert_eq!(KernelId::from_registry_name("bogus"), None);
        assert_eq!(KernelId::from_registry_name(""), None);
        assert_eq!(KernelId::from_registry_name("OCCT"), None);
        assert_eq!(KernelId::from_registry_name("Manifold"), None);
    }

    // (d) derived Ord equals registry-name lexical order.
    //     (a) consecutive names across ALL are strictly increasing
    //     (b) BTreeMap<String, KernelId> keyed by registry name iterates in ALL order
    //     (c) sort(ALL) == ALL (sorted is a no-op)
    #[test]
    fn kernel_id_ord_matches_registry_name_lexical_order() {
        // (a) registry names strictly increasing in declaration order
        let names: Vec<&'static str> =
            KernelId::ALL.iter().map(|k| k.as_registry_name()).collect();
        for w in names.windows(2) {
            assert!(
                w[0] < w[1],
                "registry names must be strictly lexically increasing: {:?} !< {:?}",
                w[0],
                w[1]
            );
        }

        // (b) BTreeMap<String, KernelId> iterates in ALL order
        let map: BTreeMap<String, KernelId> = KernelId::ALL
            .iter()
            .map(|k| (k.as_registry_name().to_string(), *k))
            .collect();
        let by_name_order: Vec<KernelId> = map.values().copied().collect();
        assert_eq!(by_name_order, KernelId::ALL.to_vec());

        // (c) derived Ord is a no-op sort
        let mut sorted = KernelId::ALL.to_vec();
        sorted.sort();
        assert_eq!(sorted, KernelId::ALL.to_vec());
    }

    // (e) Display / to_string() equals as_registry_name() for every variant.
    #[test]
    fn display_equals_as_registry_name() {
        for k in KernelId::ALL {
            assert_eq!(
                k.to_string(),
                k.as_registry_name(),
                "Display for {k:?} must equal as_registry_name()"
            );
        }
    }
}
