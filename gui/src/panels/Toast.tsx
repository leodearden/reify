import { type Component, onMount, onCleanup } from 'solid-js';
import styles from './Toast.module.css';

export interface ToastProps {
  message: string;
  type: 'success' | 'error' | 'info';
  onDismiss: () => void;
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
      <button class={styles.close} aria-label="Close" onClick={() => props.onDismiss()}>
        &times;
      </button>
    </div>
  );
};
