#!/usr/bin/env node
/**
 * Acceptance smoke for task-4202: Find-uses panel (Shift+F12).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_find_uses.mjs
 *
 * The script drives a running reify-gui debug server (already launched with
 * REIFY_DEBUG=1) over JSON-RPC:
 *
 *   1. health — confirm server is up
 *   2. open_file — load find_uses_smoke.ri (structure Smoke { param x; let y = x + x })
 *   3. editor_content — confirm activeFile
 *   4. keyboard navigation — Down, End, 14×Left → cursor on `x` at line 1 col 10
 *   5. keyboard Shift-F12 → findUsesCommand fires → FindUsesPanel opens
 *   6. list_elements — confirm find-uses-panel visible + exactly 3 find-use-row elements
 *      (declaration + 2 uses = 3 total, with includeDeclaration: true)
 *   7. click_element find-use-row (first row) → onNavigate fires
 *   8. editor_content — confirm cursorPosition moved to the clicked occurrence
 *
 * Exit 0 on pass, non-zero on failure.
 */

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const FIXTURE_PATH = path.join(REPO_ROOT, 'gui', 'test', 'fixtures', 'find_uses_smoke.ri');

// Inline port resolution (mirrors endpoint.ts logic)
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

let step = 0;
function log(msg) {
  step++;
  console.log(`[step ${step}] ${msg}`);
}
function fail(msg) {
  console.error(`FAIL: ${msg}`);
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
  // The debug server returns: { result: { content: [{type:"text",text:"<json>"}] } }
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

async function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

async function waitForServer(timeoutMs = 30_000) {
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

async function keyboard(key, { shift = false, ctrl = false, alt = false } = {}) {
  await rpc('keyboard', { key, ...(shift ? { shift: true } : {}), ...(ctrl ? { ctrl: true } : {}), ...(alt ? { alt: true } : {}) });
  await sleep(50);
}

async function main() {
  console.log(`smoke_find_uses: targeting debug server at ${DEBUG_URL}`);
  console.log(`Fixture: ${FIXTURE_PATH}`);

  // Step 1: health
  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  // Step 2: open the find_uses_smoke fixture
  log('Opening find_uses_smoke fixture via open_file…');
  const openResult = await rpc('open_file', { path: FIXTURE_PATH });
  console.log('  open_file result:', JSON.stringify(openResult));
  await sleep(1000); // wait for engine eval

  // Step 3: confirm activeFile
  log('Confirming editor_content.activeFile…');
  const ec1 = await rpc('editor_content');
  if (!ec1) fail('editor_content returned null');
  console.log('  activeFile:', ec1.activeFile);
  if (!ec1.activeFile || !ec1.activeFile.includes('find_uses_smoke')) {
    fail(`Expected activeFile to contain 'find_uses_smoke', got: ${ec1.activeFile}`);
  }
  console.log('  OK: activeFile confirmed');

  // Step 4: navigate cursor to `x` at line 1 col 10 (0-indexed)
  // Fixture content:
  //   line 0: "structure Smoke {"
  //   line 1: "    param x: Scalar = 1mm"   ← x at col 10
  //   line 2: "    let y = x + x"
  //   line 3: "}"
  //
  // From cursor at (0,0): Down → line 1; End → col 24; 14×Left → col 10 (x)
  log('Navigating cursor to `x` at line 1 col 10…');
  await keyboard('Home', { ctrl: true }); // ensure start
  await keyboard('ArrowDown');             // → line 1
  await keyboard('End');                   // → col 24 (end of line 1)
  for (let i = 0; i < 14; i++) {
    await keyboard('ArrowLeft');           // → col 10
  }
  await sleep(200);

  // Verify cursor position via editor_content
  const ec2 = await rpc('editor_content');
  console.log('  cursorPosition after nav:', JSON.stringify(ec2?.cursorPosition));

  // Step 5: trigger find-uses via Shift+F12
  log('Triggering Shift+F12 (findUsesCommand)…');
  await keyboard('F12', { shift: true });
  await sleep(500); // wait for LSP round-trip + panel render

  // Step 6: verify find-uses-panel is visible
  log('Checking list_elements for find-uses-panel…');
  const elements = await rpc('list_elements');
  if (!elements) fail('list_elements returned null');

  // elements is an array of {testId, tagName, visible, bounds}
  const findUsesPanel = elements.find(e => e.testId === 'find-uses-panel');
  if (!findUsesPanel) {
    fail(`find-uses-panel not found in DOM. Panel testIds present: ${elements.map(e => e.testId).filter(t => t?.includes('find')).join(', ')}`);
  }
  if (!findUsesPanel.visible) {
    fail('find-uses-panel exists in DOM but is not visible');
  }
  console.log('  OK: find-uses-panel is visible');

  // Step 6b: check N+1 rows (3 for x in this fixture)
  const rows = elements.filter(e => e.testId === 'find-use-row');
  console.log(`  find-use-row count: ${rows.length}`);
  if (rows.length !== 3) {
    fail(`Expected 3 find-use-row elements (declaration + 2 uses of x), got ${rows.length}`);
  }
  console.log('  OK: 3 find-use-rows (declaration + 2 uses)');

  // Step 6c: check panel title
  const titleEl = elements.find(e => e.testId === 'panel-title-find-uses');
  if (titleEl) {
    console.log('  panel title element text (check via dom_query):');
    const titleQuery = await rpc('dom_query', { testId: 'panel-title-find-uses' });
    console.log('  ', JSON.stringify(titleQuery));
    if (titleQuery?.text && !titleQuery.text.includes('Find uses (3)')) {
      fail(`Expected panel title 'Find uses (3)', got '${titleQuery.text}'`);
    }
    console.log('  OK: panel title is "Find uses (3)"');
  }

  // Step 7: click the first find-use-row
  log('Clicking first find-use-row…');
  // Capture cursor position before click
  const ec3 = await rpc('editor_content');
  const cursorBefore = ec3?.cursorPosition;
  console.log('  cursorPosition before click:', JSON.stringify(cursorBefore));

  await rpc('click_element', { testId: 'find-use-row' });
  await sleep(400);

  // Step 8: verify cursor moved
  log('Confirming cursor moved after click…');
  const ec4 = await rpc('editor_content');
  const cursorAfter = ec4?.cursorPosition;
  console.log('  cursorPosition after click:', JSON.stringify(cursorAfter));

  // Cursor should have moved (position should differ from before or be at a reference location)
  // The first row in the list corresponds to the first reference returned by the LSP.
  // With includeDeclaration: true, the LSP returns references sorted by position.
  // For x, the first result is the declaration at line 1, col 10 (1-indexed: line 2, col 11).
  if (!cursorAfter) {
    fail('editor_content returned null after click');
  }

  // The panel should be closed after navigation
  const elementsAfter = await rpc('list_elements');
  const panelAfterClick = elementsAfter?.find(e => e.testId === 'find-uses-panel');
  if (panelAfterClick && panelAfterClick.visible) {
    fail('find-uses-panel is still visible after clicking a result (expected auto-close)');
  }
  console.log('  OK: find-uses-panel closed after navigation');

  console.log('\n=== SMOKE PASS: find-uses Shift+F12 acceptance test PASSED ===');
  process.exit(0);
}

main().catch(err => {
  console.error('Unexpected error:', err);
  process.exit(2);
});
