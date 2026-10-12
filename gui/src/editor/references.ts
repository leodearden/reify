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
 *
 * An optional VersionSkewGuard keeps the panel from showing ranges computed
 * against text the user has since edited: a stale answer is re-asked once and
 * otherwise dropped (see resolveFreshLocations).
 */
import type { EditorView } from '@codemirror/view';
import { versionsDisagree, type VersionSkewGuard } from './documentVersions';
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

/** One Location as the Find-uses panel shows it, with its best-effort preview. */
function toReferenceResult(view: EditorView, location: Location): ReferenceResult {
  return {
    uri: location.uri,
    line: location.range.start.line,
    character: location.range.start.character,
    endLine: location.range.end.line,
    endCharacter: location.range.end.character,
    preview: previewForLocation(view, location),
  };
}

/**
 * Request references whose ranges still describe the text on screen,
 * re-issuing ONCE on version skew.
 *
 * LSP `Location` is `{uri, range}` with no version field, so a
 * textDocument/references answer cannot be versioned at the protocol level, and
 * inventing an out-of-band channel would be protocol divergence (#7118). The
 * client therefore records the version it had SENT when it asked and compares
 * it with the version it holds when the answer arrives — purely client-side.
 *
 * The second `syncServer` is what makes an edit typed during the request, still
 * inside the didChange debounce, visible as a version bump; without it both
 * readings agree while the text on screen has moved.
 *
 * The re-issue is bounded at one, mirroring rename's resolveApplicableEdit: a
 * typing user can invalidate every answer, so a second stale answer is dropped
 * (null) rather than looped on. An empty Location[] is a fresh answer; only
 * null means stale. With no `guard` the single answer passes straight through.
 */
async function resolveFreshLocations(
  requestReferences: () => Promise<Location[]>,
  view: EditorView,
  uri: string,
  guard?: VersionSkewGuard,
): Promise<Location[] | null> {
  if (!guard) return requestReferences();
  const request = async (): Promise<Location[] | null> => {
    await guard.syncServer(view);
    const askedAt = guard.currentVersion(uri);
    const locations = await requestReferences();
    await guard.syncServer(view);
    return versionsDisagree(askedAt, guard.currentVersion(uri)) ? null : locations;
  };
  return (await request()) ?? request();
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
 * An answer that arrives after a switch to another file is dropped: the editor
 * reuses one EditorView across files, so previews would be read from, and the
 * panel opened over, another document.
 *
 * @param uriGetter  Getter for the current document URI (re-resolved at call time).
 * @param client     LSP client (only its `references` method is used).
 * @param onResults  Receives the mapped ReferenceResult[] (possibly empty).
 * @param guard      Arms the version-skew guard (see resolveFreshLocations);
 *                   omitted = one unguarded request.
 */
export function findUsesCommand(
  uriGetter: () => string,
  client: Pick<LspClient, 'references'>,
  onResults: (results: ReferenceResult[]) => void,
  guard?: VersionSkewGuard,
): (view: EditorView) => boolean {
  return (view: EditorView): boolean => {
    const head = view.state.selection.main.head;
    const line = view.state.doc.lineAt(head);
    const lspLine = line.number - 1;
    const lspChar = head - line.from;
    const uri = uriGetter();

    resolveFreshLocations(() => client.references(uri, lspLine, lspChar, true), view, uri, guard)
      .then((locations) => {
        // Checked after the LAST await, guard or not: Editor forgets the old
        // URI's version on a switch, which leaves the version comparison blind.
        if (!locations || uriGetter() !== uri) return;
        onResults(locations.map((location) => toReferenceResult(view, location)));
      })
      .catch((err) => console.warn('findUses: failed to collect references', err));

    return true; // Always consume the key
  };
}
