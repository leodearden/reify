// JS Debug Bridge — listens for debug-request events from the Rust backend,
// dispatches commands, and returns results via the debug_response Tauri command.

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { DebugStores, DebugViewport, ReifyDebugContext } from './types';
import { convertRawGuiState } from '../types';
import type { RawGuiState } from '../types';
import { Box3, Vector3 } from 'three';
import type { Mesh, BufferGeometry } from 'three';
import { testMode, setTestMode } from './testMode';
import { toPng } from 'html-to-image';

// Reject oversize payloads before they hit the Tauri IPC channel.
// 16 MB ceiling is empirical: html-to-image silently truncates output above the
// ~16 MB SVG foreignObject XML limit, and payloads beyond this also risk crashing
// the Tauri WebView IPC channel (observed in task-3634 / commit 412aa4b8bd).
// ascii base64 data URLs are 1 char ≈ 1 byte, so string length is a valid proxy.
const MAX_SCREENSHOT_CHARS = 16 * 1024 * 1024;

// Default curated CSS property subset for get_computed_style (step-8 GREEN).
// PRD §4 resolved decision #2: avoid dumping all ~400 computed properties.
const CURATED_STYLE_PROPS = [
  'display', 'visibility', 'opacity', 'color', 'backgroundColor',
  'fontSize', 'fontFamily', 'fontWeight', 'overflow', 'position', 'width', 'height',
] as const;

type CommandHandler = (params: Record<string, unknown>) => unknown | Promise<unknown>;

/** Returns true iff v is a 3-element array of finite numbers. */
function validVec3(v: unknown): v is [number, number, number] {
  return (
    Array.isArray(v) &&
    v.length === 3 &&
    v.every((n) => typeof n === 'number' && Number.isFinite(n))
  );
}

/**
 * Resolve which DebugViewport to target for a given command invocation.
 *
 * Precedence (documented in design decisions):
 *  1. params.viewportId present → look up in ctx.viewports; error if unknown.
 *  2. First entry in ctx.viewports whose getMeshes().size > 0 (insertion order).
 *  3. First registered entry in ctx.viewports (any registered).
 *  4. Legacy ctx.viewport single slot (backward compat with direct-stub tests).
 *  5. { error: 'viewport not ready' }.
 */
function pickViewport(
  ctx: ReifyDebugContext,
  params: Record<string, unknown>,
): { viewport: DebugViewport } | { error: string } {
  const id = params.viewportId;

  if (id !== undefined) {
    // Reject non-string values before the lookup so the caller gets a clear
    // schema-violation message rather than an ambiguous "not registered" error.
    if (typeof id !== 'string') return { error: 'viewportId must be a string' };
    // Explicit selection — must exist.
    const vp = ctx.viewports?.[id];
    if (!vp) return { error: `viewport '${id}' not registered` };
    return { viewport: vp };
  }

  // No explicit id — scan for first populated viewport.
  if (ctx.viewports) {
    // Insertion-order scan: first with meshes > 0.
    for (const vp of Object.values(ctx.viewports)) {
      if (vp.getMeshes().size > 0) return { viewport: vp };
    }
    // Fallback: first registered (any).
    const first = Object.values(ctx.viewports)[0];
    if (first) return { viewport: first };
  }

  // Legacy single slot (preserves existing direct-stub tests).
  if (ctx.viewport) return { viewport: ctx.viewport };

  return { error: 'viewport not ready' };
}

// Shared element descriptor used by query_selector and query_selector_all.
// Mirrors the bounds + visible formula from dom_query for cross-tool consistency.
function describeElement(el: HTMLElement) {
  const rect = el.getBoundingClientRect();
  const style = window.getComputedStyle(el);
  return {
    tagName: el.tagName.toLowerCase(),
    testId: el.getAttribute('data-testid'),
    text: el.innerText?.slice(0, 500) ?? '',
    bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    visible: style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0,
  };
}

// Validates selector param, queries the DOM, and returns either an error, the
// matched element, or null (no match). Handlers map null → {exists:false}.
function resolveElement(params: Record<string, unknown>): { error: string } | { el: Element | null } {
  const selector = params.selector as string;
  if (!selector) return { error: 'selector is required' };
  try {
    return { el: document.querySelector(selector) };
  } catch (e) {
    return { error: (e as Error).message };
  }
}

// Returns focusable elements in document order, excluding disabled/tabindex=-1
// and elements hidden via computed display:none or visibility:hidden.
// Does NOT use offsetParent/getBoundingClientRect — unavailable in jsdom.
// Note: input[type=hidden] is excluded explicitly; the [tabindex] group also
// guards against disabled elements that carry an explicit non-negative tabindex.
function collectTabbables(): HTMLElement[] {
  const candidates = document.querySelectorAll<HTMLElement>(
    'a[href]:not([tabindex="-1"]), button:not([disabled]):not([tabindex="-1"]), input:not([type="hidden"]):not([disabled]):not([tabindex="-1"]), select:not([disabled]):not([tabindex="-1"]), textarea:not([disabled]):not([tabindex="-1"]), [tabindex]:not([tabindex="-1"]):not([disabled])',
  );
  return Array.from(candidates).filter((el) => {
    const style = window.getComputedStyle(el);
    return style.display !== 'none' && style.visibility !== 'hidden';
  });
}

function describeActive(el: HTMLElement) {
  return { testId: el.getAttribute('data-testid'), tagName: el.tagName.toLowerCase() };
}

function buildHandlers(ctx: ReifyDebugContext): Record<string, CommandHandler> {
  return {
    // --- Read commands (frontend-mediated) ---

    store_state: () => {
      const { engine, editor, selection, claude } = ctx.stores;
      return {
        engine: {
          meshKeys: Object.keys(engine.state.meshes),
          meshCount: Object.keys(engine.state.meshes).length,
          values: engine.state.values,
          constraints: engine.state.constraints,
          evalStatus: engine.state.evalStatus,
        },
        editor: {
          openFiles: editor.state.openFiles.map((f) => ({
            path: f.path,
            length: f.content.length,
          })),
          activeFile: editor.state.activeFile,
          dirtyFiles: editor.state.dirtyFiles,
          cursorPosition: editor.state.cursorPosition,
        },
        selection: {
          selectedEntity: selection.state.selectedEntity,
          selectedEntities: selection.state.selectedEntities,
          anchorEntity: selection.state.anchorEntity,
          hoveredEntity: selection.state.hoveredEntity,
          highlightedParams: selection.state.highlightedParams,
        },
        claude: {
          sessionStatus: claude.state.sessionStatus,
          messageCount: claude.state.messages.length,
          currentMessageId: claude.state.currentMessageId,
        },
      };
    },

    viewport_state: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;

      const { camera, scene } = vp;
      const meshes = vp.getMeshes();

      // Compute scene bounding box
      const bounds = new Box3();
      meshes.forEach((mesh) => {
        bounds.expandByObject(mesh as Mesh);
      });

      const meshInfo: Array<Record<string, unknown>> = [];
      meshes.forEach((mesh, entityPath) => {
        const m = mesh as Mesh;
        const geom = m.geometry as BufferGeometry;
        const posAttr = geom.getAttribute('position');
        const indexAttr = geom.getIndex();
        meshInfo.push({
          entityPath,
          vertexCount: posAttr ? posAttr.count : 0,
          faceCount: indexAttr ? indexAttr.count / 3 : 0,
        });
      });

      return {
        camera: {
          position: { x: camera.position.x, y: camera.position.y, z: camera.position.z },
          rotation: { x: camera.rotation.x, y: camera.rotation.y, z: camera.rotation.z },
          fov: camera.fov,
          near: camera.near,
          far: camera.far,
        },
        meshCount: meshes.size,
        meshInfo,
        selectedEntity: ctx.stores.selection.state.selectedEntity,
        selectedEntities: ctx.stores.selection.state.selectedEntities,
        sceneBounds: bounds.isEmpty()
          ? null
          : {
              min: { x: bounds.min.x, y: bounds.min.y, z: bounds.min.z },
              max: { x: bounds.max.x, y: bounds.max.y, z: bounds.max.z },
            },
      };
    },

    screenshot: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;

      const { renderer, scene, camera } = vp;
      // Force a render to ensure the canvas has current content
      renderer.render(scene, camera);
      const dataUrl = renderer.domElement.toDataURL('image/png');
      // Canvas path: no size guard — the IPC limit is html-to-image-specific (SVG foreignObject).
      return { data: dataUrl };
    },

    screenshot_window: async (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;

      const { renderer, scene, camera } = vp;
      renderer.render(scene, camera);
      const dataUrl = await toPng(document.documentElement, { cacheBust: true });
      if (dataUrl.length > MAX_SCREENSHOT_CHARS)
        return { error: 'screenshot too large', size: dataUrl.length, limit: MAX_SCREENSHOT_CHARS };
      return { data: dataUrl };
    },

    editor_content: () => {
      const { editor } = ctx.stores;
      const activeFile = editor.state.activeFile;
      const file = activeFile
        ? editor.state.openFiles.find((f) => f.path === activeFile)
        : undefined;

      return {
        activeFile,
        content: file?.content ?? null,
        cursorPosition: editor.state.cursorPosition,
        activeFileOutOfSyncWithDisk: activeFile !== null && activeFile !== undefined
          ? editor.state.externallyChanged.includes(activeFile)
          : false,
        openFiles: editor.state.openFiles.map((f) => ({
          path: f.path,
          length: f.content.length,
          dirty: editor.state.dirtyFiles.includes(f.path),
          externallyChanged: editor.state.externallyChanged.includes(f.path),
        })),
      };
    },

    dom_query: (params) => {
      const testId = params.testId as string;
      if (!testId) return { error: 'testId is required' };

      const el = document.querySelector(`[data-testid="${CSS.escape(testId)}"]`);
      if (!el) return { exists: false };

      const rect = (el as HTMLElement).getBoundingClientRect();
      const style = window.getComputedStyle(el);
      return {
        exists: true,
        visible: style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0,
        text: (el as HTMLElement).innerText?.slice(0, 500) ?? '',
        tagName: el.tagName.toLowerCase(),
        bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      };
    },

    list_elements: () => {
      const elements = document.querySelectorAll('[data-testid]');
      const result: Array<Record<string, unknown>> = [];
      elements.forEach((el) => {
        const rect = (el as HTMLElement).getBoundingClientRect();
        const style = window.getComputedStyle(el);
        result.push({
          testId: el.getAttribute('data-testid'),
          tagName: el.tagName.toLowerCase(),
          visible: style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0,
          bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
        });
      });
      return { elements: result };
    },

    // --- DOM/style/layout/window inspection tools (R1, task-4296) ---

    query_selector: (params) => {
      const r = resolveElement(params);
      if ('error' in r) return { error: r.error };
      if (!r.el) return { exists: false };
      return { exists: true, ...describeElement(r.el as HTMLElement) };
    },

    query_selector_all: (params) => {
      const selector = params.selector as string;
      if (!selector) return { error: 'selector is required' };
      let nodes: NodeListOf<Element>;
      try {
        nodes = document.querySelectorAll(selector);
      } catch (e) {
        return { error: (e as Error).message };
      }
      const MAX = 200;
      const all = Array.from(nodes);
      const truncated = all.length > MAX;
      const elements = all.slice(0, MAX).map((el) => describeElement(el as HTMLElement));
      return { count: all.length, elements, truncated };
    },

    get_layout_metrics: (params) => {
      const r = resolveElement(params);
      if ('error' in r) return { error: r.error };
      if (!r.el) return { exists: false };
      const h = r.el as HTMLElement;
      const rect = h.getBoundingClientRect();
      return {
        exists: true,
        bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
        scroll: { top: h.scrollTop, left: h.scrollLeft, width: h.scrollWidth, height: h.scrollHeight },
        client: { width: h.clientWidth, height: h.clientHeight },
        overflow: {
          horizontal: h.scrollWidth > h.clientWidth,
          vertical: h.scrollHeight > h.clientHeight,
        },
      };
    },

    get_computed_style: (params) => {
      const r = resolveElement(params);
      if ('error' in r) return { error: r.error };
      if (!r.el) return { exists: false };
      const cs = window.getComputedStyle(r.el);
      const props: string[] =
        Array.isArray(params.properties) && (params.properties as unknown[]).length > 0
          ? (params.properties as string[])
          : [...CURATED_STYLE_PROPS];
      const style: Record<string, string> = {};
      for (const prop of props) {
        style[prop] = (cs as unknown as Record<string, string>)[prop] ?? '';
      }
      return { exists: true, style };
    },

    active_element: () => {
      const el = document.activeElement;
      if (!el) return { tagName: 'body', testId: null, role: null };
      return {
        tagName: el.tagName.toLowerCase(),
        testId: el.getAttribute('data-testid'),
        role: el.getAttribute('role'),
      };
    },

    get_window_state: () => ({
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      screenX: window.screenX,
      screenY: window.screenY,
      devicePixelRatio: window.devicePixelRatio,
      focused: document.hasFocus(),
    }),

    // --- App-chrome commands (frontend-mediated, C1) ---

    open_menu: (params) => {
      const name = params.name as string;
      if (!name) return { error: 'name is required' };

      // Menu names are simple lowercase identifiers — no CSS-escaping needed,
      // and CSS.escape is absent in jsdom (unit-test environment).
      const el = document.querySelector(`[data-testid="menu-trigger-${name}"]`);
      if (!el) return { error: `menu trigger not found: ${name}` };

      // Idempotency: if the requested menu is already open, skip the click.
      // toggleMenu would close it on a second click — we must not do that.
      const current = ctx.menuBar?.openMenu?.() ?? null;
      if (current !== name) {
        (el as HTMLElement).click();
      }

      return { ok: true, open: ctx.menuBar?.openMenu?.() ?? name };
    },

    menu_state: () => {
      const open = ctx.menuBar?.openMenu?.() ?? null;
      const items: Array<{ testId: string | null; label: string; enabled: boolean }> = [];
      document.querySelectorAll('[role="menuitem"]').forEach((el) => {
        const btn = el as HTMLButtonElement;
        // Target the un-classed label span explicitly rather than the first span
        // by position — the shortcut span always carries a CSS-module class, so
        // span:not([class]) reliably reaches the label regardless of DOM ordering.
        const label =
          btn.querySelector('span:not([class])')?.textContent?.trim() ??
          btn.innerText?.trim() ??
          '';
        items.push({
          testId: btn.getAttribute('data-testid'),
          label,
          enabled: !btn.disabled,
        });
      });
      return { open, items };
    },

    // Advance focus to the next focusable element in document order.
    // Synthetic Tab keydown is untrusted (isTrusted=false) and never moves
    // focus in a WebView or jsdom; focus is driven programmatically instead.
    // Positive-tabindex WHATWG priority ordering is not replicated (document
    // order only) — an accepted, documented limitation for app-chrome use.
    press_tab: () => {
      const list = collectTabbables();
      if (list.length === 0) return { active_element: null };
      const idx = list.indexOf(document.activeElement as HTMLElement);
      const next = list[(idx + 1) % list.length];
      next.focus();
      return { active_element: describeActive(document.activeElement as HTMLElement) };
    },

    tab_order: () => ({ order: collectTabbables().map(describeActive) }),

    // --- Write commands (frontend-mediated) ---

    click_element: (params) => {
      const testId = params.testId as string;
      if (!testId) return { error: 'testId is required' };

      const el = document.querySelector(`[data-testid="${CSS.escape(testId)}"]`);
      if (!el) return { error: `element with data-testid="${testId}" not found` };

      (el as HTMLElement).click();
      return { ok: true };
    },

    type_in_editor: (params) => {
      const content = params.content as string;
      if (content === undefined) return { error: 'content is required' };

      const view = ctx.editorView;
      if (!view) return { error: 'editor view not ready' };

      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: content },
      });
      return { ok: true };
    },

    keyboard: (params) => {
      const key = params.key as string;
      if (!key) return { error: 'key is required' };

      const event = new KeyboardEvent('keydown', {
        key,
        ctrlKey: !!params.ctrl,
        shiftKey: !!params.shift,
        altKey: !!params.alt,
        metaKey: !!params.meta,
        bubbles: true,
      });
      (document.activeElement ?? document.body).dispatchEvent(event);
      return { ok: true };
    },

    select_entity: (params) => {
      const entityPath = (params.entityPath as string) ?? null;
      ctx.stores.selection.selectEntity(entityPath);
      return { ok: true };
    },

    clear_selection: () => {
      // clearSelection is exposed on the store if available, else fall back to selectEntity(null)
      const sel = ctx.stores.selection as any;
      if (typeof sel.clearSelection === 'function') {
        sel.clearSelection();
      } else {
        ctx.stores.selection.selectEntity(null);
      }
      return { ok: true };
    },

    toggle_select: (params) => {
      const entityPath = params.entityPath as string;
      if (!entityPath) return { error: 'entityPath is required' };
      const sel = ctx.stores.selection as any;
      if (typeof sel.toggleSelect === 'function') {
        sel.toggleSelect(entityPath);
      } else {
        ctx.stores.selection.selectEntity(entityPath);
      }
      return { ok: true };
    },

    fit_to_view: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      picked.viewport.fitToView();
      return { ok: true };
    },

    set_camera: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;

      const { position, target, up, zoom } = params as {
        position: unknown;
        target: unknown;
        up: unknown;
        zoom: unknown;
      };

      if (!validVec3(position)) {
        return { error: 'position must be a 3-element array of finite numbers' };
      }
      if (!validVec3(target)) {
        return { error: 'target must be a 3-element array of finite numbers' };
      }
      if (up !== undefined && !validVec3(up)) {
        return { error: 'up must be a 3-element array of finite numbers' };
      }
      if (zoom !== undefined && !(typeof zoom === 'number' && Number.isFinite(zoom) && zoom > 0)) {
        return { error: 'zoom must be a positive finite number' };
      }

      const { camera, scene, renderer, controls } = vp;

      // Apply pose — set up before lookAt/controls.update so orientation is correct
      camera.position.set(...position);
      if (up !== undefined) camera.up.set(...up);
      if (controls) {
        controls.target.set(...target);
      } else {
        // No OrbitControls — orient camera directly toward target so the contract
        // "same input → same camera frame" holds even without controls attached.
        camera.lookAt(target[0], target[1], target[2]);
      }
      if (zoom !== undefined) camera.zoom = zoom;

      // Update matrices and render — updateMatrixWorld after controls.update() so it
      // reflects the post-controls transform (controls.update() repositions the camera).
      camera.updateProjectionMatrix();
      if (controls) controls.update();
      camera.updateMatrixWorld();
      renderer.render(scene, camera);

      // Build the full applied pose — snapshot camera state for omitted params
      const appliedUp = up ?? ([camera.up.x, camera.up.y, camera.up.z] as [number, number, number]);
      const appliedZoom = zoom ?? (camera.zoom ?? 1);

      return {
        ok: true,
        applied: { position, target, up: appliedUp, zoom: appliedZoom },
      };
    },

    set_test_mode: (params) => {
      const enabled = !!params.enabled;
      setTestMode(enabled);
      if (enabled) {
        document.documentElement.dataset.testMode = 'true';
      } else {
        delete document.documentElement.dataset.testMode;
      }
      // test-mode only affects CSS (animations/transitions via the global
      // data-test-mode rule in global.css). There is no Three.js scene-graph
      // subscriber, so a WebGL re-render here would not change what a
      // follow-up screenshot captures and is therefore omitted.
      return { ok: true, test_mode: enabled };
    },

    open_file: (params) => {
      const path = params.path as string;
      const content = params.content as string;
      if (!path || content === undefined) return { error: 'path and content are required' };

      const { editor, engine, viewState } = ctx.stores;
      editor.openFile({ path, content });

      // If guiState was provided, init the engine store (meshes, values, constraints)
      // then reset visibility to the post-restart baseline so freshly-loaded meshes render.
      const rawGuiState = params.guiState as RawGuiState | undefined;
      if (rawGuiState) {
        const guiState = convertRawGuiState(rawGuiState);
        engine.initFromState(guiState);
        viewState.resetToDefaultView();
      }

      return { ok: true, path };
    },

    wait_for_idle: async (params) => {
      const timeoutMs =
        typeof params.timeout_ms === 'number' && params.timeout_ms > 0
          ? params.timeout_ms
          : 30000;
      const start = performance.now();
      // Poll evalStatus.phase at ~60 Hz until the engine settles.
      while (true) {
        const phase = ctx.stores.engine.state.evalStatus.phase;
        if (phase === 'idle') break;
        // Terminal non-idle phase (e.g. 'error'): return immediately so the
        // harness can distinguish a stuck engine from one that finished with errors.
        if (phase !== 'evaluating') {
          return { error: 'engine_phase', phase };
        }
        if (performance.now() - start >= timeoutMs) {
          return { error: 'timeout' };
        }
        await new Promise((r) => setTimeout(r, 16));
      }
      // Await one requestAnimationFrame tick so Solid has flushed its render pass
      // and the renderer has drawn the updated frame to the canvas.
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      return { ok: true, idle_after_ms: Math.round(performance.now() - start) };
    },
  };
}

interface DebugRequest {
  id: number;
  command: string;
  params: Record<string, unknown>;
}

export async function initDebugBridge(stores: DebugStores): Promise<() => void> {
  const ctx: ReifyDebugContext = { stores, testMode };
  window.__REIFY_DEBUG__ = ctx;

  const handlers = buildHandlers(ctx);

  const unlisten = await listen<DebugRequest>('debug-request', async (event) => {
    const { id, command, params } = event.payload;
    let result: unknown;

    try {
      const handler = handlers[command];
      if (!handler) {
        result = { error: `unknown command: ${command}` };
      } else {
        result = await handler(params ?? {});
      }
    } catch (e) {
      result = { error: e instanceof Error ? e.message : String(e) };
    }

    try {
      await invoke('debug_response', { id, result: JSON.stringify(result) });
    } catch (e) {
      console.error(`[debug-bridge] failed to send response for ${command}:`, e);
    }
  });

  console.info('[debug-bridge] initialized');
  return unlisten;
}
