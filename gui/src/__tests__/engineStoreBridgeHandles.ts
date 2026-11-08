// Typed vi.mocked handles onto the shared engineStore bridge mock, plus the
// per-test default priming. Importable only from a test file that installs
// engineStoreBridgeMock() (see engineStoreBridgeMockFactory.ts).
import { vi } from 'vitest';
import {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
  onMeshRemoved,
  onValueRemoved,
  onConstraintRemoved,
  onTessellationDiagnostics,
  onCompileDiagnostics,
  onAutoResolveStart,
  onAutoResolveIteration,
  onAutoResolveComplete,
  onSolverProgress,
  cancelSolve,
  onFeaDiagnosticsChanged,
  onFeaConvergenceChanged,
  onTensegrityWiresUpdate,
  onTensegritySurfacesUpdate,
  onDisplayPanesUpdate,
  onDisplayAppearanceUpdate,
  onEvalGeneration,
} from '../bridge';

const INSTALL_SHARED_BRIDGE_MOCK =
  "vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());";

const subscriptionExports = {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
  onMeshRemoved,
  onValueRemoved,
  onConstraintRemoved,
  onTessellationDiagnostics,
  onCompileDiagnostics,
  onFeaDiagnosticsChanged,
  onFeaConvergenceChanged,
  onTensegrityWiresUpdate,
  onTensegritySurfacesUpdate,
  onDisplayPanesUpdate,
  onDisplayAppearanceUpdate,
  onAutoResolveStart,
  onAutoResolveIteration,
  onAutoResolveComplete,
  onSolverProgress,
  onEvalGeneration,
};

for (const [name, exported] of Object.entries({ ...subscriptionExports, cancelSolve })) {
  if (!vi.isMockFunction(exported)) {
    throw new Error(
      `engineStoreBridgeHandles: '../bridge' export '${name}' is not a vi mock, ` +
        'so the importing test file is bound to the real bridge. Install the ' +
        `shared bridge mock in that file:\n  ${INSTALL_SHARED_BRIDGE_MOCK}`,
    );
  }
}

export const mockOnMeshUpdate = vi.mocked(onMeshUpdate);
export const mockOnValueUpdate = vi.mocked(onValueUpdate);
export const mockOnConstraintUpdate = vi.mocked(onConstraintUpdate);
export const mockOnEvaluationStatus = vi.mocked(onEvaluationStatus);
export const mockOnMeshRemoved = vi.mocked(onMeshRemoved);
export const mockOnValueRemoved = vi.mocked(onValueRemoved);
export const mockOnConstraintRemoved = vi.mocked(onConstraintRemoved);
export const mockOnTessellationDiagnostics = vi.mocked(onTessellationDiagnostics);
export const mockOnCompileDiagnostics = vi.mocked(onCompileDiagnostics);
export const mockOnFeaDiagnosticsChanged = vi.mocked(onFeaDiagnosticsChanged);
export const mockOnFeaConvergenceChanged = vi.mocked(onFeaConvergenceChanged);
export const mockOnAutoResolveStart = vi.mocked(onAutoResolveStart);
export const mockOnAutoResolveIteration = vi.mocked(onAutoResolveIteration);
export const mockOnAutoResolveComplete = vi.mocked(onAutoResolveComplete);
export const mockOnSolverProgress = vi.mocked(onSolverProgress);
export const mockCancelSolve = vi.mocked(cancelSolve);
export const mockOnTensegrityWiresUpdate = vi.mocked(onTensegrityWiresUpdate);
export const mockOnTensegritySurfacesUpdate = vi.mocked(onTensegritySurfacesUpdate);
export const mockOnDisplayPanesUpdate = vi.mocked(onDisplayPanesUpdate);
export const mockOnDisplayAppearanceUpdate = vi.mocked(onDisplayAppearanceUpdate);
export const mockOnEvalGeneration = vi.mocked(onEvalGeneration);

const subscriptionHandles = Object.values(subscriptionExports).map((subscribe) => vi.mocked(subscribe));

/**
 * Clears every mock's call history, then resets every channel subscribeToEvents
 * subscribes to (and cancelSolve) to a fresh default: a no-op unlisten
 * (cancelSolve: undefined). The reset drops queued *Once stubs as well as
 * persistent ones, so no test inherits an earlier test's stub.
 */
export function primeEngineStoreBridgeMocks(): void {
  vi.clearAllMocks();
  for (const subscribe of subscriptionHandles) subscribe.mockReset().mockResolvedValue(vi.fn());
  mockCancelSolve.mockReset().mockResolvedValue(undefined);
}
