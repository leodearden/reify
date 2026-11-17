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
 * attempt is refused, by refuseIfDocumentPending, with a LaunchFailure under its
 * own code naming that path. Only the attempt's own settling clears pending (the
 * store's reset does not), so a load that never settles leaves every document
 * attempt refused until the app restarts.
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

/**
 * set_mode's failure code per mode while the listener is not known to be live:
 * only Debug's set_mode can bind it (launch_mode.rs::set_mode_impl).
 */
const SELECT_MODE_FAILURE_CODES: Readonly<Record<LaunchMode, LaunchErrorCode>> = Object.freeze({
  design: LAUNCH_ERROR_CODES.E_GUI_MODE_FAILED,
  assistant: LAUNCH_ERROR_CODES.E_GUI_MODE_FAILED,
  debug: LAUNCH_ERROR_CODES.E_GUI_DEBUG_LISTENER_FAILED,
});

export type DocumentAttemptKind = 'open' | 'create';

/** The code a document attempt of each kind is refused under while another is pending. */
const DOCUMENT_ATTEMPT_CODES: Readonly<Record<DocumentAttemptKind, LaunchErrorCode>> = Object.freeze({
  open: LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED,
  create: LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED,
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

export function createLauncherActions(store: LauncherStore, deps: LauncherDeps) {
  function debugBridgeStage(): Promise<void> {
    return inStage(LAUNCH_ERROR_CODES.E_GUI_DEBUG_BRIDGE_FAILED, () => deps.ensureDebugBridge());
  }

  async function loadAndRecord(path: string): Promise<void> {
    await inStage(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, () => deps.loadDocument(path));
    try {
      store.setRecent(await deps.recordRecent(path));
    } catch (err) {
      deps.notify(`Recent list not updated: ${errorMessage(err)}`);
    }
  }

  /** Callers refuse before their own preparation (a native file dialog); the action re-checks. */
  function refuseIfDocumentPending(kind: DocumentAttemptKind): void {
    const inFlight = store.state.document.pending;
    if (inFlight !== null) {
      throw new LaunchFailure(DOCUMENT_ATTEMPT_CODES[kind], `${inFlight} is still opening`);
    }
  }

  async function exclusiveDocumentAttempt(
    kind: DocumentAttemptKind,
    path: string,
    attempt: () => Promise<void>,
  ): Promise<void> {
    refuseIfDocumentPending(kind);
    store.setDocumentPending(path);
    try {
      await attempt();
    } finally {
      store.setDocumentPending(null);
    }
  }

  function openDocument(path: string): Promise<void> {
    return exclusiveDocumentAttempt('open', path, () => loadAndRecord(path));
  }

  function createDocument(path: string): Promise<void> {
    return exclusiveDocumentAttempt('create', path, async () => {
      const created = await inStage(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, () =>
        deps.createNewFile(path),
      );
      await loadAndRecord(created.path);
    });
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
    refuseIfDocumentPending,
    refreshRecent,
    selectMode,
    adoptLiveDebugServer,
  };
}

export type LauncherActions = ReturnType<typeof createLauncherActions>;
