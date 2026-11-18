// Binds a test file that renders <App /> to the shared App harness mocks: the
// import-time check that they are installed, the per-test priming, and the
// render helpers. Import it only from a file that writes the nine
// APP_HARNESS_INSTALL_LINES; the rules live in bridgeMockCoverage.test.ts.
import { vi, expect } from 'vitest';
import { render, screen, waitFor } from '@solidjs/testing-library';
import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';
import * as bridge from '../../bridge';
import * as debug from '../../debug';
import * as sidecarPersistence from '../../stores/sidecarPersistence';
import * as viewPersistence from '../../stores/viewPersistence';
import * as viewport from '../../viewport';
import * as editor from '../../editor/Editor';
import * as fileTabs from '../../editor/FileTabs';
import App from '../../App';
import type { GuiState } from '../../types';
import { makeGuiState } from '../test_utils/guiState';
import {
  APP_BRIDGE_DEFAULTS,
  DEBUG_DEFAULTS,
  SIDECAR_PERSISTENCE_DEFAULTS,
  TAURI_CORE_DEFAULTS,
  TAURI_EVENT_DEFAULTS,
  VIEW_PERSISTENCE_DEFAULTS,
  type MockImplementation,
} from './moduleMocks';
import { editorStubModule, fileTabsStubModule, resetRenderStubCaptures, viewportStubModule } from './renderStubs';
import { APP_HARNESS_INSTALL_LINES, type HarnessSpecifier } from './installLines';

export interface PrimedModule {
  specifier: string;
  namespace: object;
  defaults: Readonly<Record<string, MockImplementation>>;
  installLine: string;
  /** The module's exports must be exactly the table's keys, so no other factory can stand in for it. */
  exactKeys: boolean;
}

export const PRIMED_MODULES: readonly PrimedModule[] = [
  primed('../bridge', bridge, APP_BRIDGE_DEFAULTS, true),
  primed('../stores/sidecarPersistence', sidecarPersistence, SIDECAR_PERSISTENCE_DEFAULTS, false),
  primed('../stores/viewPersistence', viewPersistence, VIEW_PERSISTENCE_DEFAULTS, false),
  primed('../debug', debug, DEBUG_DEFAULTS, false),
  primed('@tauri-apps/api/core', tauriCore, TAURI_CORE_DEFAULTS, false),
  primed('@tauri-apps/api/event', tauriEvent, TAURI_EVENT_DEFAULTS, false),
];

function primed(
  specifier: HarnessSpecifier,
  namespace: object,
  defaults: PrimedModule['defaults'],
  exactKeys: boolean,
): PrimedModule {
  return { specifier, namespace, defaults, installLine: APP_HARNESS_INSTALL_LINES[specifier], exactKeys };
}

const RENDER_STUB_MODULES: ReadonlyArray<{ specifier: HarnessSpecifier; namespace: object; stubs: object }> = [
  { specifier: '../viewport', namespace: viewport, stubs: viewportStubModule() },
  { specifier: '../editor/Editor', namespace: editor, stubs: editorStubModule() },
  { specifier: '../editor/FileTabs', namespace: fileTabs, stubs: fileTabsStubModule() },
];

function harnessInvariantError(subject: string, invariant: string, offending: string[], remedy: string): Error {
  return new Error(
    `appHarness: '${subject}' breaks the harness invariant "${invariant}" for ` +
      `${offending.map((name) => `'${name}'`).join(', ')}. ${remedy}`,
  );
}

const installRemedy = (installLine: string) =>
  `The importing test file must install the shared mock for it, and no other factory:\n  ${installLine}`;

/** Every install line must name a module this file checks at import and primes before each test. */
export function assertEveryInstallBound(installSpecifiers: readonly string[], boundSpecifiers: readonly string[]): void {
  const unbound = installSpecifiers.filter((specifier) => !boundSpecifiers.includes(specifier));
  if (unbound.length > 0) {
    throw harnessInvariantError(
      'APP_HARNESS_INSTALL_LINES',
      'each install line has a PRIMED_MODULES or RENDER_STUB_MODULES entry in harness.tsx',
      unbound,
      'Add an entry for each, so its mock is checked at import and reset before every test.',
    );
  }
}

export function assertMockedModule(entry: PrimedModule): void {
  const exports = entry.namespace as Record<string, unknown>;
  const notMocked = Object.keys(entry.defaults).filter((name) => !vi.isMockFunction(exports[name]));
  if (notMocked.length > 0) {
    throw harnessInvariantError(entry.specifier, 'every table default is a vi mock export', notMocked, installRemedy(entry.installLine));
  }
  if (entry.exactKeys) {
    const untabled = Object.keys(exports).filter((name) => !Object.hasOwn(entry.defaults, name));
    if (untabled.length > 0) {
      throw harnessInvariantError(entry.specifier, 'its exports are exactly its table keys', untabled, installRemedy(entry.installLine));
    }
  }
}

assertEveryInstallBound(
  Object.keys(APP_HARNESS_INSTALL_LINES),
  [...PRIMED_MODULES, ...RENDER_STUB_MODULES].map(({ specifier }) => specifier),
);

for (const entry of PRIMED_MODULES) assertMockedModule(entry);

for (const { specifier, namespace, stubs } of RENDER_STUB_MODULES) {
  const exports = namespace as Record<string, unknown>;
  const notStubbed = Object.entries(stubs)
    .filter(([name, stub]) => exports[name] !== stub)
    .map(([name]) => name);
  if (notStubbed.length > 0) {
    throw harnessInvariantError(
      specifier,
      'each export is its renderStubs component',
      notStubbed,
      installRemedy(APP_HARNESS_INSTALL_LINES[specifier]),
    );
  }
}

/**
 * Per-test reset, run as `beforeEach(primeAppHarness)`: clears call history,
 * localStorage, fake timers and the render-stub captures, then resets every
 * export of every harness-mocked module to its table default. The reset drops
 * persistent and queued *Once overrides alike.
 */
export function primeAppHarness(): void {
  vi.clearAllMocks();
  localStorage.clear();
  vi.useRealTimers();
  resetRenderStubCaptures();
  for (const entry of PRIMED_MODULES) {
    const exports = entry.namespace as Record<string, unknown>;
    for (const [name, impl] of Object.entries(entry.defaults)) {
      const exported = exports[name];
      if (!vi.isMockFunction(exported)) {
        throw harnessInvariantError(
          entry.specifier,
          'every table default is a vi mock export',
          [name],
          installRemedy(entry.installLine),
        );
      }
      exported.mockReset().mockImplementation(impl);
    }
  }
}

/** Helper: render App and wait for init to complete (ready state). */
export async function renderAndWaitForReady() {
  const result = render(() => <App />);
  await waitFor(() => {
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });
  return result;
}

export const oneFileState = (): GuiState => makeGuiState({ files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }] });
