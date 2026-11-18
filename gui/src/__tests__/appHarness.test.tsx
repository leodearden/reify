import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@solidjs/testing-library';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStub());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStub());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStub());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';
import * as bridge from '../bridge';
import * as debug from '../debug';
import * as sidecarPersistence from '../stores/sidecarPersistence';
import * as viewPersistence from '../stores/viewPersistence';
import { Viewport, DualViewport, MultiViewport } from '../viewport';
import { Editor } from '../editor/Editor';
import { FileTabs } from '../editor/FileTabs';
import {
  APP_BRIDGE_DEFAULTS,
  DEBUG_DEFAULTS,
  SIDECAR_PERSISTENCE_DEFAULTS,
  TAURI_CORE_DEFAULTS,
  TAURI_EVENT_DEFAULTS,
  VIEW_PERSISTENCE_DEFAULTS,
} from './appHarness/moduleMocks';
import { assertMockedModule, primeAppHarness, renderAndWaitForReady } from './appHarness/harness';
import * as stubs from './appHarness/renderStubs';

beforeEach(primeAppHarness);

const SENTINEL = Symbol('persistent override');
const ONCE = Symbol('queued once');

const MOCKED_MODULES: ReadonlyArray<{ specifier: string; namespace: object; defaults: Record<string, unknown> }> = [
  { specifier: '../bridge', namespace: bridge, defaults: APP_BRIDGE_DEFAULTS },
  { specifier: '../stores/sidecarPersistence', namespace: sidecarPersistence, defaults: SIDECAR_PERSISTENCE_DEFAULTS },
  { specifier: '../stores/viewPersistence', namespace: viewPersistence, defaults: VIEW_PERSISTENCE_DEFAULTS },
  { specifier: '../debug', namespace: debug, defaults: DEBUG_DEFAULTS },
  { specifier: '@tauri-apps/api/core', namespace: tauriCore, defaults: TAURI_CORE_DEFAULTS },
  { specifier: '@tauri-apps/api/event', namespace: tauriEvent, defaults: TAURI_EVENT_DEFAULTS },
];

const mockedCases = MOCKED_MODULES.flatMap(({ specifier, namespace, defaults }) =>
  Object.keys(defaults).map((name) => ({ specifier, name, namespace, defaults })),
);

describe('primeAppHarness resets every harness-mocked export to its table default', () => {
  it('covers a non-empty set of exports, the bridge table included', () => {
    expect(Object.keys(APP_BRIDGE_DEFAULTS).length).toBeGreaterThanOrEqual(55);
    expect(mockedCases.length).toBeGreaterThan(Object.keys(APP_BRIDGE_DEFAULTS).length);
  });

  it.each(mockedCases)('$specifier $name', ({ name, namespace, defaults }) => {
    const exported = (namespace as Record<string, () => unknown>)[name];
    if (!vi.isMockFunction(exported)) throw new Error(`${name} is not a vi mock`);
    exported.mockImplementation(() => SENTINEL);
    exported.mockReturnValueOnce(ONCE);

    primeAppHarness();

    expect(exported.getMockImplementation()).toBe(defaults[name]);
    const first = exported();
    expect(first).not.toBe(ONCE);
    expect(first).not.toBe(SENTINEL);
  });
});

describe('primeAppHarness resets the render-stub captures', () => {
  it('records every slot from the stubs, then restores the initial values with fresh ref mocks', () => {
    const flyToEntityRef = vi.fn();
    const fitToViewRef = vi.fn();
    const dualFlyToEntityRef = vi.fn();
    const dualFitToViewRef = vi.fn();
    const viewportProps = { flyToEntityRef, fitToViewRef };
    const dualProps = { flyToEntityRef: dualFlyToEntityRef, fitToViewRef: dualFitToViewRef };
    const multiProps = { panes: [] };
    const editorProps = {
      store: { sentinel: 'store' },
      onOpen: () => {},
      scrollToLocation: () => null,
      compileDiagnostics: { sentinel: 'diagnostics' },
      liveContentRef: () => {},
      onShowReferences: () => {},
    };

    const viewportEl = (Viewport as (props: object) => HTMLElement)(viewportProps);
    const dualEl = (DualViewport as (props: object) => HTMLElement)(dualProps);
    const multiEl = (MultiViewport as (props: object) => HTMLElement)(multiProps);
    const editorEl = (Editor as (props: object) => HTMLElement)(editorProps);
    const fileTabsEl = (FileTabs as (props: object) => HTMLElement)({});

    expect(viewportEl.getAttribute('data-testid')).toBe('viewport-container');
    expect(dualEl.getAttribute('data-testid')).toBe('dual-viewport');
    expect(multiEl.getAttribute('data-testid')).toBe('multi-viewport');
    expect(editorEl.getAttribute('data-testid')).toBe('editor-container');
    expect(fileTabsEl.getAttribute('data-testid')).toBe('file-tabs');

    expect(stubs.capturedViewportProps).toBe(viewportProps);
    expect(stubs.capturedDualViewportProps).toBe(dualProps);
    expect(stubs.capturedMultiViewportProps).toBe(multiProps);
    expect(stubs.capturedEditorStore).toBe(editorProps.store);
    expect(stubs.capturedEditorOnOpen).toBe(editorProps.onOpen);
    expect(stubs.capturedEditorScrollToLocation).toBe(editorProps.scrollToLocation);
    expect(stubs.capturedEditorCompileDiagnostics).toBe(editorProps.compileDiagnostics);
    expect(stubs.capturedEditorLiveContentRef).toBe(editorProps.liveContentRef);
    expect(stubs.capturedEditorOnShowReferences).toBe(editorProps.onShowReferences);
    const flyBefore = stubs.mockFlyToEntity;
    const fitBefore = stubs.mockViewportFitToView;
    expect(flyToEntityRef).toHaveBeenCalledWith(flyBefore);
    expect(fitToViewRef).toHaveBeenCalledWith(fitBefore);
    expect(dualFlyToEntityRef).toHaveBeenCalledWith(flyBefore);
    expect(dualFitToViewRef).toHaveBeenCalledWith(fitBefore);
    flyBefore('Bracket.body');
    fitBefore();

    primeAppHarness();

    expect(stubs.capturedViewportProps).toStrictEqual({});
    expect(stubs.capturedDualViewportProps).toStrictEqual({});
    expect(stubs.capturedMultiViewportProps).toStrictEqual({});
    expect(stubs.capturedEditorStore).toBeNull();
    expect(stubs.capturedEditorOnOpen).toBeUndefined();
    expect(stubs.capturedEditorScrollToLocation).toBeUndefined();
    expect(stubs.capturedEditorCompileDiagnostics).toBeUndefined();
    expect(stubs.capturedEditorLiveContentRef).toBeUndefined();
    expect(stubs.capturedEditorOnShowReferences).toBeUndefined();
    expect(stubs.mockFlyToEntity).not.toBe(flyBefore);
    expect(stubs.mockViewportFitToView).not.toBe(fitBefore);
    expect(vi.isMockFunction(stubs.mockFlyToEntity)).toBe(true);
    expect(vi.isMockFunction(stubs.mockViewportFitToView)).toBe(true);
    expect(stubs.mockFlyToEntity).not.toHaveBeenCalled();
    expect(stubs.mockViewportFitToView).not.toHaveBeenCalled();
  });
});

describe('primeAppHarness resets the per-test globals', () => {
  it('clears localStorage and restores real timers', () => {
    localStorage.setItem('appHarness.probe', 'leaked');
    vi.useFakeTimers();

    primeAppHarness();

    expect(localStorage.length).toBe(0);
    expect(vi.isFakeTimers()).toBe(false);
  });
});

describe('renderAndWaitForReady', () => {
  it('leaves the App layout mounted', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });
});

describe('assertMockedModule', () => {
  const installLine = "vi.mock('synthetic-module', async () => (await import('./synthetic')).syntheticMock());";

  it('names the module, the export and the install line when an export is not a vi mock', () => {
    const descriptor = {
      specifier: 'synthetic-module',
      namespace: { plainExport: () => 1 },
      defaults: { plainExport: () => 1 },
      installLine,
      exactKeys: false,
    };
    expect(() => assertMockedModule(descriptor)).toThrow(/synthetic-module/);
    expect(() => assertMockedModule(descriptor)).toThrow(/plainExport/);
    expect(() => assertMockedModule(descriptor)).toThrow(installLine);
  });

  it('names an extra key when the module must carry exactly the table keys', () => {
    const descriptor = {
      specifier: 'synthetic-module',
      namespace: { tabled: vi.fn(), untabledExtra: vi.fn() },
      defaults: { tabled: () => 1 },
      installLine,
      exactKeys: true,
    };
    expect(() => assertMockedModule(descriptor)).toThrow(/untabledExtra/);
  });

  it('accepts a well-formed descriptor', () => {
    const descriptor = {
      specifier: 'synthetic-module',
      namespace: { tabled: vi.fn() },
      defaults: { tabled: () => 1 },
      installLine,
      exactKeys: true,
    };
    expect(() => assertMockedModule(descriptor)).not.toThrow();
  });
});
