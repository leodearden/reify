//! The kernel-neutral product structure of an imported CAD assembly (STEP via
//! OCCT XDE today; `docs/prds/v0_6/step-assembly-import.md` §5 C1).
//!
//! Products are addressed by name plus a 1-based dedupe index assigned in reader
//! traversal order, never by a kernel label. A [`Placement`] maps product-local
//! coordinates into the PARENT product's frame,
//! `p_parent = rotation · p_local + translation`, in metres with a row-major
//! rotation carried exactly as read: not orthonormalised and possibly improper.
//! Validating it is the generator's job (C2.4).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

/// Name + 1-based dedupe index: the stable address of one product.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProductRef {
    pub name: String,
    pub dedupe_index: u32,
}

impl fmt::Display for ProductRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.name, self.dedupe_index)
    }
}

/// A component's pose in its parent product's frame (metres, row-major rotation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub translation: [f64; 3],
    pub rotation: [[f64; 3]; 3],
}

impl Placement {
    pub const IDENTITY: Placement = Placement {
        translation: [0.0; 3],
        rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };
}

/// One occurrence of `product` inside an assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub product: ProductRef,
    pub instance_name: String,
    pub location: Placement,
}

/// An assembly places components and owns no bodies; a part is the reverse.
#[derive(Clone, Debug, PartialEq)]
pub enum ProductKind {
    Assembly { components: Vec<Component> },
    Part { solid_count: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProductNode {
    pub product: ProductRef,
    pub kind: ProductKind,
}

impl ProductNode {
    /// The occurrences this product places: none for a [`ProductKind::Part`].
    pub fn components(&self) -> &[Component] {
        match &self.kind {
            ProductKind::Assembly { components } => components,
            ProductKind::Part { .. } => &[],
        }
    }
}

/// A validated product DAG: every reference resolves, refs are unique and
/// 1-based, and no product contains itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ProductTree {
    products: Vec<ProductNode>,
    roots: Vec<ProductRef>,
    positions: HashMap<ProductRef, usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductTreeError {
    NoRoots,
    ZeroDedupeIndex(ProductRef),
    DuplicateProduct(ProductRef),
    DanglingComponent {
        parent: ProductRef,
        target: ProductRef,
    },
    DanglingRoot(ProductRef),
    Cycle(ProductRef),
}

impl fmt::Display for ProductTreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRoots => write!(f, "product tree has no root product"),
            Self::ZeroDedupeIndex(product) => {
                write!(
                    f,
                    "product {product} has dedupe index 0 (indices are 1-based)"
                )
            }
            Self::DuplicateProduct(product) => write!(f, "product {product} appears twice"),
            Self::DanglingComponent { parent, target } => write!(
                f,
                "assembly {parent} has a component referencing absent product {target}"
            ),
            Self::DanglingRoot(product) => write!(f, "root {product} is not a product of the tree"),
            Self::Cycle(product) => write!(f, "product {product} contains itself"),
        }
    }
}

impl std::error::Error for ProductTreeError {}

impl ProductTree {
    /// Validates and builds a tree, returning the first violated invariant.
    pub fn new(
        products: Vec<ProductNode>,
        roots: Vec<ProductRef>,
    ) -> Result<ProductTree, ProductTreeError> {
        if roots.is_empty() {
            return Err(ProductTreeError::NoRoots);
        }
        let mut positions = HashMap::with_capacity(products.len());
        for (index, node) in products.iter().enumerate() {
            if node.product.dedupe_index == 0 {
                return Err(ProductTreeError::ZeroDedupeIndex(node.product.clone()));
            }
            match positions.entry(node.product.clone()) {
                Entry::Occupied(_) => {
                    return Err(ProductTreeError::DuplicateProduct(node.product.clone()));
                }
                Entry::Vacant(slot) => {
                    slot.insert(index);
                }
            }
        }
        let tree = ProductTree {
            products,
            roots,
            positions,
        };
        for node in &tree.products {
            if let Some(component) = node
                .components()
                .iter()
                .find(|component| tree.position(&component.product).is_none())
            {
                return Err(ProductTreeError::DanglingComponent {
                    parent: node.product.clone(),
                    target: component.product.clone(),
                });
            }
        }
        if let Some(root) = tree.roots.iter().find(|root| tree.position(root).is_none()) {
            return Err(ProductTreeError::DanglingRoot(root.clone()));
        }
        tree.check_acyclic()?;
        Ok(tree)
    }

    pub fn products(&self) -> &[ProductNode] {
        &self.products
    }

    pub fn roots(&self) -> &[ProductRef] {
        &self.roots
    }

    pub fn product(&self, product: &ProductRef) -> Option<&ProductNode> {
        self.position(product).map(|index| &self.products[index])
    }

    /// Index of `product` in [`Self::products`].
    pub fn position(&self, product: &ProductRef) -> Option<usize> {
        self.positions.get(product).copied()
    }

    /// Iterative white/grey/black DFS over the component graph from every
    /// product; a component reaching a grey product closes a cycle.
    /// Precondition: every component target resolves.
    fn check_acyclic(&self) -> Result<(), ProductTreeError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Colour {
            White,
            Grey,
            Black,
        }
        let target_index = |component: &Component| {
            self.position(&component.product)
                .expect("component targets were resolved before the cycle check")
        };
        let mut colour = vec![Colour::White; self.products.len()];
        for start in 0..self.products.len() {
            if colour[start] != Colour::White {
                continue;
            }
            colour[start] = Colour::Grey;
            let mut stack = vec![(start, 0usize)];
            while let Some(frame) = stack.last_mut() {
                let (node, next_component) = *frame;
                match self.products[node].components().get(next_component) {
                    Some(component) => {
                        frame.1 += 1;
                        let target = target_index(component);
                        match colour[target] {
                            Colour::Grey => {
                                return Err(ProductTreeError::Cycle(component.product.clone()));
                            }
                            Colour::White => {
                                colour[target] = Colour::Grey;
                                stack.push((target, 0));
                            }
                            Colour::Black => {}
                        }
                    }
                    None => {
                        colour[node] = Colour::Black;
                        stack.pop();
                    }
                }
            }
        }
        Ok(())
    }
}

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
            product: pref(name, dedupe_index),
            kind: ProductKind::Part { solid_count },
        }
    }

    fn assembly(name: &str, dedupe_index: u32, components: Vec<Component>) -> ProductNode {
        ProductNode {
            product: pref(name, dedupe_index),
            kind: ProductKind::Assembly { components },
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
        assert!(casting.components().is_empty());

        let container = tree
            .product(&pref("Container", 1))
            .expect("root is present");
        let components = container.components();
        assert_eq!(components.len(), 3);
        assert_eq!(components[0].product, pref("Casting", 1));
        assert_eq!(components[1].product, pref("Casting", 1));
        assert_eq!(components[1].location.translation, [1.0, 0.0, 0.0]);

        for (index, node) in tree.products().iter().enumerate() {
            assert_eq!(tree.position(&node.product), Some(index));
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
