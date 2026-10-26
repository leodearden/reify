//! STEP-content audits of `OcctKernelHandle::export` / `export_with_options`:
//! the ISO header, the declared length and plane-angle units against the
//! coordinate payload, and the selected application-protocol schema.

#![cfg(has_occt)]

use reify_ir::{GeometryOp, Value};
use reify_kernel_occt::OcctKernelHandle;

#[test]
fn export_step_contains_iso_header() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let mut buf = Vec::new();
    handle
        .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();
    assert!(
        content.contains("ISO-10303-21"),
        "STEP export should contain ISO-10303-21 header"
    );
}

/// STEP export must keep its unit DECLARATION and its coordinate PAYLOAD
/// in agreement: reify model space is SI metres, the exported file declares
/// `SI_UNIT(.MILLI.,.METRE.)`, so the coordinates it carries must be
/// millimetres.
///
/// The two halves DISAGREE today, and that asymmetry IS the defect. The
/// declaration half already passes (OCCT writes the millimetre `SI_UNIT`
/// by default); the payload half fails, because reify's SI-metre
/// coordinates reach OCCT under a unit regime whose scale factor is 1.0
/// and are emitted verbatim. A 30 mm cube is written as 0.030 and read by
/// any STEP consumer as 30 µm — a 1000× shrink.
///
/// The payload assertion is on the per-axis AABB extent parsed out of the
/// `CARTESIAN_POINT` entities rather than on formatted coordinate strings,
/// so it is invariant under everything that legitimately varies: OCCT's
/// float formatting, entity ordering, box centring, and the
/// AP203/AP214/AP242 schema selection the sibling tests flip. The 1e-6 mm
/// bound is derived, not tuned: one exactly-representable ×1000 f64
/// multiply (≤1 ulp ≈ 3.6e-15 mm at 30) plus OCCT's ≥12-significant-digit
/// decimal round-trip (≤5e-11 mm) — about four orders of margin, and still
/// four orders tighter than the 0.030-vs-30.0 gap it guards.
#[test]
fn export_step_declares_millimetres_and_scales_metre_coordinates() {
    // A 30 mm cube expressed in reify's SI-metre model space.
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(0.030),
        size_y: Value::Real(0.030),
        size_z: Value::Real(0.030),
    };
    let gh = handle.execute(&op).unwrap();
    let mut buf = Vec::new();
    handle
        .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();

    // STEP wraps long lines, so `contains` (and any parse) on the raw text
    // is flaky — drop every ASCII whitespace byte first.
    let stripped: String = content
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();

    // (1) DECLARATION half — passes today.
    assert!(
        stripped.contains("SI_UNIT(.MILLI.,.METRE.)"),
        "STEP export should declare millimetres via SI_UNIT(.MILLI.,.METRE.)"
    );

    // (2) PAYLOAD half — every CARTESIAN_POINT coordinate triple, folded
    // into a per-axis AABB by the shared `cartesian_point_aabb` helper.
    let (min, max, n_points) = cartesian_point_aabb(&stripped);
    assert!(
        n_points > 0,
        "expected at least one 3D CARTESIAN_POINT in the STEP export, found none"
    );

    for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
        let extent = max[axis] - min[axis];
        assert!(
            (extent - 30.0).abs() < 1e-6,
            "a 30 mm cube (0.030 m in reify model space) should span 30.0 mm on {name} in a \
             millimetre-declared STEP file, but the CARTESIAN_POINT AABB extent is {extent} \
             (min {}, max {}) — declaration and payload disagree",
            min[axis],
            max[axis]
        );
    }
}

/// Fold every 3D `CARTESIAN_POINT` in a whitespace-stripped STEP file
/// into a per-axis AABB, returning `(min, max, n_points)`.
///
/// Entity body shape: `CARTESIAN_POINT('',(x,y,z))` once whitespace is
/// stripped, so the coordinate list is the first parenthesised group after
/// the entity's opening paren. Non-3D bodies (and unparsable ones) are
/// skipped rather than failing, so a file mixing 2D and 3D points still
/// yields a usable 3D box.
///
/// Asserting on this box rather than on formatted coordinate strings keeps
/// the callers invariant under everything that legitimately varies: OCCT's
/// float formatting, entity ordering, shape centring, and the
/// AP203/AP214/AP242 schema selection the sibling tests flip.
fn cartesian_point_aabb(stripped: &str) -> ([f64; 3], [f64; 3], usize) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut n_points = 0usize;
    for tail in stripped.split("CARTESIAN_POINT(").skip(1) {
        let Some(open) = tail.find('(') else { continue };
        let Some(close) = tail[open + 1..].find(')') else {
            continue;
        };
        let coords: Vec<f64> = tail[open + 1..open + 1 + close]
            .split(',')
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();
        if coords.len() != 3 {
            // Not a 3D point (or an unparsable body) — ignore it.
            continue;
        }
        n_points += 1;
        for axis in 0..3 {
            min[axis] = min[axis].min(coords[axis]);
            max[axis] = max[axis].max(coords[axis]);
        }
    }
    (min, max, n_points)
}

/// One `GLOBAL_UNIT_ASSIGNED_CONTEXT` entity, with its unit-reference list
/// already resolved against the file's instance table.
struct UnitContext {
    /// The context instance's own `#N` id, for failure messages.
    id: String,
    /// The whole instance record, for failure messages.
    record: String,
    /// EVERY unit instance this context references, resolved against the
    /// instance table, as `(#N, whole record)` pairs — the FULL list, not
    /// just the plane-angle subset. The full list is what the interesting
    /// failure needs: when a context reaches no plane-angle unit at all,
    /// the diagnostic question is "then what DID it reach?", and a
    /// pre-filtered subset answers that with an empty vector.
    units: Vec<(String, String)>,
}

impl UnitContext {
    /// Does this context reach a `PLANE_ANGLE_UNIT()` declaration?
    fn reaches_plane_angle_unit(&self) -> bool {
        self.units
            .iter()
            .any(|(_, body)| body.contains("PLANE_ANGLE_UNIT()"))
    }

    /// One line per resolved unit reference, for the failure message that
    /// fires when `reaches_plane_angle_unit` is false.
    fn resolved_units_summary(&self) -> String {
        if self.units.is_empty() {
            return "    (none — the context's reference list resolved to no \
                    instance in the file)"
                .to_owned();
        }
        self.units
            .iter()
            .map(|(id, body)| format!("    {id} -> {body}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The result of walking a STEP file's plane-angle unit declarations.
struct PlaneAngleAudit {
    /// EVERY instance record in the file that declares a
    /// `PLANE_ANGLE_UNIT()`, whether or not a context references it.
    records: Vec<String>,
    /// One entry per `GLOBAL_UNIT_ASSIGNED_CONTEXT`, in file order.
    contexts: Vec<UnitContext>,
}

/// Audit a STEP file's PLANE-ANGLE unit declarations BY ASSOCIATION.
///
/// Takes ALREADY-STRIPPED text, like its sibling `cartesian_point_aabb` —
/// the two helpers share one input convention so a caller cannot get them
/// out of step, and each caller strips the file exactly once. STEP wraps
/// long lines at ~72 chars, so every `contains`/parse here must run over
/// whitespace-stripped text; that is the same idiom the sibling
/// `export_step_declares_millimetres_and_scales_metre_coordinates` uses.
/// Part-21 instances terminate at `;`, so splitting the stripped text on
/// `;` yields one string per entity instance, which is what makes a
/// per-record ("EVERY declaration is radians") assertion possible rather
/// than a file-wide substring grep.
///
/// Beyond that, this RESOLVES each `GLOBAL_UNIT_ASSIGNED_CONTEXT`'s unit
/// reference list (`GLOBAL_UNIT_ASSIGNED_CONTEXT((#13,#17,#18))`) against
/// the `#N =` instance table, so a caller can ask the sound question — does
/// THIS context reference a radian plane-angle unit? — rather than the
/// count proxy `n_plane_angle_unit == n_global_unit_assigned_context`.
/// The proxy was measured-true against system OCCT 7.8 (each context gets
/// its own unit entities) but STEP explicitly permits several contexts to
/// share one unit instance, so an OCCT bump that deduped unit entities
/// would have failed the proxy on a perfectly correct file. Walking the
/// association is the form task #6344's runtime guard uses too: this
/// helper's model-level mirror is `audit_step_plane_angle_units` in
/// `cpp/occt_wrapper.cpp`, pinned by
/// `tests/harness_step_export/step_plane_angle_guard_integration.rs`. That
/// guard refuses the export before any bytes are written; this pin quantifies
/// over the bytes that were.
fn plane_angle_unit_audit(stripped: &str) -> PlaneAngleAudit {
    // Instance table: `#N` -> whole record (`#N=(...)`), for every Part-21
    // instance in the file. HEADER-section records carry no `#N =` and are
    // skipped by the `starts_with('#')` filter.
    let mut instances: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for rec in stripped.split(';') {
        if !rec.starts_with('#') {
            continue;
        }
        let Some(eq) = rec.find('=') else { continue };
        let id = rec[..eq].to_owned();
        if id[1..].chars().all(|c| c.is_ascii_digit()) && id.len() > 1 {
            instances.insert(id, rec.to_owned());
        }
    }

    let records: Vec<String> = stripped
        .split(';')
        .filter(|rec| rec.contains("PLANE_ANGLE_UNIT()"))
        .map(str::to_owned)
        .collect();

    let mut contexts: Vec<UnitContext> = Vec::new();
    for rec in stripped.split(';') {
        let Some(marker) = rec.find("GLOBAL_UNIT_ASSIGNED_CONTEXT(") else {
            continue;
        };
        let id = rec
            .find('=')
            .map_or_else(String::new, |eq| rec[..eq].to_owned());
        // The list is a flat `(#a,#b,#c)` — no nesting — so the first `)`
        // after the opening paren closes it.
        let tail = &rec[marker + "GLOBAL_UNIT_ASSIGNED_CONTEXT(".len()..];
        let refs = tail
            .strip_prefix('(')
            .and_then(|inner| inner.find(')').map(|close| &inner[..close]))
            .unwrap_or("");
        // Resolve the WHOLE reference list; `UnitContext` filters for the
        // plane-angle subset on demand and keeps the rest for diagnostics.
        let units = refs
            .split(',')
            .map(str::trim)
            .filter(|r| r.starts_with('#'))
            .filter_map(|r| instances.get(r).map(|body| (r.to_owned(), body.clone())))
            .collect();
        contexts.push(UnitContext {
            id,
            record: rec.to_owned(),
            units,
        });
    }

    PlaneAngleAudit { records, contexts }
}

/// The three arms that BOTH STEP angular pins share, applied to whichever
/// write path `write_path` names (used only to make a failure say which of
/// the two exports produced the file).
///
/// Extracted so a wording fix, or an added arm — rejecting `.GRAD.`, say —
/// lands in ONE place. Applied to only one of the two pins, such a change
/// would silently leave the other write path's coverage behind, which is
/// exactly the drift these pins exist to prevent.
///
/// What deliberately stays at the call sites, because it is genuinely
/// per-fixture: the BRep pin's `contexts.len() >= 2` multi-context guard
/// and its `CONICAL_SURFACE` witness, and the wireframe pin's
/// `TRIMMED_CURVE` / `PARAMETER_VALUE` / AABB payload checks. This helper
/// asserts only the floor both share — at least ONE context exists — so
/// arm (c)'s loop cannot pass vacuously over an empty vector.
fn assert_plane_angle_units_are_si_radians(audit: &PlaneAngleAudit, write_path: &str) {
    // (a) EVERY plane-angle declaration is the SI radian.
    assert!(
        !audit.records.is_empty(),
        "{write_path} STEP export declared no PLANE_ANGLE_UNIT at all — the angular boundary \
         convention INV-AD-4 requires is missing from the file entirely"
    );
    for rec in &audit.records {
        assert!(
            rec.contains("SI_UNIT($,.RADIAN.)"),
            "every PLANE_ANGLE_UNIT record must declare SI radians as `SI_UNIT($,.RADIAN.)` \
             (`$` is the null SI prefix; the `*` in the sibling `NAMED_UNIT(*)` is the \
             redeclared marker, NOT a prefix), but this {write_path} record does not: {rec}"
        );

        // (b) No degree/grad CONVERSION_BASED_UNIT chain around a
        // PLANE-ANGLE unit — the shape a flipped angle unit must take.
        // Scoped to the plane-angle records deliberately: a file-wide count
        // would also see the LENGTH regime's chain the moment
        // `write.step.unit` moves off millimetres (OCCT spells inch/foot
        // that way), reddening an ANGLE pin for a LENGTH-side change.
        assert!(
            !rec.contains("CONVERSION_BASED_UNIT"),
            "a PLANE_ANGLE_UNIT must not be wrapped in a CONVERSION_BASED_UNIT chain (that is \
             how a degree or grad plane-angle unit would be spelled), but this {write_path} \
             record is: {rec}"
        );
    }

    // (c) ASSOCIATION — every unit context must actually REACH a
    // plane-angle unit. A context that declares none fails here even
    // though every record the file does declare is radians. Resolving the
    // reference list (rather than comparing counts) keeps this green if a
    // future OCCT shares one radian unit instance across all contexts,
    // which STEP permits.
    assert!(
        !audit.contexts.is_empty(),
        "expected at least one GLOBAL_UNIT_ASSIGNED_CONTEXT in the {write_path} export, found \
         none — the file carries no unit context to declare anything in, so the per-context \
         assertion below would pass vacuously"
    );
    for ctx in &audit.contexts {
        assert!(
            ctx.reaches_plane_angle_unit(),
            "{write_path} unit context {} references no PLANE_ANGLE_UNIT — it crosses the \
             boundary with no declared angular convention. Context record: {}\nThe units it \
             DID reach:\n{}",
            ctx.id,
            ctx.record,
            ctx.resolved_units_summary()
        );
    }
}

/// STEP export must declare SI RADIANS for plane angles in **every** unit
/// context the file carries — the boundary declaration INV-AD-4 requires
/// (`docs/prds/v0_6/angle-dimension-completion.md`, §9 B7; task #6184).
///
/// REGRESSION PIN, GREEN ON ARRIVAL — exactly the posture task 6186's
/// `export_unit_regime_e2e.rs` documents for the length regime. reify's
/// angle declaration is correct *by construction*:
/// `STEPConstruct_UnitContext::Init`, the sole builder of the write-side
/// unit context, emits `SI_UNIT($,.RADIAN.)` unconditionally, with no
/// preceding branch and no data dependence on any argument. There is no
/// write-side angle knob to get wrong. So this pin does not prove a fix
/// landed — it guards a correct-today property against an OCCT bump or a
/// future writer-option change. **If it ever fails, the defect is real:
/// debug it, do not relax it.**
///
/// Why the universal quantifier rather than `content.contains(".RADIAN.")`:
/// STEP scopes units *per representation_context*, and OCCT uses that
/// freedom for every compound. Measured on this branch against the linked
/// system OCCT 7.8, the two-cone union below emits **three**
/// `GLOBAL_UNIT_ASSIGNED_CONTEXT` entities, each carrying its **own**
/// `PLANE_ANGLE_UNIT` (`CONICAL_SURFACE` count 2). A substring grep is
/// therefore satisfied by any one of N contexts and stays green through a
/// *partial* flip, and it cannot detect a context that declares no
/// plane-angle unit at all.
///
/// The second hole is closed by ASSOCIATION, not by a count: the helper
/// resolves each context's unit reference list and this test asserts every
/// context reaches a radian plane-angle unit. An earlier draft compared
/// `n_plane_angle_unit == n_global_unit_assigned_context`, which was
/// measured-true here but is not a property STEP guarantees — several
/// contexts may legally share ONE unit instance, so an OCCT bump that
/// deduped unit entities would have reddened this pin on a correct file
/// and pointed the reader at the export rather than at the proxy.
///
/// Token detail a naive pin gets wrong: the emitted form is
/// `SI_UNIT($,.RADIAN.)` — `$` is the *null SI prefix*. The `*` in the
/// sibling `NAMED_UNIT(*)` is the *redeclared* marker, so a pin written
/// for `SI_UNIT(*,.RADIAN.)` fails immediately.
///
/// No runtime `OCCT_AVAILABLE` skip: inside this member's `#![cfg(has_occt)]`
/// gate such a check is tautological (`src/lib.rs`'s module docs, #6343), and
/// build-time OCCT presence is gated before any compile by the OCCT arm of
/// `scripts/check-manifold-deps.sh`.
#[test]
fn export_step_declares_si_radians_in_every_unit_context() {
    // Two disjoint 30 mm cones, unioned — a compound, which is what makes
    // OCCT emit more than one representation context.
    let handle = OcctKernelHandle::spawn();
    let cone = GeometryOp::Cone {
        bottom_radius: Value::Real(0.015),
        top_radius: Value::Real(0.0),
        height: Value::Real(0.030),
    };
    let left = handle.execute(&cone).unwrap();
    let right_untranslated = handle.execute(&cone).unwrap();
    let right = handle
        .execute(&GeometryOp::Translate {
            target: right_untranslated.id,
            dx: 0.100,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    let union = handle
        .execute(&GeometryOp::Union {
            left: left.id,
            right: right.id,
        })
        .unwrap();

    let mut buf = Vec::new();
    handle
        .export(union.id, reify_ir::ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();
    let stripped: String = content
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();

    let audit = plane_angle_unit_audit(&stripped);

    // Arms (a) declaration-is-radians, (b) no CONVERSION_BASED_UNIT chain,
    // and (c) every context REACHES a plane-angle unit — shared verbatim
    // with the wireframe pin below.
    assert_plane_angle_units_are_si_radians(&audit, "BRep");

    // Per-fixture on top of (c): this pin is the one that exercises the
    // MULTI-context case, so a single-context file means the fixture has
    // degraded even though the shared arms passed.
    assert!(
        audit.contexts.len() >= 2,
        "this pin exists to exercise the MULTI-context case, but the file carries only {} \
         GLOBAL_UNIT_ASSIGNED_CONTEXT entity/entities — the two-cone union fixture no longer \
         produces a compound, so the pin has silently degraded into the single-context case",
        audit.contexts.len()
    );

    // (d) Branch-reached witness: the BRep angle path (GeomToStep_Make-
    // ConicalSurface writes a semi-angle) was actually exercised, so the
    // pin cannot degrade into testing an angle-free file.
    assert!(
        stripped.contains("CONICAL_SURFACE"),
        "expected the cone fixture to emit a CONICAL_SURFACE (whose semi-angle is the BRep \
         angular payload this pin covers); without it the file carries no angle at all and \
         the assertions above are vacuous"
    );
}

/// The WIREFRAME half of the STEP angular boundary — a cone-only fixture
/// covers one of the two angle-bearing write paths, and this is the other
/// (task #6184; `docs/prds/v0_6/angle-dimension-completion.md`, INV-AD-4).
///
/// REGRESSION PIN, GREEN ON ARRIVAL, same posture as the BRep pin above
/// and as 6186's `export_unit_regime_e2e.rs`: the declaration is correct by
/// construction (`STEPConstruct_UnitContext::Init` emits
/// `SI_UNIT($,.RADIAN.)` unconditionally). **If it ever fails, the defect
/// is real: debug it, do not relax it.**
///
/// Why this pin lives in the kernel crate rather than beside 6186's
/// reify-eval `.ri` e2e: `STEPOutput.subject : Solid`
/// (`crates/reify-compiler/stdlib/io.ri`) cannot carry a free curve, so the
/// DSL route physically cannot reach the wireframe branch. The kernel API
/// can — `OcctKernel::export`'s STEP arm hands `get_shape(handle)` straight
/// to `ffi::export_step` with no solid restriction, so an `Arc` wire handle
/// exports as a `GEOMETRIC_CURVE_SET`.
///
/// The DECLARATION-vs-PAYLOAD half is what proves the trim parameter space
/// really is radians. Measured on this branch against the linked system
/// OCCT 7.8, the fixture emits
/// `TRIMMED_CURVE('',#17,(#22,PARAMETER_VALUE(0.)),(#23,PARAMETER_VALUE(1.)),.T.,.PARAMETER.)`
/// — the trim bounds are the arc's `start_angle`/`end_angle` verbatim, and
/// `.PARAMETER.` says the parameter (not the redundant cartesian point) is
/// the master representation. The arc's `CARTESIAN_POINT` AABB then has
/// y-extent `20·sin(1)` = 16.829419696157930 mm.
///
/// BOUND DERIVATION (do not retune) — the same basis 6186 documented: one
/// exactly-representable ×1000 f64 multiply (≤1 ulp ≈ 3.6e-15 mm at 20)
/// plus OCCT's ≥12-significant-digit decimal round-trip (≤2e-11 mm at 20),
/// roughly five orders of margin inside the 1e-6 mm bound. And the bound is
/// ~7 orders TIGHTER than the defect it guards: reading the same trim
/// parameter as degrees puts the endpoint at `20·sin(1°)` = 0.349 mm, a
/// 16.48 mm gap.
///
/// The x-extent is deliberately NOT asserted: it is 20.0 under BOTH
/// interpretations (the θ=0 endpoint and the circle centre both sit on it),
/// so it cannot discriminate. Do not "strengthen" this test by adding it.
///
/// Independently cross-checked at chartering against the redundant
/// cartesian trim point (r·cos 1, r·sin 1) to 12 digits.
#[test]
fn export_step_declares_si_radians_for_wireframe_curve_parameters() {
    // A free 20 mm-radius arc swept through 1 RADIAN, expressed in reify's
    // SI-metre / SI-radian model space.
    let handle = OcctKernelHandle::spawn();
    let arc = handle
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius: 0.020,
            start_angle: 0.0,
            end_angle: 1.0,
            axis: [0.0, 0.0, 1.0],
        })
        .unwrap();

    let mut buf = Vec::new();
    handle
        .export(arc.id, reify_ir::ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();
    let stripped: String = content
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();

    let audit = plane_angle_unit_audit(&stripped);

    // Arms (a)/(b)/(c), the SAME universal quantifier the BRep pin applies,
    // so neither write path can drift from the other. No `contexts.len()`
    // floor beyond the helper's `>= 1` here: a single-context wireframe
    // file is legitimate, and the multi-context case is the BRep pin's job.
    assert_plane_angle_units_are_si_radians(&audit, "wireframe");

    // (d) DECLARATION-vs-PAYLOAD agreement. Locate the TRIMMED_CURVE
    // instance; a missing one would make everything below vacuous, so this
    // panics with the whole file rather than passing silently.
    let trimmed = stripped
        .split(';')
        .find(|rec| rec.contains("TRIMMED_CURVE("))
        .unwrap_or_else(|| {
            panic!(
                "expected the arc fixture to emit a TRIMMED_CURVE (the wireframe angular \
                 payload this pin covers); without it the assertions above are vacuous. \
                 Stripped file:\n{stripped}"
            )
        });
    assert!(
        trimmed.contains(".PARAMETER."),
        "the trimmed curve's master representation must be `.PARAMETER.` (the trim bounds ARE \
         the angular parameters, not merely redundant cartesian points), but the record is: \
         {trimmed}"
    );

    // The record carries both trim bounds — PARAMETER_VALUE(0.) and
    // PARAMETER_VALUE(1.) — i.e. the arc's start_angle/end_angle verbatim,
    // in radians.
    let mut params: Vec<f64> = Vec::new();
    for tail in trimmed.split("PARAMETER_VALUE(").skip(1) {
        let Some(close) = tail.find(')') else {
            continue;
        };
        if let Ok(v) = tail[..close].parse::<f64>() {
            params.push(v);
        }
    }
    assert_eq!(
        params.len(),
        2,
        "expected two PARAMETER_VALUE trim bounds on the trimmed curve, parsed {params:?} \
         from: {trimmed}"
    );
    let hi = params.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lo = params.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(
        (lo - 0.0).abs() < 1e-12 && (hi - 1.0).abs() < 1e-12,
        "the arc was built with start_angle 0.0 rad and end_angle 1.0 rad, so its STEP trim \
         bounds must be 0.0 and 1.0 (parameter space is radians, unscaled); parsed \
         {params:?} instead — a degree rescale would put the upper bound at 57.29578"
    );

    // The payload half: 20·sin(1) mm on y. Under a degree misreading the
    // same arc would span 20·sin(1°) = 0.349 mm.
    let (min, max, n_points) = cartesian_point_aabb(&stripped);
    assert!(
        n_points > 0,
        "expected at least one 3D CARTESIAN_POINT in the wireframe STEP export, found none"
    );
    let expected_y = 20.0 * (1.0_f64).sin();
    let y_extent = max[1] - min[1];
    assert!(
        (y_extent - expected_y).abs() < 1e-6,
        "a 20 mm-radius arc swept through 1 RADIAN should span 20·sin(1) = {expected_y} mm on \
         y, but the CARTESIAN_POINT AABB y-extent is {y_extent} (min {}, max {}) — \
         declaration and payload disagree. 20·sin(1°) = {} is what a degree misreading of the \
         trim parameter would give.",
        min[1],
        max[1],
        20.0 * (1.0_f64).to_radians().sin()
    );
}

/// Real-OCCT schema selection through the new `export_with_options`.
///
/// Asserts the declared STEP schema reaches the OCCT writer and is
/// observable in the written FILE_SCHEMA. The assertions use the actual
/// OCCT EXPRESS schema identifiers (verified against linked OCCT 7.9.3):
/// AP203 → `CONFIG_CONTROL_DESIGN`, AP214 → `AUTOMOTIVE_DESIGN`,
/// AP242 → a name containing `AP242`. The literal token "AP203" is never
/// written by OCCT, so we assert the EXPRESS schema name instead.
///
/// All three exports run in one process, so they share the process-global
/// `write.step.schema` static — this is exactly the case the per-call
/// `Interface_Static::SetCVal` must make deterministic.
#[test]
fn export_with_options_selects_step_schema() {
    use reify_ir::{ExportFormat, ExportOptions, StepSchema};
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();

    // (a) AP203 → CONFIG_CONTROL_DESIGN, never AUTOMOTIVE_DESIGN.
    let mut buf_203 = Vec::new();
    let w_203 = handle
        .export_with_options(
            gh.id,
            ExportFormat::Step,
            &ExportOptions {
                step_schema: StepSchema::Ap203,
                ..ExportOptions::default()
            },
            &mut buf_203,
        )
        .unwrap();
    let content_203 = String::from_utf8(buf_203).unwrap();
    assert!(
        content_203.contains("CONFIG_CONTROL_DESIGN"),
        "AP203 export must write the CONFIG_CONTROL_DESIGN EXPRESS schema"
    );
    assert!(
        !content_203.contains("AUTOMOTIVE_DESIGN"),
        "AP203 export must NOT write the AP214 AUTOMOTIVE_DESIGN schema"
    );
    assert!(w_203.is_empty(), "AP203 export raises no warnings");

    // (b) default (AP214) → AUTOMOTIVE_DESIGN.
    let mut buf_def = Vec::new();
    let w_def = handle
        .export_with_options(
            gh.id,
            ExportFormat::Step,
            &ExportOptions::default(),
            &mut buf_def,
        )
        .unwrap();
    let content_def = String::from_utf8(buf_def).unwrap();
    assert!(
        content_def.contains("AUTOMOTIVE_DESIGN"),
        "default (AP214) export must write the AUTOMOTIVE_DESIGN schema"
    );
    assert!(w_def.is_empty());

    // (c) The schema really changed: AP203 and AP214 bytes differ.
    assert_ne!(
        content_203, content_def,
        "AP203 and AP214 exports must differ in their FILE_SCHEMA"
    );

    // (d) AP242 → schema name contains "AP242". OCCT 7.9.3 supports
    // AP242DIS, so the happy path succeeds with no fallback warning.
    let mut buf_242 = Vec::new();
    let w_242 = handle
        .export_with_options(
            gh.id,
            ExportFormat::Step,
            &ExportOptions {
                step_schema: StepSchema::Ap242,
                ..ExportOptions::default()
            },
            &mut buf_242,
        )
        .unwrap();
    let content_242 = String::from_utf8(buf_242).unwrap();
    assert!(
        content_242.contains("AP242"),
        "AP242 export must write a schema name containing AP242"
    );
    assert!(
        w_242.is_empty(),
        "OCCT 7.9.3 supports AP242DIS — AP242 happy path raises no warning"
    );
}

/// The plain `export(Step)` path is unchanged by the options plumbing: it
/// still writes the ISO-10303-21 header and defaults to the AP214
/// AUTOMOTIVE_DESIGN schema (the per-call SetCVal on the default path
/// keeps this deterministic even after a prior AP203 export in-process).
#[test]
fn plain_export_step_still_writes_default_ap214_schema() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let mut buf = Vec::new();
    handle
        .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();
    assert!(
        content.contains("ISO-10303-21"),
        "plain export must still write the ISO-10303-21 header"
    );
    assert!(
        content.contains("AUTOMOTIVE_DESIGN"),
        "plain export must default to the AP214 AUTOMOTIVE_DESIGN schema"
    );
}
