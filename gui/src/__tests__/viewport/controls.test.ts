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
    // Real OrbitControls seeds `target = new Vector3()` in its constructor, and
    // createControls reads it to size the startup floor — a mock without it would
    // make that read throw rather than exercise the policy.  A plain triple, not a
    // Vector3: vi.mock factories are hoisted above the imports, and Vector3.distanceTo
    // reads only x/y/z from its argument.
    target = { x: 0, y: 0, z: 0 };
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

import { PerspectiveCamera, Vector3 } from 'three';
import { createControls } from '../../viewport/controls';
import {
  fittedDistanceFor,
  orbitFloorFor,
  ORBIT_MIN_DISTANCE_FLOOR,
  ORBIT_MAX_DISTANCE,
} from '../../viewport/orbitDistance';
// The app's real field of view, so the headroom asserted below is the headroom the
// shipped camera actually gets.  A restatement here would keep passing against a
// stale angle after scene.ts retuned it — the whole point of deriving it.
import { CAMERA_FOV_DEG } from '../../viewport/scene';

beforeEach(() => {
  vi.clearAllMocks();
  capturedCamera = undefined;
  capturedDomElement = undefined;
  capturedInstance = undefined;
});

describe('createControls', () => {
  // Mirrors createScene: a real PerspectiveCamera at the iso-ish default pose, which is
  // what createControls measures its provisional floor against.
  const STARTUP_POSITION = new Vector3(5, 5, 5);

  function setup(position: Vector3 = STARTUP_POSITION) {
    const camera = new PerspectiveCamera(CAMERA_FOV_DEG, 4 / 3, 0.1, 10000);
    camera.position.copy(position);
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

  it('camera and mock agree on the startup pose the floor is measured from', () => {
    const { camera } = setup();
    expect(camera.position.distanceTo(capturedInstance.target)).toBeCloseTo(
      STARTUP_POSITION.length(),
      12,
    );
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
  // no bounds to derive a floor from.  It applies the SAME policy to the startup orbit
  // distance instead, which rules out both degenerate seeds: a guessed absolute blocks
  // commanded poses closer than itself (0.5 m made a fitted 75 mm part unreachable —
  // #6965), while ORBIT_MIN_DISTANCE_FLOOR blocks nothing at all and lets the wheel dolly
  // an empty scene to a 1e-6 radius that is ~160 multiplicative ticks from workable.
  it('seeds minDistance by applying the policy to the startup orbit distance', () => {
    const { camera } = setup();
    const startupDistance = camera.position.distanceTo(capturedInstance.target);
    expect(capturedInstance.minDistance).toBe(orbitFloorFor(startupDistance));
  });

  it('the startup seed is neither the degenerate floor nor a pose-blocking guess', () => {
    const { camera } = setup();
    const startupDistance = camera.position.distanceTo(capturedInstance.target);
    // Strictly above the NaN guard: an empty scene stays wheel-recoverable.
    expect(capturedInstance.minDistance).toBeGreaterThan(ORBIT_MIN_DISTANCE_FLOOR);
    // …and strictly below where the camera already is, so it blocks no startup pose.
    expect(capturedInstance.minDistance).toBeLessThan(startupDistance);
  });

  // A camera sitting exactly on its target has no orbit distance to scale from.  The
  // policy's guard must produce the strictly-positive floor here, never 0 or NaN —
  // OrbitControls' _clampDistance turns either into unrecoverable NaN positions.
  it('a degenerate startup pose falls back to the strictly-positive floor', () => {
    setup(new Vector3(0, 0, 0));
    expect(capturedInstance.minDistance).toBe(ORBIT_MIN_DISTANCE_FLOOR);
    expect(capturedInstance.minDistance).toBeGreaterThan(0);
  });

  it('seeds maxDistance from the policy constant', () => {
    setup();
    expect(capturedInstance.maxDistance).toBe(ORBIT_MAX_DISTANCE);
  });
});

// The policy is stated as a RELATION to each model's own fitted distance, never as an
// absolute — that is the whole point of the fix.  Every fitted distance below comes from
// `fittedDistanceFor` at the app's real FOV, the same function fitCameraToBox calls, so
// retuning the padding or the field of view moves these expectations with the shipped
// behaviour instead of leaving them pinned to a stale hand-derived multiple.
describe('orbitFloorFor', () => {
  /**
   * Two real scales, four orders of magnitude apart: the 75 mm litter-tray round-3 probe
   * (#6965) and the 1 m printer from the printer_v01 repro (#6496).
   */
  const SCALES = [
    { label: '75 mm probe', radius: 0.0375 },
    { label: '1 m printer', radius: 0.87 },
  ];

  /** Pane shapes bracketing the design pane: wide, square, and tall/narrow (esc-4280). */
  const ASPECTS = [16 / 9, 1, 0.4];

  for (const { label, radius } of SCALES) {
    for (const aspect of ASPECTS) {
      it(`${label} at aspect ${aspect.toFixed(2)}: floor is under 1/100 of the fitted distance`, () => {
        const fitted = fittedDistanceFor(radius, CAMERA_FOV_DEG, aspect);
        const floor = orbitFloorFor(fitted);
        expect(floor).toBeGreaterThan(0);
        expect(floor).toBeLessThan(fitted / 100);
      });
    }
  }

  it('the relation is scale-free: both scales sit at the same fraction of their fit distance', () => {
    const [small, large] = SCALES.map((s) => {
      const fitted = fittedDistanceFor(s.radius, CAMERA_FOV_DEG);
      return orbitFloorFor(fitted) / fitted;
    });
    expect(small).toBeCloseTo(large, 12);
  });

  // A zero, negative or non-finite floor would make OrbitControls' _clampDistance
  // produce NaN camera positions, which is unrecoverable without a reload.
  for (const framedDistance of [0, NaN, -1, Infinity]) {
    it(`degenerate framed distance ${framedDistance} falls back to the strictly-positive floor`, () => {
      expect(orbitFloorFor(framedDistance)).toBe(ORBIT_MIN_DISTANCE_FLOOR);
      expect(orbitFloorFor(framedDistance)).toBeGreaterThan(0);
    });
  }
});

// fittedDistanceFor is the SPOT for "how far back frames a sphere of this radius".  These
// pin the properties every caller — fitCameraToBox and the floor policy alike — relies on.
describe('fittedDistanceFor', () => {
  it('scales linearly with radius, so the floor stays a fixed fraction at any scale', () => {
    const unit = fittedDistanceFor(1, CAMERA_FOV_DEG);
    expect(fittedDistanceFor(1000, CAMERA_FOV_DEG)).toBeCloseTo(unit * 1000, 9);
  });

  it('scales linearly with padding', () => {
    const base = fittedDistanceFor(1, CAMERA_FOV_DEG, 1, 1.1);
    expect(fittedDistanceFor(1, CAMERA_FOV_DEG, 1, 2.2)).toBeCloseTo(base * 2, 12);
  });

  it('a tall/narrow pane fits FARTHER back than a square one; a wide pane does not', () => {
    const square = fittedDistanceFor(1, CAMERA_FOV_DEG, 1);
    expect(fittedDistanceFor(1, CAMERA_FOV_DEG, 0.4)).toBeGreaterThan(square);
    expect(fittedDistanceFor(1, CAMERA_FOV_DEG, 16 / 9)).toBeCloseTo(square, 12);
  });

  it('a wider field of view fits closer in', () => {
    expect(fittedDistanceFor(1, 90)).toBeLessThan(fittedDistanceFor(1, 30));
  });
});
