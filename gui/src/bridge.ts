/**
 * Typed Tauri IPC bridge layer.
 * Wraps @tauri-apps/api invoke() and listen() with typed functions.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { save, open } from '@tauri-apps/plugin-dialog';
import type {
  GuiState,
  MeshData,
  RawMeshData,
  RawGuiState,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  SourceLocation,
  FileData,
} from './types';
import { convertRawMesh, convertRawGuiState } from './types';
import type { OutboundMessage } from '../sidecar/src/types';

// ── Commands (invoke wrappers) ──────────────────────────────────────

/** Fetch the full initial GUI state from the backend. Converts mesh wire data to typed arrays. */
export async function getInitialState(): Promise<GuiState> {
  const raw = await invoke<RawGuiState>('get_initial_state');
  return convertRawGuiState(raw);
}

/** Refresh the full GUI state for recovery from missed events. Semantic alias for getInitialState. */
export async function refreshFullState(): Promise<GuiState> {
  const raw = await invoke<RawGuiState>('get_initial_state');
  return convertRawGuiState(raw);
}

/** Set a parameter value by cell ID. Returns the updated GUI state for optional reconciliation. */
export async function setParameter(cellId: string, value: string): Promise<GuiState> {
  const raw = await invoke<RawGuiState>('set_parameter', { cellId, value });
  return convertRawGuiState(raw);
}

/** Update source file content. Returns the updated GUI state for optional reconciliation. */
export async function updateSource(path: string, content: string): Promise<GuiState> {
  const raw = await invoke<RawGuiState>('update_source', { path, content });
  return convertRawGuiState(raw);
}

/** Save a file to disk. */
export async function saveFile(path: string, content: string): Promise<void> {
  return invoke('save_file', { path, content });
}

/** Open a file from disk (text only, no engine evaluation). */
export async function openFile(path: string): Promise<FileData> {
  return invoke<FileData>('open_file', { path });
}

/** Open a file and load it into the engine for evaluation. Returns updated GUI state. */
export async function openFileEngine(path: string): Promise<GuiState> {
  const raw = await invoke<RawGuiState>('open_file_engine', { path });
  return convertRawGuiState(raw);
}

/** Export geometry to a file in the specified format. */
export async function exportGeometry(format: string, outputPath: string): Promise<void> {
  return invoke('export', { format, path: outputPath });
}

/** Open a native save-file dialog for export. Returns the chosen path, or null if cancelled. */
export async function pickSavePath(defaultName: string, formatExtension: string): Promise<string | null> {
  const result = await save({
    defaultPath: defaultName,
    filters: [
      {
        name: formatExtension.toUpperCase(),
        extensions: [formatExtension],
      },
    ],
  });
  return result ?? null;
}

/** Open a native open-file dialog. Returns the chosen path, or null if cancelled. */
export async function pickOpenPath(): Promise<string | null> {
  const result = await open({
    filters: [
      {
        name: 'Reify files',
        extensions: ['ri'],
      },
    ],
  });
  return (result as string) ?? null;
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

// ── Claude commands ─────────────────────────────────────────────────

/** Context for a Claude message (camelCase for TS consumers). */
export interface ClaudeMessageContext {
  selectedEntity?: string;
  diagnostics?: string[];
  constraints?: string[];
}

/** Send a message to the Claude sidecar. Maps camelCase context to snake_case for Rust. */
export async function claudeSendMessage(text: string, context?: ClaudeMessageContext): Promise<void> {
  return invoke('claude_send_message', {
    text,
    context: context
      ? {
          selected_entity: context.selectedEntity,
          diagnostics: context.diagnostics,
          constraints: context.constraints,
        }
      : undefined,
  });
}

/** Abort the current Claude response. */
export async function claudeAbort(): Promise<void> {
  return invoke('claude_abort');
}

/** Clear the Claude session. */
export async function claudeClearSession(): Promise<void> {
  return invoke('claude_clear_session');
}

// ── Claude event subscription ───────────────────────────────────────

/**
 * Subscribe to all Claude sidecar events and map payloads to OutboundMessage.
 * Returns a combined unlisten function that tears down all 7 subscriptions.
 */
export async function subscribeToClaudeEvents(
  handler: (msg: OutboundMessage) => void,
): Promise<() => void> {
  const unlisteners = await Promise.all([
    listen<{ id: string; content: string }>('claude-text-delta', (event) => {
      handler({ type: 'text_delta', ...event.payload });
    }),
    listen<{ id: string; content: string }>('claude-thinking-delta', (event) => {
      handler({ type: 'thinking_delta', ...event.payload });
    }),
    listen<{ id: string; tool_name: string; tool_input: Record<string, unknown> }>(
      'claude-tool-call',
      (event) => {
        handler({ type: 'tool_call', ...event.payload });
      },
    ),
    listen<{ id: string; tool_name: string; result: unknown }>(
      'claude-tool-result',
      (event) => {
        handler({ type: 'tool_result', ...event.payload });
      },
    ),
    listen<{ id: string }>('claude-done', (event) => {
      handler({ type: 'done', ...event.payload });
    }),
    listen<{ id: string; message: string }>('claude-error', (event) => {
      handler({ type: 'error', ...event.payload });
    }),
    listen('claude-ready', () => {
      handler({ type: 'ready' });
    }),
  ]);

  return () => {
    for (const unsub of unlisteners) {
      unsub();
    }
  };
}

// ── Event listeners (listen wrappers) ───────────────────────────────

/** Subscribe to mesh update events. Converts wire-format number[] to typed arrays. */
export async function onMeshUpdate(
  callback: (data: MeshData) => void,
): Promise<UnlistenFn> {
  return listen<RawMeshData>('mesh-update', (event) => {
    callback(convertRawMesh(event.payload));
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

/** Subscribe to mesh removal events. */
export async function onMeshRemoved(
  callback: (entityPath: string) => void,
): Promise<UnlistenFn> {
  return listen<string>('mesh-removed', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to value removal events. */
export async function onValueRemoved(
  callback: (cellId: string) => void,
): Promise<UnlistenFn> {
  return listen<string>('value-removed', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to constraint removal events. */
export async function onConstraintRemoved(
  callback: (nodeId: string) => void,
): Promise<UnlistenFn> {
  return listen<string>('constraint-removed', (event) => {
    callback(event.payload);
  });
}
