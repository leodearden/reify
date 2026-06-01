// Implementation added in step-2.

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
