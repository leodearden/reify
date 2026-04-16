/**
 * Centralized keyboard shortcut registry.
 *
 * This is the single source of truth for shortcut definitions consumed by
 * both MenuBar (for hotkey annotations) and KeyboardHelp (for the overlay).
 */

export interface ShortcutDef {
  /** Unique identifier for the shortcut */
  id: string;
  /** Display key combination (e.g. "Ctrl+O", "F5", "?"). Empty string if no shortcut. */
  key: string;
  /** Human-readable description of the action */
  description: string;
  /** When true, the shortcut is defined but not currently functional */
  disabled?: boolean;
}

export const SHORTCUTS: ShortcutDef[] = [
  { id: 'open', key: 'Ctrl+O', description: 'Open file' },
  { id: 'save', key: 'Ctrl+S', description: 'Save file' },
  { id: 'export', key: 'Ctrl+E', description: 'Export' },
  { id: 'undo', key: 'Ctrl+Z', description: 'Undo', disabled: true },
  { id: 'redo', key: 'Ctrl+Shift+Z', description: 'Redo', disabled: true },
  { id: 'reEvaluate', key: 'F5', description: 'Re-evaluate' },
  { id: 'fitToView', key: '', description: 'Fit to view' },
  { id: 'toggleChat', key: 'Ctrl+J', description: 'Toggle chat panel' },
  { id: 'reload', key: 'Ctrl+Shift+R', description: 'Reload changed files' },
  { id: 'help', key: '?', description: 'Toggle this help' },
];

/**
 * Look up a shortcut definition by id.
 * Returns `undefined` if no entry with that id exists.
 */
export function getShortcut(id: string): ShortcutDef | undefined {
  return SHORTCUTS.find((s) => s.id === id);
}

/**
 * Returns the display key string for a shortcut id.
 * Returns an empty string if the id is not found or has no key.
 */
export function shortcutKey(id: string): string {
  return getShortcut(id)?.key ?? '';
}
