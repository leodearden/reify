/**
 * Colormap utility — pure-JS, no Three.js or FEA dependency.
 *
 * Public API:
 *   - `applyColormap(value, range, palette, options?)` → [r, g, b] floats in [0, 1]
 *   - `bakeColours(scalars, range, palette, options?)` → Float32Array(N*3), interleaved RGB
 *   - LUT constants: `viridisLut`, `magmaLut`, `rainbowLut` (Float32Array of 768 values each)
 *
 * Out-of-range defaults: above-max → black (0,0,0), below-min → grey (0.5,0.5,0.5),
 * NaN → grey (0.5,0.5,0.5). All configurable via `ColormapOptions`.
 *
 * Range modes (auto / fixed / locked) share identical min/max clamp logic.
 * The `locked.source` provenance string is inert in this module — UI labelling only.
 */

export { viridisLut } from './colormaps/viridis';
export { magmaLut }   from './colormaps/magma';
export { rainbowLut } from './colormaps/rainbow';

import { viridisLut } from './colormaps/viridis';
import { magmaLut }   from './colormaps/magma';
import { rainbowLut } from './colormaps/rainbow';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Supported palette names. */
export type Palette = 'viridis' | 'magma' | 'rainbow';

/**
 * Scalar range for colormap normalization.
 *
 * All three modes carry `min` and `max` — the colormap math is identical.
 * The discriminator conveys UI intent:
 *   - `auto`   — bounds were auto-computed by the caller from the scalar field.
 *   - `fixed`  — bounds are user-specified and held constant.
 *   - `locked` — bounds are locked with provenance; `source` is a display label.
 */
export type Range =
  | { mode: 'auto';   min: number; max: number }
  | { mode: 'fixed';  min: number; max: number }
  | { mode: 'locked'; min: number; max: number; source: string };

/**
 * Per-call saturation and NaN colour overrides.
 * All values are normalized RGB floats in [0, 1].
 */
export interface ColormapOptions {
  /** Colour returned when `value > range.max`. Default: black `[0, 0, 0]`. */
  aboveColor?: readonly [number, number, number];
  /** Colour returned when `value < range.min`. Default: grey `[0.5, 0.5, 0.5]`. */
  belowColor?: readonly [number, number, number];
  /** Colour returned when `value` is NaN. Default: grey `[0.5, 0.5, 0.5]`. */
  nanColor?:   readonly [number, number, number];
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

const DEFAULT_ABOVE: readonly [number, number, number] = [0, 0, 0];
const DEFAULT_BELOW: readonly [number, number, number] = [0.5, 0.5, 0.5];
const DEFAULT_NAN:   readonly [number, number, number] = [0.5, 0.5, 0.5];

function resolveLut(palette: Palette): Readonly<Float32Array> {
  switch (palette) {
    case 'viridis': return viridisLut;
    case 'magma':   return magmaLut;
    case 'rainbow': return rainbowLut;
  }
}

/**
 * Linearly interpolate a single scalar through the LUT.
 * `t` is pre-clamped to [0, 1] by the caller.
 */
function lutLerp(lut: Readonly<Float32Array>, t: number): [number, number, number] {
  const f   = t * 255;
  const lo  = Math.floor(f);
  const hi  = Math.min(lo + 1, 255);
  const frac = f - lo;
  const r = lut[lo * 3]     + frac * (lut[hi * 3]     - lut[lo * 3]);
  const g = lut[lo * 3 + 1] + frac * (lut[hi * 3 + 1] - lut[lo * 3 + 1]);
  const b = lut[lo * 3 + 2] + frac * (lut[hi * 3 + 2] - lut[lo * 3 + 2]);
  return [r, g, b];
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Map a single scalar value to an RGB colour using the given palette and range.
 *
 * @param value   - Scalar to map (NaN is handled gracefully).
 * @param range   - Min/max bounds for normalization (mode is inert here).
 * @param palette - Palette name: 'viridis' | 'magma' | 'rainbow'.
 * @param options - Optional saturation/NaN colour overrides.
 * @returns       Tuple `[r, g, b]` of normalized floats in [0, 1].
 *
 * @note Precondition: `range.min <= range.max` (inverted range is not validated).
 *       Non-finite bounds (`NaN`, `Infinity`) return the `nanColor` sentinel.
 *
 * @note For bulk conversion over large scalar arrays, use `bakeColours` — it writes
 *       directly into a `Float32Array` and avoids a per-element tuple allocation.
 *       `applyColormap` is intended for one-off and UI use.
 */
export function applyColormap(
  value:   number,
  range:   Range,
  palette: Palette,
  options?: ColormapOptions,
): [number, number, number] {
  const aboveColor = options?.aboveColor ?? DEFAULT_ABOVE;
  const belowColor = options?.belowColor ?? DEFAULT_BELOW;
  const nanColor   = options?.nanColor   ?? DEFAULT_NAN;

  // Guard: non-finite bounds produce undefined lerp results; return NaN colour.
  if (!Number.isFinite(range.min) || !Number.isFinite(range.max)) {
    return [nanColor[0], nanColor[1], nanColor[2]];
  }

  // NaN check must come first — comparisons with NaN are always false.
  if (Number.isNaN(value)) return [nanColor[0], nanColor[1], nanColor[2]];
  if (value > range.max)   return [aboveColor[0], aboveColor[1], aboveColor[2]];
  if (value < range.min)   return [belowColor[0], belowColor[1], belowColor[2]];

  const span = range.max - range.min;
  const t = span === 0 ? 0 : (value - range.min) / span;
  return lutLerp(resolveLut(palette), t);
}

/**
 * Map an array of scalars to interleaved RGB colours in a single allocation.
 *
 * Output layout: `[R0, G0, B0, R1, G1, B1, …]` — suitable for direct
 * assignment into a Three.js `BufferAttribute('color', 3)`.
 *
 * @param scalars - Input scalar values (Float32Array or any array-like of numbers).
 * @param range   - Min/max bounds for normalization.
 * @param palette - Palette name.
 * @param options - Optional saturation/NaN colour overrides.
 * @returns       `Float32Array` of length `scalars.length * 3`.
 */
export function bakeColours(
  scalars: ArrayLike<number>,
  range:   Range,
  palette: Palette,
  options?: ColormapOptions,
): Float32Array {
  const out = new Float32Array(scalars.length * 3);
  const lut = resolveLut(palette);

  const aboveColor = options?.aboveColor ?? DEFAULT_ABOVE;
  const belowColor = options?.belowColor ?? DEFAULT_BELOW;
  const nanColor   = options?.nanColor   ?? DEFAULT_NAN;

  // Guard: non-finite bounds produce undefined lerp results; fill all with NaN colour.
  if (!Number.isFinite(range.min) || !Number.isFinite(range.max)) {
    for (let i = 0; i < scalars.length; i++) {
      out[i * 3]     = nanColor[0];
      out[i * 3 + 1] = nanColor[1];
      out[i * 3 + 2] = nanColor[2];
    }
    return out;
  }

  const span = range.max - range.min;

  for (let i = 0; i < scalars.length; i++) {
    const v = scalars[i];
    let r: number, g: number, b: number;

    if (Number.isNaN(v)) {
      r = nanColor[0]; g = nanColor[1]; b = nanColor[2];
    } else if (v > range.max) {
      r = aboveColor[0]; g = aboveColor[1]; b = aboveColor[2];
    } else if (v < range.min) {
      r = belowColor[0]; g = belowColor[1]; b = belowColor[2];
    } else {
      // Lerp inlined from lutLerp() to avoid allocating a [r,g,b] tuple per
      // element — bakeColours is the hot path for large meshes (10k–100k vertices).
      const t  = span === 0 ? 0 : (v - range.min) / span;
      const f  = t * 255;
      const lo = Math.floor(f);
      const hi = Math.min(lo + 1, 255);
      const frac = f - lo;
      r = lut[lo * 3]     + frac * (lut[hi * 3]     - lut[lo * 3]);
      g = lut[lo * 3 + 1] + frac * (lut[hi * 3 + 1] - lut[lo * 3 + 1]);
      b = lut[lo * 3 + 2] + frac * (lut[hi * 3 + 2] - lut[lo * 3 + 2]);
    }

    out[i * 3]     = r;
    out[i * 3 + 1] = g;
    out[i * 3 + 2] = b;
  }

  return out;
}
