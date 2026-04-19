import { onMount, onCleanup } from 'solid-js';
import { SHORTCUTS, matchesEvent, type ShortcutId } from '../shortcuts';

export interface KeyboardShortcutCallbacks {
  onOpen?: () => void;
  onSave?: () => void;
  onReEvaluate?: () => void;
  onExportDialog?: () => void;
  onHelp?: () => void;
  onReloadShortcut?: () => void;
  onDismissReload?: () => void;
  onToggleChatPanel?: () => void;
  onClearSelection?: () => void;
}

/**
 * Internal map from shortcut id to the corresponding callback key.
 * Shortcuts without a callback (undo, redo, fitToView) are omitted —
 * the registry loop skips them when no entry is found here.
 *
 * Keyed by ShortcutId so typos in shortcut IDs (e.g. 'toogleChat') are
 * caught at compile time rather than silently failing at runtime.
 */
const ID_TO_CALLBACK: Partial<Record<ShortcutId, keyof KeyboardShortcutCallbacks>> = {
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

    // Registry-driven matching.
    // Array order in SHORTCUTS determines priority: if two bindings could
    // match the same event, the earlier entry fires and the loop returns.
    for (const shortcut of SHORTCUTS) {
      if (!shortcut.bind) continue;
      if (!matchesEvent(shortcut.bind, e)) continue;
      const callbackKey = ID_TO_CALLBACK[shortcut.id];
      if (!callbackKey) continue;
      e.preventDefault();
      callbacks[callbackKey]?.();
      return;
    }

    // Escape — Dismiss reload prompt, then clear selection.
    // Handled separately: Escape is a UI-dismiss action, not a formal application
    // shortcut shown in the KeyboardHelp overlay.
    if (e.key === 'Escape') {
      e.preventDefault();
      callbacks.onDismissReload?.();
      callbacks.onClearSelection?.();
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
