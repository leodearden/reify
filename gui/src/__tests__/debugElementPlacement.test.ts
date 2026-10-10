// Every tool that reports `bounds` carries the same {bounds, visible, hitTestable} trio (docs/debug-mcp-contract.md §3).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

import { listen } from '@tauri-apps/api/event';
import { initDebugBridge } from '../debug/bridge';
import {
  makeCmdDispatcher,
  makeDebugStores,
  type DebugRequestHandler,
} from './debugBridgeTestHelpers';

const selectorFor = (testId: string) => `[data-testid="${testId}"]`;

interface PlacementTool {
  tool: string;
  params: (testId: string) => Record<string, unknown>;
  placementOf: (result: any, testId: string) => any;
}

const PLACEMENT_TOOLS: PlacementTool[] = [
  { tool: 'dom_query', params: (testId) => ({ testId }), placementOf: (result) => result },
  {
    tool: 'query_selector',
    params: (testId) => ({ selector: selectorFor(testId) }),
    placementOf: (result) => result,
  },
  {
    tool: 'query_selector_all',
    params: (testId) => ({ selector: selectorFor(testId) }),
    placementOf: (result) => result.elements[0],
  },
  {
    tool: 'list_elements',
    params: () => ({}),
    placementOf: (result, testId) => result.elements.find((e: any) => e.testId === testId),
  },
  {
    tool: 'get_layout_metrics',
    params: (testId) => ({ selector: selectorFor(testId) }),
    placementOf: (result) => result,
  },
];

const TARGET_BOUNDS = { x: 100, y: 50, width: 80, height: 40 };
const TARGET_CENTRE = { x: 140, y: 70 };

describe.each(PLACEMENT_TOOLS)('$tool reports the placement trio', ({ tool, params, placementOf }) => {
  let capturedHandler: DebugRequestHandler | undefined;
  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);
  let target: HTMLElement;
  let elsewhere: HTMLElement;

  beforeEach(async () => {
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
    document.body.innerHTML = `<div data-testid="target"></div><div data-testid="elsewhere"></div>`;
    target = document.querySelector(selectorFor('target')) as HTMLElement;
    elsewhere = document.querySelector(selectorFor('elsewhere')) as HTMLElement;
    const { x, y, width, height } = TARGET_BOUNDS;
    vi.spyOn(target, 'getBoundingClientRect').mockReturnValue({
      x, y, width, height, left: x, top: y, right: x + width, bottom: y + height,
    } as DOMRect);
    await initDebugBridge(makeDebugStores());
  });

  afterEach(() => {
    vi.restoreAllMocks();
    delete window.__REIFY_DEBUG__;
    document.body.innerHTML = '';
  });

  function hitTestReturns(hit: (x: number, y: number) => Element | null): void {
    vi.spyOn(document, 'elementFromPoint').mockImplementation(hit);
  }

  async function placementOfTarget(): Promise<any> {
    return placementOf(await dispatchCmd(1, tool, params('target')), 'target');
  }

  it('(i) a centre hit on the element itself is hit-testable', async () => {
    hitTestReturns(() => target);
    const placement = await placementOfTarget();
    expect(placement.hitTestable).toBe(true);
    expect(placement.visible).toBe(true);
    expect(placement.bounds).toEqual(TARGET_BOUNDS);
  });

  it('(ii) the probe point is the bounds centre', async () => {
    hitTestReturns((x, y) => (x === TARGET_CENTRE.x && y === TARGET_CENTRE.y ? target : elsewhere));
    expect((await placementOfTarget()).hitTestable).toBe(true);
  });

  it('(iii) a centre hit on a descendant is hit-testable', async () => {
    const child = document.createElement('span');
    target.appendChild(child);
    hitTestReturns(() => child);
    expect((await placementOfTarget()).hitTestable).toBe(true);
  });

  it('(iv) a centre hit on another element (clipped or occluded) is not hit-testable, yet visible with unclipped bounds', async () => {
    hitTestReturns(() => elsewhere);
    const placement = await placementOfTarget();
    expect(placement.hitTestable).toBe(false);
    expect(placement.visible).toBe(true);
    expect(placement.bounds).toEqual(TARGET_BOUNDS);
  });

  it('(v) a centre that hits nothing (outside the window) is not hit-testable', async () => {
    hitTestReturns(() => null);
    expect((await placementOfTarget()).hitTestable).toBe(false);
  });

  it('(vi) a hidden element is neither visible nor hit-testable, whatever the hit test says', async () => {
    target.style.display = 'none';
    hitTestReturns(() => target);
    const placement = await placementOfTarget();
    expect(placement.visible).toBe(false);
    expect(placement.hitTestable).toBe(false);
  });
});
