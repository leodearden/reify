import type { Component } from 'solid-js';
import styles from './ContextChip.module.css';

export interface ContextChipProps {
  label: string;
  type: string;
  onRemove: () => void;
}

export const ContextChip: Component<ContextChipProps> = (props) => {
  return (
    <span
      class={styles.chip}
      data-testid="context-chip"
      data-context-type={props.type}
    >
      <span class={styles.label}>{props.label}</span>
      <button
        class={styles.remove}
        data-testid="chip-remove"
        onClick={() => props.onRemove()}
        onKeyDown={(e: KeyboardEvent) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            props.onRemove();
          }
        }}
        aria-label={`Remove ${props.label}`}
      >
        ×
      </button>
    </span>
  );
};
