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

/**
 * Factory that returns a MockMeshPhongMaterial class bound to a caller-supplied
 * tracking array. Mirrors makeMockMeshBasicMaterial. Captures vertexColors,
 * flatShading, and side so tests can assert on these options.
 *
 * Usage inside an async vi.mock factory:
 *
 *   const mockPhongMaterials: any[] = vi.hoisted(() => []);
 *   vi.mock('three', async () => {
 *     const { makeMockMeshPhongMaterial } = await import('./mocks/threeMocks');
 *     return { MeshPhongMaterial: makeMockMeshPhongMaterial(mockPhongMaterials), ... };
 *   });
 *
 *   beforeEach(() => { mockPhongMaterials.length = 0; });
 */
export function makeMockMeshPhongMaterial(instances: any[]) {
  return class MockMeshPhongMaterial {
    vertexColors: boolean;
    flatShading: boolean;
    side: any;
    dispose = vi.fn();

    constructor(opts?: any) {
      this.vertexColors = opts?.vertexColors ?? false;
      this.flatShading = opts?.flatShading ?? true;
      this.side = opts?.side;
      instances.push(this);
    }
  };
}
