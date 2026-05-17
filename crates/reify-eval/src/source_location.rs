//! Shared helper for resolving MCP `reify_get_source_location` queries against
//! a compiled module. Factored out of the GUI (`EngineSession`) and CLI
//! (`CliToolContext`) adapters so both transports use identical traversal logic.
//!
//! Accepted `entity_path` shapes:
//! 1. **Template name** (e.g., `"Bracket"`) → returns the first value cell's
//!    span as a proxy for the entity.
//! 2. **Full cell ID** (e.g., `"Bracket.width"`) → returns that cell's span.
//!
//! Returns `None` for anything else (unknown name, bare member without entity
//! prefix, empty string).
//!
//! **Behavior change vs. pre-refactor CLI:** the prior CLI implementation also
//! matched bare member names (e.g., `"width"`) across all templates; this is
//! intentionally dropped for parity with the GUI surface — callers must use
//! the `Entity.member` form.

use reify_compiler::CompiledModule;
use reify_types::SourceLocationInfo;

/// Resolve the entity (and optionally member) at a given 1-based `(line, col)`
/// source position within `source`.
///
/// `line_offsets` must be the result of `reify_types::build_line_offsets(source)`.
/// Passing a pre-built table makes the byte-offset conversion O(log M + line_length)
/// instead of the O(M) character walk of the old local helper.
///
/// Uses the compiled module's value-cell spans to approximate each template's
/// source range (`[min(cell.span.start), max(cell.span.end))`).  Within the
/// matching template, the narrow step checks (in priority order):
/// 1. `value_cells` — returns `"Entity.member"` for the first matching cell.
/// 2. `realizations` (skipping `name: None`) — returns `"Entity.name"` for
///    the first matching realization.
/// 3. `sub_components` — returns `"Entity.name"` for the first matching sub.
///
/// Returns:
/// - `Some("Entity.member")` when the cursor is inside a value cell's span.
/// - `Some("Entity.name")` when the cursor is inside a realization or
///   sub_component declaration body.
/// - `Some("Entity")` when the cursor is inside the template's approximate
///   span but outside any specific named member (e.g. a constraint line).
/// - `None` when `line` or `col` is 0, when the position is outside every
///   template's approximate span, or when the position is past the end of
///   `source`.
pub fn resolve_entity_at_source_position(
    compiled: &CompiledModule,
    source: &str,
    line_offsets: &[usize],
    line: u32,
    col: u32,
) -> Option<String> {
    // 1-based coordinate guard: zero line or col is out-of-range.
    if line == 0 || col == 0 {
        return None;
    }

    // Convert (line, col) → byte offset using the pre-built newline table.
    // The helper is infallible (returns source.len() for past-end positions);
    // the template-walk's half-open `offset < max_end` check filters those out,
    // preserving the documented None contract for past-end positions.
    let offset = reify_types::line_col_to_byte_offset_with_offsets(source, line, col, line_offsets);

    // Walk templates and find the one whose approximate span contains the offset.
    //
    // The approximate span is derived from the union of all member spans:
    // value_cells, constraints, realizations, and sub_components.  This covers
    // positions in constraint lines, geometry body cells, and sub-component
    // declarations — all of which live inside the structure body but may fall
    // between value-cell spans.
    //
    // The approximation is [min(all_member.span.start), max(all_member.span.end)).
    // Templates with no spanned members are skipped.
    let mut best_template: Option<&reify_compiler::TopologyTemplate> = None;
    let mut best_span_size = usize::MAX;

    for template in &compiled.templates {
        // Collect span bounds from all member kinds that carry source spans.
        let mut min_start = usize::MAX;
        let mut max_end = 0usize;

        for vc in &template.value_cells {
            min_start = min_start.min(vc.span.start as usize);
            max_end = max_end.max(vc.span.end as usize);
        }
        for c in &template.constraints {
            min_start = min_start.min(c.span.start as usize);
            max_end = max_end.max(c.span.end as usize);
        }
        for r in &template.realizations {
            min_start = min_start.min(r.span.start as usize);
            max_end = max_end.max(r.span.end as usize);
        }
        for sc in &template.sub_components {
            min_start = min_start.min(sc.span.start as usize);
            max_end = max_end.max(sc.span.end as usize);
        }

        if min_start == usize::MAX {
            // No spanned members — skip this template.
            continue;
        }

        if offset >= min_start && offset < max_end {
            let size = max_end - min_start;
            if size < best_span_size {
                best_template = Some(template);
                best_span_size = size;
            }
        }
    }

    let template = best_template?;

    // Narrow step: check member kinds in priority order.
    // Span is a half-open interval [start, end): start ≤ offset < end.

    // 1. value_cells — highest priority.
    if let Some(cell) = template
        .value_cells
        .iter()
        .find(|vc| offset >= vc.span.start as usize && offset < vc.span.end as usize)
    {
        return Some(format!("{}.{}", template.name, cell.id.member));
    }

    // 2. realizations — skip entries with name: None (only emitted by test helpers).
    if let Some(r) = template
        .realizations
        .iter()
        .filter(|r| r.name.is_some())
        .find(|r| offset >= r.span.start as usize && offset < r.span.end as usize)
    {
        return Some(format!(
            "{}.{}",
            template.name,
            r.name.as_ref().unwrap()
        ));
    }

    // 3. sub_components — name is always populated for compiler-produced entries.
    if let Some(sc) = template
        .sub_components
        .iter()
        .find(|sc| offset >= sc.span.start as usize && offset < sc.span.end as usize)
    {
        return Some(format!("{}.{}", template.name, sc.name));
    }

    // 4. Position is inside the template's approximate span but outside any
    //    named member (e.g. a constraint line) — return the template name.
    Some(template.name.clone())
}


/// Resolve source location for `entity_path` against `compiled`.
///
/// Accepts two forms:
/// - **Template name** (no `.`) — returns the first value cell's span as a
///   proxy for the entity location.
/// - **`Entity.member`** (splits on the first `.`) — returns that cell's span.
///   If the member part itself contains a `.` the input will not match any
///   value cell (members never contain dots), so `None` is returned.
///
/// Returns `None` when the entity or member is not found, or when the input
/// does not match either accepted form (e.g., bare member name, empty string).
///
/// # Migration
///
/// See the module-level documentation for behavior changes vs. the
/// pre-refactor CLI implementation (dropped bare-member fallback).
pub fn resolve_entity_source_location(
    compiled: &CompiledModule,
    source: &str,
    file_path: &str,
    entity_path: &str,
) -> Option<SourceLocationInfo> {
    if entity_path.is_empty() {
        return None;
    }

    let span = if let Some((entity, member)) = entity_path.split_once('.') {
        // "Entity.member" form — split on first dot only.
        // Reject malformed inputs: empty entity, empty member, or a member
        // that itself contains a dot (no value cell has a dotted member name).
        if entity.is_empty() || member.is_empty() || member.contains('.') {
            return None;
        }
        compiled
            .templates
            .iter()
            .filter(|t| t.name == entity)
            .flat_map(|t| t.value_cells.iter())
            .find(|vc| vc.id.member == member)
            .map(|vc| vc.span)?
    } else {
        // Plain template-name form (no '.') — proxy to the first value cell.
        compiled
            .templates
            .iter()
            .find(|t| t.name == entity_path)
            .and_then(|t| t.value_cells.first())
            .map(|vc| vc.span)?
    };

    let (line, col) = reify_types::byte_offset_to_line_col(source, span.start as usize);
    let (end_line, end_col) = reify_types::byte_offset_to_line_col(source, span.end as usize);

    Some(SourceLocationInfo {
        file_path: file_path.to_owned(),
        line: line as u32,
        column: col as u32,
        end_line: end_line as u32,
        end_column: end_col as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_entity_at_source_position, resolve_entity_source_location};
    use reify_types::ModulePath;

    /// Build a CompiledModule from bracket_source() using the stdlib pipeline.
    fn bracket_compiled() -> reify_compiler::CompiledModule {
        let source = reify_test_support::bracket_source();
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("bracket"));
        reify_compiler::compile_with_stdlib(&parsed)
    }

    // (a) Template name "Bracket" → returns Some(loc) whose (line, column)
    //     equals the first value cell's (width) start position.
    //     file_path must be "bracket.ri".
    #[test]
    fn template_name_returns_first_cell_span_with_correct_file_path() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket");
        assert!(
            loc.is_some(),
            "expected Some for entity_path 'Bracket', got None"
        );
        let loc = loc.unwrap();
        assert_eq!(
            loc.file_path, "bracket.ri",
            "file_path must equal the supplied file_path argument"
        );
        assert!(loc.line >= 1, "line must be >= 1 (1-based)");
        assert!(loc.column >= 1, "column must be >= 1 (1-based)");
        assert!(
            loc.end_line > loc.line || loc.end_column >= loc.column,
            "end position must not precede start: ({},{}) -> ({},{})",
            loc.line,
            loc.column,
            loc.end_line,
            loc.end_column
        );
        assert!(
            loc.end_line >= loc.line,
            "end_line ({}) must be >= line ({})",
            loc.end_line,
            loc.line
        );
    }

    // (b) Template name "Bracket" and "Bracket.width" must return the SAME
    //     (line, column, end_line, end_column) because width is the first cell.
    #[test]
    fn template_name_and_width_cell_id_return_identical_span() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let by_name = resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket")
            .expect("'Bracket' must resolve");
        let by_id =
            resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.width")
                .expect("'Bracket.width' must resolve");
        assert_eq!(
            (
                by_name.line,
                by_name.column,
                by_name.end_line,
                by_name.end_column
            ),
            (by_id.line, by_id.column, by_id.end_line, by_id.end_column),
            "template-name resolution must proxy to the first value cell (width), \
             so its span must match 'Bracket.width'"
        );
    }

    // (c) "Bracket.thickness" returns the thickness cell's span.
    //     Proves the helper isn't always returning the first cell.
    //     thickness is declared after width and height, so its line must be > width's line.
    #[test]
    fn cell_id_thickness_returns_different_span_than_width() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let width_loc =
            resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.width")
                .expect("'Bracket.width' must resolve");
        let thickness_loc =
            resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.thickness")
                .expect("'Bracket.thickness' must resolve");
        assert!(
            thickness_loc.line > width_loc.line,
            "thickness (line {}) must be declared after width (line {})",
            thickness_loc.line,
            width_loc.line
        );
    }

    // (d) Unknown template name returns None.
    #[test]
    fn unknown_entity_name_returns_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", "Nonexistent");
        assert!(
            loc.is_none(),
            "expected None for unknown entity 'Nonexistent', got {:?}",
            loc
        );
    }

    // (e) Known entity but unknown member returns None.
    #[test]
    fn known_entity_unknown_member_returns_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let loc =
            resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.nonexistent");
        assert!(
            loc.is_none(),
            "expected None for 'Bracket.nonexistent', got {:?}",
            loc
        );
    }

    // (f) Bare member name (no entity prefix) returns None.
    //     Locks in the dropped bare-member fallback from the old CLI implementation.
    #[test]
    fn bare_member_name_returns_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", "width");
        assert!(
            loc.is_none(),
            "expected None for bare member 'width' (no entity prefix), got {:?}",
            loc
        );
    }

    // (g) Empty string returns None.
    #[test]
    fn empty_entity_path_returns_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", "");
        assert!(
            loc.is_none(),
            "expected None for empty entity_path, got {:?}",
            loc
        );
    }

    // (h-k) Malformed inputs — all must return None.
    //     h: ".width"          — empty entity (leading dot)
    //     i: "Bracket."        — empty member (trailing dot)
    //     j: "Bracket.foo.bar" — member containing a further dot
    //     k: "Bracket..width"  — consecutive dots (member starts with dot)
    //
    // Using a table so all four shapes are covered by a single guard: any
    // future change to the malformed-input handling is caught by this test,
    // and adding a new case is a one-liner.
    #[test]
    fn malformed_inputs_return_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        for &input in &[".width", "Bracket.", "Bracket.foo.bar", "Bracket..width"] {
            let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", input);
            assert!(
                loc.is_none(),
                "expected None for malformed input {:?}, got {:?}",
                input,
                loc
            );
        }
    }

    // ---- resolve_entity_at_source_position tests ----

    // (a) cursor mid-"width" identifier → Some("Bracket.width")
    //     bracket_source() line 2: "    param width: Scalar = 80mm"
    //     col 11 (1-based) = 'w' in "width" — inside the width cell span.
    #[test]
    fn entity_at_source_position_width_cell_returns_bracket_width() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        let result = resolve_entity_at_source_position(&compiled, source, &line_offsets, 2, 11);
        assert_eq!(
            result,
            Some("Bracket.width".to_string()),
            "cursor at (2, 11) should resolve to Bracket.width"
        );
    }

    // (b) cursor mid-"thickness" identifier → Some("Bracket.thickness")
    //     bracket_source() line 4: "    param thickness: Scalar = 5mm"
    //     col 11 (1-based) = 't' in "thickness" — inside the thickness cell span.
    #[test]
    fn entity_at_source_position_thickness_cell_returns_bracket_thickness() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        let result = resolve_entity_at_source_position(&compiled, source, &line_offsets, 4, 11);
        assert_eq!(
            result,
            Some("Bracket.thickness".to_string()),
            "cursor at (4, 11) should resolve to Bracket.thickness"
        );
    }

    // (c) cursor on a constraint line (inside structure body, outside any value cell)
    //     → Some("Bracket").
    //     bracket_source() line 10: "    constraint thickness > 2mm"
    //     col 5 is 'c' in "constraint" — inside Bracket's approximate span (between
    //     the first and last value cell spans) but not within any value cell.
    #[test]
    fn entity_at_source_position_constraint_line_returns_template_name() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        let result = resolve_entity_at_source_position(&compiled, source, &line_offsets, 10, 5);
        assert_eq!(
            result,
            Some("Bracket".to_string()),
            "cursor on constraint line at (10, 5) should resolve to Bracket (template name)"
        );
    }

    // (d) cursor before any value cell (line 16, col 1) → None.
    //     bracket_source() has 15 lines; line 16 is past the end of the source.
    //     The resulting byte offset equals source.len(), which is outside every
    //     template's approximate span → None.
    #[test]
    fn entity_at_source_position_past_end_of_source_returns_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        let result = resolve_entity_at_source_position(&compiled, source, &line_offsets, 16, 1);
        assert!(
            result.is_none(),
            "cursor past end of source at (16, 1) should return None, got {:?}",
            result
        );
    }

    // (e) zero line or zero col → None.
    //     Both are documented out-of-range guards (1-based coordinate system).
    #[test]
    fn entity_at_source_position_zero_line_or_col_returns_none() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        assert!(
            resolve_entity_at_source_position(&compiled, source, &line_offsets, 0, 1).is_none(),
            "zero line should return None"
        );
        assert!(
            resolve_entity_at_source_position(&compiled, source, &line_offsets, 1, 0).is_none(),
            "zero col should return None"
        );
        assert!(
            resolve_entity_at_source_position(&compiled, source, &line_offsets, 0, 0).is_none(),
            "zero line and col should return None"
        );
    }

    // (f) cursor at exact start byte of the width cell span → Some("Bracket.width").
    //     Uses the forward lookup to obtain (line, col) of span.start and verifies
    //     the reverse function returns the same cell.
    #[test]
    fn entity_at_source_position_at_cell_span_start_returns_cell() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.width")
            .expect("forward lookup for Bracket.width must succeed");
        // loc.line and loc.column are 1-based and map to span.start of width cell.
        let result =
            resolve_entity_at_source_position(&compiled, source, &line_offsets, loc.line, loc.column);
        assert_eq!(
            result,
            Some("Bracket.width".to_string()),
            "cursor at span.start (line={}, col={}) should resolve to Bracket.width",
            loc.line,
            loc.column
        );
    }

    // (g) cursor at end byte (exclusive) of the width cell span → Some("Bracket").
    //     The end byte is exclusive: the cursor sits in the gap between value cells.
    //     Per the function doc-block and the half-open span contract, span.end is
    //     outside the width cell, so the narrow step misses it and falls through to
    //     the enclosing template name.
    #[test]
    fn entity_at_source_position_at_cell_span_end_does_not_return_that_cell() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        let loc = resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.width")
            .expect("forward lookup for Bracket.width must succeed");
        // loc.end_line and loc.end_column map to span.end (exclusive upper bound).
        let result = resolve_entity_at_source_position(
            &compiled,
            source,
            &line_offsets,
            loc.end_line,
            loc.end_column,
        );
        // span.end (exclusive) falls outside the width cell and outside any other
        // named member → must resolve to the enclosing template name, not a cell.
        assert_eq!(
            result,
            Some("Bracket".to_string()),
            "cursor at span.end (exclusive) (line={}, col={}) must resolve to the enclosing \
             template name, not the cell or any other entity",
            loc.end_line,
            loc.end_column
        );
    }

    // (h) cursor inside the `body` realization declaration → Some("Bracket.body").
    //     bracket_source() line 14: "    let body = box(width, height, thickness)"
    //     col 9 (1-based) = 'b' in "body" — inside the body realization's span.
    //
    //     Before step-6 (extend narrow step to realizations), this returns
    //     Some("Bracket") because only value_cells are checked; the assert_eq
    //     fires RED. After step-6 it returns Some("Bracket.body"), GREEN.
    #[test]
    fn entity_at_source_position_realization_body_returns_template_dot_realization() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        // line 14, col 9 = 'b' of "body" in "    let body = box(width, height, thickness)"
        let result = resolve_entity_at_source_position(&compiled, source, &line_offsets, 14, 9);
        assert_eq!(
            result,
            Some("Bracket.body".to_string()),
            "cursor at (14, 9) inside the 'body' realization should resolve to Bracket.body"
        );
    }

    // (i) cursor with col past end of an interior line → Some("Bracket").
    //
    //     This pins the deliberate clamp-to-line-end behavior in
    //     `line_col_to_byte_offset_with_offsets` (introduced when the helper was
    //     moved from engine.rs to reify-types).
    //
    //     bracket_source() line 2: "    param width: Scalar = 80mm" (30 chars).
    //     col=99 is past the end of that line.  The new helper clamps to the byte
    //     offset of the trailing '\n', which falls in the gap between the width cell
    //     span and the height cell span → the narrow step misses all cells and
    //     returns the enclosing template name.
    //
    //     The old char-walking `line_col_to_byte_offset` (now deleted) would have
    //     walked past the '\n' into the following line's content (height param) and
    //     returned Some("Bracket.height") instead.  This test pins the new semantics
    //     so a future regression is caught explicitly.
    #[test]
    fn entity_at_source_position_col_past_line_end_clamps_to_template_name() {
        let compiled = bracket_compiled();
        let source = reify_test_support::bracket_source();
        let line_offsets = reify_types::build_line_offsets(source);
        // line 2 = "    param width: Scalar = 80mm" (30 chars).
        // col=99 is well past the end; new helper clamps to the trailing '\n' of line 2,
        // which falls outside any value cell → enclosing template name.
        let result = resolve_entity_at_source_position(&compiled, source, &line_offsets, 2, 99);
        assert_eq!(
            result,
            Some("Bracket".to_string()),
            "cursor at (2, 99) — col past end of line 2 — must clamp to '\\n' at line end \
             and resolve to the enclosing template name, not a cell (got {:?})",
            result
        );
    }
}
