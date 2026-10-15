//! Acceptance ladder for `sweep`'s degenerate-pose guard (task #7248).
//!
//! `sweep(profile, path)` transports the profile **as posed**; it never orients
//! it onto the spine, so the swept volume is
//!
//! ```text
//!     V = (profile area) · (spine displacement · profile normal)
//! ```
//!
//! As-posed transport is the intended contract (esc-7248-3). A profile posed
//! edge-on to its spine is therefore refused with a typed error when
//! `|t̂·n̂| < sin(4°)`, never auto-oriented.
//!
//! The rungs:
//! - 1 and 2 are refused: the in-plane spine, and 3.8° just below the band.
//! - 2b, 2c and 3 realize: 4.2° just above the band, 4.6149° as a real-subject
//!   proxy, and 6° as a wide-margin fence.
//! - 4 and 5 are controls: a +Z spine, and 45° holding the as-posed volume.
//! - 6: `pipe()` orients its own section and is untouched by the guard.
//! - 7: a CURVE profile is refused one layer up, by the compiler.
//! - 8: the inline posed spelling the refusal suggests realizes.
//!
//! Sources are embedded inline rather than read from disk, so this file adds no
//! entry to `_RUST_COUPLED_RI_FIXTURES` (`scripts/verify.sh`).

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::compile_source_with_stdlib;

/// `rectangle(20mm, 20mm)` in m² — the swept section of every spine rung.
const PROFILE_AREA_M2: f64 = 0.020 * 0.020;

/// Relative tolerance for an admitted PRISM rung's volume.
///
/// The law above is exact arithmetic on the kernel's own numbers, not an
/// approximation, and every rung it governs is a planar-faced prism whose
/// volume integral OCCT evaluates exactly, so this bounds float round-trip
/// only.
const PRISM_VOLUME_REL_TOL: f64 = 1e-9;

/// Relative tolerance for rung 6, the one admitted rung that is NOT a
/// planar-faced prism.
///
/// `pipe()` builds a real cylinder and rung 6 checks it against analytic
/// π·r²·L, so the integrand is not polynomial and the exactness argument above
/// does not carry: at 1e-9 that rung would be pinning OCCT's quadrature rather
/// than the pose guard, and would red for a reason unrelated to the band. Still
/// three decades tighter than the 5e-2 its sibling
/// `sweep_circle_along_line_creates_pipe` uses.
const CYLINDER_VOLUME_REL_TOL: f64 = 1e-6;

/// What one rung of the ladder observed: which layer refused the design, and
/// whether a body with a readable volume came out the other side.
///
/// The two error lists are kept apart because every assertion below turns on
/// WHICH layer spoke. "Compiles clean, and the kernel refuses it" (rungs 1, 2)
/// is a different claim from "the compiler never let it through" (rung 7), and
/// a single merged list could not tell them apart.
struct Rung {
    compile_errors: Vec<String>,
    realize_errors: Vec<String>,
    /// The swept body's volume in m³, or `None` if no body realized — the cell
    /// is absent or `Undef`. A cell that is neither of those and is also not a
    /// VOLUME-dimensioned scalar is a defect in its own right and panics.
    volume_m3: Option<f64>,
}

/// Compile `source`, realize it on a real OCCT kernel, and read cell `S.v`.
///
/// Returns `None` when OCCT is unavailable. Eval-layer tests cannot use
/// `#[cfg(has_occt)]` — that cfg is set by reify-kernel-occt's own build.rs —
/// so the gate is the runtime flag.
///
/// The terminal is `realize_for_check`, which is what `reify eval` and `reify
/// check` run. Do NOT substitute `Engine::eval`: it mints placeholder handles
/// and never reaches OCCT, so an eval-based assertion about kernel behaviour
/// would be vacuous.
fn rung(source: &str) -> Option<Rung> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    let compiled = compile_source_with_stdlib(source);
    let compile_errors = messages(&compiled.diagnostics);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.realize_for_check(&compiled);

    let volume_m3 = match result.values.get(&ValueCellId::new("S", "v")) {
        None | Some(Value::Undef) => None,
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "cell S.v must carry VOLUME dimension, got {dimension:?}"
            );
            Some(*si_value)
        }
        Some(other) => panic!("expected a VOLUME scalar or Undef in cell S.v, got {other:?}"),
    };

    Some(Rung {
        compile_errors,
        realize_errors: messages(&result.diagnostics),
        volume_m3,
    })
}

/// The Error-severity messages of a diagnostic list.
fn messages(diagnostics: &[reify_core::Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// Assert some message contains every word in `words`.
///
/// Deliberately matches a small word set rather than an exact string, and
/// deliberately asserts no diagnostic COUNT: a refused design may legitimately
/// report more than one failure, and the exact wording is owned by the kernel
/// validator, not by this file.
fn assert_mentions(messages: &[String], words: &[&str], what: &str) {
    assert!(
        messages.iter().any(|m| words.iter().all(|w| m.contains(w))),
        "{what}: expected an Error diagnostic containing all of {words:?}; got: {messages:?}"
    );
}

/// One spine rung: the `.ri` module to realize, and the volume it must yield.
///
/// Both are derived from a single spelling of the spine coordinates, so a
/// rung cannot assert one spine while measuring another.
struct SweptBox {
    source: String,
    expected_m3: f64,
}

/// A `.ri` module sweeping a 20mm × 20mm section along a straight 200 mm spine
/// that leaves the origin for `(x_mm, 0, z_mm)`, reading the result into `S.v`,
/// paired with the volume that module must produce: section area times the
/// spine displacement's component along the section normal, which is +Z.
///
/// The coordinates arrive as source TEXT, not as `f64`, so the literal each
/// rung's `|t̂·n̂|` was computed from is the literal the kernel sees. The
/// expected volume is parsed back out of that same literal rather than
/// transcribed a second time: a slip in a hand-copied `f64` would silently
/// change what the rung asserts instead of failing to compile.
fn swept_box_along_spine(x_mm: &str, z_mm: &str) -> SweptBox {
    SweptBox {
        source: format!(
            "structure S {{\n    \
             let b = sweep(rectangle(20mm, 20mm), line_segment(0mm, 0mm, 0mm, {x_mm}mm, 0mm, {z_mm}mm))\n    \
             let v = volume(b)\n\
             }}\n"
        ),
        expected_m3: PROFILE_AREA_M2
            * z_mm
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("spine z literal {z_mm:?} must parse as f64: {e}"))
            / 1000.0,
    }
}

/// Assert a PRISM rung compiled clean, realized silently, and produced
/// `expected` m³ to within [`PRISM_VOLUME_REL_TOL`].
fn assert_admitted(rung: &Rung, expected_m3: f64, what: &str) {
    assert_admitted_within(rung, expected_m3, PRISM_VOLUME_REL_TOL, what);
}

/// [`assert_admitted`] with the tolerance named explicitly, for the one rung
/// the prism exactness argument does not cover.
fn assert_admitted_within(rung: &Rung, expected_m3: f64, rel_tol: f64, what: &str) {
    assert!(
        rung.compile_errors.is_empty(),
        "{what}: should compile clean, got: {:?}",
        rung.compile_errors
    );
    assert!(
        rung.realize_errors.is_empty(),
        "{what}: should realize with no Error diagnostic, got: {:?}",
        rung.realize_errors
    );
    let actual = rung
        .volume_m3
        .unwrap_or_else(|| panic!("{what}: no body realized, so there is no volume to check"));
    let rel_err = (actual - expected_m3).abs() / expected_m3;
    assert!(
        rel_err < rel_tol,
        "{what}: expected {expected_m3:.9e} m³, got {actual:.9e} \
         (rel_err={rel_err:.3e}, tolerance {rel_tol:.0e})"
    );
}

/// Assert the KERNEL refused a rung that the compiler accepted: an Error-
/// severity geometry diagnostic naming the degenerate pose, and no body.
fn assert_refused_by_the_kernel(rung: &Rung, what: &str) {
    assert!(
        rung.compile_errors.is_empty(),
        "{what}: must reach the kernel to be refused BY it, but the compiler \
         rejected it first: {:?}",
        rung.compile_errors
    );
    assert_mentions(
        &rung.realize_errors,
        &["geometry error", "sweep", "tangent"],
        what,
    );
    assert_eq!(
        rung.volume_m3, None,
        "{what}: a refused sweep must realize no body, but one came back with \
         a readable volume"
    );
}

// ── The band: refused below, admitted above ──────────────────────────────────

/// Rung 1 — the reported defect. A spine lying in the section's own plane
/// (`|t̂·n̂|` = 0 exactly) must be refused. Before the guard it realized
/// silently at 0 m³, which is the whole complaint: a design that produced
/// nothing reported success.
#[test]
fn in_plane_spine_is_refused() {
    let Some(rung) = rung(&swept_box_along_spine("200", "0").source) else {
        return;
    };
    assert_refused_by_the_kernel(&rung, "in-plane (+X) spine");
}

/// The refusal's four facts must survive the trip to the user, not just exist
/// inside the kernel. The eval layer composes the kernel's payload into
/// `geometry error: geometry operation failed: <msg>`, which is verbatim the
/// stderr line `reify eval` prints, so asserting on `Diagnostic.message` here
/// covers the CLI surface without shelling out.
///
/// The same fact set is pinned kernel-side
/// (`degenerate_pose_error_carries_every_fact_the_author_needs`). That is not
/// accidental duplication: the kernel test pins what the validator PRODUCES,
/// this one pins what SURVIVES two layers of wrapping. Either could go green
/// while the other reds.
#[test]
fn the_refusal_reaches_the_user_with_every_fact_intact() {
    let Some(rung) = rung(&swept_box_along_spine("200", "0").source) else {
        return;
    };
    assert_mentions(
        &rung.realize_errors,
        &[
            "geometry error",
            "4 degrees",
            "0.000000000",
            "0.000 degrees off-plane",
            "encloses almost nothing",
            "pose the profile normal",
            "start tangent (1.000000,",
            "sweep(rotate(",
        ],
        "in-plane spine, as the author sees it",
    );
    assert!(
        !rung
            .realize_errors
            .iter()
            .any(|m| m.contains("rejected as a sweep profile")),
        "the refusal must not claim an inline posed profile is rejected; rung 8 \
         realizes one. Got: {:?}",
        rung.realize_errors
    );
}

/// Rung 2 — 3.8°, `|t̂·n̂|` = 0.066273900, 5.0% BELOW the floor. The lower half
/// of the straddle that pins the constant. Before the guard it realized at
/// 5.30191204e-6 m³ — a valid-looking sliver, which is why an exact predicate
/// would not have been enough.
#[test]
fn spine_just_below_the_band_is_refused() {
    let Some(rung) = rung(&swept_box_along_spine("199.5602937", "13.2547801").source) else {
        return;
    };
    assert_refused_by_the_kernel(&rung, "3.8° spine (5% below the floor)");
}

/// Rung 2b — 4.2°, `|t̂·n̂|` = 0.073238197, 5.0% ABOVE the floor. The upper half
/// of the straddle: together with rung 2 it pins the band to ±0.2°, so a
/// constant that drifted in either direction reds here.
#[test]
fn spine_just_above_the_band_realizes() {
    let subject = swept_box_along_spine("199.4628954", "14.6476394");
    let Some(rung) = rung(&subject.source) else {
        return;
    };
    assert_admitted(
        &rung,
        subject.expected_m3,
        "4.2° spine (5% above the floor)",
    );
}

/// Rung 2c — 4.6149°, constructed to reproduce `|t̂·n̂|` = 0.080458525, the
/// MEASURED pose of `tests/prd-gate/fixtures/pnrg_envelope_sweep.ri`. It is the
/// tightest real in-tree constraint on how wide the band may grow.
///
/// This rung is load-bearing and must not be dropped as redundant with 2b. That
/// fixture is referenced by no probe-set JSON and by no `.rs`/`.sh`/`.py`/
/// `.toml` file anywhere — only by itself and by
/// `docs/notes/precision-refine-envelope-calibration.md` — so NOTHING else in
/// the tree would go red if a future widening past 4.6149° silently broke it.
/// This proxy encodes that constraint at zero measurement cost: the real interp
/// subject costs ~16 s to realize, this one is a straight line segment.
#[test]
fn real_subject_pose_proxy_realizes() {
    let subject = swept_box_along_spine("199.3515915", "16.0917050");
    let Some(rung) = rung(&subject.source) else {
        return;
    };
    assert_admitted(
        &rung,
        subject.expected_m3,
        "4.6149° spine (pnrg_envelope_sweep.ri pose proxy)",
    );
}

/// Rung 3 — 6°, `|t̂·n̂|` = 0.104528464. The wide-margin fence: 50% above the
/// floor, so it fails only on a band that has grown grossly rather than drifted.
#[test]
fn spine_well_above_the_band_realizes() {
    let subject = swept_box_along_spine("198.9043790", "20.9056927");
    let Some(rung) = rung(&subject.source) else {
        return;
    };
    assert_admitted(&rung, subject.expected_m3, "6° spine (wide-margin fence)");
}

// ── Controls: what the guard must leave alone ────────────────────────────────

/// Rung 4 — the positive control. A spine normal to the section builds a
/// genuinely closed, capped 20×20×200 box. Its 8e-5 m³ is the same code path as
/// every refused rung above, which is what establishes that the FACE-profile
/// sweep does cap and that the refusals are about pose, not about capping.
#[test]
fn normal_spine_realizes() {
    let subject = swept_box_along_spine("0", "200");
    let Some(rung) = rung(&subject.source) else {
        return;
    };
    assert_admitted(&rung, subject.expected_m3, "+Z spine (positive control)");
}

/// Rung 5 — the as-posed contract. At 45° the swept volume is the section area
/// times the spine's PROJECTION onto the section normal (141.4213562 mm), not
/// the spine's length (200 mm). A fix that moved this value to 8e-5 would have
/// auto-oriented the profile — refused by the esc-7248-3 ruling, because
/// designs pose their sweep profiles deliberately.
#[test]
fn as_posed_transport_is_preserved() {
    let subject = swept_box_along_spine("141.4213562", "141.4213562");
    let Some(rung) = rung(&subject.source) else {
        return;
    };
    assert_admitted(&rung, subject.expected_m3, "45° spine (as-posed transport)");
}

/// Rung 6 — the working control. `pipe()` derives its own section normal from
/// the path's start tangent, so an in-plane SPINE is not a degenerate pose for
/// it at all: `pipe(+X line, 10mm)` is a legitimate 200 mm rod. The guard must
/// not reach it.
#[test]
fn pipe_along_the_same_in_plane_spine_realizes() {
    const SOURCE: &str = r#"structure S {
    let b = pipe(line_segment(0mm, 0mm, 0mm, 200mm, 0mm, 0mm), 10mm)
    let v = volume(b)
}
"#;
    let Some(rung) = rung(SOURCE) else {
        return;
    };
    // π·(10mm)²·200mm
    let expected = std::f64::consts::PI * 0.010_f64.powi(2) * 0.200;
    assert_admitted_within(
        &rung,
        expected,
        CYLINDER_VOLUME_REL_TOL,
        "pipe along the in-plane spine",
    );
}

/// Rung 7 — the compiler half stays closed. A CURVE profile has no surface
/// normal for the pose guard to compare against, so the guard deliberately
/// fails OPEN on one; what keeps `sweep(<Curve>, <Curve>)` out is the
/// compiler's profile precondition, one layer up. Pin that measured behaviour
/// so the two halves cannot both quietly stop holding.
///
/// Only the COMPILE diagnostic is asserted. Handed the module anyway, the
/// engine still realizes — measured: a single 20mm × 200mm face reading 0 m³,
/// which is precisely the mislabelled-`Solid` defect that #7248's honest
/// `BRepKind` stamp addresses, not something the pose guard should claim. What
/// makes the refusal user-observable is the CLI's generic compile-error gate:
/// `reify eval` on this source exits 1 and prints no value cells at all. That
/// gate is not sweep-specific and is not this file's to pin.
#[test]
fn curve_profile_is_refused_by_the_compiler() {
    const SOURCE: &str = r#"structure S {
    let b = sweep(line_segment(0mm, 0mm, 0mm, 20mm, 0mm, 0mm), line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 200mm))
    let v = volume(b)
}
"#;
    let Some(rung) = rung(SOURCE) else {
        return;
    };
    assert_mentions(
        &rung.compile_errors,
        &["profile", "Surface"],
        "curve profile",
    );
}

/// Rung 8 — the refusal's advice must be followable as written. It suggests an
/// INLINE posed profile, `sweep(rotate(<profile>, …), path)`; the compiler's
/// profile precondition sees through the transform to the Closed ∧ Planar
/// section (#6188), and the pose puts the section normal along the +Y spine.
#[test]
fn an_inline_posed_profile_realizes() {
    const SOURCE: &str = r#"structure S {
    let b = sweep(rotate(rectangle(20mm, 20mm), 1.0, 0.0, 0.0, -90deg), line_segment(0mm, 0mm, 0mm, 0mm, 200mm, 0mm))
    let v = volume(b)
}
"#;
    let Some(rung) = rung(SOURCE) else {
        return;
    };
    assert_admitted(
        &rung,
        PROFILE_AREA_M2 * 0.200,
        "inline posed profile along a +Y spine",
    );
}
