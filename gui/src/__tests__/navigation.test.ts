import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { SourceLocation, ConstraintData, ValueData } from '../types';

// The module under test (doesn't exist yet)
import {
  navigateToSource,
  navigateToEntity,
  navigateFromConstraint,
} from '../navigation';

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
  });
});
