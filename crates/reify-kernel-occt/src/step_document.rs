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
            ProductKind::Assembly => return Err(StepBodyError::NotAPart(product.clone())),
            ProductKind::Part { solid_count } => solid_count,
        };
        if body_index >= solid_count {
            return Err(StepBodyError::BodyIndexOutOfRange {
                product: product.clone(),
                index: body_index,
                solid_count,
            });
        }
        // The tree and the native document come from one walk, so a validated
        // request failing here is a reader defect, not a user error.
        let product_index =
            u32::try_from(position).expect("product positions come from u32 walk indices");
        let shape = ffi::step_document_body(&self.native, product_index, body_index)
            .unwrap_or_else(|exception| {
                panic!(
                    "STEP document {}: body {body_index} of validated product {product} is \
                     missing from the native walk: {exception}",
                    self.path.display()
                )
            });
        Ok(shape)
    }
}

/// The process-global `xstep.cascade.unit` static, read under the XSTEP lock,
/// so tests can observe that [`StepDocument::read`] restores it.
#[cfg(feature = "test-fixtures")]
pub fn xstep_cascade_unit_for_test() -> String {
    ffi::xstep_cascade_unit_for_test().expect("read the xstep.cascade.unit static")
}

/// Why the reader's flat records do not form a valid [`ProductTree`].
#[derive(Debug)]
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
            Self::Tree(error) => write!(f, "{error}"),
        }
    }
}

fn decode(records: &ffi::StepTreeRecords) -> Result<ProductTree, DecodeError> {
    let refs = dedupe_refs(&records.products);
    let product_ref = |index: u32| {
        refs.get(index as usize)
            .cloned()
            .ok_or(DecodeError::ProductIndexOutOfRange {
                index,
                product_count: refs.len(),
            })
    };
    let products = records
        .products
        .iter()
        .zip(&refs)
        .map(|(record, product)| {
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
            let components = component_records
                .iter()
                .map(|component| {
                    Ok(Component {
                        product: product_ref(component.product_index)?,
                        instance_name: component.instance_name.clone(),
                        location: placement(component),
                    })
                })
                .collect::<Result<Vec<_>, DecodeError>>()?;
            Ok(ProductNode {
                name: record.name.clone(),
                dedupe_index: product.dedupe_index,
                kind: if record.is_assembly {
                    ProductKind::Assembly
                } else {
                    ProductKind::Part {
                        solid_count: record.solid_count,
                    }
                },
                components,
            })
        })
        .collect::<Result<Vec<_>, DecodeError>>()?;
    let roots = records
        .roots
        .iter()
        .map(|&index| product_ref(index))
        .collect::<Result<Vec<_>, _>>()?;
    ProductTree::new(products, roots).map_err(DecodeError::Tree)
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
