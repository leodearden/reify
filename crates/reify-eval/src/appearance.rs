//! Appearance resolution seam: `resolve_color` and `resolve_appearance`.
//!
//! Consumed by task δ (3MF color egress, `engine_build.rs`) and cross-PRD
//! PRD-2 (viewport recolor).  Both functions are `pub`; downstream callers
//! compose them as `resolve_color(&resolve_appearance(body).fields["color"], diags)`.
//!
//! PRD: `docs/prds/v0_6/appearance-substrate.md` §4.2/§7.3 (task β, #4761).

use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{PersistentMap, Rgb8, StructureInstanceData, StructureTypeId, Value};

// ── private helpers ───────────────────────────────────────────────────────────

/// Extract an `f64` from a numeric Value cell (Int / Real / Scalar).
///
/// Intentional mirror of `dynamics_ops::cell_f64` (same match arms, same semantics).
/// Both constants are kept private to their respective modules; the duplication is
/// small and self-documenting. If either module's cell extraction needs to diverge,
/// the mirror relationship is made explicit here.  Non-numeric → `None`.
fn color_cell_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Real(r) => Some(*r),
        Value::Scalar { si_value, .. } => Some(*si_value),
        _ => None,
    }
}

/// Map an f64 component through clamp([0,1]) → * 255.0 → round → u8.
/// Used for the `named=""` (rgb-component) path.
///
/// Note: `0.7_f64 * 255.0 ≈ 178.499...` rounds to **178** (half-away-from-zero
/// via `f64::round`, which rounds 178.5 → 179 but 178.499 → 178). Tests that
/// assert via `clamp_round(0.7)` will naturally agree with this formula.
fn clamp_round(x: f64) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Parse a `#RRGGBB` (6 hex digits) string into `Rgb8`.
/// Returns `None` if the length or characters are wrong.
fn parse_hex6(s: &str) -> Option<Rgb8> {
    // s must be "#RRGGBB" — 7 bytes, all ASCII hex after the '#'.
    let bytes = s.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }
    let r = hex_byte(bytes[1], bytes[2])?;
    let g = hex_byte(bytes[3], bytes[4])?;
    let b = hex_byte(bytes[5], bytes[6])?;
    Some(Rgb8 { r, g, b })
}

/// Parse a `#RGB` (3 hex digits, nibble-doubled) string into `Rgb8`.
/// Returns `None` if the length or characters are wrong.
fn parse_hex3(s: &str) -> Option<Rgb8> {
    // s must be "#RGB" — 4 bytes, each hex nibble after '#' doubled.
    let bytes = s.as_bytes();
    if bytes.len() != 4 || bytes[0] != b'#' {
        return None;
    }
    let r = nibble_double(bytes[1])?;
    let g = nibble_double(bytes[2])?;
    let b = nibble_double(bytes[3])?;
    Some(Rgb8 { r, g, b })
}

/// Combine two hex ASCII digits (high nibble, low nibble) into a byte.
fn hex_byte(hi: u8, lo: u8) -> Option<u8> {
    let h = hex_nibble(hi)?;
    let l = hex_nibble(lo)?;
    Some((h << 4) | l)
}

/// Double a single hex ASCII nibble: `'A'` → `0xAA`.
fn nibble_double(c: u8) -> Option<u8> {
    let n = hex_nibble(c)?;
    Some((n << 4) | n)
}

/// Convert a hex ASCII digit (0-9, A-F, a-f) to its nibble value.
fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

// ── RAL Classic seed table ────────────────────────────────────────────────────

/// A small documented seed of RAL Classic → sRGB byte triples.
///
/// Values are approximate sRGB conversions from the widely-cited RAL
/// Classic standard. Breadth is tactical (PRD §11 OQ1); unknown names
/// fall through to `W_UNKNOWN_COLOR_NAME`.  Match is exact and
/// case-sensitive (the RAL naming convention is all-caps with no spaces,
/// e.g. `"RAL7035"`).
///
/// Required entries (PRD §3 fixture + B4 boundary tests):
/// - `RAL7035` (Light grey)
/// - `RAL9006` (White aluminium)
static RAL_SEED: &[(&str, Rgb8)] = &[
    // Greys / neutrals
    ("RAL7016", Rgb8 { r: 41,  g: 49,  b: 51  }), // Anthracite grey
    ("RAL7035", Rgb8 { r: 215, g: 215, b: 215 }), // Light grey  ← PRD §3 fixture
    // Whites / near-whites
    ("RAL9005", Rgb8 { r: 14,  g: 14,  b: 16  }), // Jet black
    ("RAL9006", Rgb8 { r: 164, g: 167, b: 160 }), // White aluminium  ← B4
    ("RAL9016", Rgb8 { r: 246, g: 246, b: 242 }), // Traffic white
];

/// Look up a RAL Classic name in the seed table.
/// Returns `Some(Rgb8)` on an exact case-sensitive match, `None` on miss.
fn ral_lookup(name: &str) -> Option<Rgb8> {
    RAL_SEED.iter().find(|(n, _)| *n == name).map(|(_, rgb)| *rgb)
}

// ── public seam ───────────────────────────────────────────────────────────────

/// Resolve a `Color` `Value::StructureInstance` to an sRGB byte triple.
///
/// Resolution ladder (PRD §4.2/§7.3):
/// 1. `named == ""` → `clamp_round(r, g, b)`.
/// 2. `named` starts with `#` and is exactly `#RRGGBB` or `#RGB` → parse EXACTLY
///    (no clamp/round; `#RGB` nibble-doubles each nibble).
/// 3. `named` is a seeded RAL Classic name (e.g. `"RAL7035"`) → tabled sRGB value.
/// 4. Any other non-empty `named` (including malformed `#…`) → push
///    `W_UNKNOWN_COLOR_NAME` warning and fall back to `clamp_round(r, g, b)`.
///
/// This function is TOTAL: it always returns an `Rgb8`, using the clamp fallback
/// on unrecognised names so downstream callers never see a silent black default.
///
/// # Malformed-input behaviour (intentional, silent)
///
/// A `color` value that is not a `StructureInstance`, or a `Color` `StructureInstance`
/// missing one or more of `named`/`r`/`g`/`b`, falls back silently to `Rgb8 { 0, 0, 0 }`.
/// This is intentional and documented here explicitly:
/// the reify type system guarantees that `Appearance.color` is always a
/// fully-populated `Color` instance (mandatory field, no `Option`), so these branches
/// are only reachable from hand-crafted `Value`s or eval-layer bugs — neither of which
/// warrants a `W_UNKNOWN_COLOR_NAME` diagnostic.
/// See unit tests `resolve_color_non_struct_value_returns_black` and
/// `resolve_color_missing_rgb_fields_returns_black` for coverage of these paths.
pub fn resolve_color(color: &Value, diagnostics: &mut Vec<Diagnostic>) -> Rgb8 {
    // Extract named / r / g / b from the Color StructureInstance.
    //
    // Graceful degradation for out-of-contract inputs (intentional, silent — see doc-comment):
    // - Non-StructureInstance `color` → treat as named="", r=g=b=0.0 → Rgb8{0,0,0}.
    // - Missing field in a Color StructureInstance → each absent field defaults to "" / 0.0.
    // Both paths return Rgb8{0,0,0} with no diagnostic.
    let (named, r, g, b) = if let Value::StructureInstance(data) = color {
        let named = match data.fields.get("named") {
            Some(Value::String(s)) => s.as_str(),
            _ => "",
        };
        let r = data.fields.get("r").and_then(color_cell_f64).unwrap_or(0.0);
        let g = data.fields.get("g").and_then(color_cell_f64).unwrap_or(0.0);
        let b = data.fields.get("b").and_then(color_cell_f64).unwrap_or(0.0);
        (named.to_string(), r, g, b)
    } else {
        (String::new(), 0.0, 0.0, 0.0)
    };

    if named.is_empty() {
        // Path 1: rgb-component path.
        return Rgb8 { r: clamp_round(r), g: clamp_round(g), b: clamp_round(b) };
    }

    if named.starts_with('#') {
        // Path 2: hex parse (exact, no clamp/round).
        if let Some(rgb) = parse_hex6(&named).or_else(|| parse_hex3(&named)) {
            return rgb;
        }
        // Malformed `#…` falls through to the unknown-name path (path 4 below).
    }

    // Path 3: seeded RAL Classic name → tabled sRGB (exact, case-sensitive match).
    // Breadth is tactical — PRD §11 OQ1; unknown names use W_UNKNOWN_COLOR_NAME.
    if let Some(rgb) = ral_lookup(&named) {
        return rgb;
    }

    // Path 4: unknown name → warn + clamp fallback.
    diagnostics.push(
        Diagnostic::warning(format!(
            "resolve_color: unknown color name '{named}'; falling back to (r,g,b)"
        ))
        .with_code(DiagnosticCode::UnknownColorName),
    );
    Rgb8 { r: clamp_round(r), g: clamp_round(g), b: clamp_round(b) }
}

/// Sentinel `StructureTypeId` for engine-assembled (registry-free) instances.
///
/// Intentional mirror of `dynamics_ops::REGISTRY_FREE_TYPE_ID = StructureTypeId(u32::MAX)`.
/// Both constants agree on the sentinel value; downstream code keys on `type_name`, not
/// `type_id`, so they cannot silently diverge in observable behaviour.  The duplication
/// is small and self-contained; promoting to a shared constant would require a common
/// reify-eval or reify-ir site and is deferred (noted in reviewer suggestion 4, #4761).
const REGISTRY_FREE_TYPE_ID: StructureTypeId = StructureTypeId(u32::MAX);

/// Hand-mint a neutral-grey `Appearance` `Value::StructureInstance` that mirrors the
/// default `Appearance()` from `crates/reify-compiler/stdlib/materials_appearance.ri`.
///
/// Defaults: `color = Color(named:"", r:0.7, g:0.7, b:0.7)` (light grey),
/// `finish = Finish.Satin`, `metalness = 0.0`, `roughness = 0.5`.
///
/// The S7 e2e anti-drift test (`resolve_appearance_e2e_styled_and_plain_bodies`) asserts
/// that this hand-minted value resolves to the same colour as the real stdlib `Appearance()`
/// default, guarding against the two drifting if the .ri defaults are ever updated.
fn neutral_appearance() -> Value {
    // Inner Color StructureInstance — r/g/b = 0.7 (neutral grey).
    let neutral_color: Value = {
        let fields: PersistentMap<String, Value> = [
            ("named".to_string(), Value::String(String::new())),
            ("r".to_string(), Value::Real(0.7)),
            ("g".to_string(), Value::Real(0.7)),
            ("b".to_string(), Value::Real(0.7)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: REGISTRY_FREE_TYPE_ID,
            type_name: "Color".to_string(),
            version: 1,
            fields,
        }))
    };

    // Outer Appearance StructureInstance.
    let fields: PersistentMap<String, Value> = [
        ("color".to_string(), neutral_color),
        (
            "finish".to_string(),
            Value::enum_unit("Finish", "Satin"),
        ),
        ("metalness".to_string(), Value::Real(0.0)),
        ("roughness".to_string(), Value::Real(0.5)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE_TYPE_ID,
        type_name: "Appearance".to_string(),
        version: 1,
        fields,
    }))
}

// ── private mint helpers ──────────────────────────────────────────────────────

/// Construct a `Color` `Value::StructureInstance` with the given RGBA components.
/// Used by `coating_appearance` and `finish_modulation` to build Color values inline
/// without duplicating the field-map idiom from `neutral_appearance`.
fn make_color(named: &str, r: f64, g: f64, b: f64) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("named".to_string(), Value::String(named.to_string())),
        ("r".to_string(), Value::Real(r)),
        ("g".to_string(), Value::Real(g)),
        ("b".to_string(), Value::Real(b)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE_TYPE_ID,
        type_name: "Color".to_string(),
        version: 1,
        fields,
    }))
}

/// Construct an `Appearance` `Value::StructureInstance` from pre-built components.
/// Used by `coating_appearance` and `finish_modulation`; mirrors the field layout of
/// `neutral_appearance()` and the stdlib `Appearance()` default.
fn make_appearance(color: Value, finish: &str, metalness: f64, roughness: f64) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("color".to_string(), color),
        ("finish".to_string(), Value::enum_unit("Finish", finish)),
        ("metalness".to_string(), Value::Real(metalness)),
        ("roughness".to_string(), Value::Real(roughness)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE_TYPE_ID,
        type_name: "Appearance".to_string(),
        version: 1,
        fields,
    }))
}

// ── public functional producers ───────────────────────────────────────────────

/// Derive an `Appearance` from a `Coating` StructureInstance.
///
/// Returns `None` when `coating.process == Uncoated` (inert sentinel: preserves B5
/// back-compat — any body with a defaulted Uncoated coating resolves identically to
/// the pre-β material/neutral path).
///
/// For all non-Uncoated processes returns `Some(Appearance)` where:
/// - `color`: `coating.color` when meaningful (named non-empty OR any of r/g/b nonzero),
///   else a process-characteristic default `Color` so egress never sees a silent black.
/// - `finish` / `metalness` / `roughness`: editorial PBR projection (PRD §OQ2):
///   Anodize → Matte, dielectric, rougher;
///   PowderCoat/Paint → color pass-through, dielectric, Satin/Gloss (S4);
///   Electroplate → metallic (S4); Passivate → subtle (S4).
///   S2 implements Anodize fully; other arms default to Satin/dielectric/0.5 (S4).
///
/// `pub`: mirrors `resolve_color` as a named seam fn consumed by `resolve_appearance_opt`.
pub fn coating_appearance(coating: &Value) -> Option<Value> {
    // Navigate coating.process; any non-StructureInstance or missing process → None.
    let data = match coating {
        Value::StructureInstance(d) => d,
        _ => return None,
    };
    let variant = match data.fields.get("process") {
        Some(Value::Enum { variant, .. }) => variant.as_str(),
        _ => return None,
    };
    if variant == "Uncoated" {
        return None;
    }

    // Determine whether coating.color is "meaningful" (named non-empty OR any channel nonzero).
    let coating_color = data.fields.get("color");
    let is_meaningful = if let Some(Value::StructureInstance(cd)) = coating_color {
        let named_nonempty =
            matches!(cd.fields.get("named"), Some(Value::String(s)) if !s.is_empty());
        let r_nz = cd.fields.get("r").and_then(color_cell_f64).unwrap_or(0.0) != 0.0;
        let g_nz = cd.fields.get("g").and_then(color_cell_f64).unwrap_or(0.0) != 0.0;
        let b_nz = cd.fields.get("b").and_then(color_cell_f64).unwrap_or(0.0) != 0.0;
        named_nonempty || r_nz || g_nz || b_nz
    } else {
        false
    };

    // PBR projection for each process (editorial table, PRD §OQ2).
    // `default_color` is only used when `is_meaningful` is false (the coating's Color
    // field is the all-zero inert default) — ensures the never-silent-black invariant.
    let (finish_variant, metalness, roughness, default_color) = match variant {
        // Oxide coating: dark, matte, dielectric.
        "Anodize" => (
            "Matte",
            0.0_f64,
            0.6_f64,
            make_color("", 0.15, 0.15, 0.15), // characteristic dark grey
        ),
        // Deposited metal: bright, polished, metallic.
        "Electroplate" => (
            "Gloss",
            0.9_f64,
            0.15_f64,
            make_color("", 0.82, 0.82, 0.86), // light metallic silver (~209/209/219)
        ),
        // Powder-coat paint: pass color, satin, dielectric.
        "PowderCoat" => (
            "Satin",
            0.0_f64,
            0.4_f64,
            make_color("", 0.5, 0.5, 0.5), // neutral mid-grey fallback
        ),
        // Liquid paint: pass color, gloss, dielectric.
        "Paint" => (
            "Gloss",
            0.0_f64,
            0.3_f64,
            make_color("", 0.5, 0.5, 0.5), // neutral mid-grey fallback
        ),
        // Passivation (chemical conversion): subtle metalness, near-substrate light.
        "Passivate" => (
            "Satin",
            0.1_f64,
            0.4_f64,
            make_color("", 0.75, 0.78, 0.72), // near-substrate light grey
        ),
        // Unknown future variants: safe dielectric mid-grey default.
        _ => ("Satin", 0.0_f64, 0.5_f64, make_color("", 0.5, 0.5, 0.5)),
    };

    let color_field = if is_meaningful {
        coating_color.unwrap().clone()
    } else {
        default_color
    };

    Some(make_appearance(color_field, finish_variant, metalness, roughness))
}

/// Modulate the surface `Appearance` of a material with a cosmetic `FinishProcess`.
///
/// `AsMachined` is the **inert sentinel**: returns `base_appearance` unchanged (clone),
/// preserving B5 back-compat for bodies with the default `FinishProcess` value.
/// Any non-`Value::Enum` input is also treated as identity.
///
/// For all other variants, only `finish` and `roughness` are overwritten; `color`
/// (material pigment) and `metalness` (dielectric vs. metal character) are preserved.
/// Editorial PBR projection (PRD §OQ2):
/// - Polished / Lapped → Gloss, roughness ~0.1 (high sheen = low roughness)
/// - Ground / Brushed → Satin, roughness ~0.35
/// - BeadBlasted / AsCast → Matte, roughness ~0.8
/// - Unknown future variants → identity (conservative; back-compat)
///
/// `pub`: consumed by the functional layer of `resolve_appearance_opt` (§7.3 precedence).
pub fn finish_modulation(finish_process: &Value, base_appearance: &Value) -> Value {
    // Non-enum or AsMachined (inert sentinel) → identity.
    let variant = match finish_process {
        Value::Enum { variant, .. } => variant.as_str(),
        _ => return base_appearance.clone(),
    };
    if variant == "AsMachined" {
        return base_appearance.clone();
    }

    // Map variant to (Finish enum string, roughness scalar).
    let (finish_variant, roughness): (&str, f64) = match variant {
        "Polished" | "Lapped" => ("Gloss", 0.1),
        "Ground" | "Brushed" => ("Satin", 0.35),
        "BeadBlasted" | "AsCast" => ("Matte", 0.8),
        // Unknown future variants → identity (conservative; forward-compat).
        _ => return base_appearance.clone(),
    };

    // Clone base and overwrite only finish+roughness; color and metalness are preserved.
    let mut data = match base_appearance.clone() {
        Value::StructureInstance(d) => d,
        other => return other, // non-struct base → return unchanged
    };
    data.fields.insert("finish".to_string(), Value::enum_unit("Finish", finish_variant));
    data.fields.insert("roughness".to_string(), Value::Real(roughness));
    Value::StructureInstance(data)
}

/// Resolve the `Appearance` for a body IF it has a material with an appearance, otherwise
/// return `None`.
///
/// This is the **egress predicate** for the PRD-2 §7.1 invariant: `MeshData.appearance`
/// must be `Some` IFF the entity resolves to a material; `None` means "honest hash
/// fallback (layer 1)" — never a silent neutral-grey.
///
/// Navigation: `body.material.appearance` — any missing link (non-struct body, no
/// `material` field, non-struct material, no `appearance` field, non-struct appearance)
/// returns `None` rather than the neutral-grey fallback.
///
/// Returns `Some(app.clone())` where `app` is the `Appearance` StructureInstance when all
/// links navigate successfully; `None` otherwise.
///
/// # Relation to [`resolve_appearance`]
///
/// [`resolve_appearance`] is TOTAL and delegates to this function:
/// `resolve_appearance_opt(body).unwrap_or_else(neutral_appearance)`.
/// Use `resolve_appearance_opt` for the layer-2 egress path (viewport/engine) and
/// `resolve_appearance` where a neutral-grey fallback is always acceptable (3MF/δ).
pub fn resolve_appearance_opt(body: &Value) -> Option<Value> {
    if let Value::StructureInstance(data) = body
        && let Some(Value::StructureInstance(material)) = data.fields.get("material")
        && let Some(app @ Value::StructureInstance(_)) = material.fields.get("appearance")
    {
        return Some(app.clone());
    }
    None
}

/// Resolve the `Appearance` for a body, navigating `body.material.appearance`.
///
/// Mirrors `dynamics_ops::body_material_density` (dynamics_ops.rs `body_material_density`):
/// any missing link (non-struct body, no `material` field, non-struct material, no
/// `appearance` field, non-struct appearance) falls back to [`neutral_appearance`].
///
/// Returns a `Value::StructureInstance` with `type_name == "Appearance"` in all cases.
///
/// # Composing with `resolve_color`
///
/// δ (3MF color egress, `engine_build.rs`) and PRD-2 (viewport recolor) compose as:
/// ```ignore
/// let app = resolve_appearance(body);
/// let color = struct_field(&app, "color").unwrap_or_default();
/// let rgb = resolve_color(&color, &mut diagnostics);
/// ```
pub fn resolve_appearance(body: &Value) -> Value {
    resolve_appearance_opt(body).unwrap_or_else(neutral_appearance)
}

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticCode, Severity};
    use reify_ir::{PersistentMap, Rgb8, StructureInstanceData, StructureTypeId, Value};

    use super::{
        clamp_round, coating_appearance, finish_modulation, resolve_appearance,
        resolve_appearance_opt, resolve_color,
    };

    /// Sentinel type_id for hand-minted test Color instances (no registry lookup needed).
    const TEST_TYPE_ID: StructureTypeId = StructureTypeId(u32::MAX);

    /// Build a `Color` `Value::StructureInstance` for test inputs.
    /// Mirrors the `Color` struct from `stdlib/materials_appearance.ri`:
    /// `structure def Color { named:String=""; r/g/b:Real=0.0 }`.
    fn color(named: &str, r: f64, g: f64, b: f64) -> Value {
        let fields: PersistentMap<String, Value> = [
            ("named".to_string(), Value::String(named.to_string())),
            ("r".to_string(), Value::Real(r)),
            ("g".to_string(), Value::Real(g)),
            ("b".to_string(), Value::Real(b)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Color".to_string(),
            version: 1,
            fields,
        }))
    }

    // ── B3: hex exact + empty-named clamp_round ───────────────────────────────

    /// `#RRGGBB` → byte-exact parse; no diagnostics.
    #[test]
    fn resolve_color_six_hex_exact() {
        let c = color("#8899AA", 0.0, 0.0, 0.0);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(result, Rgb8 { r: 0x88, g: 0x99, b: 0xAA }, "#8899AA must parse byte-exact");
        assert!(diags.is_empty(), "no diagnostics expected for valid hex, got: {diags:#?}");
    }

    /// `#RGB` (3 hex digits) → nibble-doubled parse; no diagnostics.
    #[test]
    fn resolve_color_short_hex_nibble_doubled() {
        let c = color("#8AF", 0.0, 0.0, 0.0);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(
            result,
            Rgb8 { r: 0x88, g: 0xAA, b: 0xFF },
            "#8AF must nibble-double to 0x88, 0xAA, 0xFF"
        );
        assert!(diags.is_empty(), "no diagnostics expected for valid short hex, got: {diags:#?}");
    }

    /// Empty `named` with (r,g,b) in [0,1] → clamp_round path.
    /// 0.5 * 255 = 127.5 → round() = 128.
    #[test]
    fn resolve_color_empty_named_clamp_round() {
        let c = color("", 0.0, 0.5, 1.0);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(result, Rgb8 { r: 0, g: 128, b: 255 }, "empty named: clamp_round((0,0.5,1))");
        assert!(
            diags.is_empty(),
            "no diagnostics expected for empty named, got: {diags:#?}"
        );
    }

    /// Empty `named` with out-of-range components → clamp first, then round.
    /// clamp(-0.2,0,1)=0 → 0; clamp(1.4,0,1)=1 → 255; clamp(0.42,0,1)=0.42 → 0.42*255≈107.1→107.
    #[test]
    fn resolve_color_empty_named_clamp_out_of_range() {
        let c = color("", -0.2, 1.4, 0.42);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(
            result,
            Rgb8 { r: 0, g: 255, b: 107 },
            "empty named: clamp((-0.2,1.4,0.42)) → (0,255,107)"
        );
        assert!(
            diags.is_empty(),
            "no diagnostics expected for empty named out-of-range, got: {diags:#?}"
        );
    }

    // ── B4: RAL seed + unknown-name warning ───────────────────────────────────

    /// Round-trip fixture constants for the seeded RAL entries.
    /// These exact values must match the RAL_SEED table added in S6.
    const RAL7035_RGB: Rgb8 = Rgb8 { r: 215, g: 215, b: 215 };
    const RAL9006_RGB: Rgb8 = Rgb8 { r: 164, g: 167, b: 160 };

    /// RAL9006 (White Aluminium) — seeded name → tabled sRGB, no diagnostics.
    /// The rgb fields (0,0,0) are ignored when `named` is non-empty and in the seed.
    /// Fails until S6 adds the RAL seed table.
    #[test]
    fn resolve_color_ral9006_seeded() {
        let c = color("RAL9006", 0.0, 0.0, 0.0);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(result, RAL9006_RGB, "RAL9006 must return its tabled sRGB value");
        assert!(diags.is_empty(), "seeded RAL name must produce no diagnostics, got: {diags:#?}");
    }

    /// RAL7035 (Light Grey) — seeded name → tabled sRGB, no diagnostics.
    /// PRD §3 fixture; the rgb fields (0,0,0) are ignored when `named` hits the seed.
    /// Fails until S6 adds the RAL seed table.
    #[test]
    fn resolve_color_ral7035_seeded() {
        let c = color("RAL7035", 0.0, 0.0, 0.0);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(result, RAL7035_RGB, "RAL7035 must return its tabled sRGB value");
        assert!(diags.is_empty(), "seeded RAL name must produce no diagnostics, got: {diags:#?}");
    }

    /// Unknown name → exactly one W_UNKNOWN_COLOR_NAME Warning + clamp_round fallback.
    /// 0.4*255 = 102 (exact); 0.42*255 ≈ 107.1 → 107.
    #[test]
    fn resolve_color_unknown_name_warns_and_falls_back() {
        let c = color("RALZZZZ", 0.4, 0.4, 0.42);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(
            result,
            Rgb8 { r: 102, g: 102, b: 107 },
            "unknown name must fall back to clamp_round(r,g,b)"
        );
        assert_eq!(diags.len(), 1, "expected exactly one diagnostic for unknown name, got: {diags:#?}");
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::UnknownColorName),
            "expected UnknownColorName code, got: {:?}",
            diags[0].code
        );
        assert_eq!(
            diags[0].severity,
            Severity::Warning,
            "expected Warning severity, got: {:?}",
            diags[0].severity
        );
    }

    /// Malformed `#…` hex → routed through the unknown-name path (not a valid hex
    /// parse), so exactly one W_UNKNOWN_COLOR_NAME Warning + clamp_round fallback.
    /// 0.1*255=25.5→26; 0.2*255=51.0→51; 0.3*255=76.5→77.
    #[test]
    fn resolve_color_malformed_hex_warns_and_falls_back() {
        let c = color("#XYZ", 0.1, 0.2, 0.3);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&c, &mut diags);
        assert_eq!(
            result,
            Rgb8 { r: 26, g: 51, b: 77 },
            "malformed hex must fall back to clamp_round(r,g,b)"
        );
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one diagnostic for malformed hex, got: {diags:#?}"
        );
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::UnknownColorName),
            "expected UnknownColorName code for malformed hex, got: {:?}",
            diags[0].code
        );
        assert_eq!(
            diags[0].severity,
            Severity::Warning,
            "expected Warning severity for malformed hex, got: {:?}",
            diags[0].severity
        );
    }

    // ── B5: resolve_appearance unit tests ─────────────────────────────────────

    /// Extract a field from a StructureInstance for test assertions.
    fn struct_field_unit(val: &Value, key: &str) -> Option<Value> {
        match val {
            Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
            _ => None,
        }
    }

    /// Build an `Appearance` `Value::StructureInstance` for tests.
    /// Mirrors `materials_appearance.ri` Appearance(color=Color(r,g,b), finish=Satin, ...).
    fn appearance_val(r: f64, g: f64, b: f64) -> Value {
        let color_val = color("", r, g, b);
        let fields: PersistentMap<String, Value> = [
            ("color".to_string(), color_val),
            (
                "finish".to_string(),
                Value::enum_unit("Finish", "Satin"),
            ),
            ("metalness".to_string(), Value::Real(0.0)),
            ("roughness".to_string(), Value::Real(0.5)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Appearance".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a `Material` `Value::StructureInstance` carrying the given appearance.
    fn material_with_appearance(app: Value) -> Value {
        let fields: PersistentMap<String, Value> = [
            ("name".to_string(), Value::String("test".to_string())),
            ("appearance".to_string(), app),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Material".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a synthetic body `Value::StructureInstance` with a single `material` field.
    fn body_with_material(material: Value) -> Value {
        let fields: PersistentMap<String, Value> =
            [("material".to_string(), material)].into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "SyntheticBody".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Round-trip: body → material → appearance → color = (0.4, 0.4, 0.42).
    /// resolve_appearance extracts the Appearance; resolve_color maps to Rgb8{102,102,107}.
    /// 0.4*255 = 102.0 (exact); 0.42*255 ≈ 107.1 → 107.
    /// Fails until S8 introduces `resolve_appearance`.
    #[test]
    fn resolve_appearance_extracts_nested_color() {
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app);
        let body = body_with_material(material);

        let app_result = resolve_appearance(&body);
        let color_result =
            struct_field_unit(&app_result, "color").expect("Appearance must have a `color` field");

        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_result, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 102, g: 102, b: 107 },
            "r:0.4→102; g:0.4→102; b:0.42→107 via resolve_color"
        );
        assert!(diags.is_empty(), "no diags expected, got: {diags:#?}");
    }

    /// Neutral fallback: `Value::Int` body (non-StructureInstance) → resolve_appearance
    /// returns a hand-minted neutral-grey Appearance with type_name "Appearance".
    /// Its color resolves to Rgb8{N,N,N} where N = clamp_round(0.7) = 178.
    /// (0.7 * 255 ≈ 178.499 → round → 178.)
    /// Fails until S8 introduces `resolve_appearance` / `neutral_appearance`.
    #[test]
    fn resolve_appearance_neutral_fallback_non_struct_body() {
        let body = Value::Int(42);
        let app = resolve_appearance(&body);
        match &app {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "Appearance", "fallback type_name must be Appearance");
            }
            other => panic!("expected Appearance StructureInstance, got {other:?}"),
        }
        let color_result =
            struct_field_unit(&app, "color").expect("neutral Appearance must have a `color` field");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_result, &mut diags);
        let n = clamp_round(0.7); // 178
        assert_eq!(rgb, Rgb8 { r: n, g: n, b: n }, "neutral grey via clamp_round(0.7)=178");
        assert!(diags.is_empty(), "no diags expected, got: {diags:#?}");
    }

    /// Neutral fallback: body StructureInstance with NO `material` field →
    /// resolve_appearance returns the hand-minted neutral Appearance.
    /// Fails until S8.
    #[test]
    fn resolve_appearance_neutral_fallback_no_material_field() {
        let body = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "BodyNoMaterial".to_string(),
            version: 1,
            fields: PersistentMap::new(),
        }));
        let app = resolve_appearance(&body);
        match &app {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "Appearance", "fallback type_name must be Appearance");
            }
            other => panic!("expected Appearance StructureInstance, got {other:?}"),
        }
        let color_result =
            struct_field_unit(&app, "color").expect("neutral Appearance must have a `color` field");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_result, &mut diags);
        let n = clamp_round(0.7);
        assert_eq!(rgb, Rgb8 { r: n, g: n, b: n }, "neutral grey fallback");
        assert!(diags.is_empty(), "no diags expected, got: {diags:#?}");
    }

    /// Neutral fallback: body.material exists but has NO `appearance` field →
    /// resolve_appearance returns the hand-minted neutral Appearance.
    /// Fails until S8.
    #[test]
    fn resolve_appearance_neutral_fallback_no_appearance_field() {
        let material_no_app = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Material".to_string(),
            version: 1,
            fields: [("name".to_string(), Value::String("bare".to_string()))].into_iter().collect(),
        }));
        let body = body_with_material(material_no_app);

        let app = resolve_appearance(&body);
        match &app {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "Appearance", "fallback type_name must be Appearance");
            }
            other => panic!("expected Appearance StructureInstance, got {other:?}"),
        }
        let color_result =
            struct_field_unit(&app, "color").expect("neutral Appearance must have a `color` field");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_result, &mut diags);
        let n = clamp_round(0.7);
        assert_eq!(rgb, Rgb8 { r: n, g: n, b: n }, "neutral grey fallback");
        assert!(diags.is_empty(), "no diags expected, got: {diags:#?}");
    }

    // ── malformed-input fallbacks (intentional, silent — documented in resolve_color) ─

    /// A non-`StructureInstance` `color` (e.g. `Value::Int`) falls back silently to
    /// `Rgb8{0,0,0}` with no diagnostic.
    ///
    /// Rationale (intentional silent black): the reify type system guarantees that
    /// `Appearance.color` is always a fully-populated `Color` StructureInstance; this
    /// branch is only reachable from hand-crafted Values or eval-layer bugs.
    #[test]
    fn resolve_color_non_struct_value_returns_black() {
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&Value::Int(0), &mut diags);
        assert_eq!(
            result,
            Rgb8 { r: 0, g: 0, b: 0 },
            "non-struct color must fall back to Rgb8{{0,0,0}}"
        );
        assert!(
            diags.is_empty(),
            "no diagnostic expected for non-struct color, got: {diags:#?}"
        );
    }

    // ── B6: resolve_appearance_opt unit tests ────────────────────────────────

    /// (a) Body whose material carries an explicit Appearance → `Some(app)`.
    /// The returned Appearance StructureInstance has the correct color.
    /// Fails until S1 (β) introduces `resolve_appearance_opt`.
    #[test]
    fn resolve_appearance_opt_some_when_material_has_appearance() {
        // color 0.4/0.4/0.42 → Rgb8{102,102,107} via resolve_color
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app);
        let body = body_with_material(material);

        let result = resolve_appearance_opt(&body);
        let app_val = result.expect("body with material+appearance must yield Some");
        match &app_val {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "Appearance", "must be Appearance StructureInstance");
            }
            other => panic!("expected Appearance StructureInstance, got {other:?}"),
        }
        let color_val =
            struct_field_unit(&app_val, "color").expect("Appearance must have color field");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_val, &mut diags);
        assert_eq!(rgb, Rgb8 { r: 102, g: 102, b: 107 }, "color 0.4→102, 0.42→107");
        assert!(diags.is_empty(), "no diags, got: {diags:#?}");
    }

    /// (b) Non-StructureInstance body (`Value::Int(42)`) → `None`
    /// (not `Some(neutral_appearance())`).
    /// Fails until S1 (β) introduces `resolve_appearance_opt`.
    #[test]
    fn resolve_appearance_opt_none_for_non_struct_body() {
        let body = Value::Int(42);
        let result = resolve_appearance_opt(&body);
        assert!(
            result.is_none(),
            "non-struct body must yield None, not neutral fallback; got {result:?}"
        );
    }

    /// (c) Body StructureInstance with NO `material` field → `None`.
    /// Fails until S1 (β) introduces `resolve_appearance_opt`.
    #[test]
    fn resolve_appearance_opt_none_no_material_field() {
        let body = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "BodyNoMaterial".to_string(),
            version: 1,
            fields: PersistentMap::new(),
        }));
        let result = resolve_appearance_opt(&body);
        assert!(result.is_none(), "body with no material field must yield None; got {result:?}");
    }

    // ── S1: coating_appearance RED tests ──────────────────────────────────────

    /// Build a `Coating` `Value::StructureInstance` for test inputs.
    /// Mirrors the `Coating` struct from `stdlib/surface_finish.ri`:
    /// `structure def Coating { process: CoatingProcess = Uncoated; color: Color = Color(); … }`.
    /// Only `process` and `color` are read by the β seam; the extra fields are defaulted.
    fn coating(process_variant: &str, color_val: Value) -> Value {
        let fields: PersistentMap<String, Value> = [
            ("process".to_string(), Value::enum_unit("CoatingProcess", process_variant)),
            ("color".to_string(), color_val),
            // Defaulted fields not read by the seam:
            ("thickness".to_string(), Value::Real(0.0)),
            ("spec".to_string(), Value::String(String::new())),
            ("process_cost".to_string(), Value::Real(0.0)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Coating".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Uncoated process → `coating_appearance` returns `None` (inert sentinel; back-compat B5).
    #[test]
    fn coating_appearance_uncoated_returns_none() {
        let c = coating("Uncoated", color("", 0.0, 0.0, 0.0));
        let result = coating_appearance(&c);
        assert!(
            result.is_none(),
            "Uncoated coating must yield None (inert sentinel), got {result:?}"
        );
    }

    /// Anodize with RAL9005 (jet black) → `Some(Appearance)`.
    /// Checks: type_name "Appearance"; color resolves to `Rgb8{14,14,16}` (RAL9005 seed);
    /// metalness == 0.0 (dielectric); finish ∈ {"Matte","Satin"}; roughness ∈ [0.0, 1.0].
    #[test]
    fn coating_appearance_anodize_ral9005_returns_some() {
        let c = coating("Anodize", color("RAL9005", 0.0, 0.0, 0.0));
        let app = coating_appearance(&c).expect("Anodize must yield Some(Appearance)");

        // Must be an Appearance StructureInstance.
        match &app {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "Appearance", "type_name must be Appearance");
            }
            other => panic!("expected Appearance StructureInstance, got {other:?}"),
        }

        // color field must exist and resolve via resolve_color to RAL9005 exact bytes.
        let color_field = struct_field_unit(&app, "color")
            .expect("Appearance must have a color field");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 14, g: 14, b: 16 },
            "Anodize+RAL9005 color must resolve to {{14,14,16}} (RAL_SEED entry)"
        );
        assert!(diags.is_empty(), "no color-name diagnostics expected, got: {diags:#?}");

        // metalness == 0.0 (dielectric — Anodize is an oxide coating, not metallic).
        let metalness = struct_field_unit(&app, "metalness")
            .expect("Appearance must have metalness field");
        match metalness {
            Value::Real(m) => assert_eq!(m, 0.0, "Anodize must be dielectric: metalness 0.0"),
            other => panic!("expected Real metalness, got {other:?}"),
        }

        // finish variant ∈ {"Matte", "Satin"}.
        let finish = struct_field_unit(&app, "finish")
            .expect("Appearance must have finish field");
        match &finish {
            Value::Enum { variant, .. } => {
                assert!(
                    variant == "Matte" || variant == "Satin",
                    "Anodize finish must be Matte or Satin, got '{variant}'"
                );
            }
            other => panic!("expected Finish Enum, got {other:?}"),
        }

        // roughness ∈ [0.0, 1.0].
        let roughness = struct_field_unit(&app, "roughness")
            .expect("Appearance must have roughness field");
        match roughness {
            Value::Real(r) => {
                assert!(
                    (0.0..=1.0).contains(&r),
                    "roughness must be in [0.0, 1.0], got {r}"
                );
            }
            other => panic!("expected Real roughness, got {other:?}"),
        }
    }

    // ── S3: full editorial projection table + never-silent-black ─────────────

    /// Electroplate with DEFAULT color (all-zero, not meaningful) →
    /// characteristic light metallic substituted; high metalness; low roughness.
    /// RED until S4 implements the Electroplate arm.
    #[test]
    fn coating_appearance_electroplate_default_metallic() {
        let c = coating("Electroplate", color("", 0.0, 0.0, 0.0));
        let app = coating_appearance(&c).expect("Electroplate must yield Some(Appearance)");

        // metalness >= 0.7 (high-metalness metallic).
        let metalness = struct_field_unit(&app, "metalness")
            .expect("Appearance must have metalness");
        match metalness {
            Value::Real(m) => assert!(
                m >= 0.7,
                "Electroplate metalness must be >= 0.7 (metallic), got {m}"
            ),
            other => panic!("expected Real metalness, got {other:?}"),
        }

        // roughness <= 0.3 (polished/low roughness).
        let roughness = struct_field_unit(&app, "roughness")
            .expect("Appearance must have roughness");
        match roughness {
            Value::Real(r) => assert!(
                r <= 0.3,
                "Electroplate roughness must be <= 0.3 (polished), got {r}"
            ),
            other => panic!("expected Real roughness, got {other:?}"),
        }

        // Color must be light (each channel >= 150) — never-silent-black + characteristic.
        let color_field = struct_field_unit(&app, "color")
            .expect("Appearance must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert!(
            rgb.r >= 150 && rgb.g >= 150 && rgb.b >= 150,
            "Electroplate default color must be light (each >= 150), got {rgb:?}"
        );
    }

    /// PowderCoat with explicit hex color → color passes through; dielectric; Satin or Gloss.
    #[test]
    fn coating_appearance_powdercoat_color_passthrough() {
        let c = coating("PowderCoat", color("#3366CC", 0.0, 0.0, 0.0));
        let app = coating_appearance(&c).expect("PowderCoat must yield Some(Appearance)");

        // color == #3366CC exact.
        let color_field = struct_field_unit(&app, "color")
            .expect("Appearance must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 0x33, g: 0x66, b: 0xCC },
            "PowderCoat must pass coating color through exactly"
        );
        assert!(diags.is_empty(), "no diags expected, got: {diags:#?}");

        // metalness == 0.0 (dielectric).
        let metalness = struct_field_unit(&app, "metalness")
            .expect("Appearance must have metalness");
        match metalness {
            Value::Real(m) => assert_eq!(m, 0.0, "PowderCoat must be dielectric: metalness 0.0"),
            other => panic!("expected Real metalness, got {other:?}"),
        }

        // finish ∈ {"Satin","Gloss"}.
        let finish = struct_field_unit(&app, "finish")
            .expect("Appearance must have finish");
        match &finish {
            Value::Enum { variant, .. } => assert!(
                variant == "Satin" || variant == "Gloss",
                "PowderCoat finish must be Satin or Gloss, got '{variant}'"
            ),
            other => panic!("expected Finish Enum, got {other:?}"),
        }
    }

    /// Paint with explicit hex color → same contract as PowderCoat (color pass-through,
    /// dielectric, Satin or Gloss).
    #[test]
    fn coating_appearance_paint_color_passthrough() {
        let c = coating("Paint", color("#FF4400", 0.0, 0.0, 0.0));
        let app = coating_appearance(&c).expect("Paint must yield Some(Appearance)");

        // color == #FF4400 exact.
        let color_field = struct_field_unit(&app, "color")
            .expect("Appearance must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 0xFF, g: 0x44, b: 0x00 },
            "Paint must pass coating color through exactly"
        );

        // metalness == 0.0 (dielectric).
        let metalness = struct_field_unit(&app, "metalness")
            .expect("Appearance must have metalness");
        match metalness {
            Value::Real(m) => assert_eq!(m, 0.0, "Paint must be dielectric: metalness 0.0"),
            other => panic!("expected Real metalness, got {other:?}"),
        }

        // finish ∈ {"Satin","Gloss"}.
        let finish = struct_field_unit(&app, "finish")
            .expect("Appearance must have finish");
        match &finish {
            Value::Enum { variant, .. } => assert!(
                variant == "Satin" || variant == "Gloss",
                "Paint finish must be Satin or Gloss, got '{variant}'"
            ),
            other => panic!("expected Finish Enum, got {other:?}"),
        }
    }

    /// Passivate → near-substrate subtle: metalness modest (<= 0.5), color non-black.
    #[test]
    fn coating_appearance_passivate_subtle() {
        let c = coating("Passivate", color("", 0.0, 0.0, 0.0));
        let app = coating_appearance(&c).expect("Passivate must yield Some(Appearance)");

        // metalness modest (<= 0.5).
        let metalness = struct_field_unit(&app, "metalness")
            .expect("Appearance must have metalness");
        match metalness {
            Value::Real(m) => {
                assert!(m <= 0.5, "Passivate metalness must be <= 0.5 (modest), got {m}");
            }
            other => panic!("expected Real metalness, got {other:?}"),
        }

        // color non-black (at least one channel > 0).
        let color_field = struct_field_unit(&app, "color")
            .expect("Appearance must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert!(
            rgb.r > 0 || rgb.g > 0 || rgb.b > 0,
            "Passivate default color must be non-black (never-silent-black), got {rgb:?}"
        );
    }

    // ── S5: finish_modulation RED tests ──────────────────────────────────────

    /// Base appearance used by all S5 tests: color(0.4,0.4,0.42)→Rgb8{102,102,107},
    /// finish Satin, metalness 0.0, roughness 0.5.

    /// Polished → Gloss finish, roughness <= 0.2 (high sheen), color+metalness preserved.
    /// RED until S6 implements `finish_modulation`.
    #[test]
    fn finish_modulation_polished_gloss_low_roughness() {
        let base = appearance_val(0.4, 0.4, 0.42);
        let fp = Value::enum_unit("FinishProcess", "Polished");
        let result = finish_modulation(&fp, &base);

        // finish == "Gloss".
        let finish = struct_field_unit(&result, "finish").expect("must have finish");
        match &finish {
            Value::Enum { variant, .. } => {
                assert_eq!(variant, "Gloss", "Polished → Gloss finish")
            }
            other => panic!("expected Finish Enum, got {other:?}"),
        }

        // roughness <= 0.2 (high sheen = low roughness).
        let roughness = struct_field_unit(&result, "roughness").expect("must have roughness");
        match roughness {
            Value::Real(r) => {
                assert!(r <= 0.2, "Polished roughness must be <= 0.2, got {r}")
            }
            other => panic!("expected Real roughness, got {other:?}"),
        }

        // color preserved: still {102,102,107}.
        let color_field = struct_field_unit(&result, "color").expect("must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(rgb, Rgb8 { r: 102, g: 102, b: 107 }, "color must be preserved by Polished");
        assert!(diags.is_empty(), "no diags expected, got: {diags:#?}");

        // metalness preserved: 0.0.
        let metalness = struct_field_unit(&result, "metalness").expect("must have metalness");
        match metalness {
            Value::Real(m) => assert_eq!(m, 0.0, "metalness must be preserved by Polished"),
            other => panic!("expected Real metalness, got {other:?}"),
        }
    }

    /// Ground → Satin finish, color+metalness preserved, roughness mid-range.
    /// RED until S6.
    #[test]
    fn finish_modulation_ground_satin_mid_roughness() {
        let base = appearance_val(0.4, 0.4, 0.42);
        let fp = Value::enum_unit("FinishProcess", "Ground");
        let result = finish_modulation(&fp, &base);

        // finish == "Satin".
        let finish = struct_field_unit(&result, "finish").expect("must have finish");
        match &finish {
            Value::Enum { variant, .. } => {
                assert_eq!(variant, "Satin", "Ground → Satin finish")
            }
            other => panic!("expected Finish Enum, got {other:?}"),
        }

        // color preserved.
        let color_field = struct_field_unit(&result, "color").expect("must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(rgb, Rgb8 { r: 102, g: 102, b: 107 }, "color must be preserved by Ground");

        // metalness preserved.
        let metalness = struct_field_unit(&result, "metalness").expect("must have metalness");
        match metalness {
            Value::Real(m) => assert_eq!(m, 0.0, "metalness must be preserved by Ground"),
            other => panic!("expected Real metalness, got {other:?}"),
        }
    }

    /// BeadBlasted → Matte finish, roughness >= 0.7, color preserved.
    /// RED until S6.
    #[test]
    fn finish_modulation_bead_blasted_matte_high_roughness() {
        let base = appearance_val(0.4, 0.4, 0.42);
        let fp = Value::enum_unit("FinishProcess", "BeadBlasted");
        let result = finish_modulation(&fp, &base);

        // finish == "Matte".
        let finish = struct_field_unit(&result, "finish").expect("must have finish");
        match &finish {
            Value::Enum { variant, .. } => {
                assert_eq!(variant, "Matte", "BeadBlasted → Matte finish")
            }
            other => panic!("expected Finish Enum, got {other:?}"),
        }

        // roughness >= 0.7 (high roughness).
        let roughness = struct_field_unit(&result, "roughness").expect("must have roughness");
        match roughness {
            Value::Real(r) => {
                assert!(r >= 0.7, "BeadBlasted roughness must be >= 0.7, got {r}")
            }
            other => panic!("expected Real roughness, got {other:?}"),
        }

        // color preserved.
        let color_field = struct_field_unit(&result, "color").expect("must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 102, g: 102, b: 107 },
            "color must be preserved by BeadBlasted"
        );
    }

    /// AsCast → Matte finish, roughness >= 0.7 (same class as BeadBlasted).
    /// RED until S6.
    #[test]
    fn finish_modulation_as_cast_matte_high_roughness() {
        let base = appearance_val(0.4, 0.4, 0.42);
        let fp = Value::enum_unit("FinishProcess", "AsCast");
        let result = finish_modulation(&fp, &base);

        // finish == "Matte".
        let finish = struct_field_unit(&result, "finish").expect("must have finish");
        match &finish {
            Value::Enum { variant, .. } => {
                assert_eq!(variant, "Matte", "AsCast → Matte finish")
            }
            other => panic!("expected Finish Enum, got {other:?}"),
        }

        // roughness >= 0.7.
        let roughness = struct_field_unit(&result, "roughness").expect("must have roughness");
        match roughness {
            Value::Real(r) => {
                assert!(r >= 0.7, "AsCast roughness must be >= 0.7, got {r}")
            }
            other => panic!("expected Real roughness, got {other:?}"),
        }
    }

    /// AsMachined → identity: returned Appearance is `PartialEq`-equal to the base.
    /// RED until S6.
    #[test]
    fn finish_modulation_as_machined_identity() {
        let base = appearance_val(0.4, 0.4, 0.42);
        let fp = Value::enum_unit("FinishProcess", "AsMachined");
        let result = finish_modulation(&fp, &base);
        assert_eq!(
            result,
            base,
            "AsMachined is the inert sentinel: returned Appearance must equal the base"
        );
    }

    /// (d) Body whose `material` has NO `appearance` field → `None`.
    /// Fails until S1 (β) introduces `resolve_appearance_opt`.
    #[test]
    fn resolve_appearance_opt_none_material_without_appearance() {
        let material_no_app = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Material".to_string(),
            version: 1,
            fields: [("name".to_string(), Value::String("bare".to_string()))].into_iter().collect(),
        }));
        let body = body_with_material(material_no_app);
        let result = resolve_appearance_opt(&body);
        assert!(
            result.is_none(),
            "body with material but no appearance field must yield None; got {result:?}"
        );
    }

    /// A `Color` StructureInstance with missing r/g/b fields falls back silently to
    /// `Rgb8{0,0,0}` with no diagnostic (missing fields → `unwrap_or(0.0)` → `clamp_round(0.0) = 0`).
    ///
    /// Rationale (intentional silent black): same as `resolve_color_non_struct_value_returns_black`.
    #[test]
    fn resolve_color_missing_rgb_fields_returns_black() {
        // A Color struct with only `named=""` — no r, g, or b fields present.
        let fields: PersistentMap<String, Value> =
            [("named".to_string(), Value::String(String::new()))].into_iter().collect();
        let color_only_named = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "Color".to_string(),
            version: 1,
            fields,
        }));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let result = resolve_color(&color_only_named, &mut diags);
        assert_eq!(
            result,
            Rgb8 { r: 0, g: 0, b: 0 },
            "missing r/g/b fields must fall back to Rgb8{{0,0,0}}"
        );
        assert!(
            diags.is_empty(),
            "no diagnostic expected for missing rgb fields, got: {diags:#?}"
        );
    }

    // ── S7: resolve_appearance_opt §7.3 precedence + back-compat guards ───────

    /// Build a synthetic body with optional material, coating, and finish_process fields.
    /// Used for S7 §7.3 functional-precedence tests.
    fn body_with_surface(
        material: Option<Value>,
        coating_val: Option<Value>,
        finish_process_variant: Option<&str>,
    ) -> Value {
        let mut fields_vec: Vec<(String, Value)> = Vec::new();
        if let Some(m) = material {
            fields_vec.push(("material".to_string(), m));
        }
        if let Some(c) = coating_val {
            fields_vec.push(("coating".to_string(), c));
        }
        if let Some(fp) = finish_process_variant {
            fields_vec.push((
                "finish_process".to_string(),
                Value::enum_unit("FinishProcess", fp),
            ));
        }
        let fields: PersistentMap<String, Value> = fields_vec.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: TEST_TYPE_ID,
            type_name: "SyntheticBody".to_string(),
            version: 1,
            fields,
        }))
    }

    /// (a) Coating overrides material color: body{material(0.4,0.4,0.42), Anodize+RAL9005}.
    /// resolve_appearance color must be {14,14,16} (coating), not {102,102,107} (material).
    /// RED until S8 adds the §7.3 functional layer.
    #[test]
    fn resolve_appearance_coating_overrides_material() {
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app);
        let body = body_with_surface(
            Some(material),
            Some(coating("Anodize", color("RAL9005", 0.0, 0.0, 0.0))),
            None,
        );
        let result = resolve_appearance(&body);
        let color_field = struct_field_unit(&result, "color").expect("Appearance must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 14, g: 14, b: 16 },
            "Anodize+RAL9005 must override material color; expected {{14,14,16}}, got {rgb:?}"
        );
    }

    /// (b) Finish modulation: body{material(0.4,0.4,0.42), Polished, no coating}.
    /// Color preserved {102,102,107}, finish "Gloss", roughness <= 0.2.
    /// RED until S8.
    #[test]
    fn resolve_appearance_finish_modulates_material() {
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app);
        let body = body_with_surface(Some(material), None, Some("Polished"));

        let result = resolve_appearance(&body);

        // color preserved: {102,102,107}.
        let color_field = struct_field_unit(&result, "color").expect("must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(rgb, Rgb8 { r: 102, g: 102, b: 107 }, "color must be preserved by Polished");

        // finish == "Gloss".
        let finish = struct_field_unit(&result, "finish").expect("must have finish");
        match &finish {
            Value::Enum { variant, .. } => {
                assert_eq!(variant, "Gloss", "Polished → Gloss finish via finish_modulation")
            }
            other => panic!("expected Finish Enum, got {other:?}"),
        }

        // roughness <= 0.2 (high sheen).
        let roughness = struct_field_unit(&result, "roughness").expect("must have roughness");
        match roughness {
            Value::Real(r) => assert!(r <= 0.2, "Polished roughness must be <= 0.2, got {r}"),
            other => panic!("expected Real roughness, got {other:?}"),
        }
    }

    /// (c) Coating beats finish: body{material, Anodize+RAL9005, Polished} → coating-derived dark.
    /// RED until S8.
    #[test]
    fn resolve_appearance_coating_beats_finish() {
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app);
        let body = body_with_surface(
            Some(material),
            Some(coating("Anodize", color("RAL9005", 0.0, 0.0, 0.0))),
            Some("Polished"),
        );
        let result = resolve_appearance(&body);
        let color_field = struct_field_unit(&result, "color").expect("must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 14, g: 14, b: 16 },
            "Coating must win over finish_process; expected {{14,14,16}} (RAL9005), got {rgb:?}"
        );
    }

    /// (d) Coating without material → `resolve_appearance_opt` is `Some`.
    /// Coating is a producer even without a material (§7.3).
    /// RED until S8.
    #[test]
    fn resolve_appearance_opt_coating_without_material_is_some() {
        let body = body_with_surface(
            None, // no material
            Some(coating("Anodize", color("RAL9005", 0.0, 0.0, 0.0))),
            None,
        );
        let result = resolve_appearance_opt(&body);
        assert!(
            result.is_some(),
            "Coating without material must yield Some (coating is a producer); got {result:?}"
        );
        let app = result.unwrap();
        let color_field = struct_field_unit(&app, "color").expect("must have color");
        let mut diags: Vec<Diagnostic> = Vec::new();
        let rgb = resolve_color(&color_field, &mut diags);
        assert_eq!(
            rgb,
            Rgb8 { r: 14, g: 14, b: 16 },
            "coating-only appearance must resolve to RAL9005 {{14,14,16}}"
        );
    }

    // Back-compat guards (already green, locked here):

    /// (e) Inert sentinels: body{material(0.4,0.4,0.42), Uncoated, AsMachined} →
    /// resolve_appearance equals the bare material appearance (B5 back-compat).
    #[test]
    fn resolve_appearance_uncoated_as_machined_preserves_material() {
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app.clone());
        let body = body_with_surface(
            Some(material),
            Some(coating("Uncoated", color("", 0.0, 0.0, 0.0))),
            Some("AsMachined"),
        );
        let result = resolve_appearance(&body);
        assert_eq!(
            result,
            app,
            "Uncoated+AsMachined must resolve to the bare material appearance (B5 back-compat)"
        );
    }

    /// (f) Uncoated+AsMachined+no material → `resolve_appearance_opt` is `None`.
    #[test]
    fn resolve_appearance_opt_inert_sentinels_no_material_is_none() {
        let body = body_with_surface(
            None,
            Some(coating("Uncoated", color("", 0.0, 0.0, 0.0))),
            Some("AsMachined"),
        );
        let result = resolve_appearance_opt(&body);
        assert!(
            result.is_none(),
            "Uncoated+AsMachined+no material must yield None (honest hash fallback); got {result:?}"
        );
    }

    /// (g) Body with only a material field → unchanged from pre-β behavior.
    #[test]
    fn resolve_appearance_material_only_unchanged() {
        let app = appearance_val(0.4, 0.4, 0.42);
        let material = material_with_appearance(app.clone());
        let body = body_with_surface(Some(material), None, None);
        let result = resolve_appearance(&body);
        assert_eq!(
            result,
            app,
            "material-only body must resolve to the material's appearance (unchanged from pre-β)"
        );
    }
}
