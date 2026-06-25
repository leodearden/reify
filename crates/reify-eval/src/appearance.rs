//! Appearance resolution seam: `resolve_color` and `resolve_appearance`.
//!
//! Consumed by task δ (3MF color egress, `engine_build.rs`) and cross-PRD
//! PRD-2 (viewport recolor).  Both functions are `pub`; downstream callers
//! compose them as `resolve_color(&resolve_appearance(body).fields["color"], diags)`.
//!
//! PRD: `docs/prds/v0_6/appearance-substrate.md` §4.2/§7.3 (task β, #4761).

// Implementation arrives in steps S4 (resolve_color) and S8 (resolve_appearance).

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
