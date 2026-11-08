// The '../bridge' mock module every engineStore harness consumer installs with
// the vi.mock line spelled out in INSTALL_SHARED_BRIDGE_MOCK (engineStoreBridgeHandles.ts);
// it runs inside that mock's own resolution, so it must never runtime-import '../bridge'.
import { vi } from 'vitest';
import type * as Bridge from '../bridge';

const subscription = () => vi.fn(() => Promise.resolve(() => {}));

export function engineStoreBridgeMock() {
  return {
    onMeshUpdate: subscription(),
    onValueUpdate: subscription(),
    onConstraintUpdate: subscription(),
    onEvaluationStatus: subscription(),
    onMeshRemoved: subscription(),
    onValueRemoved: subscription(),
    onConstraintRemoved: subscription(),
    onTessellationDiagnostics: subscription(),
    onCompileDiagnostics: subscription(),
    onKernelStatus: subscription(),
    onAutoResolveStart: subscription(),
    onAutoResolveIteration: subscription(),
    onAutoResolveComplete: subscription(),
    onSolverProgress: subscription(),
    cancelSolve: vi.fn(() => Promise.resolve()),
    onFeaDiagnosticsChanged: subscription(),
    onFeaConvergenceChanged: subscription(),
    onTensegrityWiresUpdate: subscription(),
    onTensegritySurfacesUpdate: subscription(),
    onDisplayPanesUpdate: subscription(),
    onDisplayAppearanceUpdate: subscription(),
    onEvalGeneration: subscription(),
    syncObservedDemand: vi.fn(() => Promise.resolve()),
    syncDemand: vi.fn(() => Promise.resolve()),
  } satisfies Partial<Record<keyof typeof Bridge, unknown>>;
}
