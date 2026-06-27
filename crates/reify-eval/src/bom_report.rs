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

                // Classify the sub's *type* into at most one lifecycle bucket.
                // The io lattice makes Buy / Discard / Input mutually exclusive;
                // the `else if` dispatch below keeps a single sub from
                // double-counting even for an exotic user lattice.
                let is_buy = conforms_to_trait(bounds, &merged_trait_defs, "Buy");
                let is_discard = conforms_to_trait(bounds, &merged_trait_defs, "Discard");
                let is_input = conforms_to_trait(bounds, &merged_trait_defs, "Input");

                // Collection subs (`sub items : List<T>`) do NOT elaborate to a
                // single `StructureInstance` at `ValueCellId(owner, sub)` — their
                // members live at indexed scoped cells. The per-instance read
                // below would therefore silently drop a *list* of lifecycle
                // items, under-counting the BOM with zero diagnostics. Surface a
                // warning naming the skipped sub (collection line items are a v1
                // limitation) so the under-count is visible. Non-lifecycle
                // collections stay silent — they are not BOM items.
                if sub.is_collection {
                    if is_buy || is_discard || is_input {
                        report.warnings.push(format!(
                            "collection sub {}.{} (List<{}>) is not rolled up into \
                             the BOM — collection line items are a v1 limitation",
                            template.name, sub.name, sub.structure_name
                        ));
                    }
                    continue;
                }

                // Read the elaborated StructureInstance at ValueCellId(owner, sub).
                // A non-collection non-instance cell (absent / not a structure)
                // is not a BOM item and is skipped.
                let instance_id = ValueCellId::new(&template.name, &sub.name);
                let Some(Value::StructureInstance(data)) = values.get(&instance_id) else {
                    continue;
                };
                let fields = &data.fields;

                // Dispatch into the matching lifecycle bucket (Buy > Discard >
                // Input priority via the `else if` chain).
                if is_buy {
                    let unit_cost = fields.get("unit_cost").and_then(money_si);
                    // Prefer the materialized `line_cost` (Costed); else fall
                    // back to `unit_cost` (a plain Buy, quantity 1). `None` when
                    // both are absent / `Undef` / non-MONEY — the line is then
                    // undetermined: excluded from the total and flagged.
                    let line_total = fields.get("line_cost").and_then(money_si).or(unit_cost);
                    let supplier = string_field(fields, "supplier").unwrap_or_default();
                    let part_number = string_field(fields, "part_number").unwrap_or_default();
                    let undetermined = line_total.is_none();
                    if undetermined {
                        // A Buy line with no determined cost is dropped from the
                        // grand total; surface a warning so a partially-`auto` /
                        // `undef` BOM is not silently under-counted. Name it by
                        // part_number when present, else by its sub field.
                        let label = if part_number.is_empty() {
                            format!("{}.{}", template.name, sub.name)
                        } else {
                            format!("{}.{} ({})", template.name, sub.name, part_number)
                        };
                        report.warnings.push(format!(
                            "BOM line {label} has an undetermined cost — excluded from the total"
                        ));
                    }
                    report.lines.push(BomLine {
                        entity: template.name.clone(),
                        sub: sub.name.clone(),
                        type_name: data.type_name.clone(),
                        supplier,
                        part_number,
                        unit_cost,
                        quantity: fields.get("quantity_produced").and_then(value_as_f64),
                        line_total,
                        undetermined,
                    });
                } else if is_discard {
                    report.waste.push(WasteEntry {
                        entity: template.name.clone(),
                        sub: sub.name.clone(),
                        type_name: data.type_name.clone(),
                        reason: enum_variant(fields, "reason").unwrap_or_default(),
                        disposal_method: enum_variant(fields, "disposal_method")
                            .unwrap_or_default(),
                    });
                } else if is_input {
                    // The nested `provenance : Provenance` struct carries the
                    // audit trail; tolerate it being absent / Undef (a custom
                    // Input that omits provenance still yields a row).
                    let prov_fields = match fields.get("provenance") {
                        Some(Value::StructureInstance(p)) => Some(&p.fields),
                        _ => None,
                    };
                    report.provenance.push(ProvenanceEntry {
                        entity: template.name.clone(),
                        sub: sub.name.clone(),
                        type_name: data.type_name.clone(),
                        source: string_field(fields, "source").unwrap_or_default(),
                        source_tool: prov_fields
                            .and_then(|pf| string_field(pf, "source_tool"))
                            .unwrap_or_default(),
                        timestamp: prov_fields
                            .and_then(|pf| string_field(pf, "timestamp"))
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

impl BomReport {
    /// `true` when the report has nothing worth rendering: no cost lines, no
    /// waste rows, no provenance rows, **and** no warnings.
    ///
    /// The CLI (`reify report --bom`) uses this to choose between a friendly
    /// "no BOM line items" message and a full [`Self::render`]. Warnings are
    /// part of the emptiness contract on purpose: a design whose only lifecycle
    /// item is a *collection* `Buy` sub rolls up to zero rows but a NON-empty
    /// [`Self::warnings`] (the un-rolled-up collection is a v1 limitation). Such
    /// a report is NOT renderable-empty — it must route through `render()`, the
    /// only sink for warnings, so the under-count stays visible.
    ///
    /// Keeping the predicate here (next to the fields it reads) means a future
    /// section added to `BomReport` updates the emptiness contract in one place,
    /// rather than silently diverging from a hand-rolled predicate at the CLI
    /// call site.
    pub fn is_renderable_empty(&self) -> bool {
        self.lines.is_empty()
            && self.waste.is_empty()
            && self.provenance.is_empty()
            && self.warnings.is_empty()
    }

    /// Render a human-readable text report with four sections: a Bill-of-
    /// Materials table (aligned columns: supplier / part number / unit cost /
    /// qty / line cost), a grand-total line, a Waste / Discard section, and a
    /// Provenance section; any [`Self::warnings`] are appended last.
    ///
    /// Money is shown as its USD magnitude to two decimals (USD factor 1.0);
    /// an undetermined cost cell renders as an em dash (`—`), and the grand
    /// total renders `(undetermined)` when no line had a determined cost.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;

        // Undetermined cost / quantity cells render as an em dash.
        const DASH: &str = "—";
        // Cost columns: currency-style two decimals (USD magnitude).
        let money = |v: Option<f64>| v.map_or_else(|| DASH.to_string(), |m| format!("{m:.2}"));
        // Quantity column: a dimensionless count, NOT money — render with the
        // f64 `Display` shortest form (trims trailing zeros: 10.0 → "10",
        // 2.5 → "2.5") so a whole count is not mis-shown as currency "10.00".
        let qty = |v: Option<f64>| v.map_or_else(|| DASH.to_string(), |q| format!("{q}"));

        let mut out = String::new();

        // ── Bill of Materials ────────────────────────────────────────────────
        out.push_str("Bill of Materials\n=================\n");
        if self.lines.is_empty() {
            out.push_str("(no line items)\n");
        } else {
            // Header + one row per line; column widths size to the widest cell
            // (char count, so the multi-byte em dash aligns as one column).
            let header = ["SUPPLIER", "PART NUMBER", "UNIT COST", "QTY", "LINE COST"];
            let mut rows: Vec<[String; 5]> = vec![header.map(str::to_string)];
            for l in &self.lines {
                rows.push([
                    l.supplier.clone(),
                    l.part_number.clone(),
                    money(l.unit_cost),
                    qty(l.quantity),
                    money(l.line_total),
                ]);
            }
            let mut width = [0usize; 5];
            for r in &rows {
                for (i, cell) in r.iter().enumerate() {
                    width[i] = width[i].max(cell.chars().count());
                }
            }
            for r in &rows {
                let mut line = String::new();
                for (i, cell) in r.iter().enumerate() {
                    line.push_str(cell);
                    // Pad every column but the last (2-space gutter).
                    if i + 1 < r.len() {
                        let pad = width[i] - cell.chars().count() + 2;
                        for _ in 0..pad {
                            line.push(' ');
                        }
                    }
                }
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }

        // Grand total (excludes undetermined lines; see `build_bom_report`).
        match self.total {
            Some(t) => {
                let _ = writeln!(out, "\nTotal: {t:.2} USD");
            }
            None => out.push_str("\nTotal: (undetermined)\n"),
        }

        // ── Waste / Discard ──────────────────────────────────────────────────
        out.push_str("\nWaste / Discard\n===============\n");
        if self.waste.is_empty() {
            out.push_str("(none)\n");
        } else {
            for w in &self.waste {
                let _ = writeln!(
                    out,
                    "{}.{} ({}): {} → {}",
                    w.entity, w.sub, w.type_name, w.reason, w.disposal_method
                );
            }
        }

        // ── Provenance ───────────────────────────────────────────────────────
        out.push_str("\nProvenance\n==========\n");
        if self.provenance.is_empty() {
            out.push_str("(none)\n");
        } else {
            for p in &self.provenance {
                // Append the importing tool when known, e.g. " (step-import)".
                let tool = if p.source_tool.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", p.source_tool)
                };
                let _ = writeln!(out, "{}.{}: {}{}", p.entity, p.sub, p.source, tool);
            }
        }

        // ── Warnings (e.g. an undetermined-cost line) ────────────────────────
        if !self.warnings.is_empty() {
            out.push_str("\nWarnings\n========\n");
            for warn in &self.warnings {
                let _ = writeln!(out, "- {warn}");
            }
        }

        out
    }
}
