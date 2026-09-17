//! Minimal sync MCP HTTP client for the jcodemunch code-analysis service.
//!
//! Exposes [`RealJCodemunchOps`], the production implementation of the
//! [`crate::JCodemunchOps`] trait. The companion [`MockJCodemunchOps`]
//! (gated behind `feature = "test-support"`) lives in `lib.rs`.
//!
//! ## Wire protocol
//!
//! MCP streamable-HTTP, protocol version `2024-11-05`. Same SSE/JSON
//! dual-path and same `into_reader()` no-10 MiB cap discipline as
//! `fused_memory_client.rs`.
//!
//! ## Session lifecycle
//!
//! The session id is assigned by the **server**, never minted by the
//! client:
//!
//! 1. `initialize` is POSTed with **no** `mcp-session-id` request header.
//! 2. The id the server returns in that response's `Mcp-Session-Id`
//!    header is stored on the client.
//! 3. Every subsequent POST — starting with `notifications/initialized` —
//!    replays that stored id verbatim.
//!
//! A client-minted id is not merely redundant: a live jcodemunch serve
//! answers such an `initialize` with `404 Invalid or expired session ID`,
//! which [`post`](JcodemunchClient::post) maps to [`LoadError::Http`] and
//! `reify-audit` then fail-softs into a no-op detector — silently.
//!
//! `fused_memory_client.rs` still mints its own id. That divergence is
//! deliberate: fixing it is out of scope here (it works today against the
//! fused-memory server) and belongs to its own task.
//!
//! ## MUNCH/1 encoding
//!
//! Three jcodemunch tools (`get_changed_symbols`, `get_dead_code_v2`,
//! `get_untested_symbols`) return a custom columnar text encoding instead of
//! JSON inside `content[0].text`. [`munch_decode`] parses this into a
//! `serde_json::Value` object keyed by table name → array of row objects keyed
//! by column name, letting each adapter read fields by name (mirroring
//! `task_metadata_from_wire`'s `.get(field)` style).
//!
//! The two `get_layer_violations` variants return plain JSON payloads (not
//! MUNCH). [`decode_tool_result`] routes between the two formats by inspecting
//! the `#MUNCH/` prefix.

use std::cell::Cell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::{
    ChangedSymbol, DeadSymbol, DeclSuppression, JCodemunchOps, LayerViolation, SymbolReference,
    UntestedSymbol,
};

const PROTOCOL_VERSION: &str = "2024-11-05";
const CLIENT_NAME: &str = "reify-audit-jcodemunch";
const HTTP_TIMEOUT_SECS: u64 = 60;

// -----------------------------------------------------------------------
// LoadError
// -----------------------------------------------------------------------

/// Errors returned by [`JcodemunchClient`]. Variants map to fail-soft empty
/// results at the [`RealJCodemunchOps`] boundary (per design decision:
/// a down serve must not crash the sweep).
#[derive(Debug)]
pub enum LoadError {
    /// Transport-level failure: connection refused, timeout, non-2xx status,
    /// body read failure.
    Http(String),
    /// Protocol-level failure: malformed JSON-RPC envelope, missing expected
    /// fields, server-returned `error` payload, MUNCH decode failure.
    Protocol(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Http(m) => write!(f, "jcodemunch HTTP error: {m}"),
            LoadError::Protocol(m) => write!(f, "jcodemunch protocol error: {m}"),
        }
    }
}

impl std::error::Error for LoadError {}

// -----------------------------------------------------------------------
// MUNCH/1 decoder
// -----------------------------------------------------------------------

/// Decode a MUNCH/1 text payload into a `serde_json::Value` object.
///
/// The returned value is a JSON object keyed by table name; each value is a
/// JSON array of row objects keyed by column name. Types are coerced per the
/// `__tables` type list (int/float→Number, T/F→Bool, str→String).
///
/// Returns `Err(LoadError::Protocol(...))` if the header is missing, the
/// `__tables` declaration is absent, or a row cannot be parsed.
fn munch_decode(text: &str) -> Result<Value, LoadError> {
    // 1. Verify header
    let first_line = text.lines().next().unwrap_or("");
    if !first_line.starts_with("#MUNCH/") {
        return Err(LoadError::Protocol(format!(
            "munch_decode: expected #MUNCH/ header, got: {:?}",
            first_line.chars().take(40).collect::<String>()
        )));
    }

    // 2. Split into sections: ref-table lines, meta line, data rows
    let mut refs: HashMap<u32, String> = HashMap::new();
    let mut table_specs: Vec<TableSpec> = Vec::new();
    let mut result_obj: serde_json::Map<String, Value> = serde_json::Map::new();

    for line in text.lines().skip(1) {
        if line.starts_with('@') {
            // Ref table entry: @N=<literal>
            if let Some((n, val)) = parse_ref_entry(line) {
                refs.insert(n, val);
            }
        } else if line.contains("__tables=") {
            // Meta line — parse __tables declaration
            table_specs = parse_tables_decl(line).map_err(|e| {
                LoadError::Protocol(format!("munch_decode: __tables parse: {e}"))
            })?;
            // Pre-populate empty arrays for each table
            for spec in &table_specs {
                result_obj.insert(spec.table_name.clone(), Value::Array(Vec::new()));
            }
        } else if line.is_empty() || line.starts_with('#') {
            // Blank or comment lines — skip
            continue;
        } else {
            // Data row: find matching table spec by prefix
            let Some(comma_pos) = line.find(',') else {
                // No comma → no table prefix → silently skip malformed row
                continue;
            };
            let prefix = &line[..comma_pos];
            if let Some(spec) = table_specs.iter().find(|s| s.prefix == prefix) {
                let fields = split_munch_row(&line[comma_pos + 1..]);
                if fields.len() != spec.columns.len() {
                    return Err(LoadError::Protocol(format!(
                        "munch_decode: row has {} fields, expected {} for table {}; row={:?}",
                        fields.len(),
                        spec.columns.len(),
                        spec.table_name,
                        line.chars().take(80).collect::<String>()
                    )));
                }
                let mut row_obj = serde_json::Map::new();
                for (i, col) in spec.columns.iter().enumerate() {
                    let raw = &fields[i];
                    let expanded = expand_ref(raw, &refs);
                    let coerced = coerce_value(&expanded, &spec.col_types[i]);
                    row_obj.insert(col.clone(), coerced);
                }
                if let Some(Value::Array(arr)) = result_obj.get_mut(&spec.table_name) {
                    arr.push(Value::Object(row_obj));
                }
            }
            // Rows whose prefix matches no table spec are silently skipped
        }
    }

    // Require at least one table spec — a MUNCH payload with no __tables=
    // line is malformed (the caller cannot know what to do with unlabelled rows).
    if table_specs.is_empty() {
        return Err(LoadError::Protocol(
            "munch_decode: no __tables= declaration found in MUNCH payload".to_string(),
        ));
    }

    Ok(Value::Object(result_obj))
}

/// One parsed table spec from the `__tables=` declaration.
#[derive(Debug, Clone)]
struct TableSpec {
    prefix: String,
    table_name: String,
    columns: Vec<String>,
    col_types: Vec<ColType>,
}

#[derive(Debug, Clone, PartialEq)]
enum ColType {
    Str,
    Int,
    Float,
    Bool,
}

/// Parse a single `@N=<literal>` ref-table line. Returns `(N, literal)`.
fn parse_ref_entry(line: &str) -> Option<(u32, String)> {
    let rest = line.strip_prefix('@')?;
    let eq_pos = rest.find('=')?;
    let n: u32 = rest[..eq_pos].parse().ok()?;
    let val = rest[eq_pos + 1..].to_string();
    Some((n, val))
}

/// Extract and parse the `__tables=<spec>` declaration from the meta line.
/// Returns a list of [`TableSpec`] entries.
fn parse_tables_decl(meta_line: &str) -> Result<Vec<TableSpec>, String> {
    // Locate `__tables=` in the meta line
    let idx = meta_line
        .find("__tables=")
        .ok_or("__tables= not found in meta line")?;
    let after = &meta_line[idx + "__tables=".len()..];

    // The value may be `"..."`-wrapped (with `""` escaping inside)
    let raw_value = if let Some(inner) = after.strip_prefix('"') {
        // Extract quoted value: scan for the closing `"` (not `""`)
        let mut chars = inner.char_indices().peekable();
        let mut out = String::new();
        loop {
            match chars.next() {
                None => break,
                Some((_, '"')) => {
                    // Check if next char is also `"` (escaped quote)
                    if chars.peek().map(|(_, c)| *c) == Some('"') {
                        chars.next();
                        out.push('"');
                    } else {
                        break; // End of quoted value
                    }
                }
                Some((_, c)) => out.push(c),
            }
        }
        out
    } else {
        // Unquoted: value runs to end of line (no spaces in unquoted __tables)
        after.split_whitespace().next().unwrap_or(after).to_string()
    };

    // Split into individual table specs on `,` — BUT only at top level
    // (table specs don't contain unescaped commas themselves)
    let specs_str = raw_value;
    let mut specs = Vec::new();
    for spec_str in specs_str.split(',') {
        if spec_str.is_empty() {
            continue;
        }
        let spec = parse_one_table_spec(spec_str)?;
        specs.push(spec);
    }
    Ok(specs)
}

/// Parse a single `<prefix>:<table_name>:<col1>|<col2>|...[:<type1>|<type2>|...]`
/// table spec from the `__tables=` declaration.
///
/// Accepts two grammars:
/// - 4 segments `<prefix>:<table>:<col>|...:<type>|...` — the original
///   shape, captured by every fixture under `tests/fixtures/jcodemunch/`
///   (jcodemunch-mcp 1.108.27).
/// - 3 segments `<prefix>:<table>:<col>|...` — the type list omitted.
///   Measured live against jcodemunch-mcp 1.108.54's `find_references`
///   response on 2026-08-22 (`r:__rows__:file|specifier|match_type`).
///   Every column defaults to `ColType::Str` in this case.
///
/// This is a widening of the original 4-segment-only grammar, not a
/// migration away from it — both shapes must keep decoding.
///
/// `columns.len() == col_types.len()` is an INVARIANT of every `TableSpec`
/// this function returns: it is what keeps the `spec.col_types[i]` /
/// `fields[i]` indexing in [`munch_decode`] in bounds. The 3-segment path
/// therefore materialises a full `col_types` vector (`ColType` derives
/// `Clone`) rather than leaving the field optional.
fn parse_one_table_spec(spec: &str) -> Result<TableSpec, String> {
    // Format: `<prefix>:<table_name>:<col1>|<col2>|...[:<type1>|<type2>|...]`
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    if parts.len() != 3 && parts.len() != 4 {
        return Err(format!(
            "table spec has {} colon-segments (expected 3 or 4): {:?}",
            parts.len(),
            spec
        ));
    }
    let prefix = parts[0].to_string();
    let table_name = parts[1].to_string();
    let columns: Vec<String> = parts[2].split('|').map(|s| s.to_string()).collect();
    let col_types = if let Some(types_part) = parts.get(3) {
        let type_strs: Vec<&str> = types_part.split('|').collect();
        if columns.len() != type_strs.len() {
            return Err(format!(
                "table {} has {} columns but {} types",
                table_name,
                columns.len(),
                type_strs.len()
            ));
        }
        type_strs
            .iter()
            .map(|t| match *t {
                "int" => ColType::Int,
                "float" => ColType::Float,
                "bool" => ColType::Bool,
                _ => ColType::Str,
            })
            .collect()
    } else {
        // 3-segment spec: type list omitted, every column is ColType::Str.
        vec![ColType::Str; columns.len()]
    };
    Ok(TableSpec {
        prefix,
        table_name,
        columns,
        col_types,
    })
}

/// Split a MUNCH data row (everything after the leading `prefix,`) into
/// fields, respecting `"..."`-quoted values (commas inside quotes are
/// literal). Quote escaping inside: `""` → `"`.
fn split_munch_row(row: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = row.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                // Quoted field — consume until closing `"`
                loop {
                    match chars.next() {
                        None => break,
                        Some('"') => {
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                current.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(inner) => current.push(inner),
                    }
                }
            }
            ',' => {
                fields.push(current.clone());
                current.clear();
            }
            other => current.push(other),
        }
    }
    fields.push(current);
    fields
}

/// Expand `@N` references in an unquoted field value.
///
/// Handles:
/// - Standalone `@N` (entire field is the ref): returns the interned string.
/// - `@N<suffix>` (ref value prepended to a literal suffix): e.g. `@3foo`
///   where `@3=crates/` → `crates/foo`. N is the maximal leading digit run.
fn expand_ref(field: &str, refs: &HashMap<u32, String>) -> String {
    if !field.starts_with('@') {
        return field.to_string();
    }
    // Find the maximal digit run after `@`
    let digits_end = field[1..]
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i + 1)
        .unwrap_or(0);

    if digits_end == 0 {
        return field.to_string(); // Bare `@` with no digits — leave as-is
    }

    let n: u32 = match field[1..1 + digits_end].parse() {
        Ok(n) => n,
        Err(_) => return field.to_string(),
    };

    let expanded_prefix = match refs.get(&n) {
        Some(v) => v.as_str(),
        None => return field.to_string(), // Unknown ref — leave as-is
    };

    let suffix = &field[1 + digits_end..];
    format!("{}{}", expanded_prefix, suffix)
}

/// Coerce a string value to the specified column type.
fn coerce_value(s: &str, col_type: &ColType) -> Value {
    match col_type {
        ColType::Str => Value::String(s.to_string()),
        ColType::Int => s
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .unwrap_or(Value::String(s.to_string())),
        ColType::Float => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::String(s.to_string())),
        ColType::Bool => match s {
            "T" => Value::Bool(true),
            "F" => Value::Bool(false),
            _ => Value::String(s.to_string()),
        },
    }
}

// -----------------------------------------------------------------------
// decode_tool_result: MUNCH vs. JSON routing
// -----------------------------------------------------------------------

/// Decode a JSON-RPC `result` value into the inner payload.
///
/// MUNCH payloads in `content[0].text` are always decoded via [`munch_decode`],
/// even when `structuredContent` is also present — `structuredContent` (if any)
/// would not carry the table shape the adapters expect for MUNCH tools.
///
/// For non-MUNCH tools (plain JSON text or no text content), `structuredContent`
/// is preferred when present; otherwise `content[0].text` is parsed as JSON.
///
/// This resolves the non-uniform wire shape described in the fixtures README:
/// some tools return MUNCH, others return plain JSON.
fn decode_tool_result(result: &Value) -> Result<Value, LoadError> {
    // Check content[0].text first — MUNCH must always route through munch_decode.
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for entry in content {
            if entry.get("type").and_then(|t| t.as_str()) == Some("text")
                && let Some(text) = entry.get("text").and_then(|t| t.as_str())
            {
                if text.starts_with("#MUNCH/") {
                    return munch_decode(text);
                }
                // Non-MUNCH text found — fall through to structuredContent check.
                break;
            }
        }
    }

    // For non-MUNCH tools prefer structuredContent when present.
    if let Some(sc) = result.get("structuredContent")
        && !sc.is_null()
    {
        return Ok(sc.clone());
    }

    // Fall back to content[0].text as plain JSON.
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for entry in content {
            if entry.get("type").and_then(|t| t.as_str()) == Some("text")
                && let Some(text) = entry.get("text").and_then(|t| t.as_str())
            {
                return serde_json::from_str(text).map_err(|e| {
                    LoadError::Protocol(format!(
                        "decode_tool_result: content text not JSON: {e}; text={:?}",
                        text.chars().take(80).collect::<String>()
                    ))
                });
            }
        }
    }

    // Fall through: return result as-is
    Ok(result.clone())
}

// -----------------------------------------------------------------------
// Wire → struct adapters
// -----------------------------------------------------------------------

/// Read a `u64` field from a MUNCH row. Accepts a JSON number, an integral
/// non-negative float, and the string spelling of either; `None` when the
/// field is absent or is not a whole non-negative number.
///
/// The tolerance keeps a grammar drift costing a FIELD rather than a whole
/// record. A type-less `__tables` spec (see [`parse_one_table_spec`])
/// decodes every column as `ColType::Str`, and `coerce_value` gives a
/// `float`-typed column an f64-backed number that `serde_json`'s `as_u64()`
/// rejects however integral — so a strict read evaluates `None` inside a
/// `filter_map` adapter and the corpus silently empties.
///
/// The widening stays BOUNDED (see [`u64_from_integral_f64`]): nothing is
/// rounded or saturated into a plausible-looking line number. Callers
/// collapse `None` to the `0` "not reported" sentinel that
/// [`stale_decl_line_diagnostic`] reports as a grammar drift, so misreading
/// a PRESENT column as absent misdirects the operator as well as losing the
/// value.
fn row_u64(row: &Value, key: &str) -> Option<u64> {
    let v = row.get(key)?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if let Some(f) = v.as_f64() {
        // A JSON number that is not a `u64`: integral and in range, or nothing.
        return u64_from_integral_f64(f);
    }
    let s = v.as_str()?.trim();
    s.parse::<u64>()
        .ok()
        .or_else(|| s.parse::<f64>().ok().and_then(u64_from_integral_f64))
}

/// `Some(f as u64)` when `f` is a non-negative whole number representable as
/// a `u64`, `None` otherwise. Shared by [`row_u64`]'s JSON-number and
/// string-parse paths so the same test governs both spellings.
///
/// The upper bound is STRICT: `u64::MAX as f64` rounds UP to 2^64, which is
/// NOT a `u64`, so a `<=` bound would admit it and `f as u64` would saturate
/// back to `u64::MAX` — inventing exactly the plausible-looking line number
/// [`row_u64`]'s bounded widening refuses to invent. Nothing representable
/// is lost: the largest `f64` below 2^64 is 2^64 - 2048, well inside `u64`.
fn u64_from_integral_f64(f: f64) -> Option<u64> {
    (f.is_finite() && f.fract() == 0.0 && f >= 0.0 && f < u64::MAX as f64).then_some(f as u64)
}

/// Read an `f64` field from a MUNCH row — the float counterpart of
/// [`row_u64`], tolerating the same spellings for the same reason.
/// `confidence` is typed `float` in both captured fixtures, so a type-less
/// spec delivers it as a `Value::String`.
///
/// The pair covers every numeric column the adapters treat as MANDATORY.
/// `layer_violations_from_wire`'s `rule_index` keeps a bare `as_u64()`
/// deliberately: it is OPTIONAL, so a string there degrades the synthesized
/// `rule[..]` label instead of dropping the violation.
fn row_f64(row: &Value, key: &str) -> Option<f64> {
    let v = row.get(key)?;
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
}

/// Parse signals from a Python-list string like `"['a', 'b', 'c']"`.
///
/// Strips surrounding `[`/`]`, splits on `,`, trims whitespace and surrounding
/// `'` or `"` quotes from each element.
fn parse_signals_list(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|part| {
            let p = part.trim();
            // Strip surrounding single or double quotes
            let p = if (p.starts_with('\'') && p.ends_with('\''))
                || (p.starts_with('"') && p.ends_with('"'))
            {
                &p[1..p.len() - 1]
            } else {
                p
            };
            p.to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

// -----------------------------------------------------------------------
// dropped_rows_diagnostic helper
// -----------------------------------------------------------------------

/// Compare a decoded table's row count against the records an adapter
/// actually produced and summarise any SHORTFALL into ONE line, for the
/// caller to `eprintln!`.
///
/// Every wire adapter here is a `filter_map`, so a MANDATORY column a future
/// release renames or drops does not fail loudly — it evaluates `None` per
/// row and the corpus quietly becomes an empty `Vec`, which is worse than an
/// error: `get_dead_code_v2` without `confidence`, or `find_references`
/// without `file`, reads byte-for-byte as "this repo is clean" / "this
/// symbol has zero references", and P1/PDEAD/PUNTESTED report accordingly.
/// Dropping such rows stays correct; dropping them SILENTLY does not.
///
/// `None` — deliberately, these are NOT drops — when `table` is absent from
/// `decoded` or is not an array (there were never any rows), when the table
/// is EMPTY (a genuine zero answer, which the P1 producer-orphan case
/// depends on being left alone), or when `kept >= rows.len()`.
///
/// Names the first row's actual column names, so a RENAME is legible
/// without a second round-trip to the server; summarises to ONE line,
/// because a per-row print is a stderr storm.
#[must_use = "a dropped wire row must be surfaced, not discarded"]
fn dropped_rows_diagnostic(
    tool: &str,
    decoded: &Value,
    table: &str,
    kept: usize,
) -> Option<String> {
    let rows = decoded.get(table)?.as_array()?;
    let dropped = rows.len().checked_sub(kept)?;
    if dropped == 0 {
        return None;
    }
    let columns: Vec<&str> = rows
        .first()
        .and_then(Value::as_object)
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    Some(format!(
        "reify-audit: jcodemunch {tool}: dropped {dropped} of {} `{table}` row(s) — \
         a mandatory column is missing or unreadable, so findings over those rows \
         are silently absent; first row's columns: [{}]",
        rows.len(),
        columns.join(", ")
    ))
}

/// Adapter: MUNCH-decoded value → `Vec<DeadSymbol>`.
///
/// Reads the `dead_symbols` table; maps `id/name/kind/file/line/confidence`
/// by column name; parses `signals` via [`parse_signals_list`].
///
/// `line` is OPTIONAL and defaults to the `0` "not reported" sentinel, as in
/// [`changed_symbols_from_wire`] and [`references_from_rows`]: a release
/// that stops emitting a column must cost this adapter a FIELD, never the
/// whole result set — a mandatory read would drop every PDEAD row inside the
/// `filter_map` and the audit would report a clean corpus.
/// `id`/`name`/`kind`/`file` stay mandatory (they are what make a symbol
/// identifiable at all), and so does `confidence` (it is what
/// `min_confidence` filters on, so a default would silently change which
/// rows survive).
fn dead_symbols_from_wire(decoded: &Value) -> Vec<DeadSymbol> {
    let rows = match decoded
        .get("dead_symbols")
        .and_then(|v| v.as_array())
    {
        Some(a) => a,
        None => return Vec::new(),
    };
    rows.iter()
        .filter_map(|row| {
            let id = row.get("id")?.as_str()?.to_string();
            let name = row.get("name")?.as_str()?.to_string();
            let kind = row.get("kind")?.as_str()?.to_string();
            let file = row.get("file")?.as_str()?.to_string();
            let line = row_u64(row, "line").unwrap_or(0) as usize;
            let confidence = row_f64(row, "confidence")?;
            let signals_raw = row
                .get("signals")
                .and_then(|s| s.as_str())
                .unwrap_or("[]");
            let signals = parse_signals_list(signals_raw);
            Some(DeadSymbol {
                id,
                name,
                kind,
                file,
                line,
                confidence,
                signals,
            })
        })
        .collect()
}

/// Adapter: MUNCH-decoded value → `Vec<UntestedSymbol>`.
///
/// Reads the `symbols` table; maps `symbol_id/name/file/confidence` by column
/// name; derives `reached = (wire reason != "unreached")`.
fn untested_symbols_from_wire(decoded: &Value) -> Vec<UntestedSymbol> {
    let rows = match decoded.get("symbols").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    rows.iter()
        .filter_map(|row| {
            let symbol_id = row.get("symbol_id")?.as_str()?.to_string();
            let name = row.get("name")?.as_str()?.to_string();
            let file = row.get("file")?.as_str()?.to_string();
            let confidence = row_f64(row, "confidence")?;
            let reason = row.get("reason").and_then(|r| r.as_str()).unwrap_or("");
            let reached = reason != "unreached";
            Some(UntestedSymbol {
                symbol_id,
                name,
                file,
                reached,
                confidence,
            })
        })
        .collect()
}

/// Adapter: MUNCH-decoded value → `Vec<ChangedSymbol>`.
///
/// Reads ONLY the `added_symbols` table (per PRD §8; removed/changed are
/// decoded but ignored). Maps `name/file/line` by column name; suppression is
/// left UNKNOWN, since the decode step reads no source file and so has no
/// basis for an opt-out judgement — enrichment happens later in
/// [`RealJCodemunchOps::get_changed_symbols`].
///
/// `name` and `file` are MANDATORY — a row without them names no locatable
/// symbol, so it is dropped. `line` is NOT: absent or unparseable, it
/// decodes to the `0` "not reported" sentinel instead of costing the whole
/// symbol, so a drifted grammar under-reports the declaration LOCATION
/// rather than shrinking the P1 sweep's input set. The sentinel is carried
/// end-to-end: [`decl_line_out_of_range`] treats `0` as unlocatable,
/// [`extract_suppression`] declines to answer for it, and
/// [`stale_decl_line_diagnostic`] surfaces it on stderr.
fn changed_symbols_from_wire(decoded: &Value) -> Vec<ChangedSymbol> {
    let rows = match decoded
        .get("added_symbols")
        .and_then(|v| v.as_array())
    {
        Some(a) => a,
        None => return Vec::new(),
    };
    rows.iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.to_string();
            let file = row.get("file")?.as_str()?.to_string();
            let line = row_u64(row, "line").unwrap_or(0) as usize;
            Some(ChangedSymbol {
                name,
                file,
                line,
                suppression: None,
            })
        })
        .collect()
}

/// Adapter: plain-JSON decoded value → `Vec<LayerViolation>`.
///
/// Reads the `violations` array; maps `from_file=from`, `to_file=to`;
/// synthesizes `rule` from `rule_index + from_symbol + to_symbol`.
/// Skips records where `allowed == true`.
fn layer_violations_from_wire(decoded: &Value) -> Vec<LayerViolation> {
    let violations = match decoded.get("violations").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    violations
        .iter()
        .filter_map(|v| {
            let allowed = v.get("allowed").and_then(|a| a.as_bool()).unwrap_or(false);
            if allowed {
                return None;
            }
            let from_file = v.get("from")?.as_str()?.to_string();
            let to_file = v.get("to")?.as_str()?.to_string();
            let rule_index = v
                .get("rule_index")
                .and_then(|r| r.as_u64())
                .map(|n| n.to_string())
                .unwrap_or_default();
            let from_symbol = v
                .get("from_symbol")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let to_symbol = v
                .get("to_symbol")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let rule = format!("rule[{rule_index}]: {from_symbol} → {to_symbol}");
            Some(LayerViolation {
                from_file,
                to_file,
                rule,
            })
        })
        .collect()
}

/// References decoded from a `find_references` response, paired with the
/// table they were read from.
///
/// The table's identity travels OUT with the rows because the caller
/// diagnoses dropped rows against it (see [`dropped_rows_diagnostic`]), and
/// only [`find_references_from_wire`] knows which table it selected — the
/// response names it `__rows__` today, but the fallback scan may read any
/// other. Deciding that twice is how the count comes to be taken over a
/// table the decoder never read, and an all-dropped fallback table then
/// reports zero references in silence.
struct DecodedReferences<'a> {
    /// Key in the decoded response the rows came from.
    table: &'a str,
    /// One entry per row the decoder could read; see
    /// [`references_from_rows`] for what it refuses.
    refs: Vec<SymbolReference>,
}

impl<'a> DecodedReferences<'a> {
    fn read(table: &'a str, rows: &[Value]) -> Self {
        Self {
            table,
            refs: references_from_rows(rows),
        }
    }
}

/// Select the table a `find_references` response carries its rows in and
/// decode them; `None` when no table qualifies.
///
/// A present `__rows__` ARRAY is AUTHORITATIVE, empty included. Only when
/// it is absent entirely does the scan fall back to the first other table
/// whose rows carry a `file` column — `file` being the only column P1
/// consumes.
///
/// Both halves of that rule close the same hazard from opposite sides.
/// `decoded` is a `serde_json::Map`, i.e. a `BTreeMap` (this crate does not
/// enable `serde_json`'s `preserve_order`), so the fallback scan visits
/// tables ALPHABETICALLY rather than in wire order, and `"Aux"` sorts
/// before `"__rows__"`. Without the `__rows__` preference a decoy table
/// wins over the real one; without empty-is-authoritative, ZERO references
/// — the producer-orphan answer P1 exists to detect — falls through to that
/// same decoy. Either way `p1_producer_orphan`'s `has_non_test_caller`
/// reads the wrong reference set and says nothing.
///
/// The live jcodemunch-mcp 1.108.54 shape (measured 2026-08-22,
/// re-confirmed against `local/reify-4ae45bbd`) is `__rows__` with
/// `file|specifier|match_type` — no `line` column at all, so `line == 0`
/// means "not reported", not "line 1". No captured `find_references`
/// fixture exists under `tests/fixtures/jcodemunch/`, so this doc is the
/// record of that shape.
fn find_references_from_wire(decoded: &Value) -> Option<DecodedReferences<'_>> {
    let obj = decoded.as_object()?;

    if let Some(rows) = obj.get("__rows__").and_then(Value::as_array) {
        return Some(DecodedReferences::read("__rows__", rows));
    }

    obj.iter()
        .filter(|(table, _)| *table != "__rows__")
        .find_map(|(table, val)| {
            let rows = val.as_array()?;
            rows.iter()
                .any(|r| r.get("file").is_some())
                .then(|| DecodedReferences::read(table, rows))
        })
}

/// Decode a MUNCH row array into `Vec<SymbolReference>`, dropping any row
/// whose `file` is absent or not a string. `line` is optional and defaults
/// to the `0` "not reported" sentinel.
///
/// Makes NO precondition on `file`: [`find_references_from_wire`]'s
/// fallback scan selects a table on it, but its `__rows__` path passes the
/// table through unchecked, so a release that renames `file` — the drift
/// 1.108.54 already shipped for `line` — drops every row here. That is why
/// the caller pairs this with [`dropped_rows_diagnostic`]: an empty vec
/// reads as "this symbol has zero references" and turns every
/// well-referenced symbol into a P1 producer-orphan finding.
fn references_from_rows(rows: &[Value]) -> Vec<SymbolReference> {
    rows.iter()
        .filter_map(|row| {
            let file = row.get("file")?.as_str()?.to_string();
            let line = row_u64(row, "line").unwrap_or(0) as usize;
            Some(SymbolReference { file, line })
        })
        .collect()
}

// -----------------------------------------------------------------------
// read_source_lines_for_enrichment helper

/// Read source lines for suppression-flag enrichment.
///
/// `Ok` carries the file's lines however few, INCLUDING none — an empty
/// file read fine, and its symbols are unlocatable for that reason rather
/// than for want of a reading. `Err` carries a diagnostic naming the path
/// and the I/O error, for the caller to `eprintln!` once per path. Keeping
/// the two apart here is what spares every caller from re-deriving them
/// from an empty vec, which cannot tell them apart.
fn read_source_lines_for_enrichment(path: &Path) -> Result<Vec<String>, String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .map_err(|e| {
            format!(
                "reify-audit: jcodemunch suppression enrichment: failed to read {}: {e}",
                path.display()
            )
        })
}

// -----------------------------------------------------------------------
// stale-declaration-line classification
// -----------------------------------------------------------------------

/// One out-of-range declaration line: `(file, wire_line, file_line_count)`.
type StaleDeclLine = (String, usize, usize);

/// Why a symbol's wire-reported declaration line could not be located.
///
/// [`decl_line_out_of_range`] folds two conditions into one predicate — a
/// line past EOF and the `0` "no line reported" sentinel — which is right
/// for `extract_suppression`'s guard (both are unlocatable, both get the
/// neutral triple) and wrong for the operator, because the two have
/// OPPOSITE remedies. This enum is what keeps the distinction a value
/// rather than a substring of the rendered message, so
/// `stale_decl_line_diagnostic`'s prose can be reworded freely and a
/// genuine mis-bucketing still fails a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleCause {
    /// The wire reported a line beyond the declaring file's current last
    /// line: the jcodemunch index describes a file that has since shrunk.
    /// Re-indexing fixes it.
    PastEof,
    /// The wire reported the `0` sentinel — it emitted no `line` column, or
    /// a value [`row_u64`] could not read as one, and the adapter's
    /// `.unwrap_or(0)` landed here. That is a grammar-version drift
    /// (`find_references` dropped its `line` column in jcodemunch-mcp
    /// 1.108.54); re-indexing a perfectly current repo would not change one
    /// row.
    NoLineReported,
}

/// Classify one wire-reported declaration line. `0` is the sentinel
/// [`changed_symbols_from_wire`] and [`references_from_rows`] produce for
/// an unreadable or absent `line`; anything else that reached
/// [`decl_line_out_of_range`] was a real number past its file's end.
fn stale_cause(wire_line: usize) -> StaleCause {
    if wire_line == 0 {
        StaleCause::NoLineReported
    } else {
        StaleCause::PastEof
    }
}

/// Split out-of-range entries into `(past_eof, no_line)` by [`stale_cause`],
/// preserving input order within each bucket.
///
/// Separated from the rendering in [`stale_decl_line_diagnostic`] so the
/// classification is asserted as VALUES rather than through substrings of
/// the rendered message: pinned to wording, a behaviour-neutral reword reds
/// the suite while a real mis-bucketing that keeps both phrases present goes
/// undetected.
fn partition_stale_decl_lines(
    entries: &[StaleDeclLine],
) -> (Vec<&StaleDeclLine>, Vec<&StaleDeclLine>) {
    entries
        .iter()
        .partition(|(_, wire_line, _)| stale_cause(*wire_line) == StaleCause::PastEof)
}

// -----------------------------------------------------------------------
// stale_decl_line_diagnostic helper

/// Summarise symbols whose wire-reported declaration line fell outside
/// their (successfully read) file's current line range into ONE diagnostic
/// line, mirroring [`read_source_lines_for_enrichment`]'s "return the
/// diagnostic, let the caller `eprintln!` it" idiom.
///
/// `out_of_range` entries are `(file, wire_line, file_line_count)` for
/// symbols where `extract_suppression` hit its totality guard
/// (`decl_line_1based == 0 || decl_line_1based > lines.len()`) — i.e. the
/// jcodemunch index reported a declaration line that no longer exists in
/// the file on disk. Returns `None` on the happy-path empty slice.
///
/// Partitions those entries by [`StaleCause`] and gives each cause its OWN
/// remedy, since they are structurally different failures wearing one
/// predicate. The bucketing is asserted as VALUES, by
/// [`partition_stale_decl_lines`], so the prose below can be reworded
/// freely and a genuine mis-bucketing still reds.
///
/// ONE line however many symbols are affected, naming only each bucket's
/// count and first entry: a per-symbol print would be the stderr storm this
/// module exists to remove. `RealJCodemunchOps::get_changed_symbols` calls
/// this once per invocation.
fn stale_decl_line_diagnostic(out_of_range: &[StaleDeclLine]) -> Option<String> {
    if out_of_range.is_empty() {
        return None;
    }
    let (past_eof, no_line) = partition_stale_decl_lines(out_of_range);

    // Each clause names only its OWN bucket's first entry and count:
    // entries can span files of different lengths, so a line count
    // attributed to all `n` symbols would be wrong for every entry but the
    // one it was read off.
    let mut clauses: Vec<String> = Vec::new();
    if let Some((file, wire_line, file_line_count)) = past_eof.first() {
        // No `>`-shaped claim even in this bucket, where it would be true:
        // keeping both clauses phrased alike is what stops a later reader
        // from reintroducing "line 0 > 200 lines" by copying this one.
        clauses.push(format!(
            "{} symbol(s) have a declaration line past their file's current end \
             (first: {file} line {wire_line}, which has {file_line_count} lines) — \
             the jcodemunch index may be stale; re-index the repo",
            past_eof.len()
        ));
    }
    if let Some((file, wire_line, _file_line_count)) = no_line.first() {
        // Deliberately carries no "re-index" imperative: the file's own
        // line count is irrelevant to a wire that never reported a line, so
        // naming it here would only invite the wrong remedy.
        clauses.push(format!(
            "{} symbol(s) reported no declaration line (first: {file}, reported \
             line {wire_line}) — the jcodemunch wire omitted the `line` column, \
             or sent a value that is not a line number; that is a grammar drift, \
             not a stale index",
            no_line.len()
        ));
    }
    // Still ONE line however many buckets fired — `; `-joined, never
    // newline-joined; see this function's doc on the per-symbol storm.
    Some(format!(
        "reify-audit: jcodemunch suppression enrichment: {}. Suppression flags \
         are unavailable for these symbols.",
        clauses.join("; ")
    ))
}

/// Collect `(file, wire_line, file_line_count)` for every symbol whose
/// wire-reported declaration line is out of range for its file, per
/// [`decl_line_out_of_range`] — the SAME predicate `extract_suppression`
/// uses for its own totality guard, so the two can never drift apart: the
/// diagnostic this feeds is only correct when it fires for exactly the
/// symbols `extract_suppression` treats as unlocatable.
///
/// `line_count_for(file)` should return `None` only when the file's line
/// count is UNKNOWN (e.g. the file could not be read), so an unreadable
/// file is reported once by [`read_source_lines_for_enrichment`]'s own
/// diagnostic and not double-reported here. A file that read fine and is
/// empty is a known count of `0`, not an unknown one — it must return
/// `Some(0)`, which makes every one of its symbols out-of-range (nothing
/// is locatable in a 0-line file) and so reportable.
fn collect_stale_decl_lines(
    symbols: &[ChangedSymbol],
    line_count_for: impl Fn(&str) -> Option<usize>,
) -> Vec<StaleDeclLine> {
    symbols
        .iter()
        .filter_map(|sym| {
            let n = line_count_for(&sym.file)?;
            decl_line_out_of_range(sym.line, n).then(|| (sym.file.clone(), sym.line, n))
        })
        .collect()
}

/// Enrich `symbols`' suppression flags IN PLACE by reading each declaring
/// source file under `project_root`, and RETURN the once-per-invocation
/// stale-index diagnostic instead of printing it — the same "return the
/// diagnostic, let the caller `eprintln!` it" idiom as
/// [`read_source_lines_for_enrichment`] and [`stale_decl_line_diagnostic`].
///
/// Returning it is what makes the stale-index report observable from a test
/// at all: an in-process test cannot read its own process's stderr, so a
/// report printed from inside here could be deleted with the whole suite
/// still green.
///
/// Per-path READ failures are still printed here rather than returned:
/// there is one per unreadable path (not one per invocation), and the
/// caller has no de-duplication this function's own cache does not already
/// provide.
#[must_use = "the stale-index diagnostic must be surfaced, not dropped"]
fn enrich_suppression_flags(symbols: &mut [ChangedSymbol], project_root: &Path) -> Option<String> {
    // Cache by path: many symbols share the same file (e.g. decl.rs has
    // 1110+ rows), so reading each file once avoids O(symbols) disk reads.
    // The `Err` arm keeps its message only until it is printed; what the
    // cache carries forward is that this path was already reported, so the
    // stale-index pass below does not report it a second time.
    let mut file_cache: HashMap<PathBuf, Result<Vec<String>, ()>> = HashMap::new();
    for sym in symbols.iter_mut() {
        let path = project_root.join(&sym.file);
        let cached = match file_cache.entry(path) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(v) => {
                let entry = read_source_lines_for_enrichment(v.key()).map_err(|msg| {
                    eprintln!("{msg}");
                });
                v.insert(entry)
            }
        };
        // No `else`: a file that could not be read leaves `suppression` at
        // the `None` the decode step set, which is the CORRECT answer — no
        // declaration was scanned, so no opt-out judgement exists. That is an
        // absence of evidence, not evidence of absence, and
        // `read_source_lines_for_enrichment`'s own per-path message above is
        // what keeps it from being silent.
        if let Ok(lines) = cached {
            sym.suppression = extract_suppression(lines, sym.line);
        }
    }
    // A second pass over the now-fully-populated `file_cache` rather than
    // an inline push in the loop above: `collect_stale_decl_lines` is its
    // own independently-tested function (see its doc, and
    // `decl_line_out_of_range`'s), so the wiring here reduces to "call it
    // and hand the result to `stale_decl_line_diagnostic`" — deleting
    // either line fails to compile on an unresolved `out_of_range`.
    let out_of_range = collect_stale_decl_lines(symbols, |file| {
        // `Some(0)` for a readable-but-empty file (nothing is locatable in
        // a 0-line file, so its symbols get summarised), `None` only for a
        // read failure, already reported once per path above.
        file_cache
            .get(&project_root.join(file))
            .and_then(|cached| cached.as_ref().ok().map(Vec::len))
    });
    stale_decl_line_diagnostic(&out_of_range)
}

/// True when a 1-based declaration line is out of range for a file of
/// `line_count` lines: either `0` (the "no line reported" sentinel) or
/// beyond the file's last line.
///
/// Shared by [`extract_suppression`]'s totality guard and
/// [`collect_stale_decl_lines`]'s diagnostic collection so the two can
/// never drift apart — without a shared predicate, a later change to one
/// (e.g. relaxing the guard's `>` to `>=`) could leave the other reporting
/// symbols that were in fact scanned, or silently missing ones that were
/// not.
fn decl_line_out_of_range(decl_line_1based: usize, line_count: usize) -> bool {
    decl_line_1based == 0 || decl_line_1based > line_count
}

// extract_suppression helper
// -----------------------------------------------------------------------

/// Scan the contiguous attribute/comment block immediately above a
/// declaration and extract the opt-outs it carries.
///
/// Returns `Some(DeclSuppression)` when the declaration was located and
/// scanned, `None` when it was not. The two are DIFFERENT answers, and
/// collapsing them is what let an unlocatable declaration read as "the
/// author declined every opt-out".
///
/// Scans upward from the line immediately above `decl_line_1based` over
/// lines that are: attribute lines (`#[...]`), line-comments (`//`), or
/// doc-comment lines (`///` or `//!`). Stops at the first non-matching line.
///
/// - `has_allow_dead_code` — an attribute contains `allow(` and `dead_code`
/// - `has_cfg_test` — an attribute contains `cfg(test)`
/// - `g_allow_marker` — first `// G-allow: <reason>` with non-blank reason
///
/// `decl_line_1based` arrives verbatim off the jcodemunch wire as
/// [`ChangedSymbol::line`](crate::ChangedSymbol::line) and is UNTRUSTED: it
/// must never index `lines` unchecked. This function is TOTAL — `0` or a
/// line beyond `lines.len()` returns `None` rather than panicking.
/// (Observed 2026-08-22, before that guard:
/// `index out of bounds: the len is 13165 but the index is 18319`, a stale
/// index pointing past a 13165-line `crates/reify-eval/src/engine_build.rs`.)
///
/// Deliberately does NOT clamp to `lines.len()` and scan from there: that
/// reads an arbitrary unrelated block and could fabricate an
/// `#[allow(dead_code)]` / `// G-allow:` the symbol never carried. `None` —
/// declining to answer — is the only answer that cannot invent one, and
/// [`stale_decl_line_diagnostic`] is what keeps it from being silent.
///
/// Takes `&[String]`, not `&[&str]`, so the caller's cached file contents
/// pass straight through without re-materialising an adapter per symbol.
fn extract_suppression(lines: &[String], decl_line_1based: usize) -> Option<DeclSuppression> {
    if decl_line_out_of_range(decl_line_1based, lines.len()) {
        return None;
    }
    let decl_idx = decl_line_1based - 1; // 0-based
    let mut has_allow_dead_code = false;
    let mut has_cfg_test = false;
    let mut g_allow_marker: Option<String> = None;

    // Walk upward from the line above the declaration
    let mut idx = decl_idx;
    loop {
        if idx == 0 {
            break;
        }
        idx -= 1;
        let line = lines[idx].trim();
        if is_attr_or_comment(line) {
            // Check for allow(dead_code)
            if line.starts_with("#[") && line.contains("allow(") && line.contains("dead_code") {
                has_allow_dead_code = true;
            }
            // Check for cfg(test)
            if line.starts_with("#[") && line.contains("cfg(test)") {
                has_cfg_test = true;
            }
            // Check for G-allow marker
            if g_allow_marker.is_none()
                && let Some(reason) = extract_g_allow(line)
            {
                g_allow_marker = Some(reason);
            }
        } else {
            break;
        }
    }

    Some(DeclSuppression {
        has_allow_dead_code,
        has_cfg_test,
        g_allow_marker,
    })
}

fn is_attr_or_comment(line: &str) -> bool {
    // Stop scanning at blank lines — the block must be contiguous.
    line.starts_with("#[") || line.starts_with("//")
}

/// Extract a `// G-allow: <reason>` marker from a comment line.
///
/// Requires non-blank reason text (mirrors `scripts/audit-orphan-producers.sh:150`
/// `G_ALLOW_RE = //\s*G-allow:\s*(.+)` where `(.+)` is non-empty).
fn extract_g_allow(line: &str) -> Option<String> {
    // Match `//\s*G-allow:\s*(.+)` — non-blank capture
    let rest = line.strip_prefix("//")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("G-allow:")?;
    let reason = rest.trim_start();
    if reason.is_empty() {
        return None;
    }
    Some(reason.to_string())
}

// -----------------------------------------------------------------------
// filter_refs_to_file
// -----------------------------------------------------------------------

/// Retain only references whose `.file == file`. Input order is preserved.
///
/// This is the key client-side scoping step: jcodemunch's `find_references`
/// API has no server-side file-scope parameter, so filtering is done here.
///
/// KNOWN LIMITATION: this discards every cross-file reference, not only
/// same-named symbols declared in other files. A symbol consumed ONLY from
/// another file — arguably the strongest evidence that it is NOT an
/// orphan — ends up with an empty reference list after this filter runs.
/// `p1_producer_orphan.rs`'s non-test-caller check therefore only ever sees
/// same-file callers, even though its own condition does not spell out a
/// same-file conjunct. Loosening this (e.g. if jcodemunch ever grows a
/// server-side file-scope parameter — see the
/// [`JCodemunchOps::find_references`](crate::JCodemunchOps::find_references)
/// doc, which is what states the MUST — or having P1 treat cross-file refs
/// as their own signal) is tracked as a follow-up (#6504) rather than
/// fixed here.
fn filter_refs_to_file(refs: Vec<SymbolReference>, file: &str) -> Vec<SymbolReference> {
    refs.into_iter().filter(|r| r.file == file).collect()
}

// -----------------------------------------------------------------------
// HTTP transport
// -----------------------------------------------------------------------

/// Sync MCP streamable-HTTP client for jcodemunch. One instance == one
/// MCP session.
///
/// `session_id` holds the id the **server** assigned during the handshake
/// (see the module-level "Session lifecycle" notes); it is never minted
/// here. Differs from [`crate::fused_memory_client::FusedMemoryClient`] in
/// that session handling, in `CLIENT_NAME`, and in the `call_tool` content
/// step (which routes MUNCH-vs-JSON via [`decode_tool_result`]).
pub struct JcodemunchClient {
    url: String,
    session_id: String,
    agent: ureq::Agent,
    next_id: Cell<u64>,
}

impl JcodemunchClient {
    /// Connect to `url` and complete the MCP handshake
    /// (initialize + notifications/initialized).
    ///
    /// The `session_id` starts empty and is filled in by
    /// [`Self::initialize`] before the value is returned. That window is
    /// unobservable: `new` is the only constructor and it propagates a
    /// failed handshake, so every instance a caller can hold carries a
    /// real server-assigned id.
    pub fn new(url: impl Into<String>) -> Result<Self, LoadError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build();
        let mut client = Self {
            url: url.into(),
            session_id: String::new(),
            agent,
            next_id: Cell::new(1),
        };
        client.initialize()?;
        Ok(client)
    }

    /// Run the MCP handshake: `initialize` with no session header, store
    /// the id the server assigns in its response, then acknowledge with
    /// `notifications/initialized` carrying that id.
    ///
    /// An `initialize` response that assigns no session id is a hard
    /// failure, not an empty session: without an id every later POST is
    /// answered `400 Missing session ID`, so an `Ok` here would hand back
    /// a client that cannot make a single successful call.
    ///
    /// Gotcha, observed against a live serve: a jcodemunch **404**
    /// response also carries a fresh `mcp-session-id` header, so the id
    /// must only ever be read off a *success* response. `ureq` returns
    /// `Err(Error::Status(..))` for 4xx and [`Self::post_raw`] maps that to
    /// [`LoadError::Http`] before the header is read, so the confusion is
    /// unreachable today — this note exists so a future refactor (e.g. one
    /// that inspects error responses) does not quietly make it reachable.
    fn initialize(&mut self) -> Result<(), LoadError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {},
            },
        });
        // No session header on `initialize`: the server assigns it.
        let (assigned, _) = self.post_raw(&payload, None)?;
        self.session_id = assigned.ok_or_else(|| {
            LoadError::Protocol(
                "initialize response carried no Mcp-Session-Id header — the \
                 server did not assign a session; not a live jcodemunch seam"
                    .into(),
            )
        })?;

        let ack = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        });
        let _ = self.post_raw(&ack, Some(&self.session_id))?;
        Ok(())
    }

    fn next_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    /// POST `payload`, attaching `mcp-session-id` only when `session` is
    /// `Some`. Returns the response's own `mcp-session-id` header (if any)
    /// alongside the decoded body.
    ///
    /// The session header is copied out **before** anything else touches
    /// the response: the 202 branch returns early, and `into_reader()`
    /// consumes the response by value, so any later read is impossible.
    /// `ureq`'s `Response::header` matches case-insensitively — a live
    /// jcodemunch serve emits the name lowercase.
    fn post_raw(
        &self,
        payload: &Value,
        session: Option<&str>,
    ) -> Result<(Option<String>, Value), LoadError> {
        let mut request = self
            .agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream");
        if let Some(session) = session {
            request = request.set("mcp-session-id", session);
        }
        let response = request
            .send_json(payload.clone())
            .map_err(|e| LoadError::Http(format!("POST {}: {e}", self.url)))?;

        let assigned = response.header("mcp-session-id").map(|s| s.to_string());

        if response.status() == 202 {
            return Ok((assigned, Value::Null));
        }

        let ctype = response
            .header("content-type")
            .unwrap_or("")
            .to_string();
        // Use into_reader() to avoid ureq's 10 MiB into_string cap
        let mut body = String::new();
        response
            .into_reader()
            .read_to_string(&mut body)
            .map_err(|e| LoadError::Http(format!("read body: {e}")))?;

        let value = if ctype.contains("text/event-stream") {
            let mut parsed: Option<Value> = None;
            for line in body.lines() {
                if let Some(rest) = line.strip_prefix("data:") {
                    parsed = Some(serde_json::from_str(rest.trim()).map_err(|e| {
                        LoadError::Protocol(format!(
                            "SSE data parse: {e}; body={body}"
                        ))
                    })?);
                    break;
                }
            }
            parsed.ok_or_else(|| {
                LoadError::Protocol(format!("no SSE data line in response: {body}"))
            })?
        } else if body.is_empty() {
            return Ok((assigned, Value::Null));
        } else {
            serde_json::from_str(&body).map_err(|e| {
                LoadError::Protocol(format!("body parse: {e}; body={body}"))
            })?
        };

        if let Some(err) = value.get("error") {
            return Err(LoadError::Protocol(format!("JSON-RPC error: {err}")));
        }
        Ok((assigned, value))
    }

    /// POST `payload` on the established session. Thin wrapper over
    /// [`Self::post_raw`] for every call after the handshake.
    fn post(&self, payload: &Value) -> Result<Value, LoadError> {
        self.post_raw(payload, Some(&self.session_id))
            .map(|(_, value)| value)
    }

    fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, LoadError> {
        let resp = self
            .post(&json!({
                "jsonrpc": "2.0",
                "id": self.next_id(),
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            }))
            .map_err(|e| match e {
                LoadError::Protocol(m) => {
                    LoadError::Protocol(format!("{name}: {m}"))
                }
                other => other,
            })?;
        let result = resp.get("result").cloned().unwrap_or(Value::Null);
        decode_tool_result(&result)
            .map_err(|e| match e {
                LoadError::Protocol(m) => LoadError::Protocol(format!("{name}: {m}")),
                other => other,
            })
    }

    /// Client-side counterpart of the MCP `tools/list` method: the names of
    /// every tool this session's server advertises.
    ///
    /// This is the observable signal for boundary scenario B1 — a completed
    /// handshake only means the seam is live if the server actually offers
    /// the tools the detectors call, which is what
    /// `tests/jcodemunch_session_live.rs` asserts against a real serve.
    ///
    /// A missing or non-array `result.tools`, or an entry with no `name`, is
    /// a [`LoadError::Protocol`] — never an empty or silently shortened
    /// list. An absent tool list is a protocol violation, not a server with
    /// no tools, and reporting it as `Ok(vec![])` would recreate exactly the
    /// PASS-shaped nothing this client's session handling exists to prevent.
    ///
    /// Goes through [`Self::post`], so it carries the stored server-assigned
    /// session id and reuses the SSE-vs-JSON routing unchanged (a live serve
    /// answers `tools/list` as `text/event-stream`). It deliberately does
    /// NOT route through [`decode_tool_result`], which decodes a
    /// `tools/call` result's MUNCH-vs-JSON content and does not apply to a
    /// `tools/list` envelope.
    ///
    /// Compiled out of production builds entirely
    /// (`#[cfg(any(test, feature = "test-support"))]`, the same gate
    /// `MockGitOps` uses in `lib.rs`): no production path calls it, and the
    /// crate's own `[dev-dependencies]` self-pull enables `test-support`, so
    /// `tests/jcodemunch_session_live.rs` — a separate crate — still sees it
    /// without the surface leaking into the released API.
    #[cfg(any(test, feature = "test-support"))]
    // G-allow: test-facing pub fn, compiled out of production builds by the `test-support` gate above (sole external caller: tests/jcodemunch_session_live.rs, a separate crate; pub(crate) would break it). The observable signal for PRD boundary scenario B1 — that a live serve advertises the tools the detectors call. The marker stays because scripts/audit-orphan-producers.sh masks only a literal `#[cfg(test)]`, not this gate.
    pub fn list_tools(&self) -> Result<Vec<String>, LoadError> {
        let resp = self.post(&json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "tools/list",
            "params": {},
        }))?;
        let tools = resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .ok_or_else(|| {
                LoadError::Protocol(format!(
                    "tools/list: response carried no `result.tools`; got {resp}"
                ))
            })?
            .as_array()
            .ok_or_else(|| {
                LoadError::Protocol(format!(
                    "tools/list: `result.tools` is not an array; got {resp}"
                ))
            })?;
        tools
            .iter()
            .map(|tool| {
                tool.get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        LoadError::Protocol(format!(
                            "tools/list: tool entry has no string `name`; got {tool}"
                        ))
                    })
            })
            .collect()
    }
}

// -----------------------------------------------------------------------
// RealJCodemunchOps
// -----------------------------------------------------------------------

/// Production implementation of [`JCodemunchOps`] backed by a live
/// jcodemunch MCP server.
///
/// Holds `project_root` to resolve workspace-relative file paths when
/// enriching [`ChangedSymbol`] suppression flags (jcodemunch wire records
/// carry no source attributes).
pub struct RealJCodemunchOps {
    client: JcodemunchClient,
    repo: String,
    project_root: PathBuf,
}

impl RealJCodemunchOps {
    /// Create a new `RealJCodemunchOps`.
    ///
    /// Performs the MCP handshake on construction. Returns an error if the
    /// server is unreachable.
    pub fn new(
        url: impl Into<String>,
        repo: impl Into<String>,
        project_root: impl Into<PathBuf>,
    ) -> Result<Self, LoadError> {
        let client = JcodemunchClient::new(url)?;
        Ok(Self {
            client,
            repo: repo.into(),
            project_root: project_root.into(),
        })
    }
}

impl JCodemunchOps for RealJCodemunchOps {
    fn get_changed_symbols(&self, since_sha: &str, until_sha: &str) -> Vec<ChangedSymbol> {
        let decoded = match self.client.call_tool(
            "get_changed_symbols",
            json!({
                "repo": self.repo,
                "since_sha": since_sha,
                "until_sha": until_sha,
            }),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("jcodemunch get_changed_symbols: {e}");
                return Vec::new();
            }
        };
        let mut symbols = changed_symbols_from_wire(&decoded);
        // A dropped row means a MANDATORY column (`name`/`file`) the wire
        // no longer carries. Reported once per call, never once per row.
        if let Some(msg) =
            dropped_rows_diagnostic("get_changed_symbols", &decoded, "added_symbols", symbols.len())
        {
            eprintln!("{msg}");
        }
        // Two separate guards hold this wiring in place, because neither
        // covers the other: `#[must_use]` reds the `-D warnings` gate if the
        // returned report is discarded, and a test pins the CALL, which
        // `#[must_use]` cannot do once someone deletes it outright.
        if let Some(msg) = enrich_suppression_flags(&mut symbols, &self.project_root) {
            eprintln!("{msg}");
        }
        symbols
    }

    fn find_references(&self, symbol: &ChangedSymbol) -> Vec<SymbolReference> {
        let decoded = match self.client.call_tool(
            "find_references",
            json!({
                "repo": self.repo,
                "identifier": symbol.name,
            }),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("jcodemunch find_references({}): {e}", symbol.name);
                return Vec::new();
            }
        };
        // Diagnosed against the table the decoder ACTUALLY read, which is
        // why it travels back with the rows: `__rows__` today, any other
        // table on the fallback scan. Counted BEFORE `filter_refs_to_file`,
        // since scoping to the declaring file is a deliberate narrowing,
        // not a decode failure. What must be loud is rows the decoder could
        // not read at all — an empty result there is indistinguishable
        // from the genuine zero-reference answer P1 reads as a producer
        // orphan.
        let refs = match find_references_from_wire(&decoded) {
            Some(DecodedReferences { table, refs }) => {
                if let Some(msg) =
                    dropped_rows_diagnostic("find_references", &decoded, table, refs.len())
                {
                    eprintln!("{msg}");
                }
                refs
            }
            // No table carried references at all, so there were no rows to
            // drop and nothing to report.
            None => Vec::new(),
        };
        filter_refs_to_file(refs, &symbol.file)
    }

    fn get_dead_code(&self, min_confidence: f64) -> Vec<DeadSymbol> {
        let decoded = match self.client.call_tool(
            "get_dead_code_v2",
            json!({
                "repo": self.repo,
                "min_confidence": min_confidence,
            }),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("jcodemunch get_dead_code_v2: {e}");
                return Vec::new();
            }
        };
        let symbols = dead_symbols_from_wire(&decoded);
        // `confidence` is mandatory here (it is what `min_confidence`
        // filters on), so its drift would empty the whole PDEAD corpus and
        // report a clean repo.
        if let Some(msg) =
            dropped_rows_diagnostic("get_dead_code_v2", &decoded, "dead_symbols", symbols.len())
        {
            eprintln!("{msg}");
        }
        symbols
    }

    fn get_untested_symbols(&self, min_confidence: f64) -> Vec<UntestedSymbol> {
        let decoded = match self.client.call_tool(
            "get_untested_symbols",
            json!({
                "repo": self.repo,
                "min_confidence": min_confidence,
            }),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("jcodemunch get_untested_symbols: {e}");
                return Vec::new();
            }
        };
        let symbols = untested_symbols_from_wire(&decoded);
        if let Some(msg) =
            dropped_rows_diagnostic("get_untested_symbols", &decoded, "symbols", symbols.len())
        {
            eprintln!("{msg}");
        }
        symbols
    }

    fn get_layer_violations(&self) -> Vec<LayerViolation> {
        let decoded = match self.client.call_tool(
            "get_layer_violations",
            json!({
                "repo": self.repo,
            }),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("jcodemunch get_layer_violations: {e}");
                return Vec::new();
            }
        };
        layer_violations_from_wire(&decoded)
    }
}

// -----------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // step-1 / step-2: munch_decode mechanics
    // ------------------------------------------------------------------
    #[allow(clippy::approx_constant)] // 3.14 is a fixture value, not an approximation of π
    #[test]
    fn munch_decode_mechanics_on_inline_string() {
        // Controlled MUNCH/1 payload: @N ref table, one table spec,
        // 2 data rows exercising:
        //   - standalone @N reference
        //   - @N<suffix> prefix expansion
        //   - "..."-quoted field with commas (stays one literal)
        //   - int and float coercion
        let munch = concat!(
            "#MUNCH/1 tool=test_tool enc=gen1\n",
            "\n",
            "@1=prefix/\n",
            "@2=hello\n",
            "\n",
            "meta=val __stypes=x:int __tables=t:things:name|count|score|tag:str|int|float|str\n",
            "\n",
            // Row 1: standalone @2 ref, count=42 (int), score=3.14 (float), tag="a,b,c" (quoted with commas)
            "t,@2,42,3.14,\"a,b,c\"\n",
            // Row 2: @1suffix expansion (prefix/ + world), count=0, score=0.0, plain tag
            "t,@1world,0,0.0,plain\n",
        );

        let v = munch_decode(munch).expect("munch_decode should succeed");

        let things = v.get("things").and_then(|t| t.as_array()).expect("things table");
        assert_eq!(things.len(), 2);

        let row0 = &things[0];
        assert_eq!(row0.get("name").and_then(|n| n.as_str()), Some("hello"));
        assert_eq!(row0.get("count").and_then(|n| n.as_i64()), Some(42));
        assert!(
            (row0.get("score").and_then(|n| n.as_f64()).unwrap() - 3.14).abs() < 1e-9
        );
        assert_eq!(row0.get("tag").and_then(|t| t.as_str()), Some("a,b,c"));

        let row1 = &things[1];
        assert_eq!(row1.get("name").and_then(|n| n.as_str()), Some("prefix/world"));
        assert_eq!(row1.get("count").and_then(|n| n.as_i64()), Some(0));
        assert_eq!(row1.get("tag").and_then(|t| t.as_str()), Some("plain"));
    }

    // ------------------------------------------------------------------
    // step-1 / step-2: 3-segment __tables spec (jcodemunch-mcp 1.108.54)
    // ------------------------------------------------------------------

    /// The real-wire `__tables` spec measured against jcodemunch-mcp
    /// 1.108.54 on 2026-08-22 for `find_references`:
    /// `r:__rows__:file|specifier|match_type` — 3 colon-segments, the type
    /// list omitted. Every fixture under `tests/fixtures/jcodemunch/` was
    /// captured against 1.108.27 and uses the 4-segment form, so both
    /// grammars must be accepted (widening, not migration). Omitted types
    /// mean every column decodes as `ColType::Str`.
    #[test]
    fn munch_decode_accepts_a_three_segment_table_spec_as_all_str() {
        let munch = concat!(
            "#MUNCH/1 tool=find_references enc=gen1\n",
            "\n",
            "@1=crates/reify-audit/\n",
            "\n",
            "x=1 __stypes= __tables=r:__rows__:file|specifier|match_type\n",
            "r,@1src/jcodemunch_client.rs,crate,named\n",
            "r,@1tests/p1.rs,reify_audit,named\n",
        );

        let v = munch_decode(munch).expect("3-segment __tables spec should decode");

        let rows = v
            .get("__rows__")
            .and_then(|t| t.as_array())
            .expect("__rows__ table");
        assert_eq!(rows.len(), 2);

        for row in rows {
            for key in ["file", "specifier", "match_type"] {
                assert!(
                    matches!(row.get(key), Some(Value::String(_))),
                    "field {key:?} should decode as a String (type list \
                     omitted => every column is ColType::Str); row={row:?}"
                );
            }
        }

        assert_eq!(
            rows[0].get("file").and_then(|f| f.as_str()),
            Some("crates/reify-audit/src/jcodemunch_client.rs"),
            "the @1 ref must expand on the file column"
        );
        assert_eq!(rows[0].get("specifier").and_then(|f| f.as_str()), Some("crate"));
        assert_eq!(rows[0].get("match_type").and_then(|f| f.as_str()), Some("named"));

        assert_eq!(
            rows[1].get("file").and_then(|f| f.as_str()),
            Some("crates/reify-audit/tests/p1.rs")
        );
        assert_eq!(
            rows[1].get("specifier").and_then(|f| f.as_str()),
            Some("reify_audit")
        );
        assert_eq!(rows[1].get("match_type").and_then(|f| f.as_str()), Some("named"));
    }

    /// Pins that the step-2 relaxation stays bounded: a 2-segment spec
    /// (prefix + table name, no columns at all) is still rejected. Passes
    /// today; exists so step-2 cannot over-widen the grammar past 3-or-4.
    #[test]
    fn munch_decode_still_rejects_a_two_segment_table_spec() {
        let munch = concat!(
            "#MUNCH/1 tool=find_references enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=r:__rows__\n",
        );
        match munch_decode(munch) {
            Err(LoadError::Protocol(msg)) => {
                assert!(
                    msg.contains("colon-segments"),
                    "error should mention colon-segments: {msg}"
                );
            }
            Ok(_) => panic!("expected Protocol error for 2-segment table spec, got Ok"),
            Err(LoadError::Http(_)) => panic!("expected Protocol error, got Http error"),
        }
    }

    // ------------------------------------------------------------------
    // step-3 / step-4: decode_tool_result routing
    // ------------------------------------------------------------------

    #[test]
    fn decode_tool_result_routes_munch_payload() {
        let munch_text = concat!(
            "#MUNCH/1 tool=t enc=gen1\n",
            "\n",
            "x=1 __stypes=x:int __tables=t:rows:val:str\n",
            "t,hello\n",
        );
        let result = serde_json::json!({
            "content": [{"type": "text", "text": munch_text}]
        });
        let decoded = decode_tool_result(&result).expect("should succeed");
        assert!(decoded.get("rows").is_some());
    }

    #[test]
    fn decode_tool_result_routes_plain_json() {
        let result = serde_json::json!({
            "content": [{"type": "text", "text": "{\"violations\":[]}"}]
        });
        let decoded = decode_tool_result(&result).expect("should succeed");
        assert!(decoded.get("violations").is_some());
    }

    #[test]
    fn decode_tool_result_munch_garbage_returns_error() {
        // A #MUNCH/1 payload with no __tables= declaration is malformed.
        let result = serde_json::json!({
            "content": [{"type": "text", "text": "#MUNCH/1 tool=t enc=gen1\n\nbad line with no __tables\ndata,row\n"}]
        });
        match decode_tool_result(&result) {
            Err(LoadError::Protocol(_)) => {} // expected
            Ok(_) => panic!("expected Protocol error for garbage MUNCH, got Ok"),
            Err(LoadError::Http(_)) => panic!("expected Protocol error, got Http error"),
        }
    }

    // ------------------------------------------------------------------
    // step-5 / step-6: dead_symbols_from_wire (live fixture)
    // ------------------------------------------------------------------

    #[test]
    fn dead_symbols_from_wire_live_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jcodemunch/get_dead_code_v2.json"
        ));
        let envelope: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
        let decoded = decode_tool_result(&envelope).expect("decode_tool_result");
        let symbols = dead_symbols_from_wire(&decoded);
        assert_eq!(symbols.len(), 100, "expected 100 dead symbols");
        let row0 = &symbols[0];
        assert_eq!(row0.id, "analysis/phase_a_match.py::blob#function");
        assert_eq!(row0.name, "blob");
        assert_eq!(row0.kind, "function");
        assert_eq!(row0.file, "analysis/phase_a_match.py");
        assert_eq!(row0.line, 7);
        assert!((row0.confidence - 1.0).abs() < 1e-9);
        assert_eq!(
            row0.signals,
            vec!["unreachable_file", "no_callers", "not_barrel_exported"]
        );
    }

    // ------------------------------------------------------------------
    // step-7 / step-8: untested_symbols_from_wire (live fixture)
    // ------------------------------------------------------------------

    #[test]
    fn untested_symbols_from_wire_live_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jcodemunch/get_untested_symbols.json"
        ));
        let envelope: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
        let decoded = decode_tool_result(&envelope).expect("decode_tool_result");
        let symbols = untested_symbols_from_wire(&decoded);
        assert_eq!(symbols.len(), 100, "expected 100 untested symbols");
        let row0 = &symbols[0];
        assert_eq!(row0.symbol_id, "analysis/phase_a_match.py::blob#function");
        assert_eq!(row0.name, "blob");
        assert_eq!(row0.file, "analysis/phase_a_match.py");
        assert!((row0.confidence - 1.0).abs() < 1e-9);
        assert!(!row0.reached, "reached should be false (reason=unreached)");
    }

    // ------------------------------------------------------------------
    // step-9 / step-10: changed_symbols_from_wire (live fixture)
    // ------------------------------------------------------------------

    #[test]
    fn changed_symbols_from_wire_live_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jcodemunch/get_changed_symbols.json"
        ));
        let envelope: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
        let decoded = decode_tool_result(&envelope).expect("decode_tool_result");
        let symbols = changed_symbols_from_wire(&decoded);
        assert_eq!(symbols.len(), 1110, "expected 1110 added symbols");
        let row0 = &symbols[0];
        assert_eq!(row0.file, "crates/reify-ast/src/decl.rs");
        assert_eq!(row0.line, 877);
        assert!(!row0.name.is_empty(), "name should be non-empty");
    }

    /// The decode step reads no source file, so it has no basis for ANY
    /// opt-out judgement — every decoded symbol must leave suppression
    /// UNKNOWN rather than claim the author declined every opt-out.
    ///
    /// Asserted over every row of the live fixture, not just the first: a
    /// per-row default that held for row 0 and drifted elsewhere would be
    /// exactly the conflation `DeclSuppression` exists to make impossible.
    #[test]
    fn changed_symbols_from_wire_leaves_suppression_unknown() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jcodemunch/get_changed_symbols.json"
        ));
        let envelope: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
        let decoded = decode_tool_result(&envelope).expect("decode_tool_result");
        let symbols = changed_symbols_from_wire(&decoded);
        assert!(!symbols.is_empty(), "fixture must decode some symbols");
        assert!(
            symbols.iter().all(|sym| !sym.decl_located()),
            "decoding reads no source file — suppression must stay UNKNOWN for \
             every row until enrichment runs; got {:?}",
            symbols.iter().find(|sym| sym.decl_located()),
        );
    }

    /// A 3-segment (type-less) `__tables` spec — the grammar
    /// `parse_one_table_spec` widened to accept — decodes EVERY column as
    /// `ColType::Str`, so `line` arrives as a JSON string rather than a
    /// number. A strict `as_u64()` read would drop the row inside
    /// `filter_map`; this pins that a string-encoded `line` still decodes.
    #[test]
    fn changed_symbols_from_wire_tolerates_a_string_encoded_line_under_a_typeless_spec() {
        let munch = concat!(
            "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:added_symbols:name|file|line\n",
            "t,widget,a.rs,99\n",
        );
        let v = munch_decode(munch).expect("decode type-less added_symbols munch");
        let symbols = changed_symbols_from_wire(&v);
        assert_eq!(
            symbols.len(),
            1,
            "a string-encoded line must not cause the row to be dropped; got {symbols:?}"
        );
        assert_eq!(symbols[0].name, "widget");
        assert_eq!(symbols[0].file, "a.rs");
        assert_eq!(symbols[0].line, 99);
    }

    /// A `line` column typed `float` on the wire, or written float-shaped
    /// under a type-less spec, is PRESENT and CORRECT — it must not be
    /// reported as an omitted column.
    ///
    /// `coerce_value` routes `ColType::Float` through
    /// `serde_json::Number::from_f64`, and `serde_json`'s `as_u64()`
    /// returns `None` for an f64-backed number however integral. Read
    /// strictly, the call site's `.unwrap_or(0)` then yields the SAME `0`
    /// that means "the wire emitted no `line` column" — and the operator is
    /// told to expect a grammar drift that never happened, defeating the
    /// remedy split `stale_decl_line_diagnostic` exists to make.
    #[test]
    fn changed_symbols_from_wire_reads_an_integral_float_line_column() {
        // Explicitly `float`-typed: a JSON number `serde_json` stores as f64.
        let typed = concat!(
            "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:added_symbols:name|file|line:str|str|float\n",
            "t,widget,a.rs,99\n",
        );
        let v = munch_decode(typed).expect("decode float-typed added_symbols munch");
        assert!(
            v["added_symbols"][0]["line"].as_u64().is_none(),
            "premise: an f64-backed JSON number has no as_u64(); got {:?}",
            v["added_symbols"][0]["line"]
        );
        let symbols = changed_symbols_from_wire(&v);
        assert_eq!(symbols.len(), 1, "the row must survive; got {symbols:?}");
        assert_eq!(
            symbols[0].line, 99,
            "a float-typed but integral `line` is a REPORTED line, not the \
             `0` no-line sentinel; got {symbols:?}"
        );

        // Float-SHAPED under a type-less spec: arrives as the string "99.0",
        // which `parse::<u64>()` alone rejects.
        let typeless = concat!(
            "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:added_symbols:name|file|line\n",
            "t,widget,a.rs,99.0\n",
        );
        let v2 = munch_decode(typeless).expect("decode type-less added_symbols munch");
        let symbols2 = changed_symbols_from_wire(&v2);
        assert_eq!(symbols2.len(), 1, "the row must survive; got {symbols2:?}");
        assert_eq!(
            symbols2[0].line, 99,
            "a float-shaped string line must decode too; got {symbols2:?}"
        );

        // The widening stays BOUNDED: a value that is genuinely not a
        // non-negative whole number still falls back to the `0` sentinel
        // rather than being rounded into a plausible-looking line number.
        for bad in ["abc", "-5", "99.5"] {
            let row = json!({ "name": "widget", "file": "a.rs", "line": bad });
            assert_eq!(
                row_u64(&row, "line"),
                None,
                "{bad:?} is not a usable 1-based line number"
            );
        }
        assert_eq!(
            row_u64(&json!({ "line": -5 }), "line"),
            None,
            "a negative JSON number is not a usable 1-based line number"
        );
        assert_eq!(
            row_u64(&json!({ "line": 99.5 }), "line"),
            None,
            "a non-integral JSON number is not a usable 1-based line number"
        );
        // The boundary the `<=`-shaped guard admitted: `u64::MAX as f64`
        // rounds UP to 2^64, so `f as u64` saturated back to `u64::MAX` and
        // an unreadable value became a plausible-looking line number.
        for too_big in [json!(18446744073709551616.0_f64), json!("18446744073709551616")] {
            assert_eq!(
                row_u64(&json!({ "line": too_big }), "line"),
                None,
                "2^64 is out of `u64` range and must not saturate to u64::MAX; \
                 got a value from {too_big}"
            );
        }
        // The largest value that IS representable still reads exactly, so
        // the strict bound narrows nothing real.
        assert_eq!(
            row_u64(&json!({ "line": u64::MAX }), "line"),
            Some(u64::MAX),
            "u64::MAX itself is a readable JSON number"
        );
    }

    /// The other half of the same tolerance contract: `line` is OPTIONAL.
    /// `name`/`file` are what make a symbol locatable at all, so a row
    /// missing either is correctly dropped — but a wire that simply stops
    /// emitting a `line` column (the `find_references` grammar already
    /// does exactly that; see
    /// `find_references_from_wire_decodes_line_less_real_wire_rows`) must
    /// under-report the LOCATION, not delete the symbol from the P1 sweep.
    /// `0` is the documented "not reported" sentinel and is already
    /// handled downstream by `decl_line_out_of_range` /
    /// `stale_decl_line_diagnostic`.
    #[test]
    fn changed_symbols_from_wire_keeps_a_symbol_whose_line_column_is_absent() {
        let munch = concat!(
            "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:added_symbols:name|file:str|str\n",
            "t,widget,a.rs\n",
        );
        let v = munch_decode(munch).expect("decode line-less added_symbols munch");
        let symbols = changed_symbols_from_wire(&v);
        assert_eq!(
            symbols.len(),
            1,
            "an absent `line` column must not delete the symbol; got {symbols:?}"
        );
        assert_eq!(symbols[0].name, "widget");
        assert_eq!(symbols[0].file, "a.rs");
        assert_eq!(
            symbols[0].line, 0,
            "an unreported line decodes to the 0 sentinel"
        );
        assert!(
            decl_line_out_of_range(symbols[0].line, 500),
            "the 0 sentinel must still be treated as unlocatable downstream"
        );
    }

    /// The `dead_symbols` sibling of
    /// [`changed_symbols_from_wire_keeps_a_symbol_whose_line_column_is_absent`]:
    /// a wire that stops emitting the `line` column must cost PDEAD the
    /// symbol's LOCATION, never the symbol. A mandatory read here would drop
    /// every row inside `filter_map` on that drift and report a clean corpus
    /// — the silently-empty result this module exists to prevent, and the
    /// exact drift `find_references` already shipped in 1.108.54.
    #[test]
    fn dead_symbols_from_wire_keeps_a_symbol_whose_line_column_is_absent() {
        let munch = concat!(
            "#MUNCH/1 tool=get_dead_code_v2 enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:dead_symbols:id|name|kind|file|confidence|signals:\
             str|str|str|str|float|str\n",
            "t,a.rs::widget#function,widget,function,a.rs,0.93,['no_callers']\n",
        );
        let v = munch_decode(munch).expect("decode line-less dead_symbols munch");
        let symbols = dead_symbols_from_wire(&v);
        assert_eq!(
            symbols.len(),
            1,
            "an absent `line` column must not delete the dead symbol; got {symbols:?}"
        );
        assert_eq!(symbols[0].name, "widget");
        assert_eq!(symbols[0].file, "a.rs");
        assert_eq!(symbols[0].confidence, 0.93);
        assert_eq!(
            symbols[0].line, 0,
            "an unreported line decodes to the same 0 sentinel `ChangedSymbol` \
             and `SymbolReference` use"
        );
    }

    /// Companion to
    /// [`changed_symbols_from_wire_tolerates_a_string_encoded_line_under_a_typeless_spec`]
    /// for the FLOAT half of the same second-order defect. Both captured
    /// fixtures type `confidence` as `float`
    /// (`t:dead_symbols:id|name|kind|file|line|confidence|signals:str|str|str|str|int|float|str`),
    /// but under the 3-segment (type-less) spec `parse_one_table_spec` now
    /// ACCEPTS, `coerce_value` yields `Value::String("0.93")` — and a bare
    /// `row.get("confidence")?.as_f64()?` inside `filter_map` silently drops
    /// every PDEAD row rather than erroring loudly.
    #[test]
    fn dead_symbols_from_wire_tolerates_a_string_encoded_confidence_under_a_typeless_spec() {
        let munch = concat!(
            "#MUNCH/1 tool=get_dead_code_v2 enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:dead_symbols:id|name|kind|file|line|confidence|signals\n",
            "t,a.rs::widget#function,widget,function,a.rs,99,0.93,['no_callers']\n",
        );
        let v = munch_decode(munch).expect("decode type-less dead_symbols munch");
        let symbols = dead_symbols_from_wire(&v);
        assert_eq!(
            symbols.len(),
            1,
            "a string-encoded confidence must not cause the row to be dropped; got {symbols:?}"
        );
        assert_eq!(symbols[0].id, "a.rs::widget#function");
        assert_eq!(symbols[0].name, "widget");
        assert_eq!(symbols[0].kind, "function");
        assert_eq!(symbols[0].file, "a.rs");
        assert_eq!(symbols[0].line, 99, "the int column must stay intact too");
        assert!(
            (symbols[0].confidence - 0.93).abs() < 1e-9,
            "confidence should round-trip out of the string form; got {}",
            symbols[0].confidence
        );
        assert_eq!(symbols[0].signals, vec!["no_callers"]);
    }

    /// The PUNTESTED half of the same defect: `get_untested_symbols`'s
    /// captured fixture types `confidence` as `float` too
    /// (`t:symbols:symbol_id|name|kind|file|line|confidence|reason:...|float|str`),
    /// so a type-less spec drops every untested row the same way.
    #[test]
    fn untested_symbols_from_wire_tolerates_a_string_encoded_confidence_under_a_typeless_spec() {
        let munch = concat!(
            "#MUNCH/1 tool=get_untested_symbols enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:symbols:symbol_id|name|kind|file|line|confidence|reason\n",
            "t,a.rs::widget#function,widget,function,a.rs,99,0.93,unreached\n",
        );
        let v = munch_decode(munch).expect("decode type-less symbols munch");
        let symbols = untested_symbols_from_wire(&v);
        assert_eq!(
            symbols.len(),
            1,
            "a string-encoded confidence must not cause the row to be dropped; got {symbols:?}"
        );
        assert_eq!(symbols[0].symbol_id, "a.rs::widget#function");
        assert_eq!(symbols[0].name, "widget");
        assert_eq!(symbols[0].file, "a.rs");
        assert!(
            !symbols[0].reached,
            "reason \"unreached\" must derive reached == false"
        );
        assert!(
            (symbols[0].confidence - 0.93).abs() < 1e-9,
            "confidence should round-trip out of the string form; got {}",
            symbols[0].confidence
        );
    }

    /// `row_u64` and `row_f64` are documented counterparts, so the same
    /// string spelling must read alike through both. Rust's
    /// `f64::from_str` rejects surrounding whitespace outright, so an
    /// untrimmed float reader turns a padded `confidence` into ABSENT and
    /// drops the whole PDEAD/PUNTESTED row, while a padded `line` decodes
    /// fine — an asymmetry that costs a record rather than a field.
    #[test]
    fn the_numeric_readers_treat_a_whitespace_padded_string_alike() {
        let padded = json!({ "line": " 99 ", "confidence": " 0.93 " });
        assert_eq!(row_u64(&padded, "line"), Some(99));
        let confidence = row_f64(&padded, "confidence").expect(
            "a padded confidence must not read as absent — `confidence` is \
             mandatory, so absent drops the whole row",
        );
        assert!((confidence - 0.93).abs() < 1e-9, "got {confidence}");

        // Padding is the ONLY thing forgiven: a genuinely non-numeric
        // value still reads as absent through both.
        let junk = json!({ "line": "9 9", "confidence": "0.9 3" });
        assert_eq!(row_u64(&junk, "line"), None);
        assert_eq!(row_f64(&junk, "confidence"), None);
    }

    // ------------------------------------------------------------------
    // amendment: the MANDATORY half of the mandatory/optional contract
    // ------------------------------------------------------------------

    /// The tolerance tests above all pin ONE direction of the
    /// mandatory/optional contract — "an absent or string-encoded OPTIONAL
    /// column must not drop the row". The complementary claim
    /// (`dead_symbols_from_wire`'s doc: "`confidence` stays mandatory — it
    /// is the value `min_confidence` filters on, so a default would
    /// silently change which rows survive") lived only in prose, so a later
    /// change that followed `line`'s precedent and added
    /// `.unwrap_or_default()` to it would have left the whole suite green
    /// while silently changing which rows PDEAD reports.
    ///
    /// Also pins the drop's LOUDNESS: dropping the row is correct, dropping
    /// it silently is the "clean corpus" failure this module exists to
    /// prevent.
    #[test]
    fn dead_symbols_from_wire_drops_a_row_missing_the_mandatory_confidence_column() {
        let munch = concat!(
            "#MUNCH/1 tool=get_dead_code_v2 enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:dead_symbols:id|name|kind|file|line|signals\n",
            "t,a.rs::widget#function,widget,function,a.rs,99,['no_callers']\n",
        );
        let v = munch_decode(munch).expect("decode confidence-less dead_symbols munch");
        let symbols = dead_symbols_from_wire(&v);
        assert!(
            symbols.is_empty(),
            "`confidence` is MANDATORY — it is what `min_confidence` filters \
             on, so a defaulted value would silently change which rows \
             survive; got {symbols:?}"
        );
        let msg = dropped_rows_diagnostic("get_dead_code_v2", &v, "dead_symbols", symbols.len())
            .expect("a dropped row must not be silent");
        assert!(
            msg.contains("reify-audit: jcodemunch"),
            "diagnostic must carry the reify-audit: jcodemunch prefix; got: {msg}"
        );
        assert!(
            msg.contains("confidence") || msg.contains("signals"),
            "diagnostic must name the columns the row DID carry, so a rename \
             is legible; got: {msg}"
        );
    }

    /// The `added_symbols` sibling of the test above, for the other prose-only
    /// mandatory claim (`changed_symbols_from_wire`'s doc: "`name` and `file`
    /// are MANDATORY — a row without them names no locatable symbol").
    /// Defaulting `name` would inject unnamed symbols into the P1 sweep.
    #[test]
    fn changed_symbols_from_wire_drops_a_row_missing_the_mandatory_name_column() {
        let munch = concat!(
            "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:added_symbols:file|line\n",
            "t,a.rs,99\n",
        );
        let v = munch_decode(munch).expect("decode name-less added_symbols munch");
        let symbols = changed_symbols_from_wire(&v);
        assert!(
            symbols.is_empty(),
            "`name` is MANDATORY — an unnamed symbol is not locatable and must \
             not reach the P1 sweep; got {symbols:?}"
        );
        assert!(
            dropped_rows_diagnostic("get_changed_symbols", &v, "added_symbols", symbols.len())
                .is_some(),
            "a dropped row must not be silent"
        );
    }

    /// `dropped_rows_diagnostic` is the adapters' counterpart to
    /// `stale_decl_line_diagnostic`: collect, summarise into ONE line,
    /// return it for the caller to print.
    #[test]
    fn dropped_rows_diagnostic_fires_only_on_a_shortfall() {
        let decoded = json!({
            "dead_symbols": [
                { "id": "a", "name": "widget", "file": "a.rs" },
                { "id": "b", "name": "gadget", "file": "b.rs" },
            ]
        });
        assert!(
            dropped_rows_diagnostic("t", &decoded, "dead_symbols", 2).is_none(),
            "no shortfall, no diagnostic"
        );
        assert!(
            dropped_rows_diagnostic("t", &decoded, "absent_table", 0).is_none(),
            "an ABSENT table is a different condition (the adapter returns \
             an empty vec by design) and must not be reported as a drop"
        );
        assert!(
            dropped_rows_diagnostic("t", &json!({ "dead_symbols": 7 }), "dead_symbols", 0)
                .is_none(),
            "a non-array table carries no rows to have dropped"
        );
        assert!(
            dropped_rows_diagnostic("t", &json!({ "dead_symbols": [] }), "dead_symbols", 0)
                .is_none(),
            "an EMPTY table is a genuine empty answer, not a drop"
        );

        let msg = dropped_rows_diagnostic("get_dead_code_v2", &decoded, "dead_symbols", 1)
            .expect("1 of 2 kept must diagnose");
        assert!(
            msg.contains("reify-audit: jcodemunch"),
            "diagnostic must carry the reify-audit: jcodemunch prefix; got: {msg}"
        );
        assert!(
            msg.contains("get_dead_code_v2"),
            "diagnostic must name the tool whose rows were dropped; got: {msg}"
        );
        assert!(
            msg.contains("dead_symbols"),
            "diagnostic must name the table; got: {msg}"
        );
        assert!(
            mentions_count(&msg, 1) && mentions_count(&msg, 2),
            "diagnostic must name both the dropped count and the row total; got: {msg}"
        );
        assert!(
            msg.contains("id") && msg.contains("name") && msg.contains("file"),
            "diagnostic must name the columns the first row actually carries, \
             so a renamed column is legible without a second round-trip; got: {msg}"
        );
        assert_eq!(
            msg.lines().count(),
            1,
            "diagnostic must stay one line regardless of the dropped count; got: {msg:?}"
        );
    }

    // ------------------------------------------------------------------
    // step-11 / step-12: layer_violations_from_wire (both fixtures)
    // ------------------------------------------------------------------

    #[test]
    fn layer_violations_from_wire_empty_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jcodemunch/get_layer_violations.json"
        ));
        // This fixture is plain JSON (not MUNCH), returned directly
        let payload: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
        let violations = layer_violations_from_wire(&payload);
        assert_eq!(violations.len(), 0, "expected 0 violations");
    }

    #[test]
    fn layer_violations_from_wire_populated_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jcodemunch/get_layer_violations_populated.json"
        ));
        let payload: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
        let violations = layer_violations_from_wire(&payload);
        assert_eq!(violations.len(), 1, "expected 1 violation");
        let v = &violations[0];
        assert_eq!(v.from_file, "crates/reify-cli");
        assert_eq!(v.to_file, "crates/reify-kernel");
        assert!(v.rule.contains('0'), "rule should contain rule_index 0");
    }

    // ------------------------------------------------------------------
    // step-13 / step-14: extract_suppression
    // ------------------------------------------------------------------

    #[test]
    fn extract_suppression_all_flags() {
        let src = [
            "#[allow(dead_code)]",
            "#[cfg(test)]",
            "// G-allow: reason text",
            "pub fn my_fn() {}",
        ]
        .map(String::from);
        let found = extract_suppression(&src, 4)
            .expect("line 4 is in range, so the declaration was located");
        assert!(found.has_allow_dead_code, "has_allow_dead_code should be true");
        assert!(found.has_cfg_test, "has_cfg_test should be true");
        assert_eq!(found.g_allow_marker, Some("reason text".to_string()));
    }

    /// The distinction this seam exists for: "the declaration was READ and
    /// carries no opt-out" and "the declaration could not be located, so
    /// nothing was read" must be DIFFERENT answers.
    ///
    /// Under the neutral `(false, false, None)` triple they were byte-identical,
    /// and every consumer read the second as the first — reporting symbols whose
    /// authors had in fact opted out. `Some(DeclSuppression::default())` vs
    /// `None` is what keeps them apart, so the contrast is asserted directly
    /// rather than as two independent equalities that could both hold of one
    /// conflated value.
    #[test]
    fn extract_suppression_reads_a_clean_decl_but_declines_an_unlocatable_one() {
        let src = ["pub fn clean() {}"].map(String::from);
        let clean = extract_suppression(&src, 1);
        assert_eq!(
            clean,
            Some(DeclSuppression::default()),
            "an in-range declaration carrying no opt-out was READ — a positive \
             answer, not a declined one",
        );

        // Every way a declaration goes unlocatable, each distinct from `clean`.
        for (answer, cause) in [
            (extract_suppression(&src, 0), "line 0 (the wire reported none)"),
            (extract_suppression(&src, 2), "a line past lines.len() (stale index)"),
            (extract_suppression(&[], 1), "an empty lines slice"),
        ] {
            assert_eq!(answer, None, "{cause} must decline to answer");
            assert_ne!(
                answer, clean,
                "{cause} must not read as \"the author declined every opt-out\"",
            );
        }
    }

    #[test]
    fn extract_suppression_blank_g_allow_returns_none() {
        let src = ["// G-allow:", "pub fn my_fn() {}"].map(String::from);
        let found = extract_suppression(&src, 2).expect("line 2 is in range");
        assert!(
            found.g_allow_marker.is_none(),
            "blank G-allow: should not produce a marker"
        );
    }

    // ------------------------------------------------------------------
    // step-15 / step-16: filter_refs_to_file
    // ------------------------------------------------------------------

    #[test]
    fn filter_refs_to_file_keeps_matching() {
        let refs = vec![
            SymbolReference { file: "a.rs".to_string(), line: 1 },
            SymbolReference { file: "b.rs".to_string(), line: 2 },
            SymbolReference { file: "a.rs".to_string(), line: 3 },
            SymbolReference { file: "c.rs".to_string(), line: 4 },
        ];
        let filtered = filter_refs_to_file(refs, "a.rs");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].line, 1);
        assert_eq!(filtered[1].line, 3);
    }

    #[test]
    fn filter_refs_to_file_empty_input() {
        let filtered = filter_refs_to_file(Vec::new(), "a.rs");
        assert!(filtered.is_empty());
    }

    // ------------------------------------------------------------------
    // step-17 / step-18: find_references_from_wire (inline MUNCH)
    // ------------------------------------------------------------------

    #[test]
    fn find_references_from_wire_inline_munch() {
        // Build a small references-shaped MUNCH with file|line columns
        let munch = concat!(
            "#MUNCH/1 tool=find_references enc=gen1\n",
            "\n",
            "@1=src/\n",
            "\n",
            "x=1 __stypes= __tables=r:refs:file|line:str|int\n",
            "r,@1foo.rs,10\n",
            "r,@1bar.rs,20\n",
        );
        let v = munch_decode(munch).expect("decode inline refs munch");
        let selected =
            find_references_from_wire(&v).expect("the fallback scan must select `refs`");
        assert_eq!(
            selected.table, "refs",
            "the selected table travels out so the caller can diagnose against it"
        );
        let refs = selected.refs;
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].file, "src/foo.rs");
        assert_eq!(refs[0].line, 10);
        assert_eq!(refs[1].file, "src/bar.rs");
        assert_eq!(refs[1].line, 20);
    }

    // ------------------------------------------------------------------
    // step-3 / step-4: find_references_from_wire over real-wire (line-less)
    // rows, through the full production decode_tool_result route
    // ------------------------------------------------------------------

    /// The real jcodemunch-mcp 1.108.54 `find_references` wire shape (see
    /// `munch_decode_accepts_a_three_segment_table_spec_as_all_str`) carries
    /// no `line` column at all. Wraps the same MUNCH text in the JSON-RPC
    /// `result` envelope the client actually receives, so the `#MUNCH/`
    /// routing in `decode_tool_result` is exercised too, not just
    /// `munch_decode` directly.
    #[test]
    fn find_references_from_wire_decodes_line_less_real_wire_rows() {
        let munch = concat!(
            "#MUNCH/1 tool=find_references enc=gen1\n",
            "\n",
            "@1=crates/reify-audit/\n",
            "\n",
            "x=1 __stypes= __tables=r:__rows__:file|specifier|match_type\n",
            "r,@1src/jcodemunch_client.rs,crate,named\n",
            "r,@1tests/p1.rs,reify_audit,named\n",
        );
        let result = json!({
            "content": [{"type": "text", "text": munch}]
        });
        let decoded = decode_tool_result(&result).expect("decode_tool_result should succeed");
        let selected =
            find_references_from_wire(&decoded).expect("the real wire names its table `__rows__`");
        assert_eq!(selected.table, "__rows__");
        let refs = selected.refs;

        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].file, "crates/reify-audit/src/jcodemunch_client.rs");
        assert_eq!(refs[0].line, 0, "the real wire reports no line; sentinel is 0");
        assert_eq!(refs[1].file, "crates/reify-audit/tests/p1.rs");
        assert_eq!(refs[1].line, 0, "the real wire reports no line; sentinel is 0");
    }

    /// Pins that the coming relaxation widens the table selector from
    /// "carries `file` AND `line`" to "carries `file`" only — it must not
    /// start matching arbitrary tables that merely happen to have rows.
    #[test]
    fn find_references_from_wire_ignores_a_table_without_a_file_column() {
        let munch = concat!(
            "#MUNCH/1 tool=t enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:rows:path|line:str|int\n",
            "t,foo.rs,10\n",
        );
        let v = munch_decode(munch).expect("decode should succeed");
        assert!(
            find_references_from_wire(&v).is_none(),
            "a table with no `file` column must not be matched"
        );
    }

    /// The `__rows__` branch's counterpart to the test above, and the
    /// failure direction that branch could not previously express.
    ///
    /// `find_references_from_wire` treats a present `__rows__` array as
    /// AUTHORITATIVE and hands it straight to `references_from_rows`
    /// WITHOUT the `file`-column check the fallback scan applies. So a
    /// future release that renames the `file` column — exactly the drift
    /// 1.108.54 already shipped for `line` — yields a `Vec` that is
    /// byte-for-byte indistinguishable from the genuine "this symbol has
    /// zero references" answer the empty-`__rows__` case deliberately
    /// preserves. `p1_producer_orphan::check`'s
    /// `has_non_test_caller` would then evaluate false for EVERY symbol
    /// and P1 would emit a producer-orphan finding for every
    /// well-referenced public symbol in a done task — a false-POSITIVE
    /// storm, the opposite direction to the decoy-table hazard the
    /// selector's doc reasons about.
    ///
    /// Dropping the rows stays correct (a reference with no file is not
    /// something `filter_refs_to_file` can scope). Dropping them SILENTLY
    /// is not, so this pins the diagnostic rather than the empty vec alone.
    #[test]
    fn find_references_from_wire_is_loud_when_rows_carry_no_file_column() {
        let decoded = json!({
            "__rows__": [
                { "path": "crates/reify-audit/src/jcodemunch_client.rs", "match_type": "named" },
                { "path": "crates/reify-audit/tests/p1.rs", "match_type": "named" },
            ]
        });
        let selected = find_references_from_wire(&decoded)
            .expect("a present `__rows__` is authoritative however unreadable its rows");
        assert_eq!(selected.table, "__rows__");
        let refs = selected.refs;
        assert!(
            refs.is_empty(),
            "a reference with no `file` cannot be scoped by filter_refs_to_file; \
             got {refs:?}"
        );
        let msg = dropped_rows_diagnostic("find_references", &decoded, selected.table, refs.len())
            .expect(
                "a renamed `file` column must not read as a genuine zero-reference \
                 answer — that is a producer-orphan false positive for every symbol",
            );
        assert!(
            msg.contains("path") && msg.contains("match_type"),
            "diagnostic must name the columns the rows DO carry, so the rename is \
             legible; got: {msg}"
        );
        assert!(
            mentions_count(&msg, 2),
            "diagnostic must name how many rows were dropped; got: {msg}"
        );

        // The genuine zero-reference answer stays silent: an EMPTY
        // `__rows__` is the P1 producer-orphan case and must not be
        // reported as drift.
        assert!(
            dropped_rows_diagnostic("find_references", &json!({ "__rows__": [] }), "__rows__", 0)
                .is_none(),
            "an empty __rows__ is a real answer, not a dropped row"
        );
    }

    /// `serde_json`'s default `Map` (this crate does not enable the
    /// `preserve_order` feature) is a `BTreeMap`, so iterating it visits
    /// keys in ALPHABETICAL order, not wire order. `"Aux"` sorts before
    /// `"__rows__"` (`'A'` = 0x41 < `'_'` = 0x5F), so a selector that just
    /// returns the first table whose rows carry a `file` column would pick
    /// the decoy `"Aux"` table over the real `"__rows__"` table here.
    /// `find_references_from_wire` must prefer `__rows__` regardless of
    /// where it sorts.
    #[test]
    fn find_references_from_wire_prefers_rows_table_when_another_table_sorts_first() {
        let decoded = json!({
            "Aux": [
                { "file": "wrong/decoy.rs", "line": 1 },
            ],
            "__rows__": [
                { "file": "right/actual.rs", "specifier": "crate", "match_type": "named" },
            ],
        });
        let selected = find_references_from_wire(&decoded).expect("a table must be selected");
        assert_eq!(
            selected.table, "__rows__",
            "must select __rows__, not the alphabetically-earlier decoy `Aux`"
        );
        assert_eq!(selected.refs.len(), 1, "got {:?}", selected.refs);
        assert_eq!(selected.refs[0].file, "right/actual.rs");
    }

    /// The empty-array edge of the same preference. A PRESENT but EMPTY
    /// `__rows__` is the answer "this symbol has zero references" — which
    /// is exactly the case P1 exists to detect — so it must be returned as
    /// such, not treated as "no result here, keep looking". Before this
    /// fix the `__rows__` arm was additionally gated on
    /// `rows.iter().any(|r| r.get("file").is_some())`, so an empty
    /// `__rows__` fell through to the scan and any other table carrying a
    /// `file` column was handed back as the symbol's references —
    /// silently suppressing a real producer-orphan finding via
    /// `p1_producer_orphan`'s `has_non_test_caller` check.
    #[test]
    fn find_references_from_wire_empty_rows_table_beats_a_decoy_table() {
        let decoded = json!({
            "Aux": [
                { "file": "wrong/decoy.rs", "line": 1 },
            ],
            "__rows__": [],
        });
        let selected = find_references_from_wire(&decoded)
            .expect("an empty `__rows__` is still the selected table");
        assert_eq!(selected.table, "__rows__");
        assert!(
            selected.refs.is_empty(),
            "an empty __rows__ means zero references and must not fall \
             through to a decoy table; got {:?}",
            selected.refs
        );
    }

    /// The SPOT half of the drop diagnostic. `find_references_from_wire`
    /// may read a table other than `__rows__` — its fallback scan selects
    /// the first whose rows carry a `file` column — so the caller must
    /// count drops against the table the decoder ACTUALLY read. Against a
    /// hardcoded `__rows__` the count is taken over a table that is not
    /// there, `dropped_rows_diagnostic` returns `None`, and the shrunken
    /// result is silent: the producer-orphan false positive the diagnostic
    /// exists to prevent.
    #[test]
    fn find_references_from_wire_reports_drops_against_the_fallback_table() {
        let decoded = json!({
            "refs": [
                { "file": "src/good.rs" },
                { "file": 17 },
            ],
        });
        let selected =
            find_references_from_wire(&decoded).expect("the fallback scan must select `refs`");
        assert_eq!(selected.table, "refs");
        assert_eq!(
            selected.refs.len(),
            1,
            "a non-string `file` is unreadable and drops; got {:?}",
            selected.refs
        );

        assert!(
            dropped_rows_diagnostic("find_references", &decoded, "__rows__", selected.refs.len())
                .is_none(),
            "premise: a hardcoded `__rows__` names no table here, so the drop \
             would go unreported"
        );
        let msg = dropped_rows_diagnostic(
            "find_references",
            &decoded,
            selected.table,
            selected.refs.len(),
        )
        .expect("diagnosing against the selected table must surface the drop");
        assert!(
            mentions_count(&msg, 1) && msg.contains("refs"),
            "diagnostic must name the dropped count and the table read; got: {msg}"
        );
    }

    // ------------------------------------------------------------------
    // munch_decode: row field-count mismatch returns Protocol error
    // ------------------------------------------------------------------

    #[test]
    fn munch_decode_row_field_count_mismatch_returns_error() {
        // Table expects 3 columns (a|b|c) but the data row only supplies 2.
        let munch = concat!(
            "#MUNCH/1 tool=t enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:rows:a|b|c:str|str|str\n",
            "t,one,two\n", // only 2 fields, expected 3
        );
        match munch_decode(munch) {
            Err(LoadError::Protocol(msg)) => {
                assert!(
                    msg.contains("row has") && msg.contains("fields"),
                    "error should mention field count: {msg}"
                );
            }
            Ok(_) => panic!("expected Protocol error for field-count mismatch, got Ok"),
            Err(LoadError::Http(_)) => panic!("expected Protocol error, got Http"),
        }
    }

    // ------------------------------------------------------------------
    // expand_ref: unknown @N and bare '@' fallbacks
    // ------------------------------------------------------------------

    #[test]
    fn expand_ref_unknown_n_leaves_as_is() {
        let refs: HashMap<u32, String> = HashMap::new();
        // @99 is not in the ref table
        assert_eq!(expand_ref("@99suffix", &refs), "@99suffix");
    }

    #[test]
    fn expand_ref_bare_at_no_digits_leaves_as_is() {
        let refs: HashMap<u32, String> = HashMap::new();
        // '@' with no following digits should be returned verbatim
        assert_eq!(expand_ref("@", &refs), "@");
        assert_eq!(expand_ref("@abc", &refs), "@abc");
    }

    // ------------------------------------------------------------------
    // parse_signals_list: empty and quoted/unquoted variants
    // ------------------------------------------------------------------

    #[test]
    fn parse_signals_list_empty_brackets() {
        assert!(parse_signals_list("[]").is_empty());
    }

    #[test]
    fn parse_signals_list_empty_string() {
        assert!(parse_signals_list("").is_empty());
    }

    #[test]
    fn parse_signals_list_single_quoted_elements() {
        let v = parse_signals_list("['unreachable_file', 'no_callers']");
        assert_eq!(v, vec!["unreachable_file", "no_callers"]);
    }

    #[test]
    fn parse_signals_list_double_quoted_elements() {
        let v = parse_signals_list(r#"["unreachable_file", "no_callers"]"#);
        assert_eq!(v, vec!["unreachable_file", "no_callers"]);
    }

    #[test]
    fn parse_signals_list_unquoted_elements() {
        let v = parse_signals_list("[a, b, c]");
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    // ------------------------------------------------------------------
    // extract_suppression: scan stops at blank/code line between attrs
    // ------------------------------------------------------------------

    #[test]
    fn extract_suppression_stops_at_blank_line() {
        // The blank line between the attribute and the declaration means the
        // attribute is NOT contiguous — scanner should stop and not see it.
        let src = [
            "#[allow(dead_code)]",
            "",                    // blank line breaks contiguity
            "pub fn my_fn() {}",
        ]
        .map(String::from);
        let found = extract_suppression(&src, 3).expect("line 3 is in range");
        assert!(
            !found.has_allow_dead_code,
            "scanner should not cross blank line to find #[allow(dead_code)]"
        );
    }

    #[test]
    fn extract_suppression_stops_at_code_line() {
        // A non-attribute, non-comment line stops the upward scan.
        let src = [
            "#[allow(dead_code)]",
            "let x = 1;",          // code line breaks contiguity
            "#[cfg(test)]",
            "pub fn my_fn() {}",
        ]
        .map(String::from);
        let found = extract_suppression(&src, 4).expect("line 4 is in range");
        // cfg(test) is directly above the declaration — should be found.
        assert!(found.has_cfg_test, "cfg(test) is directly above the declaration");
        // allow(dead_code) is separated by a code line — should NOT be found.
        assert!(
            !found.has_allow_dead_code,
            "scanner should not cross code line to find #[allow(dead_code)]"
        );
    }

    // ------------------------------------------------------------------
    // step-6 / step-7: extract_suppression totality guard, IN-range boundary
    //
    // Pins the guard's `>` exactly, so a later widening to `>=` cannot go
    // unnoticed: the boundary `== lines.len()` (declaration on the final
    // line) is IN range and must still scan upward. The guard's OUT-of-range
    // half — line 0, past-EOF, empty slice — is pinned by
    // `extract_suppression_reads_a_clean_decl_but_declines_an_unlocatable_one`
    // above, which is also where the declined answer is contrasted against a
    // clean one.
    // ------------------------------------------------------------------

    #[test]
    fn extract_suppression_scans_upward_from_a_final_line_declaration() {
        let src = [
            "fn placeholder() {}",
            "#[allow(dead_code)]",
            "pub fn my_fn() {}",
        ]
        .map(String::from);
        let found = extract_suppression(&src, 3).expect(
            "decl_line_1based == lines.len() is IN range — the guard is strictly `>`",
        );
        assert!(
            found.has_allow_dead_code,
            "decl_line_1based == lines.len() must still scan upward for attrs"
        );
    }

    // ------------------------------------------------------------------
    // decl_line_out_of_range: the shared guard predicate
    // ------------------------------------------------------------------

    #[test]
    fn decl_line_out_of_range_boundary_cases() {
        assert!(
            decl_line_out_of_range(0, 10),
            "line 0 (not reported) is always out of range"
        );
        assert!(
            !decl_line_out_of_range(10, 10),
            "decl_line == line_count (final line) is IN range — strictly `>`, not `>=`"
        );
        assert!(
            decl_line_out_of_range(11, 10),
            "decl_line > line_count is out of range"
        );
        assert!(
            decl_line_out_of_range(1, 0),
            "any positive decl_line is out of range for a 0-line file"
        );
    }

    // ------------------------------------------------------------------
    // collect_stale_decl_lines: the enrichment loop's out-of-range
    // collection, factored out so its wiring is independently testable
    // ------------------------------------------------------------------

    #[test]
    fn collect_stale_decl_lines_only_reports_symbols_out_of_range() {
        fn sym(name: &str, file: &str, line: usize) -> ChangedSymbol {
            ChangedSymbol {
                name: name.to_string(),
                file: file.to_string(),
                line,
                suppression: None,
            }
        }
        let symbols = vec![
            sym("in_range", "a.rs", 5),   // in range for a.rs (10 lines) — excluded
            sym("zero_line", "b.rs", 0),  // line 0 — included
            sym("past_eof", "c.rs", 999), // past c.rs's 3 lines — included
            sym("unreadable", "d.rs", 1), // d.rs has no known line count — excluded
        ];
        let line_counts: HashMap<&str, usize> =
            HashMap::from([("a.rs", 10), ("b.rs", 10), ("c.rs", 3)]);
        let out_of_range =
            collect_stale_decl_lines(&symbols, |file| line_counts.get(file).copied());

        assert_eq!(
            out_of_range,
            vec![("b.rs".to_string(), 0, 10), ("c.rs".to_string(), 999, 3),],
            "must collect exactly the out-of-range symbols whose file's line count is known, \
             in symbol order; got {out_of_range:?}"
        );
    }

    // ------------------------------------------------------------------
    // decode_tool_result: MUNCH takes priority over structuredContent
    // ------------------------------------------------------------------

    #[test]
    fn decode_tool_result_munch_beats_structured_content() {
        // When both structuredContent and a MUNCH text body are present,
        // the MUNCH body should win (structuredContent lacks the table shape).
        let munch_text = concat!(
            "#MUNCH/1 tool=t enc=gen1\n",
            "\n",
            "x=1 __stypes= __tables=t:rows:val:str\n",
            "t,hello\n",
        );
        let result = serde_json::json!({
            "structuredContent": {"wrong": "shape"},
            "content": [{"type": "text", "text": munch_text}]
        });
        let decoded = decode_tool_result(&result).expect("should succeed");
        // Must have 'rows' (from MUNCH), not 'wrong' (from structuredContent)
        assert!(decoded.get("rows").is_some(), "MUNCH table 'rows' should be present");
        assert!(decoded.get("wrong").is_none(), "structuredContent 'wrong' key must not appear");
    }

    // ------------------------------------------------------------------
    // read_source_lines_for_enrichment
    // ------------------------------------------------------------------

    #[test]
    fn read_source_lines_for_enrichment_nonexistent_path() {
        use std::path::Path;
        let path = Path::new("/nonexistent/path/that/does/not/exist.rs");
        let msg = read_source_lines_for_enrichment(path)
            .expect_err("a nonexistent path must read as an ERROR, not as an empty file");
        assert!(
            msg.contains("/nonexistent/path/that/does/not/exist.rs"),
            "diagnostic must contain the path; got: {msg}"
        );
        assert!(
            msg.contains("read"),
            "diagnostic must mention 'read'; got: {msg}"
        );
    }

    #[test]
    fn read_source_lines_for_enrichment_readable_file() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::fs::write(tmp.path(), "line one\nline two\nline three\n")
            .expect("write tempfile");
        assert_eq!(
            read_source_lines_for_enrichment(tmp.path()),
            Ok(vec![
                "line one".to_string(),
                "line two".to_string(),
                "line three".to_string()
            ]),
            "a readable file must return its lines"
        );

        // An EMPTY file read fine: `Ok(vec![])`, never the `Err` a
        // failed read produces. The two used to share one `vec![]`, and
        // they have opposite consequences downstream — an unreadable file
        // is already reported, an empty one still owes the operator a
        // stale-index line for every symbol it cannot locate.
        std::fs::write(tmp.path(), "").expect("truncate tempfile");
        assert_eq!(
            read_source_lines_for_enrichment(tmp.path()),
            Ok(Vec::new()),
            "an empty file is a successful read of no lines"
        );
    }

    // ------------------------------------------------------------------
    // step-7 / step-8: stale_decl_line_diagnostic
    //
    // The operator-visible half of the step-6 fix: a symbol whose wire line
    // is out of range no longer panics (step-6), but must not silently
    // degrade into unexplained P1 orphan findings either. This pure helper
    // summarises the out-of-range symbols collected by
    // `RealJCodemunchOps::get_changed_symbols`'s enrichment loop into ONE
    // diagnostic line, mirroring the "return the diagnostic, let the
    // caller `eprintln!` it" idiom of `read_source_lines_for_enrichment`
    // above (including its `reify-audit: jcodemunch` message prefix).
    // ------------------------------------------------------------------

    /// True when `msg` names `n` as a STANDALONE number — a maximal digit
    /// run equal to `n`'s decimal form, never a substring of a longer
    /// number (so `1` is not satisfied by the `18321` already in the
    /// message).
    ///
    /// Deliberately weaker than a phrase pin like `contains("1 symbol")`:
    /// the operator-visible property is "the diagnostic reports how many
    /// symbols are affected", which survives any cosmetic reword that
    /// keeps the number ("1 affected symbol", "symbols: 1").
    fn mentions_count(msg: &str, n: usize) -> bool {
        let needle = n.to_string();
        msg.split(|c: char| !c.is_ascii_digit())
            .any(|tok| tok == needle)
    }

    #[test]
    fn stale_decl_line_diagnostic_summarises_out_of_range_symbols() {
        assert!(
            stale_decl_line_diagnostic(&[]).is_none(),
            "empty slice must produce no diagnostic on the happy path"
        );

        let one = vec![(
            "crates/reify-eval/src/engine_build.rs".to_string(),
            18321usize,
            13165usize,
        )];
        let msg = stale_decl_line_diagnostic(&one).expect("one entry must produce a diagnostic");
        assert!(
            msg.contains("reify-audit: jcodemunch"),
            "diagnostic must carry the reify-audit: jcodemunch prefix; got: {msg}"
        );
        assert!(
            mentions_count(&msg, 1),
            "diagnostic must name the affected count 1; got: {msg}"
        );
        assert!(
            msg.contains("crates/reify-eval/src/engine_build.rs"),
            "diagnostic must name the path; got: {msg}"
        );
        assert!(
            msg.contains("18321"),
            "diagnostic must name the wire line 18321; got: {msg}"
        );
        assert!(
            msg.contains("13165"),
            "diagnostic must name the file's line count 13165; got: {msg}"
        );

        let three = vec![
            (
                "crates/reify-eval/src/engine_build.rs".to_string(),
                18321,
                13165,
            ),
            ("crates/other/src/lib.rs".to_string(), 500, 100),
            ("crates/third/src/mod.rs".to_string(), 42, 10),
        ];
        let msg3 =
            stale_decl_line_diagnostic(&three).expect("three entries must produce a diagnostic");
        assert!(
            mentions_count(&msg3, 3),
            "diagnostic must name the affected count 3; got: {msg3}"
        );
        assert!(
            msg3.contains("crates/reify-eval/src/engine_build.rs"),
            "diagnostic must name only the first entry's path; got: {msg3}"
        );
        assert!(
            !msg3.contains("crates/other/src/lib.rs"),
            "diagnostic must not name the second entry's path; got: {msg3}"
        );
        assert!(
            !msg3.contains("crates/third/src/mod.rs"),
            "diagnostic must not name the third entry's path; got: {msg3}"
        );
        assert!(
            mentions_count(&msg3, 13165),
            "diagnostic must report the FIRST entry's line count; got: {msg3}"
        );
        assert!(
            !mentions_count(&msg3, 100) && !mentions_count(&msg3, 10),
            "the reported line count belongs to the named entry alone — entries \
             can span files of different lengths, so the other entries' counts \
             must not appear; got: {msg3}"
        );
        assert_eq!(
            msg3.lines().count(),
            1,
            "diagnostic must stay one line regardless of the affected count; got: {msg3:?}"
        );
    }

    /// The bucketing itself, asserted as VALUES.
    ///
    /// `decl_line_out_of_range` folds two conditions into one predicate, so
    /// the CLASSIFICATION — not just the rendering — is real behaviour and
    /// needs its own pin. Pinning it only through the rendered message's
    /// prose (as this test's sibling below once did on its own) is brittle
    /// in both directions: a behaviour-neutral reword reds the suite, and a
    /// genuine mis-bucketing that happens to keep both remedy phrases
    /// present goes undetected.
    #[test]
    fn partition_stale_decl_lines_buckets_by_cause() {
        assert_eq!(stale_cause(0), StaleCause::NoLineReported);
        assert_eq!(
            stale_cause(1),
            StaleCause::PastEof,
            "any line the wire actually reported is past-EOF by the time it \
             reaches here — `decl_line_out_of_range` already excluded the \
             in-range ones"
        );
        assert_eq!(stale_cause(18321), StaleCause::PastEof);

        let (past_eof, no_line) = partition_stale_decl_lines(&[]);
        assert!(past_eof.is_empty() && no_line.is_empty());

        let mixed: Vec<StaleDeclLine> = vec![
            ("crates/some/src/lib.rs".to_string(), 0, 200),
            ("crates/other/src/lib.rs".to_string(), 500, 100),
            ("crates/third/src/mod.rs".to_string(), 0, 42),
            ("crates/fourth/src/mod.rs".to_string(), 9, 8),
        ];
        let (past_eof, no_line) = partition_stale_decl_lines(&mixed);
        assert_eq!(
            past_eof
                .iter()
                .map(|(f, _, _)| f.as_str())
                .collect::<Vec<_>>(),
            vec!["crates/other/src/lib.rs", "crates/fourth/src/mod.rs"],
            "past-EOF bucket must hold exactly the entries with a reported line, \
             in input order"
        );
        assert_eq!(
            no_line
                .iter()
                .map(|(f, _, _)| f.as_str())
                .collect::<Vec<_>>(),
            vec!["crates/some/src/lib.rs", "crates/third/src/mod.rs"],
            "no-line bucket must hold exactly the `0`-sentinel entries, in input order"
        );
        assert_eq!(
            past_eof.len() + no_line.len(),
            mixed.len(),
            "the partition must be total — an entry that fell out of both buckets \
             would be an out-of-range symbol reported to nobody"
        );
    }

    /// The two [`StaleCause`]s must carry different REMEDIES: a line past
    /// EOF means the index is stale (re-index), a line of `0` means the wire
    /// reported no usable one (a grammar drift — re-indexing changes
    /// nothing). Without this, `changed_symbols_from_wire`'s `unwrap_or(0)`
    /// routes a future jcodemunch that drops `line` from
    /// `get_changed_symbols` — as it already did for `find_references` —
    /// into telling the operator to re-index a perfectly current repo, once
    /// per symbol's worth of volume.
    ///
    /// Membership and counts are pinned structurally by
    /// [`partition_stale_decl_lines`]; what is left here is the WIRING —
    /// that each bucket's clause carries its OWN bucket's remedy — with one
    /// loose prose anchor per bucket. In a single-bucket message that
    /// positive assertion already catches a swap, since the other bucket's
    /// anchor would be the one present.
    #[test]
    fn stale_decl_line_diagnostic_splits_its_remedy_by_cause() {
        // `wire_line == 0` — the no-line sentinel — on its own.
        let no_line: Vec<StaleDeclLine> = vec![("crates/some/src/lib.rs".to_string(), 0, 200)];
        let msg = stale_decl_line_diagnostic(&no_line).expect("a 0 entry must diagnose");
        assert!(
            msg.contains("`line` column"),
            "the 0 sentinel's clause must carry the grammar-drift remedy, not the \
             stale-index one; got: {msg}"
        );
        assert!(
            msg.contains("reify-audit: jcodemunch"),
            "diagnostic must carry the reify-audit: jcodemunch prefix; got: {msg}"
        );
        assert!(
            msg.contains("crates/some/src/lib.rs"),
            "diagnostic must name the path; got: {msg}"
        );
        assert!(
            mentions_count(&msg, 0),
            "diagnostic must still report the wire line it read (0); got: {msg}"
        );

        // Past EOF on its own.
        let past_eof: Vec<StaleDeclLine> = vec![("crates/other/src/lib.rs".to_string(), 500, 100)];
        let msg_eof =
            stale_decl_line_diagnostic(&past_eof).expect("a past-EOF entry must diagnose");
        assert!(
            msg_eof.contains("re-index"),
            "a line past EOF is exactly the stale-index case; got: {msg_eof}"
        );

        // Mixed: both causes present at once must both be reported, each
        // with its own count and first entry, still on ONE line.
        let mixed: Vec<StaleDeclLine> = vec![
            ("crates/some/src/lib.rs".to_string(), 0, 200),
            ("crates/other/src/lib.rs".to_string(), 500, 100),
            ("crates/third/src/mod.rs".to_string(), 0, 42),
        ];
        let msg_mixed = stale_decl_line_diagnostic(&mixed).expect("mixed entries must diagnose");
        assert!(
            msg_mixed.contains("re-index") && msg_mixed.contains("`line` column"),
            "both causes must be reported when both are present; got: {msg_mixed}"
        );
        let (eof_bucket, no_line_bucket) = partition_stale_decl_lines(&mixed);
        assert!(
            mentions_count(&msg_mixed, no_line_bucket.len()),
            "the no-line bucket's own size must appear; got: {msg_mixed}"
        );
        assert!(
            msg_mixed.contains(no_line_bucket[0].0.as_str())
                && msg_mixed.contains(eof_bucket[0].0.as_str()),
            "each bucket must name its OWN first entry; got: {msg_mixed}"
        );
        assert!(
            !msg_mixed.contains(no_line_bucket[1].0.as_str()),
            "only each bucket's first entry is named; got: {msg_mixed}"
        );
        assert_eq!(
            msg_mixed.lines().count(),
            1,
            "splitting the remedy must not split the line; got: {msg_mixed:?}"
        );
    }

    /// A declaring file that reads FINE but is EMPTY — truncated by the very
    /// refactor that made the index stale — leaves every one of its symbols
    /// unlocatable, and must be summarised rather than silently skipped.
    ///
    /// Its sibling — a file that could not be READ at all — must stay OUT
    /// of that summary: it already has its own per-path diagnostic, and
    /// counting it here too double-reports it. Keeping the two apart is
    /// what [`read_source_lines_for_enrichment`]'s `Result` is for; a
    /// single empty `Vec` for both leaves zero operator output on the input
    /// shape most likely to BE a stale index.
    #[test]
    fn enrich_suppression_flags_reports_a_readable_but_empty_declaring_file() {
        fn sym(file: &str, line: usize) -> ChangedSymbol {
            ChangedSymbol {
                name: "widget".to_string(),
                file: file.to_string(),
                line,
                suppression: None,
            }
        }

        let tmp = tempfile::TempDir::new().expect("create tempdir");
        std::fs::write(tmp.path().join("empty.rs"), "").expect("write empty.rs");

        let mut symbols = vec![sym("empty.rs", 7)];
        let msg = enrich_suppression_flags(&mut symbols, tmp.path()).expect(
            "a readable-but-empty declaring file leaves its symbols unlocatable — \
             that must be reported, not silently skipped",
        );
        assert!(
            msg.contains("empty.rs"),
            "diagnostic must name the empty declaring file; got: {msg}"
        );
        assert_eq!(
            symbols[0].suppression, None,
            "nothing is locatable in a 0-line file — the declaration was never \
             found, so suppression stays UNKNOWN; got {:?}",
            symbols[0]
        );

        // The sibling state must stay OUT of the summary: a file that could
        // not be read at all is already reported once per path by
        // `read_source_lines_for_enrichment`, so counting it here too would
        // double-report it.
        let mut unreadable = vec![sym("does-not-exist.rs", 7)];
        assert!(
            enrich_suppression_flags(&mut unreadable, tmp.path()).is_none(),
            "an unreadable file has its own per-path diagnostic and must not \
             also appear in the stale-index summary"
        );
    }

    /// The third way a declaration goes unlocatable, and the one #6447's
    /// KNOWN LIMITATION did not even name: the declaring file could not be
    /// READ at all, so `enrich_suppression_flags` never enters its `Ok` arm
    /// and nothing was scanned.
    ///
    /// The sibling above pins the OPERATOR-facing half of this state (an
    /// unreadable file stays out of the stale-index summary because it has
    /// its own per-path diagnostic). This pins the DETECTOR-facing half: what
    /// the symbol itself carries afterwards must be "unknown", never
    /// "checked, carries nothing" — the latter is what turns an unreadable
    /// file into a false-positive orphan for every symbol it declares.
    #[test]
    fn enrich_suppression_flags_leaves_suppression_unknown_for_an_unreadable_declaring_file() {
        let tmp = tempfile::TempDir::new().expect("create tempdir");
        // Deliberately never written: the path does not exist under the root.
        let mut symbols = vec![ChangedSymbol {
            name: "widget".to_string(),
            file: "does-not-exist.rs".to_string(),
            line: 7,
            suppression: None,
        }];

        let _ = enrich_suppression_flags(&mut symbols, tmp.path());

        assert_eq!(
            symbols[0].suppression, None,
            "a declaring file that could not be read leaves suppression \
             UNKNOWN — enrichment scanned nothing, so it may not report that \
             the author declined every opt-out; got {:?}",
            symbols[0]
        );
        assert!(
            !symbols[0].decl_located(),
            "decl_located() is the accessor a detector branches on; got {:?}",
            symbols[0]
        );
    }

    // ------------------------------------------------------------------
    // MCP session-handshake contract
    // ------------------------------------------------------------------
    //
    // A hermetic loopback recording stub plus the assertions that pin the
    // streamable-HTTP session lifecycle: the server assigns the session id,
    // the client never mints one.
    //
    // The stub copies `tests/cli.rs`'s `spawn_mock_mcp_on` discipline —
    // one request per connection (`Connection: close`, so `ureq` reconnects
    // predictably) and a stop-flag + non-blocking accept poll teardown, so
    // a failing assertion cannot leak the accept thread. The one addition
    // is that it records each request's HEADERS alongside its body, which
    // is exactly what `tests/cli.rs`'s version discards and what these
    // contract assertions need.
    mod session_contract {
        use super::*;

        use std::io::{BufRead, BufReader, Write};
        use std::net::{SocketAddr, TcpListener, TcpStream};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};
        use std::thread;

        /// The session id the stub hands out in its `initialize` response.
        ///
        /// Deliberately NOT 32 lowercase hex: a client-minted id (the bug
        /// under test) can then never accidentally satisfy an equality
        /// assertion against it.
        const ASSIGNED_SESSION: &str = "srv-assigned-0001";

        /// How the stub answers `initialize` — the one axis these tests
        /// vary. Everything after the handshake is identical across modes.
        #[derive(Clone, Copy)]
        enum InitializeReply {
            /// A healthy serve: 200, a well-formed JSON-RPC result body,
            /// and an `Mcp-Session-Id` header.
            WithSession,
            /// 200 and the same well-formed body, but NO session header —
            /// the server never assigned a session.
            NoSessionHeader,
            /// 202 with an empty body and no session header. The shape a
            /// notification-only responder produces.
            AcceptedEmpty,
        }

        /// How the stub answers `tools/list` — the second axis these tests
        /// vary, and the one [`JcodemunchClient::list_tools`] decodes.
        ///
        /// Orthogonal to [`InitializeReply`]: every mode here is reached
        /// only after a healthy [`InitializeReply::WithSession`] handshake,
        /// so a `list_tools` failure can be attributed to the `tools/list`
        /// reply alone.
        #[derive(Clone, Copy)]
        enum ToolsListReply {
            /// A well-formed `result.tools` array advertising exactly these
            /// names, in this order.
            Names(&'static [&'static str]),
            /// A well-formed JSON-RPC result carrying no `tools` key at all.
            NoToolsKey,
            /// `result.tools` present, but an object rather than an array.
            ToolsNotAnArray,
            /// A well-formed array whose middle entry carries no `name`.
            EntryWithoutName,
        }

        impl ToolsListReply {
            /// The `result` object this mode puts in its JSON-RPC reply.
            fn result(self) -> Value {
                match self {
                    Self::Names(names) => json!({
                        "tools": names
                            .iter()
                            .map(|n| json!({"name": n, "description": "stub tool"}))
                            .collect::<Vec<_>>(),
                    }),
                    Self::NoToolsKey => json!({"nextCursor": Value::Null}),
                    Self::ToolsNotAnArray => json!({"tools": {"name": "not-an-array"}}),
                    Self::EntryWithoutName => json!({
                        "tools": [
                            {"name": "get_layer_violations"},
                            {"description": "an entry with no name at all"},
                            {"name": "find_references"},
                        ],
                    }),
                }
            }
        }

        /// How the stub answers `tools/call` — the third axis, dispatched
        /// on the recorded request's `params.name` so different tools can
        /// be answered differently within one session.
        ///
        /// Orthogonal to [`InitializeReply`] and [`ToolsListReply`]: reached
        /// only after a healthy handshake, independent of the `tools/list`
        /// advertisement.
        #[derive(Clone, Copy)]
        enum ToolCallReply {
            /// No tool-call bodies configured — every `tools/call` gets the
            /// inert `200 {}` reply with no session header, exactly what
            /// every mode answered before this axis existed. What `start()`
            /// / `start_with()` / `start_with_tools()` use, so their wire
            /// behaviour is unchanged by this axis's addition.
            Inert,
            /// `name` → MUNCH body pairs. A `tools/call` whose `params.name`
            /// matches an entry is answered
            /// `{"result":{"content":[{"type":"text","text":<munch>}]}}`,
            /// replaying `ASSIGNED_SESSION`. A name with no matching entry
            /// falls back to the same inert reply as `Inert`.
            Munch(&'static [(&'static str, &'static str)]),
        }

        /// One recorded request: lowercased header names → values, plus the
        /// parsed JSON body.
        #[derive(Clone, Debug)]
        struct Recorded {
            headers: HashMap<String, String>,
            body: Value,
        }

        impl Recorded {
            fn method(&self) -> &str {
                self.body.get("method").and_then(|m| m.as_str()).unwrap_or("")
            }

            fn session_header(&self) -> Option<&str> {
                self.headers.get("mcp-session-id").map(|s| s.as_str())
            }
        }

        /// Read one complete HTTP/1.1 request, returning its lowercased
        /// headers and parsed JSON body. Assumes `Content-Length` is
        /// present, which `ureq`'s `send_json` always sets.
        fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
            let mut reader = BufReader::new(stream.try_clone().ok()?);
            // Request line: read and discard. The stub serves every path.
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).ok()? == 0 {
                return None;
            }
            let mut headers: HashMap<String, String> = HashMap::new();
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).ok()? == 0 {
                    return None;
                }
                if line == "\r\n" || line == "\n" {
                    break;
                }
                let (name, value) = line.split_once(':')?;
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name == "content-length" {
                    content_length = value.parse().ok()?;
                }
                headers.insert(name, value);
            }
            let body = if content_length == 0 {
                Value::Null
            } else {
                let mut buf = vec![0u8; content_length];
                reader.read_exact(&mut buf).ok()?;
                serde_json::from_slice(&buf).ok()?
            };
            Some(Recorded { headers, body })
        }

        fn write_response(
            stream: &mut TcpStream,
            status: u16,
            session: Option<&str>,
            body: &[u8],
        ) {
            let status_text = match status {
                202 => "Accepted",
                _ => "OK",
            };
            let mut head = format!(
                "HTTP/1.1 {status} {status_text}\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n",
                body.len()
            );
            if let Some(session) = session {
                // Mixed case on purpose. HTTP header names are
                // case-insensitive and a real jcodemunch serve emits this
                // one lowercase, so the client must not match on case.
                head.push_str(&format!("Mcp-Session-Id: {session}\r\n"));
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }

        /// A recording MCP stub on loopback. Serves every path.
        struct RecordingStub {
            url: String,
            addr: SocketAddr,
            stop: Arc<AtomicBool>,
            requests: Arc<Mutex<Vec<Recorded>>>,
            handle: Option<thread::JoinHandle<()>>,
        }

        impl RecordingStub {
            /// A healthy stub: `initialize` answers 200 with a session id.
            fn start() -> Self {
                Self::start_with(InitializeReply::WithSession)
            }

            /// Vary only the handshake. `tools/list` is never called by
            /// these tests, so its reply is an inert empty advertisement,
            /// and `tools/call` is inert too.
            fn start_with(reply: InitializeReply) -> Self {
                Self::start_full(reply, ToolsListReply::Names(&[]), ToolCallReply::Inert)
            }

            /// Vary only the `tools/list` reply, behind a healthy handshake.
            /// `tools/call` stays inert.
            fn start_with_tools(tools_reply: ToolsListReply) -> Self {
                Self::start_full(InitializeReply::WithSession, tools_reply, ToolCallReply::Inert)
            }

            /// Vary only the `tools/call` reply, behind a healthy handshake
            /// and an inert (empty) `tools/list` advertisement — `list_tools`
            /// is never called by these tests.
            fn start_with_tool_calls(tool_call_reply: ToolCallReply) -> Self {
                Self::start_full(
                    InitializeReply::WithSession,
                    ToolsListReply::Names(&[]),
                    tool_call_reply,
                )
            }

            /// Bind an ephemeral loopback port and start serving, answering
            /// `initialize` per `reply`, `tools/list` per `tools_reply`, and
            /// `tools/call` per `tool_call_reply`. The listener is bound
            /// once and kept — never dropped and re-bound — so nothing else
            /// can win the port in between.
            fn start_full(
                reply: InitializeReply,
                tools_reply: ToolsListReply,
                tool_call_reply: ToolCallReply,
            ) -> Self {
                let listener =
                    TcpListener::bind("127.0.0.1:0").expect("bind loopback stub");
                let addr = listener.local_addr().expect("stub local_addr");
                // No trailing slash: a real jcodemunch serve 307-redirects
                // `/mcp/` and drops `mcp-session-id` on the way.
                let url = format!("http://127.0.0.1:{}/mcp", addr.port());
                listener
                    .set_nonblocking(true)
                    .expect("set_nonblocking on stub listener");

                let stop = Arc::new(AtomicBool::new(false));
                let requests: Arc<Mutex<Vec<Recorded>>> = Arc::new(Mutex::new(Vec::new()));
                let stop_thread = Arc::clone(&stop);
                let requests_thread = Arc::clone(&requests);

                let handle = thread::spawn(move || loop {
                    if stop_thread.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut stream = match listener.accept() {
                        Ok((s, _)) => s,
                        Err(_) => {
                            // WouldBlock (the common case) and any other
                            // transient error both back off, so the loop
                            // can never peg a CPU nor miss the stop flag.
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    };
                    // Restore blocking semantics for BufReader, but bound
                    // the read so a stalled peer can't wedge the join.
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let recorded = match read_request(&mut stream) {
                        Some(r) => r,
                        None => continue,
                    };
                    let req_id = recorded.body.get("id").cloned().unwrap_or(Value::Null);
                    let method = recorded.method().to_string();
                    // Extract the tool-call name before `recorded` is moved
                    // into the request log below.
                    let tool_call_name = recorded
                        .body
                        .get("params")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    requests_thread
                        .lock()
                        .expect("stub request log")
                        .push(recorded);

                    match method.as_str() {
                        "initialize" => {
                            let body = json!({
                                "jsonrpc": "2.0",
                                "id": req_id,
                                "result": {
                                    "protocolVersion": PROTOCOL_VERSION,
                                    "capabilities": {},
                                    "serverInfo": {
                                        "name": "recording-stub",
                                        "version": "0.1",
                                    },
                                },
                            })
                            .to_string();
                            match reply {
                                InitializeReply::WithSession => write_response(
                                    &mut stream,
                                    200,
                                    Some(ASSIGNED_SESSION),
                                    body.as_bytes(),
                                ),
                                InitializeReply::NoSessionHeader => {
                                    write_response(&mut stream, 200, None, body.as_bytes())
                                }
                                InitializeReply::AcceptedEmpty => {
                                    write_response(&mut stream, 202, None, b"")
                                }
                            }
                        }
                        // A real serve answers this notification 202 with
                        // an empty body — keep that shape.
                        "notifications/initialized" => {
                            write_response(&mut stream, 202, None, b"")
                        }
                        "tools/list" => {
                            let body = json!({
                                "jsonrpc": "2.0",
                                "id": req_id,
                                "result": tools_reply.result(),
                            })
                            .to_string();
                            write_response(&mut stream, 200, None, body.as_bytes())
                        }
                        "tools/call" => {
                            let munch = match tool_call_reply {
                                ToolCallReply::Munch(pairs) => pairs
                                    .iter()
                                    .find(|(n, _)| *n == tool_call_name)
                                    .map(|(_, m)| *m),
                                ToolCallReply::Inert => None,
                            };
                            match munch {
                                Some(munch) => {
                                    let body = json!({
                                        "jsonrpc": "2.0",
                                        "id": req_id,
                                        "result": {
                                            "content": [{"type": "text", "text": munch}],
                                        },
                                    })
                                    .to_string();
                                    write_response(
                                        &mut stream,
                                        200,
                                        Some(ASSIGNED_SESSION),
                                        body.as_bytes(),
                                    )
                                }
                                // No body configured for this tool name —
                                // same inert reply every mode used before
                                // this axis existed.
                                None => write_response(&mut stream, 200, None, b"{}"),
                            }
                        }
                        _ => write_response(&mut stream, 200, None, b"{}"),
                    }
                });

                Self {
                    url,
                    addr,
                    stop,
                    requests,
                    handle: Some(handle),
                }
            }

            fn url(&self) -> &str {
                &self.url
            }

            /// Every request recorded so far, in arrival order.
            fn requests(&self) -> Vec<Recorded> {
                self.requests.lock().expect("stub request log").clone()
            }

            /// The one recorded request whose JSON-RPC `method` is
            /// `method`. Panics unless there is exactly one — zero or
            /// several would make the caller's assertion vacuous or
            /// ambiguous rather than wrong.
            fn request_for(&self, method: &str) -> Recorded {
                let mut matches: Vec<Recorded> = self
                    .requests()
                    .into_iter()
                    .filter(|r| r.method() == method)
                    .collect();
                assert_eq!(
                    matches.len(),
                    1,
                    "expected exactly one recorded `{method}` request; recorded: {:?}",
                    self.requests()
                        .iter()
                        .map(|r| r.method().to_string())
                        .collect::<Vec<_>>(),
                );
                matches.pop().expect("checked len == 1")
            }
        }

        impl Drop for RecordingStub {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Relaxed);
                // Best-effort wakeup; the non-blocking accept poll is the
                // safety net, so this failing cannot hang the join.
                let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
                if let Some(handle) = self.handle.take() {
                    let _ = handle.join();
                }
            }
        }

        /// Contract item 1: the `initialize` POST carries NO
        /// `mcp-session-id` request header.
        ///
        /// In MCP streamable-HTTP the session is assigned by the server on
        /// `initialize`. A client-minted id makes a real jcodemunch serve
        /// answer `404 Invalid or expired session ID`, which `post` maps to
        /// `LoadError::Http` and `reify-audit` fail-softs into a no-op —
        /// silently, which is how the seam stayed dead for ten weeks.
        #[test]
        fn initialize_carries_no_client_minted_session_header() {
            let stub = RecordingStub::start();

            let client = JcodemunchClient::new(stub.url());
            assert!(
                client.is_ok(),
                "handshake against the recording stub must succeed; got {:?}",
                client.err(),
            );

            let requests = stub.requests();
            let first = requests.first().expect("stub recorded no request at all");
            assert_eq!(
                first.method(),
                "initialize",
                "the handshake's first POST must be `initialize`",
            );
            assert_eq!(
                first.session_header(),
                None,
                "`initialize` must carry no mcp-session-id request header — \
                 the server assigns the session, the client never mints one",
            );
        }

        /// Contract items 2 and 3: the client stores the `Mcp-Session-Id`
        /// the server returned on `initialize` and replays exactly that
        /// value on every subsequent POST.
        ///
        /// Asserted behaviourally — on the wire — rather than through a
        /// getter, so the lock costs no new public surface.
        #[test]
        fn server_assigned_session_id_is_replayed_on_every_later_post() {
            let stub = RecordingStub::start();
            let client = JcodemunchClient::new(stub.url()).expect("handshake");

            // The stub's fallback arm answers `200 {}`, so the decoded
            // result is irrelevant here — the claim is about the request
            // headers this call puts on the wire.
            let _ = client.call_tool("get_layer_violations", json!({}));

            assert_eq!(
                stub.request_for("notifications/initialized").session_header(),
                Some(ASSIGNED_SESSION),
                "`notifications/initialized` must replay the server-assigned \
                 session id verbatim",
            );
            assert_eq!(
                stub.request_for("tools/call").session_header(),
                Some(ASSIGNED_SESSION),
                "every post-handshake call must replay the server-assigned \
                 session id verbatim",
            );
        }

        /// α's share of contract item 5: a responder that assigns no
        /// session id is not a live seam, so the handshake must fail.
        ///
        /// Without an assigned id, contract item 3 (replay it on every
        /// later POST) is unsatisfiable by construction — there is nothing
        /// to replay. Returning `Ok` here would hand back a client whose
        /// every subsequent call is answered `400 Missing session ID`,
        /// which is exactly the PASS-shaped nothing this task exists to
        /// remove.
        ///
        /// The residual hole — a response that DOES carry a session id but
        /// whose body is 202/empty/not an initialize result — belongs to
        /// task #5832 and is deliberately not asserted here.
        #[test]
        fn initialize_without_an_assigned_session_id_is_a_protocol_error() {
            let stub = RecordingStub::start_with(InitializeReply::NoSessionHeader);

            match JcodemunchClient::new(stub.url()) {
                Err(LoadError::Protocol(_)) => {} // expected
                Ok(_) => panic!(
                    "expected Protocol error when the initialize response \
                     carries no Mcp-Session-Id header, got Ok — a client with \
                     no session id fails every later call",
                ),
                Err(LoadError::Http(e)) => panic!(
                    "expected Protocol error for the missing session id, got \
                     Http error: {e}",
                ),
            }
        }

        /// The same claim through the 202 shape: an `initialize` answered
        /// `202` with an empty body and no session header assigns nothing,
        /// so it too must fail rather than yield a session-less client.
        #[test]
        fn initialize_answered_202_without_a_session_id_is_a_protocol_error() {
            let stub = RecordingStub::start_with(InitializeReply::AcceptedEmpty);

            match JcodemunchClient::new(stub.url()) {
                Err(LoadError::Protocol(_)) => {} // expected
                Ok(_) => panic!(
                    "expected Protocol error when initialize is answered 202 \
                     with no Mcp-Session-Id header, got Ok",
                ),
                Err(LoadError::Http(e)) => panic!(
                    "expected Protocol error for the 202-without-session-id \
                     handshake, got Http error: {e}",
                ),
            }
        }

        /// `list_tools` reports exactly what the server advertised — every
        /// name, in arrival order, nothing added or dropped — and does so
        /// on the established session (contract item 3).
        #[test]
        fn list_tools_reports_every_advertised_name_in_order() {
            const ADVERTISED: &[&str] = &[
                "get_changed_symbols",
                "find_references",
                "get_dead_code_v2",
            ];
            let stub = RecordingStub::start_with_tools(ToolsListReply::Names(ADVERTISED));
            let client = JcodemunchClient::new(stub.url()).expect("handshake");

            let tools = client
                .list_tools()
                .expect("tools/list against a well-formed stub must succeed");
            let expected: Vec<String> =
                ADVERTISED.iter().map(|n| (*n).to_string()).collect();
            assert_eq!(
                tools, expected,
                "list_tools must report the advertised names verbatim and in \
                 order",
            );

            assert_eq!(
                stub.request_for("tools/list").session_header(),
                Some(ASSIGNED_SESSION),
                "`tools/list` is a post-handshake call like any other and must \
                 replay the server-assigned session id",
            );
        }

        /// Every malformed `tools/list` shape must surface as
        /// `LoadError::Protocol`.
        ///
        /// Never `Ok`: a silently empty or silently shortened tool list is
        /// exactly the PASS-shaped nothing this client's session handling
        /// exists to remove — a caller would read it as "the serve offers
        /// no such tool" rather than "the serve answered nonsense". Never
        /// `Http` either: the transport succeeded, the payload did not.
        fn assert_list_tools_is_a_protocol_error(reply: ToolsListReply, what: &str) {
            let stub = RecordingStub::start_with_tools(reply);
            let client = JcodemunchClient::new(stub.url()).expect("handshake");

            match client.list_tools() {
                Err(LoadError::Protocol(_)) => {} // expected
                Ok(tools) => panic!(
                    "expected a Protocol error for {what}; got Ok({tools:?}) — a \
                     malformed tool list must never be reported as a (possibly \
                     empty or shortened) set of tools",
                ),
                Err(LoadError::Http(e)) => panic!(
                    "expected a Protocol error for {what}; got Http error: {e}",
                ),
            }
        }

        /// An absent tool list is a protocol violation, not a server with
        /// no tools.
        #[test]
        fn list_tools_without_result_tools_is_a_protocol_error() {
            assert_list_tools_is_a_protocol_error(
                ToolsListReply::NoToolsKey,
                "a result carrying no `tools` key",
            );
        }

        /// `result.tools` that is not an array cannot be iterated, so it is
        /// a protocol violation rather than a zero-length list.
        #[test]
        fn list_tools_with_a_non_array_tools_field_is_a_protocol_error() {
            assert_list_tools_is_a_protocol_error(
                ToolsListReply::ToolsNotAnArray,
                "`result.tools` that is an object, not an array",
            );
        }

        /// One nameless entry poisons the whole list rather than being
        /// skipped: silently returning the other two names would report a
        /// tool set the server never advertised.
        #[test]
        fn list_tools_with_a_nameless_entry_is_a_protocol_error() {
            assert_list_tools_is_a_protocol_error(
                ToolsListReply::EntryWithoutName,
                "a tool entry with no string `name`",
            );
        }

        // --------------------------------------------------------------
        // step-5 / step-6: RealJCodemunchOps end-to-end — the filed crash
        // --------------------------------------------------------------

        /// Hermetic reproduction of the filed crash: `get_changed_symbols`
        /// must not panic when the wire reports a declaration line beyond
        /// the declaring file's current length (observed 2026-08-22:
        /// `index out of bounds: the len is 13165 but the index is 18319`
        /// at `extract_suppression`, against a 13165-line
        /// `crates/reify-eval/src/engine_build.rs`).
        ///
        /// Drives the full production route:
        /// `RealJCodemunchOps::get_changed_symbols` → `call_tool` →
        /// `decode_tool_result` → `changed_symbols_from_wire` → the
        /// suppression-enrichment loop → `extract_suppression`. A 4-segment
        /// `added_symbols` payload (the shape every captured fixture uses)
        /// declares one symbol `widget` at line 99 in `a.rs`, but the
        /// tempdir's `a.rs` is only 3 lines — reproducing the past-EOF
        /// condition without touching the workspace.
        #[test]
        fn get_changed_symbols_does_not_panic_when_the_wire_line_is_past_eof() {
            const MUNCH_PAST_EOF: &str = concat!(
                "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
                "\n",
                "x=1 __stypes= __tables=t:added_symbols:name|file|line:str|str|int\n",
                "t,widget,a.rs,99\n",
            );

            let tmp = tempfile::TempDir::new().expect("create tempdir");
            std::fs::write(tmp.path().join("a.rs"), "line one\nline two\nline three\n")
                .expect("write a.rs");

            let stub = RecordingStub::start_with_tool_calls(ToolCallReply::Munch(&[(
                "get_changed_symbols",
                MUNCH_PAST_EOF,
            )]));
            let ops = RealJCodemunchOps::new(stub.url(), "test-repo", tmp.path())
                .expect("handshake against the recording stub must succeed");

            // Must return, not panic.
            let mut symbols = ops.get_changed_symbols("s^1", "s");

            assert_eq!(symbols.len(), 1, "expected exactly the one declared symbol");
            let sym = &symbols[0];
            assert_eq!(sym.name, "widget");
            assert_eq!(sym.line, 99);
            assert!(
                sym.suppression.is_none(),
                "declaration line could not be located past EOF — suppression \
                 must stay UNKNOWN, neither fabricated from an unrelated block \
                 of the file nor reported as \"carries no opt-out\"; got {sym:?}",
            );

            // …and the degradation must not be SILENT. `get_changed_symbols`
            // prints the stale-index diagnostic to this process's own stderr,
            // which an in-process test cannot read — so assert on the seam it
            // prints, re-run over the exact symbols the production route just
            // decoded (enrichment is idempotent: it re-reads the same file and
            // re-derives the same neutral flags).
            let diagnostic = enrich_suppression_flags(&mut symbols, tmp.path()).expect(
                "a past-EOF declaration line must produce a stale-index diagnostic, \
                 not a silent neutral triple",
            );
            assert!(
                diagnostic.contains("a.rs"),
                "diagnostic must name the declaring file; got: {diagnostic}"
            );
            assert!(
                diagnostic.contains("99"),
                "diagnostic must name the out-of-range wire line 99; got: {diagnostic}"
            );
        }

        /// The POSITIVE half of the enrichment contract, and the only test
        /// that pins the enrichment step's WIRING into
        /// `RealJCodemunchOps::get_changed_symbols`.
        ///
        /// Its past-EOF sibling above cannot: that test's only post-condition
        /// on the production route is the neutral
        /// `(false, false, None)` triple, which is byte-for-byte the default
        /// `changed_symbols_from_wire` already sets — so deleting the
        /// `enrich_suppression_flags` call from `get_changed_symbols`
        /// entirely would leave it, and the whole suite, green (it observes
        /// the diagnostic by calling the seam a second time itself). The
        /// `#[must_use]` guard that protects the RETURNED diagnostic cannot
        /// fire on a call site that no longer exists.
        ///
        /// Here the tempdir's `a.rs` carries all three suppressions above the
        /// wire-reported declaration line, so every flag is positively set —
        /// and none of them can be true unless `get_changed_symbols` actually
        /// read the file and enriched the decoded symbols.
        #[test]
        fn get_changed_symbols_enriches_suppression_flags_from_the_declaring_file() {
            const MUNCH_AT_LINE_4: &str = concat!(
                "#MUNCH/1 tool=get_changed_symbols enc=gen1\n",
                "\n",
                "x=1 __stypes= __tables=t:added_symbols:name|file|line:str|str|int\n",
                "t,widget,a.rs,4\n",
            );

            let tmp = tempfile::TempDir::new().expect("create tempdir");
            std::fs::write(
                tmp.path().join("a.rs"),
                "// G-allow: exercised only by the GUI sidecar\n\
                 #[allow(dead_code)]\n\
                 #[cfg(test)]\n\
                 fn widget() {}\n",
            )
            .expect("write a.rs");

            let stub = RecordingStub::start_with_tool_calls(ToolCallReply::Munch(&[(
                "get_changed_symbols",
                MUNCH_AT_LINE_4,
            )]));
            let ops = RealJCodemunchOps::new(stub.url(), "test-repo", tmp.path())
                .expect("handshake against the recording stub must succeed");

            let symbols = ops.get_changed_symbols("s^1", "s");

            assert_eq!(symbols.len(), 1, "expected exactly the one declared symbol");
            let sym = &symbols[0];
            assert_eq!(sym.name, "widget");
            assert_eq!(sym.line, 4);
            let found = sym
                .suppression
                .as_ref()
                .expect("declaration was located — line 4 is in range for a 4-line file");
            assert!(
                found.has_allow_dead_code,
                "`#[allow(dead_code)]` sits directly above the declaration — \
                 `get_changed_symbols` must enrich the decoded symbol from the \
                 declaring file, not return the wire defaults; got {sym:?}",
            );
            assert!(
                found.has_cfg_test,
                "`#[cfg(test)]` sits in the same attribute block; got {sym:?}",
            );
            assert_eq!(
                found.g_allow_marker.as_deref(),
                Some("exercised only by the GUI sidecar"),
                "the `// G-allow:` marker above the block must reach the caller; \
                 got {sym:?}",
            );
        }

        /// Pins today's `RealJCodemunchOps::find_references` scoping
        /// contract ([`JCodemunchOps::find_references`](crate::JCodemunchOps::find_references):
        /// production impls MUST scope to `symbol.file`) through the
        /// production route, over the live 3-segment `__rows__` payload.
        /// Only the first row's file matches `symbol.file`, so exactly one
        /// reference must survive `filter_refs_to_file`.
        #[test]
        fn find_references_decodes_the_real_wire_through_real_ops() {
            const MUNCH_REAL_WIRE: &str = concat!(
                "#MUNCH/1 tool=find_references enc=gen1\n",
                "\n",
                "@1=crates/reify-audit/\n",
                "\n",
                "x=1 __stypes= __tables=r:__rows__:file|specifier|match_type\n",
                "r,@1src/jcodemunch_client.rs,crate,named\n",
                "r,@1tests/p1.rs,reify_audit,named\n",
            );

            let tmp = tempfile::TempDir::new().expect("create tempdir");
            let stub = RecordingStub::start_with_tool_calls(ToolCallReply::Munch(&[(
                "find_references",
                MUNCH_REAL_WIRE,
            )]));
            let ops = RealJCodemunchOps::new(stub.url(), "test-repo", tmp.path())
                .expect("handshake against the recording stub must succeed");

            let symbol = ChangedSymbol {
                name: "JCodemunchOps".to_string(),
                file: "crates/reify-audit/src/jcodemunch_client.rs".to_string(),
                line: 1,
                suppression: None,
            };
            let refs = ops.find_references(&symbol);

            assert_eq!(
                refs.len(),
                1,
                "find_references must scope to symbol.file per the \
                 JCodemunchOps::find_references doc in lib.rs; got {refs:?}",
            );
            assert_eq!(refs[0].file, symbol.file);
            assert_eq!(refs[0].line, 0, "the real wire reports no line");
        }
    }
}
