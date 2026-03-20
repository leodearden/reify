//! Hand-written FFI bindings for the SolveSpace geometric constraint solver.
//!
//! These declarations match the v3.1 `slvs.h` API from the `libslvs1-dev`
//! system package. No bindgen needed — the API surface is small and stable.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code)]

use std::os::raw::c_int;

// --- Handle types ---
pub type Slvs_hParam = u32;
pub type Slvs_hEntity = u32;
pub type Slvs_hConstraint = u32;
pub type Slvs_hGroup = u32;

// --- Special constants ---
pub const SLVS_FREE_IN_3D: Slvs_hEntity = 0;

// --- Entity type constants ---
pub const SLVS_E_POINT_IN_3D: c_int = 50000;
pub const SLVS_E_POINT_IN_2D: c_int = 50001;
pub const SLVS_E_NORMAL_IN_3D: c_int = 60000;
pub const SLVS_E_NORMAL_IN_2D: c_int = 60001;
pub const SLVS_E_DISTANCE: c_int = 70000;
pub const SLVS_E_WORKPLANE: c_int = 80000;
pub const SLVS_E_LINE_SEGMENT: c_int = 80001;
pub const SLVS_E_CUBIC: c_int = 80002;
pub const SLVS_E_CIRCLE: c_int = 80003;
pub const SLVS_E_ARC_OF_CIRCLE: c_int = 80004;

// --- Constraint type constants ---
pub const SLVS_C_POINTS_COINCIDENT: c_int = 100000;
pub const SLVS_C_PT_PT_DISTANCE: c_int = 100001;
pub const SLVS_C_PT_PLANE_DISTANCE: c_int = 100002;
pub const SLVS_C_PT_LINE_DISTANCE: c_int = 100003;
pub const SLVS_C_PT_FACE_DISTANCE: c_int = 100004;
pub const SLVS_C_PT_IN_PLANE: c_int = 100005;
pub const SLVS_C_PT_ON_LINE: c_int = 100006;
pub const SLVS_C_PT_ON_FACE: c_int = 100007;
pub const SLVS_C_EQUAL_LENGTH_LINES: c_int = 100008;
pub const SLVS_C_LENGTH_RATIO: c_int = 100009;
pub const SLVS_C_EQ_LEN_PT_LINE_D: c_int = 100010;
pub const SLVS_C_EQ_PT_LN_DISTANCES: c_int = 100011;
pub const SLVS_C_EQUAL_ANGLE: c_int = 100012;
pub const SLVS_C_EQUAL_LINE_ARC_LEN: c_int = 100013;
pub const SLVS_C_SYMMETRIC: c_int = 100014;
pub const SLVS_C_SYMMETRIC_HORIZ: c_int = 100015;
pub const SLVS_C_SYMMETRIC_VERT: c_int = 100016;
pub const SLVS_C_SYMMETRIC_LINE: c_int = 100017;
pub const SLVS_C_AT_MIDPOINT: c_int = 100018;
pub const SLVS_C_HORIZONTAL: c_int = 100019;
pub const SLVS_C_VERTICAL: c_int = 100020;
pub const SLVS_C_DIAMETER: c_int = 100021;
pub const SLVS_C_PT_ON_CIRCLE: c_int = 100022;
pub const SLVS_C_SAME_ORIENTATION: c_int = 100023;
pub const SLVS_C_ANGLE: c_int = 100024;
pub const SLVS_C_PARALLEL: c_int = 100025;
pub const SLVS_C_PERPENDICULAR: c_int = 100026;
pub const SLVS_C_ARC_LINE_TANGENT: c_int = 100027;
pub const SLVS_C_CUBIC_LINE_TANGENT: c_int = 100028;
pub const SLVS_C_EQUAL_RADIUS: c_int = 100029;
pub const SLVS_C_PROJ_PT_DISTANCE: c_int = 100030;
pub const SLVS_C_WHERE_DRAGGED: c_int = 100031;
pub const SLVS_C_CURVE_CURVE_TANGENT: c_int = 100032;
pub const SLVS_C_LENGTH_DIFFERENCE: c_int = 100033;
pub const SLVS_C_ARC_ARC_LEN_RATIO: c_int = 100034;
pub const SLVS_C_ARC_LINE_LEN_RATIO: c_int = 100035;
pub const SLVS_C_ARC_ARC_DIFFERENCE: c_int = 100036;
pub const SLVS_C_ARC_LINE_DIFFERENCE: c_int = 100037;

// --- Result constants ---
pub const SLVS_RESULT_OKAY: c_int = 0;
pub const SLVS_RESULT_INCONSISTENT: c_int = 1;
pub const SLVS_RESULT_DIDNT_CONVERGE: c_int = 2;
pub const SLVS_RESULT_TOO_MANY_UNKNOWNS: c_int = 3;

// --- Structs ---

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Slvs_Param {
    pub h: Slvs_hParam,
    pub group: Slvs_hGroup,
    pub val: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Slvs_Entity {
    pub h: Slvs_hEntity,
    pub group: Slvs_hGroup,
    pub type_: c_int,
    pub wrkpl: Slvs_hEntity,
    pub point: [Slvs_hEntity; 4],
    pub normal: Slvs_hEntity,
    pub distance: Slvs_hEntity,
    pub param: [Slvs_hParam; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Slvs_Constraint {
    pub h: Slvs_hConstraint,
    pub group: Slvs_hGroup,
    pub type_: c_int,
    pub wrkpl: Slvs_hEntity,
    pub valA: f64,
    pub ptA: Slvs_hEntity,
    pub ptB: Slvs_hEntity,
    pub entityA: Slvs_hEntity,
    pub entityB: Slvs_hEntity,
    pub entityC: Slvs_hEntity,
    pub entityD: Slvs_hEntity,
    pub other: c_int,
    pub other2: c_int,
}

#[repr(C)]
pub struct Slvs_System {
    pub param: *mut Slvs_Param,
    pub params: c_int,
    pub entity: *mut Slvs_Entity,
    pub entities: c_int,
    pub constraint: *mut Slvs_Constraint,
    pub constraints: c_int,
    pub dragged: [Slvs_hParam; 4],
    pub calculateFaileds: c_int,
    pub failed: *mut Slvs_hConstraint,
    pub faileds: c_int,
    pub dof: c_int,
    pub result: c_int,
}

unsafe extern "C" {
    pub fn Slvs_Solve(sys: *mut Slvs_System, hg: Slvs_hGroup);

    pub fn Slvs_QuaternionU(
        qw: f64, qx: f64, qy: f64, qz: f64,
        x: *mut f64, y: *mut f64, z: *mut f64,
    );

    pub fn Slvs_QuaternionV(
        qw: f64, qx: f64, qy: f64, qz: f64,
        x: *mut f64, y: *mut f64, z: *mut f64,
    );

    pub fn Slvs_QuaternionN(
        qw: f64, qx: f64, qy: f64, qz: f64,
        x: *mut f64, y: *mut f64, z: *mut f64,
    );

    pub fn Slvs_MakeQuaternion(
        ux: f64, uy: f64, uz: f64,
        vx: f64, vy: f64, vz: f64,
        qw: *mut f64, qx: *mut f64, qy: *mut f64, qz: *mut f64,
    );
}

// --- Safe convenience constructors ---

impl Slvs_Param {
    pub fn new(h: Slvs_hParam, group: Slvs_hGroup, val: f64) -> Self {
        Self { h, group, val }
    }
}

impl Slvs_Entity {
    pub fn zeroed_with(h: Slvs_hEntity, group: Slvs_hGroup, type_: c_int) -> Self {
        Self {
            h,
            group,
            type_,
            wrkpl: SLVS_FREE_IN_3D,
            point: [0; 4],
            normal: 0,
            distance: 0,
            param: [0; 4],
        }
    }

    pub fn point_3d(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        px: Slvs_hParam,
        py: Slvs_hParam,
        pz: Slvs_hParam,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_POINT_IN_3D);
        e.param = [px, py, pz, 0];
        e
    }

    pub fn line_segment(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_LINE_SEGMENT);
        e.point = [pt_a, pt_b, 0, 0];
        e
    }
}

impl Slvs_Constraint {
    pub fn new(
        h: Slvs_hConstraint,
        group: Slvs_hGroup,
        type_: c_int,
        wrkpl: Slvs_hEntity,
        val_a: f64,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
        entity_a: Slvs_hEntity,
        entity_b: Slvs_hEntity,
    ) -> Self {
        Self {
            h,
            group,
            type_,
            wrkpl,
            valA: val_a,
            ptA: pt_a,
            ptB: pt_b,
            entityA: entity_a,
            entityB: entity_b,
            entityC: 0,
            entityD: 0,
            other: 0,
            other2: 0,
        }
    }
}
