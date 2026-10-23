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
  navigateToLocation,
} from '../navigation';
import { createSelectionStore } from '../stores/selectionStore';
import { createEditorStore } from '../stores/editorStore';

describe('navigation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('navigateToSource', () => {
    it('calls getSourceLocation with entityPath and passes result to revealLocation', async () => {
      const sourceLocation: SourceLocation = {
        file_path: 'main.ri',
        line: 5,
        column: 3,
        end_line: 5,
        end_column: 10,
      };
      const getSourceLocation = vi.fn().mockResolvedValue(sourceLocation);
      const revealLocation = vi.fn();
      const selectEntity = vi.fn();

      await navigateToSource('Bracket', {
        getSourceLocation,
        revealLocation,
        selectEntity,
      });

      expect(getSourceLocation).toHaveBeenCalledWith('Bracket');
      expect(revealLocation).toHaveBeenCalledWith(sourceLocation);
      expect(selectEntity).toHaveBeenCalledWith('Bracket');
    });

    it('handles getSourceLocation rejection gracefully', async () => {
      const getSourceLocation = vi.fn().mockRejectedValue(new Error('not found'));
      const revealLocation = vi.fn();
      const selectEntity = vi.fn();
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

      // Should not throw
      await expect(
        navigateToSource('Unknown', {
          getSourceLocation,
          revealLocation,
          selectEntity,
        }),
      ).resolves.not.toThrow();

      expect(revealLocation).not.toHaveBeenCalled();
      expect(consoleError).toHaveBeenCalled();
      consoleError.mockRestore();
    });

    it('still calls selectEntity when getSourceLocation rejects (entity lacks a source span)', async () => {
      const getSourceLocation = vi
        .fn()
        .mockRejectedValue(new Error('No source location found for BooleanResult'));
      const revealLocation = vi.fn();
      const selectEntity = vi.fn();
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

      await navigateToSource('BooleanResult', {
        getSourceLocation,
        revealLocation,
        selectEntity,
      });

      // Selection must happen unconditionally — even when no source span exists
      expect(selectEntity).toHaveBeenCalledWith('BooleanResult');
      // Editor scroll is best-effort; must be skipped when no span is available
      expect(revealLocation).not.toHaveBeenCalled();
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

  describe('navigateToLocation', () => {
    type EditorStore = ReturnType<typeof createEditorStore>;

    function makeDeps(store: EditorStore) {
      return {
        store,
        openFile: vi.fn(),
        setScrollToLocation: vi.fn(),
        reportOpenFailure: vi.fn(),
      };
    }

    it('same-file → scrolls once with the span; opens and activates nothing', async () => {
      await createRoot(async (dispose) => {
        const store = createEditorStore();
        store.openFile({ path: 'main.ri', content: '' });
        const setActiveFileSpy = vi.spyOn(store, 'setActiveFile');
        const deps = makeDeps(store);
        const loc = { file_path: 'main.ri', line: 7, column: 4, end_line: 7, end_column: 9 };

        await navigateToLocation(loc, deps);

        expect(deps.setScrollToLocation).toHaveBeenCalledTimes(1);
        expect(deps.setScrollToLocation).toHaveBeenCalledWith(loc);
        expect(deps.openFile).not.toHaveBeenCalled();
        expect(setActiveFileSpy).not.toHaveBeenCalled();

        dispose();
      });
    });

    it('cross-file, already open → activates the tab and scrolls; opens nothing', async () => {
      await createRoot(async (dispose) => {
        const store = createEditorStore();
        store.openFile({ path: 'main.ri', content: '' });
        store.openFile({ path: 'helper.ri', content: '' });
        store.setActiveFile('main.ri');
        const deps = makeDeps(store);
        const loc = { file_path: 'helper.ri', line: 10, column: 3, end_line: 10, end_column: 8 };

        await navigateToLocation(loc, deps);

        expect(store.state.activeFile).toBe('helper.ri');
        expect(deps.openFile).not.toHaveBeenCalled();
        expect(deps.setScrollToLocation).toHaveBeenCalledTimes(1);
        expect(deps.setScrollToLocation).toHaveBeenCalledWith(loc);

        dispose();
      });
    });

    it('cross-file, not open → opens from disk, loads it active, then scrolls', async () => {
      await createRoot(async (dispose) => {
        const store = createEditorStore();
        store.openFile({ path: 'main.ri', content: '' });
        const deps = makeDeps(store);
        let activeAtScroll: string | null = null;
        deps.openFile.mockResolvedValue({ path: 'helper.ri', content: 'structure Helper {}' });
        deps.setScrollToLocation.mockImplementation(() => {
          activeAtScroll = store.state.activeFile;
        });
        const loc = { file_path: 'helper.ri', line: 2, column: 1, end_line: 2, end_column: 15 };

        await navigateToLocation(loc, deps);

        expect(deps.openFile).toHaveBeenCalledTimes(1);
        expect(deps.openFile).toHaveBeenCalledWith('helper.ri');
        expect(store.state.openFiles.some((f) => f.path === 'helper.ri')).toBe(true);
        expect(store.state.activeFile).toBe('helper.ri');
        expect(deps.setScrollToLocation).toHaveBeenCalledTimes(1);
        expect(deps.setScrollToLocation).toHaveBeenCalledWith(loc);
        expect(activeAtScroll).toBe('helper.ri');

        dispose();
      });
    });

    it('open rejects → reports the failure naming the path; no scroll; active tab unchanged; resolves', async () => {
      await createRoot(async (dispose) => {
        const store = createEditorStore();
        store.openFile({ path: 'main.ri', content: '' });
        const deps = makeDeps(store);
        deps.openFile.mockRejectedValue(new Error('disk read error'));
        const loc = { file_path: 'missing.ri', line: 5, column: 2, end_line: 5, end_column: 10 };

        await expect(navigateToLocation(loc, deps)).resolves.toBeUndefined();

        expect(deps.reportOpenFailure).toHaveBeenCalledTimes(1);
        expect(deps.reportOpenFailure).toHaveBeenCalledWith(expect.stringContaining('missing.ri'));
        expect(deps.setScrollToLocation).not.toHaveBeenCalled();
        expect(store.state.activeFile).toBe('main.ri');

        dispose();
      });
    });

    it('a spelling that opens to an already-open tab activates it without reloading the dirty buffer', async () => {
      await createRoot(async (dispose) => {
        const store = createEditorStore();
        store.openFile({ path: '/proj/parts.ri', content: 'edited' });
        store.markDirty('/proj/parts.ri');
        store.openFile({ path: '/proj/main.ri', content: '' });
        const deps = makeDeps(store);
        deps.openFile.mockResolvedValue({ path: '/proj/parts.ri', content: 'disk' });

        await navigateToLocation(
          { file_path: 'parts.ri', line: 3, column: 5, end_line: 3, end_column: 10 },
          deps,
        );

        expect(store.state.activeFile).toBe('/proj/parts.ri');
        expect(store.state.openFiles.find((f) => f.path === '/proj/parts.ri')?.content).toBe('edited');
        expect(store.state.externallyChanged).not.toContain('/proj/parts.ri');
        expect(deps.setScrollToLocation).toHaveBeenCalledWith({
          file_path: '/proj/parts.ri', line: 3, column: 5, end_line: 3, end_column: 10,
        });

        dispose();
      });
    });

    it('scrolls with the store key the file opened under, not the producer spelling', async () => {
      await createRoot(async (dispose) => {
        const store = createEditorStore();
        store.openFile({ path: '/real/proj/main.ri', content: '' });
        const deps = makeDeps(store);
        deps.openFile.mockResolvedValue({ path: '/real/proj/helper.ri', content: '' });

        await navigateToLocation(
          { file_path: '/link/proj/helper.ri', line: 4, column: 1, end_line: 6, end_column: 2 },
          deps,
        );

        expect(deps.setScrollToLocation).toHaveBeenCalledTimes(1);
        expect(deps.setScrollToLocation).toHaveBeenCalledWith({
          file_path: '/real/proj/helper.ri', line: 4, column: 1, end_line: 6, end_column: 2,
        });

        dispose();
      });
    });
  });
});
