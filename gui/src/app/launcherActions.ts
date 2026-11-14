/**
 * The launcher's actions (alpha-spine β): open, create, refresh Recent, select
 * a launch mode, and adopt a debug server already live at launch. An action's
 * primary stage rejects with a LaunchFailure whose code names that stage;
 * presenting it is the caller's job (the Start view writes it to the launcher
 * store, the File menu toasts it, App's env launch does both). A secondary
 * stage reports its own failure through notify without rejecting:
 * openDocument's Recent update, and adoptLiveDebugServer's set_mode, which also
 * becomes the store's mode error.
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
  /**
   * Start the frontend half of the reify-debug bridge unless it is live or
   * starting. Every caller awaits the one start and sees its failure.
   */
  ensureDebugBridge(): Promise<void>;
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
    const reply = await inStage(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, () =>
      deps.setMode(mode),
    );
    store.applyModeState(reply);
    if (mode === 'debug') {
      await inStage(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, () => deps.ensureDebugBridge());
    } else {
      deps.showAssistant(mode === 'assistant');
    }
  }

  async function recordDebugMode(): Promise<void> {
    try {
      store.applyModeState(await deps.setMode('debug'));
    } catch (err) {
      const message = errorMessage(err);
      store.failMode({ code: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, message });
      deps.notify(`Debug mode not recorded (set_mode failed): ${message}`);
    }
  }

  /**
   * Enter Debug mode for a debug server that is already live (REIFY_DEBUG=1 at
   * launch). The bridge does not wait on set_mode, a set_mode failure becomes
   * the mode error and a notification, and the action rejects only when the
   * bridge fails.
   */
  async function adoptLiveDebugServer(): Promise<void> {
    const bridge = inStage(LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED, () => deps.ensureDebugBridge());
    await Promise.allSettled([bridge, recordDebugMode()]);
    await bridge;
  }

  return { openDocument, createDocument, refreshRecent, selectMode, adoptLiveDebugServer };
}

export type LauncherActions = ReturnType<typeof createLauncherActions>;
