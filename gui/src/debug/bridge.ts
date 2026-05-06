// JS Debug Bridge — listens for debug-request events from the Rust backend,
// dispatches commands, and returns results via the debug_response Tauri command.

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { DebugStores, ReifyDebugContext } from './types';
import { convertRawGuiState } from '../types';
import type { RawGuiState } from '../types';
import { Box3, Vector3 } from 'three';
import type { Mesh, BufferGeometry } from 'three';
import { testMode, setTestMode } from './testMode';

type CommandHandler = (params: Record<string, unknown>) => unknown;

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

    viewport_state: () => {
      const vp = ctx.viewport;
      if (!vp) return { error: 'viewport not ready' };

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

    screenshot: () => {
      const vp = ctx.viewport;
      if (!vp) return { error: 'viewport not ready' };

      const { renderer, scene, camera } = vp;
      // Force a render to ensure the canvas has current content
      renderer.render(scene, camera);
      const dataUrl = renderer.domElement.toDataURL('image/png');
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
        openFiles: editor.state.openFiles.map((f) => ({
          path: f.path,
          length: f.content.length,
          dirty: editor.state.dirtyFiles.includes(f.path),
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

    fit_to_view: () => {
      const vp = ctx.viewport;
      if (!vp) return { error: 'viewport not ready' };
      vp.fitToView();
      return { ok: true };
    },

    set_camera: (_params) => {
      const vp = ctx.viewport;
      if (!vp) return { error: 'viewport not ready' };
      return { ok: true };
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

      const { editor, engine } = ctx.stores;
      editor.openFile({ path, content });

      // If guiState was provided, init the engine store (meshes, values, constraints)
      const rawGuiState = params.guiState as RawGuiState | undefined;
      if (rawGuiState) {
        const guiState = convertRawGuiState(rawGuiState);
        engine.initFromState(guiState);
      }

      return { ok: true, path };
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
        result = handler(params ?? {});
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
