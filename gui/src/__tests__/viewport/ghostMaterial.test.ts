import { describe, it, expect, vi, beforeEach } from 'vitest';

// vi.hoisted runs before vi.mock factories (which are hoisted above module-level
// const/let declarations). This ensures mockBasicMaterials is initialized when
// the async mock factory calls makeMockMeshBasicMaterial(mockBasicMaterials).
const mockBasicMaterials = vi.hoisted<any[]>(() => []);

vi.mock('three', async () => {
  const { makeMockMeshBasicMaterial } = await import('./mocks/threeMocks');
  return {
    MeshBasicMaterial: makeMockMeshBasicMaterial(mockBasicMaterials),
    FrontSide: 0,
  };
});

import { createGhostMaterial } from '../../viewport/ghostMaterial';
import { MeshBasicMaterial } from 'three';

beforeEach(() => {
  vi.clearAllMocks();
  mockBasicMaterials.length = 0;
});

describe('createGhostMaterial', () => {
  it('returns an instance of MeshBasicMaterial', () => {
    const mat = createGhostMaterial();
    expect(mat).toBeInstanceOf(MeshBasicMaterial);
  });

  it('uses THEME_TOKENS.surface0 (#313244) as the color', () => {
    createGhostMaterial();
    expect(mockBasicMaterials).toHaveLength(1);
    expect(mockBasicMaterials[0].color).toBe('#313244');
  });

  it('sets transparent: true', () => {
    const mat = createGhostMaterial();
    expect((mat as any).transparent).toBe(true);
  });

  it('sets opacity in the range [0.12, 0.18]', () => {
    const mat = createGhostMaterial();
    expect((mat as any).opacity).toBeGreaterThanOrEqual(0.12);
    expect((mat as any).opacity).toBeLessThanOrEqual(0.18);
  });

  it('sets depthWrite: false', () => {
    const mat = createGhostMaterial();
    expect((mat as any).depthWrite).toBe(false);
  });

  it('sets side: FrontSide (0)', () => {
    const mat = createGhostMaterial();
    expect((mat as any).side).toBe(0); // FrontSide = 0
  });

  it('sets polygonOffset: true', () => {
    const mat = createGhostMaterial();
    expect((mat as any).polygonOffset).toBe(true);
  });

  it('sets polygonOffsetFactor: 1', () => {
    const mat = createGhostMaterial();
    expect((mat as any).polygonOffsetFactor).toBe(1);
  });

  it('sets polygonOffsetUnits: 1', () => {
    const mat = createGhostMaterial();
    expect((mat as any).polygonOffsetUnits).toBe(1);
  });

  it('each call returns a new material instance', () => {
    const mat1 = createGhostMaterial();
    const mat2 = createGhostMaterial();
    expect(mat1).not.toBe(mat2);
    expect(mockBasicMaterials).toHaveLength(2);
  });
});
