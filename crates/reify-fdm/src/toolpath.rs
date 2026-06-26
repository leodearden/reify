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

/// Structural role of a deposited bead, distilled from PrusaSlicer's much
/// larger `ExtrusionRole` (`;TYPE:` comment) vocabulary into the five classes
/// the downstream θ constitutive mapping distinguishes.
///
/// Sacrificial / non-part roles (skirt, brim, wipe tower, …) have **no**
/// variant here — [`role_from_prusaslicer_type`] returns `None` for them and
/// their extrusions are skipped, so they never pollute the bead graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeadRole {
    /// Perimeter shell — PrusaSlicer `Perimeter`, `External perimeter`,
    /// `Overhang perimeter`.
    Perimeter,
    /// Dense solid region — PrusaSlicer `Solid infill`, `Top solid infill`,
    /// `Bottom solid infill`, plus `Gap fill` and `Ironing` (both dense solid
    /// material).
    SolidInfill,
    /// Sparse interior lattice — PrusaSlicer `Internal infill`.
    SparseInfill,
    /// Bridged span — PrusaSlicer `Bridge infill`, `Internal bridge infill`.
    Bridge,
    /// Support structure — PrusaSlicer `Support material`,
    /// `Support material interface`.
    Support,
}

/// Map a PrusaSlicer `;TYPE:` value (the trimmed string after the colon) to a
/// structural [`BeadRole`], or `None` for a sacrificial / non-part /
/// unrecognised type whose extrusions must be skipped.
///
/// Matching is **exact** (case-sensitive) on the canonical PrusaSlicer
/// `ExtrusionRole` display strings. An unknown string yields `None` (skipped),
/// never a hard error — this keeps the parser forward-compatible with future
/// slicer TYPE strings. The groups mirror PrusaSlicer's `ExtrusionRole`
/// enum (`src/libslic3r/ExtrusionEntity.hpp` / GCodeViewer legend):
///
/// - `Perimeter` / `External perimeter` / `Overhang perimeter` → [`BeadRole::Perimeter`]
/// - `Internal infill` → [`BeadRole::SparseInfill`]
/// - `Solid infill` / `Top solid infill` / `Bottom solid infill` / `Gap fill`
///   / `Ironing` → [`BeadRole::SolidInfill`] (all dense solid material)
/// - `Bridge infill` / `Internal bridge infill` → [`BeadRole::Bridge`]
/// - `Support material` / `Support material interface` → [`BeadRole::Support`]
/// - everything else (`Skirt/Brim`, `Wipe tower`, `Custom`, unknown) → `None`
pub fn role_from_prusaslicer_type(type_str: &str) -> Option<BeadRole> {
    match type_str {
        "Perimeter" | "External perimeter" | "Overhang perimeter" => Some(BeadRole::Perimeter),
        "Internal infill" => Some(BeadRole::SparseInfill),
        "Solid infill" | "Top solid infill" | "Bottom solid infill" | "Gap fill" | "Ironing" => {
            Some(BeadRole::SolidInfill)
        }
        "Bridge infill" | "Internal bridge infill" => Some(BeadRole::Bridge),
        "Support material" | "Support material interface" => Some(BeadRole::Support),
        _ => None,
    }
}

// ── Value types ──────────────────────────────────────────────────────────────

/// A single deposited bead: a maximal run of consecutive extruding moves with
/// constant `(role, width, height, layer)`.
///
/// **Units are native G-code millimetres** (coordinates, `width`, `height`,
/// `layer_z`) and **mm·min⁻¹** (`speed`), stored exactly as parsed — no SI
/// conversion happens here. The downstream θ `FDMPrint` mapping owns the
/// mm→SI conversion when it builds the constitutive field (Plan §"Design
/// Decisions": lossless, faithful-to-source representation).
#[derive(Debug, Clone, PartialEq)]
pub struct Bead {
    /// Ordered deposited centerline polyline in mm; the first point is the
    /// pen-down position, each subsequent point an extruding-move endpoint.
    pub centerline: Vec<[f64; 3]>,
    /// Extrusion width in mm (PrusaSlicer `;WIDTH:`), constant over the bead.
    pub width: f64,
    /// Layer height in mm (PrusaSlicer `;HEIGHT:`), constant over the bead.
    pub height: f64,
    /// Structural role (PrusaSlicer `;TYPE:` → [`role_from_prusaslicer_type`]).
    pub role: BeadRole,
    /// Index of the owning [`Layer`] (0-based, deposition order).
    pub layer_index: usize,
    /// Z height of the owning layer in mm.
    pub layer_z: f64,
    /// Nominal extruder temperature in °C active when the bead was laid down
    /// (last `M104`/`M109` `S` value).
    pub nominal_temp: f64,
    /// Active feedrate in mm·min⁻¹ when the bead began extruding.
    pub speed: f64,
}

/// A print layer: an ordered group of bead indices deposited at a common Z.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// 0-based layer index in deposition order.
    pub index: usize,
    /// Layer Z height in mm (from `;Z:`, or a `G1 Z` fallback).
    pub z: f64,
    /// Indices into [`Toolpath::beads`] of the beads on this layer, in order.
    pub bead_indices: Vec<usize>,
}

/// A parsed PrusaSlicer toolpath: the ordered, layer-segmented bead graph plus
/// in-layer and inter-layer bead adjacency.
///
/// Adjacency pairs are `(lo, hi)` bead indices (sorted, de-duplicated). The
/// in-layer list connects beads on the same layer; the inter-layer list
/// connects beads on consecutive layers (`|Δlayer_index| == 1`).
#[derive(Debug, Clone, PartialEq)]
pub struct Toolpath {
    /// All deposited beads, in deposition order.
    pub beads: Vec<Bead>,
    /// All layers, in deposition order (`layers[i].index == i`).
    pub layers: Vec<Layer>,
    /// Same-layer adjacent bead-index pairs `(lo, hi)`.
    pub in_layer_adjacency: Vec<(usize, usize)>,
    /// Consecutive-layer adjacent bead-index pairs `(lo, hi)`.
    pub inter_layer_adjacency: Vec<(usize, usize)>,
}

/// Failure parsing a PrusaSlicer G-code source into a [`Toolpath`].
#[derive(Debug, Clone, PartialEq)]
pub enum ToolpathParseError {
    /// A delegated G0/G1/G2/G3/G92 move line failed to parse in the
    /// underlying `reify-gcode` low-level parser. The inner `ParseError.line`
    /// is re-stamped to the 1-indexed **toolpath source** line (not the
    /// per-line slice's line 1) so diagnostics point at the real position.
    Gcode(reify_gcode::ParseError),
    /// A structured directive comment (`;WIDTH:`, `;HEIGHT:`, `;Z:`) carried a
    /// value that did not parse as a number. `line` is 1-indexed; `raw` is the
    /// offending source line.
    Comment { line: usize, raw: String },
}

impl std::fmt::Display for ToolpathParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolpathParseError::Gcode(e) => write!(f, "g-code move parse error: {e:?}"),
            ToolpathParseError::Comment { line, raw } => {
                write!(f, "malformed directive comment at line {line}: {raw:?}")
            }
        }
    }
}

impl std::error::Error for ToolpathParseError {}

// ── Parser ───────────────────────────────────────────────────────────────────

/// Motion epsilon (mm) below which an axis delta counts as "no motion".
const POS_EPS: f64 = 1e-9;
/// Extrusion epsilon (mm) below which an E delta counts as "no extrusion".
const E_EPS: f64 = 1e-9;

/// Parse a PrusaSlicer-flavoured G-code source into a structured [`Toolpath`].
///
/// The parser walks **physical lines itself** (1-indexed): comment lines feed
/// the `;TYPE:` / `;WIDTH:` / `;HEIGHT:` state machine, and only G0/G1/G2/G3/G92
/// move lines are delegated to `reify_gcode::parse_marlin` per-line (which is
/// why reify-gcode's comment-stripping does not lose the structured markers —
/// see the module doc). Extruder mode defaults to relative-E (PrusaSlicer's
/// `M83`); `M82`/`M83`/`G92` and `G90`/`G91` are honoured if present. Unknown
/// G/M codes and free-text comments are skipped without error.
///
/// A bead is a maximal run of extruding G1 moves with constant `(role, width,
/// height, layer)`, broken by any travel (`G0`, or a `G1` with no positive
/// extrusion) or retract. Extrusions whose `;TYPE:` maps to `None`
/// (sacrificial / unknown) are skipped entirely.
///
/// # Move-line dispatch assumes space-separated tokens
///
/// A non-comment line is delegated to the low-level parser only when its
/// **first whitespace-delimited token** is exactly `G0` / `G1` / `G2` / `G3` /
/// `G92`. This matches PrusaSlicer output, which is always space-separated with
/// an explicit leading G-code. A space-less token (`G1X20Y10`, valid in some
/// dialects) or a bare feedrate line (`F2000`) does NOT match and is silently
/// skipped — no bead, no error. That is intentional for the PrusaSlicer-flavoured
/// input this function targets; it is not a general-purpose G-code front end.
pub fn parse_prusaslicer_gcode(src: &str) -> Result<Toolpath, ToolpathParseError> {
    let mut sweep = Sweep::new();

    for (idx, raw) in src.split('\n').enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(';') {
            // Comment line: structured directive or free text. A change of
            // role / width / height ends the current bead (constant within a
            // bead) — flush eagerly so the next deposition seeds a fresh bead.
            if rest == "LAYER_CHANGE" {
                sweep.layer_change();
            } else if let Some(z) = rest.strip_prefix("Z:") {
                // `;Z:` authoritatively sets the current layer's z.
                sweep.layer_z = Some(parse_comment_f64(line_no, line, z)?);
            } else if let Some(t) = rest.strip_prefix("TYPE:") {
                let new_role = role_from_prusaslicer_type(t.trim());
                if new_role != sweep.role {
                    sweep.flush();
                    sweep.role = new_role;
                }
            } else if let Some(w) = rest.strip_prefix("WIDTH:") {
                let new_w = parse_comment_f64(line_no, line, w)?;
                if (new_w - sweep.width).abs() > POS_EPS {
                    sweep.flush();
                    sweep.width = new_w;
                }
            } else if let Some(h) = rest.strip_prefix("HEIGHT:") {
                let new_h = parse_comment_f64(line_no, line, h)?;
                if (new_h - sweep.height).abs() > POS_EPS {
                    sweep.flush();
                    sweep.height = new_h;
                }
            }
            // `;LAYER_CHANGE` / `;Z:` are handled in step-8; other comments
            // (e.g. `; generated by PrusaSlicer`) are free text — skip.
            continue;
        }
        // Non-comment line: dispatch on the leading token.
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "G0" | "G1" | "G2" | "G3" | "G92" => {
                // `parse_marlin` sees only this one-line slice, so the
                // `ParseError.line` it would report is always 1 (the slice's own
                // first line). Re-stamp it with the toolpath source `line_no` so
                // the diagnostic points at the real position rather than line 1.
                let cmds = reify_gcode::parse_marlin(line).map_err(|e| {
                    ToolpathParseError::Gcode(reify_gcode::ParseError {
                        line: line_no,
                        kind: e.kind,
                    })
                })?;
                for cmd in cmds {
                    sweep.apply_command(cmd);
                }
            }
            "G90" => sweep.xyz_absolute = true,
            "G91" => sweep.xyz_absolute = false,
            "M82" => sweep.e_relative = false,
            "M83" => sweep.e_relative = true,
            "M104" | "M109" => {
                if let Some(s) = parse_s_param(line) {
                    sweep.temp = s;
                }
            }
            // Unknown G/M codes (G21, G28, M73, M201, …) are skipped.
            _ => {}
        }
    }
    sweep.flush();

    let beads = sweep.beads;
    let layers = assemble_layers(&beads);
    let (in_layer_adjacency, inter_layer_adjacency) = compute_adjacency(&beads);
    Ok(Toolpath {
        beads,
        layers,
        in_layer_adjacency,
        inter_layer_adjacency,
    })
}

/// In-progress bead accumulator; its `(role, width, height, layer_index,
/// layer_z, nominal_temp, speed)` are captured at the pen-down point and held
/// constant for the bead's lifetime.
struct BeadBuilder {
    centerline: Vec<[f64; 3]>,
    width: f64,
    height: f64,
    role: BeadRole,
    layer_index: usize,
    layer_z: f64,
    nominal_temp: f64,
    speed: f64,
}

impl BeadBuilder {
    fn finish(self) -> Bead {
        Bead {
            centerline: self.centerline,
            width: self.width,
            height: self.height,
            role: self.role,
            layer_index: self.layer_index,
            layer_z: self.layer_z,
            nominal_temp: self.nominal_temp,
            speed: self.speed,
        }
    }
}

/// Mutable position-sweep + comment state threaded across the line walk.
struct Sweep {
    beads: Vec<Bead>,
    /// Current logical XYZ position in mm.
    pos: [f64; 3],
    /// Current absolute extruder coordinate in mm.
    e_pos: f64,
    /// Extruder mode: relative-E (PrusaSlicer `M83` default) vs absolute (`M82`).
    e_relative: bool,
    /// XYZ mode: absolute (`G90` default) vs relative (`G91`).
    xyz_absolute: bool,
    /// Active feedrate in mm·min⁻¹.
    feedrate: f64,
    /// Nominal extruder temperature in °C (last `M104`/`M109` `S`).
    temp: f64,
    /// Active structural role (`None` ⇒ extrusions skipped).
    role: Option<BeadRole>,
    /// Active extrusion width in mm (`;WIDTH:`).
    width: f64,
    /// Active layer height in mm (`;HEIGHT:`).
    height: f64,
    /// Current layer index (0-based).
    layer_index: usize,
    /// Resolved Z of the current layer in mm (`;Z:` or `G1 Z` fallback).
    layer_z: Option<f64>,
    /// `beads.len()` at the start of the current layer — used to detect whether
    /// the current layer has produced any beads, so the first `;LAYER_CHANGE`
    /// opens (implicit) layer 0 while subsequent ones increment.
    beads_at_layer_start: usize,
    /// Active bead accumulator, if extruding.
    cur: Option<BeadBuilder>,
}

impl Sweep {
    fn new() -> Self {
        Sweep {
            beads: Vec::new(),
            pos: [0.0; 3],
            e_pos: 0.0,
            e_relative: true,
            xyz_absolute: true,
            feedrate: 0.0,
            temp: 0.0,
            role: None,
            width: 0.0,
            height: 0.0,
            layer_index: 0,
            layer_z: None,
            beads_at_layer_start: 0,
            cur: None,
        }
    }

    /// Handle a `;LAYER_CHANGE` directive: flush the active bead, then advance
    /// to a new layer (resetting the layer Z) — but only increment the index if
    /// the current layer already produced beads, so a leading `;LAYER_CHANGE`
    /// (or one after a bead-less skirt layer) opens implicit layer 0 rather
    /// than skipping to 1.
    fn layer_change(&mut self) {
        self.flush();
        if self.beads.len() > self.beads_at_layer_start {
            self.layer_index += 1;
            self.beads_at_layer_start = self.beads.len();
        }
        self.layer_z = None;
    }

    /// Finalise the active bead (if any) into the bead list. A bead always
    /// carries ≥2 points (pen-down + ≥1 endpoint); the length guard is a
    /// defensive backstop.
    fn flush(&mut self) {
        if let Some(b) = self.cur.take()
            && b.centerline.len() >= 2
        {
            self.beads.push(b.finish());
        }
    }

    fn apply_command(&mut self, cmd: reify_gcode::GcodeCommand) {
        use reify_gcode::GcodeCommand;
        match cmd {
            GcodeCommand::LinearMove(mv) => self.apply_linear(mv),
            GcodeCommand::SetPosition(sp) => self.apply_set_position(sp),
            GcodeCommand::ArcMove(arc) => self.apply_arc(arc),
            // Only G0/G1/G2/G3/G92 lines are delegated, so no other variant
            // can appear here; ignore defensively.
            _ => {}
        }
    }

    fn apply_linear(&mut self, mv: reify_gcode::ast::LinearMove) {
        let nx = axis(self.xyz_absolute, self.pos[0], mv.x);
        let ny = axis(self.xyz_absolute, self.pos[1], mv.y);
        let nz = axis(self.xyz_absolute, self.pos[2], mv.z);
        if let Some(f) = mv.feedrate {
            self.feedrate = f;
        }

        let e_delta = match mv.e {
            Some(e) => {
                if self.e_relative {
                    e
                } else {
                    e - self.e_pos
                }
            }
            None => 0.0,
        };

        let dx = nx - self.pos[0];
        let dy = ny - self.pos[1];
        let dz = nz - self.pos[2];
        let xy_moved = dx.hypot(dy) > POS_EPS;
        let z_moved = dz.abs() > POS_EPS;
        let extruding = e_delta > E_EPS;
        let retracting = e_delta < -E_EPS;

        // Layer-Z fallback: the first Z move of a layer establishes its z when
        // no `;Z:` directive has set it (step-8 adds the `;Z:` preference).
        if z_moved && self.layer_z.is_none() {
            self.layer_z = Some(nz);
        }

        let deposition = extruding && xy_moved && self.role.is_some();
        if mv.rapid {
            // G0 is always a travel.
            self.flush();
        } else if deposition {
            let endpoint = [nx, ny, nz];
            if self.cur.is_none() {
                // Seed a new bead with the pen-down (pre-move) position; capture
                // the constant per-bead metadata here.
                self.cur = Some(BeadBuilder {
                    centerline: vec![self.pos],
                    width: self.width,
                    height: self.height,
                    role: self.role.expect("deposition implies role is Some"),
                    layer_index: self.layer_index,
                    layer_z: self.layer_z.unwrap_or(self.pos[2]),
                    nominal_temp: self.temp,
                    speed: self.feedrate,
                });
            }
            if let Some(b) = self.cur.as_mut() {
                b.centerline.push(endpoint);
            }
        } else if retracting || xy_moved || z_moved {
            // Travel, retract, or Z-hop breaks extrusion continuity.
            self.flush();
        }
        // else: pure feedrate update / no-op — leave the active bead intact.

        self.pos = [nx, ny, nz];
        if let Some(e) = mv.e {
            self.e_pos = if self.e_relative {
                self.e_pos + e
            } else {
                e
            };
        }
    }

    fn apply_set_position(&mut self, sp: reify_gcode::ast::SetPosition) {
        // G92 rebases the logical frame (commonly `G92 E0`) and breaks bead
        // continuity. It does NOT move the nozzle physically.
        self.flush();
        if let Some(x) = sp.x {
            self.pos[0] = x;
        }
        if let Some(y) = sp.y {
            self.pos[1] = y;
        }
        if let Some(z) = sp.z {
            self.pos[2] = z;
        }
        if let Some(e) = sp.e {
            self.e_pos = e;
        }
    }

    fn apply_arc(&mut self, arc: reify_gcode::ast::ArcMove) {
        // Arc *geometry* is out of scope for ζ (PrusaSlicer's default arc-fitting
        // is off — it emits G1). Flush the active bead and advance the logical
        // position best-effort so a following move is not mis-seeded.
        self.flush();
        self.pos[0] = axis(self.xyz_absolute, self.pos[0], arc.x);
        self.pos[1] = axis(self.xyz_absolute, self.pos[1], arc.y);
        self.pos[2] = axis(self.xyz_absolute, self.pos[2], arc.z);
        if let Some(e) = arc.e {
            self.e_pos = if self.e_relative {
                self.e_pos + e
            } else {
                e
            };
        }
    }
}

/// Resolve one axis target: absolute uses the value directly; relative adds it
/// to the current coordinate; an omitted axis keeps the current value.
fn axis(absolute: bool, cur: f64, v: Option<f64>) -> f64 {
    match v {
        Some(val) => {
            if absolute {
                val
            } else {
                cur + val
            }
        }
        None => cur,
    }
}

/// Parse the `S<number>` parameter of an `M104`/`M109` temperature line.
fn parse_s_param(line: &str) -> Option<f64> {
    for tok in line.split_whitespace() {
        if let Some(body) = tok.strip_prefix('S').or_else(|| tok.strip_prefix('s')) {
            return body.parse::<f64>().ok();
        }
    }
    None
}

/// Parse the numeric body of a `;WIDTH:` / `;HEIGHT:` directive, mapping a
/// malformed value to a [`ToolpathParseError::Comment`].
fn parse_comment_f64(line_no: usize, raw: &str, value: &str) -> Result<f64, ToolpathParseError> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| ToolpathParseError::Comment {
            line: line_no,
            raw: raw.to_string(),
        })
}

/// Build the `layers` list from the (deposition-ordered) bead list: beads are
/// emitted in non-decreasing `layer_index`, so a new [`Layer`] opens each time
/// the index advances, taking its `z` from its first bead.
fn assemble_layers(beads: &[Bead]) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for (bi, bead) in beads.iter().enumerate() {
        if layers.last().is_none_or(|l| l.index != bead.layer_index) {
            layers.push(Layer {
                index: bead.layer_index,
                z: bead.layer_z,
                bead_indices: Vec::new(),
            });
        }
        layers
            .last_mut()
            .expect("just pushed or already present")
            .bead_indices
            .push(bi);
    }
    layers
}

// ── Adjacency ────────────────────────────────────────────────────────────────

/// A list of `(lo, hi)` adjacent bead-index pairs.
type AdjacencyPairs = Vec<(usize, usize)>;

/// In-layer (lateral) distance threshold below which two beads count as
/// adjacent: the sum of their half-widths (the centerline-to-centerline
/// distance at which the beads just touch) plus a slack of half the mean width,
/// admitting the sub-width gap that typical extrusion overlap leaves between
/// neighbouring beads.
fn adjacency_threshold(w_a: f64, w_b: f64) -> f64 {
    let half_sum = 0.5 * (w_a + w_b); // touching distance
    let slack = 0.25 * (w_a + w_b); // half the mean width = 0.25·(w_a + w_b)
    half_sum + slack
}

/// Inter-layer (vertical) distance threshold. Beads on consecutive layers abut
/// across the layer interface, where the governing dimension is layer **height**
/// (not extrusion width): two vertically stacked beads just touch when their
/// centerlines are `0.5·(h_a + h_b)` apart. Mirroring [`adjacency_threshold`],
/// a `0.25·(h_a + h_b)` slack admits the overlap a slicer leaves between layers.
///
/// We take the **larger** of the height- and width-derived thresholds so that
/// neither failure direction drops a genuine bond: a tall-layer vertical bond
/// (`height > 0.75·width` — the reviewer-flagged false negative, lost by a
/// width-only threshold) nor a laterally-overlapping offset bond between
/// consecutive layers (lost by a height-only threshold). Erring toward
/// connectivity is the safe direction for the downstream θ constitutive graph,
/// where a missing bond is worse than a spurious weak one.
fn inter_layer_threshold(a: &Bead, b: &Bead) -> f64 {
    let half_sum = 0.5 * (a.height + b.height); // vertical touching distance
    let slack = 0.25 * (a.height + b.height);
    (half_sum + slack).max(adjacency_threshold(a.width, b.width))
}

/// Compute `(in_layer, inter_layer)` adjacency over all bead pairs.
///
/// For each pair, the minimum 3-D polyline distance is compared against a
/// touching-distance threshold: same-`layer_index` pairs use the width-derived
/// [`adjacency_threshold`] (lateral abutment) and feed the in-layer list;
/// `|Δlayer_index| == 1` pairs use the height-aware [`inter_layer_threshold`]
/// (vertical abutment) and feed the inter-layer list. All other layer
/// separations are skipped. Both lists are `(lo, hi)`-ordered, sorted, and
/// de-duplicated.
///
/// Delegates to [`compute_adjacency_with_stats`], discarding the stats.
fn compute_adjacency(beads: &[Bead]) -> (AdjacencyPairs, AdjacencyPairs) {
    let (in_layer, inter_layer, _stats) = compute_adjacency_with_stats(beads);
    (in_layer, inter_layer)
}

/// Statistics from the instrumented [`compute_adjacency_with_stats`] seam.
///
/// Used by the in-crate complexity test
/// (`distance_probes_scale_subquadratically`) to assert sub-quadratic scaling
/// without relying on wall-clock timing (flaky under the verify-pipeline's
/// PSI-gate / test-semaphore governance — see CLAUDE.md).
struct AdjacencyStats {
    /// Number of unique candidate pairs examined via [`min_polyline_distance`].
    distance_probes: usize,
    /// Number of unique candidate pairs enumerated before distance filtering.
    candidate_pairs: usize,
}

/// Instrumented adjacency computation: same `(in_layer, inter_layer)` output
/// as [`compute_adjacency`] plus an [`AdjacencyStats`] counter record.
///
/// The body here is the **verbatim O(B²) passthrough** (the original
/// double-loop algorithm with `candidate_pairs` / `distance_probes` counters
/// added).  The body is replaced with the layer-bucketed 2-D spatial hash in
/// step-4 (task #4858).  `compute_adjacency` is the only caller-facing entry.
fn compute_adjacency_with_stats(
    beads: &[Bead],
) -> (AdjacencyPairs, AdjacencyPairs, AdjacencyStats) {
    let mut in_layer: AdjacencyPairs = Vec::new();
    let mut inter_layer: AdjacencyPairs = Vec::new();
    let mut stats = AdjacencyStats {
        distance_probes: 0,
        candidate_pairs: 0,
    };
    for i in 0..beads.len() {
        for j in (i + 1)..beads.len() {
            let a = &beads[i];
            let b = &beads[j];
            let same_layer = a.layer_index == b.layer_index;
            let consecutive = a.layer_index.abs_diff(b.layer_index) == 1;
            if !same_layer && !consecutive {
                continue; // non-adjacent layers: skip the distance probe
            }
            stats.candidate_pairs += 1;
            let d = min_polyline_distance(&a.centerline, &b.centerline);
            stats.distance_probes += 1;
            // Same-layer beads abut laterally (width governs); consecutive-layer
            // beads abut vertically (layer height governs). Distinct thresholds —
            // see the two helpers (reviewer suggestion 3).
            let threshold = if same_layer {
                adjacency_threshold(a.width, b.width)
            } else {
                inter_layer_threshold(a, b)
            };
            if d <= threshold {
                // i < j by construction, so the pair is already (lo, hi).
                if same_layer {
                    in_layer.push((i, j));
                } else {
                    inter_layer.push((i, j));
                }
            }
        }
    }
    in_layer.sort_unstable();
    in_layer.dedup();
    inter_layer.sort_unstable();
    inter_layer.dedup();
    (in_layer, inter_layer, stats)
}

// ── Geometry helpers ─────────────────────────────────────────────────────────

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `base + dir * s` — point along a segment direction.
fn along(base: [f64; 3], dir: [f64; 3], s: f64) -> [f64; 3] {
    [base[0] + dir[0] * s, base[1] + dir[1] * s, base[2] + dir[2] * s]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Minimum Euclidean distance between two 3-D line segments `[p1,q1]` and
/// `[p2,q2]`.
///
/// Clamped closest-point-between-two-segments (Ericson, *Real-Time Collision
/// Detection* §5.1.9): solves for the closest parameters `(s, t)` on each
/// segment, clamping to `[0, 1]` and handling the parallel / degenerate
/// (zero-length segment) cases. Pure `[f64; 3]` arithmetic, no dependencies.
pub(crate) fn segment_segment_distance(
    p1: [f64; 3],
    q1: [f64; 3],
    p2: [f64; 3],
    q2: [f64; 3],
) -> f64 {
    let d1 = sub(q1, p1); // direction + length of segment 1
    let d2 = sub(q2, p2); // direction + length of segment 2
    let r = sub(p1, p2);
    let a = dot(d1, d1); // squared length of segment 1, ≥ 0
    let e = dot(d2, d2); // squared length of segment 2, ≥ 0
    let f = dot(d2, r);

    // Tiny tolerance for treating a segment as a degenerate point.
    const SEG_EPS: f64 = 1e-18;

    let (s, t);
    if a <= SEG_EPS && e <= SEG_EPS {
        // Both segments are points.
        s = 0.0;
        t = 0.0;
    } else if a <= SEG_EPS {
        // Segment 1 is a point.
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= SEG_EPS {
            // Segment 2 is a point.
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            // General non-degenerate case.
            let b = dot(d1, d2);
            let denom = a * e - b * b; // ≥ 0; 0 ⇒ parallel
            let s0 = if denom > SEG_EPS {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0 // parallel: pick s = 0, resolve t below
            };
            let t0 = (b * s0 + f) / e;
            // Clamp t to [0,1] and recompute s for the clamped t.
            if t0 < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            } else {
                t = t0;
                s = s0;
            }
        }
    }

    let c1 = along(p1, d1, s);
    let c2 = along(p2, d2, t);
    norm(sub(c1, c2))
}

/// Minimum distance between two polylines: the smallest
/// [`segment_segment_distance`] over all segment pairs.
///
/// `O(|a|·|b|)` over the constituent segments. Polylines are expected to have
/// ≥ 2 points (bead centerlines always do); a shorter polyline contributes no
/// segments.
pub(crate) fn min_polyline_distance(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
    let mut min = f64::INFINITY;
    for wa in a.windows(2) {
        for wb in b.windows(2) {
            let d = segment_segment_distance(wa[0], wa[1], wb[0], wb[1]);
            if d < min {
                min = d;
            }
        }
    }
    min
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tight tolerance — parsed coordinates are `f64::from_str` of the source
    /// literal with no arithmetic accumulation (mirrors zone.rs EPS).
    const EPS: f64 = 1e-12;

    /// Assert two ordered point lists are element-wise approximately equal.
    fn assert_pts_approx(actual: &[[f64; 3]], expected: &[[f64; 3]]) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "centerline length: got {actual:?}, want {expected:?}"
        );
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            for k in 0..3 {
                assert!(
                    (a[k] - e[k]).abs() < EPS,
                    "point[{i}][{k}]: got {} want {} (full got {actual:?}, want {expected:?})",
                    a[k],
                    e[k]
                );
            }
        }
    }

    // ── step-1: role mapping ─────────────────────────────────────────────────

    #[test]
    fn perimeter_types_map_to_perimeter() {
        // PrusaSlicer ExtrusionRole strings that are all perimeter shell.
        for s in ["External perimeter", "Perimeter", "Overhang perimeter"] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::Perimeter),
                "{s:?} should map to Perimeter"
            );
        }
    }

    #[test]
    fn internal_infill_maps_to_sparse_infill() {
        assert_eq!(
            role_from_prusaslicer_type("Internal infill"),
            Some(BeadRole::SparseInfill),
            "Internal infill is the sparse interior lattice"
        );
    }

    #[test]
    fn solid_and_dense_types_map_to_solid_infill() {
        // Solid/top/bottom skin + gap fill + ironing are all dense solid material.
        for s in [
            "Solid infill",
            "Top solid infill",
            "Bottom solid infill",
            "Gap fill",
            "Ironing",
        ] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::SolidInfill),
                "{s:?} should map to SolidInfill"
            );
        }
    }

    #[test]
    fn bridge_types_map_to_bridge() {
        for s in ["Bridge infill", "Internal bridge infill"] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::Bridge),
                "{s:?} should map to Bridge"
            );
        }
    }

    #[test]
    fn support_types_map_to_support() {
        for s in ["Support material", "Support material interface"] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                Some(BeadRole::Support),
                "{s:?} should map to Support"
            );
        }
    }

    #[test]
    fn sacrificial_and_unknown_types_map_to_none() {
        // Sacrificial / non-part / unrecognised TYPEs are skipped (None), never
        // a hard error — keeps the parser forward-compatible with new strings.
        for s in [
            "Skirt/Brim",
            "Wipe tower",
            "Custom",
            "Travel",
            "",
            "perimeter", // case-sensitive: lowercase is NOT a known TYPE
            "External Perimeter", // wrong casing of the second word
            "Some future role",
        ] {
            assert_eq!(
                role_from_prusaslicer_type(s),
                None,
                "{s:?} should map to None (skipped)"
            );
        }
    }

    // ── step-3: parse-core, single bead ──────────────────────────────────────

    /// A minimal single-layer, single-perimeter snippet in PrusaSlicer's
    /// comment vocabulary, relative-E (M83). Interleaves lines the parser MUST
    /// skip without error: a free-text comment, `G21` (units), `M73` (progress).
    const SINGLE_PERIMETER: &str = "\
; generated by PrusaSlicer 2.7.0+linux
M73 P0 R0
G21
M83
M104 S210
G1 Z0.2 F7200
G1 X10 Y10 F9000
;TYPE:External perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 F1800
G1 X20 Y10 E1.2
G1 X20 Y20 E1.2
";

    #[test]
    fn parse_single_perimeter_bead() {
        let tp = parse_prusaslicer_gcode(SINGLE_PERIMETER).expect("snippet must parse");

        assert_eq!(tp.layers.len(), 1, "exactly one layer");
        assert_eq!(tp.beads.len(), 1, "exactly one bead");

        let bead = &tp.beads[0];
        assert_eq!(bead.role, BeadRole::Perimeter, "role from ;TYPE:");
        assert!((bead.width - 0.45).abs() < EPS, "width from ;WIDTH:");
        assert!((bead.height - 0.2).abs() < EPS, "height from ;HEIGHT:");
        assert!((bead.layer_z - 0.2).abs() < EPS, "layer_z from G1 Z move");
        assert!((bead.nominal_temp - 210.0).abs() < EPS, "temp from M104 S");
        assert!(
            (bead.speed - 1800.0).abs() < EPS,
            "speed = active feedrate (G1 F1800), got {}",
            bead.speed
        );
        assert_eq!(bead.layer_index, 0, "layer 0");

        // Centerline seeds with the pen-down position (post-travel [10,10,0.2]),
        // then the two extruding-move endpoints, in order.
        assert_pts_approx(
            &bead.centerline,
            &[[10.0, 10.0, 0.2], [20.0, 10.0, 0.2], [20.0, 20.0, 0.2]],
        );

        // The single layer carries index 0 and z = 0.2; the bead is recorded on it.
        let layer = &tp.layers[0];
        assert_eq!(layer.index, 0);
        assert!((layer.z - 0.2).abs() < EPS, "layer z = 0.2");
        assert_eq!(layer.bead_indices, vec![0], "bead 0 belongs to layer 0");
    }

    // ── step-5: bead segmentation ────────────────────────────────────────────

    fn roles(tp: &Toolpath) -> Vec<BeadRole> {
        tp.beads.iter().map(|b| b.role).collect()
    }

    /// (a) Two perimeter runs separated by a travel → 2 beads, both Perimeter.
    #[test]
    fn travel_breaks_into_two_beads() {
        let src = "\
M83
G1 Z0.2 F7200
G1 X10 Y10 F9000
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X20 Y10 E1.0
G1 X30 Y10 F9000
G1 X40 Y10 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 2, "travel splits the run");
        assert_eq!(roles(&tp), vec![BeadRole::Perimeter, BeadRole::Perimeter]);
    }

    /// (b) A `;TYPE:` change mid-layer (no travel) → 2 beads with distinct roles.
    #[test]
    fn role_change_breaks_bead() {
        let src = "\
M83
G1 Z0.2 F7200
G1 X10 Y10 F9000
;TYPE:External perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X20 Y10 E1.0
;TYPE:Internal infill
G1 X20 Y20 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 2, "role change splits the run");
        assert_eq!(
            roles(&tp),
            vec![BeadRole::Perimeter, BeadRole::SparseInfill]
        );
        // The second bead seeds from the pen-down point [20,10] (no travel).
        assert_pts_approx(&tp.beads[0].centerline, &[[10.0, 10.0, 0.2], [20.0, 10.0, 0.2]]);
        assert_pts_approx(&tp.beads[1].centerline, &[[20.0, 10.0, 0.2], [20.0, 20.0, 0.2]]);
    }

    /// (c) A `;WIDTH:` change mid-run (no travel) → bead break; width constant
    /// within each bead.
    #[test]
    fn width_change_breaks_bead() {
        let src = "\
M83
G1 Z0.2 F7200
G1 X10 Y10 F9000
;TYPE:External perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X20 Y10 E1.0
;WIDTH:0.60
G1 X20 Y20 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 2, "width change splits the run");
        assert!((tp.beads[0].width - 0.45).abs() < EPS);
        assert!((tp.beads[1].width - 0.60).abs() < EPS);
    }

    /// (d) A relative-E retract then resume → bead break.
    #[test]
    fn retract_breaks_bead() {
        let src = "\
M83
G1 Z0.2 F7200
G1 X10 Y10 F9000
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X20 Y10 E1.0
G1 E-0.8
G1 X30 Y10 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 2, "retract splits the run");
    }

    /// (e) Pure-travel moves (G0, E-absent G1) never create a bead and are
    /// excluded from centerlines.
    #[test]
    fn travel_moves_never_extend_centerline() {
        let src = "\
M83
G1 Z0.2 F7200
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X10 Y10 F9000
G1 X20 Y10 E1.0
G0 X30 Y30
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 1, "only the extruding move makes a bead");
        // The G0 X30 Y30 travel point is NOT part of the centerline.
        assert_pts_approx(&tp.beads[0].centerline, &[[10.0, 10.0, 0.2], [20.0, 10.0, 0.2]]);
    }

    /// (f) Absolute-E mode (M82 + G92 E0 baseline, ascending E) classifies
    /// extrusion by ΔE>0 identically to relative-E.
    #[test]
    fn absolute_e_mode_classifies_by_delta() {
        let src = "\
M82
G1 Z0.2 F7200
G1 X10 Y10 F9000
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G92 E0
G1 X20 Y10 E1.0
G1 X30 Y10 E2.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 1, "ascending absolute E is one continuous bead");
        assert_pts_approx(
            &tp.beads[0].centerline,
            &[[10.0, 10.0, 0.2], [20.0, 10.0, 0.2], [30.0, 10.0, 0.2]],
        );
    }

    // ── step-7: multi-layer segmentation ─────────────────────────────────────

    /// Two `;LAYER_CHANGE`-delimited layers at Z 0.20 / 0.40. Same role+width
    /// across layers must NOT merge into one bead, and each layer carries its
    /// own index + `;Z:` height.
    #[test]
    fn multi_layer_segmentation() {
        let src = "\
M83
;LAYER_CHANGE
;Z:0.20
;HEIGHT:0.2
G1 Z0.20 F7200
;TYPE:External perimeter
;WIDTH:0.45
G1 X10 Y10 F9000
G1 X20 Y10 E1.0
G1 X20 Y20 E1.0
;LAYER_CHANGE
;Z:0.40
;HEIGHT:0.2
G1 Z0.40 F7200
;TYPE:External perimeter
;WIDTH:0.45
G1 X10 Y10 F9000
G1 X20 Y10 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();

        assert_eq!(tp.layers.len(), 2, "two layers in deposition order");
        assert_eq!(tp.layers[0].index, 0);
        assert_eq!(tp.layers[1].index, 1);
        assert!((tp.layers[0].z - 0.20).abs() < EPS, "layer 0 z from ;Z:0.20");
        assert!((tp.layers[1].z - 0.40).abs() < EPS, "layer 1 z from ;Z:0.40");

        // Same role/width across layers does NOT merge: still 2 beads.
        assert_eq!(tp.beads.len(), 2);
        assert_eq!(tp.beads[0].layer_index, 0);
        assert!((tp.beads[0].layer_z - 0.20).abs() < EPS);
        assert_eq!(tp.beads[1].layer_index, 1);
        assert!((tp.beads[1].layer_z - 0.40).abs() < EPS);

        // Layers reference their own beads.
        assert_eq!(tp.layers[0].bead_indices, vec![0]);
        assert_eq!(tp.layers[1].bead_indices, vec![1]);
    }

    /// Layer-Z fallback: when a layer has no `;Z:` directive, the layer's first
    /// `G1 Z` move establishes its z.
    #[test]
    fn layer_z_fallback_from_g1_z() {
        let src = "\
M83
;LAYER_CHANGE
;Z:0.20
;HEIGHT:0.2
G1 Z0.20 F7200
;TYPE:Perimeter
;WIDTH:0.45
G1 X10 Y10 F9000
G1 X20 Y10 E1.0
;LAYER_CHANGE
;HEIGHT:0.2
G1 Z0.40 F7200
;TYPE:Perimeter
G1 X10 Y10 F9000
G1 X20 Y10 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.layers.len(), 2);
        assert!((tp.layers[1].z - 0.40).abs() < EPS, "layer 1 z from G1 Z0.40 fallback");
        assert_eq!(tp.beads[1].layer_index, 1);
    }

    // ── step-9: geometry helpers ─────────────────────────────────────────────

    #[test]
    fn segment_distance_parallel_offset() {
        // Two parallel X-segments offset by 2 in Y → gap 2.0.
        let d = segment_segment_distance(
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [10.0, 2.0, 0.0],
        );
        assert!((d - 2.0).abs() < EPS, "parallel gap, got {d}");
    }

    #[test]
    fn segment_distance_skew_3d() {
        // a along X at origin; b along Y at x=0,z=1. Closest approach is
        // a.start↔b.start = 1.0 (common perpendicular within both segments).
        let d = segment_segment_distance(
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
        );
        assert!((d - 1.0).abs() < EPS, "skew distance, got {d}");
    }

    #[test]
    fn segment_distance_crossing_is_zero() {
        // a along X, b along Y crossing it at [1,0,0] → 0.
        let d = segment_segment_distance(
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
        );
        assert!(d.abs() < EPS, "crossing segments, got {d}");
    }

    #[test]
    fn segment_distance_endpoint_clamped() {
        // Collinear, disjoint: closest points are the inner endpoints (a.end
        // [1,0,0] ↔ b.start [3,0,0]) → 2.0, not an interior projection.
        let d = segment_segment_distance(
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
        );
        assert!((d - 2.0).abs() < EPS, "endpoint-clamped, got {d}");
    }

    #[test]
    fn polyline_distance_interior_segment_pair() {
        // Closest approach is an interior segment pair (a's first segment vs
        // b's only segment), parallel offset 3 in Y — NOT the polyline endpoints.
        let a = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]];
        let b = [[5.0, 3.0, 0.0], [15.0, 3.0, 0.0]];
        let d = min_polyline_distance(&a, &b);
        assert!((d - 3.0).abs() < EPS, "interior closest approach, got {d}");
    }

    // ── step-11: adjacency population ────────────────────────────────────────

    /// (a) Two parallel same-layer perimeters one line-width (0.45 mm) apart are
    /// in-layer adjacent; a third bead 5 mm away is adjacent to neither.
    /// (c) Pairs are `(lo, hi)`-ordered and de-duplicated.
    #[test]
    fn in_layer_adjacency_by_width_threshold() {
        // width 0.45 ⇒ threshold = 0.5·(0.45+0.45) + slack = 0.675 mm. The two
        // close lines are 0.45 mm apart (< 0.675 ✓); the far line is 5 mm
        // (≫ 0.675 ✗). Geometry is authored, not guessed.
        let src = "\
M83
G1 Z0.2 F7200
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
G1 X0 Y0.45 F9000
G1 X10 Y0.45 E1.0
G1 X0 Y5 F9000
G1 X10 Y5 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 3, "three perimeter beads");
        assert_eq!(
            tp.in_layer_adjacency,
            vec![(0, 1)],
            "only the two close lines are adjacent, (lo,hi)-ordered + deduped"
        );
        assert!(
            tp.inter_layer_adjacency.is_empty(),
            "single layer ⇒ no inter-layer pairs"
        );
    }

    /// (b) Beads in consecutive layers with overlapping XY footprints are
    /// inter-layer adjacent; beads two layers apart are NOT (the |Δlayer|==1
    /// filter, independent of distance).
    #[test]
    fn inter_layer_adjacency_consecutive_only() {
        // Three identical X-lines stacked at Z 0.2 / 0.4 / 0.6. Vertical gap
        // 0.2 mm < threshold 0.675 mm, so consecutive layers bond.
        let src = "\
M83
;LAYER_CHANGE
;Z:0.2
G1 Z0.2 F7200
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
;LAYER_CHANGE
;Z:0.4
G1 Z0.4 F7200
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
;LAYER_CHANGE
;Z:0.6
G1 Z0.6 F7200
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 3, "one bead per layer");
        assert_eq!(tp.beads[0].layer_index, 0);
        assert_eq!(tp.beads[1].layer_index, 1);
        assert_eq!(tp.beads[2].layer_index, 2);
        assert_eq!(
            tp.inter_layer_adjacency,
            vec![(0, 1), (1, 2)],
            "consecutive layers only; (0,2) excluded by |Δlayer|==1"
        );
        assert!(
            tp.in_layer_adjacency.is_empty(),
            "one bead per layer ⇒ no in-layer pairs"
        );
    }

    /// Inter-layer adjacency is governed by layer **height**, not width: two
    /// vertically stacked beads with a tall layer height (0.5) and a narrow
    /// width (0.2) bond across a 0.5 mm vertical gap. A width-only threshold
    /// (0.75·0.4 = 0.3 mm) would wrongly drop the bond (0.5 > 0.3); the
    /// height-aware threshold (0.75·1.0 = 0.75 mm) keeps it (0.5 ≤ 0.75) —
    /// pinning the reviewer-suggestion-3 fix. Geometry is authored, not guessed.
    #[test]
    fn inter_layer_adjacency_uses_height_for_tall_layers() {
        let src = "\
M83
;LAYER_CHANGE
;Z:0.5
;HEIGHT:0.5
G1 Z0.5 F7200
;TYPE:Perimeter
;WIDTH:0.2
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
;LAYER_CHANGE
;Z:1.0
;HEIGHT:0.5
G1 Z1.0 F7200
;TYPE:Perimeter
;WIDTH:0.2
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 2, "one bead per layer");
        assert_eq!(tp.beads[0].layer_index, 0);
        assert_eq!(tp.beads[1].layer_index, 1);
        // Vertical centerline gap is exactly 0.5 mm (1.0 − 0.5); the bead
        // height is 0.5, so the layer-height threshold (0.75) admits the bond
        // that the width threshold (0.3) would have rejected.
        assert!(
            (min_polyline_distance(&tp.beads[0].centerline, &tp.beads[1].centerline) - 0.5).abs()
                < EPS,
            "stacked beads are 0.5 mm apart"
        );
        assert!(
            adjacency_threshold(0.2, 0.2) < 0.5,
            "the width-only threshold WOULD have dropped this bond"
        );
        assert_eq!(
            tp.inter_layer_adjacency,
            vec![(0, 1)],
            "height-derived threshold keeps the tall-layer vertical bond"
        );
    }

    // ── amendments: arc handling, relative-XYZ, inline-F speed ────────────────

    /// A `G2`/`G3` arc (whose deposited geometry is out of scope for ζ — see
    /// `apply_arc`) flushes the active bead and advances the logical position
    /// best-effort, so the next extruding move is seeded from the arc endpoint,
    /// NOT mis-seeded from the pre-arc point. PrusaSlicer's default arc-fitting
    /// is off, but the code path exists and must behave.
    #[test]
    fn arc_move_flushes_bead_and_advances_position() {
        let src = "\
M83
G1 Z0.2 F7200
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X10 Y10 F9000
G1 X20 Y10 E1.0
G2 X30 Y10 I5 J0 E1.0
G1 X40 Y10 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        // The arc breaks the run: the pre-arc extrusion is one bead, the
        // post-arc extrusion another — the arc's own geometry is not deposited.
        assert_eq!(tp.beads.len(), 2, "arc flushes the active bead");
        assert_pts_approx(
            &tp.beads[0].centerline,
            &[[10.0, 10.0, 0.2], [20.0, 10.0, 0.2]],
        );
        // The second bead seeds from the arc endpoint [30,10] (position advanced
        // by the arc), NOT mis-seeded from the pre-arc point [20,10].
        assert_pts_approx(
            &tp.beads[1].centerline,
            &[[30.0, 10.0, 0.2], [40.0, 10.0, 0.2]],
        );
    }

    /// Relative-XYZ mode (`G91`) accumulates each move's deltas onto the current
    /// position; the centerline is the running absolute path. Every fixture and
    /// other test is default-absolute (`G90`), so this exercises the otherwise
    /// untested relative-accumulation branch of `axis`.
    #[test]
    fn relative_xyz_mode_accumulates_centerline() {
        let src = "\
M83
G91
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X10 Y10 F9000
G1 X5 Y0 E1.0
G1 X0 Y5 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 1, "one continuous relative-mode bead");
        // Travel to [10,10], then +[5,0] → [15,10], then +[0,5] → [15,15].
        assert_pts_approx(
            &tp.beads[0].centerline,
            &[[10.0, 10.0, 0.0], [15.0, 10.0, 0.0], [15.0, 15.0, 0.0]],
        );
    }

    /// When the extruding (pen-down) move carries its own `F`, that feedrate is
    /// applied before the bead seed, so `speed` is the print speed — distinct
    /// from the preceding travel's feedrate. (The fixture's extruding moves
    /// carry no in-line F, so every fixture bead's speed is the prior travel's;
    /// this pins the F-before-seed capture directly.)
    #[test]
    fn speed_captures_inline_feedrate_of_extruding_move() {
        let src = "\
M83
G1 Z0.2 F7200
;TYPE:Perimeter
;WIDTH:0.45
;HEIGHT:0.2
G1 X10 Y10 F9000
G1 X20 Y10 E1.0 F1800
G1 X30 Y10 E1.0
";
        let tp = parse_prusaslicer_gcode(src).unwrap();
        assert_eq!(tp.beads.len(), 1);
        assert!(
            (tp.beads[0].speed - 1800.0).abs() < EPS,
            "speed = the extruding move's own F1800, not the F9000 travel, got {}",
            tp.beads[0].speed
        );
    }

    // ── step-4858: differential correctness harness ──────────────────────────

    /// In-crate O(B²) oracle — verbatim copy of the pre-task-4858
    /// `compute_adjacency` double-loop body.  This is the immutable reference
    /// and must NOT be updated to track any algorithmic change in
    /// `compute_adjacency`.
    fn compute_adjacency_reference(beads: &[Bead]) -> (AdjacencyPairs, AdjacencyPairs) {
        let mut in_layer: AdjacencyPairs = Vec::new();
        let mut inter_layer: AdjacencyPairs = Vec::new();
        for i in 0..beads.len() {
            for j in (i + 1)..beads.len() {
                let a = &beads[i];
                let b = &beads[j];
                let same_layer = a.layer_index == b.layer_index;
                let consecutive = a.layer_index.abs_diff(b.layer_index) == 1;
                if !same_layer && !consecutive {
                    continue;
                }
                let d = min_polyline_distance(&a.centerline, &b.centerline);
                let threshold = if same_layer {
                    adjacency_threshold(a.width, b.width)
                } else {
                    inter_layer_threshold(a, b)
                };
                if d <= threshold {
                    if same_layer {
                        in_layer.push((i, j));
                    } else {
                        inter_layer.push((i, j));
                    }
                }
            }
        }
        in_layer.sort_unstable();
        in_layer.dedup();
        inter_layer.sort_unstable();
        inter_layer.dedup();
        (in_layer, inter_layer)
    }

    /// Deterministic xorshift64 PRNG — avoids a `rand` dev-dependency for
    /// synthetic bead generators.
    fn xorshift64(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn xorshift64_f64(state: &mut u64) -> f64 {
        xorshift64(state) as f64 / u64::MAX as f64
    }

    /// Build a deterministic synthetic bead set: `layers` layers × `per_layer`
    /// beads per layer.  All layers share the **same XY lattice positions**
    /// (so consecutive-layer beads are stacked and inter-layer adjacent with
    /// vertical gap = layer height = 0.2 mm < threshold 0.675 mm).  Spacing
    /// = `area / ceil(sqrt(per_layer))` — choosing `area ∝ sqrt(per_layer)`
    /// holds spacing constant across different `per_layer` values, yielding
    /// constant per-cell density for the complexity scaling test.
    ///
    /// Width = 0.45 mm, height = 0.2 mm; each bead is a 1 mm horizontal
    /// centerline.  5 % jitter exercises cell-boundary straddling.
    fn build_synthetic_beads(
        layers: usize,
        per_layer: usize,
        area: f64,
        seed: u64,
    ) -> Vec<Bead> {
        let mut state = seed;
        let grid_side = (per_layer as f64).sqrt().ceil() as usize;
        let spacing = if grid_side > 0 {
            area / grid_side as f64
        } else {
            1.0
        };
        // Generate XY positions once; reuse for every layer so stacked beads
        // are at identical XY coordinates.
        let mut positions: Vec<(f64, f64)> = Vec::with_capacity(per_layer);
        let mut count = 0usize;
        'outer: for row in 0..grid_side {
            for col in 0..grid_side {
                if count >= per_layer {
                    break 'outer;
                }
                let jitter_x = (xorshift64_f64(&mut state) - 0.5) * spacing * 0.05;
                let jitter_y = (xorshift64_f64(&mut state) - 0.5) * spacing * 0.05;
                let x0 = col as f64 * spacing + jitter_x;
                let y0 = row as f64 * spacing + jitter_y;
                positions.push((x0, y0));
                count += 1;
            }
        }
        let mut beads = Vec::with_capacity(layers * per_layer);
        for layer_idx in 0..layers {
            let z = (layer_idx as f64 + 1.0) * 0.2;
            for &(x0, y0) in &positions {
                beads.push(Bead {
                    centerline: vec![[x0, y0, z], [x0 + 1.0, y0, z]],
                    width: 0.45,
                    height: 0.2,
                    role: BeadRole::Perimeter,
                    layer_index: layer_idx,
                    layer_z: z,
                    nominal_temp: 210.0,
                    speed: 9000.0,
                });
            }
        }
        beads
    }

    /// Assert `compute_adjacency_with_stats` returns lists equal to both the
    /// O(B²) oracle reference and the public `compute_adjacency`.
    fn assert_matches_reference(label: &str, beads: &[Bead]) {
        let (in_ref, inter_ref) = compute_adjacency_reference(beads);
        let (in_pub, inter_pub) = compute_adjacency(beads);
        let (in_new, inter_new, _stats) = compute_adjacency_with_stats(beads);
        assert_eq!(
            in_new, in_ref,
            "[{label}] in_layer: seam != O(B²) reference"
        );
        assert_eq!(
            inter_new, inter_ref,
            "[{label}] inter_layer: seam != O(B²) reference"
        );
        assert_eq!(
            in_new, in_pub,
            "[{label}] in_layer: seam != public compute_adjacency"
        );
        assert_eq!(
            inter_new, inter_pub,
            "[{label}] inter_layer: seam != public compute_adjacency"
        );
    }

    /// Differential correctness test: `compute_adjacency_with_stats` must
    /// return bit-identical adjacency lists to the O(B²) oracle across a
    /// battery of synthetic cases, the real fixture, and a large input.
    ///
    /// RED at step-1 (compile error — `compute_adjacency_with_stats` /
    /// `AdjacencyStats` don't exist yet).  GREEN after step-2 passthrough
    /// and stays GREEN after step-4 spatial hash (the real correctness proof).
    #[test]
    fn compute_adjacency_with_stats_matches_reference() {
        // ── case 1: empty input ──────────────────────────────────────────────
        assert_matches_reference("empty", &[]);

        // ── case 2: single bead ─────────────────────────────────────────────
        {
            let b = Bead {
                centerline: vec![[0.0, 0.0, 0.2], [10.0, 0.0, 0.2]],
                width: 0.45,
                height: 0.2,
                role: BeadRole::Perimeter,
                layer_index: 0,
                layer_z: 0.2,
                nominal_temp: 210.0,
                speed: 9000.0,
            };
            assert_matches_reference("single", &[b]);
        }

        // ── case 3: two same-layer near (adjacent) ───────────────────────────
        // width=0.45 → threshold=0.675 mm; gap=0.45 mm → adjacent
        {
            let beads = vec![
                Bead {
                    centerline: vec![[0.0, 0.0, 0.2], [10.0, 0.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
                Bead {
                    centerline: vec![[0.0, 0.45, 0.2], [10.0, 0.45, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
            ];
            assert_matches_reference("same-layer-near", &beads);
        }

        // ── case 4: two same-layer far (NOT adjacent) ────────────────────────
        {
            let beads = vec![
                Bead {
                    centerline: vec![[0.0, 0.0, 0.2], [10.0, 0.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
                Bead {
                    centerline: vec![[0.0, 5.0, 0.2], [10.0, 5.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
            ];
            assert_matches_reference("same-layer-far", &beads);
        }

        // ── case 5: consecutive-layer overlapping (inter-layer adjacent) ─────
        // vertical gap=0.2 mm < threshold 0.675 mm → inter-layer adjacent
        {
            let beads = vec![
                Bead {
                    centerline: vec![[0.0, 0.0, 0.2], [10.0, 0.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
                Bead {
                    centerline: vec![[0.0, 0.0, 0.4], [10.0, 0.0, 0.4]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 1, layer_z: 0.4, nominal_temp: 210.0, speed: 9000.0,
                },
            ];
            assert_matches_reference("consec-layer-adjacent", &beads);
        }

        // ── case 6: beads 2 layers apart (excluded by |Δlayer|==1) ──────────
        {
            let beads = vec![
                Bead {
                    centerline: vec![[0.0, 0.0, 0.2], [10.0, 0.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
                Bead {
                    centerline: vec![[0.0, 0.0, 0.6], [10.0, 0.0, 0.6]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 2, layer_z: 0.6, nominal_temp: 210.0, speed: 9000.0,
                },
            ];
            assert_matches_reference("skip-2-layers", &beads);
        }

        // ── case 7: beads straddling a spatial-hash cell boundary ────────────
        // Bead A ends at x=0; bead B starts at x=0.3.  Gap 0.3 < threshold
        // 0.675 → adjacent; must be found even when they straddle a cell edge.
        {
            let beads = vec![
                Bead {
                    centerline: vec![[-0.5, 0.0, 0.2], [0.0, 0.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
                Bead {
                    centerline: vec![[0.3, 0.0, 0.2], [0.8, 0.0, 0.2]],
                    width: 0.45, height: 0.2, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.2, nominal_temp: 210.0, speed: 9000.0,
                },
            ];
            assert_matches_reference("cell-boundary", &beads);
        }

        // ── case 8: tall-layer height-threshold (height 0.5 / width 0.2) ─────
        // Mirrors `inter_layer_adjacency_uses_height_for_tall_layers`.
        // Height threshold 0.75 > width threshold 0.3; gap 0.5 ≤ 0.75 → adjacent.
        {
            let beads = vec![
                Bead {
                    centerline: vec![[0.0, 0.0, 0.5], [10.0, 0.0, 0.5]],
                    width: 0.2, height: 0.5, role: BeadRole::Perimeter,
                    layer_index: 0, layer_z: 0.5, nominal_temp: 210.0, speed: 9000.0,
                },
                Bead {
                    centerline: vec![[0.0, 0.0, 1.0], [10.0, 0.0, 1.0]],
                    width: 0.2, height: 0.5, role: BeadRole::Perimeter,
                    layer_index: 1, layer_z: 1.0, nominal_temp: 210.0, speed: 9000.0,
                },
            ];
            assert_matches_reference("tall-layer", &beads);
        }

        // ── case 9: parsed prusaslicer_bracket.gcode fixture ─────────────────
        {
            let tp = parse_prusaslicer_gcode(include_str!(
                "../tests/fixtures/prusaslicer_bracket.gcode"
            ))
            .expect("fixture must parse");
            assert_matches_reference("bracket-fixture", &tp.beads);
        }

        // ── case 10: large deterministic synthetic input (~2000 beads) ────────
        {
            let beads = build_synthetic_beads(5, 400, 100.0, 0xdead_beef_cafe_babe);
            assert_matches_reference("large-synthetic", &beads);
        }
    }

    // ── amendments: error variants + Display + source-line provenance ─────────

    /// A malformed `;WIDTH:` / `;HEIGHT:` / `;Z:` value is a
    /// [`ToolpathParseError::Comment`] carrying the 1-indexed source line and the
    /// offending raw line — the only failure mode of the comment state machine,
    /// and previously never exercised.
    #[test]
    fn malformed_width_directive_is_comment_error() {
        let err = parse_prusaslicer_gcode(";WIDTH:notanumber\n").unwrap_err();
        match &err {
            ToolpathParseError::Comment { line, raw } => {
                assert_eq!(*line, 1, "1-indexed source line of the bad directive");
                assert!(
                    raw.contains("WIDTH:notanumber"),
                    "raw offending line preserved, got {raw:?}"
                );
            }
            other => panic!("expected Comment error, got {other:?}"),
        }
        assert!(
            err.to_string()
                .contains("malformed directive comment at line 1"),
            "Display surfaces the source line, got {err}"
        );
    }

    /// A move line that fails the delegated `reify_gcode::parse_marlin` surfaces
    /// as [`ToolpathParseError::Gcode`]. Critically, the inner `ParseError.line`
    /// is the **toolpath source** line (here 3), not the per-line slice's line 1
    /// — pinning the source-position re-stamp.
    #[test]
    fn malformed_move_line_is_gcode_error_with_source_line() {
        let src = "\
M83
G1 Z0.2 F7200
G1 Xabc
";
        let err = parse_prusaslicer_gcode(src).unwrap_err();
        match &err {
            ToolpathParseError::Gcode(e) => {
                assert_eq!(
                    e.line, 3,
                    "error carries the toolpath source line, not the slice's line 1"
                );
            }
            other => panic!("expected Gcode error, got {other:?}"),
        }
        assert!(
            err.to_string().contains("g-code move parse error"),
            "Display surfaces the g-code failure, got {err}"
        );
    }
}
