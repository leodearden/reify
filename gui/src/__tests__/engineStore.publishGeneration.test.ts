import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { GuiState, ValueData, FeaDiagnosticInfo } from '../types';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnValueUpdate,
  mockOnFeaDiagnosticsChanged,
  mockOnEvalGeneration,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { sampleMesh, sampleValue, sampleIteration } from './engineStoreFixtures';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

// ── task 7853: a whole-state snapshot never overwrites newer event-applied state ──

describe('engineStore publish-generation guard', () => {
  const width = (value: string): ValueData => ({ ...sampleValue, value });

  function snapshot(value: string, feaDiagnostics: FeaDiagnosticInfo[] = []): GuiState {
    return {
      fea_convergence: null,
      meshes: [sampleMesh],
      values: [width(value)],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
      tensegrity_surfaces: [],
      display_panes: [],
      display_appearance: [],
      fea_diagnostics: feaDiagnostics,
    };
  }

  /** A store subscribed to its events, and the callbacks it handed the bridge. */
  async function subscribedStore() {
    let announce: ((generation: number) => void) | undefined;
    let valueUpdate: ((value: ValueData) => void) | undefined;
    let feaDiagnosticsChanged: ((diags: FeaDiagnosticInfo[]) => void) | undefined;
    mockOnEvalGeneration.mockImplementation(async (cb) => {
      announce = cb;
      return vi.fn();
    });
    mockOnValueUpdate.mockImplementation(async (cb) => {
      valueUpdate = cb;
      return vi.fn();
    });
    mockOnFeaDiagnosticsChanged.mockImplementation(async (cb) => {
      feaDiagnosticsChanged = cb as (diags: FeaDiagnosticInfo[]) => void;
      return vi.fn();
    });
    const onEngineReinitialized = vi.fn();
    const store = createEngineStore({ onEngineReinitialized });
    await store.subscribeToEvents();
    return {
      store,
      onEngineReinitialized,
      announce: announce!,
      valueUpdate: valueUpdate!,
      feaDiagnosticsChanged: feaDiagnosticsChanged!,
    };
  }

  const shownWidth = (store: ReturnType<typeof createEngineStore>) =>
    store.state.values[sampleValue.cell_id]?.value;

  it('applyPublishedState applies a snapshot when no newer generation was announced', async () => {
    await createRoot(async (dispose) => {
      const { store, onEngineReinitialized } = await subscribedStore();

      store.applyPublishedState({ generation: 1, state: snapshot('50') });

      expect(shownWidth(store)).toBe('50');
      expect(store.state.meshes[sampleMesh.entity_path]).toEqual(sampleMesh);
      expect(onEngineReinitialized).toHaveBeenCalledOnce();
      dispose();
    });
  });

  it('a snapshot older than an announced generation leaves event-applied state in place', async () => {
    await createRoot(async (dispose) => {
      const { store, onEngineReinitialized, announce, valueUpdate, feaDiagnosticsChanged } =
        await subscribedStore();
      store.applyPublishedState({ generation: 3, state: snapshot('50') });
      onEngineReinitialized.mockClear();

      announce(5);
      valueUpdate(width('120'));
      const live: FeaDiagnosticInfo[] = [{ kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] }];
      feaDiagnosticsChanged(live);
      store.applyPublishedState({
        generation: 4,
        state: snapshot('80', [{ kind: 'Unconstrained', rigid_body_modes: ['TranslationY'] }]),
      });

      expect(shownWidth(store)).toBe('120');
      expect(store.state.feaDiagnostics).toEqual(live);
      expect(onEngineReinitialized).toHaveBeenCalledOnce();
      dispose();
    });
  });

  /** Run one whole auto-resolve loop, as the engine fires it on a re-eval. */
  function runAutoResolveLoop(store: ReturnType<typeof createEngineStore>) {
    store.beginAutoResolveLoop();
    store.applyAutoResolveIteration(sampleIteration);
    store.endAutoResolveLoop();
  }

  it('a reply clears an auto-resolve loop an older generation began, even when it is stale', async () => {
    await createRoot(async (dispose) => {
      const { store, announce } = await subscribedStore();
      store.applyPublishedState({ generation: 3, state: snapshot('50') });
      runAutoResolveLoop(store);

      announce(5);
      store.applyPublishedState({ generation: 4, state: snapshot('80') });

      expect(store.state.autoResolve).toEqual({ active: false, iterations: [] });
      dispose();
    });
  });

  it('a stale reply keeps the auto-resolve loop a newer generation began', async () => {
    await createRoot(async (dispose) => {
      const { store, onEngineReinitialized, announce } = await subscribedStore();
      store.applyPublishedState({ generation: 3, state: snapshot('50') });
      onEngineReinitialized.mockClear();

      announce(5);
      runAutoResolveLoop(store);
      store.applyPublishedState({ generation: 4, state: snapshot('80') });

      expect(store.state.autoResolve.iterations).toEqual([sampleIteration]);
      expect(store.state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      expect(onEngineReinitialized).toHaveBeenCalledOnce();
      dispose();
    });
  });

  it('a reply keeps the auto-resolve loop its own generation began', async () => {
    await createRoot(async (dispose) => {
      const { store, announce } = await subscribedStore();
      store.applyPublishedState({ generation: 1, state: snapshot('50') });

      announce(2);
      runAutoResolveLoop(store);
      store.applyPublishedState({ generation: 2, state: snapshot('80') });

      expect(shownWidth(store)).toBe('80');
      expect(store.state.autoResolve.iterations).toEqual([sampleIteration]);
      dispose();
    });
  });

  it('a snapshot of the announced generation itself is applied', async () => {
    await createRoot(async (dispose) => {
      const { store, announce } = await subscribedStore();
      store.applyPublishedState({ generation: 1, state: snapshot('50') });

      announce(5);
      store.applyPublishedState({ generation: 5, state: snapshot('80') });

      expect(shownWidth(store)).toBe('80');
      dispose();
    });
  });

  it('an applied snapshot raises the newest generation', async () => {
    await createRoot(async (dispose) => {
      const { store } = await subscribedStore();

      store.applyPublishedState({ generation: 7, state: snapshot('70') });
      store.applyPublishedState({ generation: 6, state: snapshot('60') });

      expect(shownWidth(store)).toBe('70');
      dispose();
    });
  });

  it('announcements never lower the newest generation', async () => {
    await createRoot(async (dispose) => {
      const { store, announce } = await subscribedStore();
      store.applyPublishedState({ generation: 1, state: snapshot('50') });

      announce(9);
      announce(4);
      store.applyPublishedState({ generation: 6, state: snapshot('60') });

      expect(shownWidth(store)).toBe('50');
      dispose();
    });
  });
});
