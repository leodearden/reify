import { vi } from 'vitest';

/**
 * Factory that returns a MockMeshBasicMaterial class bound to a caller-supplied
 * tracking array. Each constructed instance is pushed to the array so callers
 * can inspect it in assertions and reset it via `array.length = 0` in beforeEach.
 *
 * Usage inside an async vi.mock factory (required because vi.mock is hoisted):
 *
 *   const mockBasicMaterials: any[] = [];
 *   vi.mock('three', async () => {
 *     const { makeMockMeshBasicMaterial } = await import('./mocks/threeMocks');
 *     return { MeshBasicMaterial: makeMockMeshBasicMaterial(mockBasicMaterials), ... };
 *   });
 *
 *   beforeEach(() => { mockBasicMaterials.length = 0; });
 */
export function makeMockMeshBasicMaterial(instances: any[]) {
  return class MockMeshBasicMaterial {
    color: any;
    transparent: boolean;
    opacity: number;
    depthWrite: boolean;
    side: any;
    polygonOffset: boolean;
    polygonOffsetFactor: number;
    polygonOffsetUnits: number;
    dispose = vi.fn();

    constructor(opts?: any) {
      this.color = opts?.color;
      this.transparent = opts?.transparent ?? false;
      this.opacity = opts?.opacity ?? 1;
      this.depthWrite = opts?.depthWrite ?? true;
      this.side = opts?.side;
      this.polygonOffset = opts?.polygonOffset ?? false;
      this.polygonOffsetFactor = opts?.polygonOffsetFactor ?? 0;
      this.polygonOffsetUnits = opts?.polygonOffsetUnits ?? 0;
      instances.push(this);
    }
  };
}
