// SPDX-License-Identifier: AGPL-3.0-or-later

//! Structured `Toolpath` value + PrusaSlicer G-code-comment parser (task ζ).
//!
//! See `docs/prds/v0_5/fdm-as-printed-fea.md` §"The Toolpath representation"
//! (task ζ, slice 2). A [`Toolpath`] is an ordered, layer-segmented bead graph:
//! each bead carries its centerline polyline, extrusion width/height, a
//! structural [`BeadRole`], its owning layer index + layer-Z, the nominal
//! extruder temperature, and the active speed; the toolpath additionally
//! records in-layer and inter-layer bead adjacency. The downstream θ
//! `FDMPrint` constitutive mapping consumes this graph (and owns the mm→SI
//! conversion — this module stores native G-code millimetres / mm·min⁻¹
//! exactly as parsed, losslessly).
//!
//! # Why this lives here and not in reify-gcode
//!
//! `reify-gcode` is the low-level command parser; the `Toolpath` abstraction
//! is owned here (PRD design decision #5 — "reify-gcode stays the low-level
//! parser beneath it"). Critically, `reify_gcode::parse_marlin` strips every
//! `;`-to-EOL comment via `strip_comment_and_trim`, so a whole-source call
//! would throw away exactly the `;TYPE:` / `;WIDTH:` / `;HEIGHT:` /
//! `;LAYER_CHANGE` / `;Z:` markers this builder needs — and lose the
//! comment↔move interleaving that tags each bead. Therefore the parser here
//! walks physical lines itself (owning the comment state machine + position
//! sweep) and delegates ONLY G0/G1/G2/G3/G92 move lines to
//! `reify_gcode::parse_marlin(line)` per-line. reify-gcode is reused, not
//! modified.
