/**
 * Always-on, bounded ring buffer for capturing browser-side console errors
 * and unhandled exceptions.
 *
 * Installed early (at index.tsx entry top, BEFORE applyTheme/render) so that
 * startup errors are captured before the late REIFY_DEBUG-gated initDebugBridge.
 *
 * Design decisions:
 * - Module-global singleton (one buffer per window/worker) — mirrors testMode.ts.
 * - Idempotent install: a module-level `installed` flag prevents double-patching.
 * - Passthrough: patched console.error/warn call the original so normal logging
 *   is not suppressed.
 * - Buffer cap: oldest entries are dropped when the cap is exceeded (ring semantics).
 * - NOT gated on REIFY_DEBUG — the buffer must fill regardless, because
 *   list_console_errors may be called before the debug bridge is up.
 */

export type ConsoleErrorEntry = {
  source: 'onerror' | 'unhandledrejection' | 'console.error' | 'console.warn';
  message: string;
  stack: string | null;
  timestamp: number;
};

const CAP = 200;
const buffer: ConsoleErrorEntry[] = [];

// Prevent double-install across HMR re-imports or duplicate module loads.
let installed = false;

function push(entry: ConsoleErrorEntry): void {
  if (buffer.length >= CAP) {
    buffer.shift(); // drop oldest
  }
  buffer.push(entry);
}

function extractStack(args: unknown[]): string | null {
  for (const arg of args) {
    if (arg instanceof Error && typeof arg.stack === 'string') {
      return arg.stack;
    }
  }
  return null;
}

function formatMessage(args: unknown[]): string {
  return args.map((a) => String(a)).join(' ');
}

/**
 * Install the console-capture ring buffer.
 * Idempotent: calling multiple times is safe (only installs once).
 */
export function installConsoleErrorCapture(): void {
  if (installed) return;
  installed = true;

  // Capture originals BEFORE patching so the wrappers delegate correctly.
  const origError = console.error.bind(console);
  const origWarn = console.warn.bind(console);

  console.error = (...args: unknown[]) => {
    push({
      source: 'console.error',
      message: formatMessage(args),
      stack: extractStack(args),
      timestamp: Date.now(),
    });
    origError(...args);
  };

  console.warn = (...args: unknown[]) => {
    push({
      source: 'console.warn',
      message: formatMessage(args),
      stack: extractStack(args),
      timestamp: Date.now(),
    });
    origWarn(...args);
  };

  window.addEventListener('error', (evt: ErrorEvent) => {
    push({
      source: 'onerror',
      message: evt.message,
      stack: (evt.error instanceof Error ? evt.error.stack : null) ?? null,
      timestamp: Date.now(),
    });
  });

  window.addEventListener('unhandledrejection', (evt: Event) => {
    // Cast to PromiseRejectionEvent-like — jsdom lacks PromiseRejectionEvent
    // so we access `.reason` via a duck-typed cast.
    const reason = (evt as unknown as { reason?: unknown }).reason;
    const msg = reason instanceof Error ? reason.message : String(reason ?? 'Unhandled rejection');
    const stack = reason instanceof Error ? (reason.stack ?? null) : null;
    push({
      source: 'unhandledrejection',
      message: msg,
      stack,
      timestamp: Date.now(),
    });
  });
}

/**
 * Return a shallow copy of the current buffer.
 * Mutating the returned array does not affect the internal buffer.
 */
export function getConsoleErrors(): ConsoleErrorEntry[] {
  return [...buffer];
}

/**
 * Empty the buffer.
 */
export function clearConsoleErrors(): void {
  buffer.length = 0;
}
