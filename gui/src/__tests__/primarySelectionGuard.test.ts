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

/**
 * Build a fake EditorView for the focus handler with every native-selection
 * collaborator spied: getSelection()/createRange() on ownerDocument, plus
 * domAtPos() on the view. The handler must rebuild the native selection from
 * state.main.{from,to} and NEVER dispatch.
 */
function makeFocusView(opts: {
  from: number;
  to: number;
  anchor?: number;
  head?: number;
  getSelection?: () => { removeAllRanges: ReturnType<typeof vi.fn>; addRange: ReturnType<typeof vi.fn> } | null;
}) {
  const removeAllRanges = vi.fn();
  const addRange = vi.fn();
  const setStart = vi.fn();
  const setEnd = vi.fn();
  const range = { setStart, setEnd };
  const createRange = vi.fn(() => range);
  const sel = { removeAllRanges, addRange };
  const getSelection = opts.getSelection ?? (() => sel);
  const domAtPos = vi.fn((pos: number) => ({ node: 'n@' + pos, offset: pos }));
  const empty = opts.from === opts.to;
  // In real CM, main.from/to are min/max while anchor/head retain the selection
  // DIRECTION. Default a forward selection (anchor=from, head=to); a reversed
  // (leftward) selection passes anchor > head with the same from/to.
  const anchor = opts.anchor ?? opts.from;
  const head = opts.head ?? opts.to;
  const view = {
    state: {
      selection: {
        main: { from: opts.from, to: opts.to, anchor, head, empty },
        ranges: [{ from: opts.from, to: opts.to, anchor, head, empty }],
      },
    },
    contentDOM: { ownerDocument: { getSelection, createRange } },
    domAtPos,
    dispatch: vi.fn(),
  };
  return { view, removeAllRanges, addRange, setStart, setEnd, createRange, range, domAtPos };
}

describe('reifyPrimarySelectionGuard — focus', () => {
  it('rebuilds the native selection from state.main.{from,to} via domAtPos, without dispatching', () => {
    const { handlers } = extractGuard();
    const { view, removeAllRanges, addRange, setStart, setEnd, createRange, range, domAtPos } =
      makeFocusView({ from: 3, to: 8 });

    handlers.focus({} as FocusEvent, view);

    // Endpoints resolved through domAtPos(from) / domAtPos(to).
    expect(domAtPos).toHaveBeenCalledWith(3);
    expect(domAtPos).toHaveBeenCalledWith(8);
    expect(createRange).toHaveBeenCalledTimes(1);
    expect(setStart).toHaveBeenCalledWith('n@3', 3);
    expect(setEnd).toHaveBeenCalledWith('n@8', 8);
    // The native selection is replaced, not appended: removeAllRanges THEN addRange.
    expect(removeAllRanges).toHaveBeenCalledTimes(1);
    expect(addRange).toHaveBeenCalledWith(range);
    expect(removeAllRanges.mock.invocationCallOrder[0]).toBeLessThan(
      addRange.mock.invocationCallOrder[0],
    );
    // state must NEVER be mutated.
    expect(view.dispatch).not.toHaveBeenCalled();
  });

  it('is a no-op on focus when the selection is a caret (empty)', () => {
    const { handlers } = extractGuard();
    const { view, removeAllRanges, addRange, createRange } = makeFocusView({ from: 5, to: 5 });
    handlers.focus({} as FocusEvent, view);
    expect(createRange).not.toHaveBeenCalled();
    expect(removeAllRanges).not.toHaveBeenCalled();
    expect(addRange).not.toHaveBeenCalled();
    expect(view.dispatch).not.toHaveBeenCalled();
  });

  it('does not throw on focus when getSelection() returns null (no Selection API)', () => {
    const { handlers } = extractGuard();
    const { view, createRange } = makeFocusView({ from: 3, to: 8, getSelection: () => null });
    expect(() => handlers.focus({} as FocusEvent, view)).not.toThrow();
    // Bails before building a range when there is no native selection to rebuild.
    expect(createRange).not.toHaveBeenCalled();
  });

  it('rebuilds a FORWARD native range for a reversed (leftward) logical selection, leaving anchor/head untouched', () => {
    // A reversed selection: anchor=8 > head=3 (selected leftward). CM still
    // normalizes main.from/to to min/max (from=3, to=8), and the focus handler
    // rebuilds the native selection from from/to — so the native DOM Range is
    // always FORWARD (setStart at `from`, setEnd at `to`) regardless of the
    // logical direction. This is INTENTIONAL and acceptable: X11 PRIMARY carries
    // only the selected TEXT, not a direction, and view.state (which retains the
    // true anchor/head) is never mutated — so CM's own direction is preserved.
    const { handlers } = extractGuard();
    const { view, setStart, setEnd, createRange, removeAllRanges, addRange, range, domAtPos } =
      makeFocusView({ from: 3, to: 8, anchor: 8, head: 3 });

    handlers.focus({} as FocusEvent, view);

    // Endpoints come from from/to (min/max), NOT from anchor→head direction.
    expect(domAtPos).toHaveBeenCalledWith(3);
    expect(domAtPos).toHaveBeenCalledWith(8);
    expect(createRange).toHaveBeenCalledTimes(1);
    expect(setStart).toHaveBeenCalledWith('n@3', 3); // `from` (min), not the head
    expect(setEnd).toHaveBeenCalledWith('n@8', 8); //   `to` (max), not the anchor
    expect(removeAllRanges).toHaveBeenCalledTimes(1);
    expect(addRange).toHaveBeenCalledWith(range);
    // state — including its reversed anchor/head — must NEVER be mutated.
    expect(view.dispatch).not.toHaveBeenCalled();
    expect(view.state.selection.main.anchor).toBe(8);
    expect(view.state.selection.main.head).toBe(3);
  });
});
