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
//!
//! # The measured kernel-free surface of printer.ri (task #6135, pre-2)
//!
//! Every figure and every allowlist entry below was MEASURED on this branch
//! before the first assertion was written, so the loader's error handling is
//! sized to the file as it actually is. printer.ri parses with 0 errors, and
//! `IdlerPulley`'s eleven cells and `DriveTendons.r_pitch` all resolve off the
//! bare template — printer.ri instantiates `IdlerPulley` 31 times but task
//! 4147 drops parameter overrides, so the bare-template form is the one to
//! read. `DriveTendons.r_pitch` and `IdlerPulley.sheave_r` both measure
//! 0.018000000000000002 m: BIT-IDENTICAL, because `36mm / 2` and `18mm` are
//! the same IEEE-754 double. `IdlerPulley`'s three constraints are all
//! `Satisfied`, out of 406 file-wide across 29 entities.
//!
//! Pre-change `IdlerPulley` figures, for the delta claims the gates below
//! make: rim 18.000 mm, seat bottom 15.000 mm, seat opening at the rim
//! 6.000 mm, sheave width 10.000 mm.
//!
//! **The file emits Error-severity diagnostics in TWO populations, and they
//! are allowlisted asymmetrically.** That asymmetry is the measurement's real
//! finding; a single blanket filter would have hidden both, and treating them
//! alike would make this gate demand that a bug stay unfixed.
//!
//! 1. **Six `EvalUnresolved` at the CHECK stage** — `AFrame.vol_body` and the
//!    five `ToolDock.pen_*` cells, every one of them a `volume()` consumer.
//!    This is the exact analogue of
//!    [`super::capstan_groove_e2e`]'s `VOLUME_UNRESOLVED_CELLS`: `volume()` is
//!    a geometry-consumer builtin resolvable only on the build()/tessellate()
//!    path, so these are a PERMANENT property of the kernel-free surface.
//!    Allowlisted as an exact identity set, both directions — a missing entry
//!    means the cell was dropped or renamed.
//!
//! 2. **Eleven `UnresolvedName` at the COMPILE stage** — qualified
//!    enum-variant paths (`Finish.Satin`, `ElementOrder.P2`, `ShellForce.Off`)
//!    across eight cells: `CFRP_Rolled_Tube.appearance`,
//!    `HomogenisedPanel.appearance`, `GantryFea.{r_static, opts_cant,
//!    opts_ss}` and `AFrame.{opts_field, r_pil, mc}`. These are a PRE-EXISTING
//!    compiler/stdlib gap on main, not something this task introduced: the
//!    enum-name scope is built from the module's own `enum_defs`
//!    (`crates/reify-compiler/src/entity.rs`), and printer.ri is a single file
//!    that never imports the modules defining these three enums. Nothing in
//!    the repo observes them today — the one existing test that compiles
//!    printer.ri counts *infer warnings* only
//!    (`orientation_constructor_typing_tests::real_printer_ri_emits_zero_infer_warnings`).
//!    So they are allowlisted as a CEILING rather than an expectation: these
//!    cells MAY raise it and the loader tolerates them, but none is required
//!    to. Fixing the gap therefore makes this gate greener, never redder —
//!    which is the whole reason population 2 is not held to population 1's
//!    exact-identity rule. Filed as an observation; none of the eight cells is
//!    in `IdlerPulley` or `DriveTendons`, so none touches this gate's subject.
//!
//! Both populations are recognised by CELL IDENTITY, never by message text —
//! the prose belongs to another crate and a rewording of it must not reroute a
//! diagnostic. Population 1 resolves the label span through the compiled
//! module's value cells exactly as the capstan gate does. Population 2 needs
//! one refinement: its label sits on an expression INSIDE a cell rather than on
//! the cell itself, so the identity is the smallest value cell whose span
//! CONTAINS the label's. Both are computed against the same compilation the
//! diagnostics came from, so neither hard-codes a byte offset and edits to this
//! file's own `IdlerPulley` cannot shift them.
