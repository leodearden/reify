//! BT-ζ1 (task 8669; PRD docs/prds/v0_6/occt-crate-internals.md §4 D4): the
//! result kind every `execute` arm stamps, read on BOTH entry points —
//! `execute` and `execute_with_history` — through `handle.repr` and
//! `repr_of`, so the two paths cannot disagree about what a result IS.
//!
//! A transform keeps its input's kind; a sweep, pattern, boolean or local
//! feature is classified from the shape it actually built; a constructor
//! keeps its fixed kind. SweepGuided and LoftGuided are covered by
//! `harness_occt_measurement/sweep_guided_integration.rs` and
//! `loft_guided_integration.rs`. Draft's all-faces form has no row: it fails
//! on every closed solid probed (ticket tkt_0RW01PW74XY9CPGF4A4TEBCZVS). Nor
//! does a revolved wire: the revolve wrapper wraps its open shell in a SOLID
//! (ticket tkt_0RW02D1V7Z761QCHHGYSMJ674C).
//!
//! Lengths are SI metres. `GeometryOp::Box` is centred on the origin.

#![cfg(has_occt)]

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8, TAU};

use reify_ir::{BRepKind, GeometryError, GeometryHandle, GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

/// One kernel plus every row that failed its expectation, so a single run
/// reports the whole table rather than its first mismatch.
struct Table {
    kernel: OcctKernel,
    failures: Vec<String>,
}

impl Table {
    fn new() -> Self {
        Self {
            kernel: OcctKernel::new(),
            failures: Vec::new(),
        }
    }

    /// Build an input through `execute`; an input that fails is a broken
    /// fixture, not a table row, so it panics at once.
    fn input(&mut self, what: &str, op: GeometryOp) -> GeometryHandleId {
        self.kernel
            .execute(&op)
            .unwrap_or_else(|e| panic!("input {what} must build: {e}"))
            .id
    }

    fn edge_of(&mut self, solid: GeometryHandleId) -> GeometryHandleId {
        self.kernel.extract_edges(solid).expect("extract_edges")[0]
    }

    fn face_of(&mut self, solid: GeometryHandleId) -> GeometryHandleId {
        self.kernel.extract_faces(solid).expect("extract_faces")[0]
    }

    /// `op` must stamp `expected` through `execute` AND `execute_with_history`.
    fn row(&mut self, row: &str, op: &GeometryOp, expected: BRepKind) {
        self.row_on_fresh_input(row, |_| op.clone(), expected);
    }

    /// As [`Self::row`], with `op_on_fresh_input` called once per entry point,
    /// for an op that OCCT applies only once to a given input: a second
    /// `Shell` of the same solid fails "BRep_API: command not done".
    fn row_on_fresh_input(
        &mut self,
        row: &str,
        op_on_fresh_input: impl Fn(&mut Self) -> GeometryOp,
        expected: BRepKind,
    ) {
        let op = op_on_fresh_input(self);
        let plain = self.kernel.execute(&op);
        self.check(row, "execute", plain, expected);
        let op = op_on_fresh_input(self);
        let with_history = self.kernel.execute_with_history(&op).map(|(h, _)| h);
        self.check(row, "execute_with_history", with_history, expected);
    }

    fn check(
        &mut self,
        row: &str,
        entry: &str,
        result: Result<GeometryHandle, GeometryError>,
        expected: BRepKind,
    ) {
        match result {
            Err(e) => self
                .failures
                .push(format!("{row} via {entry}: failed: {e}")),
            Ok(handle) => {
                let stored = self.kernel.repr_of(handle.id);
                if handle.repr != Some(expected) || stored != Some(expected) {
                    self.failures.push(format!(
                        "{row} via {entry}: expected {expected:?}, got handle.repr = {:?}, \
                         repr_of = {stored:?}",
                        handle.repr
                    ));
                }
            }
        }
    }

    /// `op` must fail fillet/chamfer's Solid-input precondition, naming
    /// `input_kind`, with the same text through both entry points.
    fn rejected(&mut self, row: &str, op: &GeometryOp, input_kind: BRepKind) {
        let expected = format!(
            "local-feature operation requires a BRepKind::Solid input shape, got {input_kind:?}"
        );
        let plain = self.kernel.execute(op).map(|h| h.id);
        let with_history = self.kernel.execute_with_history(op).map(|(h, _)| h.id);
        let agreed = match (&plain, &with_history) {
            (Err(plain_err), Err(history_err)) => {
                matches!(plain_err, GeometryError::OperationFailed(msg) if msg.contains(&expected))
                    && history_err.to_string() == plain_err.to_string()
            }
            _ => false,
        };
        if !agreed {
            self.failures.push(format!(
                "{row}: both entry points must fail with {expected:?}; \
                 execute = {plain:?}, execute_with_history = {with_history:?}"
            ));
        }
    }

    fn finish(self) {
        assert!(
            self.failures.is_empty(),
            "{} row(s) failed:\n  {}",
            self.failures.len(),
            self.failures.join("\n  ")
        );
    }
}

/// The four inputs every family of rows draws on.
struct Inputs {
    face: GeometryHandleId,
    wire: GeometryHandleId,
    solid: GeometryHandleId,
    disjoint_compound: GeometryHandleId,
}

fn box_op(side: f64) -> GeometryOp {
    GeometryOp::Box {
        width: Value::length(side),
        height: Value::length(side),
        depth: Value::length(side),
    }
}

fn translate(target: GeometryHandleId, dx: f64, dy: f64, dz: f64) -> GeometryOp {
    GeometryOp::Translate { target, dx, dy, dz }
}

fn segment(from: [f64; 3], to: [f64; 3]) -> GeometryOp {
    GeometryOp::LineSegment {
        x1: from[0],
        y1: from[1],
        z1: from[2],
        x2: to[0],
        y2: to[1],
        z2: to[2],
    }
}

fn square_face_op() -> GeometryOp {
    GeometryOp::RectangleProfile {
        width: Value::length(0.02),
        height: Value::length(0.02),
    }
}

fn inputs(t: &mut Table) -> Inputs {
    let face = t.input("FACE", square_face_op());
    let wire = t.input("WIRE", segment([0.0; 3], [0.02, 0.0, 0.0]));
    let solid = t.input("SOLID", box_op(0.01));
    let far = t.input("far box", box_op(0.01));
    let far = t.input("far box moved", translate(far, 0.1, 0.0, 0.0));
    let disjoint_compound = t.input(
        "DISJOINT COMPOUND",
        GeometryOp::Union {
            left: solid,
            right: far,
        },
    );
    assert_eq!(
        t.kernel.repr_of(disjoint_compound),
        Some(BRepKind::Compound),
        "premise: a union of two disjoint boxes is a Compound"
    );
    Inputs {
        face,
        wire,
        solid,
        disjoint_compound,
    }
}

fn transforms_of(target: GeometryHandleId) -> Vec<(&'static str, GeometryOp)> {
    vec![
        ("Translate", translate(target, 0.01, 0.0, 0.0)),
        (
            "Rotate",
            GeometryOp::Rotate {
                target,
                axis: [0.0, 0.0, 1.0],
                angle_rad: FRAC_PI_4,
            },
        ),
        (
            "Mirror",
            GeometryOp::Mirror {
                target,
                plane_origin: [0.0; 3],
                plane_normal: [1.0, 0.0, 0.0],
            },
        ),
        (
            "Scale",
            GeometryOp::Scale {
                target,
                factor: 2.0,
            },
        ),
        (
            "RotateAround",
            GeometryOp::RotateAround {
                target,
                point: [0.01, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                angle_rad: FRAC_PI_4,
            },
        ),
        (
            "AffineApply",
            GeometryOp::AffineApply {
                target,
                linear: [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 1.0]],
                translation: [0.0; 3],
            },
        ),
        (
            "ScaleNonUniform",
            GeometryOp::ScaleNonUniform {
                target,
                sx: 1.0,
                sy: 2.0,
                sz: 1.0,
            },
        ),
        (
            "ApplyTransform",
            GeometryOp::ApplyTransform {
                target,
                rotation: [FRAC_PI_8.cos(), 0.0, 0.0, FRAC_PI_8.sin()],
                translation: [0.01, 0.0, 0.0],
            },
        ),
    ]
}

#[test]
fn transforms_inherit_their_input_kind() {
    let mut t = Table::new();
    let i = inputs(&mut t);
    let cases = [
        ("FACE", i.face, BRepKind::Face),
        ("WIRE", i.wire, BRepKind::Wire),
        ("SOLID", i.solid, BRepKind::Solid),
        ("DISJOINT COMPOUND", i.disjoint_compound, BRepKind::Compound),
    ];
    for (input, target, kind) in cases {
        for (op_label, op) in transforms_of(target) {
            t.row(&format!("{op_label} × {input}"), &op, kind);
        }
    }
    t.finish();
}

#[test]
fn sweeps_classify_a_face_profile_as_solid_and_a_wire_profile_as_shell() {
    let mut t = Table::new();
    let i = inputs(&mut t);
    let revolve_face = t.input(
        "FACE moved off the Y axis",
        translate(i.face, 0.05, 0.0, 0.0),
    );
    let path = t.input("+Z path", segment([0.0; 3], [0.0, 0.0, 0.02]));
    let raised_face = t.input("FACE raised 0.01", translate(i.face, 0.0, 0.0, 0.01));

    let profiles = [
        ("FACE", i.face, BRepKind::Solid),
        ("WIRE", i.wire, BRepKind::Shell),
    ];
    for (input, profile, kind) in profiles {
        let distance = Value::length(0.01);
        let sweeps = [
            (
                "Extrude",
                GeometryOp::Extrude {
                    profile,
                    distance: distance.clone(),
                },
            ),
            (
                "ExtrudeSymmetric",
                GeometryOp::ExtrudeSymmetric { profile, distance },
            ),
            (
                "ExtrudeInfinite",
                GeometryOp::ExtrudeInfinite {
                    profile,
                    axis: [0.0, 0.0, 1.0],
                    both: false,
                },
            ),
            ("Sweep", GeometryOp::Sweep { profile, path }),
        ];
        for (op_label, op) in sweeps {
            t.row(&format!("{op_label} × {input}"), &op, kind);
        }
    }

    let revolve = GeometryOp::Revolve {
        profile: revolve_face,
        axis_origin: [0.0; 3],
        axis_dir: [0.0, 1.0, 0.0],
        angle_rad: FRAC_PI_2,
    };
    t.row("Revolve × FACE", &revolve, BRepKind::Solid);
    let pipe = GeometryOp::Pipe {
        path,
        radius: Value::length(0.002),
    };
    t.row("Pipe × +Z path", &pipe, BRepKind::Solid);
    let loft = GeometryOp::Loft {
        profiles: vec![i.face, raised_face],
    };
    t.row("Loft × two FACEs", &loft, BRepKind::Solid);
    t.finish();
}

#[test]
fn patterns_classify_overlapping_copies_as_solid_and_disjoint_copies_as_compound() {
    let mut t = Table::new();
    let i = inputs(&mut t);
    let off_axis = t.input(
        "SOLID moved off the Z axis",
        translate(i.solid, 0.05, 0.0, 0.0),
    );

    for (layout, spacing, kind) in [
        ("overlapping", 0.005, BRepKind::Solid),
        ("disjoint", 0.02, BRepKind::Compound),
    ] {
        let linear = GeometryOp::LinearPattern {
            target: i.solid,
            direction: [1.0, 0.0, 0.0],
            count: 3,
            spacing: Value::length(spacing),
        };
        t.row(&format!("LinearPattern × {layout}"), &linear, kind);
        let grid = GeometryOp::LinearPattern2D {
            target: i.solid,
            direction1: [1.0, 0.0, 0.0],
            count1: 2,
            spacing1: Value::length(spacing),
            direction2: [0.0, 1.0, 0.0],
            count2: 2,
            spacing2: Value::length(spacing),
        };
        t.row(&format!("LinearPattern2D × {layout}"), &grid, kind);
    }

    for (layout, target, kind) in [
        ("origin-centred", i.solid, BRepKind::Solid),
        ("off-axis", off_axis, BRepKind::Compound),
    ] {
        let circular = GeometryOp::CircularPattern {
            target,
            axis_origin: [0.0; 3],
            axis_dir: [0.0, 0.0, 1.0],
            count: 4,
            angle: Value::Real(TAU),
        };
        t.row(&format!("CircularPattern × {layout}"), &circular, kind);
    }

    for (layout, offset, kind) in [
        ("overlapping", 0.005, BRepKind::Solid),
        ("disjoint", 0.05, BRepKind::Compound),
    ] {
        let arbitrary = GeometryOp::ArbitraryPattern {
            target: i.solid,
            transforms: vec![([1.0, 0.0, 0.0, 0.0], [offset, 0.0, 0.0])],
        };
        t.row(&format!("ArbitraryPattern × {layout}"), &arbitrary, kind);
    }
    t.finish();
}

#[test]
fn local_features_classify_their_one_solid_result_as_solid() {
    let mut t = Table::new();
    let i = inputs(&mut t);
    let edge = t.edge_of(i.solid);
    let face = t.face_of(i.solid);
    let small = Value::length(0.001);

    let filleted = t.input(
        "fillet(box)",
        GeometryOp::Fillet {
            target: i.solid,
            edges: vec![edge],
            radius: small.clone(),
        },
    );
    let plane_face = t.input("neutral plane face", square_face_op());
    let neutral_plane = t.input(
        "neutral plane on the box base",
        translate(plane_face, 0.0, 0.0, -0.005),
    );

    let rows = [
        (
            "Fillet all-edges × SOLID",
            GeometryOp::Fillet {
                target: i.solid,
                edges: vec![],
                radius: small.clone(),
            },
        ),
        (
            "Fillet curated × SOLID",
            GeometryOp::Fillet {
                target: i.solid,
                edges: vec![edge],
                radius: small.clone(),
            },
        ),
        (
            "Chamfer all-edges × SOLID",
            GeometryOp::Chamfer {
                target: i.solid,
                edges: vec![],
                distance: small.clone(),
            },
        ),
        (
            "Chamfer curated × SOLID",
            GeometryOp::Chamfer {
                target: i.solid,
                edges: vec![edge],
                distance: small.clone(),
            },
        ),
        (
            "ChamferAsymmetric curated × SOLID",
            GeometryOp::ChamferAsymmetric {
                target: i.solid,
                edges: vec![edge],
                d1: Value::length(0.001),
                d2: Value::length(0.002),
            },
        ),
        (
            "Fillet all-edges × fillet(box)",
            GeometryOp::Fillet {
                target: filleted,
                edges: vec![],
                radius: Value::length(0.0005),
            },
        ),
        (
            "Chamfer all-edges × fillet(box)",
            GeometryOp::Chamfer {
                target: filleted,
                edges: vec![],
                distance: Value::length(0.0005),
            },
        ),
        (
            "Draft curated × SOLID",
            GeometryOp::Draft {
                target: i.solid,
                faces: vec![face],
                angle: Value::angle(0.1),
                plane: neutral_plane,
            },
        ),
        (
            "Thicken × SOLID",
            GeometryOp::Thicken {
                target: i.solid,
                offset: Value::length(0.0005),
            },
        ),
        (
            "OffsetSolid × SOLID",
            GeometryOp::OffsetSolid {
                target: i.solid,
                distance: small,
            },
        ),
        (
            "ZoneSlab × FACE",
            GeometryOp::ZoneSlab {
                target: i.face,
                width: Value::length(0.002),
            },
        ),
    ];
    for (row, op) in rows {
        t.row(row, &op, BRepKind::Solid);
    }

    t.row_on_fresh_input(
        "Shell numeric × SOLID",
        |t| GeometryOp::Shell {
            target: t.input("fresh SOLID", box_op(0.01)),
            thickness: Value::length(0.001),
            faces_to_remove: vec![0],
            open_face_handles: vec![],
        },
        BRepKind::Solid,
    );
    t.row_on_fresh_input(
        "Shell curated × SOLID",
        |t| {
            let target = t.input("fresh SOLID", box_op(0.01));
            GeometryOp::Shell {
                target,
                thickness: Value::length(0.001),
                faces_to_remove: vec![],
                open_face_handles: vec![t.face_of(target)],
            }
        },
        BRepKind::Solid,
    );
    t.finish();
}

#[test]
fn constructors_keep_their_fixed_kind() {
    let mut t = Table::new();
    let len = Value::length;
    let solids = [
        ("Box", box_op(0.01)),
        (
            "Cylinder",
            GeometryOp::Cylinder {
                radius: len(0.005),
                height: len(0.01),
            },
        ),
        ("Sphere", GeometryOp::Sphere { radius: len(0.005) }),
        (
            "Tube",
            GeometryOp::Tube {
                outer_r: len(0.005),
                inner_r: len(0.003),
                height: len(0.01),
            },
        ),
        (
            "Cone",
            GeometryOp::Cone {
                bottom_radius: len(0.005),
                top_radius: len(0.002),
                height: len(0.01),
            },
        ),
        (
            "Wedge",
            GeometryOp::Wedge {
                width: len(0.01),
                depth: len(0.01),
                height: len(0.01),
                top_width: len(0.005),
            },
        ),
        (
            "Torus",
            GeometryOp::Torus {
                major_radius: len(0.01),
                minor_radius: len(0.002),
            },
        ),
        (
            "HalfSpace",
            GeometryOp::HalfSpace {
                px: len(0.0),
                py: len(0.0),
                pz: len(0.0),
                nx: Value::Real(0.0),
                ny: Value::Real(0.0),
                nz: Value::Real(1.0),
            },
        ),
    ];
    for (row, op) in solids {
        t.row(row, &op, BRepKind::Solid);
    }

    let wires = [
        ("LineSegment", segment([0.0; 3], [0.02, 0.0, 0.0])),
        (
            "Arc",
            GeometryOp::Arc {
                center: [0.0; 3],
                radius: 0.01,
                start_angle: 0.0,
                end_angle: FRAC_PI_2,
                axis: [0.0, 0.0, 1.0],
            },
        ),
    ];
    for (row, op) in wires {
        t.row(row, &op, BRepKind::Wire);
    }

    let faces = [
        ("RectangleProfile", square_face_op()),
        (
            "CircleProfile",
            GeometryOp::CircleProfile { radius: len(0.005) },
        ),
        (
            "PolygonProfile",
            GeometryOp::PolygonProfile {
                points: vec![[0.0, 0.0], [0.01, 0.0], [0.0, 0.01]],
            },
        ),
        (
            "EllipseProfile",
            GeometryOp::EllipseProfile {
                semi_major: len(0.01),
                semi_minor: len(0.005),
            },
        ),
    ];
    for (row, op) in faces {
        t.row(row, &op, BRepKind::Face);
    }
    t.finish();
}

#[test]
fn a_union_of_disjoint_solids_classifies_as_compound() {
    let mut t = Table::new();
    let i = inputs(&mut t);
    let far = t.input("far box", box_op(0.01));
    let far = t.input("far box moved", translate(far, 0.1, 0.0, 0.0));
    let union = GeometryOp::Union {
        left: i.solid,
        right: far,
    };
    t.row("Union × disjoint boxes", &union, BRepKind::Compound);
    t.finish();
}

/// D5: fillet and chamfer hold ONE Solid-input precondition per op, so the
/// plain all-edges `execute` path rejects what `execute_with_history` does.
#[test]
fn fillet_and_chamfer_reject_a_non_solid_input_on_both_entry_points() {
    let mut t = Table::new();
    let i = inputs(&mut t);
    let moved = t.input(
        "DISJOINT COMPOUND moved",
        translate(i.disjoint_compound, 0.0, 0.0, 0.01),
    );
    let sheet = t.input(
        "extruded WIRE",
        GeometryOp::Extrude {
            profile: i.wire,
            distance: Value::length(0.01),
        },
    );
    let fillet = |target| GeometryOp::Fillet {
        target,
        edges: vec![],
        radius: Value::length(0.001),
    };
    let cases = [
        (
            "Fillet all-edges × DISJOINT COMPOUND",
            fillet(i.disjoint_compound),
            BRepKind::Compound,
        ),
        (
            "Chamfer all-edges × DISJOINT COMPOUND",
            GeometryOp::Chamfer {
                target: i.disjoint_compound,
                edges: vec![],
                distance: Value::length(0.001),
            },
            BRepKind::Compound,
        ),
        (
            "Fillet all-edges × translated DISJOINT COMPOUND",
            fillet(moved),
            BRepKind::Compound,
        ),
        (
            "Fillet all-edges × extruded WIRE",
            fillet(sheet),
            BRepKind::Shell,
        ),
    ];
    for (row, op, input_kind) in cases {
        t.rejected(row, &op, input_kind);
    }
    t.finish();
}
