// Engineering rainbow colormap — 256 RGB triplets, values in [0, 1].
// Generated at module load via an HSV sweep: hue 240° → 0° (blue → red),
// S=V=1. Matches the engineering "blue=cold, red=hot" convention used by
// Matlab and ParaView. Computed once and frozen — no runtime cost per lookup.

function buildRainbow(): Float32Array {
  const out = new Float32Array(768);
  for (let i = 0; i < 256; i++) {
    // hue sweeps from 240° (blue) down to 0° (red)
    const h = (240 * (255 - i)) / 255;
    const c = 1; // chroma: S=V=1, so c = V*S = 1
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    let r = 0, g = 0, b = 0;
    if      (h < 60)  { r = c; g = x; b = 0; }
    else if (h < 120) { r = x; g = c; b = 0; }
    else if (h < 180) { r = 0; g = c; b = x; }
    else if (h < 240) { r = 0; g = x; b = c; }
    else              { r = 0; g = 0; b = c; }
    // m = V - c = 0 since S=V=1
    out[i * 3]     = r;
    out[i * 3 + 1] = g;
    out[i * 3 + 2] = b;
  }
  return out;
}

/** Engineering rainbow LUT: Float32Array of 768 values (256 × RGB, interleaved).
 *  Hue sweeps 240° → 0° (blue → red), S=V=1. */
export const rainbowLut: Float32Array = buildRainbow();
