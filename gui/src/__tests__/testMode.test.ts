/**
 * Unit tests for the testMode signal module (gui/src/debug/testMode.ts).
 * Step 1: these tests fail because the module does not exist yet.
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { createRoot, createEffect } from 'solid-js';

import { testMode, setTestMode } from '../debug/testMode';

describe('testMode signal', () => {
  beforeEach(() => {
    setTestMode(false);
  });

  it('defaults to false', () => {
    createRoot((dispose) => {
      expect(testMode()).toBe(false);
      dispose();
    });
  });

  it('setTestMode(true) makes testMode() return true', () => {
    createRoot((dispose) => {
      setTestMode(true);
      expect(testMode()).toBe(true);
      dispose();
    });
  });

  it('setTestMode(false) flips it back', () => {
    createRoot((dispose) => {
      setTestMode(true);
      expect(testMode()).toBe(true);
      setTestMode(false);
      expect(testMode()).toBe(false);
      dispose();
    });
  });

  it('createEffect re-runs when the signal changes', async () => {
    let runCount = 0;
    let disposeRef!: () => void;

    createRoot((dispose) => {
      disposeRef = dispose;
      createEffect(() => {
        void testMode(); // establish reactive dependency
        runCount++;
      });
    });

    // Flush initial effect subscription
    await Promise.resolve();
    expect(runCount).toBe(1); // initial run

    setTestMode(true);
    await Promise.resolve();
    expect(runCount).toBe(2); // re-ran on signal change

    setTestMode(false);
    await Promise.resolve();
    expect(runCount).toBe(3); // re-ran again

    disposeRef();
  });
});
