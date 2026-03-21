import { onMount, onCleanup } from 'solid-js';

export interface KeyboardShortcutCallbacks {
  onOpen?: () => void;
  onReEvaluate?: () => void;
  onExportDialog?: () => void;
}

/**
 * Registers global keyboard shortcuts on mount and removes them on cleanup.
 * Skips when the event target is an input, textarea, or contenteditable element.
 */
export function useKeyboardShortcuts(callbacks: KeyboardShortcutCallbacks): void {
  function handleKeyDown(e: KeyboardEvent) {
    // Skip when typing in form elements
    const target = e.target as HTMLElement;
    const tagName = target.tagName?.toLowerCase();
    if (
      tagName === 'input' ||
      tagName === 'textarea' ||
      target.isContentEditable
    ) {
      return;
    }

    // Ctrl+O — Open file
    if (e.ctrlKey && e.key === 'o') {
      e.preventDefault();
      callbacks.onOpen?.();
      return;
    }

    // F5 — Re-evaluate
    if (e.key === 'F5') {
      e.preventDefault();
      callbacks.onReEvaluate?.();
      return;
    }

    // Ctrl+E — Export dialog
    if (e.ctrlKey && e.key === 'e') {
      e.preventDefault();
      callbacks.onExportDialog?.();
      return;
    }
  }

  onMount(() => {
    document.addEventListener('keydown', handleKeyDown);
  });

  onCleanup(() => {
    document.removeEventListener('keydown', handleKeyDown);
  });
}
