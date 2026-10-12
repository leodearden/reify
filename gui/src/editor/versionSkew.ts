/**
 * The version-skew contract shared by every position-based LSP command: the
 * guard a command is handed, and the one rule for calling an answer stale.
 */
import type { EditorView } from '@codemirror/view';

/**
 * Reads the version the client last SENT to the server for `uri`.
 *
 * `undefined` means the client never tracked that URI — not that it is at
 * version zero.
 */
export type DocumentVersionReader = (uri: string) => number | undefined;

/**
 * What a command needs to tell an answer computed against the text on screen
 * from one computed against text the client has since moved past.
 */
export interface VersionSkewGuard {
  currentVersion: DocumentVersionReader;
  /**
   * Hand the server any local edit it has not received yet, or return null
   * when it already has the text on screen. The version comparison can only
   * see edits that were sent.
   */
  syncServer(view: EditorView): Promise<void> | null;
}

/**
 * The one definition of staleness: an answer is stale only on DEMONSTRATED
 * disagreement — both versions known and different. An unknown side makes
 * staleness unknowable, not proven.
 */
export function versionsDisagree(
  stated: number | null | undefined,
  held: number | null | undefined,
): boolean {
  if (stated === null || stated === undefined) return false;
  if (held === null || held === undefined) return false;
  return stated !== held;
}
