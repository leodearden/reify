use tower_lsp::lsp_types::TextEdit;

use crate::convert::position_to_offset;

/// Apply LSP `TextEdit`s to `source`, splicing in DESCENDING start order so
/// earlier byte offsets stay valid as later ones are replaced.
pub(super) fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut to_apply: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|e| {
            (
                position_to_offset(source, e.range.start),
                position_to_offset(source, e.range.end),
                e.new_text.as_str(),
            )
        })
        .collect();
    to_apply.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut buffer = source.to_string();
    for (start, end, text) in &to_apply {
        buffer.replace_range(*start..*end, text);
    }
    buffer
}
