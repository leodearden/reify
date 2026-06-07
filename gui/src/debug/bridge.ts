// JS Debug Bridge — listens for debug-request events from the Rust backend,
// dispatches commands, and returns results via the debug_response Tauri command.

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import type { DebugStores, DebugViewport, ReifyDebugContext } from './types';
import { convertRawGuiState } from '../types';
import type { RawGuiState, DiagnosticInfo } from '../types';
import { Box3, Vector3 } from 'three';
import type { Mesh, BufferGeometry } from 'three';
import { testMode, setTestMode } from './testMode';
import { getConsoleErrors, clearConsoleErrors } from './consoleErrors';
import { toPng } from 'html-to-image';
import { createLspClient, extractHoverMarkdown } from '../editor/lspClient';
import { pathToUri } from '../utils/pathUtils';
import {
  DEFAULT_EDITOR_WIDTH,
  DEFAULT_SIDE_WIDTH,
  DEFAULT_DESIGN_TREE_HEIGHT,
  DEFAULT_PROPERTY_HEIGHT,
  DEFAULT_CONSTRAINT_HEIGHT,
} from '../stores/layoutStore';

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

/** Returns true iff n is a finite number (not NaN, not ±Infinity). */
function isFiniteNumber(n: unknown): n is number {
  return typeof n === 'number' && Number.isFinite(n);
}

/** Returns true iff v is a plain object with finite x and y. */
function validXY(v: unknown): v is { x: number; y: number } {
  if (v === null || typeof v !== 'object') return false;
  const o = v as Record<string, unknown>;
  return isFiniteNumber(o.x) && isFiniteNumber(o.y);
}

/**
 * Dispatch a synthetic pointer/mouse event on `target` carrying clientX/clientY.
 *
 * Uses PointerEvent when available (real browser), falls back to MouseEvent in
 * jsdom (which lacks the PointerEvent constructor). The event TYPE string —
 * not the constructor class — determines which listeners fire, so a MouseEvent
 * dispatched as 'pointerdown' still triggers pointerdown listeners (debugContract
 * .test.ts:340 / selection.test.ts pattern). Both PointerEvent and MouseEvent
 * accept clientX/clientY in their init dict, so coordinates propagate correctly.
 */
function dispatchPointer(target: Element, type: string, x: number, y: number): void {
  const init = { clientX: x, clientY: y, bubbles: true, cancelable: true };
  const evt: Event = typeof PointerEvent === 'function'
    ? new PointerEvent(type, init)
    : new MouseEvent(type, init);
  target.dispatchEvent(evt);
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

// Helper for ui_outline: returns true if el or any ancestor up to <html> is hidden.
// CSS `display` is NOT an inherited property, so getComputedStyle(el).display==='none'
// only catches the element itself — not elements inside a hidden parent container
// (e.g. collapsed panels, closed modals, hidden tabs). Walk the ancestor chain so that
// subtrees whose parent has display:none or visibility:hidden are correctly excluded.
// (visibility IS inherited, so the child check would already catch it, but walking
// ancestors handles both uniformly and is the correct render-tree-presence criterion.)
function isEffectivelyHidden(el: Element): boolean {
  let node: Element | null = el;
  while (node && node !== document.documentElement) {
    const style = window.getComputedStyle(node);
    if (style.display === 'none' || style.visibility === 'hidden') return true;
    node = node.parentElement;
  }
  return false;
}

// Reshape a DiagnosticInfo into the get_diagnostics wire format.
// Groups the flat line/column/end_line/end_column quad into a `range` object
// matching PRD §3, with no field loss.
function shapeDiagnostic(d: DiagnosticInfo) {
  return {
    severity: d.severity,
    message: d.message,
    code: d.code,
    file_path: d.file_path,
    range: { line: d.line, column: d.column, end_line: d.end_line, end_column: d.end_column },
  };
}

/**
 * Returns true iff the element is visible in the render tree.
 * Reuses the existing isEffectivelyHidden() ancestor walk (so collapsed/hidden
 * panels count as not-visible) plus the rect.width>0 convention shared with
 * describeElement/dom_query/list_elements.
 */
function isElementVisible(el: Element): boolean {
  return !isEffectivelyHidden(el) && (el as HTMLElement).getBoundingClientRect().width > 0;
}

/**
 * Poll predicate at ~60Hz until it returns true or the deadline passes.
 * No final requestAnimationFrame tick (DOM/store predicates are satisfied
 * by current state and need no paint flush; avoids rAF stubbing in tests).
 */
async function pollUntil(
  predicate: () => boolean,
  timeoutMs: number,
): Promise<{ ok: true; waited_ms: number } | { error: 'timeout' }> {
  const start = performance.now();
  while (true) {
    if (predicate()) {
      return { ok: true, waited_ms: Math.round(performance.now() - start) };
    }
    if (performance.now() - start >= timeoutMs) {
      return { error: 'timeout' };
    }
    await new Promise((r) => setTimeout(r, 16));
  }
}

/**
 * Build a selector predicate for wait_for_selector / the selector arm of wait_for.
 * Resolves el = document.querySelector(`[data-testid="${CSS.escape(testId)}"]`).
 * 'visible': el exists AND isElementVisible AND (text===undefined OR textContent.trim()===text)
 * 'gone':    el===null OR !isElementVisible(el)
 */
function buildSelectorPredicate(opts: {
  testId: string;
  state: 'visible' | 'gone';
  text?: string;
}): () => boolean {
  const { testId, state, text } = opts;
  // CSS.escape is not available in all environments (e.g. jsdom); fall back to
  // a minimal escape that handles the most common testId characters safely.
  const escaped = typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
    ? CSS.escape(testId)
    : testId.replace(/["\\]/g, '\\$&');
  const sel = `[data-testid="${escaped}"]`;
  return () => {
    const el = document.querySelector(sel);
    if (state === 'gone') {
      return el === null || !isElementVisible(el);
    }
    // state === 'visible'
    if (!el || !isElementVisible(el)) return false;
    if (text !== undefined) {
      return (el as HTMLElement).textContent?.trim() === text;
    }
    return true;
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

export function buildHandlers(ctx: ReifyDebugContext): Record<string, CommandHandler> {
  /**
   * C2 tree-node driver — shared between expand_tree_node and collapse_tree_node.
   * Reads the current expanded state from the registered panel accessor,
   * clicks the real DOM toggle control if the state needs to change (idempotent),
   * then re-reads the state post-click for the truthful return value.
   */
  function driveTreeNode(params: Record<string, unknown>, wantExpanded: boolean): unknown {
    const path = params.path as string | undefined;
    if (!path) return { error: 'path is required' };

    const panelParam = params.panel ?? 'design';
    if (panelParam !== 'design' && panelParam !== 'constraint') {
      return { error: `unknown panel '${String(panelParam)}'; expected 'design' or 'constraint'` };
    }

    let accessor: (() => Set<string>) | undefined;
    let testid: string;

    if (panelParam === 'design') {
      if (!ctx.designTree) return { error: 'design tree not registered' };
      accessor = ctx.designTree.expanded;
      testid = `chevron-${path}`;
    } else {
      if (!ctx.constraintPanel) return { error: 'constraint tree not registered' };
      accessor = ctx.constraintPanel.expandedNodes;
      // NOTE: For the constraint panel, clicking `constraint-row-${path}` has a side
      // effect beyond toggling expansion: the row's onClick also fires
      // props.onConstraintSelect (a selection update), so calling expand_tree_node or
      // collapse_tree_node on a constraint node will also change the current selection.
      // Additionally, the toggle only fires when isExpandable(status) is true for the
      // node — clicking a non-expandable constraint row performs no expansion change
      // but still triggers selection. In that scenario the tool returns
      // { ok:true, expanded:false } regardless of wantExpanded because the accessor
      // re-read post-click reflects the real signal state. The caller can detect this
      // no-op case by checking whether expanded === wantExpanded in the response.
      testid = `constraint-row-${path}`;
    }

    const expandedNow = accessor().has(path);
    if (expandedNow !== wantExpanded) {
      // CSS.escape is not available in jsdom — use the same fallback as buildSelectorPredicate.
      const escaped = typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
        ? CSS.escape(testid)
        : testid.replace(/["\\]/g, '\\$&');
      const el = document.querySelector(`[data-testid="${escaped}"]`);
      if (!el) return { error: `tree node control not found: ${path}` };
      (el as HTMLElement).click();
    }

    // Re-read post-click — reports the real signal truth, not the pre-click assumption.
    // For the constraint panel, if the node is non-expandable the click leaves the
    // accessor state unchanged, so expanded may equal !wantExpanded (a silent no-op).
    return { ok: true, path, expanded: accessor().has(path) };
  }

  // One lspClient instance shared across all three F2 probe handlers; created
  // once per buildHandlers() call and closed over by hover_at/completion_at/
  // definition_at.  createLspClient() returns a plain object, so hoisting is
  // cost-free — this makes the comment below accurate.
  const lsp = createLspClient();

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

    // --- R2: get_diagnostics (frontend-mediated, reads engineStore) ---

    get_diagnostics: () => {
      const { engine } = ctx.stores;
      const compile = (engine.state.compileDiagnostics ?? []).map(shapeDiagnostic);
      const tessellation = (engine.state.tessellationDiagnostics ?? []).map(shapeDiagnostic);
      return {
        compile,
        tessellation,
        compileCount: compile.length,
        tessellationCount: tessellation.length,
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

      // The editorStore snapshot (file?.content) is stale-by-design on every
      // keystroke — Editor.tsx's docChanged handler deliberately never calls
      // updateFileContent (the "anti-loop invariant", Editor.tsx:493-497) so
      // that typing does not re-fire the store→view sync and compile-diagnostics
      // effects on each keystroke.  The live buffer lives on ctx.editorView,
      // the same handle that type_in_editor reads (bridge.ts:509).
      // Guard: substitute live content only when an active file is open AND
      // the EditorView is present; otherwise fall back to the store snapshot.
      // When there is no active file we must NOT use editorView (it holds ''
      // for the untitled buffer), so content stays null.
      const liveContent = activeFile && ctx.editorView
        ? ctx.editorView.state.doc.toString()
        : undefined;

      return {
        activeFile,
        content: liveContent ?? file?.content ?? null,
        // cursorPosition is intentionally kept store-derived (not read from
        // ctx.editorView.state.selection.main.head).  The store updates
        // cursorPosition on the cursor-changed transaction listener, which
        // fires on every selection change — it is not subject to the
        // anti-loop invariant that prevents calling updateFileContent on
        // typing.  A consumer mapping cursorPosition as a byte offset into
        // the live `content` field should be aware that the two values may
        // briefly diverge mid-keystroke if the cursor moves with the edit.
        cursorPosition: editor.state.cursorPosition,
        activeFileOutOfSyncWithDisk: activeFile !== null && activeFile !== undefined
          ? editor.state.externallyChanged.includes(activeFile)
          : false,
        openFiles: editor.state.openFiles.map((f) => ({
          path: f.path,
          // Reflect the live buffer length for the active entry so that
          // openFiles[active].length agrees with the top-level content field.
          // Non-active entries stay store-derived (only the active file has a
          // live EditorView handle).
          length: f.path === activeFile && liveContent !== undefined
            ? liveContent.length
            : f.content.length,
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

    // --- R2: ui_outline (frontend-mediated, reads live DOM) ---
    // Returns a flat ordered list of visible semantic elements (tagName, role,
    // data-testid, text, enabled-state). This is a pragmatic DOM APPROXIMATION —
    // NOT a true accessibility tree (deferred to tracker AX-1).
    // Visibility is determined by computed display/visibility (NOT getBoundingClientRect
    // geometry, which is all-zero under jsdom) — the correct render-tree criterion.
    ui_outline: () => {
      const MAX = 500;
      const nodes = document.querySelectorAll(
        '[data-testid], [role], button, a[href], input, select, textarea, [tabindex]',
      );
      const all = Array.from(nodes);
      const outline: Array<Record<string, unknown>> = [];
      for (const el of all) {
        if (isEffectivelyHidden(el)) continue;
        const h = el as HTMLElement & { disabled?: boolean };
        outline.push({
          tagName: el.tagName.toLowerCase(),
          role: el.getAttribute('role'),
          testId: el.getAttribute('data-testid'),
          text: ((h.innerText ?? el.textContent ?? '').slice(0, 200)),
          enabled: !h.disabled && el.getAttribute('aria-disabled') !== 'true',
        });
      }
      const truncated = outline.length > MAX;
      const sliced = outline.slice(0, MAX);
      return { outline: sliced, count: outline.length, truncated };
    },

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

    // --- I1: Synthetic pointer/scroll/focus tools (task-4299) ---
    // NOTE (contract §4 fidelity): these tools fire JS handlers (onClick,
    // onPointerDown, etc.) but do NOT apply CSS :hover/:active pseudo-classes
    // and do NOT trigger native OS hit-testing. document.elementFromPoint is
    // used for target resolution. See docs/debug-mcp-contract.md §4 for the
    // full fidelity-gaps table.

    click_at: (params) => {
      if (!validXY(params)) return { error: 'x and y must be finite numbers' };
      const { x, y } = params as { x: number; y: number };
      const el = document.elementFromPoint(x, y);
      if (!el) return { error: `no element at point (${x}, ${y})` };
      // Dispatch synthetic pointer sequence: pointerdown → pointerup → click.
      // Each event carries clientX/clientY (contract §3 coordinate convention).
      // Contract §4: fires JS handlers; CSS :hover/:active NOT applied.
      dispatchPointer(el, 'pointerdown', x, y);
      dispatchPointer(el, 'pointerup', x, y);
      dispatchPointer(el, 'click', x, y);
      return { ok: true, target: { tagName: el.tagName.toLowerCase(), testId: el.getAttribute('data-testid') } };
    },

    hover: (params) => {
      if (!validXY(params)) return { error: 'x and y must be finite numbers' };
      const { x, y } = params as { x: number; y: number };
      const el = document.elementFromPoint(x, y);
      if (!el) return { error: `no element at point (${x}, ${y})` };
      // Dispatch synthetic move events: pointermove then mousemove.
      // Contract §4: fires JS move handlers; CSS :hover pseudo-class NOT applied.
      dispatchPointer(el, 'pointermove', x, y);
      dispatchPointer(el, 'mousemove', x, y);
      return { ok: true, target: { tagName: el.tagName.toLowerCase(), testId: el.getAttribute('data-testid') } };
    },

    drag: (params) => {
      if (!validXY(params.from)) return { error: 'from must be an object with finite x and y' };
      if (!validXY(params.to)) return { error: 'to must be an object with finite x and y' };
      const from = params.from as { x: number; y: number };
      const to = params.to as { x: number; y: number };
      const elFrom = document.elementFromPoint(from.x, from.y);
      if (!elFrom) return { error: `no element at from point (${from.x}, ${from.y})` };
      // elTo falls back to elFrom when the destination resolves to null (e.g. off-canvas).
      const elTo = document.elementFromPoint(to.x, to.y) ?? elFrom;
      // Synthetic pointer-move drag: pointerdown+mousedown at from, pointermove at to,
      // pointerup+mouseup at to (all with matching clientX/clientY).
      // Contract §4: NO native HTML5 drag-and-drop — dragstart/drop are NOT fired.
      dispatchPointer(elFrom, 'pointerdown', from.x, from.y);
      dispatchPointer(elFrom, 'mousedown', from.x, from.y);
      dispatchPointer(elTo, 'pointermove', to.x, to.y);
      dispatchPointer(elTo, 'pointerup', to.x, to.y);
      dispatchPointer(elTo, 'mouseup', to.x, to.y);
      return { ok: true, from, to };
    },

    focus_element: (params) => {
      const testId = params.testId as string;
      if (!testId) return { error: 'testId is required' };
      // CSS.escape fallback for jsdom — mirrors buildSelectorPredicate (:175-177).
      const escaped = typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
        ? CSS.escape(testId)
        : testId.replace(/["\\]/g, '\\$&');
      const el = document.querySelector(`[data-testid="${escaped}"]`);
      if (!el) return { error: `element with data-testid="${testId}" not found` };
      (el as HTMLElement).focus();
      return { ok: true };
    },

    scroll: (params) => {
      if (params.target === 'editor') {
        // Editor mode: scroll the CodeMirror EditorView's scrollDOM.
        const view = ctx.editorView;
        if (!view) return { error: 'editor view not ready' };
        const sd = view.scrollDOM;
        // Reject non-finite offsets (NaN, ±Infinity) — consistent with isFiniteNumber/validXY guards.
        if (params.top !== undefined && !isFiniteNumber(params.top)) return { error: 'top must be a finite number' };
        if (params.left !== undefined && !isFiniteNumber(params.left)) return { error: 'left must be a finite number' };
        if (isFiniteNumber(params.top)) sd.scrollTop = params.top;
        if (isFiniteNumber(params.left)) sd.scrollLeft = params.left;
        return { ok: true, scrollTop: sd.scrollTop, scrollLeft: sd.scrollLeft };
      }
      // DOM mode: scroll an element resolved by data-testid.
      const testId = params.testId as string;
      if (!testId) return { error: 'testId or target:"editor" is required' };
      const escaped = typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
        ? CSS.escape(testId)
        : testId.replace(/["\\]/g, '\\$&');
      const el = document.querySelector(`[data-testid="${escaped}"]`) as HTMLElement | null;
      if (!el) return { error: `element with data-testid="${testId}" not found` };
      // Reject non-finite offsets (NaN, ±Infinity) — consistent with isFiniteNumber/validXY guards.
      if (params.top !== undefined && !isFiniteNumber(params.top)) return { error: 'top must be a finite number' };
      if (params.left !== undefined && !isFiniteNumber(params.left)) return { error: 'left must be a finite number' };
      if (isFiniteNumber(params.top)) el.scrollTop = params.top;
      if (isFiniteNumber(params.left)) el.scrollLeft = params.left;
      return { ok: true, scrollTop: el.scrollTop, scrollLeft: el.scrollLeft };
    },

    select_entity: (params) => {
      const entityPath = (params.entityPath as string) ?? null;
      ctx.stores.selection.selectEntity(entityPath);
      return { ok: true };
    },

    clear_selection: () => {
      ctx.stores.selection.clearSelection();
      return { ok: true };
    },

    toggle_select: (params) => {
      const entityPath = params.entityPath as string;
      if (!entityPath) return { error: 'entityPath is required' };
      ctx.stores.selection.toggleSelect(entityPath);
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

    wait_for: async (params) => {
      const predicate = params.predicate;
      if (predicate === null || typeof predicate !== 'object' || Array.isArray(predicate)) {
        return { error: 'predicate {kind} required' };
      }
      const pred = predicate as Record<string, unknown>;
      const kind = pred.kind;
      let timeoutMs = 5000;
      if (params.timeout_ms !== undefined) {
        if (
          typeof params.timeout_ms !== 'number' ||
          !Number.isInteger(params.timeout_ms) ||
          params.timeout_ms <= 0
        ) {
          return { error: 'timeout_ms must be a positive integer' };
        }
        timeoutMs = params.timeout_ms;
      }

      if (kind === 'selector') {
        const testId = pred.testId;
        if (typeof testId !== 'string' || testId === '') {
          return { error: 'predicate.testId is required for selector kind' };
        }
        const state = (pred.state ?? 'visible') as 'visible' | 'gone';
        const text = typeof pred.text === 'string' ? pred.text : undefined;
        return pollUntil(buildSelectorPredicate({ testId, state, text }), timeoutMs);
      }

      if (kind === 'store') {
        const path = pred.path;
        const equals = pred.equals;
        if (typeof path !== 'string' || path === '') {
          return { error: 'predicate.path is required for store kind' };
        }
        // equals is required: Object.is uses primitive-identity (object/array values
        // can never match), and an omitted equals silently matches any undefined path.
        if (equals === undefined) {
          return { error: 'predicate.equals is required for store kind' };
        }
        // Guard against unknown roots so a typo'd path surfaces a clear error
        // instead of silently timing out. layout.state is included; viewState has no .state.
        const knownStoreRoots = new Set(['engine', 'editor', 'selection', 'claude', 'layout']);
        const rootKey = path.split('.')[0];
        if (!knownStoreRoots.has(rootKey)) {
          return {
            error: `unknown store root '${rootKey}'; addressable roots: engine, editor, selection, claude, layout`,
          };
        }
        // Build a snapshot object and walk a dotted path against it.
        // Snapshot is re-evaluated each poll tick via closure.
        const getByPath = (root: Record<string, unknown>, dotted: string): unknown => {
          return dotted.split('.').reduce<unknown>((acc, key) => {
            if (acc !== null && typeof acc === 'object') {
              return (acc as Record<string, unknown>)[key];
            }
            return undefined;
          }, root);
        };
        return pollUntil(() => {
          const snapshot: Record<string, unknown> = {
            engine: ctx.stores.engine.state,
            editor: ctx.stores.editor.state,
            selection: ctx.stores.selection.state,
            claude: ctx.stores.claude.state,
            layout: ctx.stores.layout.state,
          };
          return Object.is(getByPath(snapshot, path), equals);
        }, timeoutMs);
      }

      return { error: `unknown predicate kind: ${String(kind)}` };
    },

    wait_for_selector: async (params) => {
      const testId = params.testId;
      if (typeof testId !== 'string' || testId === '') {
        return { error: 'testId is required' };
      }
      const stateParam = params.state ?? 'visible';
      if (stateParam !== 'visible' && stateParam !== 'gone') {
        return { error: 'state must be visible|gone' };
      }
      const text = typeof params.text === 'string' ? params.text : undefined;
      let timeoutMs = 5000;
      if (params.timeout_ms !== undefined) {
        if (
          typeof params.timeout_ms !== 'number' ||
          !Number.isInteger(params.timeout_ms) ||
          params.timeout_ms <= 0
        ) {
          return { error: 'timeout_ms must be a positive integer' };
        }
        timeoutMs = params.timeout_ms;
      }
      return pollUntil(
        buildSelectorPredicate({ testId, state: stateParam as 'visible' | 'gone', text }),
        timeoutMs,
      );
    },

    list_console_errors: (params) => {
      const errors = getConsoleErrors();
      if (params.clear === true) {
        clearConsoleErrors();
      }
      return { errors, count: errors.length };
    },

    // --- C2: layout-control tools (task-4302) ---

    resize_panes: (params) => {
      const DIMS = [
        ['editorWidth',      'setEditorWidth'],
        ['sideWidth',        'setSideWidth'],
        ['designTreeHeight', 'setDesignTreeHeight'],
        ['propertyHeight',   'setPropertyHeight'],
        ['constraintHeight', 'setConstraintHeight'],
      ] as const;

      // Validate first pass — reject any invalid value before applying anything.
      let anyProvided = false;
      for (const [dim] of DIMS) {
        const raw = params[dim];
        if (raw === undefined) continue;
        anyProvided = true;
        if (typeof raw !== 'number' || !Number.isFinite(raw) || raw < 0) {
          return { error: `${dim} must be a non-negative finite number` };
        }
      }
      if (!anyProvided) return { error: 'no pane dimensions provided' };

      // Apply pass — all values validated.
      // NOTE: Values are applied as-is without clamping to the interactive splitter
      // min/max bounds (the App.tsx handleLeftResize/handleRightResize handlers enforce
      // those bounds during user drag). resize_panes is intentionally unclamped to give
      // the caller full programmatic control; the caller is responsible for staying in a
      // valid range. Setting a dimension to 0 is allowed (e.g. to collapse a pane
      // programmatically) even though the splitter UI prevents this interactively.
      const layout = ctx.stores.layout;
      for (const [dim, setter] of DIMS) {
        const raw = params[dim];
        if (raw !== undefined) {
          (layout[setter] as (v: number) => void)(raw as number);
        }
      }

      return { ok: true, layout: { ...ctx.stores.layout.state } };
    },

    set_window_size: async (params) => {
      const { width, height } = params as { width: unknown; height: unknown };
      if (typeof width !== 'number' || !Number.isFinite(width) || width <= 0) {
        return { error: 'width must be a positive finite number' };
      }
      if (typeof height !== 'number' || !Number.isFinite(height) || height <= 0) {
        return { error: 'height must be a positive finite number' };
      }
      await getCurrentWindow().setSize(new LogicalSize(width, height));
      return { ok: true, width, height };
    },

    expand_tree_node: (params) => driveTreeNode(params, true),
    collapse_tree_node: (params) => driveTreeNode(params, false),

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

    // --- F2 LSP probe tools (task-4304) ---
    // `lsp` is the single client instance hoisted before this return; see above.
    // It calls the in-process LSP via the Tauri lsp_request command, reusing its
    // response normalisation (completion list→array, null-handling).

    hover_at: async (params) => {
      const target = resolveActiveProbeTarget(ctx, params);
      if ('error' in target) return target;
      const { uri, line, col } = target;
      const hover = await lsp.hover(uri, line, col);
      if (!hover) {
        return { markdown: '', markdownLength: 0, contents: null, range: null };
      }
      const markdown = extractHoverMarkdown(hover.contents);
      return {
        markdown,
        markdownLength: markdown.length,
        contents: hover.contents,
        range: hover.range ?? null,
      };
    },

    completion_at: async (params) => {
      const target = resolveActiveProbeTarget(ctx, params);
      if ('error' in target) return target;
      const { uri, line, col } = target;
      const items = await lsp.completion(uri, line, col);
      return { items, itemCount: items.length };
    },

    definition_at: async (params) => {
      const target = resolveActiveProbeTarget(ctx, params);
      if ('error' in target) return target;
      const { uri, line, col } = target;
      const loc = await lsp.gotoDefinition(uri, line, col);
      return { uri: loc?.uri ?? null, range: loc?.range ?? null, found: loc !== null };
    },

    // --- I2: canvas-interaction tools (task-4300) ---

    /**
     * Raycast at CSS-px coords (clientX/clientY from window origin) and return the
     * first-hit entity.  QUERY ONLY — never mutates selection state.
     * Omitted x/y → canvas centre (rect.left+rect.width/2, rect.top+rect.height/2).
     * Lazy import of Raycaster/Vector2 avoids polluting the top-level three import
     * (sibling tests vi.mock('three') with only {Box3,Vector3}).
     */
    pick_entity_at: async (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;

      const rawX = params.x;
      const rawY = params.y;

      // If either coord is provided, require BOTH to be finite numbers.
      if (rawX !== undefined || rawY !== undefined) {
        if (!isFiniteNumber(rawX) || !isFiniteNumber(rawY)) {
          return { error: 'x and y must be finite numbers' };
        }
      }

      const rect = vp.renderer.domElement.getBoundingClientRect();
      // Default to canvas centre when both coords are omitted.
      const cx = isFiniteNumber(rawX) ? rawX : rect.left + rect.width / 2;
      const cy = isFiniteNumber(rawY) ? rawY : rect.top + rect.height / 2;

      // Lazy import: top-level Raycaster/Vector2 would conflict with sibling
      // test files that vi.mock('three') with only {Box3,Vector3}.
      const { Raycaster, Vector2 } = await import('three');
      const ndc = new Vector2(
        ((cx - rect.left) / rect.width) * 2 - 1,
        -((cy - rect.top) / rect.height) * 2 + 1,
      );
      const rc = new Raycaster();
      (rc as any).firstHitOnly = true;
      rc.setFromCamera(ndc, vp.camera);

      const meshes = Array.from(vp.getMeshes().values()) as import('three').Object3D[];
      const hits = rc.intersectObjects(meshes);

      if (hits.length === 0) return { hit: false };
      const h = hits[0];
      return {
        hit: true,
        entityPath: h.object.name,
        point: { x: h.point.x, y: h.point.y, z: h.point.z },
        distance: h.distance,
      };
    },

    /**
     * Rotate the camera via OrbitControls.rotateLeft/rotateUp.
     * dazimuth / delevation are in RADIANS (default 0).
     * Each public method calls controls.update() internally — no extra update() call.
     * Returns observed azimuth/polar deltas so single-tool harness can assert change.
     */
    orbit_camera: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;
      if (!vp.controls) return { error: 'controls not available' };

      const rawDaz = params.dazimuth;
      const rawDel = params.delevation;
      if (rawDaz !== undefined && !isFiniteNumber(rawDaz)) {
        return { error: 'dazimuth must be a finite number' };
      }
      if (rawDel !== undefined && !isFiniteNumber(rawDel)) {
        return { error: 'delevation must be a finite number' };
      }
      const dazimuth = isFiniteNumber(rawDaz) ? rawDaz : 0;
      const delevation = isFiniteNumber(rawDel) ? rawDel : 0;

      const az0 = vp.controls.getAzimuthalAngle();
      const po0 = vp.controls.getPolarAngle();

      // rotateLeft/rotateUp each call controls.update() internally.
      vp.controls.rotateLeft(dazimuth);
      vp.controls.rotateUp(delevation);
      vp.renderer.render(vp.scene, vp.camera);

      const az1 = vp.controls.getAzimuthalAngle();
      const po1 = vp.controls.getPolarAngle();

      return {
        ok: true,
        azimuth: az1,
        polar: po1,
        azimuthDelta: Math.abs(az1 - az0),
        polarDelta: Math.abs(po1 - po0),
        camera: { position: { x: vp.camera.position.x, y: vp.camera.position.y, z: vp.camera.position.z } },
      };
    },

    /**
     * Pan the camera via OrbitControls.pan(dx, dy).
     * dx/dy are in CSS pixels (default 0).
     * pan() calls controls.update() internally — no extra update() call.
     */
    pan_camera: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;
      if (!vp.controls) return { error: 'controls not available' };

      const rawDx = params.dx;
      const rawDy = params.dy;
      if (rawDx !== undefined && !isFiniteNumber(rawDx)) {
        return { error: 'dx must be a finite number' };
      }
      if (rawDy !== undefined && !isFiniteNumber(rawDy)) {
        return { error: 'dy must be a finite number' };
      }
      const dx = isFiniteNumber(rawDx) ? rawDx : 0;
      const dy = isFiniteNumber(rawDy) ? rawDy : 0;

      // pan() calls controls.update() internally.
      vp.controls.pan(dx, dy);
      vp.renderer.render(vp.scene, vp.camera);

      return {
        ok: true,
        target: { x: vp.controls.target.x, y: vp.controls.target.y, z: vp.controls.target.z },
        camera: { position: { x: vp.camera.position.x, y: vp.camera.position.y, z: vp.camera.position.z } },
      };
    },

    /**
     * Zoom the camera via OrbitControls.dollyIn(scale).
     * scale > 1 → farther, scale < 1 → closer; scale must be finite and > 0.
     * dollyIn(scale) multiplies orbit radius by scale (verified vs three 0.183.2
     * OrbitControls.js: update() radius *= _scale; _dollyIn _scale *= dollyScale).
     * dollyIn() calls controls.update() internally — no extra update() call.
     */
    zoom_camera: (params) => {
      const picked = pickViewport(ctx, params);
      if ('error' in picked) return picked;
      const vp = picked.viewport;
      if (!vp.controls) return { error: 'controls not available' };

      const rawScale = params.scale;
      if (!isFiniteNumber(rawScale) || rawScale <= 0) {
        return { error: 'scale must be a positive finite number' };
      }

      const d0 = vp.controls.getDistance();
      // dollyIn() calls controls.update() internally.
      vp.controls.dollyIn(rawScale);
      vp.renderer.render(vp.scene, vp.camera);
      const d1 = vp.controls.getDistance();

      return {
        ok: true,
        distance: d1,
        distanceDelta: Math.abs(d1 - d0),
        camera: { position: { x: vp.camera.position.x, y: vp.camera.position.y, z: vp.camera.position.z } },
      };
    },

    // --- F1: fixture/injection/capture tools (task-4303) ---

    inject_diagnostics: (params) => {
      const raw = params.diagnostics;
      if (!Array.isArray(raw) || raw.length === 0) {
        return { error: 'diagnostics must be a non-empty array' };
      }

      // Validate each entry has required fields (severity + message).
      for (let i = 0; i < raw.length; i++) {
        const entry = raw[i];
        if (
          entry === null ||
          typeof entry !== 'object' ||
          typeof (entry as Record<string, unknown>).severity !== 'string' ||
          typeof (entry as Record<string, unknown>).message !== 'string'
        ) {
          return { error: `diagnostics[${i}] must have severity (string) and message (string)` };
        }
      }

      // Validate source (optional).
      const sourceRaw = params.source;
      if (sourceRaw !== undefined && sourceRaw !== 'compile' && sourceRaw !== 'tessellation') {
        return { error: 'source must be "compile" or "tessellation"' };
      }
      const source = (sourceRaw as 'compile' | 'tessellation') ?? 'compile';

      // Normalize: preserve caller-provided fields; default ONLY omitted positional fields.
      // Never mutate message or severity.
      const normalized: DiagnosticInfo[] = raw.map((entry: Record<string, unknown>) => {
        const line = typeof entry.line === 'number' ? entry.line : 0;
        const column = typeof entry.column === 'number' ? entry.column : 0;
        return {
          file_path: typeof entry.file_path === 'string' ? entry.file_path : 'synthetic://inject_diagnostics',
          line,
          column,
          end_line: typeof entry.end_line === 'number' ? entry.end_line : line,
          end_column: typeof entry.end_column === 'number' ? entry.end_column : column,
          severity: entry.severity as string,
          message: entry.message as string,
          code: entry.code !== undefined ? (entry.code as string | null) : null,
        };
      });

      if (source === 'tessellation') {
        ctx.stores.engine.setTessellationDiagnostics(normalized);
      } else {
        ctx.stores.engine.setCompileDiagnostics(normalized);
      }

      return { ok: true, count: normalized.length, source };
    },

    reset_app_state: () => {
      const { engine, editor, selection, viewState, layout } = ctx.stores;

      // Snapshot open file paths BEFORE closing (closeFile mutates openFiles).
      const paths = editor.state.openFiles.map((f) => f.path);
      for (const p of paths) {
        editor.closeFile(p);
      }

      selection.clearSelection();
      viewState.resetToDefaultView();
      engine.setCompileDiagnostics([]);
      engine.setTessellationDiagnostics([]);

      layout.setEditorWidth(DEFAULT_EDITOR_WIDTH);
      layout.setSideWidth(DEFAULT_SIDE_WIDTH);
      layout.setDesignTreeHeight(DEFAULT_DESIGN_TREE_HEIGHT);
      layout.setPropertyHeight(DEFAULT_PROPERTY_HEIGHT);
      layout.setConstraintHeight(DEFAULT_CONSTRAINT_HEIGHT);

      return { ok: true };
    },

    element_screenshot: async (params) => {
      const testId = params.testId;
      if (!testId || typeof testId !== 'string') {
        return { error: 'testId is required' };
      }

      // CSS.escape may not be available in all environments (e.g. jsdom).
      const escaped =
        typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
          ? CSS.escape(testId)
          : testId.replace(/["\\]/g, '\\$&');
      const el = document.querySelector(`[data-testid="${escaped}"]`);
      if (!el) {
        return { error: `element with data-testid="${testId}" not found` };
      }

      const rect = el.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) {
        return { error: 'element has zero area' };
      }

      const dpr = window.devicePixelRatio || 1;

      // Capture the full window.
      // Pass pixelRatio explicitly so the crop math stays correct regardless of any
      // future html-to-image default change.
      const dataUrl = await toPng(document.documentElement, { cacheBust: true, pixelRatio: dpr });

      // Load the full-window image and crop to the element's DPR-scaled bounds.
      // getBoundingClientRect() returns viewport-relative coordinates; add scrollX/scrollY
      // to convert to document-origin offsets that match the full-document capture origin.
      const img = await new Promise<HTMLImageElement>((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve(image as HTMLImageElement);
        image.onerror = reject;
        image.src = dataUrl;
      });

      const canvasW = Math.round(rect.width * dpr);
      const canvasH = Math.round(rect.height * dpr);
      const canvas = document.createElement('canvas');
      canvas.width = canvasW;
      canvas.height = canvasH;
      const ctx2d = canvas.getContext('2d') as CanvasRenderingContext2D;
      ctx2d.drawImage(
        img,
        (rect.x + window.scrollX) * dpr,
        (rect.y + window.scrollY) * dpr,
        rect.width * dpr,
        rect.height * dpr,
        0,
        0,
        canvasW,
        canvasH,
      );

      const cropped = canvas.toDataURL('image/png');
      if (cropped.length > MAX_SCREENSHOT_CHARS) {
        return { error: 'screenshot too large', size: cropped.length, limit: MAX_SCREENSHOT_CHARS };
      }

      return { data: cropped };
    },
  };
}

/**
 * Validate probe params and derive the LSP document URI from the active file.
 * Returns `{ uri, line, col }` on success or `{ error }` on failure.
 *
 * Guards:
 * - ctx.stores.editor.state.activeFile must be non-null ('no active file')
 * - line and col must be present, non-negative, finite integers
 *   ('line and col must be non-negative integers')
 *
 * col maps to LSP position.character (mirrors bridge.ts convention).
 */
function resolveActiveProbeTarget(
  ctx: ReifyDebugContext,
  params: Record<string, unknown>,
): { uri: string; line: number; col: number } | { error: string } {
  const activeFile = ctx.stores.editor.state.activeFile;
  if (!activeFile) return { error: 'no active file' };

  const { line, col } = params as { line: unknown; col: unknown };
  if (
    typeof line !== 'number' ||
    !Number.isInteger(line) ||
    line < 0 ||
    typeof col !== 'number' ||
    !Number.isInteger(col) ||
    col < 0
  ) {
    return { error: 'line and col must be non-negative integers' };
  }

  return { uri: pathToUri(activeFile), line, col };
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
