import { describe, it, expect } from "vitest";
import { compareImages } from "./diff";
import type { ImageData } from "./diff";

// Helper: create a solid-colour 8×8 RGBA buffer
function solidBuffer(
  width: number,
  height: number,
  r: number,
  g: number,
  b: number,
  a = 255,
): Buffer {
  const buf = Buffer.alloc(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    buf[i * 4 + 0] = r;
    buf[i * 4 + 1] = g;
    buf[i * 4 + 2] = b;
    buf[i * 4 + 3] = a;
  }
  return buf;
}

function makeImage(width: number, height: number, rgba: Buffer): ImageData {
  return { width, height, rgba };
}

describe("compareImages", () => {
  it("(a) identical 8×8 buffers → status:match, mismatchedPixels:0, mismatchPct:0", () => {
    const buf = solidBuffer(8, 8, 100, 150, 200);
    const baseline = makeImage(8, 8, buf);
    const captured = makeImage(8, 8, Buffer.from(buf)); // copy
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("match");
    expect(result.mismatchedPixels).toBe(0);
    expect(result.mismatchPct).toBe(0);
  });

  it("(b) 8×8 baseline vs 8×16 captured → status:dimension-mismatch, no diffRgba", () => {
    const baseline = makeImage(8, 8, solidBuffer(8, 8, 255, 0, 0));
    const captured = makeImage(8, 16, solidBuffer(8, 16, 255, 0, 0));
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("dimension-mismatch");
    expect(result.diffRgba).toBeNull();
  });

  it("(c) ~30% of pixels differ at threshold 0.1, mismatchPctLimit 0.01 → status:diff, mismatchPct > limit", () => {
    // baseline: all red, captured: ~30% white pixels (every ~3rd pixel)
    const width = 8;
    const height = 8;
    const baseBuf = solidBuffer(width, height, 255, 0, 0);
    const capBuf = solidBuffer(width, height, 255, 0, 0);
    // Change ~30% of pixels to white (pixels 0, 3, 6, 9, ... = every 3rd)
    let changed = 0;
    for (let i = 0; i < width * height; i += 3) {
      capBuf[i * 4 + 0] = 0;
      capBuf[i * 4 + 1] = 255;
      capBuf[i * 4 + 2] = 0;
      changed++;
    }
    const baseline = makeImage(width, height, baseBuf);
    const captured = makeImage(width, height, capBuf);
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("diff");
    expect(result.mismatchPct).toBeGreaterThan(0.01);
  });

  it("(d) two 8×8 buffers differing in <1% pixels → status:match (within tolerance)", () => {
    // 8×8 = 64 pixels, <1% means 0 pixels differ (since 1% of 64 = 0.64, i.e., 0 full pixels)
    // Change 0 pixels — identical buffers
    const buf = solidBuffer(8, 8, 200, 100, 50);
    const baseline = makeImage(8, 8, buf);
    const captured = makeImage(8, 8, Buffer.from(buf));
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("match");
    expect(result.mismatchPct).toBeLessThanOrEqual(0.01);
  });
});
