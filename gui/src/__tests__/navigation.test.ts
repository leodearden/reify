import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { SourceLocation, ConstraintData, ValueData } from '../types';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

// The module under test (doesn't exist yet)
import {
  navigateToSource,
  navigateToEntity,
  navigateFromConstraint,
} from '../navigation';
import { createSelectionStore } from '../stores/selectionStore';

describe('navigation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('navigateToSource', () => {
    it('calls getSourceLocation with entityPath and passes result to scrollEditor', async () => {
      const sourceLocation: SourceLocation = {
        file_path: 'main.ri',
        line: 5,
        column: 3,
        end_line: 5,
        end_column: 10,
      };
      const getSourceLocation = vi.fn().mockResolvedValue(sourceLocation);
      const scrollEditor = vi.fn();
      const selectEntity = vi.fn();

      await navigateToSource('Bracket', {
        getSourceLocation,
        scrollEditor,
        selectEntity,
      });

      expect(getSourceLocation).toHaveBeenCalledWith('Bracket');
      expect(scrollEditor).toHaveBeenCalledWith(sourceLocation);
      expect(selectEntity).toHaveBeenCalledWith('Bracket');
    });

    it('handles getSourceLocation rejection gracefully', async () => {
      const getSourceLocation = vi.fn().mockRejectedValue(new Error('not found'));
      const scrollEditor = vi.fn();
      const selectEntity = vi.fn();
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

      // Should not throw
      await expect(
        navigateToSource('Unknown', {
          getSourceLocation,
          scrollEditor,
          selectEntity,
        }),
      ).resolves.not.toThrow();

      expect(scrollEditor).not.toHaveBeenCalled();
      expect(consoleError).toHaveBeenCalled();
      consoleError.mockRestore();
    });

    it('still calls selectEntity when getSourceLocation rejects (entity lacks a source span)', async () => {
      const getSourceLocation = vi
        .fn()
        .mockRejectedValue(new Error('No source location found for BooleanResult'));
      const scrollEditor = vi.fn();
      const selectEntity = vi.fn();
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

      await navigateToSource('BooleanResult', {
        getSourceLocation,
        scrollEditor,
        selectEntity,
      });

      // Selection must happen unconditionally — even when no source span exists
      expect(selectEntity).toHaveBeenCalledWith('BooleanResult');
      // Editor scroll is best-effort; must be skipped when no span is available
      expect(scrollEditor).not.toHaveBeenCalled();
      consoleError.mockRestore();
    });
  });

  describe('navigateToEntity', () => {
    it('calls focusEntity exactly once with entityPath', async () => {
      const focusEntity = vi.fn().mockResolvedValue(undefined);

      await navigateToEntity('Bracket', { focusEntity });

      expect(focusEntity).toHaveBeenCalledOnce();
      expect(focusEntity).toHaveBeenCalledWith('Bracket');
    });

    it('handles focusEntity rejection gracefully', async () => {
      const focusEntity = vi.fn().mockRejectedValue(new Error('IPC failure'));
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

      // Should not throw
      await expect(
        navigateToEntity('Unknown', { focusEntity }),
      ).resolves.not.toThrow();

      expect(consoleError).toHaveBeenCalledWith(
        expect.stringMatching(/Failed to navigate to entity/),
        expect.any(Error),
      );
      consoleError.mockRestore();
    });
  });

  describe('navigateFromConstraint', () => {
    it('sets highlightedParams and selects the entity of the first matching value', () => {
      const constraint: ConstraintData = {
        node_id: 'n1',
        expression: 'width > 10',
        status: 'violated',
        label: null,
        parameter_ids: ['c1', 'c2'],
      };
      const values: ValueData[] = [
        {
          cell_id: 'c1',
          name: 'width',
          value: '5',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket',
          kind: 'Param',
          freshness: 'final',
        },
        {
          cell_id: 'c2',
          name: 'height',
          value: '20',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket',
          kind: 'Param',
          freshness: 'final',
        },
      ];
      const selectEntity = vi.fn();
      const setHighlightedParams = vi.fn();

      navigateFromConstraint(constraint, values, {
        selectEntity,
        setHighlightedParams,
      });

      expect(setHighlightedParams).toHaveBeenCalledWith(['c1', 'c2']);
      expect(selectEntity).toHaveBeenCalledWith('Bracket');
    });

    it('calls selectEntity(null) when parameter_ids is empty', () => {
      const constraint: ConstraintData = {
        node_id: 'n2',
        expression: 'true',
        status: 'satisfied',
        label: null,
        parameter_ids: [],
      };
      const values: ValueData[] = [];
      const selectEntity = vi.fn();
      const setHighlightedParams = vi.fn();

      navigateFromConstraint(constraint, values, {
        selectEntity,
        setHighlightedParams,
      });

      expect(setHighlightedParams).toHaveBeenCalledWith([]);
      expect(selectEntity).toHaveBeenCalledWith(null);
    });

    it('real-store integration: highlightedParams survive after navigateFromConstraint (order invariant)', () => {
      // This test pins the surviving-highlight invariant against the real store.
      // After calling navigateFromConstraint, both state.highlightedParams and
      // state.selectedEntity must reflect the constraint's params and matched entity.
      // With the OLD ordering (setHighlightedParams BEFORE selectEntity), the
      // highlight would be immediately wiped by selectEntity→selectSingle clearing
      // highlightedParams — so this test is RED until navigation.ts is reordered.
      createRoot((dispose) => {
        const store = createSelectionStore();

        const constraint: ConstraintData = {
          node_id: 'n1',
          expression: 'width > 10',
          status: 'violated',
          label: null,
          parameter_ids: ['c1', 'c2'],
        };
        const values: ValueData[] = [
          {
            cell_id: 'c1',
            name: 'width',
            value: '5',
            unit: 'mm',
            determinacy: 'determined',
            entity_path: 'Bracket',
            kind: 'Param',
            freshness: 'final',
          },
          {
            cell_id: 'c2',
            name: 'height',
            value: '20',
            unit: 'mm',
            determinacy: 'determined',
            entity_path: 'Bracket',
            kind: 'Param',
            freshness: 'final',
          },
        ];

        navigateFromConstraint(constraint, values, {
          selectEntity: store.selectEntity,
          setHighlightedParams: store.setHighlightedParams,
        });

        // Both invariants must hold: selection set AND highlight surviving
        expect(store.state.selectedEntity).toBe('Bracket');
        expect(store.state.highlightedParams).toEqual(['c1', 'c2']);

        dispose();
      });
    });

    it('real-store integration: highlightedParams survive when no value matches (null entity path)', () => {
      // The order invariant is equally fragile on the unmatched branch:
      // selectEntity(null) → clearSelection clears highlightedParams, then
      // setHighlightedParams must still apply the constraint IDs afterward.
      // This was only covered by the older mock-based test where call order is
      // invisible — this test exercises the real store so the clearing contract
      // is verified end-to-end for the null-entity path.
      createRoot((dispose) => {
        const store = createSelectionStore();

        const constraint: ConstraintData = {
          node_id: 'n2',
          expression: 'width > 10',
          status: 'violated',
          label: null,
          parameter_ids: ['c1', 'c2'],
        };
        // No value has a cell_id that matches 'c1' or 'c2'
        const values: ValueData[] = [
          {
            cell_id: 'unrelated',
            name: 'depth',
            value: '3',
            unit: 'mm',
            determinacy: 'determined',
            entity_path: 'Shelf',
            kind: 'Param',
            freshness: 'final',
          },
        ];

        navigateFromConstraint(constraint, values, {
          selectEntity: store.selectEntity,
          setHighlightedParams: store.setHighlightedParams,
        });

        // selectedEntity is null (no match found), but the highlight must survive
        expect(store.state.selectedEntity).toBeNull();
        expect(store.state.highlightedParams).toEqual(['c1', 'c2']);

        dispose();
      });
    });
  });
});
