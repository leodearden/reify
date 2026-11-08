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

const handledBridgeExports = {
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
};

for (const [name, exported] of Object.entries(handledBridgeExports)) {
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

const subscriptionHandles = [
  mockOnMeshUpdate,
  mockOnValueUpdate,
  mockOnConstraintUpdate,
  mockOnEvaluationStatus,
  mockOnMeshRemoved,
  mockOnValueRemoved,
  mockOnConstraintRemoved,
  mockOnTessellationDiagnostics,
  mockOnCompileDiagnostics,
  mockOnFeaDiagnosticsChanged,
  mockOnFeaConvergenceChanged,
  mockOnTensegrityWiresUpdate,
  mockOnTensegritySurfacesUpdate,
  mockOnDisplayPanesUpdate,
  mockOnDisplayAppearanceUpdate,
  mockOnAutoResolveStart,
  mockOnAutoResolveIteration,
  mockOnAutoResolveComplete,
  mockOnSolverProgress,
  mockOnEvalGeneration,
] as const;

/**
 * Clears every mock's call history and resolves every channel subscribeToEvents
 * subscribes to with a fresh no-op unlisten (cancelSolve with undefined), so no
 * test inherits an earlier test's stub.
 */
export function primeEngineStoreBridgeMocks(): void {
  vi.clearAllMocks();
  for (const subscribe of subscriptionHandles) subscribe.mockResolvedValue(vi.fn());
  mockCancelSolve.mockResolvedValue(undefined);
}
