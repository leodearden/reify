/**
 * PRIMARY-selection guard CodeMirror extension (task #5361).
 *
 * On Linux/X11 the WebKitGTK webview re-asserts ownership of the X11 PRIMARY
 * selection on its caret-blink/redraw cycle for as long as the CodeMirror
 * editor holds a NON-EMPTY native DOM selection — even while the GUI window is
 * unfocused. That stomps every other application's PRIMARY selection
 * desktop-wide (middle-click paste everywhere yields the editor's last
 * selection). reify ships zero custom clipboard/PRIMARY code and cannot patch
 * WebKitGTK, so this is an app-side mitigation:
 *
 *   - On editor `blur`, collapse the NATIVE DOM selection (removeAllRanges) so
 *     the webview no longer has a non-empty selection to re-announce.
 *   - Activate CodeMirror's `drawSelection()` so the user's selection stays
 *     VISIBLE (CM paints its own selection layer, independent of the native
 *     selection) while the native selection is collapsed.
 *   - On editor `focus`, rebuild the native selection from
 *     `view.state.selection.main` so PRIMARY reflects the editor selection
 *     again once the user is back in the editor.
 *
 * `view.state.selection` is NEVER mutated — only the native DOM selection is
 * touched — so undo/redo, save, cursor reporting, occurrence highlights and the
 * drawSelection visual are all unaffected. The X11 PRIMARY behaviour itself is
 * verified MANUALLY (jsdom has no X11); these helpers pin only the reproducible
 * app-side logic.
 *
 * Mirrors occurrenceHighlight.ts: a pure helper plus a factory returning an
 * Extension, with all DOM/view API calls kept inside the factory closure.
 */
import type { EditorState } from '@codemirror/state';

/**
 * True iff the editor has an active NON-EMPTY selection — any selection range
 * whose `from !== to` (equivalently `!range.empty`). Both handlers gate on this:
 * an all-caret / all-empty multi-cursor selection is nothing PRIMARY would
 * carry, so there is nothing to collapse or rebuild. Pure; no DOM access.
 */
export function hasNonEmptySelection(state: EditorState): boolean {
  return state.selection.ranges.some((r) => r.from !== r.to);
}
