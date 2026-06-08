/**
 * Panel layout persistence via localStorage.
 * Pure functions for loading and saving panel dimensions.
 */

export const STORAGE_KEY = 'reify-panel-layout';

export type PanelLayout = {
  editorWidth: number;
  sideWidth: number;
  designTreeHeight: number;
  propertyHeight: number;
  constraintHeight: number;
  problemsHeight: number;
  problemsCollapsed: boolean;
};

/** Load persisted panel layout from localStorage. Returns null if missing, invalid, or incomplete.
 *  Missing sub-panel heights fall back to `undefined` so the caller can apply defaults —
 *  this keeps older saved layouts forward-compatible when new panels become resizable. */
export function loadPanelLayout(): Partial<PanelLayout> | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (
      typeof parsed !== 'object' ||
      parsed === null ||
      typeof parsed.editorWidth !== 'number' ||
      typeof parsed.sideWidth !== 'number'
    ) {
      return null;
    }

    const out: Partial<PanelLayout> = {
      editorWidth: parsed.editorWidth,
      sideWidth: parsed.sideWidth,
    };
    if (typeof parsed.designTreeHeight === 'number') out.designTreeHeight = parsed.designTreeHeight;
    if (typeof parsed.propertyHeight === 'number') out.propertyHeight = parsed.propertyHeight;
    if (typeof parsed.constraintHeight === 'number') out.constraintHeight = parsed.constraintHeight;
    if (typeof parsed.problemsHeight === 'number') out.problemsHeight = parsed.problemsHeight;
    if (typeof parsed.problemsCollapsed === 'boolean') out.problemsCollapsed = parsed.problemsCollapsed;
    return out;
  } catch {
    return null;
  }
}

/** Save panel layout to localStorage. */
export function savePanelLayout(layout: PanelLayout): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}

export type SidePanelHeights = {
  designTree: number;
  property: number;
  constraint: number;
};

export type ClampOptions = {
  chatOpen: boolean;
  chatMinHeight: number;
  minPanelHeight: number;
  splitterThickness: number;
};

/**
 * Clamp side-panel sub-panel heights so that designTree + property + constraint
 * + splitters + chat-floor fit within `containerHeight`.
 *
 * Pure: no DOM, no signals. The caller writes the result back to its signals
 * if any value changed. See the side-panel-clamp PR for rationale.
 */
export function clampPanelHeightsToFit(
  preferred: SidePanelHeights,
  containerHeight: number,
  opts: ClampOptions,
): SidePanelHeights {
  const splitters = (opts.chatOpen ? 3 : 2) * opts.splitterThickness;
  const chatFloor = opts.chatOpen ? opts.chatMinHeight : 0;
  const available = containerHeight - splitters - chatFloor;
  const sum = preferred.designTree + preferred.property + preferred.constraint;

  if (sum <= available) return preferred;

  // Pathologically small container — three panels at MIN_PANEL_HEIGHT plus
  // chatFloor and splitters won't fit. Return the floor; parent CSS
  // (`.sidePanel { overflow: hidden }`) will clip.
  if (available < 3 * opts.minPanelHeight) {
    return {
      designTree: opts.minPanelHeight,
      property: opts.minPanelHeight,
      constraint: opts.minPanelHeight,
    };
  }

  // Proportionally scale, then raise any below MIN to MIN.
  const scale = available / sum;
  const heights = [
    Math.max(opts.minPanelHeight, Math.floor(preferred.designTree * scale)),
    Math.max(opts.minPanelHeight, Math.floor(preferred.property * scale)),
    Math.max(opts.minPanelHeight, Math.floor(preferred.constraint * scale)),
  ];

  // Raising to MIN may push the post-floor sum back over available.
  // Subtract from the largest panel(s) until it fits, never going below MIN.
  let postSum = heights[0] + heights[1] + heights[2];
  while (postSum > available) {
    let largestIdx = -1;
    let largestVal = -1;
    for (let i = 0; i < 3; i++) {
      if (heights[i] > opts.minPanelHeight && heights[i] > largestVal) {
        largestIdx = i;
        largestVal = heights[i];
      }
    }
    if (largestIdx < 0) break; // all already at MIN
    const reduction = Math.min(postSum - available, heights[largestIdx] - opts.minPanelHeight);
    heights[largestIdx] -= reduction;
    postSum -= reduction;
  }

  return {
    designTree: heights[0],
    property: heights[1],
    constraint: heights[2],
  };
}
