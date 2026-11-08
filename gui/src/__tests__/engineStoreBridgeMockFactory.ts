// The '../bridge' mock module every engineStore harness consumer installs with
// the vi.mock line spelled out in INSTALL_SHARED_BRIDGE_MOCK (engineStoreBridgeHandles.ts);
// it runs inside that mock's own resolution, so it must never runtime-import '../bridge'.
import { vi } from 'vitest';
import type * as Bridge from '../bridge';

export function engineStoreBridgeMock() {
  return {
    onMeshUpdate: vi.fn(),
    onValueUpdate: vi.fn(),
    onConstraintUpdate: vi.fn(),
    onEvaluationStatus: vi.fn(),
    onMeshRemoved: vi.fn(),
    onValueRemoved: vi.fn(),
    onConstraintRemoved: vi.fn(),
    onTessellationDiagnostics: vi.fn(),
    onCompileDiagnostics: vi.fn(),
    onKernelStatus: vi.fn(),
    onAutoResolveStart: vi.fn(),
    onAutoResolveIteration: vi.fn(),
    onAutoResolveComplete: vi.fn(),
    onSolverProgress: vi.fn(() => Promise.resolve(() => {})),
    cancelSolve: vi.fn(() => Promise.resolve()),
    onFeaDiagnosticsChanged: vi.fn(() => Promise.resolve(() => {})),
    onFeaConvergenceChanged: vi.fn(() => Promise.resolve(() => {})),
    onTensegrityWiresUpdate: vi.fn(() => Promise.resolve(() => {})),
    onTensegritySurfacesUpdate: vi.fn(() => Promise.resolve(() => {})),
    onDisplayPanesUpdate: vi.fn(() => Promise.resolve(() => {})),
    onDisplayAppearanceUpdate: vi.fn(() => Promise.resolve(() => {})),
    onEvalGeneration: vi.fn(() => Promise.resolve(() => {})),
    syncObservedDemand: vi.fn(() => Promise.resolve()),
    syncDemand: vi.fn(() => Promise.resolve()),
  } satisfies Partial<Record<keyof typeof Bridge, unknown>>;
}
