/**
 * Tests for the PURE helper functions in feaDiagnosticOverlay.ts (#2966, step-5/6).
 *
 * All helpers are tested with plain numbers — no WebGL or three.js types needed —
 * following the bucklingAnimator.computePointCloudBounds pattern (WebGL-free unit testing).
 *
 * Step-5 is RED: the module feaDiagnosticOverlay.ts does not yet exist.
 */

import { describe, it, expect } from 'vitest';
import type { MeshData } from '../../types';

// The module under test does not exist yet — step-6 creates it.
// @ts-expect-error — module absent until step-6
import { computeMeshesBounds, rigidBodyArrowSpecs } from '../../viewport/feaDiagnosticOverlay';

// ─── MeshData factory ─────────────────────────────────────────────────────────

/** Build a minimal MeshData from a flat XYZ vertex array. */
function makeMesh(vertices: number[]): MeshData {
  return {
    entity_path: 'test.body',
    vertices: new Float32Array(vertices),
    indices: new Uint32Array([0, 1, 2]),
    normals: null,
  };
}

// ─── computeMeshesBounds ──────────────────────────────────────────────────────

describe('computeMeshesBounds', () => {
  it('returns center [0,0,0] and radius 0 for an empty mesh list', () => {
    const result = computeMeshesBounds([]);
    expect(result.center).toEqual([0, 0, 0]);
    expect(result.radius).toBe(0);
  });

  it('returns center and radius for a single mesh', () => {
    // Vertices: two points at (-1,0,0) and (1,0,0) → center [0,0,0], radius 1
    const mesh = makeMesh([-1, 0, 0, 1, 0, 0]);
    const result = computeMeshesBounds([mesh]);
    expect(result.center[0]).toBeCloseTo(0);
    expect(result.center[1]).toBeCloseTo(0);
    expect(result.center[2]).toBeCloseTo(0);
    expect(result.radius).toBeCloseTo(1);
  });

  it('returns center and radius for a mesh with extent in all three axes', () => {
    // Vertices: (0,0,0) and (2,4,6) → center (1,2,3), diagonal = sqrt(4+16+36) ≈ 7.483, radius ≈ 3.742
    const mesh = makeMesh([0, 0, 0, 2, 4, 6]);
    const result = computeMeshesBounds([mesh]);
    expect(result.center[0]).toBeCloseTo(1);
    expect(result.center[1]).toBeCloseTo(2);
    expect(result.center[2]).toBeCloseTo(3);
    expect(result.radius).toBeCloseTo(Math.sqrt(4 + 16 + 36) / 2);
  });

  it('combines bounding boxes of multiple meshes', () => {
    // Mesh A: x ∈ [-2, 0], Mesh B: x ∈ [0, 2] → combined x ∈ [-2, 2], center [0,0,0]
    const meshA = makeMesh([-2, 0, 0, 0, 0, 0]);
    const meshB = makeMesh([0, 0, 0, 2, 0, 0]);
    const result = computeMeshesBounds([meshA, meshB]);
    expect(result.center[0]).toBeCloseTo(0);
    expect(result.radius).toBeCloseTo(2);
  });

  it('returns center [0,0,0] and radius 0 for a single-point mesh', () => {
    const mesh = makeMesh([5, 5, 5]);
    const result = computeMeshesBounds([mesh]);
    expect(result.center).toEqual([5, 5, 5]);
    expect(result.radius).toBeCloseTo(0);
  });
});

// ─── rigidBodyArrowSpecs ──────────────────────────────────────────────────────

describe('rigidBodyArrowSpecs', () => {
  const center: [number, number, number] = [1, 2, 3];
  const radius = 5;

  it('returns an empty array for an empty modes list', () => {
    expect(rigidBodyArrowSpecs([], center, radius)).toEqual([]);
  });

  it('returns one spec per mode', () => {
    const specs = rigidBodyArrowSpecs(
      ['TranslationX', 'TranslationY', 'TranslationZ', 'RotationX', 'RotationY', 'RotationZ'],
      center,
      radius,
    );
    expect(specs).toHaveLength(6);
  });

  it('spec origin equals the provided center', () => {
    const specs = rigidBodyArrowSpecs(['TranslationX'], center, radius);
    expect(specs[0].origin).toEqual(center);
  });

  it('TranslationX → dir [1,0,0], isRotation false', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationX'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(1);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(false);
  });

  it('TranslationY → dir [0,1,0], isRotation false', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationY'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(1);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(false);
  });

  it('TranslationZ → dir [0,0,1], isRotation false', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationZ'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(1);
    expect(spec.isRotation).toBe(false);
  });

  it('RotationX → dir [1,0,0], isRotation true', () => {
    const [spec] = rigidBodyArrowSpecs(['RotationX'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(1);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(true);
  });

  it('RotationY → dir [0,1,0], isRotation true', () => {
    const [spec] = rigidBodyArrowSpecs(['RotationY'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(1);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(true);
  });

  it('RotationZ → dir [0,0,1], isRotation true', () => {
    const [spec] = rigidBodyArrowSpecs(['RotationZ'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(1);
    expect(spec.isRotation).toBe(true);
  });

  it('spec length is scaled from radius (positive and non-zero)', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationX'], center, radius);
    expect(spec.length).toBeGreaterThan(0);
    // Length should scale with radius — larger radius → larger arrow
    const [specSmall] = rigidBodyArrowSpecs(['TranslationX'], center, 1);
    const [specLarge] = rigidBodyArrowSpecs(['TranslationX'], center, 10);
    expect(specLarge.length).toBeGreaterThan(specSmall.length);
  });

  it('translation and rotation specs have distinct colors', () => {
    const specs = rigidBodyArrowSpecs(['TranslationX', 'RotationX'], center, radius);
    const translationColor = specs[0].color;
    const rotationColor = specs[1].color;
    expect(translationColor).not.toBe(rotationColor);
    expect(translationColor).not.toEqual(rotationColor);
  });

  it('all translation specs share the same color', () => {
    const specs = rigidBodyArrowSpecs(['TranslationX', 'TranslationY', 'TranslationZ'], center, radius);
    expect(specs[0].color).toEqual(specs[1].color);
    expect(specs[1].color).toEqual(specs[2].color);
  });

  it('all rotation specs share the same color', () => {
    const specs = rigidBodyArrowSpecs(['RotationX', 'RotationY', 'RotationZ'], center, radius);
    expect(specs[0].color).toEqual(specs[1].color);
    expect(specs[1].color).toEqual(specs[2].color);
  });
});
