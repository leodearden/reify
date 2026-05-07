import pixelmatch from "pixelmatch";

/** Raw image data as width × height × 4 bytes (RGBA). */
export interface ImageData {
  width: number;
  height: number;
  rgba: Buffer;
}

/** Options controlling comparison tolerance. */
export interface CompareOptions {
  /** Per-pixel YIQ colour-difference threshold (0–1). */
  pixelThreshold: number;
  /** Maximum fraction of mismatched pixels allowed before status becomes "diff". */
  mismatchPctLimit: number;
}

/** Result returned by compareImages. */
export interface CompareResult {
  status: "match" | "diff" | "dimension-mismatch";
  mismatchedPixels: number;
  mismatchPct: number;
  /** Rendered diff image (RGBA) or null when unavailable (e.g. dimension-mismatch). */
  diffRgba: Buffer | null;
}

/**
 * Compare two images pixel-by-pixel using pixelmatch.
 *
 * Returns "dimension-mismatch" immediately when the images differ in size.
 * Otherwise runs pixelmatch with opts.pixelThreshold and classifies the
 * result: "match" when mismatchPct ≤ opts.mismatchPctLimit, "diff" otherwise.
 */
export function compareImages(
  baseline: ImageData,
  captured: ImageData,
  opts: CompareOptions,
): CompareResult {
  if (baseline.width !== captured.width || baseline.height !== captured.height) {
    return {
      status: "dimension-mismatch",
      mismatchedPixels: 0,
      mismatchPct: 0,
      diffRgba: null,
    };
  }

  const { width, height } = baseline;
  const diffRgba = Buffer.alloc(width * height * 4);

  const mismatchedPixels = pixelmatch(
    baseline.rgba,
    captured.rgba,
    diffRgba,
    width,
    height,
    { threshold: opts.pixelThreshold },
  );

  const mismatchPct = mismatchedPixels / (width * height);
  const status = mismatchPct <= opts.mismatchPctLimit ? "match" : "diff";

  return { status, mismatchedPixels, mismatchPct, diffRgba };
}

// ─── Outcome types ────────────────────────────────────────────────────────────

export type Outcome =
  | { kind: "baseline-created"; reason: "missing" | "update-flag" }
  | { kind: "passed"; mismatchedPixels: number; mismatchPct: number }
  | {
      kind: "failed";
      reason: "dimension-mismatch";
      baselineWidth: number;
      baselineHeight: number;
      capturedWidth: number;
      capturedHeight: number;
    }
  | {
      kind: "failed";
      reason: "tolerance-exceeded";
      mismatchedPixels: number;
      mismatchPct: number;
      diffRgba: Buffer;
    };

export interface DecideOptions extends CompareOptions {
  updateBaselines: boolean;
}

/**
 * Decide the outcome of a visual regression check.
 *
 * Logic:
 * - null baseline → baseline-created:missing (caller must write the captured PNG)
 * - updateBaselines=true → baseline-created:update-flag (caller must overwrite baseline)
 * - otherwise → run compareImages and map status to passed / failed
 */
export function decideOutcome(
  baseline: ImageData | null,
  captured: ImageData,
  opts: DecideOptions,
): Outcome {
  if (baseline === null) {
    return { kind: "baseline-created", reason: "missing" };
  }

  if (opts.updateBaselines) {
    return { kind: "baseline-created", reason: "update-flag" };
  }

  const result = compareImages(baseline, captured, opts);

  switch (result.status) {
    case "match":
      return {
        kind: "passed",
        mismatchedPixels: result.mismatchedPixels,
        mismatchPct: result.mismatchPct,
      };

    case "dimension-mismatch":
      return {
        kind: "failed",
        reason: "dimension-mismatch",
        baselineWidth: baseline.width,
        baselineHeight: baseline.height,
        capturedWidth: captured.width,
        capturedHeight: captured.height,
      };

    case "diff":
      return {
        kind: "failed",
        reason: "tolerance-exceeded",
        mismatchedPixels: result.mismatchedPixels,
        mismatchPct: result.mismatchPct,
        diffRgba: result.diffRgba!, // non-null when status is "diff"
      };
  }
}
