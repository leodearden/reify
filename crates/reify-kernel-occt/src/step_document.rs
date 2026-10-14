//! STEP assembly import through OCCT XDE (PRD docs/prds/v0_6/step-assembly-import.md
//! §5 C1): a STEP file read into a kernel-neutral [`ProductTree`] in metres,
//! with the native XCAF document kept alive for access to each product's
//! solids.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use reify_ir::{
    Component, Placement, ProductKind, ProductNode, ProductRef, ProductTree, ProductTreeError,
};

use crate::ffi::ffi;

/// Why a STEP file could not be read into a [`StepDocument`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepReadError {
    /// Missing, unreadable, not STEP, or a path that is not UTF-8.
    Unreadable { path: PathBuf },
    /// The file parsed but declares no product.
    NoRoots { path: PathBuf },
    /// The file parsed but could not be turned into a product tree. `detail`
    /// is for humans only.
    TransferFailed { path: PathBuf, detail: String },
}

impl fmt::Display for StepReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path } => {
                write!(f, "cannot read {} as a STEP file", path.display())
            }
            Self::NoRoots { path } => {
                write!(f, "STEP file {} declares no product", path.display())
            }
            Self::TransferFailed { path, detail } => write!(
                f,
                "STEP file {} could not be imported: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StepReadError {}

/// Why a product's body cannot be taken from a [`StepDocument`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepBodyError {
    UnknownProduct(ProductRef),
    /// Assemblies own no bodies; their components' products do.
    NotAPart(ProductRef),
    BodyIndexOutOfRange {
        product: ProductRef,
        index: u32,
        solid_count: u32,
    },
    /// The tree promises this body but the native document could not produce
    /// it: a reader defect, not a bad request. `detail` is for humans only.
    Native {
        product: ProductRef,
        index: u32,
        detail: String,
    },
}

impl fmt::Display for StepBodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProduct(product) => {
                write!(f, "the STEP document has no product {product}")
            }
            Self::NotAPart(product) => write!(
                f,
                "product {product} is an assembly and has no bodies of its own"
            ),
            Self::BodyIndexOutOfRange {
                product,
                index,
                solid_count,
            } => write!(
                f,
                "product {product} has {solid_count} solid(s), so body {index} does not exist"
            ),
            Self::Native {
                product,
                index,
                detail,
            } => write!(
                f,
                "body {index} of product {product} is in the product tree but the STEP \
                 reader's native document could not produce it (a reader defect): {detail}"
            ),
        }
    }
}

impl std::error::Error for StepBodyError {}

/// A STEP file read through OCCT XDE: its product tree, with lengths in
/// metres, and the native document that owns every product's shapes.
///
/// `!Send`, like `OcctKernel`: the native document stays on the thread that
/// drives OCCT.
pub struct StepDocument {
    path: PathBuf,
    native: cxx::UniquePtr<ffi::OcctStepDocument>,
    tree: ProductTree,
}

impl StepDocument {
    pub fn read(path: &Path) -> Result<Self, StepReadError> {
        let Some(path_text) = path.to_str() else {
            return Err(StepReadError::Unreadable {
                path: path.to_path_buf(),
            });
        };
        let transfer_failed = |detail: String| StepReadError::TransferFailed {
            path: path.to_path_buf(),
            detail,
        };
        let native = ffi::read_step_document(path_text)
            .map_err(|exception| transfer_failed(exception.what().to_string()))?;
        let records = ffi::step_document_tree(&native);
        match records.status {
            ffi::StepReadStatus::Read => {}
            ffi::StepReadStatus::Unreadable => {
                return Err(StepReadError::Unreadable {
                    path: path.to_path_buf(),
                });
            }
            ffi::StepReadStatus::NoRoots => {
                return Err(StepReadError::NoRoots {
                    path: path.to_path_buf(),
                });
            }
            ffi::StepReadStatus::TransferFailed => {
                return Err(transfer_failed(
                    "OCCT's XDE transfer reported failure".to_string(),
                ));
            }
            other => unreachable!("read_step_document returned undeclared status {other:?}"),
        }
        let tree = decode(&records).map_err(|error| transfer_failed(error.to_string()))?;
        Ok(StepDocument {
            path: path.to_path_buf(),
            native,
            tree,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn tree(&self) -> &ProductTree {
        &self.tree
    }

    /// The `body_index`-th solid (0-based, the order `solid_count` counts) of
    /// `product`, in the product's local frame. Validated against the tree,
    /// whose products are index-aligned with the native walk.
    pub(crate) fn body_shape(
        &self,
        product: &ProductRef,
        body_index: u32,
    ) -> Result<cxx::UniquePtr<ffi::OcctShape>, StepBodyError> {
        let position = self
            .tree
            .position(product)
            .ok_or_else(|| StepBodyError::UnknownProduct(product.clone()))?;
        let solid_count = match self.tree.products()[position].kind {
            ProductKind::Assembly { .. } => return Err(StepBodyError::NotAPart(product.clone())),
            ProductKind::Part { solid_count } => solid_count,
        };
        if body_index >= solid_count {
            return Err(StepBodyError::BodyIndexOutOfRange {
                product: product.clone(),
                index: body_index,
                solid_count,
            });
        }
        let product_index =
            u32::try_from(position).expect("product positions come from u32 walk indices");
        ffi::step_document_body(&self.native, product_index, body_index).map_err(|exception| {
            StepBodyError::Native {
                product: product.clone(),
                index: body_index,
                detail: exception.what().to_string(),
            }
        })
    }
}

/// The process-global `xstep.cascade.unit` static, read under the XSTEP lock,
/// so tests can observe that [`StepDocument::read`] restores it.
#[cfg(feature = "test-fixtures")]
pub fn xstep_cascade_unit_for_test() -> String {
    ffi::xstep_cascade_unit_for_test().expect("read the xstep.cascade.unit static")
}

/// Why the reader's flat records do not form a valid [`ProductTree`].
#[derive(Debug, PartialEq)]
enum DecodeError {
    ProductIndexOutOfRange {
        index: u32,
        product_count: usize,
    },
    ComponentRangeOutOfRange {
        product: ProductRef,
        first: u32,
        count: u32,
        component_count: usize,
    },
    PartWithComponents(ProductRef),
    Tree(ProductTreeError),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProductIndexOutOfRange {
                index,
                product_count,
            } => write!(
                f,
                "product index {index} is out of range for {product_count} products"
            ),
            Self::ComponentRangeOutOfRange {
                product,
                first,
                count,
                component_count,
            } => write!(
                f,
                "product {product} claims components {first}..{first}+{count} of \
                 {component_count}"
            ),
            Self::PartWithComponents(product) => {
                write!(f, "part {product} claims components (only assemblies may)")
            }
            Self::Tree(error) => write!(f, "{error}"),
        }
    }
}

fn decode(records: &ffi::StepTreeRecords) -> Result<ProductTree, DecodeError> {
    let refs = dedupe_refs(&records.products);
    let products = records
        .products
        .iter()
        .zip(&refs)
        .map(|(record, product)| decode_product(records, &refs, record, product))
        .collect::<Result<Vec<_>, _>>()?;
    let roots = records
        .roots
        .iter()
        .map(|&index| resolve(&refs, index))
        .collect::<Result<Vec<_>, _>>()?;
    ProductTree::new(products, roots).map_err(DecodeError::Tree)
}

fn decode_product(
    records: &ffi::StepTreeRecords,
    refs: &[ProductRef],
    record: &ffi::StepProductRecord,
    product: &ProductRef,
) -> Result<ProductNode, DecodeError> {
    let first = record.first_component as usize;
    let component_records = records
        .components
        .get(first..first + record.component_count as usize)
        .ok_or_else(|| DecodeError::ComponentRangeOutOfRange {
            product: product.clone(),
            first: record.first_component,
            count: record.component_count,
            component_count: records.components.len(),
        })?;
    let kind = if record.is_assembly {
        let components = component_records
            .iter()
            .map(|component| decode_component(refs, component))
            .collect::<Result<Vec<_>, _>>()?;
        ProductKind::Assembly { components }
    } else if component_records.is_empty() {
        ProductKind::Part {
            solid_count: record.solid_count,
        }
    } else {
        return Err(DecodeError::PartWithComponents(product.clone()));
    };
    Ok(ProductNode {
        product: product.clone(),
        kind,
    })
}

fn decode_component(
    refs: &[ProductRef],
    record: &ffi::StepComponentRecord,
) -> Result<Component, DecodeError> {
    Ok(Component {
        product: resolve(refs, record.product_index)?,
        instance_name: record.instance_name.clone(),
        location: placement(record),
    })
}

fn resolve(refs: &[ProductRef], index: u32) -> Result<ProductRef, DecodeError> {
    refs.get(index as usize)
        .cloned()
        .ok_or(DecodeError::ProductIndexOutOfRange {
            index,
            product_count: refs.len(),
        })
}

/// Index-aligned refs: each record's dedupe index is one more than the number
/// of EARLIER records with the same name (traversal order, PRD Q4).
fn dedupe_refs(products: &[ffi::StepProductRecord]) -> Vec<ProductRef> {
    let mut seen: HashMap<&str, u32> = HashMap::new();
    products
        .iter()
        .map(|record| {
            let count = seen.entry(record.name.as_str()).or_insert(0);
            *count += 1;
            ProductRef {
                name: record.name.clone(),
                dedupe_index: *count,
            }
        })
        .collect()
}

fn placement(record: &ffi::StepComponentRecord) -> Placement {
    let r = record.rotation;
    Placement {
        translation: record.translation,
        rotation: [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]],
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

    fn assembly_record(name: &str, first: u32, count: u32) -> ffi::StepProductRecord {
        ffi::StepProductRecord {
            name: name.to_string(),
            is_assembly: true,
            solid_count: 0,
            first_component: first,
            component_count: count,
        }
    }

    fn part_record(name: &str, solid_count: u32) -> ffi::StepProductRecord {
        ffi::StepProductRecord {
            name: name.to_string(),
            is_assembly: false,
            solid_count,
            first_component: 0,
            component_count: 0,
        }
    }

    fn component_record(product_index: u32, instance_name: &str) -> ffi::StepComponentRecord {
        ffi::StepComponentRecord {
            product_index,
            instance_name: instance_name.to_string(),
            rotation: [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            translation: [0.1, 0.2, 0.3],
        }
    }

    /// `Top` places two `Pin` products that share a name.
    fn records() -> ffi::StepTreeRecords {
        ffi::StepTreeRecords {
            status: ffi::StepReadStatus::Read,
            products: vec![
                assembly_record("Top", 0, 2),
                part_record("Pin", 1),
                part_record("Pin", 2),
            ],
            components: vec![component_record(1, "Pin-1"), component_record(2, "Pin-2")],
            roots: vec![0],
        }
    }

    #[test]
    fn well_formed_records_decode_to_a_tree() {
        let tree = decode(&records()).expect("well-formed records decode");

        assert_eq!(tree.roots(), &[pref("Top", 1)]);
        let top = tree.product(&pref("Top", 1)).expect("Top is decoded");
        let targets: Vec<&ProductRef> = top.components().iter().map(|c| &c.product).collect();
        assert_eq!(targets, [&pref("Pin", 1), &pref("Pin", 2)]);
        assert_eq!(
            top.components()[0].location.rotation,
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
        );
        assert_eq!(
            tree.product(&pref("Pin", 2)).map(|node| &node.kind),
            Some(&ProductKind::Part { solid_count: 2 })
        );
    }

    #[test]
    fn a_component_slice_past_the_component_records_is_rejected() {
        let mut records = records();
        records.products[0] = assembly_record("Top", 1, 2);

        assert_eq!(
            decode(&records),
            Err(DecodeError::ComponentRangeOutOfRange {
                product: pref("Top", 1),
                first: 1,
                count: 2,
                component_count: 2,
            })
        );
    }

    #[test]
    fn a_component_naming_an_absent_product_index_is_rejected() {
        let mut records = records();
        records.components[1].product_index = 7;

        assert_eq!(
            decode(&records),
            Err(DecodeError::ProductIndexOutOfRange {
                index: 7,
                product_count: 3,
            })
        );
    }

    #[test]
    fn a_root_naming_an_absent_product_index_is_rejected() {
        let mut records = records();
        records.roots.push(3);

        assert_eq!(
            decode(&records),
            Err(DecodeError::ProductIndexOutOfRange {
                index: 3,
                product_count: 3,
            })
        );
    }

    #[test]
    fn a_part_record_claiming_components_is_rejected() {
        let mut records = records();
        records.products[1].component_count = 1;

        assert_eq!(
            decode(&records),
            Err(DecodeError::PartWithComponents(pref("Pin", 1)))
        );
    }

    #[test]
    fn records_violating_a_tree_invariant_surface_the_tree_error() {
        let mut records = records();
        records.components[0].product_index = 0;

        assert_eq!(
            decode(&records),
            Err(DecodeError::Tree(ProductTreeError::Cycle(pref("Top", 1))))
        );
    }
}
