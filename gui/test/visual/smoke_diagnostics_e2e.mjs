#!/usr/bin/env node
/**
 * Acceptance smoke for task-4404 (GUI diagnostics ε): end-to-end MCP integration gate.
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1. Per the H0 harness contract and the find_uses precedent, this
 * smoke is run manually / via the npm script "test:smoke:diagnostics" which
 * executes run_diagnostics_e2e_smoke.sh (builds + launches the GUI then drives it).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_diagnostics_e2e.mjs
 *
 * Drives the running GUI debug server over JSON-RPC and asserts 5 scenarios:
 *
 *   1. FOLD/UNFOLD — inject 3 diagnostics; verify panel body hidden (count=0)
 *      when collapsed; click fold-toggle → count=3; click StatusBar badge → folds
 *      (count=0); unfold again.
 *   2. SPAN-LESS INERT — structural check with 3 injected diagnostics expanded:
 *      [data-testid="diagnostic-row"] count===3;
 *      [data-testid="diagnostic-row"][role="button"] count===2;
 *      exactly one [data-testid="diagnostic-location"] with text "—".
 *   3. IN-FILE JUMP — re-inject 1 line-tied (MAIN,L_MAIN) only; click row;
 *      editor_content → activeFile contains diagnostics_main AND
 *      cursorPosition.line===L_MAIN.
 *   4. CROSS-FILE — re-inject 1 line-tied (HELPER,L_HELPER) only; click row;
 *      editor_content → activeFile contains diagnostics_helper AND
 *      cursorPosition.line===L_HELPER.
 *   5. PERSISTENCE — resize_panes({problemsHeight:240}); ensure panel unfolded;
 *      sleep 350ms (>300ms debounce); get_local_storage('reify-panel-layout') →
 *      parse → problemsHeight===240 && problemsCollapsed===false.
 *
 * Exit 0 on all-pass, non-zero on any failure.
 */

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const MAIN   = path.join(REPO_ROOT, 'gui', 'test', 'fixtures', 'diagnostics_main.ri');
const HELPER = path.join(REPO_ROOT, 'gui', 'test', 'fixtures', 'diagnostics_helper.ri');

// 1-based line numbers for the injected line-tied diagnostics.
// diagnostics_main.ri: "let side = 20mm" is at line 15
// diagnostics_helper.ri: "let depth = 15mm" is at line 16
const L_MAIN   = 15;
const L_HELPER = 16;
const COL      = 7;   // column (1-based) for injected line-tied diagnostics

// ─── Port resolution (mirrors endpoint.ts / lib_portable.sh logic) ─────────────

function resolveDebugPort(env = process.env) {
  const raw = env['REIFY_DEBUG_PORT'];
  if (raw === undefined) return 3939;
  if (!/^\d+$/.test(raw)) return 3939;
  const parsed = parseInt(raw, 10);
  if (parsed < 1 || parsed > 65535) return 3939;
  return parsed;
}

const PORT = resolveDebugPort();
const DEBUG_URL = `http://127.0.0.1:${PORT}/mcp`;

// ─── Helpers ────────────────────────────────────────────────────────────────────

let stepNum = 0;
function log(msg) {
  stepNum++;
  console.log(`[step ${stepNum}] ${msg}`);
}
function fail(msg) {
  console.error(`\nFAIL: ${msg}`);
  process.exit(1);
}

async function rpc(method, args = {}) {
  const res = await fetch(DEBUG_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: { name: method, arguments: args },
    }),
  });
  const envelope = await res.json();
  if (envelope.error) throw new Error(`RPC error: ${JSON.stringify(envelope.error)}`);
  const content = envelope?.result?.content;
  if (!content || content.length === 0) return null;
  const textBlock = content.find(c => c.type === 'text');
  if (!textBlock) return null;
  try {
    return JSON.parse(textBlock.text);
  } catch {
    return textBlock.text;
  }
}

function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

async function waitForServer(timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await rpc('health');
      if (r !== null) return;
    } catch {}
    await sleep(500);
  }
  fail(`Debug server not ready on port ${PORT} after ${timeoutMs}ms`);
}

/** Count DOM elements matching a CSS selector via query_selector_all. */
async function countElements(selector) {
  const res = await rpc('query_selector_all', { selector });
  if (!res || typeof res.count !== 'number') {
    fail(`query_selector_all('${selector}') returned unexpected: ${JSON.stringify(res)}`);
  }
  return res;
}

/** Inject diagnostics into the compile slot (replaces current set). */
async function injectDiagnostics(diagnostics) {
  const r = await rpc('inject_diagnostics', { diagnostics, source: 'compile' });
  if (!r || !r.ok) fail(`inject_diagnostics failed: ${JSON.stringify(r)}`);
  await sleep(200);  // allow SolidJS reactivity to flush
}

// ─── Main ────────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`smoke_diagnostics_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`MAIN fixture:   ${MAIN}`);
  console.log(`HELPER fixture: ${HELPER}`);
  console.log(`L_MAIN=${L_MAIN}, L_HELPER=${L_HELPER}, COL=${COL}`);

  // ── Health ──────────────────────────────────────────────────────────────────
  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  // ── Boot: open MAIN fixture ──────────────────────────────────────────────────
  log('Opening diagnostics_main.ri via open_file…');
  const openResult = await rpc('open_file', { path: MAIN });
  console.log('  open_file result:', JSON.stringify(openResult));
  if (!openResult || !openResult.ok) fail(`open_file failed: ${JSON.stringify(openResult)}`);

  log('Waiting for engine idle…');
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 15000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));

  const ec0 = await rpc('editor_content');
  if (!ec0 || !ec0.activeFile || !ec0.activeFile.includes('diagnostics_main')) {
    fail(`Expected activeFile to contain 'diagnostics_main', got: ${ec0?.activeFile}`);
  }
  console.log('  OK: MAIN file is active');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 1: FOLD / UNFOLD
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 1: FOLD/UNFOLD — injecting 3 diagnostics…');
  await injectDiagnostics([
    { severity: 'Error',   message: 'main-diag',    file_path: MAIN,   line: L_MAIN,   column: COL },
    { severity: 'Warning', message: 'helper-diag',  file_path: HELPER, line: L_HELPER, column: COL },
    { severity: 'Info',    message: 'span-less-diag', has_location: false },
  ]);

  // Panel starts collapsed (problemsCollapsed default = true) → rows hidden
  const collapsed1 = await countElements('[data-testid="diagnostic-row"]');
  console.log(`  diagnostic-row count (collapsed): ${collapsed1.count}`);
  if (collapsed1.count !== 0) {
    fail(`Expected 0 diagnostic-row elements when panel is collapsed, got ${collapsed1.count}`);
  }
  console.log('  OK: 0 rows when collapsed');

  // Click fold-toggle → expand
  log('  Clicking diagnostics-fold-toggle to expand…');
  await rpc('click_element', { testId: 'diagnostics-fold-toggle' });
  await sleep(300);

  const expanded1 = await countElements('[data-testid="diagnostic-row"]');
  console.log(`  diagnostic-row count (expanded): ${expanded1.count}`);
  if (expanded1.count !== 3) {
    fail(`Expected 3 diagnostic-row elements after expanding, got ${expanded1.count}`);
  }
  console.log('  OK: 3 rows after expanding via fold-toggle');

  // Click StatusBar diagnostics-count badge → fold
  log('  Clicking StatusBar diagnostics-count badge to fold…');
  await rpc('click_element', { testId: 'diagnostics-count' });
  await sleep(300);

  const folded2 = await countElements('[data-testid="diagnostic-row"]');
  console.log(`  diagnostic-row count (folded via badge): ${folded2.count}`);
  if (folded2.count !== 0) {
    fail(`Expected 0 rows after clicking badge, got ${folded2.count}`);
  }
  console.log('  OK: 0 rows after folding via StatusBar badge');

  // Unfold again (for scenario 2)
  log('  Unfolding again via fold-toggle for scenario 2…');
  await rpc('click_element', { testId: 'diagnostics-fold-toggle' });
  await sleep(300);

  const expanded2 = await countElements('[data-testid="diagnostic-row"]');
  if (expanded2.count !== 3) {
    fail(`Expected 3 rows after re-expanding, got ${expanded2.count}`);
  }
  console.log('  OK: Scenario 1 PASSED (fold/unfold via toggle + badge)');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 2: SPAN-LESS INERT (structural check)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 2: SPAN-LESS INERT structural check…');

  // All 3 rows are visible (panel expanded from scenario 1 unfold)
  const allRows = await countElements('[data-testid="diagnostic-row"]');
  console.log(`  All diagnostic-row count: ${allRows.count}`);
  if (allRows.count !== 3) {
    fail(`Expected 3 diagnostic-row elements total, got ${allRows.count}`);
  }

  // Only line-tied rows have role="button"
  const buttonRows = await countElements('[data-testid="diagnostic-row"][role="button"]');
  console.log(`  role=button diagnostic-row count: ${buttonRows.count}`);
  if (buttonRows.count !== 2) {
    fail(`Expected 2 line-tied rows (role=button), got ${buttonRows.count} — span-less β contract violated`);
  }
  console.log('  OK: 3 total rows, 2 with role=button (1 span-less without role)');

  // Exactly one diagnostic-location element has text "—" (the span-less row)
  const locAll = await countElements('[data-testid="diagnostic-location"]');
  console.log(`  diagnostic-location count: ${locAll.count}`);
  if (locAll.count !== 3) {
    fail(`Expected 3 diagnostic-location elements (one per row), got ${locAll.count}`);
  }
  const dashEls = locAll.elements.filter(e => e.text === '—');
  console.log(`  Elements with text "—": ${dashEls.length}`);
  if (dashEls.length !== 1) {
    const texts = locAll.elements.map(e => JSON.stringify(e.text));
    fail(`Expected exactly 1 diagnostic-location with text "—", got ${dashEls.length}. Texts: [${texts.join(', ')}]`);
  }
  console.log('  OK: Scenario 2 PASSED (span-less row has no role=button + "—" location text)');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 3: IN-FILE JUMP
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 3: IN-FILE JUMP — re-inject 1 MAIN line-tied diagnostic…');
  await injectDiagnostics([
    { severity: 'Error', message: 'main-jump-diag', file_path: MAIN, line: L_MAIN, column: COL },
  ]);

  // Ensure panel is expanded (1 row visible)
  const rows3 = await countElements('[data-testid="diagnostic-row"]');
  console.log(`  diagnostic-row count after re-inject: ${rows3.count}`);
  if (rows3.count !== 1) {
    // Panel may have collapsed; try to expand
    if (rows3.count === 0) {
      await rpc('click_element', { testId: 'diagnostics-fold-toggle' });
      await sleep(300);
      const rows3b = await countElements('[data-testid="diagnostic-row"]');
      if (rows3b.count !== 1) {
        fail(`Expected 1 row after expand, got ${rows3b.count}`);
      }
    } else {
      fail(`Expected 1 diagnostic-row, got ${rows3.count}`);
    }
  }

  // Click the (only) diagnostic row to trigger in-file jump
  log('  Clicking diagnostic-row for in-file navigation…');
  await rpc('click_element', { testId: 'diagnostic-row' });
  await sleep(500);  // allow navigate + editor scroll + cursor update

  const ec3 = await rpc('editor_content');
  console.log('  editor_content after in-file click:', JSON.stringify({
    activeFile: ec3?.activeFile,
    cursorPosition: ec3?.cursorPosition,
  }));
  if (!ec3 || !ec3.activeFile || !ec3.activeFile.includes('diagnostics_main')) {
    fail(`Expected activeFile to contain 'diagnostics_main' after in-file jump, got: ${ec3?.activeFile}`);
  }
  if (!ec3.cursorPosition) {
    fail('cursorPosition is null after in-file navigation');
  }
  if (ec3.cursorPosition.line !== L_MAIN) {
    fail(`Expected cursorPosition.line===${L_MAIN}, got ${ec3.cursorPosition.line}`);
  }
  console.log('  OK: Scenario 3 PASSED (in-file jump → cursor at line', L_MAIN, ')');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 4: CROSS-FILE OPEN + JUMP
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 4: CROSS-FILE JUMP — re-inject 1 HELPER line-tied diagnostic…');
  await injectDiagnostics([
    { severity: 'Warning', message: 'helper-jump-diag', file_path: HELPER, line: L_HELPER, column: COL },
  ]);

  // Ensure panel expanded (1 row visible)
  const rows4 = await countElements('[data-testid="diagnostic-row"]');
  if (rows4.count !== 1) {
    if (rows4.count === 0) {
      await rpc('click_element', { testId: 'diagnostics-fold-toggle' });
      await sleep(300);
      const rows4b = await countElements('[data-testid="diagnostic-row"]');
      if (rows4b.count !== 1) {
        fail(`Expected 1 row after expand (cross-file), got ${rows4b.count}`);
      }
    } else {
      fail(`Expected 1 diagnostic-row before cross-file click, got ${rows4.count}`);
    }
  }

  log('  Clicking diagnostic-row for cross-file navigation…');
  await rpc('click_element', { testId: 'diagnostic-row' });
  await sleep(700);  // allow file open + editor activation + scroll + cursor update

  const ec4 = await rpc('editor_content');
  console.log('  editor_content after cross-file click:', JSON.stringify({
    activeFile: ec4?.activeFile,
    cursorPosition: ec4?.cursorPosition,
  }));
  if (!ec4 || !ec4.activeFile || !ec4.activeFile.includes('diagnostics_helper')) {
    fail(`Expected activeFile to contain 'diagnostics_helper' after cross-file jump, got: ${ec4?.activeFile}`);
  }
  if (!ec4.cursorPosition) {
    fail('cursorPosition is null after cross-file navigation');
  }
  if (ec4.cursorPosition.line !== L_HELPER) {
    fail(`Expected cursorPosition.line===${L_HELPER}, got ${ec4.cursorPosition.line}`);
  }
  console.log('  OK: Scenario 4 PASSED (cross-file jump → helper active, cursor at line', L_HELPER, ')');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 5: PERSISTENCE (localStorage survives debounced write)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 5: PERSISTENCE — resize_panes({problemsHeight:240})…');

  // Ensure the panel is unfolded so problemsCollapsed===false is persisted
  const rowsChk = await countElements('[data-testid="diagnostic-row"]');
  if (rowsChk.count === 0) {
    // Panel is collapsed; expand it so problemsCollapsed becomes false
    await rpc('click_element', { testId: 'diagnostics-fold-toggle' });
    await sleep(300);
    const rowsChk2 = await countElements('[data-testid="diagnostic-row"]');
    if (rowsChk2.count === 0) {
      fail('Failed to expand diagnostics panel for persistence test (0 rows after toggle)');
    }
  }
  console.log('  Panel is expanded (problemsCollapsed===false)');

  // Set problemsHeight to 240 pixels
  const resizeResult = await rpc('resize_panes', { problemsHeight: 240 });
  console.log('  resize_panes result:', JSON.stringify(resizeResult));
  if (!resizeResult || !resizeResult.ok) {
    fail(`resize_panes({problemsHeight:240}) failed: ${JSON.stringify(resizeResult)}`);
  }
  if (resizeResult.layout.problemsHeight !== 240) {
    fail(`Expected layout echo problemsHeight===240, got ${resizeResult.layout.problemsHeight}`);
  }
  console.log('  OK: resize_panes echo confirms problemsHeight=240');

  // Sleep >300ms for the debounced savePanelLayout to fire
  log('  Waiting 350ms for debounced localStorage write…');
  await sleep(350);

  // Read localStorage key "reify-panel-layout"
  const lsResult = await rpc('get_local_storage', { key: 'reify-panel-layout' });
  console.log('  get_local_storage result:', JSON.stringify(lsResult));
  if (!lsResult || !lsResult.present) {
    fail(`Expected 'reify-panel-layout' key to be present in localStorage, got: ${JSON.stringify(lsResult)}`);
  }
  let layout;
  try {
    layout = JSON.parse(lsResult.value);
  } catch (e) {
    fail(`Failed to JSON.parse localStorage 'reify-panel-layout': ${lsResult.value}`);
  }
  console.log('  Parsed layout:', JSON.stringify(layout));
  if (layout.problemsHeight !== 240) {
    fail(`Expected persisted problemsHeight===240, got ${layout.problemsHeight}`);
  }
  if (layout.problemsCollapsed !== false) {
    fail(`Expected persisted problemsCollapsed===false, got ${layout.problemsCollapsed}`);
  }
  console.log('  OK: Scenario 5 PASSED (localStorage persists problemsHeight=240, problemsCollapsed=false)');

  // ════════════════════════════════════════════════════════════════════════════

  console.log('\n=== SMOKE PASS: smoke_diagnostics_e2e all 5 scenarios PASSED ===');
  process.exit(0);
}

main().catch(err => {
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
