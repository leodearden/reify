//! STEP assembly import through OCCT XDE (PRD docs/prds/v0_6/step-assembly-import.md
//! §5 C1, boundary signal B1): the plain product tree, metre units, typed read
//! failures, process-global unit-static hygiene, and each product's solids as
//! kernel handles in product-local coordinates.
#![cfg(all(has_occt, feature = "test-fixtures"))]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use reify_ir::{
    BRepKind, Component, GeometryHandle, GeometryQuery, Placement, ProductKind, ProductNode,
    ProductRef, ProductTree, Value,
};
use reify_kernel_occt::{
    OcctKernel, StepBodyError, StepDocument, StepReadError, xstep_cascade_unit_for_test,
};

const LOCATION_TOL: f64 = 1e-9;

/// Runtime `CARGO_MANIFEST_DIR` first: the compile-time bake goes stale when a
/// warm-lane `target/` is reused from another worktree (esc-4906-57).
fn fixture(name: &str) -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
    Path::new(&manifest_dir).join("tests/fixtures").join(name)
}

fn read_fixture(name: &str) -> StepDocument {
    let path = fixture(name);
    StepDocument::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn pref(name: &str, dedupe_index: u32) -> ProductRef {
    ProductRef {
        name: name.to_string(),
        dedupe_index,
    }
}

fn node<'t>(tree: &'t ProductTree, name: &str, dedupe_index: u32) -> &'t ProductNode {
    tree.product(&pref(name, dedupe_index))
        .unwrap_or_else(|| panic!("product {name}#{dedupe_index} is missing from the tree"))
}

fn instance_names(components: &[Component]) -> Vec<&str> {
    components
        .iter()
        .map(|component| component.instance_name.as_str())
        .collect()
}

fn assert_vec_close(actual: [f64; 3], expected: [f64; 3], tol: f64, what: &str) {
    for i in 0..3 {
        assert!(
            (actual[i] - expected[i]).abs() <= tol,
            "{what}: component {i} is {actual:?}, expected {expected:?} (tol {tol})"
        );
    }
}

fn assert_mat_close(actual: [[f64; 3]; 3], expected: [[f64; 3]; 3], tol: f64, what: &str) {
    for r in 0..3 {
        for c in 0..3 {
            assert!(
                (actual[r][c] - expected[r][c]).abs() <= tol,
                "{what}: entry ({r},{c}) of {actual:?} differs from {expected:?} (tol {tol})"
            );
        }
    }
}

fn assert_placement(component: &Component, rotation: [[f64; 3]; 3], translation: [f64; 3]) {
    let what = &component.instance_name;
    assert_mat_close(
        component.location.rotation,
        rotation,
        LOCATION_TOL,
        &format!("{what} rotation"),
    );
    assert_vec_close(
        component.location.translation,
        translation,
        LOCATION_TOL,
        &format!("{what} translation"),
    );
}

fn rz(degrees: f64) -> [[f64; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn rx(degrees: f64) -> [[f64; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

const IDENTITY: [[f64; 3]; 3] = Placement::IDENTITY.rotation;

#[test]
fn small_fixture_product_tree() {
    let doc = read_fixture("step_assembly_small.step");
    let tree = doc.tree();

    assert_eq!(tree.roots(), &[pref("Container", 1)]);
    let order: Vec<(&str, u32)> = tree
        .products()
        .iter()
        .map(|p| (p.name.as_str(), p.dedupe_index))
        .collect();
    assert_eq!(
        order,
        [
            ("Container", 1),
            ("CornerCasting", 1),
            ("SideWall", 1),
            ("Panel", 1),
            ("Rail", 1),
            ("Weldment", 1),
        ],
        "DFS first-visit order with one shared CornerCasting product"
    );

    let container = node(tree, "Container", 1);
    assert_eq!(container.kind, ProductKind::Assembly);
    assert_eq!(
        instance_names(&container.components),
        [
            "CornerCasting-1",
            "CornerCasting-2",
            "CornerCasting-3",
            "CornerCasting-4",
            "SideWall-1",
            "Weldment-1",
        ]
    );
    let casting_translations = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.8, 0.0],
        [1.0, 0.8, 0.0],
    ];
    for (component, translation) in container.components[..4].iter().zip(casting_translations) {
        assert_eq!(component.product, pref("CornerCasting", 1));
        assert_placement(component, IDENTITY, translation);
    }
    assert_eq!(container.components[4].product, pref("SideWall", 1));
    assert_placement(&container.components[4], IDENTITY, [0.0, 0.0, 0.5]);
    assert_eq!(container.components[5].product, pref("Weldment", 1));

    let side_wall = node(tree, "SideWall", 1);
    assert_eq!(side_wall.kind, ProductKind::Assembly);
    assert_eq!(instance_names(&side_wall.components), ["Panel-1", "Rail-1"]);
    assert_eq!(side_wall.components[0].product, pref("Panel", 1));
    assert_placement(&side_wall.components[0], IDENTITY, [0.0, 0.0, 0.0]);
    assert_eq!(side_wall.components[1].product, pref("Rail", 1));
    assert_placement(&side_wall.components[1], IDENTITY, [0.0, 0.3, 0.0]);

    for name in ["CornerCasting", "Panel", "Rail"] {
        assert_eq!(
            node(tree, name, 1).kind,
            ProductKind::Part { solid_count: 1 },
            "{name}"
        );
        assert!(node(tree, name, 1).components.is_empty(), "{name}");
    }
    assert_eq!(
        node(tree, "Weldment", 1).kind,
        ProductKind::Part { solid_count: 2 }
    );
}

#[test]
fn rotated_occurrence_and_subassembly_rotations_round_trip() {
    let doc = read_fixture("step_assembly_rotated.step");
    let tree = doc.tree();

    let frame = node(tree, "Frame", 1);
    assert_eq!(
        instance_names(&frame.components),
        ["Bracket-1", "Hinge-1", "Pin-2", "Label-1"]
    );
    assert_eq!(frame.components[0].product, pref("Bracket", 1));
    assert_placement(&frame.components[0], rz(90.0), [0.2, 0.0, 0.0]);
    assert_eq!(frame.components[1].product, pref("Hinge", 1));
    assert_placement(&frame.components[1], rx(30.0), [0.0, 0.1, 0.05]);

    // Parent-relative: NOT composed with Hinge-1's Rx(30) placement.
    let hinge = node(tree, "Hinge", 1);
    assert_eq!(hinge.kind, ProductKind::Assembly);
    assert_eq!(instance_names(&hinge.components), ["Pin-1"]);
    assert_placement(&hinge.components[0], rz(45.0), [0.005, 0.0, 0.0]);
}

#[test]
fn duplicate_product_names_get_traversal_order_dedupe_indices() {
    let doc = read_fixture("step_assembly_rotated.step");
    let tree = doc.tree();

    let pins: Vec<u32> = tree
        .products()
        .iter()
        .filter(|p| p.name == "Pin")
        .map(|p| p.dedupe_index)
        .collect();
    assert_eq!(pins, [1, 2], "two distinct products named Pin");

    // Hinge (Frame's 2nd component) is walked before Frame's Pin-2, so the Pin
    // inside Hinge is visited first and becomes #1.
    assert_eq!(node(tree, "Hinge", 1).components[0].product, pref("Pin", 1));
    let frame = node(tree, "Frame", 1);
    let pin_2 = frame
        .components
        .iter()
        .find(|c| c.instance_name == "Pin-2")
        .expect("Frame has a Pin-2 component");
    assert_eq!(pin_2.product, pref("Pin", 2));
    assert_placement(pin_2, IDENTITY, [0.3, 0.3, 0.0]);
}

#[test]
fn every_free_root_is_reported() {
    let doc = read_fixture("step_assembly_rotated.step");
    let tree = doc.tree();

    assert_eq!(tree.roots(), &[pref("Frame", 1), pref("Spare", 1)]);
    let spare = node(tree, "Spare", 1);
    assert_eq!(spare.kind, ProductKind::Part { solid_count: 1 });
    assert!(spare.components.is_empty());
}

#[test]
fn zero_solid_product_is_reported() {
    let doc = read_fixture("step_assembly_rotated.step");
    assert_eq!(
        node(doc.tree(), "Label", 1).kind,
        ProductKind::Part { solid_count: 0 }
    );
}

const ROOTLESS_STEP: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('rootless'),'2;1');
FILE_NAME('rootless.step','2026-10-02T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
ENDSEC;
END-ISO-10303-21;
";

fn read_error(path: &Path) -> StepReadError {
    match StepDocument::read(path) {
        Ok(_) => panic!("{} read successfully; expected an error", path.display()),
        Err(error) => error,
    }
}

fn assert_names_path(error: &StepReadError, path: &Path) {
    let message = error.to_string();
    assert!(
        message.contains(&path.display().to_string()),
        "error message {message:?} does not name {}",
        path.display()
    );
}

#[test]
fn unreadable_and_rootless_files_are_typed_errors_naming_the_file() {
    let dir = reify_test_support::prefixed_tempdir("reify-step-document-");

    let missing = dir.path().join("does-not-exist.step");
    let error = read_error(&missing);
    assert!(
        matches!(&error, StepReadError::Unreadable { path } if path == &missing),
        "nonexistent file: {error:?}"
    );
    assert_names_path(&error, &missing);

    let garbage = dir.path().join("garbage.step");
    std::fs::write(&garbage, "this is not a step file").unwrap();
    let error = read_error(&garbage);
    assert!(
        matches!(&error, StepReadError::Unreadable { path } if path == &garbage),
        "garbage file: {error:?}"
    );
    assert_names_path(&error, &garbage);

    let rootless = dir.path().join("rootless.step");
    std::fs::write(&rootless, ROOTLESS_STEP).unwrap();
    let error = read_error(&rootless);
    assert!(
        matches!(&error, StepReadError::NoRoots { path } if path == &rootless),
        "rootless file: {error:?}"
    );
    assert_names_path(&error, &rootless);
}

#[test]
fn reader_restores_the_cascade_unit_static() {
    let before = xstep_cascade_unit_for_test();
    let doc = read_fixture("step_assembly_small.step");
    let after = xstep_cascade_unit_for_test();

    assert_eq!(doc.path(), fixture("step_assembly_small.step"));
    assert_eq!(after, before, "the reader must restore xstep.cascade.unit");
    assert_ne!(after, "M", "the reader sets metres for its own read only");
}

/// File names of every shared object mapped into this process.
fn mapped_library_names() -> BTreeSet<String> {
    let maps = std::fs::read_to_string("/proc/self/maps").expect("read /proc/self/maps");
    maps.lines()
        .filter_map(|line| line.split_whitespace().nth(5))
        .filter_map(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn xde_reader_runs_inside_the_gmsh_linked_binary() {
    let libraries = mapped_library_names();
    let gmsh_mapped = libraries.iter().any(|name| name.starts_with("libgmsh.so"));
    let tkxcaf: Vec<&String> = libraries
        .iter()
        .filter(|name| name.starts_with("libTKXCAF.so."))
        .collect();
    assert!(
        gmsh_mapped && !tkxcaf.is_empty(),
        "the B1 signal must run in a gmsh-linked binary: libgmsh loads TKXCAF/TKLCAF 7.9 \
         beside the OCCT 7.8 reify links (docs/prds/v0_6/step-assembly-import.md §3.1), and \
         this reader must be proven in that environment. If this binary no longer links gmsh, \
         move this module to a gmsh-linked harness rather than deleting this check. \
         libgmsh mapped: {gmsh_mapped}; TKXCAF sonames mapped: {tkxcaf:?}"
    );

    let doc = read_fixture("step_assembly_small.step");
    assert_eq!(doc.tree().roots(), &[pref("Container", 1)]);
}

/// BRepBndLib enlarges every box by Precision::Confusion (1e-7 m).
const BBOX_TOL: f64 = 1e-6;

/// `[xmin, ymin, zmin, xmax, ymax, zmax]` through the public query path.
fn bbox(kernel: &OcctKernel, handle: &GeometryHandle) -> [f64; 6] {
    let value = kernel
        .query(&GeometryQuery::BoundingBox(handle.id))
        .expect("bounding-box query");
    let Value::String(text) = value else {
        panic!("expected a JSON string, got {value:?}");
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {text:?}: {e}"));
    ["xmin", "ymin", "zmin", "xmax", "ymax", "zmax"].map(|key| {
        parsed[key]
            .as_f64()
            .unwrap_or_else(|| panic!("{key} missing from {text}"))
    })
}

fn assert_bbox(actual: [f64; 6], expected: [f64; 6], what: &str) {
    for i in 0..6 {
        assert!(
            (actual[i] - expected[i]).abs() <= BBOX_TOL,
            "{what}: bbox {actual:?}, expected {expected:?} (tol {BBOX_TOL})"
        );
    }
}

fn extent(b: [f64; 6]) -> [f64; 3] {
    [b[3] - b[0], b[4] - b[1], b[5] - b[2]]
}

fn import_body(
    kernel: &mut OcctKernel,
    doc: &StepDocument,
    product: &ProductRef,
    body_index: u32,
) -> GeometryHandle {
    kernel
        .import_step_body(doc, product, body_index)
        .unwrap_or_else(|e| panic!("import {product} body {body_index}: {e}"))
}

#[test]
fn casting_body_is_in_metres() {
    let doc = read_fixture("step_assembly_small.step");
    let mut kernel = OcctKernel::new();

    let casting = import_body(&mut kernel, &doc, &pref("CornerCasting", 1), 0);
    assert_bbox(
        bbox(&kernel, &casting),
        [0.0, 0.0, 0.0, 0.178, 0.162, 0.118],
        "CornerCasting body 0",
    );
    assert_eq!(kernel.repr_of(casting.id), Some(BRepKind::Solid));
}

#[test]
fn multi_body_product_exposes_each_solid() {
    let doc = read_fixture("step_assembly_small.step");
    let mut kernel = OcctKernel::new();
    let weldment = pref("Weldment", 1);

    let first = import_body(&mut kernel, &doc, &weldment, 0);
    assert_bbox(
        bbox(&kernel, &first),
        [0.0, 0.0, 0.0, 0.05, 0.05, 0.05],
        "Weldment body 0",
    );
    let second = import_body(&mut kernel, &doc, &weldment, 1);
    assert_bbox(
        bbox(&kernel, &second),
        [0.2, 0.0, 0.0, 0.23, 0.03, 0.03],
        "Weldment body 1",
    );
    assert_eq!(
        kernel.import_step_body(&doc, &weldment, 2).err(),
        Some(StepBodyError::BodyIndexOutOfRange {
            product: weldment.clone(),
            index: 2,
            solid_count: 2,
        })
    );
}

#[test]
fn product_body_is_product_local_not_occurrence_placed() {
    let doc = read_fixture("step_assembly_rotated.step");
    let mut kernel = OcctKernel::new();

    // The placed Bracket-1 occurrence (Rz90 at x = 0.2) would span
    // (0.15, 0, 0)-(0.2, 0.1, 0.02); the product itself is unrotated.
    let bracket = import_body(&mut kernel, &doc, &pref("Bracket", 1), 0);
    assert_bbox(
        bbox(&kernel, &bracket),
        [0.0, 0.0, 0.0, 0.1, 0.05, 0.02],
        "Bracket body 0",
    );
}

#[test]
fn dedupe_indices_address_distinct_products() {
    let doc = read_fixture("step_assembly_rotated.step");
    let mut kernel = OcctKernel::new();

    let pin_1 = import_body(&mut kernel, &doc, &pref("Pin", 1), 0);
    assert_vec_close(
        extent(bbox(&kernel, &pin_1)),
        [0.01, 0.01, 0.06],
        BBOX_TOL,
        "Pin#1 extent",
    );
    let pin_2 = import_body(&mut kernel, &doc, &pref("Pin", 2), 0);
    assert_vec_close(
        extent(bbox(&kernel, &pin_2)),
        [0.008, 0.008, 0.04],
        BBOX_TOL,
        "Pin#2 extent",
    );
}

#[test]
fn body_access_errors_are_typed() {
    let doc = read_fixture("step_assembly_rotated.step");
    let mut kernel = OcctKernel::new();
    let cases = [
        (
            pref("Label", 1),
            StepBodyError::BodyIndexOutOfRange {
                product: pref("Label", 1),
                index: 0,
                solid_count: 0,
            },
        ),
        (pref("Hinge", 1), StepBodyError::NotAPart(pref("Hinge", 1))),
        (
            pref("Ghost", 1),
            StepBodyError::UnknownProduct(pref("Ghost", 1)),
        ),
    ];
    for (product, expected) in cases {
        let shapes_before = kernel.shape_count();
        assert_eq!(
            kernel.import_step_body(&doc, &product, 0).err(),
            Some(expected),
            "{product} body 0"
        );
        assert_eq!(
            kernel.shape_count(),
            shapes_before,
            "a failed import of {product} must store nothing"
        );
    }
}
