import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import type { MeshData, AppearanceDirective, DisplayStyleData } from '../types';
import { flushMacrotasks } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';
import { createViewportStore } from '../stores/viewportStore';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStubModule());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStubModule());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStubModule());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import { computePaneGroups, reconcilePaneViewports, syncActiveViewToViewports, collectViewportLayout, applyViewportLayout, computeAppearanceOverrides } from '../App';
import * as bridge from '../bridge';
import './appHarness/harness';
import { renderAndWaitForReady } from './appHarness/harness';
import { capturedMultiViewportProps } from './appHarness/renderStubs';
import { makeMesh } from './test_utils/meshData';

// ── computePaneGroups unit tests (task-4767 δ) ───────────────────────────────

describe('computePaneGroups unit tests (task-4767 δ)', () => {
  it('case 1: empty displayPanes → all meshes land in pane 0, dropped empty', () => {
    const meshes = { a: makeMesh('a'), b: makeMesh('b') };
    const result = computePaneGroups([], meshes);
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0].pane).toBe(0);
    expect(Object.keys(result.groups[0].meshes).sort()).toEqual(['a', 'b']);
    expect(result.dropped).toEqual([]);
  });

  it('case 2: one directive routes mesh b to pane 1; unrouted a stays in pane 0, groups sorted ascending', () => {
    const meshes = { a: makeMesh('a'), b: makeMesh('b') };
    const result = computePaneGroups([{ subject: 'b', pane: 1 }], meshes);
    expect(result.groups).toHaveLength(2);
    expect(result.groups[0].pane).toBe(0);
    expect(Object.keys(result.groups[0].meshes)).toEqual(['a']);
    expect(result.groups[1].pane).toBe(1);
    expect(Object.keys(result.groups[1].meshes)).toEqual(['b']);
    expect(result.dropped).toEqual([]);
  });

  it('case 3: explicit pane:0 directive keeps mesh in design-main, no extra pane created', () => {
    const meshes = { a: makeMesh('a') };
    const result = computePaneGroups([{ subject: 'a', pane: 0 }], meshes);
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0].pane).toBe(0);
    expect(Object.keys(result.groups[0].meshes)).toEqual(['a']);
    expect(result.dropped).toEqual([]);
  });

  it('case 4: many-to-one — two directives to pane 1 both land in pane 1 (inv.3)', () => {
    const meshes = { b: makeMesh('b'), c: makeMesh('c') };
    const result = computePaneGroups(
      [{ subject: 'b', pane: 1 }, { subject: 'c', pane: 1 }],
      meshes,
    );
    expect(result.groups).toHaveLength(2); // pane 0 (empty, always present) + pane 1
    const pane1 = result.groups.find(g => g.pane === 1)!;
    expect(Object.keys(pane1.meshes).sort()).toEqual(['b', 'c']);
    expect(result.dropped).toEqual([]);
  });

  it('case 5: dangling directive (no realized mesh) → dropped, no phantom pane (Open-Q3/inv.1)', () => {
    const meshes = { a: makeMesh('a') };
    const result = computePaneGroups([{ subject: 'ghost', pane: 2 }], meshes);
    expect(result.groups).toHaveLength(1); // only pane 0, no pane 2 materialized
    expect(result.groups[0].pane).toBe(0);
    expect(Object.keys(result.groups[0].meshes)).toEqual(['a']);
    expect(result.dropped).toEqual([{ subject: 'ghost', pane: 2 }]);
  });

  it('case 6a: pane 0 always present even when all meshes route to pane 1', () => {
    const meshes = { b: makeMesh('b') };
    const result = computePaneGroups([{ subject: 'b', pane: 1 }], meshes);
    const pane0 = result.groups.find(g => g.pane === 0);
    expect(pane0).toBeDefined();
    expect(Object.keys(pane0!.meshes)).toEqual([]);
  });

  it('case 6b: pane 0 always present when meshes is empty', () => {
    const result = computePaneGroups([], {});
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0].pane).toBe(0);
    expect(result.groups[0].meshes).toEqual({});
    expect(result.dropped).toEqual([]);
  });
});

// ── reconcilePaneViewports unit tests (task-4767 δ) ──────────────────────────

describe('reconcilePaneViewports unit tests (task-4767 δ)', () => {
  it('case 1: wanted [1,2] → pane-1 and pane-2 added; design-main/def-preview untouched', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      reconcilePaneViewports(store, [1, 2]);
      expect(store.state.viewports['pane-1']).toBeDefined();
      expect(store.state.viewports['pane-1'].type).toBe('pane');
      expect(store.state.viewports['pane-1'].paneIndex).toBe(1);
      expect(store.state.viewports['pane-2']).toBeDefined();
      expect(store.state.viewports['pane-2'].type).toBe('pane');
      expect(store.state.viewports['pane-2'].paneIndex).toBe(2);
      expect(store.state.viewports['design-main']).toBeDefined();
      expect(store.state.viewports['def-preview']).toBeDefined();
      dispose();
    });
  });

  it('case 2: eviction — pane-2 removed when wanted goes from [1,2] to [1] (Open-Q4)', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      store.addPane(1);
      store.addPane(2);
      expect(store.state.viewports['pane-1']).toBeDefined();
      expect(store.state.viewports['pane-2']).toBeDefined();

      reconcilePaneViewports(store, [1]);

      expect(store.state.viewports['pane-1']).toBeDefined();
      expect(store.state.viewports['pane-2']).toBeUndefined();
      expect(store.state.viewports['design-main']).toBeDefined();
      expect(store.state.viewports['def-preview']).toBeDefined();
      dispose();
    });
  });

  it('case 3: wanted [] → all pane-type viewports removed; design-main/def-preview remain', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      store.addPane(1);
      store.addPane(2);

      reconcilePaneViewports(store, []);

      expect(store.state.viewports['pane-1']).toBeUndefined();
      expect(store.state.viewports['pane-2']).toBeUndefined();
      expect(store.state.viewports['design-main']).toBeDefined();
      expect(store.state.viewports['def-preview']).toBeDefined();
      dispose();
    });
  });

  it('case 4: pane index 0 in wanted is ignored — never creates pane-0 (design-main is the alias)', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      reconcilePaneViewports(store, [0]);
      expect(store.state.viewports['pane-0']).toBeUndefined();
      expect(store.state.viewports['design-main']).toBeDefined();
      const keys = Object.keys(store.state.viewports);
      expect(keys).toEqual(expect.arrayContaining(['design-main', 'def-preview']));
      expect(keys).not.toContain('pane-0');
      dispose();
    });
  });
});

// ── syncActiveViewToViewports unit tests (task-4767 δ) ───────────────────────

describe('syncActiveViewToViewports unit tests (task-4767 δ)', () => {
  it('assigns activeViewId to design-main and pane-1, but NOT to def-preview', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      store.addPane(1);

      syncActiveViewToViewports(store, 'view-x');

      expect(store.state.viewports['design-main'].viewId).toBe('view-x');
      expect(store.state.viewports['pane-1'].viewId).toBe('view-x');
      // def-preview is editor-cursor-driven — must NOT be touched
      expect(store.state.viewports['def-preview'].viewId).toBeNull();
      dispose();
    });
  });

  it('passes null activeViewId through to non-def-preview viewports', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      // Pre-assign a viewId to design-main
      store.assignView('design-main', 'old-view');
      store.assignView('def-preview', 'cursor-view');

      syncActiveViewToViewports(store, null);

      expect(store.state.viewports['design-main'].viewId).toBeNull();
      // def-preview must remain untouched
      expect(store.state.viewports['def-preview'].viewId).toBe('cursor-view');
      dispose();
    });
  });
});

// ── App N-pane render integration tests (task-4767 δ) ────────────────────────

describe('App N-pane render integration tests (task-4767 δ)', () => {
  it('step-9 case A: display_panes routing B to pane 1 → MultiViewport renders, DualViewport absent', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('A#realization[0]'), makeMesh('B#realization[0]')],
      display_panes: [
        { subject: 'A#realization[0]', pane: 0 },
        { subject: 'B#realization[0]', pane: 1 },
      ],
    })));
    await renderAndWaitForReady();
    // MultiViewport IS rendered (capturedMultiViewportProps populated by mock)
    expect(capturedMultiViewportProps.panes).toBeDefined();
    // DualViewport is NOT rendered (Show fallback not active)
    expect(screen.queryByTestId('dual-viewport')).toBeNull();
    // Panes array has 2 entries: pane 0 (design-main) + pane 1
    expect(capturedMultiViewportProps.panes).toHaveLength(2);
    // Pane 0 = design-main: mesh A + tensegrity props + fit/fly refs + select/hover
    const mainPane = capturedMultiViewportProps.panes[0];
    expect(mainPane.viewportId).toBe('design-main');
    expect(Object.keys(mainPane.meshes)).toContain('A#realization[0]');
    expect(mainPane.tensegrityWires).toBeDefined();
    expect(mainPane.tensegritySurfaces).toBeDefined();
    expect(mainPane.fitToViewRef).toBeDefined();
    expect(mainPane.flyToEntityRef).toBeDefined();
    expect(mainPane.onSelect).toBeDefined();
    expect(mainPane.onHover).toBeDefined();
    // Pane 1: mesh B only; no tensegrity / fitToViewRef / flyToEntityRef
    const pane1 = capturedMultiViewportProps.panes[1];
    expect(pane1.viewportId).toBe('pane-1');
    expect(Object.keys(pane1.meshes)).toContain('B#realization[0]');
    expect(pane1.tensegrityWires).toBeUndefined();
    expect(pane1.fitToViewRef).toBeUndefined();
    expect(pane1.onSelect).toBeDefined();
    expect(pane1.onHover).toBeDefined();
  });

  it('#5670: the FEA trio reaches EVERY pane, each with its own keyed store', async () => {
    // Non-empty fea_diagnostics + non-null fea_convergence so the forwarded values
    // are distinguishable from engineStore's empty/null defaults.
    const feaDiagnostics = [{ kind: 'ProblemElements' as const, ids: [3, 5] }];
    const feaConvergence = { converged: false, reason: 'MaxDofs' };
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      fea_convergence: feaConvergence,
      meshes: [makeMesh('A#realization[0]'), makeMesh('B#realization[0]')],
      display_panes: [
        { subject: 'A#realization[0]', pane: 0 },
        { subject: 'B#realization[0]', pane: 1 },
      ],
      fea_diagnostics: feaDiagnostics,
    })));

    await renderAndWaitForReady();
    expect(capturedMultiViewportProps.panes).toHaveLength(2);

    // ── Pane 0 (design-main) carries the full FEA trio ────────────────────────
    const mainPane = capturedMultiViewportProps.panes[0];
    expect(mainPane.viewportId).toBe('design-main');
    // A real FeaModeStore, not a stub — mirrors the DualViewport δ assertions.
    expect(mainPane.feaModeStore).toBeDefined();
    expect(typeof mainPane.feaModeStore.setEnabled).toBe('function');
    expect(mainPane.feaModeStore.state.enabled).toBe(false);
    expect(mainPane.feaDiagnostics).toHaveLength(feaDiagnostics.length);
    expect(mainPane.feaConvergence).toEqual(feaConvergence);
    const panes0Store = mainPane.feaModeStore;

    // ── Pane 1 (model pane) gets the trio too ─────────────────────────────────
    // The old pane-0-only carve-out existed because <Viewport> renders its own
    // <FeaModeToolbar> whenever feaModeStore is present, and N toolbars all
    // driving ONE store tripped the bridge's selectAmbiguous guard. #5670
    // dissolves that: each pane gets its OWN store, and each toolbar is stamped
    // with data-viewport-id so the bridge can address it individually.
    const pane1 = capturedMultiViewportProps.panes[1];
    expect(pane1.viewportId).toBe('pane-1');
    expect(pane1.feaModeStore).toBeDefined();
    expect(typeof pane1.feaModeStore.setEnabled).toBe('function');
    expect(pane1.feaModeStore.state.enabled).toBe(false);

    // fea_diagnostics/fea_convergence are GLOBAL to the model, not per-pane.
    // Withholding them from pane ≥ 1 was the concrete defect #5670 closes: a
    // display_panes directive routing an FEA-relevant subject to a model pane
    // made that entity's overlay silently vanish, because pane 0's
    // diagnosticOverlay.sync intersects the global diagnostics with pane 0's
    // OWN meshes and finds none, while no other pane was handed them at all.
    expect(pane1.feaDiagnostics).toEqual(mainPane.feaDiagnostics);
    expect(pane1.feaDiagnostics).toHaveLength(feaDiagnostics.length);
    expect(pane1.feaConvergence).toEqual(feaConvergence);

    // ── Keyed, not shared ─────────────────────────────────────────────────────
    // The failure mode a naive "just delete the carve-out" fix produces is one
    // shared instance handed to everyone: every pane would then render a toolbar
    // driving the SAME state, so switching pane 1's channel would silently
    // switch pane 0's too. The registry is keyed by viewportId precisely to
    // prevent that.
    expect(panes0Store).not.toBe(pane1.feaModeStore);

    // NOTE: "the registry is App-scope, not built inside the mapArray mapper" is
    // NOT assertable from `panes[0].feaModeStore` alone, however many reactive
    // pulses you push through it. computePaneGroups always emits a pane-0
    // bucket, so key 0 never leaves the mapArray key list, the mapper never
    // re-runs, and a mapper-local registry would be exactly as stable. The
    // discriminator is a reference that does NOT come through the mapper:
    // <FeaModeDebugRegistrar> is handed the registry directly, so the
    // `ctx.feaModes` identity assertions in 'App feaMode debug-context
    // registration' below are what actually pin it.
  });

  it('step-9 case B: empty display_panes → DualViewport renders, MultiViewport absent (back-compat inv.2)', async () => {
    // Default mock (beforeEach) has display_panes: [] — no model panes → DualViewport fallback
    await renderAndWaitForReady();
    expect(screen.queryByTestId('dual-viewport')).toBeTruthy();
    expect(screen.queryByTestId('multi-viewport')).toBeNull();
  });

  it('step-11: dangling directive (no realized mesh) → console.warn logged, no phantom pane, DualViewport renders', async () => {
    // Spy on console.warn BEFORE rendering so we capture all warnings during render.
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('A#realization[0]')],
      // Ghost has no realized mesh → dropped directive (Open-Q3/inv.1/boundary scenario 7)
      display_panes: [{ subject: 'Ghost#realization[0]', pane: 2 }],
    })));
    try {
      await renderAndWaitForReady();
      // dropped directive MUST be logged (step-12 impl makes this green)
      const warnMessages = warnSpy.mock.calls.map(args => String(args.join(' ')));
      expect(warnMessages.some(msg => msg.includes('Ghost#realization[0]'))).toBe(true);
      // No phantom pane-2: only directive is dangling → hasModelPanes false →
      // DualViewport fallback, MultiViewport NOT rendered.
      expect(screen.queryByTestId('dual-viewport')).toBeTruthy();
      expect(screen.queryByTestId('multi-viewport')).toBeNull();
    } finally {
      warnSpy.mockRestore();
    }
  });
});

// ── collectViewportLayout / applyViewportLayout unit tests (task-4768 ε) ─────

describe('collectViewportLayout unit tests (task-4768 ε)', () => {
  it('returns sizeWeight and forceExpanded for each viewport, cameras NOT included', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      store.setSizeWeight('design-main', 4);
      store.setForceExpanded('design-main', true);

      const layout = collectViewportLayout(store.state.viewports);

      // design-main: should reflect the mutations
      expect(layout['design-main']).toEqual({ sizeWeight: 4, forceExpanded: true });
      // def-preview: default values
      expect(layout['def-preview']).toEqual({ sizeWeight: 1, forceExpanded: false });
      // No camera/viewId/other fields — only sizeWeight + forceExpanded
      expect('camera' in layout['design-main']).toBe(false);
      expect('viewId' in layout['design-main']).toBe(false);
      dispose();
    });
  });

  it('includes all viewports (including dynamically added panes)', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      store.addPane(1);
      store.setSizeWeight('pane-1', 3);

      const layout = collectViewportLayout(store.state.viewports);

      expect(layout['pane-1']).toEqual({ sizeWeight: 3, forceExpanded: false });
      dispose();
    });
  });
});

describe('applyViewportLayout unit tests (task-4768 ε)', () => {
  it('applies sizeWeight, forceExpanded, and splitRatio from layout + splitRatio args', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      applyViewportLayout(
        store,
        { 'design-main': { sizeWeight: 3, forceExpanded: true } },
        0.2,
      );
      expect(store.state.viewports['design-main'].sizeWeight).toBe(3);
      expect(store.state.viewports['design-main'].forceExpanded).toBe(true);
      expect(store.state.splitRatio).toBeCloseTo(0.2);
      dispose();
    });
  });

  it('layout=undefined leaves store defaults unchanged (no throw)', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      const defaultSplitRatio = store.state.splitRatio;
      expect(() => applyViewportLayout(store, undefined, undefined)).not.toThrow();
      expect(store.state.splitRatio).toBe(defaultSplitRatio);
      dispose();
    });
  });

  it('unknown viewport id in layout is silently ignored (no throw, no new viewport)', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      const beforeKeys = Object.keys(store.state.viewports);
      expect(() =>
        applyViewportLayout(store, { 'does-not-exist': { sizeWeight: 5, forceExpanded: true } }, undefined),
      ).not.toThrow();
      expect(Object.keys(store.state.viewports)).toEqual(beforeKeys);
      dispose();
    });
  });

  it('non-finite splitRatio is ignored (state.splitRatio stays at prior value)', () => {
    return createRoot(dispose => {
      const store = createViewportStore();
      store.setSplitRatio(0.6);
      applyViewportLayout(store, undefined, NaN);
      expect(store.state.splitRatio).toBeCloseTo(0.6);
      dispose();
    });
  });
});

// ── computeAppearanceOverrides unit tests (task-4773 δ step-13) ──────────────

function makeStyle(r: number): DisplayStyleData {
  return { color: [r, 0, 0, 1], finish: 1, opacity: 1, wireframe: false };
}

describe('computeAppearanceOverrides unit tests (task-4773 δ)', () => {
  it('case 1: empty input → overrides {} / dropped []', () => {
    const meshes = { 'a': makeMesh('a'), 'b': makeMesh('b') };
    const result = computeAppearanceOverrides([], meshes);
    expect(result.overrides).toEqual({});
    expect(result.dropped).toEqual([]);
  });

  it('case 2: directive whose subject is a key in meshes → overrides[subject]===style', () => {
    const style = makeStyle(0.5);
    const meshes = { 'S#realization[0]': makeMesh('S#realization[0]') };
    const directive: AppearanceDirective = { subject: 'S#realization[0]', style };
    const result = computeAppearanceOverrides([directive], meshes);
    expect(result.overrides['S#realization[0]']).toEqual(style);
    expect(result.dropped).toEqual([]);
  });

  it('case 3: duplicate subjects → last-wins', () => {
    const styleA = makeStyle(0.1);
    const styleB = makeStyle(0.9);
    const meshes = { 'x': makeMesh('x') };
    const directives: AppearanceDirective[] = [
      { subject: 'x', style: styleA },
      { subject: 'x', style: styleB },
    ];
    const result = computeAppearanceOverrides(directives, meshes);
    expect(result.overrides['x']).toEqual(styleB);
    expect(result.dropped).toEqual([]);
  });

  it('case 4: directive whose subject has no realized mesh → excluded from overrides, present in dropped', () => {
    const style = makeStyle(0.3);
    const meshes = { 'real': makeMesh('real') };
    const ghost: AppearanceDirective = { subject: 'ghost', style };
    const real: AppearanceDirective = { subject: 'real', style: makeStyle(0.7) };
    const result = computeAppearanceOverrides([ghost, real], meshes);
    expect(Object.keys(result.overrides)).toEqual(['real']);
    expect(result.dropped).toEqual([ghost]);
  });

  it('case 5: empty meshes — all directives dropped', () => {
    const directives: AppearanceDirective[] = [
      { subject: 'a', style: makeStyle(0.2) },
    ];
    const result = computeAppearanceOverrides(directives, {});
    expect(result.overrides).toEqual({});
    expect(result.dropped).toEqual(directives);
  });
});

// ── display-appearance dropped-directive warn/dedup wiring (task-4773 δ amend) ─

describe('display-appearance dropped-directive warn/dedup (task-4773 δ amend)', () => {
  it('warns for a dangling display_appearance directive on initial render', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('real')],
      display_appearance: [{ subject: 'ghost-entity', style: makeStyle(0.3) }],
    })));
    try {
      await renderAndWaitForReady();
      const msgs = warnSpy.mock.calls.map(args => String(args.join(' ')));
      expect(msgs.some(m => m.includes('[display-appearance]') && m.includes('ghost-entity'))).toBe(true);
    } finally {
      warnSpy.mockRestore();
    }
  });

  it('does not re-warn for the same dropped-subject set when meshes update (dedup)', async () => {
    // Capture the onMeshUpdate callback so we can push an update after initial render.
    let meshUpdateCallback: ((mesh: MeshData) => void) | undefined;
    vi.mocked(bridge.onMeshUpdate).mockImplementation(async (cb: any) => {
      meshUpdateCallback = cb;
      return () => {};
    });

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('real')],
      display_appearance: [{ subject: 'ghost-entity', style: makeStyle(0.3) }],
    })));
    try {
      await renderAndWaitForReady();

      // Confirm the initial warn fired exactly once for the dropped subject.
      const ghostWarnsBefore = warnSpy.mock.calls.filter(
        args => String(args.join(' ')).includes('ghost-entity'),
      ).length;
      expect(ghostWarnsBefore).toBe(1);

      // Push a mesh update for an unrelated entity.  This mutates
      // engineStore.state.meshes, causing the appearanceData memo to recompute.
      // 'ghost-entity' is still dangling, so the dropped set is unchanged →
      // the dedup guard (lastDroppedKeys) must suppress a second warn.
      meshUpdateCallback!(makeMesh('other-entity'));
      await flushMacrotasks();

      const ghostWarnsAfter = warnSpy.mock.calls.filter(
        args => String(args.join(' ')).includes('ghost-entity'),
      ).length;
      expect(ghostWarnsAfter).toBe(1); // No second warn — dedup held
    } finally {
      warnSpy.mockRestore();
    }
  });
});
