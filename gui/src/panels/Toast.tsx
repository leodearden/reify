import type { Component } from 'solid-js';
import styles from './Toast.module.css';

export interface ToastProps {
  message: string;
  type: 'success' | 'error' | 'info';
  onDismiss: () => void;
}

export const Toast: Component<ToastProps> = (props) => {
  return (
    <div data-testid="toast" data-type={props.type} class={styles.toast}>
      <span class={styles.message}>{props.message}</span>
      <button class={styles.close} onClick={() => props.onDismiss()}>
        &times;
      </button>
    </div>
  );
};
