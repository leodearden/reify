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

/**
 * Ask until an answer describes the text on screen, asking at most twice.
 *
 * Every ask is bracketed by `syncServer`: before it, so the server answers
 * about the text on screen; after it, so an edit typed while the answer was in
 * flight has become a version bump by the time `isStale` judges. An answer
 * `isStale` accepts, null included, is returned as is; a second stale answer
 * yields null, since a typing user can invalidate every answer in turn.
 */
export async function askFresh<T>(
  guard: VersionSkewGuard,
  view: EditorView,
  ask: () => Promise<T>,
  isStale: (answer: T) => boolean,
): Promise<T | null> {
  const syncedAnswer = async (): Promise<T> => {
    await guard.syncServer(view);
    const answer = await ask();
    await guard.syncServer(view);
    return answer;
  };
  const first = await syncedAnswer();
  if (!isStale(first)) return first;
  const second = await syncedAnswer();
  return isStale(second) ? null : second;
}
