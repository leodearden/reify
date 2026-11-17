/**
 * The launcher's actions (alpha-spine β): open, create, refresh Recent, select
 * a launch mode, and adopt a debug server already live at launch. An action's
 * primary stage rejects with a LaunchFailure whose code names that stage;
 * presenting it is the caller's job (the Start view writes it to the launcher
 * store, the File menu toasts it, App's env launch does both). A secondary
 * stage reports its own failure through notify without rejecting:
 * openDocument's Recent update, and adoptLiveDebugServer's set_mode, which also
 * becomes the store's mode error. One document attempt (open or create) runs at
 * a time: the store's document.pending holds its requested path, and a second
 * attempt is refused with E_GUI_DOCUMENT_BUSY naming that path. Pending is
 * cleared by the attempt's own settling or by abandonDocumentAttempt, not by
 * the store's reset. Abandoning resolves the attempt's action at once, aborts
 * the signal its stages were given, and ignores how it settles later.
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
   * can still present it. An aborted `signal` stops it before its next engine
   * request or its first store write, so an abandoned document never lands.
   */
  loadDocument(path: string, signal: AbortSignal): Promise<void>;
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

/**
 * set_mode's failure code per mode while the listener is not known to be live:
 * only Debug's set_mode can bind it (launch_mode.rs::set_mode_impl).
 */
const SELECT_MODE_FAILURE_CODES: Readonly<Record<LaunchMode, LaunchErrorCode>> = Object.freeze({
  design: LAUNCH_ERROR_CODES.E_GUI_MODE_FAILED,
  assistant: LAUNCH_ERROR_CODES.E_GUI_MODE_FAILED,
  debug: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED,
});

/**
 * selectMode's set_mode failure code, given the debugPort the store held
 * before the call. A non-null debugPort came from a set_mode reply, so the
 * listener was live then; a live listener is never unbound and ensure_live
 * does not bind again (debug_server_slot.rs), so set_mode can then fail only
 * at the IPC level.
 */
function selectModeFailureCode(mode: LaunchMode, debugPort: number | null): LaunchErrorCode {
  return debugPort === null ? SELECT_MODE_FAILURE_CODES[mode] : LAUNCH_ERROR_CODES.E_GUI_MODE_FAILED;
}

/** Run `stage`, turning any rejection into a LaunchFailure tagged with `code`. */
async function inStage<T>(code: LaunchErrorCode, stage: () => Promise<T>): Promise<T> {
  try {
    return await stage();
  } catch (err) {
    throw new LaunchFailure(code, errorMessage(err));
  }
}

/**
 * Settle with `work`, or resolve as soon as `signal` aborts, whichever comes
 * first. A later settling of `work` is ignored, a rejection included, so it
 * is neither presented nor unhandled.
 */
function untilAbandoned(work: Promise<void>, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) resolve();
    signal.addEventListener('abort', () => resolve(), { once: true });
    work.then(resolve, reject);
  });
}

export function createLauncherActions(store: LauncherStore, deps: LauncherDeps) {
  /** The controller of the attempt that owns document.pending. */
  let inFlight: AbortController | null = null;

  function debugBridgeStage(): Promise<void> {
    return inStage(LAUNCH_ERROR_CODES.E_GUI_DEBUG_BRIDGE_FAILED, () => deps.ensureDebugBridge());
  }

  async function loadAndRecord(path: string, signal: AbortSignal): Promise<void> {
    await inStage(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, () => deps.loadDocument(path, signal));
    signal.throwIfAborted();
    try {
      store.setRecent(await deps.recordRecent(path));
    } catch (err) {
      deps.notify(`Recent list not updated: ${errorMessage(err)}`);
    }
  }

  async function exclusiveDocumentAttempt(
    path: string,
    attempt: (signal: AbortSignal) => Promise<void>,
  ): Promise<void> {
    const pendingPath = store.state.document.pending;
    if (pendingPath !== null) {
      throw new LaunchFailure(LAUNCH_ERROR_CODES.E_GUI_DOCUMENT_BUSY, `${pendingPath} is still opening`);
    }
    const controller = new AbortController();
    inFlight = controller;
    store.setDocumentPending(path);
    try {
      await untilAbandoned(attempt(controller.signal), controller.signal);
    } finally {
      if (inFlight === controller) {
        inFlight = null;
        store.setDocumentPending(null);
      }
    }
  }

  function openDocument(path: string): Promise<void> {
    return exclusiveDocumentAttempt(path, (signal) => loadAndRecord(path, signal));
  }

  function createDocument(path: string): Promise<void> {
    return exclusiveDocumentAttempt(path, async (signal) => {
      const created = await inStage(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, () =>
        deps.createNewFile(path),
      );
      signal.throwIfAborted();
      await loadAndRecord(created.path, signal);
    });
  }

  function abandonDocumentAttempt(): void {
    inFlight?.abort();
    inFlight = null;
    store.setDocumentPending(null);
  }

  async function refreshRecent(): Promise<void> {
    store.setRecent(await deps.listRecent());
  }

  async function selectMode(mode: LaunchMode): Promise<void> {
    const reply = await inStage(selectModeFailureCode(mode, store.state.mode.debugPort), () =>
      deps.setMode(mode),
    );
    store.applyModeState(reply);
    if (mode === 'debug') {
      await debugBridgeStage();
    } else {
      deps.showAssistant(mode === 'assistant');
    }
  }

  async function recordDebugMode(): Promise<void> {
    try {
      store.applyModeState(await deps.setMode('debug'));
    } catch (err) {
      const message = errorMessage(err);
      store.failMode({ code: LAUNCH_ERROR_CODES.E_GUI_MODE_FAILED, message });
      deps.notify(`Debug mode not recorded (set_mode failed): ${message}`);
    }
  }

  /**
   * Enter Debug mode for a debug server that is already live (REIFY_DEBUG=1 at
   * launch). The bridge does not wait on set_mode, a set_mode failure becomes
   * the mode error and a notification (E_GUI_MODE_FAILED, since the listener
   * is already live), and the action rejects only when the bridge fails.
   */
  async function adoptLiveDebugServer(): Promise<void> {
    const bridge = debugBridgeStage();
    await Promise.allSettled([bridge, recordDebugMode()]);
    await bridge;
  }

  return {
    openDocument,
    createDocument,
    abandonDocumentAttempt,
    refreshRecent,
    selectMode,
    adoptLiveDebugServer,
  };
}

export type LauncherActions = ReturnType<typeof createLauncherActions>;
