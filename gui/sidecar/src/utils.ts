/** Extract a human-readable message from an unknown thrown value. */
export function errorMessage(err: unknown): string {
  if (err instanceof Error) {
    return err.message.trim() || 'Unknown error';
  }
  if (err !== null && typeof err === 'object' && typeof (err as Record<string, unknown>).message === 'string') {
    return ((err as Record<string, unknown>).message as string).trim() || 'Unknown error';
  }
  return String(err).trim() || 'Unknown error';
}
