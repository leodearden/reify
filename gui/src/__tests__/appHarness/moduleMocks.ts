// The default implementations of every module the App harness mocks, and the
// vi.mock factories built from them. Evaluated inside those mocks' own
// resolution, so it never runtime-imports a module it mocks. The rules these
// tables serve live in bridgeMockCoverage.test.ts.
import { vi, type Mock } from 'vitest';
import type * as TauriCore from '@tauri-apps/api/core';
import type * as TauriEvent from '@tauri-apps/api/event';
import type * as Bridge from '../../bridge';
import type * as Debug from '../../debug';
import type * as SidecarPersistence from '../../stores/sidecarPersistence';
import type * as ViewPersistence from '../../stores/viewPersistence';
import { makeGuiState } from '../test_utils/guiState';
import { published } from '../test_utils/publishedState';

export type MockImplementation = (...args: any[]) => any;

type DefaultsFor<M> = { [K in keyof M]?: M[K] };
type LooseDefaultsFor<M> = { [K in keyof M]?: MockImplementation };

const unlisten = () => Promise.resolve(() => {});

export const APP_BRIDGE_DEFAULTS = {
  getInitialState: () => Promise.resolve(published(makeGuiState())),
  getEntityTree: () => Promise.resolve([]),
  setParameter: () => Promise.resolve(),
  previewParameter: () => Promise.resolve(),
  exportGeometry: () => Promise.resolve(),
  pickSavePath: () => Promise.resolve('/user/chosen/path.step'),
  pickOpenPath: () => Promise.resolve(null),
  updateSource: () => Promise.resolve(),
  saveFile: () => Promise.resolve(),
  openFile: () => Promise.resolve({ path: '', content: '' }),
  createNewFile: () => Promise.resolve({ path: '/new/untitled.ri', content: '' }),
  listRecent: () => Promise.resolve([]),
  recordRecent: () => Promise.resolve([]),
  takeInitialFileLoadFailure: () => Promise.resolve(null),
  openFileEngine: () => Promise.resolve(published(makeGuiState())),
  getSourceLocation: () => Promise.resolve({ file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 }),
  focusEntity: () => Promise.resolve(),
  onMeshUpdate: unlisten,
  onValueUpdate: unlisten,
  onConstraintUpdate: unlisten,
  onEvaluationStatus: unlisten,
  onEvalGeneration: unlisten,
  onMeshRemoved: unlisten,
  onValueRemoved: unlisten,
  onConstraintRemoved: unlisten,
  onTessellationDiagnostics: unlisten,
  onCompileDiagnostics: unlisten,
  onFeaDiagnosticsChanged: unlisten,
  onFeaConvergenceChanged: unlisten,
  setActiveFeaCase: () => Promise.resolve(),
  subscribeFeaCaseToStore: unlisten,
  onTensegrityWiresUpdate: unlisten,
  onTensegritySurfacesUpdate: unlisten,
  onDisplayPanesUpdate: unlisten,
  onDisplayAppearanceUpdate: unlisten,
  onFileChanged: unlisten,
  onFileRemoved: unlisten,
  onSerializationError: unlisten,
  onFocusEntity: unlisten,
  onNavigateToSource: unlisten,
  claudeSendMessage: () => Promise.resolve(),
  claudeAbort: () => Promise.resolve(),
  claudeClearSession: () => Promise.resolve(),
  claudePermissionDecision: () => Promise.resolve(),
  subscribeToClaudeEvents: unlisten,
  isDebugEnabled: () => Promise.resolve(false),
  setMode: () => Promise.resolve({ mode: 'design', debug_port: null }),
  onWarmPoolEvent: unlisten,
  getKernelStatus: () => Promise.resolve({ available: true, message: null }),
  onKernelStatus: unlisten,
  getContainingDefinition: () => Promise.resolve(null),
  getEntityAtSourceLocation: () => Promise.resolve(null),
  getDefPreview: () => Promise.resolve(makeGuiState()),
  readViewSidecar: () => Promise.resolve(null),
  writeViewSidecar: () => Promise.resolve(),
  getMechanismDescriptors: () => Promise.resolve([]),
  getUnitLadders: () => Promise.resolve([]),
  subscribeToSidecarCrashed: unlisten,
  onAutoResolveStart: unlisten,
  onAutoResolveIteration: unlisten,
  onAutoResolveComplete: unlisten,
  onSolverProgress: unlisten,
  onModeShapeFrame: unlisten,
  cancelSolve: () => Promise.resolve(),
  syncObservedDemand: () => Promise.resolve(),
  syncDemand: () => Promise.resolve(),
  ask: () => Promise.resolve(false),
} satisfies DefaultsFor<typeof Bridge>;

export const SIDECAR_PERSISTENCE_DEFAULTS = {
  loadSidecar: () => Promise.resolve(null),
  saveSidecar: () => Promise.resolve(),
} satisfies DefaultsFor<typeof SidecarPersistence>;

export const VIEW_PERSISTENCE_DEFAULTS = {
  loadViewPersistence: () => null,
} satisfies DefaultsFor<typeof ViewPersistence>;

export const DEBUG_DEFAULTS = {
  initDebugBridge: unlisten,
} satisfies DefaultsFor<typeof Debug>;

export const TAURI_CORE_DEFAULTS = {
  invoke: () => Promise.resolve({ meshes: [], values: [], constraints: [], files: [] }),
} satisfies LooseDefaultsFor<typeof TauriCore>;

export const TAURI_EVENT_DEFAULTS = {
  listen: unlisten,
} satisfies LooseDefaultsFor<typeof TauriEvent>;

export function mocksFrom<D extends Record<string, MockImplementation>>(defaults: D): { [K in keyof D]: Mock<D[K]> } {
  return Object.fromEntries(
    Object.entries(defaults).map(([name, impl]) => [name, vi.fn(impl)]),
  ) as { [K in keyof D]: Mock<D[K]> };
}

export const appBridgeMock = () => mocksFrom(APP_BRIDGE_DEFAULTS);
export const sidecarPersistenceMock = () => mocksFrom(SIDECAR_PERSISTENCE_DEFAULTS);
export const debugMock = () => mocksFrom(DEBUG_DEFAULTS);
export const tauriCoreMock = () => mocksFrom(TAURI_CORE_DEFAULTS);
export const tauriEventMock = () => mocksFrom(TAURI_EVENT_DEFAULTS);
export const viewPersistenceMock = (actual: typeof ViewPersistence) => ({
  ...actual,
  ...mocksFrom(VIEW_PERSISTENCE_DEFAULTS),
});
