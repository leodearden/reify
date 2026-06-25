/**
 * Selective-demand ENFORCEMENT frontend wiring (task 4737 α, step-11/12).
 *
 * The PRODUCTION counterpart to the 4532 PASSIVE `syncObservedDemand`
 * measurement channel. Pins the new production demand-sync path:
 *
 *   - `engineStore.syncDemand(getEffectiveVisibility)` pushes the viewport-visible
 *     realization mesh keys (`show` + `ghost`, EXCLUDING `hidden`) to the new
 *     bridge `syncDemand` command — reusing the exact filter the 4532
 *     `syncObservedDemand` uses;
 *   - `createSelectiveDemandSync(engineStore, viewStateStore, { debounceMs })`
 *     wires a DEBOUNCED, NON-idle-gated effect that fires that sync whenever
 *     effective visibility changes via `viewStateStore.setVisibility` /
 *     `cycleCascading`, coalescing a rapid toggle burst into a single backend
 *     round-trip (PRD §12 Q3: fire on the toggle itself, debounced, not solely
 *     at phase==='idle'; Q4: ghost stays demanded, only hidden prunes).
 *
 * RED until step-12 adds `bridge.syncDemand`, `engineStore.syncDemand`, and
 * `createSelectiveDemandSync`.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { MeshData, EntityTreeNode, VisibilityState } from '../types';

// Mock the bridge — engineStore imports the full event/command surface at module
// load, so every named import it uses must be present (mirrors engineStore.test).
vi.mock('../bridge', () => ({
  onMeshUpdate: vi.fn(),
  onValueUpdate: vi.fn(),
  onConstraintUpdate: vi.fn(),
  onEvaluationStatus: vi.fn(),
  onMeshRemoved: vi.fn(),
  onValueRemoved: vi.fn(),
  onConstraintRemoved: vi.fn(),
  onTessellationDiagnostics: vi.fn(),
  onCompileDiagnostics: vi.fn(),
  onAutoResolveStart: vi.fn(),
  onAutoResolveIteration: vi.fn(),
  onAutoResolveComplete: vi.fn(),
  onSolverProgress: vi.fn(() => Promise.resolve(() => {})),
  cancelSolve: vi.fn(() => Promise.resolve()),
  syncObservedDemand: vi.fn(() => Promise.resolve()),
  syncDemand: vi.fn(() => Promise.resolve()),
}));

import { syncDemand as bridgeSyncDemand } from '../bridge';
import { createEngineStore, createSelectiveDemandSync } from '../stores/engineStore';
import { createViewStateStore } from '../stores/viewStateStore';

const mockSyncDemand = vi.mocked(bridgeSyncDemand);

function mesh(entity_path: string): MeshData {
  return {
    entity_path,
    vertices: new Float32Array([0, 0, 0]),
    indices: new Uint32Array([0]),
    normals: new Float32Array([0, 0, 1]),
  };
}

// Realization leaf nodes carry the mesh key as `entity_path` (types.ts:426-430),
// so `getEffectiveVisibility(meshKey)` resolves and toggles map to engine meshes.
function realizationNode(meshKey: string): EntityTreeNode {
  return {
    entity_path: meshKey,
    kind: 'realization',
    type_name: null,
    has_mesh: true,
    trait_geometry: false,
    freshness: 'final',
    children: [],
  };
}

const R0 = 'S#realization[0]';
const R1 = 'S#realization[1]';
const R2 = 'S#realization[2]';

describe('selective-demand ENFORCEMENT sync (task 4737 α)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
  });
  afterEach(() => {
    // Belt-and-suspenders FM1 guard (task 4856): clear any fake debounce timer
    // still pending at the end of each test before restoring real timers, so
    // nothing can fire past this file's hoisted vi.mock('../bridge') teardown
    // and hit the createError teardown-race under event-loop starvation. This
    // is harness hygiene on top of the production onCleanup (step-2) — a
    // clearAllTimers here catches any timer that slipped through regardless of
    // dispose ordering. Must run BEFORE useRealTimers() so the clear targets
    // fake-timer state while it is still active.
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it('(a) syncDemand pushes show+ghost realizations and EXCLUDES hidden', async () => {
    const engine = createEngineStore();
    engine.applyMeshUpdate(mesh(R0));
    engine.applyMeshUpdate(mesh(R1));
    engine.applyMeshUpdate(mesh(R2));
    const vis: Record<string, VisibilityState> = { [R0]: 'show', [R1]: 'ghost', [R2]: 'hidden' };

    await engine.syncDemand((p: string) => vis[p] ?? 'show');

    expect(mockSyncDemand).toHaveBeenCalledTimes(1);
    const payload = mockSyncDemand.mock.calls[0][0];
    // ghost (R1) is a viewport demand source; only hidden (R2) is pruned.
    expect(new Set(payload)).toEqual(new Set([R0, R1]));
  });

  it('(b) a visibility-toggle burst fires a single DEBOUNCED sync_demand, even when phase!==idle', async () => {
    // createRoot's callback is synchronous; run the async body as a detached IIFE
    // and settle the outer promise via resolve/reject so a throw (e.g. a missing
    // `createSelectiveDemandSync`) fails FAST instead of hanging to the timeout.
    await new Promise<void>((resolve, reject) => {
      createRoot((dispose) => {
        void (async () => {
          try {
            const engine = createEngineStore();
            engine.applyMeshUpdate(mesh(R0));
            engine.applyMeshUpdate(mesh(R1));
            // NON-idle phase: enforcement must NOT be gated behind phase==='idle'
            // (unlike the 4532 idle-gated measurement channel).
            engine.setEvalStatus({ phase: 'evaluating' });

            const view = createViewStateStore();
            view.setTree([realizationNode(R0), realizationNode(R1)]);
            // Deterministic baseline BEFORE wiring (no fire — the effect is deferred).
            view.setVisibility(R0, 'show');
            view.setVisibility(R1, 'show');

            createSelectiveDemandSync(engine, view, { debounceMs: 150 });

            // Rapid burst of toggles, each inside the 150ms debounce window.
            view.setVisibility(R1, 'ghost'); // toggle 1
            await vi.advanceTimersByTimeAsync(50);
            view.cycleCascading(R0); // toggle 2: show -> ghost
            await vi.advanceTimersByTimeAsync(50);
            view.setVisibility(R1, 'hidden'); // toggle 3: final state hidden

            // Still inside the debounce window — nothing has fired yet.
            expect(mockSyncDemand).not.toHaveBeenCalled();

            // Quiet past the debounce → exactly ONE coalesced sync for the burst.
            await vi.advanceTimersByTimeAsync(300);
            expect(mockSyncDemand).toHaveBeenCalledTimes(1);

            // Final payload reflects the settled state: R0 ghost (visible), R1
            // hidden (pruned) — ghost stays demanded, only hidden is excluded.
            const payload = mockSyncDemand.mock.calls.at(-1)![0];
            expect(new Set(payload)).toEqual(new Set([R0]));

            resolve();
          } catch (err) {
            reject(err);
          } finally {
            dispose();
          }
        })();
      });
    });
  });

  it('(c) disposing the createSelectiveDemandSync owner before the debounce window elapses clears the pending timer — sync_demand is NOT called past unmount', async () => {
    // Mirrors test (b)'s createRoot + detached-async-IIFE settle pattern.
    // Verifies that onCleanup in createSelectiveDemandSync clears the pending
    // debounce timer when the reactive owner is disposed before the window elapses,
    // preventing a post-dispose engineStore.syncDemand -> bridgeSyncDemand call
    // that would hit the vi.mock('../bridge') teardown-race (FM1, task 4856).
    //
    // Key ordering invariant: the deferred effect's initial tracking run MUST
    // capture the 'show' baseline before the 'ghost' toggle happens. Solid.js
    // schedules the initial tracking via queueMicrotask; the `await
    // Promise.resolve()` below yields to the microtask queue so that tracking
    // completes and the effect subscribes to the signals BEFORE we call
    // setVisibility('ghost'). Without this flush the first toggle would be
    // captured as the "initial" state (defer:true skips the callback on the
    // first run), the callback would never fire, no timer would be scheduled,
    // and the test would pass vacuously — masking the FM1 bug.
    await new Promise<void>((resolve, reject) => {
      createRoot((dispose) => {
        void (async () => {
          try {
            const engine = createEngineStore();
            engine.applyMeshUpdate(mesh(R0));

            const view = createViewStateStore();
            view.setTree([realizationNode(R0)]);
            view.setVisibility(R0, 'show');

            createSelectiveDemandSync(engine, view, { debounceMs: 150 });

            // Flush the deferred effect's initial tracking run so it captures
            // 'show' as the baseline state. After this point the effect has
            // subscribed to the signals and will run its callback on subsequent
            // changes (not just silently track them as the initial state).
            await Promise.resolve();

            // Toggle visibility — the effect now sees a CHANGE ('show' → 'ghost')
            // and on the next microtask drain will schedule the debounce timer.
            view.setVisibility(R0, 'ghost');

            // Advance 50ms. Before advancing, microtasks drain and the effect
            // callback runs (scheduling setTimeout(callback, 150) at fake-time 0).
            // After 50ms fake time we are still inside the 150ms debounce window.
            await vi.advanceTimersByTimeAsync(50);
            expect(mockSyncDemand).not.toHaveBeenCalled();

            // Dispose the reactive owner BEFORE the debounce elapses.
            // RED (without onCleanup): the raw setTimeout at t=150 is still
            // pending and will fire 100ms into the next advance, calling
            // engineStore.syncDemand → bridgeSyncDemand (mockSyncDemand) and
            // making the final assertion fail.
            // GREEN (with onCleanup): clearTimeout(timer) cancels the pending
            // timer; the advance completes with no calls.
            dispose();

            // Advance well past the debounce window.
            await vi.advanceTimersByTimeAsync(300);
            expect(mockSyncDemand).not.toHaveBeenCalled();

            resolve();
          } catch (err) {
            reject(err);
          }
          // dispose() was already called above — do not call again in finally.
        })();
      });
    });
  });
});
