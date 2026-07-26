//! Hand-written FFI bindings for the SolveSpace geometric constraint solver.
//!
//! These declarations match the v3.1 `slvs.h` API from the `libslvs1-dev`
//! system package. No bindgen needed — the API surface is small and stable.
//!
//! Handle types use `#[repr(transparent)]` newtypes around `u32` to prevent
//! accidental mixing of param, entity, constraint, and group handles.

#![allow(
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    dead_code
)]

use std::os::raw::c_int;

// --- Handle types (newtype wrappers for type safety) ---

/// Handle to a SolveSpace parameter. Not interchangeable with entity/constraint handles.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Slvs_hParam(pub u32);

/// Handle to a SolveSpace entity. Not interchangeable with param/constraint handles.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Slvs_hEntity(pub u32);

/// Handle to a SolveSpace constraint. Not interchangeable with param/entity handles.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Slvs_hConstraint(pub u32);

/// Handle to a SolveSpace group. Not interchangeable with other handle types.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Slvs_hGroup(pub u32);

// --- Special constants ---
pub const SLVS_FREE_IN_3D: Slvs_hEntity = Slvs_hEntity(0);

// --- Entity type constants ---
pub const SLVS_E_POINT_IN_3D: c_int = 50000;
#[allow(dead_code)]
pub const SLVS_E_POINT_IN_2D: c_int = 50001;
pub const SLVS_E_NORMAL_IN_3D: c_int = 60000;
pub const SLVS_E_NORMAL_IN_2D: c_int = 60001;
pub const SLVS_E_DISTANCE: c_int = 70000;
pub const SLVS_E_WORKPLANE: c_int = 80000;
pub const SLVS_E_LINE_SEGMENT: c_int = 80001;
#[allow(dead_code)]
pub const SLVS_E_CUBIC: c_int = 80002;
pub const SLVS_E_CIRCLE: c_int = 80003;
pub const SLVS_E_ARC_OF_CIRCLE: c_int = 80004;

// --- Constraint type constants ---
pub const SLVS_C_POINTS_COINCIDENT: c_int = 100000;
pub const SLVS_C_PT_PT_DISTANCE: c_int = 100001;
#[allow(dead_code)]
pub const SLVS_C_PT_PLANE_DISTANCE: c_int = 100002;
#[allow(dead_code)]
pub const SLVS_C_PT_LINE_DISTANCE: c_int = 100003;
#[allow(dead_code)]
pub const SLVS_C_PT_FACE_DISTANCE: c_int = 100004;
#[allow(dead_code)]
pub const SLVS_C_PT_IN_PLANE: c_int = 100005;
pub const SLVS_C_PT_ON_LINE: c_int = 100006;
#[allow(dead_code)]
pub const SLVS_C_PT_ON_FACE: c_int = 100007;
pub const SLVS_C_EQUAL_LENGTH_LINES: c_int = 100008;
#[allow(dead_code)]
pub const SLVS_C_LENGTH_RATIO: c_int = 100009;
#[allow(dead_code)]
pub const SLVS_C_EQ_LEN_PT_LINE_D: c_int = 100010;
#[allow(dead_code)]
pub const SLVS_C_EQ_PT_LN_DISTANCES: c_int = 100011;
#[allow(dead_code)]
pub const SLVS_C_EQUAL_ANGLE: c_int = 100012;
#[allow(dead_code)]
pub const SLVS_C_EQUAL_LINE_ARC_LEN: c_int = 100013;
#[allow(dead_code)]
pub const SLVS_C_SYMMETRIC: c_int = 100014;
#[allow(dead_code)]
pub const SLVS_C_SYMMETRIC_HORIZ: c_int = 100015;
#[allow(dead_code)]
pub const SLVS_C_SYMMETRIC_VERT: c_int = 100016;
pub const SLVS_C_SYMMETRIC_LINE: c_int = 100017;
pub const SLVS_C_AT_MIDPOINT: c_int = 100018;
pub const SLVS_C_HORIZONTAL: c_int = 100019;
pub const SLVS_C_VERTICAL: c_int = 100020;
pub const SLVS_C_DIAMETER: c_int = 100021;
pub const SLVS_C_PT_ON_CIRCLE: c_int = 100022;
#[allow(dead_code)]
pub const SLVS_C_SAME_ORIENTATION: c_int = 100023;
pub const SLVS_C_ANGLE: c_int = 100024;
pub const SLVS_C_PARALLEL: c_int = 100025;
pub const SLVS_C_PERPENDICULAR: c_int = 100026;
pub const SLVS_C_ARC_LINE_TANGENT: c_int = 100027;
#[allow(dead_code)]
pub const SLVS_C_CUBIC_LINE_TANGENT: c_int = 100028;
pub const SLVS_C_EQUAL_RADIUS: c_int = 100029;
#[allow(dead_code)]
pub const SLVS_C_PROJ_PT_DISTANCE: c_int = 100030;
pub const SLVS_C_WHERE_DRAGGED: c_int = 100031;
pub const SLVS_C_CURVE_CURVE_TANGENT: c_int = 100032;
#[allow(dead_code)]
pub const SLVS_C_LENGTH_DIFFERENCE: c_int = 100033;
#[allow(dead_code)]
pub const SLVS_C_ARC_ARC_LEN_RATIO: c_int = 100034;
#[allow(dead_code)]
pub const SLVS_C_ARC_LINE_LEN_RATIO: c_int = 100035;
#[allow(dead_code)]
pub const SLVS_C_ARC_ARC_DIFFERENCE: c_int = 100036;
#[allow(dead_code)]
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
        qw: f64,
        qx: f64,
        qy: f64,
        qz: f64,
        x: *mut f64,
        y: *mut f64,
        z: *mut f64,
    );

    pub fn Slvs_QuaternionV(
        qw: f64,
        qx: f64,
        qy: f64,
        qz: f64,
        x: *mut f64,
        y: *mut f64,
        z: *mut f64,
    );

    pub fn Slvs_QuaternionN(
        qw: f64,
        qx: f64,
        qy: f64,
        qz: f64,
        x: *mut f64,
        y: *mut f64,
        z: *mut f64,
    );

    pub fn Slvs_MakeQuaternion(
        ux: f64,
        uy: f64,
        uz: f64,
        vx: f64,
        vy: f64,
        vz: f64,
        qw: *mut f64,
        qx: *mut f64,
        qy: *mut f64,
        qz: *mut f64,
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
            point: [Slvs_hEntity(0); 4],
            normal: Slvs_hEntity(0),
            distance: Slvs_hEntity(0),
            param: [Slvs_hParam(0); 4],
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
        e.param = [px, py, pz, Slvs_hParam(0)];
        e
    }

    pub fn point_2d(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        wrkpl: Slvs_hEntity,
        pu: Slvs_hParam,
        pv: Slvs_hParam,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_POINT_IN_2D);
        e.wrkpl = wrkpl;
        e.param = [pu, pv, Slvs_hParam(0), Slvs_hParam(0)];
        e
    }

    pub fn line_segment(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_LINE_SEGMENT);
        e.point = [pt_a, pt_b, Slvs_hEntity(0), Slvs_hEntity(0)];
        e
    }

    /// A line segment scoped to `wrkpl`, mirroring `Slvs_MakeLineSegment`.
    ///
    /// Distinct from [`Slvs_Entity::line_segment`], which leaves `wrkpl` at
    /// `SLVS_FREE_IN_3D`.  `SLVS_C_HORIZONTAL`, `SLVS_C_VERTICAL` and the
    /// tangent constraints are only meaningful against a workplane-scoped line,
    /// so the 2D sketch path needs this form; the 3D-scoped sibling is kept
    /// unchanged because the legacy pattern-recognition route depends on it.
    /// Having two builders rather than one workplane argument makes the choice
    /// visible at the call site instead of hidden in a parameter.
    pub fn line_segment_2d(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        wrkpl: Slvs_hEntity,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_LINE_SEGMENT);
        e.wrkpl = wrkpl;
        e.point = [pt_a, pt_b, Slvs_hEntity(0), Slvs_hEntity(0)];
        e
    }

    /// The in-plane normal of `wrkpl`, mirroring `Slvs_MakeNormal2d`.
    ///
    /// Carries no params of its own — it just names the workplane whose normal
    /// it is.  Circles and arcs need a normal entity; in a 2D sketch every one
    /// of them shares this single instance.
    pub fn normal_in_2d(h: Slvs_hEntity, group: Slvs_hGroup, wrkpl: Slvs_hEntity) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_NORMAL_IN_2D);
        e.wrkpl = wrkpl;
        e
    }

    /// A scalar-distance entity wrapping the param `d`, mirroring
    /// `Slvs_MakeDistance`.
    ///
    /// libslvs has no "circle radius param" — a circle points at a *distance
    /// entity*, which in turn holds the radius param.  This is that carrier.
    pub fn distance(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        wrkpl: Slvs_hEntity,
        d: Slvs_hParam,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_DISTANCE);
        e.wrkpl = wrkpl;
        e.param = [d, Slvs_hParam(0), Slvs_hParam(0), Slvs_hParam(0)];
        e
    }

    /// A full circle, mirroring `Slvs_MakeCircle`.
    ///
    /// `radius` is a [`Slvs_Entity::distance`] carrier, not a param and not a
    /// point — it goes in the dedicated `distance` slot.
    pub fn circle(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        wrkpl: Slvs_hEntity,
        center: Slvs_hEntity,
        normal: Slvs_hEntity,
        radius: Slvs_hEntity,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_CIRCLE);
        e.wrkpl = wrkpl;
        e.point = [center, Slvs_hEntity(0), Slvs_hEntity(0), Slvs_hEntity(0)];
        e.normal = normal;
        e.distance = radius;
        e
    }

    /// An arc of a circle, mirroring `Slvs_MakeArcOfCircle`.
    ///
    /// Unlike [`Slvs_Entity::circle`] an arc carries no radius carrier: its
    /// radius is implied by `center`→`start`, and libslvs contributes the
    /// `|center - start| = |center - end|` equation itself.  DOF accounting over
    /// a sketch containing arcs must not count that equation a second time.
    #[allow(clippy::too_many_arguments)]
    pub fn arc_of_circle(
        h: Slvs_hEntity,
        group: Slvs_hGroup,
        wrkpl: Slvs_hEntity,
        normal: Slvs_hEntity,
        center: Slvs_hEntity,
        start: Slvs_hEntity,
        end: Slvs_hEntity,
    ) -> Self {
        let mut e = Self::zeroed_with(h, group, SLVS_E_ARC_OF_CIRCLE);
        e.wrkpl = wrkpl;
        e.normal = normal;
        e.point = [center, start, end, Slvs_hEntity(0)];
        e
    }
}

impl Slvs_Constraint {
    #[allow(clippy::too_many_arguments)]
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
            entityC: Slvs_hEntity(0),
            entityD: Slvs_hEntity(0),
            other: 0,
            other2: 0,
        }
    }

    /// Set the `other` / `other2` endpoint selectors, chained off [`Self::new`].
    ///
    /// `Slvs_MakeConstraint` has no parameters for these, so [`Self::new`]
    /// leaves both at 0 — the right default for every constraint that ignores
    /// them.  `SLVS_C_ARC_LINE_TANGENT` and `SLVS_C_CURVE_CURVE_TANGENT` do not
    /// ignore them: they select which endpoint of each curve is the tangent
    /// point (`0` = the curve's start, `1` = its end).  Chaining keeps this
    /// additive, so no existing call site changes.
    pub fn with_other(mut self, other: c_int, other2: c_int) -> Self {
        self.other = other;
        self.other2 = other2;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: Slvs_hEntity = Slvs_hEntity(7);
    const G: Slvs_hGroup = Slvs_hGroup(3);
    const WP: Slvs_hEntity = Slvs_hEntity(9);

    /// Every entity field slvs.h's `Slvs_Make*` leaves at its memset-zero value.
    ///
    /// Asserting the *absence* of writes is the load-bearing half of these
    /// tests: a stray `point[1]` or `distance` on an entity libslvs expects to
    /// be zero is read as a real handle and silently changes the equations.
    fn assert_points_zero(e: &Slvs_Entity, expected: [Slvs_hEntity; 4]) {
        assert_eq!(e.point, expected, "point[] mismatch");
    }

    fn assert_params_zero(e: &Slvs_Entity) {
        assert_eq!(e.param, [Slvs_hParam(0); 4], "param[] should be all-zero");
    }

    /// `Slvs_MakeNormal2d`: type + wrkpl only, nothing else touched.
    #[test]
    fn normal_in_2d_matches_slvs_h() {
        let e = Slvs_Entity::normal_in_2d(H, G, WP);
        assert_eq!(e.h, H);
        assert_eq!(e.group, G);
        assert_eq!(e.type_, SLVS_E_NORMAL_IN_2D);
        assert_eq!(e.wrkpl, WP);
        assert_points_zero(&e, [Slvs_hEntity(0); 4]);
        assert_params_zero(&e);
        assert_eq!(e.normal, Slvs_hEntity(0));
        assert_eq!(e.distance, Slvs_hEntity(0));
    }

    /// `Slvs_MakeDistance`: the radius carrier — one param, no points.
    #[test]
    fn distance_matches_slvs_h() {
        let d = Slvs_hParam(42);
        let e = Slvs_Entity::distance(H, G, WP, d);
        assert_eq!(e.h, H);
        assert_eq!(e.group, G);
        assert_eq!(e.type_, SLVS_E_DISTANCE);
        assert_eq!(e.wrkpl, WP);
        assert_eq!(e.param, [d, Slvs_hParam(0), Slvs_hParam(0), Slvs_hParam(0)]);
        assert_points_zero(&e, [Slvs_hEntity(0); 4]);
        assert_eq!(e.normal, Slvs_hEntity(0));
        assert_eq!(e.distance, Slvs_hEntity(0));
    }

    /// `Slvs_MakeCircle`: center in `point[0]`, normal and radius-carrier in
    /// their own dedicated slots — not in `point[]`.
    #[test]
    fn circle_matches_slvs_h() {
        let center = Slvs_hEntity(11);
        let normal = Slvs_hEntity(12);
        let radius = Slvs_hEntity(13);
        let e = Slvs_Entity::circle(H, G, WP, center, normal, radius);
        assert_eq!(e.h, H);
        assert_eq!(e.group, G);
        assert_eq!(e.type_, SLVS_E_CIRCLE);
        assert_eq!(e.wrkpl, WP);
        assert_points_zero(
            &e,
            [center, Slvs_hEntity(0), Slvs_hEntity(0), Slvs_hEntity(0)],
        );
        assert_eq!(e.normal, normal);
        assert_eq!(e.distance, radius);
        assert_params_zero(&e);
    }

    /// `Slvs_MakeArcOfCircle`: center/start/end in `point[0..3]` in that order.
    ///
    /// Order is not cosmetic — libslvs derives the arc's radius from
    /// `point[0]`→`point[1]` and its sweep from `point[1]`→`point[2]`.
    #[test]
    fn arc_of_circle_matches_slvs_h() {
        let normal = Slvs_hEntity(21);
        let center = Slvs_hEntity(22);
        let start = Slvs_hEntity(23);
        let end = Slvs_hEntity(24);
        let e = Slvs_Entity::arc_of_circle(H, G, WP, normal, center, start, end);
        assert_eq!(e.h, H);
        assert_eq!(e.group, G);
        assert_eq!(e.type_, SLVS_E_ARC_OF_CIRCLE);
        assert_eq!(e.wrkpl, WP);
        assert_points_zero(&e, [center, start, end, Slvs_hEntity(0)]);
        assert_eq!(e.normal, normal);
        assert_eq!(e.distance, Slvs_hEntity(0));
        assert_params_zero(&e);
    }

    /// `Slvs_MakeLineSegment` sets `wrkpl`; the 3D-scoped [`Slvs_Entity::line_segment`]
    /// leaves it at `SLVS_FREE_IN_3D`.
    ///
    /// The two builders differ in exactly that one field, and that field is what
    /// `SLVS_C_HORIZONTAL` / `SLVS_C_VERTICAL` / the tangent constraints need —
    /// so this test pins both halves of the contrast, not just the new builder.
    #[test]
    fn line_segment_2d_is_workplane_scoped_unlike_line_segment() {
        let a = Slvs_hEntity(31);
        let b = Slvs_hEntity(32);

        let scoped = Slvs_Entity::line_segment_2d(H, G, WP, a, b);
        assert_eq!(scoped.h, H);
        assert_eq!(scoped.group, G);
        assert_eq!(scoped.type_, SLVS_E_LINE_SEGMENT);
        assert_eq!(scoped.wrkpl, WP);
        assert_points_zero(&scoped, [a, b, Slvs_hEntity(0), Slvs_hEntity(0)]);
        assert_eq!(scoped.normal, Slvs_hEntity(0));
        assert_eq!(scoped.distance, Slvs_hEntity(0));
        assert_params_zero(&scoped);

        let free = Slvs_Entity::line_segment(H, G, a, b);
        assert_eq!(
            free.wrkpl, SLVS_FREE_IN_3D,
            "the 3D-scoped builder must stay unscoped — the legacy route depends on it"
        );
        assert_eq!(free.type_, scoped.type_);
        assert_eq!(free.point, scoped.point);
    }

    /// `with_other` sets only `other`/`other2`, leaving the rest of the
    /// constraint exactly as [`Slvs_Constraint::new`] built it.
    #[test]
    fn with_other_sets_only_the_endpoint_selectors() {
        let ch = Slvs_hConstraint(5);
        let base = Slvs_Constraint::new(
            ch,
            G,
            SLVS_C_ARC_LINE_TANGENT,
            WP,
            1.5,
            Slvs_hEntity(41),
            Slvs_hEntity(42),
            Slvs_hEntity(43),
            Slvs_hEntity(44),
        );
        assert_eq!(base.other, 0, "new() must default the selectors to 0");
        assert_eq!(base.other2, 0);

        let c = base.with_other(1, 2);
        assert_eq!(c.other, 1);
        assert_eq!(c.other2, 2);

        assert_eq!(c.h, base.h);
        assert_eq!(c.group, base.group);
        assert_eq!(c.type_, base.type_);
        assert_eq!(c.wrkpl, base.wrkpl);
        assert_eq!(c.valA, base.valA);
        assert_eq!(c.ptA, base.ptA);
        assert_eq!(c.ptB, base.ptB);
        assert_eq!(c.entityA, base.entityA);
        assert_eq!(c.entityB, base.entityB);
        assert_eq!(c.entityC, base.entityC);
        assert_eq!(c.entityD, base.entityD);
    }
}
