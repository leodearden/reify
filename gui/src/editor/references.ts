/**
 * CodeMirror Shift+F12 "Find uses" command powered by the LSP server.
 *
 * Exports findUsesCommand — a CodeMirror Command factory that reads the cursor
 * from view.state.selection.main.head, sends a textDocument/references request
 * (declaration ∪ uses, scoped to the open document's enclosing entity body by
 * the α reference-collector), maps the resulting LSP Location[] into
 * ReferenceResult[], and forwards them to a callback so the GUI can populate
 * the Find-uses panel.
 *
 * Mirrors gotoDefinitionCommand (gotoDefinition.ts): no @codemirror/view or
 * @codemirror/state runtime imports are added here — the keymap.of wrapping
 * lives in Editor.tsx. The LspClient dependency is injected (Pick<…,'references'>)
 * so the command is unit-testable without the Tauri bridge.
 */
import type { EditorView } from '@codemirror/view';
import type { Location, LspClient } from './lspClient';

/**
 * A single reference occurrence, in native LSP (0-based) coordinates.
 *
 * The +1 conversion to the editor's 1-based SourceLocation happens later, at
 * the App click handler that drives setScrollToLocation — not here, so the
 * panel and command stay in LSP-native coordinates end to end.
 */
export interface ReferenceResult {
  uri: string;
  line: number;
  character: number;
  endLine: number;
  endCharacter: number;
  preview?: string;
}

/**
 * Best-effort, guarded preview of the source line a reference sits on.
 *
 * Returns the trimmed text of `doc.line(loc.range.start.line + 1)` (LSP is
 * 0-based, CodeMirror Line numbers are 1-based), or `undefined` if the line is
 * out of range / empty. Never throws: a bad single line must not drop the
 * whole result set.
 */
function previewForLocation(view: EditorView, location: Location): string | undefined {
  try {
    const trimmed = view.state.doc.line(location.range.start.line + 1).text.trim();
    return trimmed.length > 0 ? trimmed : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Create a CodeMirror Command for Shift+F12 "Find uses".
 *
 * Returns a `(view: EditorView) => boolean` suitable for use in keymap.of.
 * Reads the cursor position from view.state.selection.main.head, requests
 * references for it (includeDeclaration=true), maps Location[] → ReferenceResult[],
 * and calls `onResults`. Errors are swallowed (a warning is logged) so a failed
 * request never bubbles into CodeMirror; the command always returns true to
 * consume the key.
 *
 * @param uriGetter  Getter for the current document URI (re-resolved at call time).
 * @param client     LSP client (only its `references` method is used).
 * @param onResults  Receives the mapped ReferenceResult[] (possibly empty).
 */
export function findUsesCommand(
  uriGetter: () => string,
  client: Pick<LspClient, 'references'>,
  onResults: (results: ReferenceResult[]) => void,
): (view: EditorView) => boolean {
  return (view: EditorView): boolean => {
    const head = view.state.selection.main.head;
    const line = view.state.doc.lineAt(head);
    const lspLine = line.number - 1;
    const lspChar = head - line.from;
    const currentUri = uriGetter();

    client
      .references(currentUri, lspLine, lspChar, true)
      .then((locations) => {
        const results: ReferenceResult[] = locations.map((loc) => ({
          uri: loc.uri,
          line: loc.range.start.line,
          character: loc.range.start.character,
          endLine: loc.range.end.line,
          endCharacter: loc.range.end.character,
          preview: previewForLocation(view, loc),
        }));
        onResults(results);
      })
      .catch((err) => console.warn('findUses: failed to collect references', err));

    return true; // Always consume the key
  };
}
