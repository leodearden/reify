// NOTE: kept in sync with gui/src/utils/errorClassifier.ts – sidecar is a separate bundle
/** Extract a human-readable message from an unknown thrown value. */
export function errorMessage(err: unknown): string {
  if (err instanceof Error) {
    return err.message.trim() || 'Unknown error';
  }
  if (err !== null && typeof err === 'object') {
    const msg = (err as Record<string, unknown>).message;
    if (typeof msg === 'string') {
      return msg.trim() || 'Unknown error';
    }
    if ('message' in err) {
      return 'Unknown error';
    }
  }
  return String(err).trim() || 'Unknown error';
}
