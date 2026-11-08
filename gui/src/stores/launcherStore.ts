/**
 * The Start view's state (alpha-spine β): the document-opening error and Recent
 * list, and the selected launch mode with the debug server's live port.
 */
import { createStore } from 'solid-js/store';
import type { LaunchMode, ModeState, RecentEntry } from '../types';

export const LAUNCH_ERROR_CODES = Object.freeze({
  E_GUI_OPEN_FAILED: 'E_GUI_OPEN_FAILED',
  E_GUI_TEMPLATE_WRITE_FAILED: 'E_GUI_TEMPLATE_WRITE_FAILED',
  E_GUI_DEBUG_LISTENER_FAILED: 'E_GUI_DEBUG_LISTENER_FAILED',
} as const);

export type LaunchErrorCode = (typeof LAUNCH_ERROR_CODES)[keyof typeof LAUNCH_ERROR_CODES];

export interface LaunchErrorInfo {
  code: LaunchErrorCode;
  message: string;
}

export interface LauncherState {
  document: { error: LaunchErrorInfo | null; recent: RecentEntry[] };
  mode: { selected: LaunchMode; debugPort: number | null; error: LaunchErrorInfo | null };
}

/** The one definition of "a document is loaded": the editor has an open file. */
export function isDocumentLoaded(editor: { openFiles: readonly unknown[] }): boolean {
  return editor.openFiles.length > 0;
}

export function createLauncherStore() {
  const [state, setState] = createStore<LauncherState>({
    document: { error: null, recent: [] },
    // The chat panel opens by default, so the initial selection is the assistant.
    mode: { selected: 'assistant', debugPort: null, error: null },
  });

  function applyModeState(reply: ModeState): void {
    setState('mode', { selected: reply.mode, debugPort: reply.debug_port, error: null });
  }

  function failDocument(error: LaunchErrorInfo): void {
    setState('document', 'error', error);
  }

  function clearDocumentError(): void {
    setState('document', 'error', null);
  }

  function failMode(error: LaunchErrorInfo): void {
    setState('mode', 'error', error);
  }

  function setRecent(recent: RecentEntry[]): void {
    setState('document', 'recent', recent);
  }

  /** Clear both errors; the Recent list and the mode stay (a bound listener cannot be un-bound). */
  function reset(): void {
    setState('document', 'error', null);
    setState('mode', 'error', null);
  }

  return { state, applyModeState, failDocument, clearDocumentError, failMode, setRecent, reset };
}

export type LauncherStore = ReturnType<typeof createLauncherStore>;
