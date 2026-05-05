// Type declarations for the debug bridge.

import type { Scene, PerspectiveCamera, WebGLRenderer } from 'three';
import type { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import type { EditorView } from '@codemirror/view';
import type { Accessor } from 'solid-js';
import type { FileData, GuiState } from '../types';

/** Store references captured at App init time. */
export interface DebugStores {
  engine: {
    state: {
      meshes: Record<string, unknown>;
      values: Record<string, unknown>;
      constraints: Record<string, unknown>;
      evalStatus: { phase: string; progress?: number | null };
    };
    initFromState: (guiState: GuiState) => void;
  };
  editor: {
    state: {
      openFiles: Array<{ path: string; content: string }>;
      activeFile: string | null;
      dirtyFiles: string[];
      cursorPosition: { line: number; column: number } | null;
    };
    openFile: (file: FileData) => void;
  };
  selection: {
    state: {
      selectedEntity: string | null;
      selectedEntities: string[];
      anchorEntity: string | null;
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
  /** Reactive accessor — true when test-mode is enabled (animations frozen). */
  testMode?: Accessor<boolean>;
}

declare global {
  interface Window {
    __REIFY_DEBUG__?: ReifyDebugContext;
  }
}
