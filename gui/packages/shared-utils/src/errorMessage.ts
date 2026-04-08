/** Extract a human-readable message from an unknown thrown value. */
export function errorMessage(err: unknown): string {
  try {
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
  } catch {
    // Any branch above can throw on hostile inputs: an Error subclass with a
    // throwing .message getter, a Proxy whose get/has trap throws, or a value
    // whose toString()/valueOf() throws during String() coercion. The function
    // contract is to always return a displayable string and never throw.
    return 'Unknown error';
  }
}
