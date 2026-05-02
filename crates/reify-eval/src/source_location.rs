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
        // Plain template-name form — no dot, so reject names containing any
        // separator that would make this a different form
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
    use super::resolve_entity_source_location;
    use reify_types::ModulePath;

    /// Build a CompiledModule from bracket_source() using the stdlib pipeline.
    fn bracket_compiled() -> reify_compiler::CompiledModule {
        let source = reify_test_support::bracket_source();
        let parsed =
            reify_compiler::parse_with_stdlib(source, ModulePath::single("bracket"));
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
        let by_id = resolve_entity_source_location(&compiled, source, "bracket.ri", "Bracket.width")
            .expect("'Bracket.width' must resolve");
        assert_eq!(
            (by_name.line, by_name.column, by_name.end_line, by_name.end_column),
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
        let loc =
            resolve_entity_source_location(&compiled, source, "bracket.ri", "Nonexistent");
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
}
