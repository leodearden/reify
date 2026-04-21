import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import type { GuiState } from '../types';

// ── Helper to build a minimal GuiState with one mesh ────────────────────────
function makeGuiState(entityPath: string): GuiState {
  return {
    meshes: [
      {
        entity_path: entityPath,
        vertices: new Float32Array([0, 1, 2]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      },
    ],
    values: [],
    constraints: [],
    files: [],
    tessellation_diagnostics: [],
  };
}

// Lazy import so TypeScript doesn't complain about not-yet-existing module
// during the test (step-7 = red; step-8 = green).
async function importStore() {
  return import('../stores/defPreviewStore');
}

describe('defPreviewStore', () => {
  describe('initial state', () => {
    it('fresh store has defName === null', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.defName).toBeNull();
        dispose();
      });
    });

    it('fresh store has empty meshes record', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.meshes).toEqual({});
        dispose();
      });
    });

    it('fresh store has isLoading === false', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.isLoading).toBe(false);
        dispose();
      });
    });

    it('fresh store has error === null', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.error).toBeNull();
        dispose();
      });
    });
  });

  describe('applyPreview', () => {
    it('keys meshes by entity_path, sets defName, clears error and isLoading', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        const guiState = makeGuiState('BoltFlange.body');
        store.applyPreview('BoltFlange', guiState);

        expect(store.state.defName).toBe('BoltFlange');
        expect(store.state.meshes['BoltFlange.body']).toBeDefined();
        expect(store.state.meshes['BoltFlange.body'].entity_path).toBe('BoltFlange.body');
        expect(store.state.error).toBeNull();
        expect(store.state.isLoading).toBe(false);
        dispose();
      });
    });
  });

  describe('clearPreview', () => {
    it('resets everything to initial state', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        store.applyPreview('BoltFlange', makeGuiState('BoltFlange.body'));
        store.clearPreview();

        expect(store.state.defName).toBeNull();
        expect(store.state.meshes).toEqual({});
        expect(store.state.isLoading).toBe(false);
        expect(store.state.error).toBeNull();
        dispose();
      });
    });
  });

  describe('setError', () => {
    it('records the error and clears isLoading', async () => {
      const { createDefPreviewStore } = await importStore();
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        store.setError('boom');

        expect(store.state.error).toBe('boom');
        expect(store.state.isLoading).toBe(false);
        dispose();
      });
    });
  });
});
