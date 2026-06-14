import { describe, it, expect, vi } from 'vitest';
import { createRoot } from 'solid-js';
import type { GuiState } from '../types';
import { createDefPreviewStore } from '../stores/defPreviewStore';

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
    compile_diagnostics: [],
    tensegrity_wires: [],
    tensegrity_surfaces: [],
  };
}

describe('defPreviewStore', () => {
  describe('initial state', () => {
    it('fresh store has defName === null', () => {
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.defName).toBeNull();
        dispose();
      });
    });

    it('fresh store has empty meshes record', () => {
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.meshes).toEqual({});
        dispose();
      });
    });

    it('fresh store has isLoading === false', () => {
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.isLoading).toBe(false);
        dispose();
      });
    });

    it('fresh store has error === null', () => {
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        expect(store.state.error).toBeNull();
        dispose();
      });
    });
  });

  describe('applyPreview', () => {
    it('keys meshes by entity_path, sets defName, clears error and isLoading', () => {
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
    it('resets everything to initial state', () => {
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
    it('records the error and clears isLoading', () => {
      createRoot((dispose) => {
        const store = createDefPreviewStore();
        store.setError('boom');

        expect(store.state.error).toBe('boom');
        expect(store.state.isLoading).toBe(false);
        dispose();
      });
    });
  });

  describe('loadPreview', () => {
    it('sets isLoading=true synchronously then populates meshes on resolve', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();
          const guiState = makeGuiState('BoltFlange.body');
          let capturedLoading: boolean | undefined;

          const mockFetch = vi.fn(async (_name: string) => {
            capturedLoading = store.state.isLoading;
            return guiState;
          });

          await store.loadPreview('BoltFlange', mockFetch);

          // isLoading was true when fetch was called
          expect(capturedLoading).toBe(true);
          // After resolve, state is populated
          expect(store.state.defName).toBe('BoltFlange');
          expect(store.state.meshes['BoltFlange.body']).toBeDefined();
          expect(store.state.isLoading).toBe(false);

          dispose();
          done();
        });
      });
    });

    it('sets isLoading=false and error on reject', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();
          const mockFetch = vi.fn(async (_name: string): Promise<GuiState> => {
            throw new Error('network failure');
          });

          await store.loadPreview('BoltFlange', mockFetch);

          expect(store.state.isLoading).toBe(false);
          expect(store.state.error).toContain('network failure');

          dispose();
          done();
        });
      });
    });

    it('skips fetch when defName matches state.defName (de-duplication)', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();
          const guiState = makeGuiState('BoltFlange.body');
          const mockFetch = vi.fn(async (_name: string) => guiState);

          // First load
          await store.loadPreview('BoltFlange', mockFetch);
          expect(mockFetch).toHaveBeenCalledTimes(1);

          // Second load with same defName — should skip
          await store.loadPreview('BoltFlange', mockFetch);
          expect(mockFetch).toHaveBeenCalledTimes(1);

          dispose();
          done();
        });
      });
    });
  });

  // ── Race condition tests ─────────────────────────────────────────────────────

  describe('race condition', () => {
    it('(a) stale slow fetch result is discarded when a newer fast fetch resolves first', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();

          let resolveSlowFetch!: (gs: GuiState) => void;
          let resolveFastFetch!: (gs: GuiState) => void;
          const slowFetchPromise = new Promise<GuiState>(r => { resolveSlowFetch = r; });
          const fastFetchPromise = new Promise<GuiState>(r => { resolveFastFetch = r; });

          const gsA = makeGuiState('A.body');
          const gsB = makeGuiState('B.body');

          // Start slow fetch for 'A' (not awaited)
          const slowLoad = store.loadPreview('A', () => slowFetchPromise);
          // Start fast fetch for 'B' (not awaited)
          const fastLoad = store.loadPreview('B', () => fastFetchPromise);

          // Fast fetch resolves first with gsB
          resolveFastFetch(gsB);
          await fastLoad;

          expect(store.state.defName).toBe('B');
          expect(store.state.meshes['B.body']).toBeDefined();
          expect(store.state.meshes['A.body']).toBeUndefined();

          // Now slow fetch resolves with gsA (stale)
          resolveSlowFetch(gsA);
          await slowLoad;

          // Stale result must NOT have overwritten B
          expect(store.state.defName).toBe('B');
          expect(store.state.meshes['B.body']).toBeDefined();
          expect(store.state.meshes['A.body']).toBeUndefined();

          dispose();
          done();
        });
      });
    });

    it('(b) stale slow fetch error does not overwrite state set by a newer fetch', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();

          let rejectSlowFetch!: (err: unknown) => void;
          let resolveFastFetch!: (gs: GuiState) => void;
          // Only rejectSlowFetch is used — the slow fetch is always rejected in this scenario.
          const slowFetchPromise = new Promise<GuiState>((_res, rej) => { rejectSlowFetch = rej; });
          const fastFetchPromise = new Promise<GuiState>(r => { resolveFastFetch = r; });

          const gsB = makeGuiState('B.body');

          // Start slow fetch for 'A'
          const slowLoad = store.loadPreview('A', () => slowFetchPromise);
          // Start fast fetch for 'B'
          const fastLoad = store.loadPreview('B', () => fastFetchPromise);

          // Fast resolves first
          resolveFastFetch(gsB);
          await fastLoad;

          expect(store.state.defName).toBe('B');
          expect(store.state.error).toBeNull();

          // Slow rejects (stale error)
          rejectSlowFetch(new Error('stale network failure'));
          await slowLoad;

          // Stale error must NOT have been recorded
          expect(store.state.error).toBeNull();
          expect(store.state.defName).toBe('B');

          dispose();
          done();
        });
      });
    });

    it('(c) pure happy path: single loadPreview still populates state correctly after guard', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();
          const gsA = makeGuiState('A.body');

          await store.loadPreview('A', async () => gsA);

          expect(store.state.defName).toBe('A');
          expect(store.state.meshes['A.body']).toBeDefined();
          expect(store.state.isLoading).toBe(false);
          expect(store.state.error).toBeNull();

          dispose();
          done();
        });
      });
    });

    it('(d) clearPreview() invalidates an in-flight loadPreview so stale results do not reappear', async () => {
      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const store = createDefPreviewStore();

          let resolveSlowFetch!: (gs: GuiState) => void;
          const slowFetchPromise = new Promise<GuiState>(r => { resolveSlowFetch = r; });

          const gsA = makeGuiState('A.body');

          // Start slow fetch for 'A' — do NOT await it yet
          const slowLoad = store.loadPreview('A', () => slowFetchPromise);

          // User navigates away: clear the preview
          store.clearPreview();

          // Now the slow fetch resolves with a valid GuiState for 'A'
          resolveSlowFetch(gsA);
          await slowLoad;

          // clearPreview() should have invalidated the in-flight fetch;
          // the stale applyPreview must NOT have re-set defName to 'A'
          expect(store.state.defName).toBeNull();
          expect(store.state.meshes).toEqual({});
          expect(store.state.isLoading).toBe(false);

          dispose();
          done();
        });
      });
    });
  });
});
