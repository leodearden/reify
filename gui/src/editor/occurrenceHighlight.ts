/**
 * Occurrence-highlight CodeMirror extension (task 4204 δ).
 *
 * Renders textDocument/documentHighlight results as subtle decorations that
 * appear on cursor-idle and clear on cursor-move. Lives in its own file (NOT
 * highlight.ts, which is the lezer styleTags propSource consumed by the
 * grammar) so the semantic occurrence decoration stays distinct from syntactic
 * token coloring.
 */
import { StateEffect, StateField } from '@codemirror/state';
import { Decoration, EditorView, type DecorationSet } from '@codemirror/view';
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

/** The single mark spec applied to every occurrence; `class` is a stable signal. */
const OCCURRENCE_MARK = Decoration.mark({ class: 'cm-occurrenceHighlight' });

/**
 * Replace the current occurrence-highlight set with the given CM ranges. An
 * empty array clears all highlights (dispatched synchronously on cursor-move).
 */
export const setOccurrencesEffect = StateEffect.define<CmRange[]>();

/**
 * Holds the occurrence-highlight DecorationSet. On document changes the existing
 * marks are mapped forward so positions stay valid; a `setOccurrencesEffect`
 * rebuilds the set from the effect's ranges (sorted, since RangeSet requires
 * ascending order). Provided to the view as decorations.
 */
export const occurrenceHighlightField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none;
  },
  update(decos, tr) {
    decos = decos.map(tr.changes);
    for (const effect of tr.effects) {
      if (effect.is(setOccurrencesEffect)) {
        const marks = effect.value
          .slice()
          .sort((a, b) => a.from - b.from || a.to - b.to)
          .map((r) => OCCURRENCE_MARK.range(r.from, r.to));
        decos = Decoration.set(marks, true);
      }
    }
    return decos;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/**
 * Subtle occurrence-highlight style, co-located with the extension via
 * baseTheme so the stable `cm-occurrenceHighlight` class ships with it (no
 * CSS-module edit) and gives the reify-debug dom_query signal a deterministic
 * target.
 */
export const occurrenceHighlightTheme = EditorView.baseTheme({
  '.cm-occurrenceHighlight': {
    backgroundColor: 'rgba(120, 160, 255, 0.18)',
    borderRadius: '2px',
  },
});
