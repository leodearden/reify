import { type Component, createSignal, onCleanup, Show } from 'solid-js';
import styles from './ContextPicker.module.css';

export type ContextType = 'selection' | 'diagnostics' | 'constraints' | 'file';

export interface ContextPickerProps {
  onAttach: (type: ContextType) => void;
  hasSelection: boolean;
  hasDiagnostics: boolean;
  hasViolatedConstraints: boolean;
  hasActiveFile: boolean;
}

interface ContextOption {
  type: ContextType;
  label: string;
  available: () => boolean;
}

export const ContextPicker: Component<ContextPickerProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  const options: ContextOption[] = [
    { type: 'selection', label: 'Current selection', available: () => props.hasSelection },
    { type: 'diagnostics', label: 'Active diagnostics', available: () => props.hasDiagnostics },
    { type: 'constraints', label: 'Violated constraints', available: () => props.hasViolatedConstraints },
    { type: 'file', label: 'Current file', available: () => props.hasActiveFile },
  ];

  function handleClickOutside(e: MouseEvent) {
    if (containerRef && !containerRef.contains(e.target as Node)) {
      setOpen(false);
    }
  }

  function handleToggle() {
    const willOpen = !open();
    setOpen(willOpen);
    if (willOpen) {
      document.addEventListener('click', handleClickOutside, { once: true });
    }
  }

  function handleSelect(type: ContextType) {
    props.onAttach(type);
    setOpen(false);
  }

  onCleanup(() => {
    document.removeEventListener('click', handleClickOutside);
  });

  return (
    <div class={styles.container} ref={containerRef}>
      <button
        class={styles.btn}
        data-testid="context-picker-btn"
        aria-haspopup="menu"
        aria-expanded={open() ? 'true' : 'false'}
        aria-label="Attach context"
        title="Attach context"
        onClick={(e) => {
          e.stopPropagation();
          handleToggle();
        }}
      >
        + context
      </button>
      <Show when={open()}>
        <div class={styles.dropdown} data-testid="context-picker-dropdown">
          {options.map((opt) => (
            <button
              class={styles.option}
              disabled={!opt.available()}
              onClick={() => handleSelect(opt.type)}
            >
              {opt.label}
            </button>
          ))}
        </div>
      </Show>
    </div>
  );
};
