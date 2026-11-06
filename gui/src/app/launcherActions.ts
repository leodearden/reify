/**
 * The Start view's actions (alpha-spine β): open, create, refresh Recent and
 * select a launch mode. A failure rejects with a LaunchFailure whose code names
 * the stage that failed; presenting it is the caller's job (the Start view
 * writes it to the launcher store, the File menu toasts it).
 */
import type { FileData, LaunchMode, ModeState, RecentEntry } from '../types';
import { errorMessage } from '../utils/errorClassifier';
import {
  LAUNCH_ERROR_CODES,
  type LaunchErrorCode,
  type LaunchErrorInfo,
  type LauncherStore,
} from '../stores/launcherStore';

export class LaunchFailure extends Error {
  readonly code: LaunchErrorCode;

  constructor(code: LaunchErrorCode, message: string) {
    super(message);
    this.name = 'LaunchFailure';
    this.code = code;
  }

  toInfo(): LaunchErrorInfo {
    return { code: this.code, message: this.message };
  }
}

export interface LauncherDeps {
  /**
   * Read `path` into the editor and engine stores (App's one open funnel).
   * Rejects only before writing any store, so a failure reaches a caller that
   * can still present it.
   */
  loadDocument(path: string): Promise<void>;
  createNewFile(path: string): Promise<FileData>;
  recordRecent(path: string): Promise<RecentEntry[]>;
  listRecent(): Promise<RecentEntry[]>;
  setMode(mode: LaunchMode): Promise<ModeState>;
  showAssistant(open: boolean): void;
  /** Start the frontend half of the reify-debug bridge. */
  startDebugBridge(): void | Promise<unknown>;
  /** Report a non-fatal problem to the user. */
  notify(message: string): void;
}

/** Run `stage`, turning any rejection into a LaunchFailure tagged with `code`. */
async function inStage<T>(code: LaunchErrorCode, stage: () => Promise<T>): Promise<T> {
  try {
    return await stage();
  } catch (err) {
    throw new LaunchFailure(code, errorMessage(err));
  }
}

export function createLauncherActions(store: LauncherStore, deps: LauncherDeps) {
  async function openDocument(path: string): Promise<void> {
    store.clearDocumentError();
    await inStage(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, () => deps.loadDocument(path));
    try {
      store.setRecent(await deps.recordRecent(path));
    } catch (err) {
      deps.notify(`Recent list not updated: ${errorMessage(err)}`);
    }
  }

  async function createDocument(path: string): Promise<void> {
    const created = await inStage(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, () =>
      deps.createNewFile(path),
    );
    await openDocument(created.path);
  }

  async function refreshRecent(): Promise<void> {
    store.setRecent(await deps.listRecent());
  }

  async function selectMode(mode: LaunchMode): Promise<void> {
    const bridgeWasLive = store.state.mode.debugPort !== null;
    const reply = await inStage(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, () =>
      deps.setMode(mode),
    );
    store.applyModeState(reply);
    if (mode === 'debug') {
      if (!bridgeWasLive) await deps.startDebugBridge();
    } else {
      deps.showAssistant(mode === 'assistant');
    }
  }

  return { openDocument, createDocument, refreshRecent, selectMode };
}

export type LauncherActions = ReturnType<typeof createLauncherActions>;
