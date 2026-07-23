import { describe, it, expect } from 'vitest';
import { EditorState, EditorSelection } from '@codemirror/state';

import { hasNonEmptySelection } from '../editor/primarySelectionGuard';

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
