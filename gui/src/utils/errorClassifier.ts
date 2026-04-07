export interface ClassifiedError {
  type: 'auth' | 'rate-limit' | 'network' | 'sidecar' | 'unknown';
  userMessage: string;
}

interface ErrorRule {
  pattern: RegExp;
  type: ClassifiedError['type'];
  userMessage: string;
}

const rules: ErrorRule[] = [
  {
    pattern: /auth|unauthorized|401/i,
    type: 'auth',
    userMessage: 'Authentication required. Run `claude login` in your terminal.',
  },
  {
    pattern: /rate.?limit|429/i,
    type: 'rate-limit',
    userMessage: 'Rate limited. Please wait and try again.',
  },
  {
    pattern: /disconnect|crash|exit|spawn/i,
    type: 'sidecar',
    userMessage: 'Claude session disconnected. Click to restart.',
  },
  {
    pattern: /network|connect|ECONNREFUSED|fetch/i,
    type: 'network',
    userMessage: 'Connection failed. Check your network.',
  },
];

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

export function classifyError(message: string): ClassifiedError {
  for (const rule of rules) {
    if (rule.pattern.test(message)) {
      return { type: rule.type, userMessage: rule.userMessage };
    }
  }
  return { type: 'unknown', userMessage: message };
}
