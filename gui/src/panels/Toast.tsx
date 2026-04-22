import { type Component, For, onMount, onCleanup } from 'solid-js';
import styles from './Toast.module.css';

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface ToastProps {
  message: string;
  type: 'success' | 'error' | 'info';
  onDismiss: () => void;
  /** Optional action buttons. When provided, each button's onClick is called
   *  first, then `onDismiss` is invoked to close the toast. */
  actions?: ToastAction[];
}

export const Toast: Component<ToastProps> = (props) => {
  let timerId: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    const timeout = props.type === 'error' ? 5000 : 3000;
    timerId = setTimeout(() => {
      props.onDismiss();
    }, timeout);
  });

  onCleanup(() => {
    if (timerId !== undefined) {
      clearTimeout(timerId);
    }
  });

  return (
    <div data-testid="toast" data-type={props.type} class={`${styles.toast} ${styles.animated}`} role="alert" aria-live="assertive">
      <span class={styles.message}>{props.message}</span>
      {props.actions && props.actions.length > 0 && (
        <div class={styles.actions}>
          <For each={props.actions}>
            {(action) => (
              <button
                class={styles.actionBtn}
                onClick={() => {
                  action.onClick();
                  props.onDismiss();
                }}
              >
                {action.label}
              </button>
            )}
          </For>
        </div>
      )}
      <button class={styles.close} aria-label="Close" onClick={() => props.onDismiss()}>
        &times;
      </button>
    </div>
  );
};
