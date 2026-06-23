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
  SerializationError,
  DiagnosticInfo,
  EntityTreeNode,
  DefInfo,
  PersistentViewState,
  MechanismDescriptor,
  AutoResolveIteration,
  WarmPoolEvent,
  FeaCaseChanged,
  SolverProgress,
  ModeShapeFrame,
} from './types';
import { convertRawMesh, convertRawGuiState } from './types';
import type {
  OutboundMessage,
  TextDelta,
  ThinkingDelta,
  ToolCall,
  ToolResult,
  Done,
  ErrorMessage,
  NoticeMessage,
  PermissionRequest,
} from '../sidecar/src/types';

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

/**
 * Register the GUI's PASSIVE observed-demand sources (selective-demand
 * precondition, task 4532). OBSERVATIONAL ONLY — the backend records a
 * would-prune measurement that rides back on the NEXT `set_parameter` response's
 * `GuiState.demand_prune_measurement`; this command itself returns nothing and
 * cannot perturb evaluation.
 *
 * Args map (Tauri camelCase → snake_case Rust params, mirroring `set_parameter`'s
 * `cellId` → `cell_id`):
 *   - `visibleRealizations` → `visible_realizations` — mesh keys `Entity#realization[N]`
 *   - `displayedCells`      → `displayed_cells` — property-panel value cell ids
 *   - `panelConstraints`    → `panel_constraints` — constraint-panel node ids
 */
export async function syncObservedDemand(
  visibleRealizations: string[],
  displayedCells: string[],
  panelConstraints: string[],
): Promise<void> {
  return invoke('sync_observed_demand', { visibleRealizations, displayedCells, panelConstraints });
}

/**
 * Production selective-demand ENFORCEMENT sync (task 4737 α). Registers the
 * viewport-visible realization mesh keys (`show` + `ghost`, excluding `hidden`)
 * as the backend's PRODUCTION demand — the registry `compute_eval_set` actually
 * reads — so the next warm `edit_param` prunes a hidden body's exclusive cells.
 *
 * Counterpart to {@link syncObservedDemand} (the task-4532 PASSIVE measurement
 * channel, which never changes evaluation). Args map Tauri camelCase →
 * snake_case Rust: `visibleRealizations` → `visible_realizations`.
 */
export async function syncDemand(visibleRealizations: string[]): Promise<void> {
  return invoke('sync_demand', { visibleRealizations });
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

/** Get the entity tree from the backend. */
export async function getEntityTree(): Promise<EntityTreeNode[]> {
  return invoke<EntityTreeNode[]>('get_entity_tree');
}

/** Get mechanism descriptors from the backend (one per evaluated Mechanism cell). */
export async function getMechanismDescriptors(): Promise<MechanismDescriptor[]> {
  return invoke<MechanismDescriptor[]>('get_mechanism_descriptors');
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
export async function lspRequest(method: string, params: unknown): Promise<string> {
  return invoke<string>('lsp_request', { method, params: JSON.stringify(params) });
}

// ── Claude commands ─────────────────────────────────────────────────

/**
 * Re-export MessageContext as ClaudeMessageContext for backward compatibility.
 * The canonical definition lives in stores/claudeStore.ts — this alias preserves
 * the existing export name so all downstream imports continue to work.
 */
import type { MessageContext } from './stores/claudeStore';
export type { MessageContext as ClaudeMessageContext } from './stores/claudeStore';

/**
 * Exhaustive camelCase→snake_case mapping for MessageContext fields.
 * Uses `as const satisfies` so that:
 *  - Values are narrowed to their literal string types (e.g. 'selected_entity', not string)
 *  - Adding a new field to MessageContext without updating this table still causes a tsc error
 *
 * SYNC: When adding a field to MessageContext, update this table AND
 * ChatPanel.tsx buildMessageContext(). See gui/src/__tests__/types.typecheck.ts.
 */
export const MESSAGE_CONTEXT_FIELD_MAP = {
  selectedEntity: 'selected_entity',
  diagnostics: 'diagnostics',
  constraints: 'constraints',
  currentFile: 'current_file',
  attachedContexts: 'attached_contexts',
} as const satisfies Record<keyof Required<MessageContext>, string>;

/**
 * Every MessageContext key that buildMessageContext() in ChatPanel.tsx is
 * expected to handle.  The `satisfies` clause ensures each element is a valid
 * MessageContext key; an Equals<> assertion in types.typecheck.ts ensures
 * completeness against the full MessageContext interface.
 */
export const BUILD_CONTEXT_HANDLED_FIELDS = [
  'selectedEntity',
  'diagnostics',
  'constraints',
  'currentFile',
  'attachedContexts',
] as const satisfies readonly (keyof Required<MessageContext>)[];

/**
 * Snake_case wire representation of MessageContext, derived from
 * MESSAGE_CONTEXT_FIELD_MAP via key remapping. Adding a field to MessageContext
 * and the map automatically extends this type.
 */
export type WireMessageContext = {
  [K in keyof Required<MessageContext> as (typeof MESSAGE_CONTEXT_FIELD_MAP)[K]]?: MessageContext[K];
};

/** Convert a camelCase MessageContext to its snake_case wire representation using MESSAGE_CONTEXT_FIELD_MAP. */
export function mapContextToWire(ctx: MessageContext): WireMessageContext {
  const wire: Record<string, unknown> = {};
  for (const [camel, snake] of Object.entries(MESSAGE_CONTEXT_FIELD_MAP)) {
    if (ctx[camel as keyof MessageContext] !== undefined) {
      wire[snake] = ctx[camel as keyof MessageContext];
    }
  }
  return wire as WireMessageContext;
}

/** Send a message to the Claude sidecar. Maps camelCase context to snake_case for Rust. */
export async function claudeSendMessage(text: string, context?: MessageContext): Promise<void> {
  return invoke('claude_send_message', {
    text,
    context: context && Object.values(context).some(v => v !== undefined)
      ? mapContextToWire(context)
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

/** Forward a permission decision to the Tauri backend.
 *
 * Translates camelCase arguments to snake_case for Rust serde deserialization,
 * following the same omit-undefined discipline as mapContextToWire so that
 * Rust's `skip_serializing_if = "Option::is_none"` round-trips correctly.
 */
export async function claudePermissionDecision({
  requestId,
  behavior,
  message,
  updatedInput,
  remember,
}: {
  requestId: string;
  behavior: string;
  message?: string;
  updatedInput?: unknown;
  remember?: boolean;
}): Promise<void> {
  const payload: Record<string, unknown> = {
    request_id: requestId,
    behavior,
  };
  if (message !== undefined) payload.message = message;
  if (updatedInput !== undefined) payload.updated_input = updatedInput;
  if (remember !== undefined) payload.remember = remember;
  return invoke('claude_permission_decision', payload);
}

// ── Debug ───────────────────────────────────────────────────────────

/** Check if REIFY_DEBUG=1 is set (debug server and bridge enabled). */
export async function isDebugEnabled(): Promise<boolean> {
  return invoke<boolean>('is_debug_enabled');
}

// ── Claude event subscription ───────────────────────────────────────

/**
 * GR-016 β convention helpers — see
 * `docs/prds/v0_3/gui-event-channel-inventory.md` §3.5 and §5.2, plus the
 * canonical inventory at `docs/gui-event-channels.md`.
 *
 * For each new event channel, the consumer-side wrapper follows this shape:
 *   1. Define `KEYS_<NAME>` at module level (avoids per-call allocations).
 *   2. Export `on<Name>(callback): Promise<UnlistenFn>` that calls `listen<T>`
 *      with the channel name and passes `event.payload` through
 *      `validatePayload(channel, payload, KEYS_<NAME>)`.
 *   3. Hard-fail in DEV builds, console.warn in release builds (§5.2). Per the
 *      Phase-1 boundary, DEV-mode throwing happens at the `on<Name>` call site
 *      (e.g. `if (!p && import.meta.env.DEV) throw new Error(...)`), NOT inside
 *      `validatePayload` itself — keeping the helper warn-only preserves existing
 *      Claude-handler semantics.
 *   4. For typed-serde payloads (most channels), `validatePayload` is skipped and
 *      the `listen<T>` type-cast is trusted; downstream NPEs surface contract
 *      violations naturally per §5.2 paragraph 3.
 */

/**
 * Validate that a Tauri event payload is a non-null plain object with all
 * required keys present and of type string.
 * Returns the payload as a Record on success, or null on failure (with a console.warn).
 *
 * @internal Exported for §8.2 boundary tests in
 *   `src/__tests__/bridge/convention_smoke.test.ts`. Not a public API — the
 *   warn-only contract may be tightened to a DEV-mode throw in a future
 *   revision; production callers should compose this via `on<Name>` wrappers
 *   in this module rather than importing it directly.
 */
export function validatePayload(
  eventName: string,
  payload: unknown,
  requiredKeys: string[],
): Record<string, unknown> | null {
  if (payload == null || typeof payload !== 'object' || Array.isArray(payload)) {
    console.warn(`${eventName}: payload is not a plain object`, payload);
    return null;
  }
  const rec = payload as Record<string, unknown>;
  for (const key of requiredKeys) {
    if (typeof rec[key] !== 'string') {
      console.warn(`${eventName}: invalid payload, expected ${key} to be a string`, rec);
      return null;
    }
  }
  return rec;
}

/** Required-key arrays for validatePayload, hoisted to avoid per-call allocations. */
const KEYS_ID_CONTENT: string[] = ['id', 'content'];
const KEYS_ID_TOOL_NAME: string[] = ['id', 'tool_name'];
const KEYS_ID_TOOL_NAME_TOOL_USE_ID: string[] = ['id', 'tool_name', 'tool_use_id'];
const KEYS_ID: string[] = ['id'];
const KEYS_ID_MESSAGE: string[] = ['id', 'message'];
const KEYS_ID_CODE_MESSAGE: string[] = ['id', 'code', 'message'];
const KEYS_ID_REQUEST_ID_TOOL_NAME: string[] = ['id', 'request_id', 'tool_name'];
const KEYS_REASON: string[] = ['reason'];

/** Type guard for non-null, non-array object payloads. */
const isPlainObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

/**
 * Subscribe to all Claude sidecar events and map payloads to OutboundMessage.
 * Returns a combined unlisten function that tears down all 7 subscriptions.
 *
 * Uses sequential registration with rollback: if any listen() call fails,
 * all previously-registered listeners are torn down before the error propagates.
 */
export async function subscribeToClaudeEvents(
  handler: (msg: OutboundMessage) => void,
): Promise<() => void> {
  type EventEntry = [string, (event: { payload: unknown }) => void];

  const entries: EventEntry[] = [
    ['claude-text-delta', (event) => {
      const p = validatePayload('claude-text-delta', event.payload, KEYS_ID_CONTENT);
      if (!p) return;
      handler({ type: 'text_delta', id: p.id as string, content: p.content as string });
    }],
    ['claude-thinking-delta', (event) => {
      const p = validatePayload('claude-thinking-delta', event.payload, KEYS_ID_CONTENT);
      if (!p) return;
      handler({ type: 'thinking_delta', id: p.id as string, content: p.content as string });
    }],
    ['claude-tool-call', (event) => {
      const p = validatePayload('claude-tool-call', event.payload, KEYS_ID_TOOL_NAME_TOOL_USE_ID);
      if (!p) return;
      const toolInput = (p.tool_input != null && typeof p.tool_input === 'object' && !Array.isArray(p.tool_input))
        ? p.tool_input as Record<string, unknown>
        : {};
      handler({ type: 'tool_call', id: p.id as string, tool_use_id: p.tool_use_id as string, tool_name: p.tool_name as string, tool_input: toolInput });
    }],
    ['claude-tool-result', (event) => {
      const p = validatePayload('claude-tool-result', event.payload, KEYS_ID_TOOL_NAME);
      if (!p) return;
      handler({ type: 'tool_result', id: p.id as string, tool_name: p.tool_name as string, result: p.result });
    }],
    ['claude-done', (event) => {
      const p = validatePayload('claude-done', event.payload, KEYS_ID);
      if (!p) return;
      handler({ type: 'done', id: p.id as string });
    }],
    ['claude-error', (event) => {
      const p = validatePayload('claude-error', event.payload, KEYS_ID_MESSAGE);
      if (!p) return;
      handler({ type: 'error', id: p.id as string, message: p.message as string });
    }],
    ['claude-notice', (event) => {
      const p = validatePayload('claude-notice', event.payload, KEYS_ID_CODE_MESSAGE);
      if (!p) return;
      handler({ type: 'notice', id: p.id as string, code: p.code as string, message: p.message as string });
    }],
    ['claude-ready', () => handler({ type: 'ready' })],
    ['claude-permission-request', (event) => {
      const p = validatePayload('claude-permission-request', event.payload, KEYS_ID_REQUEST_ID_TOOL_NAME);
      if (!p) return;
      const toolInput = (p.tool_input != null && typeof p.tool_input === 'object' && !Array.isArray(p.tool_input))
        ? p.tool_input as Record<string, unknown>
        : {};
      handler({ type: 'permission_request', id: p.id as string, request_id: p.request_id as string, tool_name: p.tool_name as string, tool_input: toolInput });
    }],
  ];

  const unlisteners: UnlistenFn[] = [];
  try {
    for (const [name, mapper] of entries) {
      unlisteners.push(await listen(name, mapper));
    }
  } catch (err) {
    // Roll back all already-registered listeners before re-throwing
    for (const unsub of unlisteners) {
      unsub();
    }
    throw err;
  }

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

/** Subscribe to tessellation diagnostic events. Carries the full current list. */
export async function onTessellationDiagnostics(
  callback: (data: DiagnosticInfo[]) => void,
): Promise<UnlistenFn> {
  return listen<DiagnosticInfo[]>('tessellation-diagnostics', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to compile diagnostic events. Carries the full current list. */
export async function onCompileDiagnostics(
  callback: (data: DiagnosticInfo[]) => void,
): Promise<UnlistenFn> {
  return listen<DiagnosticInfo[]>('compile-diagnostics', (event) => {
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

/** Subscribe to file removal events. Fires when a watched file is deleted on disk. */
export async function onFileRemoved(
  callback: (payload: { path: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ path: string }>('file-removed', (event) => {
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

/** Subscribe to serialization error events. */
export async function onSerializationError(
  callback: (data: SerializationError) => void,
): Promise<UnlistenFn> {
  return listen<SerializationError>('serialization-error', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to focus-entity events emitted by the focus_entity Tauri command and the MCP focus_entity tool. */
export async function onFocusEntity(
  callback: (entityPath: string) => void,
): Promise<UnlistenFn> {
  return listen<string>('focus-entity', (event) => {
    callback(event.payload);
  });
}

/** Subscribe to navigate-to-source events emitted by the MCP navigate_to_source tool. Payload carries editor-target coordinates including the full source range. */
export async function onNavigateToSource(
  callback: (data: { file: string; line: number; column: number; end_line: number; end_column: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ file: string; line: number; column: number; end_line: number; end_column: number }>('navigate-to-source', (event) => {
    callback(event.payload);
  });
}

/** Get the containing definition (structure or occurrence) for a source position. Returns null if no definition contains the position. */
export async function getContainingDefinition(line: number, col: number): Promise<DefInfo | null> {
  return invoke<DefInfo | null>('get_containing_definition', { line, col });
}

/** Get the entity path (e.g. "Bracket.width" or "Bracket") for a cursor position. Returns null if the position is not inside any entity. */
export async function getEntityAtSourceLocation(line: number, col: number): Promise<string | null> {
  return invoke<string | null>('get_entity_at_source_location', { line, col });
}

/** Fetch the tessellated preview meshes for a named definition. Converts mesh wire data to typed arrays. */
export async function getDefPreview(defName: string): Promise<GuiState> {
  const raw = await invoke<RawGuiState>('get_def_preview', { defName });
  return convertRawGuiState(raw);
}

/** Whether the OCCT geometry kernel is available in this build. */
export interface KernelStatus {
  available: boolean;
  message: string | null;
}

/** Fetch the current kernel availability status via a Tauri command. */
export async function getKernelStatus(): Promise<KernelStatus> {
  return invoke<KernelStatus>('get_kernel_status');
}

/** Subscribe to kernel-status events emitted from Tauri setup() at startup. */
export async function onKernelStatus(
  callback: (status: KernelStatus) => void,
): Promise<UnlistenFn> {
  return listen<KernelStatus>('kernel-status', (event) => {
    callback(event.payload);
  });
}

/**
 * Subscribe to the `claude-sidecar-crashed` Tauri event synthesized by the Rust
 * `on_exit` hook when the sidecar Node process exits unexpectedly.
 * The callback receives the `reason` string from the event payload.
 * Malformed payloads (missing `reason`, non-object) are dropped via validatePayload.
 */
export async function subscribeToSidecarCrashed(
  callback: (reason: string) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('claude-sidecar-crashed', (event) => {
    const p = validatePayload('claude-sidecar-crashed', event.payload, KEYS_REASON);
    if (!p) return;
    callback(p.reason as string);
  });
}

// ── View sidecar commands ───────────────────────────────────────────

/**
 * Read the view sidecar file for `riPath` (i.e. `{riPath}.views.json`).
 *
 * Returns `null` when the file does not exist (backend returns `null`).
 * Rejects when the backend returns an error (e.g. malformed JSON, I/O error).
 */
export async function readViewSidecar(riPath: string): Promise<PersistentViewState | null> {
  const result = await invoke<PersistentViewState | null>('read_view_sidecar', { riPath });
  return result ?? null;
}

/**
 * Write the view sidecar file for `riPath` (i.e. `{riPath}.views.json`).
 *
 * Rejects when the backend returns an error (e.g. I/O error, serialisation failure).
 */
export async function writeViewSidecar(riPath: string, state: PersistentViewState): Promise<void> {
  await invoke<void>('write_view_sidecar', { riPath, state });
}

// ── Auto-resolve loop event listeners (Task 2967) ───────────────────

/** Subscribe to auto-resolve loop start events. Fires when a new solve loop begins. */
export async function onAutoResolveStart(
  callback: () => void,
): Promise<UnlistenFn> {
  return listen<void>('auto-resolve-start', () => {
    callback();
  });
}

/** Subscribe to auto-resolve iteration events. Fires after each solver iteration. */
export async function onAutoResolveIteration(
  callback: (iter: AutoResolveIteration) => void,
): Promise<UnlistenFn> {
  // Payload shape: docs/gui-event-channels/auto-resolve-iteration.md (§2)
  return listen<unknown>('auto-resolve-iteration', (event) => {
    const p = event.payload;
    if (
      !isPlainObject(p) ||
      typeof p['iteration'] !== 'number' ||
      !isPlainObject(p['parameters']) ||
      !isPlainObject(p['constraints'])
    ) {
      console.warn('[auto-resolve-iteration] malformed payload; dropping event', p);
      return;
    }
    callback(p as unknown as AutoResolveIteration);
  });
}

/** Subscribe to auto-resolve loop completion events. Fires when the solve loop finishes. */
export async function onAutoResolveComplete(
  callback: () => void,
): Promise<UnlistenFn> {
  return listen<void>('auto-resolve-complete', () => {
    callback();
  });
}

/**
 * Subscribe to warm-pool-event channel (GR-016 ε).
 *
 * Fires after each engine call boundary when the warm pool donates or evicts
 * a warm state. Payload validated with a hand-shaped guard (the numeric
 * `size_bytes` field cannot be validated by the string-only `validatePayload`
 * helper — follows the `onAutoResolveIteration` precedent at bridge.ts:620).
 *
 * Payload shape: docs/gui-event-channels/warm-pool-event.md §2.
 */
export async function onWarmPoolEvent(
  callback: (event: WarmPoolEvent) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('warm-pool-event', (event) => {
    const p = event.payload;
    if (
      !isPlainObject(p) ||
      (p['kind'] !== 'evicted' && p['kind'] !== 'donated') ||
      typeof p['size_bytes'] !== 'number' ||
      typeof p['node_id'] !== 'string'
    ) {
      console.warn('[warm-pool-event] malformed payload; dropping event', p);
      return;
    }
    callback(p as unknown as WarmPoolEvent);
  });
}

/**
 * Subscribe to `fea-case-changed` Tauri events.
 *
 * Emitted by `EngineSession::emit_fea_case_if_any` once per check that
 * observes a MultiCaseResult-shaped value in `CheckResult.values`.
 * Consumer: `FeaCasePickerDropdown`.
 *
 * Uses the inline structural-shape-guard idiom (listen<unknown> +
 * isPlainObject + per-field type checks + console.warn drop), mirroring
 * `onAutoResolveIteration` (bridge.ts:618) and `onWarmPoolEvent` (bridge.ts:656).
 * The guard additionally validates `available_cases` is a string[] array,
 * which `validatePayload`'s keys-array form cannot express.
 *
 * Per-channel spec: docs/gui-event-channels/fea-case-changed.md
 */
export async function onFeaCaseChanged(
  callback: (payload: FeaCaseChanged) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('fea-case-changed', (event) => {
    const p = event.payload;
    if (
      !isPlainObject(p) ||
      typeof p['active_case_id'] !== 'string' ||
      !Array.isArray(p['available_cases']) ||
      !p['available_cases'].every((s) => typeof s === 'string')
    ) {
      console.warn('[fea-case-changed] malformed payload; dropping event', p);
      return;
    }
    callback(p as unknown as FeaCaseChanged);
  });
}

/**
 * Wire the inbound `fea-case-changed` event into a store (task 3026 step-10).
 *
 * Calls `onFeaCaseChanged` and routes each valid payload into
 * `store.applyFeaCaseChanged`, populating `state.availableCases` and
 * `state.activeCaseId`.
 *
 * Returns the Tauri UnlistenFn promise so the caller can unsubscribe on
 * cleanup (e.g. component teardown in Viewport via onCleanup).
 */
export function subscribeFeaCaseToStore(
  store: { applyFeaCaseChanged(payload: FeaCaseChanged): void },
): Promise<UnlistenFn> {
  return onFeaCaseChanged((payload) => {
    store.applyFeaCaseChanged(payload);
  });
}

/**
 * Subscribe to `solver-progress` Tauri events (GR-016 ζ).
 *
 * Emitted at the end of each CG iteration by `solve_cg_with_progress` in
 * `crates/reify-solver-elastic/src/solver.rs`. The engine-boundary `app.emit`
 * wiring is a follow-on task; this listener is the GUI-side subscription seam.
 *
 * Uses the inline structural-shape-guard idiom (listen<unknown> +
 * isPlainObject + per-field type checks + console.warn drop on malformed),
 * mirroring `onFeaCaseChanged` (bridge.ts:699-715). `eta_ms` is optional and
 * not asserted — its absence from the wire shape is a valid case (first
 * iteration before residual history is available).
 *
 * Per-channel spec: docs/gui-event-channels/solver-progress.md
 */
export async function onSolverProgress(
  callback: (payload: SolverProgress) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('solver-progress', (event) => {
    const p = event.payload;
    if (
      !isPlainObject(p) ||
      typeof p['solver_kind'] !== 'string' ||
      typeof p['iter'] !== 'number' ||
      typeof p['residual'] !== 'number'
    ) {
      console.warn('[solver-progress] malformed payload; dropping event', p);
      return;
    }
    callback(p as unknown as SolverProgress);
  });
}

/**
 * Subscribe to mode-shape-frame events from the backend (task ι/3458).
 *
 * The backend emits one undeformed base frame (phase=0.0) and one peak frame
 * per mode (phase=1.0) on each solve completion that produces a BucklingResult.
 * Applies the inline structural-shape-guard idiom (listen<unknown> +
 * isPlainObject + per-field type checks + console.warn drop on malformed),
 * mirroring `onSolverProgress`.
 *
 * Per-channel spec: docs/gui-event-channels.md §2 (mode-shape-frame row, ACTIVE)
 */
export async function onModeShapeFrame(
  callback: (payload: ModeShapeFrame) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('mode-shape-frame', (event) => {
    const p = event.payload;
    if (
      !isPlainObject(p) ||
      typeof p['mode_index'] !== 'number' ||
      typeof p['phase'] !== 'number' ||
      !Array.isArray(p['displaced_positions']) ||
      !(p['displaced_positions'] as unknown[]).every(n => typeof n === 'number') ||
      ('eigenvalue' in p && typeof (p as Record<string, unknown>)['eigenvalue'] !== 'number')
    ) {
      console.warn('[mode-shape-frame] malformed payload; dropping event', p);
      return;
    }
    callback(p as unknown as ModeShapeFrame);
  });
}

/**
 * Invoke the `set_active_fea_case` Tauri command (task 3026 step-14).
 *
 * Tells the engine to switch the active FEA load case and rebuild the
 * per-case scalar channels (von-Mises / displaced_positions) from the
 * named case's ElasticResult.  The returned GuiState is applied by the
 * existing state-apply path in the caller.
 *
 * The `case` parameter name matches the Rust handler's `#[tauri::command]`
 * parameter: `set_active_fea_case(case: String)`.
 */
export async function setActiveFeaCase(caseName: string): Promise<void> {
  return invoke('set_active_fea_case', { case: caseName });
}

/**
 * Cancel an in-flight FEA solve (GR-016 ζ).
 *
 * Invokes the `cancel_solve` Tauri command which calls `.cancel()` on the
 * `CancellationHandle` stored in `AppState::pending_solve_cancel`, if any.
 * A no-op (returns Ok) when no solve is currently in flight.
 *
 * PRD §11 Q2 resolution: command, not event.
 */
export async function cancelSolve(): Promise<void> {
  return invoke('cancel_solve');
}
