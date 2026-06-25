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
    /// underlying `reify-gcode` low-level parser.
    Gcode(reify_gcode::ParseError),
    /// A structured directive comment (`;WIDTH:`, `;HEIGHT:`) carried a value
    /// that did not parse as a number. `line` is 1-indexed; `raw` is the
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
pub fn parse_prusaslicer_gcode(src: &str) -> Result<Toolpath, ToolpathParseError> {
    let mut sweep = Sweep::new();

    for (idx, raw) in src.split('\n').enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(';') {
            // Comment line: structured directive or free text.
            if let Some(t) = rest.strip_prefix("TYPE:") {
                sweep.role = role_from_prusaslicer_type(t.trim());
            } else if let Some(w) = rest.strip_prefix("WIDTH:") {
                sweep.width = parse_comment_f64(line_no, line, w)?;
            } else if let Some(h) = rest.strip_prefix("HEIGHT:") {
                sweep.height = parse_comment_f64(line_no, line, h)?;
            }
            // `;LAYER_CHANGE` / `;Z:` are handled in step-8; other comments
            // (e.g. `; generated by PrusaSlicer`) are free text — skip.
            continue;
        }
        // Non-comment line: dispatch on the leading token.
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "G0" | "G1" | "G2" | "G3" | "G92" => {
                let cmds =
                    reify_gcode::parse_marlin(line).map_err(ToolpathParseError::Gcode)?;
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
    Ok(Toolpath {
        beads,
        layers,
        in_layer_adjacency: Vec::new(),
        inter_layer_adjacency: Vec::new(),
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
            cur: None,
        }
    }

    /// Finalise the active bead (if any) into the bead list. A bead always
    /// carries ≥2 points (pen-down + ≥1 endpoint); the length guard is a
    /// defensive backstop.
    fn flush(&mut self) {
        if let Some(b) = self.cur.take() {
            if b.centerline.len() >= 2 {
                self.beads.push(b.finish());
            }
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
}
