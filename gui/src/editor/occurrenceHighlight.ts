/**
 * Occurrence-highlight CodeMirror extension (task 4204 δ).
 *
 * Renders textDocument/documentHighlight results as subtle decorations that
 * appear on cursor-idle and clear on cursor-move. Lives in its own file (NOT
 * highlight.ts, which is the lezer styleTags propSource consumed by the
 * grammar) so the semantic occurrence decoration stays distinct from syntactic
 * token coloring.
 */
import type { DocumentHighlight } from './lspClient';

/**
 * Occurrence-highlight debounce (ms). Distinct from the editor's 300ms
 * EDITOR_DEBOUNCE_MS: occurrence highlight should feel snappier (PRD Open
 * Question 3, ~150ms, matching useEditorSelectionSync's responsiveness tier).
 */
export const HIGHLIGHT_DEBOUNCE_MS = 150;

/** A CodeMirror offset range. */
export interface CmRange {
  from: number;
  to: number;
}

/**
 * Map LSP DocumentHighlight ranges to CodeMirror {from,to} offsets.
 *
 * LSP lines are 0-based; CodeMirror `doc.line(n)` is 1-based, so LSP line L is
 * doc.line(L + 1). Each endpoint offset is `line.from + character`, clamped to
 * `line.to` (Math.min) so an over-long character can't escape its line — the
 * same per-line arithmetic + clamp as diagnostics.ts and rename.ts. Degenerate
 * ranges (zero/negative width or out-of-order endpoints) are dropped defensively
 * so the decoration RangeSet never receives an invalid mark.
 */
export function highlightsToRanges(
  highlights: DocumentHighlight[],
  doc: { line(n: number): { from: number; to: number } },
): CmRange[] {
  const out: CmRange[] = [];
  for (const h of highlights) {
    const startLine = doc.line(h.range.start.line + 1);
    const endLine = doc.line(h.range.end.line + 1);
    const from = Math.min(startLine.from + h.range.start.character, startLine.to);
    const to = Math.min(endLine.from + h.range.end.character, endLine.to);
    // Drop degenerate / inverted ranges so the decoration set stays valid.
    if (to > from) {
      out.push({ from, to });
    }
  }
  return out;
}
