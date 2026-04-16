import { onMount, onCleanup } from 'solid-js';
import { SHORTCUTS, matchesEvent } from '../shortcuts';

export interface KeyboardShortcutCallbacks {
  onOpen?: () => void;
  onSave?: () => void;
  onReEvaluate?: () => void;
  onExportDialog?: () => void;
  onHelp?: () => void;
  onReloadShortcut?: () => void;
  onDismissReload?: () => void;
  onToggleChatPanel?: () => void;
}

/**
 * Internal map from shortcut id to the corresponding callback key.
 * Shortcuts without a callback (undo, redo, fitToView) are omitted —
 * the registry loop skips them when no entry is found here.
 */
const ID_TO_CALLBACK: Partial<Record<string, keyof KeyboardShortcutCallbacks>> = {
  open:        'onOpen',
  save:        'onSave',
  export:      'onExportDialog',
  reEvaluate:  'onReEvaluate',
  toggleChat:  'onToggleChatPanel',
  reload:      'onReloadShortcut',
  help:        'onHelp',
};

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

    // Registry-driven matching
    for (const shortcut of SHORTCUTS) {
      if (!shortcut.bind) continue;
      if (!matchesEvent(shortcut.bind, e)) continue;
      const callbackKey = ID_TO_CALLBACK[shortcut.id];
      if (!callbackKey) continue;
      e.preventDefault();
      callbacks[callbackKey]?.();
      return;
    }

    // Escape — Dismiss reload prompt.
    // Handled separately: Escape is a UI-dismiss action for a specific prompt,
    // not a formal application shortcut shown in the KeyboardHelp overlay.
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
