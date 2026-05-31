//! Minimal sync MCP HTTP client for the jcodemunch code-analysis service.
//!
//! Exposes [`RealJCodemunchOps`], the production implementation of the
//! [`crate::JCodemunchOps`] trait. The companion [`MockJCodemunchOps`]
//! (gated behind `feature = "test-support"`) lives in `lib.rs`.
//!
//! ## Wire protocol
//!
//! MCP streamable-HTTP, protocol version `2024-11-05`. Mirror of
//! `fused_memory_client.rs` — same handshake, same SSE/JSON dual-path, same
//! `into_reader()` no-10 MiB cap discipline.
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
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{
    ChangedSymbol, DeadSymbol, JCodemunchOps, LayerViolation, SymbolReference,
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
            &first_line[..first_line.len().min(40)]
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
            let comma_pos = line.find(',').unwrap_or(line.len());
            let prefix = &line[..comma_pos];
            if let Some(spec) = table_specs.iter().find(|s| s.prefix == prefix) {
                let fields = split_munch_row(&line[comma_pos + 1..]);
                if fields.len() != spec.columns.len() {
                    return Err(LoadError::Protocol(format!(
                        "munch_decode: row has {} fields, expected {} for table {}; row={:?}",
                        fields.len(),
                        spec.columns.len(),
                        spec.table_name,
                        &line[..line.len().min(80)]
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

fn parse_one_table_spec(spec: &str) -> Result<TableSpec, String> {
    // Format: `<prefix>:<table_name>:<col1>|<col2>|...:<type1>|<type2>|...`
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    if parts.len() != 4 {
        return Err(format!(
            "table spec has {} colon-segments (expected 4): {:?}",
            parts.len(),
            spec
        ));
    }
    let prefix = parts[0].to_string();
    let table_name = parts[1].to_string();
    let columns: Vec<String> = parts[2].split('|').map(|s| s.to_string()).collect();
    let type_strs: Vec<&str> = parts[3].split('|').collect();
    if columns.len() != type_strs.len() {
        return Err(format!(
            "table {} has {} columns but {} types",
            table_name,
            columns.len(),
            type_strs.len()
        ));
    }
    let col_types = type_strs
        .iter()
        .map(|t| match *t {
            "int" => ColType::Int,
            "float" => ColType::Float,
            "bool" => ColType::Bool,
            _ => ColType::Str,
        })
        .collect();
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
/// - Prefers `result.structuredContent` if present.
/// - Otherwise takes `content[0].text`:
///   - If it starts with `#MUNCH/`, calls [`munch_decode`].
///   - Otherwise parses as plain JSON.
///
/// This resolves the non-uniform wire shape described in the fixtures README:
/// some tools return MUNCH, others return plain JSON.
fn decode_tool_result(result: &Value) -> Result<Value, LoadError> {
    // Prefer structuredContent
    if let Some(sc) = result.get("structuredContent")
        && !sc.is_null()
    {
        return Ok(sc.clone());
    }

    // Fall back to content[0].text
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for entry in content {
            if entry.get("type").and_then(|t| t.as_str()) == Some("text")
                && let Some(text) = entry.get("text").and_then(|t| t.as_str())
            {
                if text.starts_with("#MUNCH/") {
                    return munch_decode(text);
                } else {
                    return serde_json::from_str(text).map_err(|e| {
                        LoadError::Protocol(format!(
                            "decode_tool_result: content text not JSON: {e}; text={:?}",
                            &text[..text.len().min(80)]
                        ))
                    });
                }
            }
        }
    }

    // Fall through: return result as-is
    Ok(result.clone())
}

// -----------------------------------------------------------------------
// Wire → struct adapters
// -----------------------------------------------------------------------

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

/// Adapter: MUNCH-decoded value → `Vec<DeadSymbol>`.
///
/// Reads the `dead_symbols` table; maps `id/name/kind/file/line/confidence`
/// by column name; parses `signals` via [`parse_signals_list`].
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
            let line = row.get("line")?.as_u64()? as usize;
            let confidence = row.get("confidence")?.as_f64()?;
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
            let confidence = row.get("confidence")?.as_f64()?;
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
/// decoded but ignored). Maps `name/file/line` by column name; suppression
/// flags are defaulted to `false/false/None` — enrichment happens later in
/// [`RealJCodemunchOps::get_changed_symbols`].
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
            let line = row.get("line")?.as_u64()? as usize;
            Some(ChangedSymbol {
                name,
                file,
                line,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
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

/// Adapter: MUNCH-decoded value → `Vec<SymbolReference>`.
///
/// Finds the first table whose rows carry both `file` and `line` fields;
/// returns an empty vec when absent (no captured fixture exists for
/// `find_references` — end-to-end validation is L-SMOKE's job).
fn find_references_from_wire(decoded: &Value) -> Vec<SymbolReference> {
    let obj = match decoded.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };
    for (_table_name, table_val) in obj {
        if let Some(rows) = table_val.as_array() {
            // Check whether this table's rows contain file + line
            if rows.iter().any(|r| r.get("file").is_some() && r.get("line").is_some()) {
                return rows
                    .iter()
                    .filter_map(|row| {
                        let file = row.get("file")?.as_str()?.to_string();
                        let line = row.get("line")?.as_u64()? as usize;
                        Some(SymbolReference { file, line })
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

// -----------------------------------------------------------------------
// extract_suppression helper
// -----------------------------------------------------------------------

/// Scan the contiguous attribute/comment block immediately above a
/// declaration and extract suppression flags.
///
/// Returns `(has_allow_dead_code, has_cfg_test, g_allow_marker)`.
///
/// Scans upward from the line immediately above `decl_line_1based` over
/// lines that are: attribute lines (`#[...]`), line-comments (`//`), or
/// doc-comment lines (`///` or `//!`). Stops at the first non-matching line.
///
/// - `has_allow_dead_code` — an attribute contains `allow(` and `dead_code`
/// - `has_cfg_test` — an attribute contains `cfg(test)`
/// - `g_allow_marker` — first `// G-allow: <reason>` with non-blank reason
fn extract_suppression(
    lines: &[&str],
    decl_line_1based: usize,
) -> (bool, bool, Option<String>) {
    if decl_line_1based == 0 {
        return (false, false, None);
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

    (has_allow_dead_code, has_cfg_test, g_allow_marker)
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
fn filter_refs_to_file(refs: Vec<SymbolReference>, file: &str) -> Vec<SymbolReference> {
    refs.into_iter().filter(|r| r.file == file).collect()
}

// -----------------------------------------------------------------------
// HTTP transport
// -----------------------------------------------------------------------

/// Sync MCP streamable-HTTP client for jcodemunch. One instance == one
/// MCP session.
///
/// Near-clone of [`crate::fused_memory_client::FusedMemoryClient`]; differs
/// only in `CLIENT_NAME` and the `call_tool` content step (routes
/// MUNCH-vs-JSON via [`decode_tool_result`]).
pub struct JcodemunchClient {
    url: String,
    session_id: String,
    agent: ureq::Agent,
    next_id: Cell<u64>,
}

impl JcodemunchClient {
    /// Connect to `url` and complete the MCP handshake
    /// (initialize + notifications/initialized).
    pub fn new(url: impl Into<String>) -> Result<Self, LoadError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build();
        let client = Self {
            url: url.into(),
            session_id: random_hex_32(),
            agent,
            next_id: Cell::new(1),
        };
        client.initialize()?;
        Ok(client)
    }

    fn initialize(&self) -> Result<(), LoadError> {
        let _ = self.post(&json!({
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
        }))?;
        let _ = self.post(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }))?;
        Ok(())
    }

    fn next_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    fn post(&self, payload: &Value) -> Result<Value, LoadError> {
        let response = self
            .agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream")
            .set("mcp-session-id", &self.session_id)
            .send_json(payload.clone())
            .map_err(|e| LoadError::Http(format!("POST {}: {e}", self.url)))?;

        if response.status() == 202 {
            return Ok(Value::Null);
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
            return Ok(Value::Null);
        } else {
            serde_json::from_str(&body).map_err(|e| {
                LoadError::Protocol(format!("body parse: {e}; body={body}"))
            })?
        };

        if let Some(err) = value.get("error") {
            return Err(LoadError::Protocol(format!("JSON-RPC error: {err}")));
        }
        Ok(value)
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
        // Enrich suppression flags by reading the declaring source file
        for sym in &mut symbols {
            let path = self.project_root.join(&sym.file);
            if let Ok(source) = std::fs::read_to_string(&path) {
                let lines: Vec<&str> = source.lines().collect();
                let (has_allow_dead_code, has_cfg_test, g_allow_marker) =
                    extract_suppression(&lines, sym.line);
                sym.has_allow_dead_code = has_allow_dead_code;
                sym.has_cfg_test = has_cfg_test;
                sym.g_allow_marker = g_allow_marker;
            }
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
        let refs = find_references_from_wire(&decoded);
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
        dead_symbols_from_wire(&decoded)
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
        untested_symbols_from_wire(&decoded)
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
// Session ID (verbatim from fused_memory_client.rs)
// -----------------------------------------------------------------------

fn random_hex_32() -> String {
    let mut buf = [0u8; 16];
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom")
            && f.read_exact(&mut buf).is_ok()
        {
            return hex32(&buf);
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut x = now ^ pid.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for chunk in buf.chunks_mut(8) {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let bytes = x.to_le_bytes();
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = bytes[i];
        }
    }
    hex32(&buf)
}

fn hex32(buf: &[u8; 16]) -> String {
    let mut out = String::with_capacity(32);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
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
        // Suppression flags defaulted
        assert!(!row0.has_allow_dead_code);
        assert!(!row0.has_cfg_test);
        assert!(row0.g_allow_marker.is_none());
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
        ];
        let (allow, cfg, g) = extract_suppression(&src, 4);
        assert!(allow, "has_allow_dead_code should be true");
        assert!(cfg, "has_cfg_test should be true");
        assert_eq!(g, Some("reason text".to_string()));
    }

    #[test]
    fn extract_suppression_clean_decl() {
        let src = ["pub fn clean() {}"];
        let (allow, cfg, g) = extract_suppression(&src, 1);
        assert!(!allow);
        assert!(!cfg);
        assert!(g.is_none());
    }

    #[test]
    fn extract_suppression_blank_g_allow_returns_none() {
        let src = ["// G-allow:", "pub fn my_fn() {}"];
        let (_allow, _cfg, g) = extract_suppression(&src, 2);
        assert!(g.is_none(), "blank G-allow: should not produce a marker");
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
        let refs = find_references_from_wire(&v);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].file, "src/foo.rs");
        assert_eq!(refs[0].line, 10);
        assert_eq!(refs[1].file, "src/bar.rs");
        assert_eq!(refs[1].line, 20);
    }
}
