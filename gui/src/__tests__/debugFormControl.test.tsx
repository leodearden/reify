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
import {
  FORM_CONTROL_ERRORS,
  MAX_FORM_CONTROL_FRAMES,
  RANGE_INPUT,
  TEXT_INPUT,
} from '../debug/formControl';
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
  document.body.innerHTML = '';
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

// ─── Refusals ────────────────────────────────────────────────────────────────
//
// Every refusal is pinned against FORM_CONTROL_ERRORS, and every one also
// asserts that NOT ONE event reached a control: a refusal that still drove the
// control would be a false error on top of a real side effect.

const HOST_TESTID = 'form-control-host';
const HOST_SELECTOR = `[data-testid="${HOST_TESTID}"] > *`;
const NOTHING_SELECTOR = '[data-testid="nothing-here"]';
const RANGE_HTML = '<input type="range" min="0" max="200" step="1" value="100">';
const TEXT_HTML = '<input type="text" value="10mm">';
const RECORDED_EVENTS = ['focus', 'blur', 'input', 'change', 'keydown'] as const;

/** Mount raw controls in a fresh host; every event reaching any of them is recorded. */
function mountControls(html: string): { controls: HTMLInputElement[]; events: string[] } {
  const host = document.createElement('div');
  host.setAttribute('data-testid', HOST_TESTID);
  host.innerHTML = html;
  document.body.appendChild(host);
  const events: string[] = [];
  const controls = Array.from(host.children) as HTMLInputElement[];
  for (const el of controls) {
    for (const type of RECORDED_EVENTS) el.addEventListener(type, () => events.push(type));
  }
  return { controls, events };
}

/** `base` with `override` applied; an override of `undefined` REMOVES the key. */
function withParams(
  base: Record<string, unknown>,
  override: Record<string, unknown>,
): Record<string, unknown> {
  const merged: Record<string, unknown> = { ...base, ...override };
  for (const [key, v] of Object.entries(override)) if (v === undefined) delete merged[key];
  return merged;
}

const TOOLS = [
  { tool: 'scrub_range_input', html: RANGE_HTML, commit: 'change', foreignCommit: 'enter', spec: RANGE_INPUT },
  { tool: 'edit_text_input', html: TEXT_HTML, commit: 'enter', foreignCommit: 'change', spec: TEXT_INPUT },
] as const;

describe.each(TOOLS)('$tool: parameter validation runs before any DOM resolution', (t) => {
  const valid = { selector: HOST_SELECTOR, value: '50', commit: t.commit };
  const tooMany = Array.from({ length: MAX_FORM_CONTROL_FRAMES + 1 }, () => '50');

  it.each([
    ['selector absent', { selector: undefined }, () => ({ error: 'selector is required' })],
    ['selector empty', { selector: '' }, () => ({ error: 'selector is required' })],
    ['selector not a string', { selector: ['input'] }, () => ({ error: 'selector is required' })],
    ['commit absent', { commit: undefined }, () => ({ error: FORM_CONTROL_ERRORS.commitRequired })],
    [
      'commit of the other tool',
      { commit: t.foreignCommit },
      () => ({ error: FORM_CONTROL_ERRORS.commitNotAllowed(t.foreignCommit, t.spec.commits) }),
    ],
    [
      'commit unknown',
      { commit: 'bogus' },
      () => ({ error: FORM_CONTROL_ERRORS.commitNotAllowed('bogus', t.spec.commits) }),
    ],
    ['value absent', { value: undefined }, () => ({ error: FORM_CONTROL_ERRORS.valueRequired })],
    ['value not a string', { value: 50 }, () => ({ error: FORM_CONTROL_ERRORS.valueNotString })],
    ['frames not an array', { frames: '50' }, () => ({ error: FORM_CONTROL_ERRORS.framesNotStrings })],
    [
      'frames holding a non-string',
      { frames: ['50', 60] },
      () => ({ error: FORM_CONTROL_ERRORS.framesNotStrings }),
    ],
    [
      'more frames than the cap',
      { frames: tooMany },
      () => ({
        error: FORM_CONTROL_ERRORS.tooManyFrames(MAX_FORM_CONTROL_FRAMES + 1, MAX_FORM_CONTROL_FRAMES),
      }),
    ],
  ])('%s is refused and drives nothing', async (_label, override, expected) => {
    const { controls, events } = mountControls(t.html);
    const before = controls[0].value;

    const result = await dispatchCmd(10, t.tool, withParams(valid, override));

    expect(result).toEqual(expected());
    expect(events).toEqual([]);
    expect(controls[0].value).toBe(before);
  });

  it('checks selector, then commit, then value, then frames', async () => {
    const { events } = mountControls(t.html);
    const allWrong = { selector: 3, commit: 'bogus', value: 5, frames: [7] };

    expect(await dispatchCmd(11, t.tool, allWrong)).toEqual({ error: 'selector is required' });
    // A selector that matches NOTHING: were resolution first, this would be notFound.
    const resolvable = { ...allWrong, selector: NOTHING_SELECTOR };
    expect(await dispatchCmd(12, t.tool, resolvable)).toEqual({
      error: FORM_CONTROL_ERRORS.commitNotAllowed('bogus', t.spec.commits),
    });
    expect(await dispatchCmd(13, t.tool, { ...resolvable, commit: t.commit })).toEqual({
      error: FORM_CONTROL_ERRORS.valueNotString,
    });
    expect(
      await dispatchCmd(14, t.tool, { ...resolvable, commit: t.commit, value: '50' }),
    ).toEqual({ error: FORM_CONTROL_ERRORS.framesNotStrings });
    expect(events).toEqual([]);
  });
});

describe('strict resolution: exactly one element, never a guess', () => {
  it('an invalid CSS selector answers the DOMException message, as resolveElement does', async () => {
    let message = '';
    try {
      document.querySelectorAll('[[');
    } catch (e) {
      message = (e as Error).message;
    }
    expect(message).not.toBe('');

    const result = await dispatchCmd(20, 'scrub_range_input', {
      selector: '[[',
      value: '50',
      commit: 'change',
    });

    expect(result).toEqual({ error: message });
  });

  it('zero matches is notFound', async () => {
    const { events } = mountControls(RANGE_HTML);

    const result = await dispatchCmd(21, 'scrub_range_input', {
      selector: NOTHING_SELECTOR,
      value: '50',
      commit: 'change',
    });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.notFound(NOTHING_SELECTOR) });
    expect(events).toEqual([]);
  });

  it('two matches is ambiguous, and NEITHER control is driven (the N-joints / N-rows case)', async () => {
    const { controls, events } = mountControls(RANGE_HTML + RANGE_HTML);

    const result = await dispatchCmd(22, 'scrub_range_input', {
      selector: HOST_SELECTOR,
      value: '50',
      commit: 'change',
    });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.ambiguous(HOST_SELECTOR, 2) });
    expect(events).toEqual([]);
    expect(controls.map((c) => c.value)).toEqual(['100', '100']);
  });
});

describe('control checks', () => {
  it.each([
    ['a <div>', 'scrub_range_input', '<div></div>', 'change', 'range', '<div>'],
    ['the range tool on a text input', 'scrub_range_input', TEXT_HTML, 'change', 'range', '<input type="text">'],
    ['the text tool on a range input', 'edit_text_input', RANGE_HTML, 'enter', 'text', '<input type="range">'],
  ])('%s is wrongControl', async (_label, tool, html, commit, expectedType, actual) => {
    const { events } = mountControls(html);

    const result = await dispatchCmd(30, tool, { selector: HOST_SELECTOR, value: '50', commit });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.wrongControl(expectedType, actual) });
    expect(events).toEqual([]);
  });

  it.each([
    ['disabled', 'scrub_range_input', RANGE_HTML.replace('>', ' disabled>'), 'change', 'disabled'],
    ['readOnly', 'edit_text_input', TEXT_HTML.replace('>', ' readonly>'), 'enter', 'read-only'],
  ] as const)('a %s control is notEditable', async (_label, tool, html, commit, state) => {
    const { controls, events } = mountControls(html);
    const before = controls[0].value;

    const result = await dispatchCmd(31, tool, { selector: HOST_SELECTOR, value: '50', commit });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.notEditable(state) });
    expect(events).toEqual([]);
    expect(controls[0].value).toBe(before);
  });
});

describe('representability preflight: refused before a single event fires', () => {
  it('a range value outside max is refused with its sanitised read-back, and .value is restored', async () => {
    // jsdom's range sanitisation CLAMPS to min/max natively (measured: '999'
    // reads back '200' on min=0 max=200), so no stub is needed here.
    const { controls, events } = mountControls(RANGE_HTML);

    const result = await dispatchCmd(40, 'scrub_range_input', {
      selector: HOST_SELECTOR,
      value: '999',
      commit: 'change',
    });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.notRepresentable('999', '200') });
    expect(events).toEqual([]);
    expect(controls[0].value).toBe('100');
  });

  it('an off-step FRAME is refused too, before the earlier frames are typed', async () => {
    // jsdom does NOT snap a range value to its step (measured: '12.5' reads back
    // '12.5' on step=1), so a browser's step snapping is pinned by an
    // instance-level accessor that rounds. Safe on <input>, unlike the proxied
    // <select> (see the set_fea_channel read-back test in debugBridge.test.tsx).
    const { controls, events } = mountControls(RANGE_HTML);
    const el = controls[0];
    const native = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!;
    Object.defineProperty(el, 'value', {
      configurable: true,
      get(this: HTMLInputElement) {
        return native.get!.call(this);
      },
      set(this: HTMLInputElement, v: string) {
        native.set!.call(this, String(Math.round(Number(v))));
      },
    });

    const result = await dispatchCmd(41, 'scrub_range_input', {
      selector: HOST_SELECTOR,
      value: '100',
      frames: ['50', '12.5'],
      commit: 'change',
    });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.notRepresentable('12.5', '13') });
    expect(events).toEqual([]);
    expect(el.value).toBe('100');
  });

  it('a text value the input strips (a newline) is refused', async () => {
    const { controls, events } = mountControls(TEXT_HTML);

    const result = await dispatchCmd(42, 'edit_text_input', {
      selector: HOST_SELECTOR,
      value: '15\n0mm',
      commit: 'enter',
    });

    expect(result).toEqual({ error: FORM_CONTROL_ERRORS.notRepresentable('15\n0mm', '150mm') });
    expect(events).toEqual([]);
    expect(controls[0].value).toBe('10mm');
  });
});

// ── A control replaced mid-gesture ───────────────────────────────────────────
//
// Measured live (task 7680): the app can unmount the very <input> a gesture is
// driving, a few frames in. Events dispatched to a detached node reach no
// delegated handler, so carrying on would answer {ok:true} for a gesture the
// application never saw.

describe('a control that leaves the document mid-gesture is refused, never a false ok', () => {
  it.each([
    { tool: 'scrub_range_input', html: RANGE_HTML, frames: ['50', '60', '70'], value: '80', commit: 'change', removeOnInput: 2, events: ['focus', 'input', 'input'] },
    { tool: 'scrub_range_input', html: RANGE_HTML, frames: [], value: '80', commit: 'change', removeOnInput: 1, events: ['focus', 'input'] },
    { tool: 'edit_text_input', html: TEXT_HTML, frames: ['1', '15'], value: '150mm', commit: 'enter', removeOnInput: 1, events: ['focus', 'input'] },
  ])(
    '$tool: removed on input $removeOnInput, then nothing more is dispatched',
    async ({ tool, html, frames, value, commit, removeOnInput, events: expected }) => {
      const { controls, events } = mountControls(html);
      let inputs = 0;
      controls[0].addEventListener('input', () => {
        inputs += 1;
        if (inputs === removeOnInput) controls[0].parentElement!.remove();
      });

      const result = await dispatchCmd(50, tool, { selector: HOST_SELECTOR, value, frames, commit });

      expect(result).toEqual({
        error: FORM_CONTROL_ERRORS.detached(removeOnInput, frames.length + 1),
      });
      expect(events).toEqual(expected);
    },
  );
});
