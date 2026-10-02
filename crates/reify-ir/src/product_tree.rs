#[cfg(test)]
mod tests {
    use super::*;

    fn pref(name: &str, dedupe_index: u32) -> ProductRef {
        ProductRef {
            name: name.to_string(),
            dedupe_index,
        }
    }

    fn part(name: &str, dedupe_index: u32, solid_count: u32) -> ProductNode {
        ProductNode {
            name: name.to_string(),
            dedupe_index,
            kind: ProductKind::Part { solid_count },
            components: Vec::new(),
        }
    }

    fn assembly(name: &str, dedupe_index: u32, components: Vec<Component>) -> ProductNode {
        ProductNode {
            name: name.to_string(),
            dedupe_index,
            kind: ProductKind::Assembly,
            components,
        }
    }

    fn comp(target: ProductRef, instance_name: &str, x: f64) -> Component {
        Component {
            product: target,
            instance_name: instance_name.to_string(),
            location: Placement {
                translation: [x, 0.0, 0.0],
                ..Placement::IDENTITY
            },
        }
    }

    fn container_tree_products() -> Vec<ProductNode> {
        vec![
            assembly(
                "Container",
                1,
                vec![
                    comp(pref("Casting", 1), "Casting-1", 0.0),
                    comp(pref("Casting", 1), "Casting-2", 1.0),
                    comp(pref("Wall", 1), "Wall-1", 0.0),
                ],
            ),
            part("Casting", 1, 1),
            assembly("Wall", 1, vec![comp(pref("Panel", 1), "Panel-1", 0.0)]),
            part("Panel", 1, 3),
        ]
    }

    #[test]
    fn valid_tree_constructs_and_resolves_refs() {
        let products = container_tree_products();
        let tree = ProductTree::new(products.clone(), vec![pref("Container", 1)])
            .expect("a well-formed tree constructs");

        assert_eq!(tree.roots(), &[pref("Container", 1)]);
        assert_eq!(tree.products(), products.as_slice());

        let casting = tree
            .product(&pref("Casting", 1))
            .expect("shared part is present");
        assert_eq!(casting.kind, ProductKind::Part { solid_count: 1 });
        assert_eq!(casting.product_ref(), pref("Casting", 1));

        let container = tree
            .product(&pref("Container", 1))
            .expect("root is present");
        assert_eq!(container.components.len(), 3);
        assert_eq!(container.components[0].product, pref("Casting", 1));
        assert_eq!(container.components[1].product, pref("Casting", 1));
        assert_eq!(
            container.components[1].location.translation,
            [1.0, 0.0, 0.0]
        );

        for (index, node) in tree.products().iter().enumerate() {
            assert_eq!(tree.position(&node.product_ref()), Some(index));
        }
        assert_eq!(tree.position(&pref("Ghost", 1)), None);
        assert_eq!(tree.position(&pref("Casting", 2)), None);
        assert!(tree.product(&pref("Ghost", 1)).is_none());
    }

    #[test]
    fn same_name_with_distinct_dedupe_indices_are_distinct_products() {
        let tree = ProductTree::new(
            vec![
                assembly(
                    "Frame",
                    1,
                    vec![
                        comp(pref("Pin", 1), "Pin-1", 0.0),
                        comp(pref("Pin", 2), "Pin-2", 0.3),
                    ],
                ),
                part("Pin", 1, 1),
                part("Pin", 2, 1),
            ],
            vec![pref("Frame", 1)],
        )
        .expect("two products sharing a name but not a dedupe index are accepted");

        assert_eq!(tree.position(&pref("Pin", 1)), Some(1));
        assert_eq!(tree.position(&pref("Pin", 2)), Some(2));
    }

    #[test]
    fn product_ref_display_is_name_hash_dedupe_index() {
        assert_eq!(pref("Pin", 2).to_string(), "Pin#2");
    }

    #[test]
    fn placement_identity_is_the_unit_rotation_at_the_origin() {
        assert_eq!(Placement::IDENTITY.translation, [0.0; 3]);
        assert_eq!(
            Placement::IDENTITY.rotation,
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn each_violated_invariant_is_rejected_with_its_own_error() {
        let cases: Vec<(&str, Vec<ProductNode>, Vec<ProductRef>, ProductTreeError)> = vec![
            (
                "no roots",
                vec![part("Solo", 1, 1)],
                vec![],
                ProductTreeError::NoRoots,
            ),
            (
                "zero dedupe index",
                vec![part("Solo", 0, 1)],
                vec![pref("Solo", 0)],
                ProductTreeError::ZeroDedupeIndex(pref("Solo", 0)),
            ),
            (
                "duplicate product",
                vec![
                    assembly("Top", 1, vec![comp(pref("Pin", 1), "Pin-1", 0.0)]),
                    part("Pin", 1, 1),
                    part("Pin", 1, 2),
                ],
                vec![pref("Top", 1)],
                ProductTreeError::DuplicateProduct(pref("Pin", 1)),
            ),
            (
                "part with components",
                vec![
                    ProductNode {
                        components: vec![comp(pref("Pin", 1), "Pin-1", 0.0)],
                        ..part("Odd", 1, 1)
                    },
                    part("Pin", 1, 1),
                ],
                vec![pref("Odd", 1)],
                ProductTreeError::PartWithComponents(pref("Odd", 1)),
            ),
            (
                "dangling component",
                vec![assembly(
                    "Top",
                    1,
                    vec![comp(pref("Ghost", 1), "Ghost-1", 0.0)],
                )],
                vec![pref("Top", 1)],
                ProductTreeError::DanglingComponent {
                    parent: pref("Top", 1),
                    target: pref("Ghost", 1),
                },
            ),
            (
                "dangling root",
                vec![part("Solo", 1, 1)],
                vec![pref("Solo", 1), pref("Ghost", 1)],
                ProductTreeError::DanglingRoot(pref("Ghost", 1)),
            ),
            (
                "cycle",
                vec![
                    assembly("A", 1, vec![comp(pref("B", 1), "B-1", 0.0)]),
                    assembly("B", 1, vec![comp(pref("A", 1), "A-1", 0.0)]),
                ],
                vec![pref("A", 1)],
                ProductTreeError::Cycle(pref("A", 1)),
            ),
        ];

        for (label, products, roots, expected) in cases {
            match ProductTree::new(products, roots) {
                Ok(_) => panic!("{label}: expected {expected:?}, but the tree was accepted"),
                Err(actual) => assert_eq!(actual, expected, "{label}"),
            }
        }
    }

    #[test]
    fn errors_name_the_offending_product_ref() {
        let error = ProductTreeError::DanglingComponent {
            parent: pref("Top", 1),
            target: pref("Ghost", 3),
        };
        let message = error.to_string();
        assert!(message.contains("Top#1"), "{message}");
        assert!(message.contains("Ghost#3"), "{message}");
        assert!(
            ProductTreeError::Cycle(pref("A", 2))
                .to_string()
                .contains("A#2")
        );
    }
}
