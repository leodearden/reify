/**
 * The side panel as one ordered entry list: each entry renders exactly one grid
 * child and owns exactly one `grid-template-rows` track. `track` is told whether
 * its entry is the last mounted one, so a panel can absorb the remaining height
 * only when nothing is mounted below it.
 */
import type { JSX } from 'solid-js';

export interface SidePanelEntry {
  readonly id: string;
  readonly mounted: () => boolean;
  readonly track: (isLast: boolean) => string;
  readonly render: () => JSX.Element;
}

export const alwaysMounted = (): boolean => true;

export function mountedEntries(entries: readonly SidePanelEntry[]): SidePanelEntry[] {
  return entries.filter((entry) => entry.mounted());
}

export function sidePanelTracks(entries: readonly SidePanelEntry[]): string {
  const mounted = mountedEntries(entries);
  return mounted.map((entry, i) => entry.track(i === mounted.length - 1)).join(' ');
}
