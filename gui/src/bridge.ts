/**
 * Typed Tauri IPC bridge layer.
 * Wraps @tauri-apps/api invoke() and listen() with typed functions.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  GuiState,
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  SourceLocation,
  FileData,
} from './types';

// ── Commands (invoke wrappers) ──────────────────────────────────────

/** Fetch the full initial GUI state from the backend. */
export async function getInitialState(): Promise<GuiState> {
  return invoke<GuiState>('get_initial_state');
}

/** Set a parameter value by cell ID. */
export async function setParameter(cellId: string, value: string): Promise<void> {
  return invoke('set_parameter', { cellId, value });
}

/** Update source file content. */
export async function updateSource(path: string, content: string): Promise<void> {
  return invoke('update_source', { path, content });
}

/** Save a file to disk. */
export async function saveFile(path: string): Promise<void> {
  return invoke('save_file', { path });
}

/** Open a file from disk. */
export async function openFile(path: string): Promise<FileData> {
  return invoke<FileData>('open_file', { path });
}

/** Export geometry to a file (e.g., STEP format). */
export async function exportGeometry(entityPath: string, outputPath: string): Promise<void> {
  return invoke('export_geometry', { entityPath, outputPath });
}

/** Get the source location for an entity. */
export async function getSourceLocation(entityPath: string): Promise<SourceLocation> {
  return invoke<SourceLocation>('get_source_location', { entityPath });
}

/** Focus the viewport on an entity. */
export async function focusEntity(entityPath: string): Promise<void> {
  return invoke('focus_entity', { entityPath });
}

/** Send an LSP request to the backend. */
export async function lspRequest(method: string, params: unknown): Promise<unknown> {
  return invoke('lsp_request', { method, params });
}

// ── Event listeners (listen wrappers) ───────────────────────────────

/** Subscribe to mesh update events. */
export async function onMeshUpdate(
  callback: (data: MeshData) => void,
): Promise<UnlistenFn> {
  return listen<MeshData>('mesh-update', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to value update events. */
export async function onValueUpdate(
  callback: (data: ValueData) => void,
): Promise<UnlistenFn> {
  return listen<ValueData>('value-update', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to constraint update events. */
export async function onConstraintUpdate(
  callback: (data: ConstraintData) => void,
): Promise<UnlistenFn> {
  return listen<ConstraintData>('constraint-update', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to evaluation status events. */
export async function onEvaluationStatus(
  callback: (data: EvaluationStatus) => void,
): Promise<UnlistenFn> {
  return listen<EvaluationStatus>('evaluation-status', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to diagnostic events. */
export async function onDiagnostics(
  callback: (data: unknown) => void,
): Promise<UnlistenFn> {
  return listen('diagnostics', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to file change events. */
export async function onFileChanged(
  callback: (data: FileData) => void,
): Promise<UnlistenFn> {
  return listen<FileData>('file-changed', (event) => {
    callback(event.payload);
  });
}
