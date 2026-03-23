import { onMount, onCleanup } from 'solid-js';

export interface KeyboardShortcutCallbacks {
  onOpen?: () => void;
  onReEvaluate?: () => void;
  onExportDialog?: () => void;
  onHelp?: () => void;
  onReloadShortcut?: () => void;
  onDismissReload?: () => void;
  onToggleChatPanel?: () => void;
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

    // Ctrl+J — Toggle chat panel
    if (e.ctrlKey && e.key === 'j') {
      e.preventDefault();
      callbacks.onToggleChatPanel?.();
      return;
    }

    // ? — Toggle keyboard help (only without modifier keys)
    if (e.key === '?' && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      callbacks.onHelp?.();
      return;
    }

    // Ctrl+Shift+R — Reload changed files
    if (e.ctrlKey && e.shiftKey && (e.key === 'R' || e.key === 'r')) {
      e.preventDefault();
      callbacks.onReloadShortcut?.();
      return;
    }

    // Escape — Dismiss reload prompt
    if (e.key === 'Escape') {
      e.preventDefault();
      callbacks.onDismissReload?.();
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
