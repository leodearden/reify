import { type Component } from 'solid-js';
import styles from './PermissionPrompt.module.css';

export interface PermissionDecision {
  behavior: 'allow' | 'deny';
  remember?: boolean;
}

export interface PermissionPromptProps {
  toolName: string;
  toolInput: Record<string, unknown>;
  onDecide: (decision: PermissionDecision) => void;
}

export const PermissionPrompt: Component<PermissionPromptProps> = (props) => {
  return (
    <div data-testid="permission-prompt" class={styles.card}>
      <div class={styles.header}>
        <span class={styles.title}>
          Claude wants to use <strong>{props.toolName}</strong>
        </span>
      </div>
      <div class={styles.body}>
        <pre class={styles.json}>{JSON.stringify(props.toolInput, null, 2)}</pre>
      </div>
      <div class={styles.actions}>
        <button
          data-testid="permission-allow"
          class={styles.buttonAllow}
          onClick={() => props.onDecide({ behavior: 'allow' })}
        >
          Allow
        </button>
        <button
          data-testid="permission-deny"
          class={styles.buttonDeny}
          onClick={() => props.onDecide({ behavior: 'deny' })}
        >
          Deny
        </button>
        <button
          data-testid="permission-allow-always"
          class={styles.buttonAlways}
          onClick={() => props.onDecide({ behavior: 'allow', remember: true })}
        >
          Always allow this tool
        </button>
      </div>
    </div>
  );
};
