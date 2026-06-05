/**
 * Shared LSP-line → CodeMirror-offset helpers (task 4341).
 *
 * Consolidates the duplicated `doc.line(lspLine + 1).from + character` arithmetic
 * — clamped to `line.to` via Math.min — that was previously repeated in
 * diagnostics.ts, rename.ts, and occurrenceHighlight.ts.
 *
 * Design decisions:
 *  - `lspPositionToOffset` is a composable primitive that PROPAGATES doc.line()'s
 *    RangeError for an out-of-range line.  The guard lives one level up.
 *  - `lspRangeToCmRange` wraps both endpoint calls in a single try/catch and
 *    returns null on any RangeError (version-skew: LSP server's line count differs
 *    from the live document).  It does NOT drop degenerate/zero-width ranges —
 *    that filter is consumer-specific (diagnostics need zero-width point squiggles;
 *    rename needs pure-insertion edits; only occurrenceHighlight's decoration
 *    RangeSet requires non-degenerate marks, so that drop stays local there).
 */

/** A CodeMirror offset range. */
export interface CmRange {
  from: number;
  to: number;
}

/**
 * Map a 0-based LSP position to a clamped CodeMirror document offset.
 *
 * LSP lines are 0-based; CodeMirror `doc.line(n)` is 1-based, so LSP line L is
 * `doc.line(L + 1)`.  The character offset is clamped to `line.to` via Math.min
 * so an over-long character can't escape its line.
 *
 * **PROPAGATES** the RangeError that `doc.line()` throws for a line outside the
 * document.  The guard (try/catch → null) lives in `lspRangeToCmRange` so it
 * covers the whole range in one place.
 */
export function lspPositionToOffset(
  doc: { line(n: number): { from: number; to: number } },
  line: number,
  character: number,
): number {
  const l = doc.line(line + 1);
  return Math.min(l.from + character, l.to);
}

/**
 * Map an LSP range to a CodeMirror `{from, to}` pair, or null when either
 * endpoint's line is outside the current document (version skew).
 *
 * Both endpoints are clamped via `lspPositionToOffset`.  Zero-width / degenerate
 * ranges (`from === to`) are returned as-is — diagnostics and rename both need
 * them; only the decoration RangeSet in occurrenceHighlight drops them (locally).
 *
 * @returns `{from, to}` on success, or `null` when `doc.line()` throws for
 *   either endpoint (stale LSP response vs. the live document).
 */
export function lspRangeToCmRange(
  doc: { line(n: number): { from: number; to: number } },
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  },
): CmRange | null {
  try {
    const from = lspPositionToOffset(doc, range.start.line, range.start.character);
    const to = lspPositionToOffset(doc, range.end.line, range.end.character);
    return { from, to };
  } catch {
    // Out-of-range line: the LSP server's document version is briefly ahead of
    // or behind the live editor doc (didChange in flight).  Return null so the
    // caller can skip/filter this range rather than propagating an exception.
    return null;
  }
}
