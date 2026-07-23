import { describe, it, expect, vi } from 'vitest';
import { EditorState, EditorSelection } from '@codemirror/state';

// Sentinel returned by the mocked drawSelection() so tests can assert the guard
// wired CM's own selection layer into its extension array. vi.hoisted makes it
// available to the (hoisted) vi.mock factory below without a TDZ error.
const { DRAW_SENTINEL } = vi.hoisted(() => ({
  DRAW_SENTINEL: { __drawSelection: true },
}));

// Mock @codemirror/view (mirror gotoDefinition.test.ts:15-19): domEventHandlers
// just echoes its handler map so we can extract blur/focus, and drawSelection
// returns the sentinel. @codemirror/state is deliberately NOT mocked — the
// hasNonEmptySelection block below builds REAL EditorState instances.
vi.mock('@codemirror/view', () => ({
  EditorView: {
    domEventHandlers: (handlers: Record<string, unknown>) => ({ handlers }),
  },
  drawSelection: () => DRAW_SENTINEL,
}));

import {
  hasNonEmptySelection,
  reifyPrimarySelectionGuard,
} from '../editor/primarySelectionGuard';

type Handler = (event: unknown, view: unknown) => unknown;

/** Pull the extension array + blur/focus handler map out of the factory result. */
function extractGuard() {
  const ext = reifyPrimarySelectionGuard() as unknown[];
  const entry = ext.find(
    (e): e is { handlers: Record<string, Handler> } =>
      !!e && typeof e === 'object' && 'handlers' in (e as object),
  );
  return { ext, handlers: entry!.handlers };
}

describe('hasNonEmptySelection', () => {
  it('returns false for a collapsed caret (from === to)', () => {
    const state = EditorState.create({
      doc: 'abcdef',
      selection: EditorSelection.single(2, 2),
    });
    expect(hasNonEmptySelection(state)).toBe(false);
  });

  it('returns true for a non-empty single range (from !== to)', () => {
    const state = EditorState.create({
      doc: 'abcdef',
      selection: EditorSelection.single(1, 4),
    });
    expect(hasNonEmptySelection(state)).toBe(true);
  });

  it('returns true when at least one range of a multi-range selection is non-empty', () => {
    // Main range (index 0) is an empty caret; a sibling range is non-empty. The
    // predicate must inspect every range, not just the main one.
    const state = EditorState.create({
      doc: 'abcdef',
      selection: EditorSelection.create(
        [EditorSelection.range(0, 0), EditorSelection.range(2, 5)],
        0,
      ),
      extensions: EditorState.allowMultipleSelections.of(true),
    });
    expect(hasNonEmptySelection(state)).toBe(true);
  });

  it('returns false for an all-empty multi-cursor selection', () => {
    const state = EditorState.create({
      doc: 'abcdef',
      selection: EditorSelection.create(
        [EditorSelection.range(1, 1), EditorSelection.range(3, 3)],
        0,
      ),
      extensions: EditorState.allowMultipleSelections.of(true),
    });
    expect(hasNonEmptySelection(state)).toBe(false);
  });
});

/**
 * Build a fake EditorView for the blur handler: a hand-built state.selection
 * (real @codemirror/state is not used here — the handler only reads .ranges via
 * hasNonEmptySelection) plus a spied native-selection collaborator reached
 * through contentDOM.ownerDocument.getSelection().
 */
function makeBlurView(opts: {
  from: number;
  to: number;
  getSelection?: () => { removeAllRanges: ReturnType<typeof vi.fn> } | null;
}) {
  const removeAllRanges = vi.fn();
  const sel = { removeAllRanges };
  const getSelection = opts.getSelection ?? (() => sel);
  const empty = opts.from === opts.to;
  const view = {
    state: {
      selection: {
        main: { from: opts.from, to: opts.to, empty },
        ranges: [{ from: opts.from, to: opts.to, empty }],
      },
    },
    contentDOM: { ownerDocument: { getSelection } },
    dispatch: vi.fn(),
  };
  return { view, sel, removeAllRanges };
}

describe('reifyPrimarySelectionGuard — blur', () => {
  it('wires drawSelection() and blur/focus handlers into the extension array', () => {
    const { ext, handlers } = extractGuard();
    expect(ext).toContain(DRAW_SENTINEL);
    expect(typeof handlers.blur).toBe('function');
    expect(typeof handlers.focus).toBe('function');
  });

  it('collapses the native selection on blur when the selection is non-empty, without dispatching', () => {
    const { handlers } = extractGuard();
    const { view, removeAllRanges } = makeBlurView({ from: 1, to: 4 });
    handlers.blur({} as FocusEvent, view);
    expect(removeAllRanges).toHaveBeenCalledTimes(1);
    // state must NEVER be mutated — only the native DOM selection is touched.
    expect(view.dispatch).not.toHaveBeenCalled();
  });

  it('does not touch the native selection on blur when the selection is a caret (empty)', () => {
    const { handlers } = extractGuard();
    const { view, removeAllRanges } = makeBlurView({ from: 2, to: 2 });
    handlers.blur({} as FocusEvent, view);
    expect(removeAllRanges).not.toHaveBeenCalled();
    expect(view.dispatch).not.toHaveBeenCalled();
  });

  it('does not throw on blur when getSelection() returns null (no Selection API)', () => {
    const { handlers } = extractGuard();
    const { view } = makeBlurView({ from: 1, to: 4, getSelection: () => null });
    expect(() => handlers.blur({} as FocusEvent, view)).not.toThrow();
  });
});
