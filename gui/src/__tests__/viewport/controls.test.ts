import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockOrbitControlsDispose = vi.fn();
const mockOrbitControlsUpdate = vi.fn();
let capturedCamera: any;
let capturedDomElement: any;
let capturedInstance: any;

vi.mock('three/addons/controls/OrbitControls.js', () => {
  class MockOrbitControls {
    enableDamping = false;
    dampingFactor = 0;
    minDistance = 0;
    maxDistance = Infinity;
    dispose = mockOrbitControlsDispose;
    update = mockOrbitControlsUpdate;

    constructor(camera: any, domElement: any) {
      capturedCamera = camera;
      capturedDomElement = domElement;
      capturedInstance = this;
    }
  }
  return { OrbitControls: MockOrbitControls };
});

import { createControls } from '../../viewport/controls';
import {
  orbitMinDistanceFor,
  ORBIT_MIN_DISTANCE_FLOOR,
  ORBIT_MAX_DISTANCE,
} from '../../viewport/orbitDistance';

beforeEach(() => {
  vi.clearAllMocks();
  capturedCamera = undefined;
  capturedDomElement = undefined;
  capturedInstance = undefined;
});

describe('createControls', () => {
  function setup() {
    const camera = { type: 'PerspectiveCamera' } as any;
    const domElement = document.createElement('canvas');
    return { result: createControls(camera, domElement), camera, domElement };
  }

  it('returns object with update and dispose methods', () => {
    const { result } = setup();
    expect(typeof result.update).toBe('function');
    expect(typeof result.dispose).toBe('function');
  });

  it('OrbitControls constructor is called with camera and domElement', () => {
    const { camera, domElement } = setup();
    expect(capturedCamera).toBe(camera);
    expect(capturedDomElement).toBe(domElement);
  });

  it('enableDamping is set to true', () => {
    setup();
    expect(capturedInstance.enableDamping).toBe(true);
  });

  it('dispose calls controls.dispose()', () => {
    const { result } = setup();
    result.dispose();
    expect(mockOrbitControlsDispose).toHaveBeenCalled();
  });

  it('update calls controls.update()', () => {
    const { result } = setup();
    result.update();
    expect(mockOrbitControlsUpdate).toHaveBeenCalled();
  });

  // createControls runs in Viewport.tsx's onMount, BEFORE any geometry exists, so it has
  // no bounds to derive a floor from.  It must therefore seed the absolute floor rather
  // than guess an absolute distance: a guess blocks any commanded pose closer than itself
  // (a 0.5 m guess made a fitted 75 mm part unreachable — #6496), whereas the floor blocks
  // nothing and fitCameraToBox tightens it to the model scale as soon as anything is framed.
  it('seeds minDistance from the policy floor, not a magic absolute distance', () => {
    setup();
    expect(capturedInstance.minDistance).toBe(ORBIT_MIN_DISTANCE_FLOOR);
    // Pinned independently of the constant's value so the INTENT — far below any part
    // scale reify models — survives a future retune of ORBIT_MIN_DISTANCE_FLOOR.
    expect(capturedInstance.minDistance).toBeLessThan(0.05);
    expect(capturedInstance.minDistance).toBeGreaterThan(0);
  });

  it('seeds maxDistance from the policy constant', () => {
    setup();
    expect(capturedInstance.maxDistance).toBe(ORBIT_MAX_DISTANCE);
  });
});

// The policy is stated as a RELATION to each model's own fitted distance, never as an
// absolute — that is the whole point of the fix.  fitCameraToBox uses padding 1.1 at a
// 60° vertical FOV, so a fitted camera always sits at 2.2 · radius from its target.
describe('orbitMinDistanceFor', () => {
  const FIT_DISTANCE_PER_RADIUS = 2.2;

  /** The two real scales from the #6496 defect report, four orders of magnitude apart. */
  const SCALES = [
    { label: '75 mm probe', radius: 0.0375 },
    { label: '1 m printer', radius: 0.87 },
  ];

  for (const { label, radius } of SCALES) {
    it(`${label}: the floor is under 1/100 of that model's own fitted distance`, () => {
      const floor = orbitMinDistanceFor(radius);
      expect(floor).toBeGreaterThan(0);
      expect(floor).toBeLessThan((radius * FIT_DISTANCE_PER_RADIUS) / 100);
    });
  }

  it('the relation is scale-free: both scales sit at the same fraction of their fit distance', () => {
    const [small, large] = SCALES.map(
      (s) => orbitMinDistanceFor(s.radius) / (s.radius * FIT_DISTANCE_PER_RADIUS),
    );
    expect(small).toBeCloseTo(large, 12);
  });

  // A zero, negative or non-finite floor would make OrbitControls' _clampDistance
  // produce NaN camera positions, which is unrecoverable without a reload.
  for (const radius of [0, NaN, -1, Infinity]) {
    it(`degenerate radius ${radius} falls back to the strictly-positive floor`, () => {
      expect(orbitMinDistanceFor(radius)).toBe(ORBIT_MIN_DISTANCE_FLOOR);
      expect(orbitMinDistanceFor(radius)).toBeGreaterThan(0);
    });
  }
});
