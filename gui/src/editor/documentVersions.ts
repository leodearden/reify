/**
 * Per-document LSP version counters.
 *
 * LSP versions are per TextDocument: every didOpen/didChange for a URI carries
 * the next number in THAT document's sequence, and a server stamps its rename
 * edits with the version it computed them against. Comparing those two numbers
 * is only meaningful if the client tracks one counter per document — a single
 * shared counter makes "the version I last sent for this file" unknowable.
 * This module also owns the version-skew contract every position-based command
 * shares: the guard it is handed and the one rule for calling an answer stale.
 */
import type { EditorView } from '@codemirror/view';

/**
 * The version bookkeeping for a set of open documents.
 *
 * `next` is the only way to move a version, so what the client last SENT for a
 * URI and what `current` reports can never drift apart: callers bump at exactly
 * the moment they notify the server.
 */
export interface DocumentVersions {
  /** Advance `uri` to its next version and return it (1 for a fresh document). */
  next(uri: string): number;
  /** The version last handed out for `uri`, or undefined if it has none. */
  current(uri: string): number | undefined;
  /** Drop `uri`'s counter — its next `next` starts a fresh sequence at 1. */
  forget(uri: string): void;
}

/** Create an empty set of per-document version counters. */
export function createDocumentVersions(): DocumentVersions {
  const versions = new Map<string, number>();
  return {
    next(uri: string): number {
      const version = (versions.get(uri) ?? 0) + 1;
      versions.set(uri, version);
      return version;
    },
    current(uri: string): number | undefined {
      return versions.get(uri);
    },
    forget(uri: string): void {
      versions.delete(uri);
    },
  };
}

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
   * when it already has the text on screen. Awaited before EVERY guarded
   * request, a re-issue included, because the version comparison can only see
   * edits that were sent.
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
