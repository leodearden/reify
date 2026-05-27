/**
 * Centralised save-blocked user-facing messages.
 *
 * Both call sites that handle a Ctrl+S / Mod-s save action (App.tsx#handleSave
 * and Editor.tsx's Mod-s keymap) must show the same wording for the same
 * pre-flight policy decision.  Keeping the strings and the reason type here
 * means a future wording change, new reason, or telemetry hook touches only
 * this file rather than every call site in lockstep.
 */

/**
 * Re-exported from `editorStore` (the data layer that owns the type) so that
 * consumers can import it from either location.
 */
import type { SaveBlockedReason } from '../stores/editorStore';
export type { SaveBlockedReason };

/**
 * Shown when the user attempts to save a file that has been modified on disk
 * since it was last opened/reloaded.  The exact historical wording (em-dash
 * and all) is preserved so existing UX is unchanged.
 */
export const EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG =
  'File changed externally — reload or dismiss the prompt before saving';

/**
 * Shown when the user attempts to save a path that is not currently open in
 * the editor store.  Distinct from the externally-changed message so that
 * future diagnostics / telemetry can differentiate the two conditions.
 */
export const FILE_NOT_OPEN_SAVE_BLOCKED_MSG =
  'Cannot save: file is not open in the editor';

/**
 * Shown as the body of the conflict-prompt toast when the user attempts to
 * save a file that is externally changed.  Unlike {@link EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG}
 * (which is a dead-end error), this prompt is accompanied by action buttons
 * (see {@link SAVE_CONFLICT_RELOAD_LABEL} and {@link SAVE_CONFLICT_OVERWRITE_LABEL}).
 *
 * Consumed by both App.tsx#handleSave's conflict prompt and Editor.tsx's
 * Mod-s keymap (via the onSaveConflict prop) so wording is a single source of
 * truth for both call sites.
 */
export const EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG =
  'File changed externally — choose Reload from disk or Overwrite';

/**
 * Label for the "reload from disk" action button in the save conflict prompt.
 * Consumed by App.tsx#showSaveConflictPrompt and Editor.tsx Mod-s keymap.
 */
export const SAVE_CONFLICT_RELOAD_LABEL = 'Reload from disk';

/**
 * Label for the "overwrite" action button in the save conflict prompt.
 * Consumed by App.tsx#showSaveConflictPrompt and Editor.tsx Mod-s keymap.
 */
export const SAVE_CONFLICT_OVERWRITE_LABEL = 'Overwrite';

/**
 * Maps a {@link SaveBlockedReason} to the appropriate user-facing message.
 *
 * The switch is exhaustive — TypeScript will raise a type error if a new
 * reason is added to {@link SaveBlockedReason} without updating this function.
 */
export function messageForSaveBlocked(reason: SaveBlockedReason): string {
  switch (reason) {
    case 'externally-changed':
      return EXTERNALLY_CHANGED_SAVE_BLOCKED_MSG;
    case 'not-found':
      return FILE_NOT_OPEN_SAVE_BLOCKED_MSG;
  }
}
