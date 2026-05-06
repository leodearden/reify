// Module-global Solid signal that tracks whether the debug bridge is in
// test-mode (CSS animations/transitions frozen for pixel-stable screenshots).
//
// Module-global (not a factory) because the debug bridge itself is a
// process-wide singleton — one MCP server, one window.__REIFY_DEBUG__.
// Vitest's per-file module isolation prevents cross-test leakage; a
// beforeEach(() => setTestMode(false)) call in each test file is sufficient.

import { createSignal } from 'solid-js';
import type { Accessor } from 'solid-js';

const [_testMode, _setTestMode] = createSignal<boolean>(false);

/** Reactive accessor — returns true when the debug test-mode is enabled. */
export const testMode: Accessor<boolean> = _testMode;

/** Toggle test-mode on or off. Called by the set_test_mode bridge handler. */
export function setTestMode(enabled: boolean): void {
  _setTestMode(enabled);
}
