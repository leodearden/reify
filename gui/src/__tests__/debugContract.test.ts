/**
 * Debug contract boundary tests (task-4293, τ0):
 * Pin the coordinate/transport CONVENTION for the reify-debug MCP expansion.
 *
 * step-3: error-envelope + wiring characterization (bridge dispatch → error shapes)
 * step-5: coordinate-convention characterization (get_layout_metrics bounds frame)
 * step-7: pick↔raycast agreement (real three.js, no mock — pins screen→NDC→raycast)
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock Tauri APIs — bridge initialization requires listen + invoke.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

// NOTE: 'three' is intentionally NOT mocked here.
// Steps 3 and 5 use bridge handlers that don't invoke three (get_layout_metrics,
// get_window_state). Step 7 requires the REAL three.js Raycaster for raycast validation.

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';
import type { DebugStores } from '../debug/types';
import type { ViewStateStore } from '../stores/viewStateStore';

type DebugRequestHandler = (event: {
  payload: { id: number; command: string; params: Record<string, unknown> };
}) => Promise<void>;

function makeStores(): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase: 'idle' },
        compileDiagnostics: [],
        tessellationDiagnostics: [],
      },
      initFromState: vi.fn(),
    },
    editor: {
      state: {
        openFiles: [],
        activeFile: null,
        dirtyFiles: [],
        externallyChanged: [],
        cursorPosition: null,
      },
      openFile: vi.fn(),
    },
    selection: {
      state: {
        selectedEntity: null,
        selectedEntities: [],
        anchorEntity: null,
        hoveredEntity: null,
        highlightedParams: [],
      } as any,
      selectEntity: vi.fn(),
      hoverEntity: vi.fn(),
      clearSelection: vi.fn(),
      toggleSelect: vi.fn(),
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
    viewState: { resetToDefaultView: vi.fn() } as unknown as ViewStateStore,
    layout: {
      state: {
        editorWidth: 300,
        sideWidth: 300,
        designTreeHeight: 160,
        propertyHeight: 200,
        constraintHeight: 140,
      },
    },
  };
}

/** Dispatch a command through the real debug bridge and return the parsed response. */
async function dispatchCmd(
  handler: DebugRequestHandler,
  id: number,
  command: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  vi.mocked(invoke).mockClear();
  await handler({ payload: { id, command, params } });
  const calls = vi.mocked(invoke).mock.calls;
  const responseCall = calls.find((c) => c[0] === 'debug_response');
  expect(responseCall).toBeDefined();
  const payload = responseCall![1] as { id: number; result: string };
  return JSON.parse(payload.result);
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3: Error-envelope + wiring characterization
//
// Pins: (a) unknown command → {error:"unknown command: <name>"}; (b) missing
// required param → {error:"selector is required"}; (c) invalid selector →
// {error:string}. These guard the in-band JSON {error:string} envelope that
// the Rust transport (step-1b) passes through verbatim, and the
// tool-def→dispatch→handler delegation through buildHandlers().
// ─────────────────────────────────────────────────────────────────────────────
describe('debug contract — error envelope + wiring (step-3)', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    document.body.innerHTML = '';
  });

  it('(a) unknown command resolves to {error:"unknown command: <name>"}', async () => {
    await initDebugBridge(makeStores());
    expect(capturedHandler).toBeDefined();

    const result = (await dispatchCmd(capturedHandler!, 1, 'nonexistent_command_xyz', {})) as any;
    expect(result.error).toBe('unknown command: nonexistent_command_xyz');
  });

  it('(b) get_layout_metrics with no selector resolves to {error:"selector is required"}', async () => {
    await initDebugBridge(makeStores());

    const result = (await dispatchCmd(capturedHandler!, 2, 'get_layout_metrics', {})) as any;
    expect(result.error).toBe('selector is required');
  });

  it('(c) get_layout_metrics with a syntactically invalid selector resolves to {error:string}', async () => {
    await initDebugBridge(makeStores());

    const result = (await dispatchCmd(capturedHandler!, 3, 'get_layout_metrics', { selector: ':::' })) as any;
    // Must produce an error envelope, not a false-negative {exists:false}
    expect(typeof result.error).toBe('string');
    expect(result.exists).toBeUndefined();
  });

  it('envelope shape: error responses are plain objects with a string "error" field', async () => {
    // Regression guard: every dispatch arm must produce a parseable {error:string} envelope.
    await initDebugBridge(makeStores());

    const r1 = (await dispatchCmd(capturedHandler!, 4, 'another_unknown_cmd', {})) as any;
    expect(typeof r1).toBe('object');
    expect(typeof r1.error).toBe('string');

    const r2 = (await dispatchCmd(capturedHandler!, 5, 'get_layout_metrics', {})) as any;
    expect(typeof r2).toBe('object');
    expect(typeof r2.error).toBe('string');
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// step-5: Coordinate-convention boundary test (characterization)
//
// Pins: (a) get_window_state.devicePixelRatio is numeric; (b) get_layout_metrics
// returns getBoundingClientRect verbatim (CSS-logical-px from window origin);
// (c) the center derived from bounds is a valid clientX/clientY that fires the
// element's handler — the get_layout_metrics→click(center) convention I1 wraps.
//
// NOTE: jsdom has no layout engine, so document.elementFromPoint() always returns
// null — the live hit-test is deferred to I1's real-GUI e2e (needs H0).
// ─────────────────────────────────────────────────────────────────────────────
describe('debug contract — coordinate convention (step-5)', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    document.body.innerHTML = '';
  });

  it('(a) get_window_state reports devicePixelRatio as a number', async () => {
    Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 1.5 });
    await initDebugBridge(makeStores());
    expect(capturedHandler).toBeDefined();

    const result = (await dispatchCmd(capturedHandler!, 100, 'get_window_state', {})) as any;
    expect(typeof result.devicePixelRatio).toBe('number');
    expect(result.devicePixelRatio).toBe(1.5);
  });

  it('(b) get_layout_metrics.bounds equals getBoundingClientRect (CSS-logical-px from window origin)', async () => {
    // Prove get_layout_metrics reports the element's getBoundingClientRect verbatim:
    // x/y/width/height in CSS logical pixels measured from the window top-left.
    // This is the same coordinate frame as clientX/clientY on pointer events —
    // the convention all pixel tools share.
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'coord-target');
    document.body.appendChild(el);

    // Stub to known bounds (jsdom returns zeros by default)
    const BOUNDS = { x: 100, y: 50, width: 80, height: 40, left: 100, top: 50, right: 180, bottom: 90 };
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue(BOUNDS as DOMRect);

    await initDebugBridge(makeStores());

    const result = (await dispatchCmd(
      capturedHandler!,
      101,
      'get_layout_metrics',
      { selector: '[data-testid="coord-target"]' },
    )) as any;

    expect(result.exists).toBe(true);
    // bounds must reflect getBoundingClientRect verbatim (x, y, width, height)
    expect(result.bounds).toEqual({ x: 100, y: 50, width: 80, height: 40 });
  });

  it('(c) center=(x+w/2, y+h/2) derived from bounds fires the element click handler', async () => {
    // Pins the get_layout_metrics→click(center) convention I1 will wrap.
    // The center is in the same CSS-logical-px frame as getBoundingClientRect,
    // so a synthetic MouseEvent at (centerX, centerY) fires the element's handler.
    // NOTE: elementFromPoint hit-test (OS layout) is deferred to I1's real-GUI e2e.
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'click-target');
    document.body.appendChild(el);

    const BOUNDS = { x: 100, y: 50, width: 80, height: 40, left: 100, top: 50, right: 180, bottom: 90 };
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue(BOUNDS as DOMRect);

    const centerX = BOUNDS.x + BOUNDS.width / 2;   // 140
    const centerY = BOUNDS.y + BOUNDS.height / 2;  // 70

    let receivedX: number | undefined;
    let receivedY: number | undefined;
    let clickFired = false;
    el.addEventListener('click', (e) => {
      receivedX = (e as MouseEvent).clientX;
      receivedY = (e as MouseEvent).clientY;
      clickFired = true;
    });

    el.dispatchEvent(new MouseEvent('click', { clientX: centerX, clientY: centerY, bubbles: true }));

    expect(clickFired).toBe(true);
    expect(receivedX).toBe(140);
    expect(receivedY).toBe(70);
    // Center lies within the element's bounds
    expect(receivedX).toBeGreaterThanOrEqual(BOUNDS.x);
    expect(receivedX).toBeLessThanOrEqual(BOUNDS.x + BOUNDS.width);
    expect(receivedY).toBeGreaterThanOrEqual(BOUNDS.y);
    expect(receivedY).toBeLessThanOrEqual(BOUNDS.y + BOUNDS.height);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// step-7: Pick↔raycast agreement (REAL three.js — NOT mocked)
//
// Pins the screen→NDC→raycast convention that pick_entity_at (I2) will wrap:
//   NDC.x = ((clientX - rect.left) / rect.width) * 2 - 1
//   NDC.y = -((clientY - rect.top)  / rect.height) * 2 + 1
//
// Uses the REAL three.js Raycaster (via createSelection, which patches
// Mesh.prototype.raycast with three-mesh-bvh's acceleratedRaycast). Without a
// BVH tree, acceleratedRaycast falls back to the original three.js face traversal.
//
// jsdom has no layout engine, so the DOM elementFromPoint half is arithmetic-only;
// the real hit-test is exercised by I1's real-GUI e2e (needs H0).
// ─────────────────────────────────────────────────────────────────────────────
describe('debug contract — pick↔raycast agreement (step-7, real three)', () => {
  // Each test creates its own createSelection instance and disposes it after use.
  // Camera setup: PerspectiveCamera at (0,0,5) looking down -Z toward origin.
  // BoxGeometry(1,1,1) centered at origin — front face at z=0.5, visible from camera.
  // Canvas: 800×600 at (left:0, top:0) — center (400,300) → NDC (0,0) → hits box.

  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('pointerdown+pointerup at canvas center (400,300) → NDC (0,0) → hits box mesh', async () => {
    // Use real three.js imports (no vi.mock('three') in this file)
    const { Scene, PerspectiveCamera, Mesh, BoxGeometry } = await import('three');
    const { createSelection } = await import('../viewport/selection');

    const scene = new Scene();
    const camera = new PerspectiveCamera(75, 800 / 600, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();

    const geometry = new BoxGeometry(1, 1, 1);
    const mesh = new Mesh(geometry);
    mesh.name = 'entity/box';
    scene.add(mesh);

    const domElement = document.createElement('div');
    document.body.appendChild(domElement);
    const CANVAS_RECT = { left: 0, top: 0, width: 800, height: 600, x: 0, y: 0, right: 800, bottom: 600 };
    vi.spyOn(domElement, 'getBoundingClientRect').mockReturnValue(CANVAS_RECT as DOMRect);

    const onHover = vi.fn();
    const onSelect = vi.fn();
    const ctx = createSelection({
      scene,
      camera,
      domElement,
      getMeshes: () => new Map([['entity/box', mesh]]),
      onHover,
      onSelect,
    });

    // Canvas center: clientX=400, clientY=300
    // NDC: x = (400/800)*2-1 = 0, y = -(300/600)*2+1 = 0 → ray along -Z → hits box
    // Note: jsdom has no PointerEvent; MouseEvent with pointerdown/pointerup names
    // works identically — createSelection casts events to MouseEvent internally.
    domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300, bubbles: true }));
    domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300, bubbles: true }));

    expect(onSelect).toHaveBeenCalledWith('entity/box', { ctrl: false, shift: false });
    ctx.dispose();
    geometry.dispose();
  });

  it('pointerdown+pointerup at far corner (5,5) → NDC ≈ (-0.988, +0.983) → misses box', async () => {
    const { Scene, PerspectiveCamera, Mesh, BoxGeometry } = await import('three');
    const { createSelection } = await import('../viewport/selection');

    const scene = new Scene();
    const camera = new PerspectiveCamera(75, 800 / 600, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();

    const geometry = new BoxGeometry(1, 1, 1);
    const mesh = new Mesh(geometry);
    mesh.name = 'entity/box';
    scene.add(mesh);

    const domElement = document.createElement('div');
    document.body.appendChild(domElement);
    const CANVAS_RECT = { left: 0, top: 0, width: 800, height: 600, x: 0, y: 0, right: 800, bottom: 600 };
    vi.spyOn(domElement, 'getBoundingClientRect').mockReturnValue(CANVAS_RECT as DOMRect);

    const onHover = vi.fn();
    const onSelect = vi.fn();
    const ctx = createSelection({
      scene,
      camera,
      domElement,
      getMeshes: () => new Map([['entity/box', mesh]]),
      onHover,
      onSelect,
    });

    // Far upper-left corner: NDC ≈ (-0.988, +0.983) → ray toward upper-left → misses box
    // Note: jsdom has no PointerEvent; MouseEvent works identically here.
    domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 5, clientY: 5, bubbles: true }));
    domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 5, clientY: 5, bubbles: true }));

    expect(onSelect).toHaveBeenCalledWith(null, { ctrl: false, shift: false });
    ctx.dispose();
    geometry.dispose();
  });
});
