export interface DiffLine {
  type: 'add' | 'remove' | 'context';
  content: string;
}

/**
 * Compute a unified diff between two strings using line-based LCS.
 * Returns DiffLine[] with context, add, and remove entries.
 */
export function computeUnifiedDiff(before: string, after: string): DiffLine[] {
  if (before === '' && after === '') return [];

  const oldLines = before === '' ? [] : before.split('\n');
  const newLines = after === '' ? [] : after.split('\n');

  // Compute LCS via dynamic programming
  const m = oldLines.length;
  const n = newLines.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (oldLines[i - 1] === newLines[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
  }

  // Backtrack to build diff
  const result: DiffLine[] = [];
  let i = m;
  let j = n;

  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
      result.push({ type: 'context', content: oldLines[i - 1] });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      result.push({ type: 'add', content: newLines[j - 1] });
      j--;
    } else {
      result.push({ type: 'remove', content: oldLines[i - 1] });
      i--;
    }
  }

  return result.reverse();
}
