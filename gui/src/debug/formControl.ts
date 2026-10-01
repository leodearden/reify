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

export interface FormControlResult {
  ok: true;
  /** The control's `.value` after the gesture: reported, not judged. */
  value: string;
  inputEvents: number;
  commit: FormControlCommit;
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

export async function driveFormControl(
  control: FormControlSpec,
  params: Record<string, unknown>,
): Promise<FormControlResult> {
  const selector = params.selector as string;
  const commit = params.commit as FormControlCommit;
  const values = [...((params.frames as string[] | undefined) ?? []), params.value as string];
  const el = document.querySelectorAll(selector)[0] as HTMLInputElement;

  el.dispatchEvent(new FocusEvent('focus'));
  await typeValues(el, values);
  const terminal = TERMINAL_EVENTS[commit];
  if (terminal) el.dispatchEvent(terminal());
  return { ok: true, value: el.value, inputEvents: values.length, commit };
}
