//! Utilities for mapping byte offsets in source text to human-readable
//! `(line, column)` positions.

/// Convert a byte offset in `source` to a 1-based `(line, column)` pair.
///
/// The function iterates over Unicode scalar values and increments the column
/// counter for each character, resetting to column 1 and advancing the line
/// counter whenever a `'\n'` is encountered.
///
/// # Edge cases
///
/// - **Empty source**: returns `(1, 1)` — the initial position, since the loop
///   body never executes.
/// - **Offset beyond `source.len()`**: panics in debug builds via
///   `debug_assert!(offset <= source.len())`; in release builds the
///   `debug_assert` is a no-op, so the loop exhausts all characters without
///   reaching the break condition and returns the position *after* the last
///   character (silent clamping).
/// - **Empty spans** (`start == end`): calling this function twice with the
///   same offset produces identical `(line, col)` coordinates, as expected for
///   zero-length diagnostic spans.
pub fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    debug_assert!(offset <= source.len());
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::byte_offset_to_line_col;

    #[test]
    fn byte_offset_to_line_col_basic_conversion() {
        let source = "abc\ndef";
        // offset 0 → start of first line → (1, 1)
        assert_eq!(byte_offset_to_line_col(source, 0), (1, 1));
        // offset 3 → just before the '\n' → (1, 4) (col after 'a','b','c')
        assert_eq!(byte_offset_to_line_col(source, 3), (1, 4));
        // offset 4 → first char of second line → (2, 1)
        assert_eq!(byte_offset_to_line_col(source, 4), (2, 1));
        // offset 6 → last char 'f' → (2, 3)
        assert_eq!(byte_offset_to_line_col(source, 6), (2, 3));
    }

    #[test]
    fn byte_offset_to_line_col_empty_source() {
        // Empty source: offset 0 → initial position (1, 1)
        assert_eq!(byte_offset_to_line_col("", 0), (1, 1));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "offset <= source.len()")]
    fn byte_offset_to_line_col_offset_beyond_len() {
        // In debug/test builds, passing an offset beyond source.len() must panic
        // with a clear message, so stale-span bugs are caught loudly.
        let _ = byte_offset_to_line_col("ab", 100);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn byte_offset_to_line_col_offset_beyond_len_release() {
        // In release builds, debug_assert is a no-op, so passing an offset beyond
        // source.len() silently clamps: the loop exhausts all characters and returns
        // the position after the last character.
        // "ab" → 'a' col→2, 'b' col→3; loop ends → (1, 3).
        assert_eq!(byte_offset_to_line_col("ab", 100), (1, 3));
    }

    #[test]
    fn byte_offset_to_line_col_multibyte_chars() {
        // Source: "αβ\nγ"
        // α = U+03B1, 2 bytes (UTF-8: 0xCE 0xB1), byte offset 0
        // β = U+03B2, 2 bytes (UTF-8: 0xCE 0xB2), byte offset 2
        // \n              ,  byte offset 4
        // γ = U+03B3, 2 bytes (UTF-8: 0xCE 0xB3), byte offset 5
        //
        // Columns must be codepoint-based (1, 2, 3), not byte-based (1, 3, 5).
        let source = "αβ\nγ";
        assert_eq!(source.len(), 7, "sanity-check byte length");

        // offset 0 → 'α' (codepoint 1 on line 1) → (1, 1)
        assert_eq!(byte_offset_to_line_col(source, 0), (1, 1));
        // offset 1 → mid-codepoint inside α; loop processes α (col→2) then sees i=2 ≥ 1 → (1, 2)
        assert_eq!(byte_offset_to_line_col(source, 1), (1, 2));
        // offset 2 → 'β' (codepoint 2 on line 1) → (1, 2)
        assert_eq!(byte_offset_to_line_col(source, 2), (1, 2));
        // offset 4 → '\n' (codepoint 3 on line 1) → (1, 3)
        assert_eq!(byte_offset_to_line_col(source, 4), (1, 3));
        // offset 5 → 'γ' (first codepoint on line 2) → (2, 1)
        assert_eq!(byte_offset_to_line_col(source, 5), (2, 1));
    }

    #[test]
    fn byte_offset_to_line_col_at_source_len() {
        // Source "abc\ndef" has byte length 7.
        // offset == source.len() is the EOF position, one past the last char 'f'.
        // The loop iterates all chars exhausting them:
        // 'a'→col2, 'b'→col3, 'c'→col4, '\n'→line2,col1, 'd'→col2, 'e'→col3, 'f'→col4
        // Then the loop ends and we return (2, 4).
        let source = "abc\ndef";
        assert_eq!(source.len(), 7, "sanity-check byte length");
        assert_eq!(byte_offset_to_line_col(source, 7), (2, 4));
    }

    /// Explicitly verify that byte_offset_to_line_col returns 1-based (line, col)
    /// at every byte offset of a known multi-line string "ab\ncd".
    ///
    /// Byte layout:
    ///   0:'a'  1:'b'  2:'\n'  3:'c'  4:'d'
    ///
    /// Expected results:
    ///   offset 0 → line 1, col 1  (start of 'a')
    ///   offset 1 → line 1, col 2  (start of 'b')
    ///   offset 2 → line 1, col 3  (start of '\n', still on line 1)
    ///   offset 3 → line 2, col 1  (start of 'c')
    ///   offset 4 → line 2, col 2  (start of 'd')
    ///   offset 5 → line 2, col 3  (EOF position, one past last char)
    #[test]
    fn byte_offset_to_line_col_returns_one_based_columns() {
        let source = "ab\ncd";
        assert_eq!(source.len(), 5, "sanity-check byte length");

        // Every byte offset in the string plus EOF
        let expected: &[(usize, (usize, usize))] = &[
            (0, (1, 1)),
            (1, (1, 2)),
            (2, (1, 3)),
            (3, (2, 1)),
            (4, (2, 2)),
            (5, (2, 3)), // EOF
        ];

        for &(offset, expected_pos) in expected {
            let actual = byte_offset_to_line_col(source, offset);
            assert_eq!(
                actual, expected_pos,
                "offset {}: expected {:?} got {:?} — columns must be 1-based",
                offset, expected_pos, actual
            );
        }

        // Spot-check: smallest possible col value is 1, never 0
        for &(_, (_, col)) in expected {
            assert!(col >= 1, "column must be >= 1 (1-based), got {}", col);
        }
    }
}
