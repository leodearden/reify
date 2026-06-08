import { onMount, onCleanup } from 'solid-js';
import { SHORTCUTS, matchesEvent, type ShortcutId } from '../shortcuts';

export interface KeyboardShortcutCallbacks {
  onNew?: () => void;
  onOpen?: () => void;
  onSave?: () => void;
  onReEvaluate?: () => void;
  onExportDialog?: () => void;
  onHelp?: () => void;
  onReloadShortcut?: () => void;
  onDismissReload?: () => void;
  onToggleChatPanel?: () => void;
  onClearSelection?: () => void;
  onCommandPalette?: () => void;
  onSymbolJump?: () => void;
  onToggleDiagnostics?: () => void;
  /**
   * Called when the user presses a bare digit key 1–9 (no modifiers, not in a
   * text input context). The `index` argument is 0-based: key "1" → 0, "9" → 8.
   * Consumers use this to switch to the N-th entry in the ViewSelector list.
   */
  onSwitchViewByIndex?: (index: number) => void;
}

/**
 * Source of truth for bind→callback wiring: maps each shortcut id to the
 * corresponding callback key on KeyboardShortcutCallbacks.
 *
 * A shortcut is absent when it is `disabled: true` on SHORTCUTS (undo, redo),
 * or when it has no `bind` field (fitToView). The registry loop skips any id
 * not found here.
 *
 * Keyed by ShortcutId so typos in shortcut IDs (e.g. 'toogleChat') are
 * caught at compile time rather than silently failing at runtime.
 *
 * Use `hasCallbackWiring(id)` to check membership from outside this module.
 */
export const ID_TO_CALLBACK: Partial<Record<ShortcutId, keyof KeyboardShortcutCallbacks>> = {
  new:            'onNew',
  open:           'onOpen',
  save:           'onSave',
  export:         'onExportDialog',
  reEvaluate:     'onReEvaluate',
  toggleChat:     'onToggleChatPanel',
  reload:         'onReloadShortcut',
  help:           'onHelp',
  commandPalette:    'onCommandPalette',
  symbolJump:        'onSymbolJump',
  toggleDiagnostics: 'onToggleDiagnostics',
};

/**
 * Shortcut ids that are exempt from the typing-context guard (input/textarea/
 * contentEditable). These are global palette-open actions that must fire from
 * any focus state, including while the CodeMirror editor is focused.
 */
const PALETTE_GLOBAL_IDS = new Set<ShortcutId>(['commandPalette', 'symbolJump']);

/**
 * Returns true when `id` has a callback wiring entry in ID_TO_CALLBACK, i.e.
 * the shortcut is neither `disabled: true` on SHORTCUTS nor missing a `bind`.
 * This is the narrow predicate the invariant test uses to check membership
 * without depending on the mapping's value shape.
 */
export function hasCallbackWiring(id: ShortcutId): boolean {
  return ID_TO_CALLBACK[id] !== undefined;
}

export interface PaletteCommand {
  id: ShortcutId;
  title: string;
  key: string;
}

/**
 * Returns the list of commands available in the command palette.
 * Derived from SHORTCUTS entries that have callback wiring, excluding the two
 * palette-control ids themselves (commandPalette, symbolJump) to prevent a
 * recursive "open command palette" entry appearing inside the palette.
 */
export function paletteCommands(): PaletteCommand[] {
  return SHORTCUTS.filter(
    (s) => hasCallbackWiring(s.id) && !PALETTE_GLOBAL_IDS.has(s.id),
  ).map((s) => ({ id: s.id, title: s.description, key: s.key }));
}

/**
 * Invoke the callback for a given shortcut id against the provided callbacks
 * object. If the id has no wiring, this is a no-op.
 */
export function runCommand(id: ShortcutId, callbacks: KeyboardShortcutCallbacks): void {
  const callbackKey = ID_TO_CALLBACK[id];
  if (!callbackKey) return;
  (callbacks[callbackKey] as (() => void) | undefined)?.();
}

/**
 * Registers global keyboard shortcuts on mount and removes them on cleanup.
 * Skips when the event target is an input, textarea, or contenteditable element,
 * EXCEPT for palette-global shortcuts (commandPalette, symbolJump) which are
 * exempt from the typing-context guard.
 */
export function useKeyboardShortcuts(callbacks: KeyboardShortcutCallbacks): void {
  function handleKeyDown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const tagName = target.tagName?.toLowerCase();
    const inTypingContext =
      tagName === 'input' ||
      tagName === 'textarea' ||
      target.isContentEditable;

    // Registry-driven matching.
    // Array order in SHORTCUTS determines priority: if two bindings could
    // match the same event, the earlier entry fires and the loop returns.
    for (const shortcut of SHORTCUTS) {
      if (!shortcut.bind) continue;
      if (!matchesEvent(shortcut.bind, e)) continue;
      const callbackKey = ID_TO_CALLBACK[shortcut.id];
      if (!callbackKey) continue;

      // Palette-global shortcuts bypass the typing-context guard.
      // All other shortcuts respect the guard (do not fire in inputs/editors).
      if (inTypingContext && !PALETTE_GLOBAL_IDS.has(shortcut.id)) continue;

      e.preventDefault();
      // All entries in ID_TO_CALLBACK map to zero-argument callbacks.
      // Cast to silence TypeScript's union-type inference for the parameterised
      // onSwitchViewByIndex callback (which is handled via its own special-case
      // block below, not through this registry loop).
      (callbacks[callbackKey] as (() => void) | undefined)?.();
      return;
    }

    // Skip remaining special-case handlers when typing in form elements.
    if (inTypingContext) return;

    // Escape — Dismiss reload prompt, then clear selection.
    // Handled separately: Escape is a UI-dismiss action, not a formal application
    // shortcut shown in the KeyboardHelp overlay.
    if (e.key === 'Escape') {
      e.preventDefault();
      callbacks.onDismissReload?.();
      callbacks.onClearSelection?.();
      return;
    }

    // 1–9 number-key view switch (VM-6).
    // Handled as a special case (no bind on the switchViewByIndex SHORTCUTS entry)
    // because the key is a dynamic range, not a literal binding.  Only fires when
    // no modifier is held, mirroring the restriction on the registry-driven loop.
    if (/^[1-9]$/.test(e.key) && !e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      callbacks.onSwitchViewByIndex?.(parseInt(e.key, 10) - 1);
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
