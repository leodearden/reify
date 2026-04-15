// Type declarations for the debug bridge.

import type { Scene, PerspectiveCamera, WebGLRenderer } from 'three';
import type { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import type { EditorView } from '@codemirror/view';

/** Store references captured at App init time. */
export interface DebugStores {
  engine: {
    state: {
      meshes: Record<string, unknown>;
      values: Record<string, unknown>;
      constraints: Record<string, unknown>;
      evalStatus: { phase: string; progress?: number | null };
    };
  };
  editor: {
    state: {
      openFiles: Array<{ path: string; content: string }>;
      activeFile: string | null;
      dirtyFiles: string[];
      cursorPosition: { line: number; column: number } | null;
    };
  };
  selection: {
    state: {
      selectedEntity: string | null;
      hoveredEntity: string | null;
      highlightedParams: string[];
    };
    selectEntity: (path: string | null) => void;
    hoverEntity: (path: string | null) => void;
  };
  claude: {
    state: {
      messages: unknown[];
      sessionStatus: string;
      currentMessageId: string | null;
    };
  };
}

/** Three.js viewport references set by Viewport.tsx onMount. */
export interface DebugViewport {
  scene: Scene;
  camera: PerspectiveCamera;
  renderer: WebGLRenderer;
  getMeshes: () => Map<string, unknown>;
  getGhostMeshes: () => Map<string, unknown>;
  fitToView: () => void;
  flyToEntity: (entityPath: string) => void;
}

/** The window.__REIFY_DEBUG__ global shape. */
export interface ReifyDebugContext {
  stores: DebugStores;
  viewport?: DebugViewport;
  editorView?: EditorView;
}

declare global {
  interface Window {
    __REIFY_DEBUG__?: ReifyDebugContext;
  }
}
