/**
 * Panel layout persistence via localStorage: raw read/write of the saved
 * layout plus pure clamp helpers. The layout's schema and validation live with
 * LAYOUT_DIMENSIONS in stores/layoutStore.ts.
 */

export const STORAGE_KEY = 'reify-panel-layout';

/** The JSON value stored under STORAGE_KEY, or null when absent or unparseable. Not validated. */
export function readStoredPanelLayout(): unknown {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw === null ? null : JSON.parse(raw);
  } catch {
    return null;
  }
}

/** Save panel layout to localStorage. */
export function savePanelLayout(layout: Readonly<Record<string, number | boolean>>): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}

export type ProblemsClampOptions = {
  minPanelHeight: number;
  editorMinHeight: number;
  splitterThickness: number;
};

/**
 * Clamp the problems panel height so that the editor area keeps at least
 * `editorMinHeight` and the splitter itself fits.
 *
 * available = containerHeight - editorMinHeight - splitterThickness
 * Returns Math.max(minPanelHeight, Math.min(preferred, available)).
 * When the container is too small (available < minPanelHeight), returns
 * minPanelHeight — the parent CSS overflow:hidden will clip.
 *
 * Pure: no DOM, no signals.
 */
export function clampProblemsHeight(
  preferred: number,
  containerHeight: number,
  opts: ProblemsClampOptions,
): number {
  const available = containerHeight - opts.editorMinHeight - opts.splitterThickness;
  return Math.max(opts.minPanelHeight, Math.min(preferred, available));
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
