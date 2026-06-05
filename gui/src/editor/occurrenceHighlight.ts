/**
 * Occurrence-highlight CodeMirror extension (task 4204 δ).
 *
 * Renders textDocument/documentHighlight results as subtle decorations that
 * appear on cursor-idle and clear on cursor-move. Lives in its own file (NOT
 * highlight.ts, which is the lezer styleTags propSource consumed by the
 * grammar) so the semantic occurrence decoration stays distinct from syntactic
 * token coloring.
 */
import { StateEffect, StateField, type Extension } from '@codemirror/state';
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';
import type { DocumentHighlight } from './lspClient';
import { lspRangeToCmRange, type CmRange } from './lspRange';

/** Re-export the shared CmRange type so existing importers don't change. */
export type { CmRange } from './lspRange';

/**
 * Occurrence-highlight debounce (ms). Distinct from the editor's 300ms
 * EDITOR_DEBOUNCE_MS: occurrence highlight should feel snappier (PRD Open
 * Question 3, ~150ms, matching useEditorSelectionSync's responsiveness tier).
 */
export const HIGHLIGHT_DEBOUNCE_MS = 150;

/**
 * Map LSP DocumentHighlight ranges to CodeMirror {from,to} offsets.
 *
 * LSP lines are 0-based; CodeMirror `doc.line(n)` is 1-based, so LSP line L is
 * doc.line(L + 1). Each endpoint offset is `line.from + character`, clamped to
 * `line.to` (Math.min) so an over-long character can't escape its line — the
 * same per-line arithmetic + clamp as diagnostics.ts and rename.ts. Degenerate
 * ranges (zero/negative width or out-of-order endpoints) are dropped defensively
 * so the decoration RangeSet never receives an invalid mark.
 *
 * Each highlight is mapped under its own guard: `doc.line()` throws a RangeError
 * for a line outside the document, and the ranges come from the LSP server's
 * copy of the document, which can be briefly version-skewed (a didChange in
 * flight). The token/docChanged stale-guards make this unlikely but don't fully
 * exclude skew, since the line count is read from the live doc at paint time. A
 * stale/out-of-range response therefore drops the offending mark rather than
 * throwing out of the dispatch path — and one bad mark never poisons the rest.
 */
export function highlightsToRanges(
  highlights: DocumentHighlight[],
  doc: { line(n: number): { from: number; to: number } },
): CmRange[] {
  const out: CmRange[] = [];
  for (const h of highlights) {
    const r = lspRangeToCmRange(doc, h.range);
    // Guard: null means out-of-range line (stale response vs. the current doc).
    // Degenerate-drop: the decoration RangeSet requires non-degenerate marks;
    // zero-width ranges are dropped here (consumer-specific, not in the helper).
    if (r && r.to > r.from) {
      out.push(r);
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
 * Holds the occurrence-highlight DecorationSet. Priority:
 *  1. A `setOccurrencesEffect` rebuilds the set from the effect's ranges (sorted,
 *     since RangeSet requires ascending order) — these ranges are already
 *     computed against the post-transaction document by the requester.
 *  2. A cursor move (any transaction whose selection changes) with no such
 *     effect clears the set in that same transaction, so stale highlights never
 *     flicker while the debounced re-request is in flight. This synchronous
 *     clear lives in the field rather than the ViewPlugin because a ViewPlugin
 *     cannot dispatch during update() ("Calls to EditorView.update are not
 *     allowed while an update is in progress").
 *  3. Otherwise existing marks are mapped forward across document changes so
 *     positions stay valid. Provided to the view as decorations.
 */
export const occurrenceHighlightField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none;
  },
  update(decos, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setOccurrencesEffect)) {
        const marks = effect.value
          .slice()
          .sort((a, b) => a.from - b.from || a.to - b.to)
          .map((r) => OCCURRENCE_MARK.range(r.from, r.to));
        return Decoration.set(marks, true);
      }
    }
    if (tr.selection) return Decoration.none;
    return decos.map(tr.changes);
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

/** The subset of the LSP client this extension needs. */
interface DocumentHighlightClient {
  documentHighlight(
    uri: string,
    line: number,
    character: number,
  ): Promise<DocumentHighlight[]>;
}

/**
 * Occurrence-highlight extension: on cursor-idle, request
 * textDocument/documentHighlight at the cursor and paint the results; on every
 * cursor move (or edit) the highlights clear immediately (via
 * {@link occurrenceHighlightField}) and a fresh request is debounced.
 *
 * Returns `[field, theme, plugin]` so a single call wires up the decoration
 * state, its style, and the request driver.
 *
 * @param uriGetter  Re-resolves the active document URI at request time — the
 *   EditorView is reused across files, so this is read both when capturing the
 *   request and again when applying the response (stale-guard).
 * @param client     Provides `documentHighlight(uri, line, character)`.
 * @param debounceMs Idle delay before requesting (default {@link HIGHLIGHT_DEBOUNCE_MS}).
 */
export function occurrenceHighlightExtension(
  uriGetter: () => string,
  client: DocumentHighlightClient,
  debounceMs = HIGHLIGHT_DEBOUNCE_MS,
): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      private timer: ReturnType<typeof setTimeout> | null = null;
      /**
       * Monotonic token, bumped on every cursor move. A response whose captured
       * token is no longer the latest is discarded — the same race-guard
       * useEditorSelectionSync uses for cursor-driven async LSP work.
       */
      private latestToken = 0;

      constructor(private readonly view: EditorView) {}

      update(u: ViewUpdate): void {
        // React only to cursor moves and edits. Our own setOccurrencesEffect
        // dispatches (and pure geometry updates) carry neither, so they don't
        // re-trigger a request — preventing a feedback loop.
        if (!u.selectionSet && !u.docChanged) return;

        // The cursor moved: the field already cleared the highlights in this
        // same transaction. Cancel any pending request, invalidate any in-flight
        // one, and debounce a fresh request.
        if (this.timer !== null) clearTimeout(this.timer);
        const token = ++this.latestToken;
        this.timer = setTimeout(() => {
          this.timer = null;
          void this.request(token);
        }, debounceMs);
      }

      private async request(token: number): Promise<void> {
        const { view } = this;
        const head = view.state.selection.main.head;
        const line = view.state.doc.lineAt(head);
        const lspLine = line.number - 1; // CM lines are 1-based; LSP 0-based
        const lspChar = head - line.from;
        const uri = uriGetter();

        let result: DocumentHighlight[];
        try {
          result = await client.documentHighlight(uri, lspLine, lspChar);
        } catch {
          return; // request failed — leave the already-cleared highlights as-is
        }

        // Stale-guard: a newer cursor move fired, the file switched, or the view
        // was torn down while this was in flight → never paint the wrong buffer.
        if (token !== this.latestToken) return;
        if (uriGetter() !== uri) return;
        if (!view.dom.isConnected) return;

        view.dispatch({
          effects: setOccurrencesEffect.of(highlightsToRanges(result, view.state.doc)),
        });
      }

      destroy(): void {
        if (this.timer !== null) clearTimeout(this.timer);
      }
    },
  );

  return [occurrenceHighlightField, occurrenceHighlightTheme, plugin];
}
