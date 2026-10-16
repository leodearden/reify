/**
 * The side panel as one ordered entry list: each entry renders exactly one grid
 * child and owns exactly one `grid-template-rows` track. `track` is told whether
 * its entry is the last mounted one, so a panel can absorb the remaining height
 * only when nothing is mounted below it. Filter once with `mountedEntries` and
 * feed that one list to both the rendered children and `sidePanelTracks`.
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

export function sidePanelTracks(mounted: readonly SidePanelEntry[]): string {
  return mounted.map((entry, i) => entry.track(i === mounted.length - 1)).join(' ');
}
