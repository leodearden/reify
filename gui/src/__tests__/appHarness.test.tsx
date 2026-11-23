import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen } from '@solidjs/testing-library';
import { readFileSync } from 'node:fs';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStubModule());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStubModule());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStubModule());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import * as bridge from '../bridge';
import { Viewport, DualViewport, MultiViewport } from '../viewport';
import { Editor } from '../editor/Editor';
import { FileTabs } from '../editor/FileTabs';
import { APP_BRIDGE_DEFAULTS } from './appHarness/moduleMocks';
import { APP_HARNESS_INSTALL_LINES } from './appHarness/installLines';
import './appHarness/harness';
import {
  PRIMED_MODULES,
  assertEveryInstallBound,
  assertMockedModule,
  primeAppHarness,
  renderAndWaitForReady,
} from './appHarness/harness';
import * as stubs from './appHarness/renderStubs';

const SENTINEL = Symbol('persistent override');
const ONCE = Symbol('queued once');

const mockedCases = PRIMED_MODULES.flatMap(({ specifier, namespace, defaults }) =>
  Object.keys(defaults).map((name) => ({ specifier, name, namespace, defaults })),
);

describe('primeAppHarness resets every harness-mocked export to its table default', () => {
  it('covers a non-empty set of exports, the bridge table included', () => {
    expect(Object.keys(APP_BRIDGE_DEFAULTS).length).toBeGreaterThanOrEqual(55);
    expect(PRIMED_MODULES.map(({ specifier }) => specifier)).toContain('../bridge');
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

describe('importing the harness primes every test of the importing file', () => {
  const PLANTED_KEY = 'appHarness.planted';

  afterEach(() => {
    localStorage.setItem(PLANTED_KEY, 'leaked');
    vi.mocked(bridge.getEntityTree)
      .mockImplementation(() => SENTINEL as never)
      .mockReturnValueOnce(ONCE as never);
  });

  // The two runs are adjacent in any order, so the second always starts after the first's plant.
  it.each(['one', 'two'])('run %s starts from the table defaults, with no beforeEach in this file', () => {
    expect(localStorage.getItem(PLANTED_KEY)).toBeNull();
    expect(vi.mocked(bridge.getEntityTree).getMockImplementation()).toBe(APP_BRIDGE_DEFAULTS.getEntityTree);
    expect(bridge.getEntityTree()).not.toBe(ONCE);
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

describe('assertEveryInstallBound', () => {
  it('names an install specifier that no harness table binds', () => {
    const check = () => assertEveryInstallBound(['../bridge', '../newlyMocked'], ['../bridge']);
    expect(check).toThrow(/'\.\.\/newlyMocked'/);
    expect(check).toThrow(/APP_HARNESS_INSTALL_LINES/);
  });

  it('accepts install specifiers that are all bound', () => {
    expect(() => assertEveryInstallBound(['../bridge'], ['../bridge', '../viewport'])).not.toThrow();
  });

  it('quotes, as each install line, the exact line this file is installed under', () => {
    const ownSource = readFileSync(__filename, 'utf-8');
    const notWritten = Object.values(APP_HARNESS_INSTALL_LINES).filter((line) => !ownSource.includes(line));
    expect(notWritten).toStrictEqual([]);
  });
});
