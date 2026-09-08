//! Designer-facing end-to-end acceptance for OCCT boolean-result normalization
//! (task #7054, acceptance criterion (c)).
//!
//! The kernel-level half lives in
//! `crates/reify-kernel-occt/tests/harness_occt/boolean_result_normalization_integration.rs`;
//! this module pins the same defect one layer up, through the whole
//! parse → compile → `Engine::build` pipeline, using the exact idiom a designer
//! writes:
//!
//! ```text
//! let body = rounded_box(100mm, 60mm, 20mm, 10mm)
//! fillet(translate(body, 0mm, 0mm, 10mm), edges_at_height(body, 20mm, 0.5mm), 1mm)
//! ```
//!
//! `rounded_box` desugars (compiler `emit_rounded_union_compose`) into a
//! left-folded chain of five binary fuses over two boxes and four corner
//! cylinders. Without normalization that chain leaves the top face split into
//! same-domain fragments, so `edges_at_height` — whose predicate is bbox-only
//! (reify-eval `topology_selectors.rs`) — selects the phantom seam edges too.
//!
//! Measured on OCCT 7.8:
//!   * RED   — on THIS fixture (the translated body) `BRepFilletAPI_MakeFillet`
//!     fails outright over the 40-edge selection: the build reports
//!     `geometry operation failed: OCCT make_fillet_edges_with_history` and
//!     exports no product geometry at all (0 faces, `Deck.mass` = Undef).
//!     The architect probe measured the SAME un-unified rim fillet SUCCEEDING
//!     with 66 faces on the untranslated body — which is exactly why symptom 3
//!     is recorded as fixture-dependent, and why nothing here asserts that
//!     today's behaviour is an error.
//!   * GREEN — the realized body has **18** faces (10 unified faces of the
//!     prism + 8 rim-fillet faces).
//!   * mass is **150.137 g** in BOTH states — bit-identical (118218.498221 mm³
//!     at the fixture's 1.27 g/cm³), because the 40 phantom seam edges sweep
//!     exactly the same material as the 8 real ones. The mass assertion here is
//!     an invariance guard proving the fix removed no material; it is **not**
//!     the RED signal. Anchoring on mass alone would be a false green.
//!
//! Deliberately NOT asserted: `IsWatertight` on the filleted body.
//! `BRepFilletAPI_MakeFillet::Shape()` was measured returning a bare COMPOUND
//! too — a genuine sibling defect with its own blast radius over
//! `per_edge_fillet` / `per_edge_chamfer` / the shell suites, explicitly out of
//! scope for #7054 and filed as a follow-up.
//!
//! Every test is gated on `reify_kernel_occt::OCCT_AVAILABLE` and skips with an
//! `eprintln!` when OCCT is absent, matching the sibling e2e modules. The build
//! helper drives `OcctKernelHandle::spawn()` DIRECTLY rather than wrapping it in
//! `SingleKernelHolder`: the holder does not forward `extract_faces` /
//! `extract_edges` to the inner kernel, so the face-count assertion this module
//! is built around would silently read an empty table instead of failing loudly
//! (documented caveat copied from `topology_attribute_boolean_e2e.rs`).

use reify_ir::{ExportFormat, Value};
use reify_core::{DimensionVector, Severity, ValueCellId};

/// The acceptance idiom, in the shape a designer actually writes it — the same
/// `aux let` blank + `edges_at_height` + curated 3-arg `fillet` construction
/// `designs/litter_tray/bottom_deck.ri` uses. That file carries a round-3
/// comment saying `rounded_box` had to be abandoned for exactly this reason
/// ("every later `edges_at_height` fillet then fails in OCCT").
///
/// Density 1270 kg/m³ (PETG) makes the expected mass land on the architect's
/// measured 150.137 g.
const SOURCE: &str = r#"default Material = Material(name: "petg", density: 1270kg/m^3, youngs_modulus: 2GPa)

structure def Deck : Physical {
    aux let blank = translate(rounded_box(100mm, 60mm, 20mm, 10mm), 0mm, 0mm, 10mm)
    param geometry : Solid = fillet(blank, edges_at_height(blank, 20mm, 0.5mm), 1mm)
}
"#;

/// Volume of the rim-filleted body, in mm³ (architect probe, OCCT 7.8).
/// Bit-identical with and without unification — see the module header.
const FILLETED_VOLUME_MM3: f64 = 118218.498221;
/// PETG density declared by `SOURCE`, in kg/m³.
const DENSITY_KG_PER_M3: f64 = 1270.0;
/// 118218.498221 mm³ × 1270 kg/m³ = 0.150137492741 kg = 150.137 g.
const EXPECTED_MASS_KG: f64 = FILLETED_VOLUME_MM3 * 1.0e-9 * DENSITY_KG_PER_M3;
/// 10 unified faces of the prism + 8 rim-fillet faces.
const EXPECTED_FACES: usize = 18;

/// Count the faces of the exported product body.
///
/// A STEP AP203/214 part writes exactly one `ADVANCED_FACE` entity per face of
/// the shell — verified against this same pipeline with a `box` (6) plus a
/// `cylinder` (3), which exported 9. Counting the product artifact is the most
/// designer-faithful face-count observable available at the eval layer: the
/// engine owns its kernel, so a test cannot re-query `extract_faces`, and
/// `topology_attribute_table()` accumulates every intermediate realization
/// rather than just the final body.
fn step_face_count(step: &[u8]) -> usize {
    String::from_utf8_lossy(step).matches("ADVANCED_FACE(").count()
}

/// End-to-end acceptance (c): the designer-facing `rounded_box` + curated rim
/// fillet must realize an 18-face body of 150.137 g.
///
/// RED today: `rounded_box` desugars to a five-fuse chain whose coplanar seams
/// are never unified, so `edges_at_height` selects the 40 phantom seam edges
/// instead of the 8 real rim edges and `BRepFilletAPI_MakeFillet` fails
/// outright on this fixture — the build reports
/// `geometry operation failed: OCCT make_fillet_edges_with_history` and exports
/// no product geometry at all (0 faces, mass Undef).
///
/// The assertion is deliberately on the GREEN contract, never on today's error:
/// the architect probe measured the SAME un-unified rim fillet SUCCEEDING (with
/// 66 faces) on the untranslated body, so "the fillet errors" is a
/// fixture-dependent symptom and pinning it would be flaky.
#[test]
fn rounded_box_curated_rim_fillet_realizes_an_eighteen_face_body() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(SOURCE);

    // The OCCT kernel is passed DIRECTLY, not wrapped in `SingleKernelHolder`:
    // the holder does not forward `extract_faces` / `extract_edges`, which
    // `edges_at_height` needs to resolve its selection at all.
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(
        Box::new(checker),
        Some(Box::new(reify_kernel_occt::OcctKernelHandle::spawn())),
    );
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "the curated rim fillet over a rounded_box must build cleanly; got: {build_errors:#?}"
    );

    let step = result
        .geometry_output
        .as_deref()
        .expect("build must export product geometry for Deck.geometry");
    let faces = step_face_count(step);
    assert_eq!(
        faces, EXPECTED_FACES,
        "the realized body must have {EXPECTED_FACES} faces (10 unified prism \
         faces + 8 rim-fillet faces); a larger count means the fuse chain's \
         coplanar seams survived and the rim selection picked up phantom edges \
         (measured RED on the untranslated body: 66)"
    );

    match result.values.get(&ValueCellId::new("Deck", "mass")) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::MASS,
                "Deck.mass must be MASS-dimensioned"
            );
            assert!(
                (si_value - EXPECTED_MASS_KG).abs() <= 1.0e-6,
                "Deck.mass must be {:.9} kg (= {:.3} g); got {si_value:.9} kg. \
                 This is an INVARIANCE guard proving the fix removed no material \
                 — the mass is bit-identical with and without unification, so it \
                 is NOT the RED signal for this test.",
                EXPECTED_MASS_KG,
                EXPECTED_MASS_KG * 1000.0
            );
        }
        other => panic!(
            "Deck.mass must be a MASS scalar, got {other:?} \
             (Undef means the geometry never realized)"
        ),
    }
}
