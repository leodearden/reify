/**
 * Drive a native form control's value the way its own handlers observe a user
 * doing it: assign `.value`, dispatch the DOM events the control binds, then
 * end the gesture with a terminal event.
 *
 * DOM only. No Tauri command and no component function is called, so what a
 * caller asserts is the application's handling of those events, not the
 * browser's thumb or caret behaviour. Contract: docs/debug-mcp-contract.md §4.
 */

/** How a gesture ends. `hold` ends it without a terminal event: a pointer still down. */
export type FormControlCommit = 'change' | 'enter' | 'blur' | 'hold';

/** What one tool drives: the `<input type>` it accepts and the commits it allows. */
export interface FormControlSpec {
  readonly inputType: string;
  readonly commits: readonly FormControlCommit[];
}

export const RANGE_INPUT: FormControlSpec = Object.freeze({
  inputType: 'range',
  commits: Object.freeze(['change', 'hold'] as const),
});

export const TEXT_INPUT: FormControlSpec = Object.freeze({
  inputType: 'text',
  commits: Object.freeze(['enter', 'blur', 'hold'] as const),
});

/**
 * The event each commit dispatches. Solid delegates `input` and `keydown` to
 * the document, so those bubble; `change`, `focus` and `blur` are bound
 * directly on the element.
 */
const TERMINAL_EVENTS: Readonly<Record<FormControlCommit, (() => Event) | null>> = Object.freeze({
  change: () => new Event('change', { bubbles: true }),
  enter: () => new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
  blur: () => new FocusEvent('blur'),
  hold: null,
});

/**
 * Each refusal's wording, shared with the tests that pin it. `selectorRequired`
 * is THE BOUNDARY RULE's whole-selector wording (bridge.ts), deliberately the
 * same string resolveElement answers.
 */
export const FORM_CONTROL_ERRORS = {
  selectorRequired: 'selector is required',
  commitRequired: 'commit is required',
  commitNotAllowed: (commit: unknown, allowed: readonly string[]) =>
    `commit ${JSON.stringify(commit)} is not accepted by this tool; expected one of ${allowed
      .map((c) => `"${c}"`)
      .join(', ')}`,
  valueRequired: 'value is required',
  valueNotString: 'value must be a string',
  framesNotStrings: 'frames must be an array of strings',
  tooManyFrames: (count: number, max: number) =>
    `frames has ${count} entries; at most ${max} are accepted`,
  notFound: (selector: string) => `no element matches selector ${JSON.stringify(selector)}`,
  ambiguous: (selector: string, count: number) =>
    `selector ${JSON.stringify(selector)} matches ${count} elements; it must match exactly one`,
  wrongControl: (expectedType: string, actual: string) =>
    `expected <input type="${expectedType}">, found ${actual}`,
  notEditable: (state: 'disabled' | 'read-only') => `control is ${state}`,
  notRepresentable: (requested: string, sanitised: string) =>
    `value ${JSON.stringify(requested)} is not representable by this control (it reads back as ${JSON.stringify(sanitised)}); no event was dispatched`,
} as const;

/**
 * Each frame awaits one animation frame, so 60 is about one second at 60 Hz:
 * comfortably inside query_frontend's 5 s default timeout.
 */
export const MAX_FORM_CONTROL_FRAMES = 60;

export interface FormControlResult {
  ok: true;
  /** The control's `.value` after the gesture: reported, not judged. */
  value: string;
  inputEvents: number;
  commit: FormControlCommit;
}

interface Refusal {
  error: string;
}

/** A validated request: the values to type, in order, and how the gesture ends. */
interface Gesture {
  selector: string;
  values: readonly string[];
  commit: FormControlCommit;
}

function parseCommit(control: FormControlSpec, commit: unknown): FormControlCommit | Refusal {
  if (commit === undefined) return { error: FORM_CONTROL_ERRORS.commitRequired };
  const allowed = control.commits.find((c) => c === commit);
  return allowed ?? { error: FORM_CONTROL_ERRORS.commitNotAllowed(commit, control.commits) };
}

/** `[...frames, value]`, every one a string, the frames within the cap. */
function parseValues(value: unknown, frames: unknown): string[] | Refusal {
  if (value === undefined) return { error: FORM_CONTROL_ERRORS.valueRequired };
  if (typeof value !== 'string') return { error: FORM_CONTROL_ERRORS.valueNotString };
  const typed = frames === undefined ? [] : frames;
  if (!Array.isArray(typed) || !typed.every((f) => typeof f === 'string')) {
    return { error: FORM_CONTROL_ERRORS.framesNotStrings };
  }
  if (typed.length > MAX_FORM_CONTROL_FRAMES) {
    return { error: FORM_CONTROL_ERRORS.tooManyFrames(typed.length, MAX_FORM_CONTROL_FRAMES) };
  }
  return [...typed, value];
}

/** Validate every param, selector first, before anything touches the DOM. */
function parseGesture(control: FormControlSpec, params: Record<string, unknown>): Gesture | Refusal {
  const selector = params.selector;
  if (typeof selector !== 'string' || selector === '') {
    return { error: FORM_CONTROL_ERRORS.selectorRequired };
  }
  const commit = parseCommit(control, params.commit);
  if (typeof commit !== 'string') return commit;
  const values = parseValues(params.value, params.frames);
  if (!Array.isArray(values)) return values;
  return { selector, values, commit };
}

/**
 * The ONE element the selector matches. Stricter than resolveElement's first
 * match, for pickFeaChannelSelect's reason: guessing between N form controls
 * would misapply a value silently.
 */
function resolveSoleElement(selector: string): Element | Refusal {
  let matches: NodeListOf<Element>;
  try {
    matches = document.querySelectorAll(selector);
  } catch (e) {
    return { error: (e as Error).message };
  }
  if (matches.length === 0) return { error: FORM_CONTROL_ERRORS.notFound(selector) };
  if (matches.length > 1) return { error: FORM_CONTROL_ERRORS.ambiguous(selector, matches.length) };
  return matches[0];
}

function describeElement(el: Element): string {
  return el instanceof HTMLInputElement
    ? `<input type="${el.type}">`
    : `<${el.tagName.toLowerCase()}>`;
}

/** The element as the editable `<input>` this tool drives. */
function asEditableControl(control: FormControlSpec, el: Element): HTMLInputElement | Refusal {
  if (!(el instanceof HTMLInputElement) || el.type !== control.inputType) {
    return { error: FORM_CONTROL_ERRORS.wrongControl(control.inputType, describeElement(el)) };
  }
  if (el.disabled) return { error: FORM_CONTROL_ERRORS.notEditable('disabled') };
  if (el.readOnly) return { error: FORM_CONTROL_ERRORS.notEditable('read-only') };
  return el;
}

/**
 * The first value the browser would not hold verbatim (clamped, step-snapped,
 * stripped). Assigns without dispatching, and always restores the original.
 */
function findUnrepresentable(el: HTMLInputElement, values: readonly string[]): Refusal | null {
  const original = el.value;
  try {
    for (const v of values) {
      el.value = v;
      if (el.value !== v) return { error: FORM_CONTROL_ERRORS.notRepresentable(v, el.value) };
    }
    return null;
  } finally {
    el.value = original;
  }
}

function nextAnimationFrame(): Promise<void> {
  return new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

/** Type each value in turn, one `input` event and one animation frame apiece. */
async function typeValues(el: HTMLInputElement, values: readonly string[]): Promise<void> {
  for (const v of values) {
    el.value = v;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    await nextAnimationFrame();
  }
}

async function performGesture(el: HTMLInputElement, gesture: Gesture): Promise<FormControlResult> {
  el.dispatchEvent(new FocusEvent('focus'));
  await typeValues(el, gesture.values);
  const terminal = TERMINAL_EVENTS[gesture.commit];
  if (terminal) el.dispatchEvent(terminal());
  return { ok: true, value: el.value, inputEvents: gesture.values.length, commit: gesture.commit };
}

export async function driveFormControl(
  control: FormControlSpec,
  params: Record<string, unknown>,
): Promise<FormControlResult | Refusal> {
  const gesture = parseGesture(control, params);
  if ('error' in gesture) return gesture;
  // `instanceof`, not `'error' in`: an HTMLMediaElement has an `error` property.
  const el = resolveSoleElement(gesture.selector);
  if (!(el instanceof Element)) return el;
  const input = asEditableControl(control, el);
  if (!(input instanceof HTMLInputElement)) return input;
  return findUnrepresentable(input, gesture.values) ?? performGesture(input, gesture);
}
