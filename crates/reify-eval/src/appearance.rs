//! Appearance resolution seam: `resolve_color` and `resolve_appearance`.
//!
//! Consumed by task δ (3MF color egress, `engine_build.rs`) and cross-PRD
//! PRD-2 (viewport recolor).  Both functions are `pub`; downstream callers
//! compose them as `resolve_color(&resolve_appearance(body).fields["color"], diags)`.
//!
//! PRD: `docs/prds/v0_6/appearance-substrate.md` §4.2/§7.3 (task β, #4761).

// Implementation arrives in step S8 (resolve_appearance).

use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{Rgb8, Value};

// ── private helpers ───────────────────────────────────────────────────────────

/// Extract an `f64` from a numeric Value cell (Int / Real / Scalar).
/// Mirrors `dynamics_ops::cell_f64`. Non-numeric → `None`.
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

// ── public seam ───────────────────────────────────────────────────────────────

/// Resolve a `Color` `Value::StructureInstance` to an sRGB byte triple.
///
/// Resolution ladder (PRD §4.2/§7.3):
/// 1. `named == ""` → `clamp_round(r, g, b)`.
/// 2. `named` starts with `#` and is exactly `#RRGGBB` or `#RGB` → parse EXACTLY
///    (no clamp/round; `#RGB` nibble-doubles each nibble).
/// 3. `named` is a seeded RAL Classic name (e.g. `"RAL7035"`) → tabled sRGB value.
///    (RAL table added in S6.)
/// 4. Any other non-empty `named` (including malformed `#…`) → push
///    `W_UNKNOWN_COLOR_NAME` warning and fall back to `clamp_round(r, g, b)`.
///
/// This function is TOTAL: it always returns an `Rgb8`, using the clamp fallback
/// on unrecognised names so downstream callers never see a silent black default.
///
/// The RAL seed table (step S6) is not included yet; path (4) handles any
/// currently-unrecognised name via the fallback until S6 extends this.
pub fn resolve_color(color: &Value, diagnostics: &mut Vec<Diagnostic>) -> Rgb8 {
    // Extract named / r / g / b from the Color StructureInstance.
    // On any missing or wrong-type field use the safe defaults (empty named, 0.0 rgb).
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

    // Path 3 (RAL table) is added in S6.
    // For now all non-empty, non-valid-hex names fall through to path 4.

    // Path 4: unknown name → warn + clamp fallback.
    diagnostics.push(
        Diagnostic::warning(format!(
            "resolve_color: unknown color name '{named}'; falling back to (r,g,b)"
        ))
        .with_code(DiagnosticCode::UnknownColorName),
    );
    Rgb8 { r: clamp_round(r), g: clamp_round(g), b: clamp_round(b) }
}

#[cfg(test)]
mod tests {
    use reify_core::Diagnostic;
    use reify_ir::{PersistentMap, Rgb8, StructureInstanceData, StructureTypeId, Value};

    use super::{resolve_color};

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
}
