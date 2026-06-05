/**
 * Unit tests for gui/src/debug/consoleErrors.ts — the always-on console-error ring buffer.
 *
 * JSDOM constraints addressed:
 * - PromiseRejectionEvent is not reliably constructible in jsdom; use
 *   Object.assign(new Event('unhandledrejection'), {reason: new Error(...)}).
 * - ErrorEvent IS supported for the window 'error' case.
 *
 * Test-structure note: installConsoleErrorCapture() is idempotent (module-level flag).
 * We call it in beforeAll to install once, then use beforeEach(clearConsoleErrors)
 * to reset the buffer. This avoids the pattern of spy.mockRestore() removing the
 * wrapper (spy-first, install-second requires restore which removes the wrapper).
 */
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest';
import {
  installConsoleErrorCapture,
  getConsoleErrors,
  clearConsoleErrors,
} from '../debug/consoleErrors';

// cap must match the module implementation (200)
const CAP = 200;

// Install once before any test runs — the wrapper stays for all tests in this file.
// vi.spyOn after install wraps our wrapper; spy.mockRestore() restores to our wrapper,
// not the true original, so the wrapper remains active.
beforeAll(() => {
  installConsoleErrorCapture();
});

beforeEach(() => {
  clearConsoleErrors();
});

describe('console.error capture', () => {
  it('pushes an entry on console.error and calls through to the original', () => {
    // Spy is installed ON TOP of our already-installed wrapper.
    // spy.mockRestore() will restore console.error back to our wrapper.
    const spy = vi.spyOn(console, 'error');

    const errObj = new Error('original-error');
    console.error('boom', errObj);

    const errors = getConsoleErrors();
    expect(errors.length).toBeGreaterThanOrEqual(1);
    const entry = errors.find((e) => e.message.includes('boom'));
    expect(entry).toBeDefined();
    expect(entry!.source).toBe('console.error');
    expect(entry!.stack).toBe(errObj.stack);

    // Spy was called — proves our wrapper called through (spy sits above wrapper
    // in the call chain; spy is called, then spy delegates to our wrapper)
    expect(spy).toHaveBeenCalled();

    // Restore spy to our wrapper (not the true original) — wrapper stays active
    spy.mockRestore();
  });

  it('pushes an entry on console.warn with source console.warn', () => {
    console.warn('warn-message');

    const errors = getConsoleErrors();
    const entry = errors.find((e) => e.source === 'console.warn');
    expect(entry).toBeDefined();
    expect(entry!.message).toContain('warn-message');
  });
});

describe('window event capture', () => {
  it('captures window error event with source onerror', () => {
    const err = new Error('window-error-x');
    const evt = new ErrorEvent('error', { message: 'window-error-x', error: err });
    window.dispatchEvent(evt);

    const errors = getConsoleErrors();
    const entry = errors.find((e) => e.source === 'onerror');
    expect(entry).toBeDefined();
    expect(entry!.message).toContain('window-error-x');
    expect(entry!.stack).toBe(err.stack);
  });

  it('captures unhandledrejection event with source unhandledrejection', () => {
    const reason = new Error('reject me');
    // jsdom lacks PromiseRejectionEvent — fabricate via Object.assign
    const evt = Object.assign(new Event('unhandledrejection'), { reason });
    window.dispatchEvent(evt);

    const errors = getConsoleErrors();
    const entry = errors.find((e) => e.source === 'unhandledrejection');
    expect(entry).toBeDefined();
    expect(entry!.message).toContain('reject me');
    expect(entry!.stack).toBe(reason.stack);
  });
});

describe('idempotency', () => {
  it('calling installConsoleErrorCapture() twice does NOT double-capture', () => {
    // First install already ran in beforeAll; second call here must be a no-op
    installConsoleErrorCapture(); // second call

    console.error('single-event');

    const errors = getConsoleErrors();
    const matching = errors.filter((e) => e.message.includes('single-event'));
    // Should appear exactly once
    expect(matching.length).toBe(1);
  });

  it('does not recurse on console.error calls inside the patch', () => {
    // Install capture (already done in beforeAll — this is a no-op); then trigger
    installConsoleErrorCapture();
    expect(() => console.error('no-recurse')).not.toThrow();
  });
});

describe('buffer cap', () => {
  it('caps the buffer at CAP entries, dropping the oldest', () => {
    // Push CAP + 10 entries
    for (let i = 0; i < CAP + 10; i++) {
      console.error(`entry-${i}`);
    }

    const errors = getConsoleErrors();
    expect(errors.length).toBe(CAP);

    // The oldest entries (entry-0 through entry-9) should have been dropped
    const hasOldest = errors.some((e) => e.message.includes('entry-0 ') || e.message === 'entry-0');
    expect(hasOldest).toBe(false);

    // The newest entries should be present
    const hasNewest = errors.some((e) => e.message.includes(`entry-${CAP + 9}`));
    expect(hasNewest).toBe(true);
  });
});

describe('getConsoleErrors returns a copy', () => {
  it('mutating the returned array does not affect the buffer', () => {
    console.error('buffer-test');

    const copy = getConsoleErrors();
    const originalLength = copy.length;
    copy.length = 0; // clear the copy

    // Buffer should be unchanged
    const second = getConsoleErrors();
    expect(second.length).toBe(originalLength);
  });
});

describe('clearConsoleErrors', () => {
  it('empties the buffer', () => {
    console.error('to-be-cleared');

    const before = getConsoleErrors();
    expect(before.length).toBeGreaterThan(0);

    clearConsoleErrors();

    const after = getConsoleErrors();
    expect(after.length).toBe(0);
  });
});

describe('entry shape', () => {
  it('entries have timestamp as a number', () => {
    console.error('timestamp-test');

    const errors = getConsoleErrors();
    const entry = errors.find((e) => e.message.includes('timestamp-test'));
    expect(entry).toBeDefined();
    expect(typeof entry!.timestamp).toBe('number');
  });

  it('stack is null when no Error object is present', () => {
    console.error('no-error-object', 42);

    const errors = getConsoleErrors();
    const entry = errors.find((e) => e.message.includes('no-error-object'));
    expect(entry).toBeDefined();
    expect(entry!.stack).toBeNull();
  });
});
