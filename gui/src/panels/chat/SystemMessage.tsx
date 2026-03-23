import type { Component } from 'solid-js';
import styles from './SystemMessage.module.css';

export interface SystemMessageProps {
  errorType: string;
  text: string;
}

export const SystemMessage: Component<SystemMessageProps> = (props) => {
  return (
    <div
      class={styles.systemMessage}
      data-testid="system-message"
      data-error-type={props.errorType}
    >
      <span class={styles.icon}>⚠</span>
      <span class={styles.text}>{props.text}</span>
    </div>
  );
};
