import type { Component } from 'solid-js';
import styles from './AbortButton.module.css';

export interface AbortButtonProps {
  onAbort: () => void;
}

export const AbortButton: Component<AbortButtonProps> = (props) => {
  return (
    <button
      data-testid="abort-button"
      class={styles.abortButton}
      aria-label="Abort generation"
      onClick={() => props.onAbort()}
    >
      <span class={styles.stopIcon}>■</span>
    </button>
  );
};
