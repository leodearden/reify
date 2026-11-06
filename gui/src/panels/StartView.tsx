/**
 * The launcher's Start view (alpha-spine β): shown over the layout while no
 * document is open. Every action failure is written to the launcher store,
 * which keeps this view up with a coded error the debug MCP can observe.
 */
import { createSignal, For, onMount, Show, type Component } from 'solid-js';
import { pickOpenPath, pickSavePath } from '../bridge';
import { LaunchFailure, type LauncherActions } from '../app/launcherActions';
import {
  LAUNCH_ERROR_CODES,
  type LaunchErrorCode,
  type LaunchErrorInfo,
  type LauncherStore,
} from '../stores/launcherStore';
import { errorMessage } from '../utils/errorClassifier';
import type { LaunchMode } from '../types';
import styles from './StartView.module.css';

export interface StartViewProps {
  launcher: LauncherStore;
  actions: LauncherActions;
}

const MODES: readonly { mode: LaunchMode; label: string }[] = [
  { mode: 'design', label: 'Design' },
  { mode: 'assistant', label: 'Assistant' },
  { mode: 'debug', label: 'Debug' },
];

const ErrorLine: Component<{ testId: string; error: LaunchErrorInfo }> = (props) => (
  <p class={styles.error} role="alert" data-testid={props.testId}>
    {props.error.code}: {props.error.message}
  </p>
);

export const StartView: Component<StartViewProps> = (props) => {
  const [typedPath, setTypedPath] = createSignal('');

  onMount(() => {
    props.actions.refreshRecent().catch((err: unknown) => {
      console.warn('Recent list unavailable:', err);
    });
  });

  /**
   * Run a document attempt and present any failure in the store. A rejection
   * that is not already a LaunchFailure (a native dialog that cannot open, an
   * unexpected throw) is presented under `code`, so no attempt fails silently.
   */
  async function attemptDocument(code: LaunchErrorCode, run: () => Promise<void>): Promise<void> {
    props.launcher.clearDocumentError();
    try {
      await run();
    } catch (err) {
      const failure = err instanceof LaunchFailure ? err.toInfo() : { code, message: errorMessage(err) };
      props.launcher.failDocument(failure);
    }
  }

  async function attemptMode(mode: LaunchMode): Promise<void> {
    try {
      await props.actions.selectMode(mode);
    } catch (err) {
      if (!(err instanceof LaunchFailure)) throw err;
      props.launcher.failMode(err.toInfo());
    }
  }

  const openPath = (path: string) =>
    attemptDocument(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, () => props.actions.openDocument(path));
  const createPath = (path: string) =>
    attemptDocument(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, () => props.actions.createDocument(path));

  function openFromDialog(): Promise<void> {
    return attemptDocument(LAUNCH_ERROR_CODES.E_GUI_OPEN_FAILED, async () => {
      const path = await pickOpenPath();
      if (path) await props.actions.openDocument(path);
    });
  }

  function createFromDialog(): Promise<void> {
    return attemptDocument(LAUNCH_ERROR_CODES.E_GUI_TEMPLATE_WRITE_FAILED, async () => {
      const path = await pickSavePath('untitled.ri', 'ri');
      if (path) await props.actions.createDocument(path);
    });
  }

  function withTypedPath(run: (path: string) => Promise<void>): void {
    const path = typedPath().trim();
    if (path) void run(path);
  }

  const state = () => props.launcher.state;

  return (
    <div class={styles.startView} data-testid="start-view">
      <div class={styles.content}>
        <h1 class={styles.title}>Reify</h1>

        <section class={styles.section}>
          <h2 class={styles.heading}>Mode</h2>
          <div class={styles.modes}>
            <For each={MODES}>
              {({ mode, label }) => (
                <button
                  type="button"
                  class={styles.modeButton}
                  data-testid={`start-mode-${mode}`}
                  aria-pressed={state().mode.selected === mode}
                  onClick={() => void attemptMode(mode)}
                >
                  {label}
                </button>
              )}
            </For>
          </div>
          <p class={styles.note} data-testid="start-assistant-note">
            The assistant needs a Claude Code login on this machine.
          </p>
          <Show when={state().mode.debugPort}>
            {(port) => (
              <p class={styles.note} data-testid="start-debug-port">
                Debug server listening on 127.0.0.1:{port()}
              </p>
            )}
          </Show>
          <Show when={state().mode.error}>
            {(error) => <ErrorLine testId="start-mode-error" error={error()} />}
          </Show>
        </section>

        <section class={styles.section}>
          <h2 class={styles.heading}>Document</h2>
          <div class={styles.row}>
            <button type="button" class={styles.button} data-testid="start-open-dialog" onClick={() => void openFromDialog()}>
              Open…
            </button>
            <button type="button" class={styles.button} data-testid="start-new-dialog" onClick={() => void createFromDialog()}>
              New…
            </button>
          </div>
          <label class={styles.pathLabel}>
            Or enter a path
            <input
              type="text"
              class={styles.pathInput}
              data-testid="start-path-input"
              value={typedPath()}
              onInput={(e) => setTypedPath(e.currentTarget.value)}
            />
          </label>
          <div class={styles.row}>
            <button
              type="button"
              class={styles.button}
              data-testid="start-open-path"
              onClick={() => withTypedPath(openPath)}
            >
              Open
            </button>
            <button
              type="button"
              class={styles.button}
              data-testid="start-create-path"
              onClick={() => withTypedPath(createPath)}
            >
              Create
            </button>
          </div>
          <Show when={state().document.error}>
            {(error) => <ErrorLine testId="start-document-error" error={error()} />}
          </Show>
        </section>

        <section class={styles.section}>
          <h2 class={styles.heading}>Recent</h2>
          <Show when={state().document.recent.length > 0} fallback={<p class={styles.note}>No recent documents.</p>}>
            <ul class={styles.recent}>
              <For each={state().document.recent}>
                {(entry) => (
                  <li>
                    <button
                      type="button"
                      class={styles.recentItem}
                      data-testid="start-recent-item"
                      onClick={() => void openPath(entry.path)}
                    >
                      {entry.path}
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </section>
      </div>
    </div>
  );
};
