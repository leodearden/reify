//! BOM / cost / waste / provenance rollup over the std.io lifecycle traits
//! (`reify report --bom`, io-lifecycle-bom-cost #4292).
//!
//! [`Engine::build_bom_report`] walks `module.templates × sub_components` in
//! declaration order — the same enumeration the io-export δ driver
//! ([`crate::engine_build::Engine::build_outputs_with_result`]) uses — but
//! filters each sub by **trait conformance only** (NOT `EntityKind::Occurrence`),
//! because canonical cost line items are `structure def`s conforming to
//! `Costed : Buy`, not occurrences. Each sub is classified into at most one
//! lifecycle bucket via [`crate::tolerance_combine::conforms_to_trait`]:
//!
//!   * `Buy`      → a [`BomLine`] (cost); its `line_cost` rolls into the total.
//!   * `Discard`  → a [`WasteEntry`].
//!   * `Input`    → a [`ProvenanceEntry`].
//!
//! Money is displayed as its SI/base-currency magnitude: USD has factor 1.0
//! (`units.ri`), so a `Value::Scalar { si_value, dimension == MONEY }` carries
//! the USD value directly (multi-currency display is deferred).

use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, Value, ValueMap};

use crate::Engine;
use crate::tolerance_combine::conforms_to_trait;

/// One purchased / costed line item in the Bill of Materials (a `Buy`-conforming
/// sub instance — e.g. via `Costed : Buy`).
#[derive(Debug, Clone)]
pub struct BomLine {
    /// Owning (parent) template where the sub is declared, e.g. `"Widget"`.
    pub entity: String,
    /// Sub field name, e.g. `"bolts"`.
    pub sub: String,
    /// Resolved line-item structure type, e.g. `"Bolt"`.
    pub type_name: String,
    /// `Buy.supplier` (empty when absent).
    pub supplier: String,
    /// `Buy.part_number` (empty when absent).
    pub part_number: String,
    /// `Buy.unit_cost` as its SI/USD magnitude; `None` when undetermined.
    pub unit_cost: Option<f64>,
    /// `Costed.quantity_produced`; `None` for a plain `Buy` (no quantity).
    pub quantity: Option<f64>,
    /// Per-line total — the materialized `Costed.line_cost` if present, else
    /// `unit_cost` (a plain `Buy`, quantity 1). `None` when undetermined.
    pub line_total: Option<f64>,
    /// `true` when the line has no determined cost — excluded from the grand
    /// total and flagged in [`BomReport::warnings`].
    pub undetermined: bool,
}

/// One `Discard : Sink` waste row.
#[derive(Debug, Clone)]
pub struct WasteEntry {
    /// Owning (parent) template where the sub is declared.
    pub entity: String,
    /// Sub field name.
    pub sub: String,
    /// Resolved discard structure type.
    pub type_name: String,
    /// `Discard.reason` enum variant (e.g. `"Offcut"`).
    pub reason: String,
    /// `Discard.disposal_method` enum variant (e.g. `"Recycle"`).
    pub disposal_method: String,
}

/// One `Input : Source` provenance row (e.g. an imported STEP file).
#[derive(Debug, Clone)]
pub struct ProvenanceEntry {
    /// Owning (parent) template where the sub is declared.
    pub entity: String,
    /// Sub field name.
    pub sub: String,
    /// Resolved input structure type.
    pub type_name: String,
    /// `Input.source` — the originating file path / identifier.
    pub source: String,
    /// `Input.provenance.source_tool` — the importing tool (empty when absent).
    pub source_tool: String,
    /// `Input.provenance.timestamp` (empty when absent).
    pub timestamp: String,
}

/// A rolled-up BOM / cost / waste / provenance report.
#[derive(Debug, Clone, Default)]
pub struct BomReport {
    /// Cost line items, in source declaration order.
    pub lines: Vec<BomLine>,
    /// Waste / discard rows, in source declaration order.
    pub waste: Vec<WasteEntry>,
    /// Provenance rows, in source declaration order.
    pub provenance: Vec<ProvenanceEntry>,
    /// Grand total of determined line totals (SI/USD magnitude). `None` when no
    /// line has a determined cost.
    pub total: Option<f64>,
    /// Human-readable warnings (e.g. a `Buy` line whose cost is undetermined).
    pub warnings: Vec<String>,
}

/// Read a `Money`-dimensioned scalar as its SI/USD magnitude. `None` for any
/// other value (absent, `Undef`, or a non-`MONEY` scalar).
fn money_si(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } if *dimension == DimensionVector::MONEY => Some(*si_value),
        _ => None,
    }
}

/// Read a numeric value as an `f64` — a dimensionless `Real`/`Int` or any
/// `Scalar`'s SI magnitude (e.g. `quantity_produced : Real`).
fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Real(r) => Some(*r),
        Value::Int(i) => Some(*i as f64),
        Value::Scalar { si_value, .. } => Some(*si_value),
        _ => None,
    }
}

/// Read a `String` field, or `None` when absent / not a string.
fn string_field(fields: &PersistentMap<String, Value>, name: &str) -> Option<String> {
    match fields.get(name) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Read an enum field's variant name (e.g. `DiscardReason.Offcut` → `"Offcut"`),
/// or `None` when absent / not an enum.
fn enum_variant(fields: &PersistentMap<String, Value>, name: &str) -> Option<String> {
    match fields.get(name) {
        Some(Value::Enum { variant, .. }) => Some(variant.clone()),
        _ => None,
    }
}

impl Engine {
    /// Roll up a [`BomReport`] from an evaluated module: classify every sub
    /// instance by lifecycle-trait conformance and read its cost / waste /
    /// provenance fields off the elaborated `Value::StructureInstance`.
    ///
    /// `values` is the [`crate::EvalResult::values`] map from a prior
    /// `engine.eval(module)` — the rollup is kernel-free (lifecycle cells
    /// populate under plain eval; no geometry realization is required).
    pub fn build_bom_report(&self, module: &CompiledModule, values: &ValueMap) -> BomReport {
        // Merge module + prelude trait defs so the std.io refinement lattice
        // (Costed:Buy:Source, Discard:Sink, Input:Source) is visible: user
        // modules carry empty `trait_defs`; the io traits live in the prelude.
        // Mirrors `build_outputs_with_result`'s merge.
        let mut merged_trait_defs: Vec<reify_compiler::CompiledTrait> = module.trait_defs.clone();
        for pm in self.prelude {
            merged_trait_defs.extend(pm.trait_defs.iter().cloned());
        }

        let mut report = BomReport::default();

        // Deterministic declaration-order walk over every sub of every template.
        for template in &module.templates {
            for sub in &template.sub_components {
                // Resolve the sub's own template (module first, then prelude).
                let Some(sub_template) = crate::engine_eval::find_template_with_prelude(
                    module,
                    self.prelude,
                    &sub.structure_name,
                ) else {
                    continue;
                };
                let bounds = &sub_template.trait_bounds;

                // Read the elaborated StructureInstance at ValueCellId(owner, sub).
                // Collection subs and non-instance cells are skipped here.
                let instance_id = ValueCellId::new(&template.name, &sub.name);
                let Some(Value::StructureInstance(data)) = values.get(&instance_id) else {
                    continue;
                };
                let fields = &data.fields;

                // Classify into at most one lifecycle bucket. The io lattice
                // makes Buy / Discard / Input mutually exclusive, but the
                // `else if` chain keeps a single sub from double-counting even
                // for an exotic user lattice.
                if conforms_to_trait(bounds, &merged_trait_defs, "Buy") {
                    let unit_cost = fields.get("unit_cost").and_then(money_si);
                    // Prefer the materialized `line_cost` (Costed); else fall
                    // back to `unit_cost` (a plain Buy, quantity 1).
                    let line_total = fields.get("line_cost").and_then(money_si).or(unit_cost);
                    report.lines.push(BomLine {
                        entity: template.name.clone(),
                        sub: sub.name.clone(),
                        type_name: data.type_name.clone(),
                        supplier: string_field(fields, "supplier").unwrap_or_default(),
                        part_number: string_field(fields, "part_number").unwrap_or_default(),
                        unit_cost,
                        quantity: fields.get("quantity_produced").and_then(value_as_f64),
                        line_total,
                        undetermined: line_total.is_none(),
                    });
                } else if conforms_to_trait(bounds, &merged_trait_defs, "Discard") {
                    report.waste.push(WasteEntry {
                        entity: template.name.clone(),
                        sub: sub.name.clone(),
                        type_name: data.type_name.clone(),
                        reason: enum_variant(fields, "reason").unwrap_or_default(),
                        disposal_method: enum_variant(fields, "disposal_method")
                            .unwrap_or_default(),
                    });
                }
            }
        }

        // Grand total over determined line totals (SI/USD magnitude). `None`
        // when no line has a determined cost.
        let mut determined = report.lines.iter().filter_map(|l| l.line_total).peekable();
        report.total = if determined.peek().is_some() {
            Some(determined.sum())
        } else {
            None
        };

        report
    }
}
