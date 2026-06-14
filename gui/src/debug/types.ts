// Type declarations for the debug bridge.

import type { Scene, PerspectiveCamera, WebGLRenderer } from 'three';
import type { ViewStateStore } from '../stores/viewStateStore';
import type { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import type { EditorView } from '@codemirror/view';
import { onMount, onCleanup } from 'solid-js';
import type { Accessor } from 'solid-js';
import type { FileData, GuiState, DiagnosticInfo } from '../types';

/** Store references captured at App init time. */
export interface DebugStores {
  engine: {
    state: {
      meshes: Record<string, unknown>;
      values: Record<string, unknown>;
      constraints: Record<string, unknown>;
      evalStatus: { phase: string; progress?: number | null };
      compileDiagnostics: DiagnosticInfo[];
      tessellationDiagnostics: DiagnosticInfo[];
    };
    initFromState: (guiState: GuiState) => void;
    /** F1/inject_diagnostics + reset_app_state: inject synthetic compile diagnostics into the store. */
    setCompileDiagnostics: (diags: DiagnosticInfo[]) => void;
    /** F1/inject_diagnostics + reset_app_state: inject synthetic tessellation diagnostics into the store. */
    setTessellationDiagnostics: (diags: DiagnosticInfo[]) => void;
  };
  editor: {
    state: {
      openFiles: Array<{ path: string; content: string }>;
      activeFile: string | null;
      dirtyFiles: string[];
      externallyChanged: string[];
      cursorPosition: { line: number; column: number } | null;
    };
    openFile: (file: FileData) => void;
    /** F1/reset_app_state: close a file by path. */
    closeFile: (path: string) => void;
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
    clearSelection: () => void;
    toggleSelect: (entityPath: string) => void;
  };
  claude: {
    state: {
      messages: unknown[];
      sessionStatus: string;
      currentMessageId: string | null;
    };
  };
  viewState: ViewStateStore;
  /** Pane/splitter dimensions. C2/resize_panes owns these setters (breadcrumb from L0). */
  layout: {
    state: {
      editorWidth: number;
      sideWidth: number;
      designTreeHeight: number;
      propertyHeight: number;
      constraintHeight: number;
      /** Height of the diagnostics/problems panel in pixels (task-4404 ε). */
      problemsHeight?: number;
      /** Whether the problems panel is collapsed (task-4404 ε). */
      problemsCollapsed?: boolean;
    };
    /** Accept a plain value or a functional updater, matching the layoutStore setter signature. */
    setEditorWidth: (v: number | ((prev: number) => number)) => void;
    setSideWidth: (v: number | ((prev: number) => number)) => void;
    setDesignTreeHeight: (v: number | ((prev: number) => number)) => void;
    setPropertyHeight: (v: number | ((prev: number) => number)) => void;
    setConstraintHeight: (v: number | ((prev: number) => number)) => void;
    /** resize_panes: problems panel height setter (task-4404 ε). */
    setProblemsHeight?: (v: number | ((prev: number) => number)) => void;
    /** resize_panes: problems panel collapsed setter (task-4404 ε). */
    setProblemsCollapsed?: (v: boolean | ((prev: boolean) => boolean)) => void;
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
  /** OrbitControls instance — optional so test stubs need not construct one. */
  controls?: OrbitControls;
}

/** The window.__REIFY_DEBUG__ global shape. */
export interface ReifyDebugContext {
  stores: DebugStores;
  /** Legacy single-slot — kept for backward compat with direct-stub-injection tests. */
  viewport?: DebugViewport;
  /** Multi-viewport map keyed by viewportId. Each <Viewport> registers/unregisters here. */
  viewports?: Record<string, DebugViewport>;
  editorView?: EditorView;
  /** Reactive accessor — true when test-mode is enabled (animations frozen). */
  testMode?: Accessor<boolean>;
  /** MenuBar registration: openMenu accessor reports which menu is currently open (null = closed). */
  menuBar?: { openMenu: Accessor<string | null> };
  /** DesignTree registration: expanded accessor reports the set of expanded entity paths. */
  designTree?: { expanded: Accessor<Set<string>> };
  /** ConstraintPanel registration: expandedNodes accessor reports the set of expanded constraint node ids. */
  constraintPanel?: { expandedNodes: Accessor<Set<string>> };
}

declare global {
  interface Window {
    __REIFY_DEBUG__?: ReifyDebugContext;
  }
}

/**
 * Registers a panel accessor onto window.__REIFY_DEBUG__[key] on component mount
 * and removes it on cleanup. Gated on ctx presence so production builds
 * (no __REIFY_DEBUG__) are no-ops.
 *
 * The identity guard on cleanup prevents a late-running dismount from a prior
 * instance from evicting a freshly-mounted second instance's registration.
 */
export function registerDebugPanel<K extends Exclude<keyof ReifyDebugContext, 'stores'>>(
  key: K,
  value: NonNullable<ReifyDebugContext[K]>,
): void {
  onMount(() => {
    if (!window.__REIFY_DEBUG__) return;
    window.__REIFY_DEBUG__[key] = value as ReifyDebugContext[K];
    onCleanup(() => {
      const ctx = window.__REIFY_DEBUG__;
      if (ctx && (ctx as Partial<ReifyDebugContext>)[key] === value) {
        delete (ctx as Partial<ReifyDebugContext>)[key];
      }
    });
  });
}
