/**
 * Tests for fitCameraToBox helper (gui/src/viewport/fitCamera.ts).
 *
 * These tests use REAL three.js (no vi.mock('three')) — PerspectiveCamera,
 * Box3, and Vector3.project all work in jsdom for pure math/projection.
 *
 * The printer-scale regression test is the screenshot-equivalent acceptance
 * check: project all 8 scene-bounds corners through the camera and assert
 * each lands inside the view frustum (NDC x,y ∈ (-1,1)).  This case FAILS
 * against the old vertical-only/maxDim formula (corners clip horizontally on
 * a narrow-aspect pane) and PASSES after the fix, pinning the real behaviour.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { PerspectiveCamera, Box3, Vector3 } from 'three';
import { fitCameraToBox, type FitCameraOptions } from '../../viewport/fitCamera';

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

/** Build the 8 corners of an axis-aligned Box3. */
function boxCorners(box: Box3): Vector3[] {
  const { min, max } = box;
  return [
    new Vector3(min.x, min.y, min.z),
    new Vector3(max.x, min.y, min.z),
    new Vector3(min.x, max.y, min.z),
    new Vector3(max.x, max.y, min.z),
    new Vector3(min.x, min.y, max.z),
    new Vector3(max.x, min.y, max.z),
    new Vector3(min.x, max.y, max.z),
    new Vector3(max.x, max.y, max.z),
  ];
}

/**
 * Project all 8 corners of `box` through `camera` and return the maximum
 * absolute NDC coordinate across both axes (the "worst corner").
 */
function maxNdcExtent(camera: PerspectiveCamera, box: Box3): number {
  camera.updateMatrixWorld(true);
  return boxCorners(box).reduce((worst, corner) => {
    const ndc = corner.clone().project(camera);
    return Math.max(worst, Math.abs(ndc.x), Math.abs(ndc.y));
  }, 0);
}

/**
 * Set up a camera aimed at `center` from an iso-ish direction, then call
 * fitCameraToBox.  Returns the camera for further assertions.
 */
function setupAndFit(
  fov: number,
  aspect: number,
  box: Box3,
  fitOptions?: FitCameraOptions,
): PerspectiveCamera {
  const camera = new PerspectiveCamera(fov, aspect, 0.1, 1e5);
  camera.up.set(0, 0, 1); // Z-up, matching scene.ts

  const center = new Vector3();
  box.getCenter(center);

  // Position camera at center + offset in (1,1,1) direction so it has a
  // well-defined view direction toward the assembly.
  const iso = new Vector3(1, 1, 1).normalize().multiplyScalar(5000);
  camera.position.copy(center).add(iso);
  camera.lookAt(center);
  camera.updateMatrixWorld(true);

  fitCameraToBox(camera, box, fitOptions);

  camera.updateMatrixWorld(true);
  return camera;
}

// ---------------------------------------------------------------------------
// Printer-scale elongated box — mirrors printer.ri rods/tendons geometry
//   long axis Y = 800 mm, short axes X/Z ≈ 200 mm
//   center at origin
// ---------------------------------------------------------------------------
const PRINTER_BOX = new Box3(
  new Vector3(-100, -400, -100),
  new Vector3(100, 400, 100),
);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('fitCameraToBox', () => {
  // (1) REGRESSION + PADDING: on a tall/narrow pane (aspect ≈ 0.4) the old
  //     vertical-only formula clips the printer assembly horizontally.  After
  //     the fix all 8 corners must be inside the frustum AND the assembly must
  //     span a meaningful fraction of the frame (predictable padding margin).
  it('frames the printer-scale assembly with margin on a tall/narrow viewport (aspect ≈ 0.4)', () => {
    const aspect = 0.4; // tall/narrow design pane
    const camera = setupAndFit(60, aspect, PRINTER_BOX);

    const worst = maxNdcExtent(camera, PRINTER_BOX);
    // All 8 corners must be inside the frustum — no horizontal/vertical clipping.
    expect(worst).toBeLessThan(1.0);
    // Assembly spans a meaningful fraction of the frame (padding is not absurd).
    expect(worst).toBeGreaterThan(0.3);
  });

  // (3) ASPECT MONOTONICITY: narrower viewport → camera must be farther away.
  //     The old vertical-only formula gives equal distances regardless of aspect,
  //     so this test encodes that aspect is properly accounted for.
  it('places the camera farther from the assembly on a narrower viewport', () => {
    const center = new Vector3();
    PRINTER_BOX.getCenter(center);

    const cameraWide = setupAndFit(60, 3.0, PRINTER_BOX);
    const cameraNarrow = setupAndFit(60, 0.3, PRINTER_BOX);

    const distWide = cameraWide.position.distanceTo(center);
    const distNarrow = cameraNarrow.position.distanceTo(center);

    expect(distNarrow).toBeGreaterThan(distWide);
  });

  // (4) WIDE-ASPECT CONTAINMENT: vertical FOV becomes the binding constraint —
  //     symmetric guard that all corners still fit when aspect > 1.
  it('frames the full assembly on a wide-aspect viewport (aspect = 3.0)', () => {
    const aspect = 3.0;
    const camera = setupAndFit(60, aspect, PRINTER_BOX);

    const worst = maxNdcExtent(camera, PRINTER_BOX);
    expect(worst).toBeLessThan(1.0);
  });

  // (5) CONTRACT: controls.target is set to box center; view direction is preserved.
  it('sets controls.target to box center and preserves the view direction', () => {
    const controlsTarget = new Vector3();
    const controls = { target: controlsTarget };

    const box = PRINTER_BOX;
    const boxCenter = new Vector3();
    box.getCenter(boxCenter);

    const camera = new PerspectiveCamera(60, 0.4, 0.1, 1e5);
    camera.up.set(0, 0, 1);
    const iso = new Vector3(1, 1, 1).normalize().multiplyScalar(5000);
    camera.position.copy(boxCenter).add(iso);
    camera.lookAt(boxCenter);
    camera.updateMatrixWorld(true);

    // Capture view direction BEFORE fit
    const beforeDir = new Vector3();
    camera.getWorldDirection(beforeDir);

    fitCameraToBox(camera, box, { controls });
    camera.updateMatrixWorld(true);

    // After fit — view direction must be essentially unchanged
    const afterDir = new Vector3();
    camera.getWorldDirection(afterDir);
    const dot = beforeDir.dot(afterDir);
    expect(dot).toBeCloseTo(1.0, 4); // ≥ 0.9999

    // controls.target must be at box center
    expect(controls.target.x).toBeCloseTo(boxCenter.x, 3);
    expect(controls.target.y).toBeCloseTo(boxCenter.y, 3);
    expect(controls.target.z).toBeCloseTo(boxCenter.z, 3);
  });

  // (6) DEGENERATE BOX: zero-volume box (min === max) must not move the camera
  //     or mutate controls.target — guard against NaN positions.
  it('no-ops on a zero-volume (degenerate) box', () => {
    const camera = new PerspectiveCamera(60, 1, 0.1, 1e5);
    const initialPos = camera.position.clone();
    const controls = { target: new Vector3(99, 99, 99) };

    // Box with zero extent (min === max → radius = 0 → guard should fire)
    const degenBox = new Box3(new Vector3(5, 5, 5), new Vector3(5, 5, 5));

    fitCameraToBox(camera, degenBox, { controls });

    // Camera position must be unchanged
    expect(camera.position.x).toBe(initialPos.x);
    expect(camera.position.y).toBe(initialPos.y);
    expect(camera.position.z).toBe(initialPos.z);
    // controls.target must also be unchanged
    expect(controls.target.x).toBe(99);
    expect(controls.target.y).toBe(99);
    expect(controls.target.z).toBe(99);
  });

  // (7) CUSTOM PADDING: caller-supplied `padding` must scale camera distance
  //     linearly — ratio of padded distance to default-padded distance must
  //     equal the ratio of the padding values (2.2 / 1.1 = 2.0).
  it('scales camera distance proportionally with a custom padding option', () => {
    const center = new Vector3();
    PRINTER_BOX.getCenter(center);
    const aspect = 1.0;

    // Default padding (1.1)
    const cameraDefault = setupAndFit(60, aspect, PRINTER_BOX);
    const distDefault = cameraDefault.position.distanceTo(center);

    // Explicit padding = 2.2 (exactly 2× the default)
    const cameraPadded = setupAndFit(60, aspect, PRINTER_BOX, { padding: 2.2 });
    const distPadded = cameraPadded.position.distanceTo(center);

    // Distance must scale linearly: ratio ≈ 2.2 / 1.1 = 2.0
    expect(distPadded / distDefault).toBeCloseTo(2.0, 3);
    // Sanity-check: larger padding → strictly larger distance
    expect(distPadded).toBeGreaterThan(distDefault);
  });
});
