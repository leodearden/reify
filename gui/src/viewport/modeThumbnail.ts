/**
 * Pure helper: project buckling mode peak positions into a 2D SVG thumbnail
 * (task 4072, GR-016).
 *
 * computeModeThumbnail(base, peak) — both flat XYZ number[] of equal length —
 * picks the two axes with the largest extent in the base bounding box, projects
 * each peak node position onto those axes, and normalises into a shared [0,1]²
 * viewBox. Degenerate (zero-extent) axes map to 0.5. No DOM or WebGL
 * dependencies; deterministically unit-testable.
 */

export interface ModeThumbnail {
  /** One [x,y] pair per node, coordinates in [0,1]. */
  points: [number, number][];
  /** Always "0 0 1 1". */
  viewBox: string;
}

/**
 * Project peak node positions onto the two largest-extent base-bbox axes,
 * normalise into [0,1]², and return the points + viewBox string.
 *
 * @param base  Flat XYZ array for the undeformed reference, length 3·n_nodes.
 * @param peak  Flat XYZ array for the mode's peak displaced positions, same length.
 */
export function computeModeThumbnail(base: number[], peak: number[]): ModeThumbnail {
  const n = Math.floor(base.length / 3);

  if (n === 0) {
    return { points: [], viewBox: '0 0 1 1' };
  }

  // Compute per-axis (X=0, Y=1, Z=2) min/max over the BASE positions.
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < n; i++) {
    for (let a = 0; a < 3; a++) {
      const v = base[i * 3 + a]!;
      if (v < min[a]!) min[a] = v;
      if (v > max[a]!) max[a] = v;
    }
  }

  const extents = [max[0]! - min[0]!, max[1]! - min[1]!, max[2]! - min[2]!];

  // Pick the two axes with the largest extent (argsort descending).
  const axes = [0, 1, 2].sort((a, b) => extents[b]! - extents[a]!);
  const axisU = axes[0]!; // largest extent → horizontal
  const axisV = axes[1]!; // second-largest extent → vertical

  const extU = extents[axisU]!;
  const extV = extents[axisV]!;

  // Project each PEAK node onto the two chosen axes and normalise into [0,1].
  const points: [number, number][] = new Array(n);
  for (let i = 0; i < n; i++) {
    const rawU = peak[i * 3 + axisU]!;
    const rawV = peak[i * 3 + axisV]!;
    const u = extU > 0 ? (rawU - min[axisU]!) / extU : 0.5;
    const v = extV > 0 ? (rawV - min[axisV]!) / extV : 0.5;
    // Clamp to [0,1] to handle slight overshoot from scaled peak positions.
    points[i] = [Math.min(1, Math.max(0, u)), Math.min(1, Math.max(0, v))];
  }

  return { points, viewBox: '0 0 1 1' };
}
