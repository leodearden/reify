/**
 * Parity guard (task 7049): the in-app assistant's SYSTEM_PROMPT
 * (gui/sidecar/src/system-prompt.ts) ↔ tool_defs() in debug_server.rs, the
 * registry the sidecar's reify-debug MCP connection is served from.
 *
 * It lives in the gui suite, not beside the prompt, because the merge gate runs
 * only typecheck for gui/sidecar, never its vitest suite.
 *
 * A failure is fixed by classifying the tool: name it in the prompt's tool
 * table, or list it in NOT_ADVERTISED_TO_SIDECAR below.
 *
 * Extraction sanity (exact count, duplicates) is asserted once, in
 * ./debugParity.test.ts case (b), and deliberately not repeated here.
 */
import { describe, it, expect } from 'vitest';
import { SYSTEM_PROMPT } from '../../sidecar/src/system-prompt';
import { extractToolDefNames, readDebugServerSource } from './toolDefNames';

const toolDefNames = extractToolDefNames(readDebugServerSource());

const promptNamed = new Set(
  [...SYSTEM_PROMPT.matchAll(/mcp__reify-debug__([A-Za-z0-9_]+)/g)].map((m) => m[1]),
);

/**
 * Callable (ALLOWED_TOOLS grants mcp__reify-debug__*) but withheld from the
 * prompt: they drive or probe the GUI rather than serve design work, or alias an
 * advertised tool.
 */
const NOT_ADVERTISED_TO_SIDECAR = {
  alias: ['open_file'],
  guiAutomation: [
    'dom_query',
    'list_elements',
    'click_element',
    'type_in_editor',
    'keyboard',
    'query_selector',
    'query_selector_all',
    'get_layout_metrics',
    'get_computed_style',
    'active_element',
    'get_window_state',
    'open_menu',
    'menu_state',
    'press_tab',
    'tab_order',
    'ui_outline',
    'resize_panes',
    'get_local_storage',
    'set_window_size',
    'expand_tree_node',
    'collapse_tree_node',
    'click_at',
    'hover',
    'drag',
    'focus_element',
    'focus_editor',
    'scroll',
    'wait_for',
    'wait_for_selector',
  ],
  testHarness: [
    'health',
    'store_state',
    'load_fixture',
    'inject_diagnostics',
    'reset_app_state',
    'set_test_mode',
    'list_console_errors',
    'screenshot_window',
    'element_screenshot',
    'demand_dispatch',
    'morph_stats',
    'mesh_morph_stats',
  ],
  editorProbes: ['hover_at', 'completion_at', 'definition_at'],
  pointerCamera: ['pick_entity_at', 'orbit_camera', 'pan_camera', 'zoom_camera'],
} as const satisfies Readonly<Record<string, readonly string[]>>;

const withheld: readonly string[] = Object.values(NOT_ADVERTISED_TO_SIDECAR).flat();

describe('sidecar SYSTEM_PROMPT ↔ tool_defs() parity', () => {
  it('(a) every reify-debug tool the prompt names is served by tool_defs()', () => {
    expect(
      promptNamed.size,
      'no mcp__reify-debug__<name> token found in SYSTEM_PROMPT — the prefix this guard scans for has changed',
    ).toBeGreaterThan(0);

    const unserved = [...promptNamed].filter((n) => !toolDefNames.includes(n));
    expect(unserved, 'SYSTEM_PROMPT names reify-debug tools tool_defs() does not serve').toStrictEqual(
      [],
    );
  });

  it('(b) every tool_defs() tool is advertised or deliberately withheld', () => {
    const unclassified = toolDefNames.filter((n) => !promptNamed.has(n) && !withheld.includes(n));
    expect(
      unclassified,
      'unclassified tool_defs() tools: name each in gui/sidecar/src/system-prompt.ts, or add it to NOT_ADVERTISED_TO_SIDECAR in this file',
    ).toStrictEqual([]);
  });

  it('(c) the withheld allowlist is self-checking', () => {
    const stale = withheld.filter((n) => !toolDefNames.includes(n));
    expect(stale, 'NOT_ADVERTISED_TO_SIDECAR entries tool_defs() no longer serves').toStrictEqual([]);

    const contradictory = withheld.filter((n) => promptNamed.has(n));
    expect(
      contradictory,
      'NOT_ADVERTISED_TO_SIDECAR entries the prompt nevertheless names',
    ).toStrictEqual([]);

    const duplicates = withheld.filter((n, i) => withheld.indexOf(n) !== i);
    expect(duplicates, 'NOT_ADVERTISED_TO_SIDECAR entries listed more than once').toStrictEqual([]);
  });

  it('(d) the five ai-native-editing write tools are advertised', () => {
    const AI_WRITE_TOOLS = [
      'reify_set_parameter',
      'reify_update_source',
      'reify_open_file',
      'reify_save_file',
      'reify_export',
    ];
    const unadvertised = AI_WRITE_TOOLS.filter((n) => !promptNamed.has(n));
    expect(unadvertised, 'AI write tools SYSTEM_PROMPT does not advertise').toStrictEqual([]);
  });
});
