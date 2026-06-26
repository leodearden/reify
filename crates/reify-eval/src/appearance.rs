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
            Value::Enum { type_name: "Finish".to_string(), variant: "Satin".to_string() },
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
    if let Value::StructureInstance(data) = body
        && let Some(Value::StructureInstance(material)) = data.fields.get("material")
        && let Some(app @ Value::StructureInstance(_)) = material.fields.get("appearance")
    {
        return app.clone();
    }
    neutral_appearance()
}

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticCode, Severity};
    use reify_ir::{PersistentMap, Rgb8, StructureInstanceData, StructureTypeId, Value};

    use super::{clamp_round, resolve_appearance, resolve_appearance_opt, resolve_color};

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
                Value::Enum { type_name: "Finish".to_string(), variant: "Satin".to_string() },
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
}
