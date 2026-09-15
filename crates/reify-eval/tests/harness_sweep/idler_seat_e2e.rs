//! End-to-end acceptance gate for the printer_v01 `IdlerPulley`'s rope seat
//! (task #6135), which extends the DIN 15061 oversize seat arc from the
//! Capstan — where #5683 landed it — to the idler sheave, in lockstep across
//! the design's two copies of the structure.
//!
//! The sibling module [`super::capstan_groove_e2e`] gates the Capstan end of
//! the same standard; this one reuses its externally-pinned ratio
//! ([`super::capstan_groove_e2e::DIN_15061_SEAT_RATIO`]) and its
//! construction-independent arc-centre formula
//! ([`super::capstan_groove_e2e::seat_arc_centre`]) rather than restating
//! either. What does NOT carry over is that module's
//! `MIN_MOUTH_CLEARANCE_FRAC`: it is the Capstan's land-at-arc-centre case and
//! over-predicts the mouth here, where the rim is pinned at the rope
//! centreline instead. The floor is re-derived from the ratio below.
//!
//! **Why the seat moves at all.** Both copies cut the seat with a `torus`
//! whose tube radius was the tendon's own radius — the zero-clearance slip fit
//! DIN 15061 exists to open up. Oversizing the tube to `0.53·d` alone would
//! sink the seated rope 0.180 mm below the rim, and `sheave_od/2 == r_pitch`
//! is not decoration in this design: `printer.ri` threads it into
//! `CapstanUnit.r_pitch`, into `CarriageIdlers.ab_split` and into
//! `DriveTendons` ("every tendon centreline is tangent to its rope's pitch
//! circle"), behind 31 hand-derived placements. So the arc's CENTRE moves
//! outboard to compensate, and two exact, ratio-independent identities fall
//! out — the seat bottom and the seated rope's centreline both stay where they
//! were. Those identities are what the gates here pin.
//!
//! **Why this module is split across two design files.** The contract lives in
//! `prj/printer_v01/printer.ri`, which is the original, and is read
//! KERNEL-FREE (`compile_with_stdlib_checked` + `Engine::check`): cells and
//! `constraint_results` only. `Engine::tessellate_realizations` takes no
//! entity or scope argument, so tessellating printer.ri means tessellating all
//! 32 of its structures — 3635 lines, 31 torus-boolean idlers, never once
//! tessellated by any test in this repo and plausibly minutes. A full stdlib
//! COMPILE of it, by contrast, already runs in CI today
//! (`crates/reify-compiler/tests/harness_constructor_typing/orientation_constructor_typing_tests.rs`),
//! so the kernel-free surface is proven affordable. The OCCT mesh readback
//! therefore comes from `prj/printer_v01/dev_capstan.ri` (345 lines, ~5 s),
//! which carries four `IdlerPulley` instances in its `Fairlead` shuttle — and
//! `idler_copies_stay_in_lockstep` is precisely the assertion that licenses
//! taking the EXPECTATIONS from one file and the MESH from the other.
