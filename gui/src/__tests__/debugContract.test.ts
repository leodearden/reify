/**
 * Debug contract boundary tests (task-4293, τ0):
 * Pin the coordinate/transport CONVENTION for the reify-debug MCP expansion.
 *
 * step-3: error-envelope + wiring characterization (bridge dispatch → error shapes)
 * step-5: coordinate-convention characterization (get_layout_metrics bounds frame)
 * step-7: pick↔raycast agreement (real three.js, no mock — pins screen→NDC→raycast)
 */
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest';

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
// The REAL editor store, for the `apply_gui_state` `file`-member cases: the
// dirty/clean split they pin lives in editorStore.openFile, so a mock would
// pin nothing.
import { createEditorStore } from '../stores/editorStore';

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
      setCompileDiagnostics: vi.fn(),
      setTessellationDiagnostics: vi.fn(),
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
      closeFile: vi.fn(),
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
      setEditorWidth: vi.fn(),
      setSideWidth: vi.fn(),
      setDesignTreeHeight: vi.fn(),
      setPropertyHeight: vi.fn(),
      setConstraintHeight: vi.fn(),
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
// task 5097 δ: apply_gui_state — AI write-tool editor sync
//
// `apply_gui_state` gained a second class of caller: the five `reify_*` AI
// write tools on the reify-debug MCP server. `reify_update_source` routes
// through the IN-MEMORY `EngineSession::update_source` and writes no disk, so
// no FS-watcher re-fire will reconcile the editor buffer — the push carries
// the new text in an OPTIONAL `file` member instead. The existing
// `handle_set_fea_case` caller sends no `file` and must stay byte-identical.
// ─────────────────────────────────────────────────────────────────────────────
describe('apply_gui_state — AI write-tool editor sync (task 5097)', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  /** Minimal well-formed RawGuiState — `apply_gui_state` requires the key. */
  const RAW_GUI_STATE = {
    meshes: [],
    values: [],
    constraints: [],
    files: [],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
  };

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

  it('(a) with a file member, reopens the editor buffer and still does not reset the view', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = (await dispatchCmd(capturedHandler!, 900, 'apply_gui_state', {
      guiState: RAW_GUI_STATE,
      file: { path: '/tmp/part.ri', content: 'NEW' },
    })) as any;

    expect(result.ok).toBe(true);
    // The AI wrote the engine's in-memory buffer; nothing else will bring the
    // editor along, so the push must.
    expect(stores.editor.openFile).toHaveBeenCalledTimes(1);
    expect(stores.editor.openFile).toHaveBeenCalledWith({
      path: '/tmp/part.ri',
      content: 'NEW',
    });
    expect(stores.engine.initFromState).toHaveBeenCalledTimes(1);
    // The camera-stability property handle_set_fea_case depends on covers the
    // AI path too: a parameter tweak must not throw the user's view away.
    expect(stores.viewState.resetToDefaultView).not.toHaveBeenCalled();
  });

  it('(b) without a file member, behaves exactly as before (set_fea_case regression guard)', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = (await dispatchCmd(capturedHandler!, 901, 'apply_gui_state', {
      guiState: RAW_GUI_STATE,
      case: 'overload',
    })) as any;

    expect(result.ok).toBe(true);
    expect(result.case).toBe('overload');
    expect(stores.editor.openFile).not.toHaveBeenCalled();
    expect(stores.engine.initFromState).toHaveBeenCalledTimes(1);
    expect(stores.viewState.resetToDefaultView).not.toHaveBeenCalled();
  });

  it('(c) a malformed file member is refused and mutates neither store', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = (await dispatchCmd(capturedHandler!, 902, 'apply_gui_state', {
      guiState: RAW_GUI_STATE,
      file: { path: '/tmp/part.ri' },
    })) as any;

    expect(result.error).toBe('file requires path and content');
    expect(result.ok).toBeUndefined();
    // Refusing HALFWAY — applying the GuiState but not the buffer — is the
    // desync this handler exists to prevent, so neither store may move.
    expect(stores.editor.openFile).not.toHaveBeenCalled();
    expect(stores.engine.initFromState).not.toHaveBeenCalled();
  });

  // ───────────────────────────────────────────────────────────────────────────
  // (d)/(e) task 5097 δ amendment (review finding): what the `file` member
  // actually DOES to the editor store.
  //
  // (a) above pins only that `openFile` was CALLED, with a mock. But the member
  // is delivered through `editorStore.openFile`, whose contract is "reopen from
  // DISK" — while `reify_update_source` writes no disk at all. So the two arms
  // of `openFile`'s dirty/clean split (editorStore.ts, task-5359) land
  // differently here than they do for a watcher re-fire, and which way they land
  // is a real product decision that was neither stated nor covered. These drive
  // the REAL store so the decision is pinned in observable state, not prose; the
  // rationale for each arm is on the `apply_gui_state` handler in bridge.ts.
  // ───────────────────────────────────────────────────────────────────────────

  /** The real editor store standing in for the mocked `editor` slot. */
  function storesWithRealEditor(): DebugStores & { editor: ReturnType<typeof createEditorStore> } {
    const editor = createEditorStore();
    return { ...makeStores(), editor } as DebugStores & {
      editor: ReturnType<typeof createEditorStore>;
    };
  }

  it('(d) a CLEAN tab takes the pushed buffer — and stays clean', async () => {
    const stores = storesWithRealEditor();
    stores.editor.openFile({ path: '/tmp/part.ri', content: 'OLD' });
    await initDebugBridge(stores);

    const result = (await dispatchCmd(capturedHandler!, 903, 'apply_gui_state', {
      guiState: RAW_GUI_STATE,
      file: { path: '/tmp/part.ri', content: 'NEW' },
    })) as any;

    expect(result.ok).toBe(true);
    // The desync the member exists to close: the engine took NEW, so the buffer
    // must too.
    expect(stores.editor.state.openFiles).toHaveLength(1);
    expect(stores.editor.state.openFiles[0].content).toBe('NEW');
    // …and the tab is left CLEAN. This is the accepted consequence, stated so it
    // cannot change silently: the buffer now differs from disk with no unsaved
    // indicator, and a later CLEAN reopen of the same path (an FS-watcher
    // re-fire from reify_set_parameter, File→Open) will overwrite the AI's edit.
    // reify_save_file is the commit step; reify_set_parameter is the durable
    // write path.
    expect(stores.editor.state.dirtyFiles).toStrictEqual([]);
    expect(stores.editor.state.externallyChanged).toStrictEqual([]);
  });

  it('(e) a DIRTY tab keeps the user’s text and surfaces the conflict', async () => {
    const stores = storesWithRealEditor();
    stores.editor.openFile({ path: '/tmp/part.ri', content: 'OLD' });
    stores.editor.markDirty('/tmp/part.ri');
    stores.editor.updateFileContent('/tmp/part.ri', 'USER EDIT');
    await initDebugBridge(stores);

    const result = (await dispatchCmd(capturedHandler!, 904, 'apply_gui_state', {
      guiState: RAW_GUI_STATE,
      file: { path: '/tmp/part.ri', content: 'AI EDIT' },
    })) as any;

    expect(result.ok).toBe(true);
    // openFile does NOT clobber unsaved edits, so the engine holds the AI's text
    // while the editor keeps the user's. That divergence is real; the point is
    // that it is SURFACED rather than silently resolved in either direction.
    expect(stores.editor.state.openFiles[0].content).toBe('USER EDIT');
    expect(stores.editor.state.dirtyFiles).toStrictEqual(['/tmp/part.ri']);
    expect(stores.editor.state.externallyChanged).toStrictEqual(['/tmp/part.ri']);
    // The engine half of the push still lands — refusing it would leave the
    // design un-rendered for a conflict the user has not resolved yet.
    expect(stores.engine.initFromState).toHaveBeenCalledTimes(1);
  });

  it('(f) a no-op push over a dirty tab raises no spurious conflict', async () => {
    // The refinement editorStore.openFile makes over App.onFileChanged: a dirty
    // reopen flags a conflict only when the incoming text actually DIVERGES.
    // Pinned from this caller because a write tool re-pushing the buffer the
    // user already has is the ordinary case, not an edge one.
    const stores = storesWithRealEditor();
    stores.editor.openFile({ path: '/tmp/part.ri', content: 'OLD' });
    stores.editor.markDirty('/tmp/part.ri');
    await initDebugBridge(stores);

    const result = (await dispatchCmd(capturedHandler!, 905, 'apply_gui_state', {
      guiState: RAW_GUI_STATE,
      file: { path: '/tmp/part.ri', content: 'OLD' },
    })) as any;

    expect(result.ok).toBe(true);
    expect(stores.editor.state.openFiles[0].content).toBe('OLD');
    expect(stores.editor.state.externallyChanged).toStrictEqual([]);
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

  // Hoist the real three.js + selection imports into beforeAll so module resolution
  // (which can be slow on a loaded system — three.js is large) does not count against
  // individual test timeouts (default 15 s). The imports are cached after the first
  // call, so the per-test cost is just a Promise.resolve().
  let THREE!: typeof import('three');
  let selectionModule!: typeof import('../viewport/selection');
  beforeAll(async () => {
    THREE = await import('three');
    selectionModule = await import('../viewport/selection');
  }, 60_000); // Allow up to 60 s for the first load under system load

  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('pointerdown+pointerup at canvas center (400,300) → NDC (0,0) → hits box mesh', async () => {
    // Use real three.js imports (resolved in beforeAll — no vi.mock('three') in this file)
    const { Scene, PerspectiveCamera, Mesh, BoxGeometry } = THREE;
    const { createSelection } = selectionModule;

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
    const { Scene, PerspectiveCamera, Mesh, BoxGeometry } = THREE;
    const { createSelection } = selectionModule;

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

// ─────────────────────────────────────────────────────────────────────────────
// Camera-framing coherence for sub-150 mm parts (task 6965)
//
// The defect this pins: createControls seeds an ABSOLUTE minDistance of 0.5 m in a
// workspace whose parts span four orders of magnitude, and OrbitControls.update()
// clamps the orbit radius unconditionally (_clampDistance, OrbitControls.js:1072,
// invoked from :771/:776/:814).  So a 75 mm probe fits to ~86 mm, gets silently
// relocated back out to 0.5 m, and a follow-up zoom_camera saturates the same floor
// and reports distanceDelta: 0 — a no-op the MCP surface reports as success.
//
// Real three AND a real OrbitControls, because _clampDistance is the library
// behaviour under test; a hand-rolled stub would pass in both the broken and the
// fixed state.  Every assertion reads LIVE state (controls.getDistance(),
// camera.position) rather than a command's echoed response.
// ─────────────────────────────────────────────────────────────────────────────
describe('debug contract — small-part camera framing (real three + real OrbitControls)', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let controls: import('three/addons/controls/OrbitControls.js').OrbitControls;

  // A 75 mm-long probe, matching the dogfood part in #6496.  Dimensions in metres.
  const PROBE = { x: 0.075, y: 0.02, z: 0.01 };
  // fitCameraToBox frames the sphere circumscribing the box: radius = ½·diagonal.
  const RADIUS = 0.5 * Math.hypot(PROBE.x, PROBE.y, PROBE.z);
  // The expected framing distance comes from fitCameraToBox's own formula
  // (`fittedDistanceFor`) at the app's real FOV, never a hand-derived multiple — at
  // CAMERA_FOV_DEG = 60 and the default padding that lands at ≈ 86 mm, well inside the
  // old 0.5 m floor, which is exactly why the floor swallowed it.
  const ASPECT = 800 / 600;
  const ZOOM_SCALE = 0.3;

  let FIT_DISTANCE: number;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
    await initDebugBridge(makeStores());
    expect(capturedHandler).toBeDefined();

    const { Scene, PerspectiveCamera, Mesh, BoxGeometry, Box3 } = await import('three');
    const { fitCameraToBox } = await import('../viewport/fitCamera');
    const { CAMERA_FOV_DEG } = await import('../viewport/scene');
    const { createControls } = await import('../viewport/controls');
    const { fittedDistanceFor } = await import('../viewport/orbitDistance');

    FIT_DISTANCE = fittedDistanceFor(RADIUS, CAMERA_FOV_DEG, ASPECT);

    const scene = new Scene();
    // Same fov/aspect/near/far as createScene, so the framing arithmetic under test is
    // the shipped arithmetic.  up stays the three default (0,1,0) rather than
    // createScene's Z-up: the camera here looks straight down -Z, which would be the
    // orbit pole under Z-up and make the spherical maths degenerate for an unrelated reason.
    const camera = new PerspectiveCamera(CAMERA_FOV_DEG, ASPECT, 0.1, 10000);
    camera.position.set(0, 0, 1);
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();

    const mesh = new Mesh(new BoxGeometry(PROBE.x, PROBE.y, PROBE.z));
    mesh.name = 'entity/probe';
    mesh.updateMatrixWorld();
    scene.add(mesh);

    const domElement = document.createElement('canvas');
    Object.defineProperty(domElement, 'clientHeight', { value: 600 });
    Object.defineProperty(domElement, 'clientWidth', { value: 800 });
    vi.spyOn(domElement, 'getBoundingClientRect').mockReturnValue({
      left: 0, top: 0, width: 800, height: 600,
      x: 0, y: 0, right: 800, bottom: 600, toJSON: () => ({}),
    } as DOMRect);

    // createControls, not `new OrbitControls`, so the startup minDistance under test is
    // the one the app actually seeds.
    controls = createControls(camera, domElement).controls;
    controls.enableDamping = false;
    controls.update();

    const box = new Box3().expandByObject(mesh);

    window.__REIFY_DEBUG__!.viewport = {
      scene,
      camera,
      renderer: { domElement, render: vi.fn() } as any,
      getMeshes: () => new Map([['entity/probe', mesh]]),
      getGhostMeshes: () => new Map(),
      // Mirrors selection.ts's fitToView (fitCameraToBox over the mesh bounds), plus the
      // controls.update() that Viewport.tsx's RAF loop runs on the very next frame — which
      // is where _clampDistance actually bites.
      fitToView: () => {
        fitCameraToBox(camera, box, { controls });
        controls.update();
      },
      flyToEntity: vi.fn(),
      controls: controls as any,
    };
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('fit_to_view frames a 75 mm part at its computed distance, not a fixed floor', async () => {
    const result = (await dispatchCmd(capturedHandler!, 6001, 'fit_to_view', {})) as any;
    expect(result.ok).toBe(true);

    // LIVE state, not the echoed response.  Before the fix this read exactly 0.5 —
    // _clampDistance relocated the camera and nothing reported it.
    expect(controls.getDistance()).toBeCloseTo(FIT_DISTANCE, 5);
    expect(controls.getDistance()).toBeLessThan(0.5);
  });

  it('zoom_camera actually dollies in from a fitted small part', async () => {
    await dispatchCmd(capturedHandler!, 6002, 'fit_to_view', {});
    const fitted = controls.getDistance();

    const result = (await dispatchCmd(capturedHandler!, 6003, 'zoom_camera', {
      scale: ZOOM_SCALE,
    })) as any;

    // _dollyIn multiplies the scale into the orbit radius (OrbitControls.js:1034), so
    // scale 0.3 means "30% of the current distance".
    expect(result.ok).toBe(true);
    expect(result.distanceDelta).toBeGreaterThan(0);
    expect(controls.getDistance()).toBeCloseTo(fitted * ZOOM_SCALE, 5);
  });

  it('pick_entity_at still resolves the part from the close-in pose', async () => {
    await dispatchCmd(capturedHandler!, 6004, 'fit_to_view', {});
    await dispatchCmd(capturedHandler!, 6005, 'zoom_camera', { scale: ZOOM_SCALE });

    const result = (await dispatchCmd(capturedHandler!, 6006, 'pick_entity_at', {
      x: 400,
      y: 300,
    })) as any;

    // The screenshot → set_camera → pick → identify loop has to work at the close-in
    // pose, not just at the fitted one.
    expect(result.hit).toBe(true);
    expect(result.entityPath).toBe('entity/probe');
    // Canvas centre looks straight down -Z at the box centre, so the hit is the +Z face.
    expect(result.point.z).toBeCloseTo(PROBE.z / 2, 5);
  });
});
