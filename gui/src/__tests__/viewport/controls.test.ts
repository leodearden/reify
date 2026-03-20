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
});
