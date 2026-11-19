// The nine vi.mock lines a test file in gui/src/__tests__/ writes to install the
// App harness, keyed by mocked specifier. Each line is built from its factory
// VALUE, so renaming a factory is a type error here rather than a stale line in
// every error message that quotes these. The rules they serve live in
// bridgeMockCoverage.test.ts.
import * as moduleMocks from './moduleMocks';
import * as renderStubs from './renderStubs';

type Factory = (...args: never[]) => object;

const FACTORY_HOMES = [
  { importPath: './appHarness/moduleMocks', namespace: moduleMocks },
  { importPath: './appHarness/renderStubs', namespace: renderStubs },
] as const;

function importPathOf(factory: Factory): string {
  const home = FACTORY_HOMES.find(({ namespace }) => (namespace as Record<string, unknown>)[factory.name] === factory);
  if (home === undefined) {
    throw new Error(
      `appHarness: install factory '${factory.name}' is not exported under that name by ` +
        FACTORY_HOMES.map(({ importPath }) => importPath).join(' or '),
    );
  }
  return home.importPath;
}

const install = (specifier: string, factory: () => object) =>
  `vi.mock('${specifier}', async () => (await import('${importPathOf(factory)}')).${factory.name}());`;

const installOverOriginal = <Actual>(specifier: string, factory: (actual: Actual) => object) =>
  `vi.mock('${specifier}', async (importOriginal) => ` +
  `(await import('${importPathOf(factory)}')).${factory.name}(await importOriginal()));`;

export const APP_HARNESS_INSTALL_LINES = {
  '@tauri-apps/api/core': install('@tauri-apps/api/core', moduleMocks.tauriCoreMock),
  '@tauri-apps/api/event': install('@tauri-apps/api/event', moduleMocks.tauriEventMock),
  '../viewport': install('../viewport', renderStubs.viewportStubModule),
  '../editor/Editor': install('../editor/Editor', renderStubs.editorStubModule),
  '../editor/FileTabs': install('../editor/FileTabs', renderStubs.fileTabsStubModule),
  '../bridge': install('../bridge', moduleMocks.appBridgeMock),
  '../debug': install('../debug', moduleMocks.debugMock),
  '../stores/sidecarPersistence': install('../stores/sidecarPersistence', moduleMocks.sidecarPersistenceMock),
  '../stores/viewPersistence': installOverOriginal('../stores/viewPersistence', moduleMocks.viewPersistenceMock),
} as const;

export type HarnessSpecifier = keyof typeof APP_HARNESS_INSTALL_LINES;
