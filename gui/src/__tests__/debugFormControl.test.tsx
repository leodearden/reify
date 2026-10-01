/**
 * scrub_range_input / edit_text_input: the debug-bridge tools that drive a
 * native form control's value, exercised through the REAL debug-request
 * dispatch against the REAL MechanismPanel and PropertyEditor.
 *
 * Frame order is made deterministic by a FIFO setTimeout-backed
 * requestAnimationFrame: the component's preview frame is registered during the
 * `input` dispatch, so it runs before the tool's own await-one-frame resumes.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@solidjs/testing-library';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
  open: vi.fn(),
  ask: vi.fn(),
}));
vi.mock('three', () => ({
  Box3: class { expandByObject() {} isEmpty() { return true; } },
  Vector3: class {},
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';
import { previewParameter, setParameter } from '../bridge';
import { MechanismPanel } from '../panels/MechanismPanel';
import { PropertyEditor } from '../panels/PropertyEditor';
import type { MechanismDescriptor, ValueData } from '../types';
import {
  makeCmdDispatcher,
  makeDebugStores,
  type DebugRequestHandler,
} from './debugBridgeTestHelpers';

type ParamCallback = (cellId: string, value: string) => void | Promise<void>;

const SLIDER_PARAM = 'Kinematic.y_pos';
const SLIDER_SELECTOR =
  '[data-testid="mechanism-section-Kinematic.m1"] [data-testid="joint-row-0"] input[type="range"]';

/** One prismatic joint bound to `Kinematic.y_pos`, range 0..800 mm, at 100 mm. */
const SLIDER_DESCRIPTOR: MechanismDescriptor = {
  cell_id: 'Kinematic.m1',
  entity_path: 'Kinematic',
  name: 'm1',
  bodies_count: 2,
  joints: [
    {
      joint_index: 0,
      kind: 'prismatic',
      dimension: 'length',
      range_lower_si: 0,
      range_upper_si: 0.8,
      axis: [0, 1, 0],
      driving_param_cell_id: SLIDER_PARAM,
      current_value_si: 0.1,
      binding: { kind: 'param_bound', param_cell_id: SLIDER_PARAM, current_value_si: 0.1 },
    },
  ],
};

const EDIT_CELL = 'Bracket.width';
const EDIT_SELECTOR = `[data-testid="prop-row-${EDIT_CELL}"] input[type="text"]`;
const EDIT_FRAMES = ['1', '15', '150', '150m'];

/** One determined Length cell; no ladders, so the base-unit floor accepts `mm`. */
const EDIT_VALUES: Record<string, ValueData> = {
  [EDIT_CELL]: {
    cell_id: EDIT_CELL,
    name: 'width',
    value: '50',
    unit: 'mm',
    determinacy: 'determined',
    entity_path: EDIT_CELL,
    kind: 'Param',
    freshness: 'final',
    dimension: 'Length',
    si_value: 0.05,
  },
};

/** FIFO requestAnimationFrame over setTimeout; returns the restore function. */
function installTimeoutRaf(): () => void {
  const originalRequest = globalThis.requestAnimationFrame;
  const originalCancel = globalThis.cancelAnimationFrame;
  globalThis.requestAnimationFrame = (cb: FrameRequestCallback): number =>
    setTimeout(() => cb(performance.now()), 0) as unknown as number;
  globalThis.cancelAnimationFrame = (id: number): void => clearTimeout(id);
  return () => {
    globalThis.requestAnimationFrame = originalRequest;
    globalThis.cancelAnimationFrame = originalCancel;
  };
}

/** Yield past a macrotask boundary, draining every settled promise continuation. */
function flushPendingPromises(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** The Tauri commands invoked since the last dispatch, the bridge's own reply excluded. */
function appInvokes(): unknown[][] {
  return vi.mocked(invoke).mock.calls.filter((c) => c[0] !== 'debug_response');
}

let capturedHandler: DebugRequestHandler | undefined;
let restoreRaf: () => void;
const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

beforeEach(async () => {
  vi.clearAllMocks();
  capturedHandler = undefined;
  vi.mocked(listen).mockImplementation(async (_event, handler) => {
    capturedHandler = handler as unknown as DebugRequestHandler;
    return () => {};
  });
  await initDebugBridge(makeDebugStores());
  restoreRaf = installTimeoutRaf();
});

afterEach(() => {
  cleanup();
  restoreRaf();
});

function renderSlider(onPreviewParameter: ParamCallback, onSetParameter: ParamCallback) {
  render(() => (
    <MechanismPanel
      descriptors={[SLIDER_DESCRIPTOR]}
      onPreviewParameter={onPreviewParameter}
      onSetParameter={onSetParameter}
      onScrubLocal={vi.fn()}
    />
  ));
}

function renderEditor(onSetParameter: ParamCallback) {
  render(() => (
    <PropertyEditor
      values={EDIT_VALUES}
      selectedEntity={null}
      onSetParameter={onSetParameter}
      unitLadders={undefined}
    />
  ));
}

describe('scrub_range_input against the real MechanismPanel', () => {
  const FRAMES = ['100', '200'];
  const PREVIEWS = [
    [SLIDER_PARAM, '100mm'],
    [SLIDER_PARAM, '200mm'],
    [SLIDER_PARAM, '300mm'],
  ];

  it('hold: previews every frame, commits nothing, and makes no Tauri call of its own', async () => {
    const onPreviewParameter = vi.fn();
    const onSetParameter = vi.fn();
    renderSlider(onPreviewParameter, onSetParameter);

    const result = await dispatchCmd(1, 'scrub_range_input', {
      selector: SLIDER_SELECTOR,
      value: '300',
      frames: FRAMES,
      commit: 'hold',
    });
    await flushPendingPromises();

    expect(result).toEqual({ ok: true, value: '300', inputEvents: 3, commit: 'hold' });
    expect(onPreviewParameter.mock.calls).toEqual(PREVIEWS);
    expect(onSetParameter).not.toHaveBeenCalled();
    expect(appInvokes()).toEqual([]);
  });

  it('change: previews every frame, then commits the release value once, last', async () => {
    const onPreviewParameter = vi.fn();
    const onSetParameter = vi.fn();
    renderSlider(onPreviewParameter, onSetParameter);

    const result = await dispatchCmd(2, 'scrub_range_input', {
      selector: SLIDER_SELECTOR,
      value: '300',
      frames: FRAMES,
      commit: 'change',
    });
    await flushPendingPromises();

    expect(result).toEqual({ ok: true, value: '300', inputEvents: 3, commit: 'change' });
    expect(onPreviewParameter.mock.calls).toEqual(PREVIEWS);
    expect(onSetParameter.mock.calls).toEqual([[SLIDER_PARAM, '300mm']]);
    const lastPreviewOrder = Math.max(...onPreviewParameter.mock.invocationCallOrder);
    expect(onSetParameter.mock.invocationCallOrder[0]).toBeGreaterThan(lastPreviewOrder);
    expect(appInvokes()).toEqual([]);
  });

  it('the gesture crosses the IPC hop: preview_parameter per frame, then one set_parameter', async () => {
    renderSlider(previewParameter, setParameter);

    await dispatchCmd(3, 'scrub_range_input', {
      selector: SLIDER_SELECTOR,
      value: '300',
      frames: FRAMES,
      commit: 'change',
    });
    await flushPendingPromises();

    const ipc = (command: string, value: string) => [
      command,
      expect.objectContaining({ cellId: SLIDER_PARAM, value }),
    ];
    expect(appInvokes()).toEqual([
      ipc('preview_parameter', '100mm'),
      ipc('preview_parameter', '200mm'),
      ipc('preview_parameter', '300mm'),
      ipc('set_parameter', '300mm'),
    ]);
  });
});

describe('edit_text_input against the real PropertyEditor', () => {
  it.each(['enter', 'blur'] as const)(
    '%s commits the final literal exactly once',
    async (commit) => {
      const onSetParameter = vi.fn();
      renderEditor(onSetParameter);

      const result = await dispatchCmd(4, 'edit_text_input', {
        selector: EDIT_SELECTOR,
        value: '150mm',
        frames: EDIT_FRAMES,
        commit,
      });

      expect(result).toMatchObject({ ok: true, inputEvents: 5, commit });
      // A read-back, not a verdict: after the commit the editor legitimately
      // rewrites the control to its at-rest display.
      expect(typeof result.value).toBe('string');
      expect(onSetParameter.mock.calls).toEqual([[EDIT_CELL, '150mm']]);
      expect(appInvokes()).toEqual([]);
    },
  );

  it('hold types every frame but commits nothing', async () => {
    const onSetParameter = vi.fn();
    renderEditor(onSetParameter);

    const result = await dispatchCmd(5, 'edit_text_input', {
      selector: EDIT_SELECTOR,
      value: '150mm',
      frames: EDIT_FRAMES,
      commit: 'hold',
    });

    expect(result).toEqual({ ok: true, value: '150mm', inputEvents: 5, commit: 'hold' });
    expect(onSetParameter).not.toHaveBeenCalled();
  });
});
