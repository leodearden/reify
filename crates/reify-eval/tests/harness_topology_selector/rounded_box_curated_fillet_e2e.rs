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
//! Measured on OCCT 7.8 (architect probe, this worktree):
//!   * RED   — the realized body has **66** faces.
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
