/**
 * Splits a CSS `grid-template-rows` value into its tracks. Whitespace separates
 * tracks only outside parentheses, so `minmax(160px, 1fr)` stays one track.
 */
export function splitGridTracks(template: string): string[] {
  const tracks: string[] = [];
  let depth = 0;
  let current = '';
  for (const ch of template) {
    if (ch === '(') depth++;
    else if (ch === ')') depth--;
    if (depth === 0 && /\s/.test(ch)) {
      if (current !== '') tracks.push(current);
      current = '';
    } else {
      current += ch;
    }
  }
  if (current !== '') tracks.push(current);
  return tracks;
}
