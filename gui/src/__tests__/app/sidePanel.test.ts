import { describe, it, expect } from 'vitest';
import { sidePanelTracks, mountedEntries, alwaysMounted, type SidePanelEntry } from '../../app/sidePanel';
import { splitGridTracks } from '../test_utils/gridTracks';

const OPTIONAL_AXES = ['autoResolve', 'buckling', 'feaDiagnostics', 'mechanism', 'chat'] as const;
type OptionalAxis = (typeof OPTIONAL_AXES)[number];
type Mounts = Record<OptionalAxis, boolean>;

const ALL_COMBINATIONS: Mounts[] = Array.from({ length: 1 << OPTIONAL_AXES.length }, (_, mask) =>
  Object.fromEntries(OPTIONAL_AXES.map((axis, bit) => [axis, (mask & (1 << bit)) !== 0])) as Mounts,
);

function describeMounts(mounts: Mounts): string {
  const on = OPTIONAL_AXES.filter((axis) => mounts[axis]);
  return on.length > 0 ? on.join('+') : 'none';
}

type TrackCall = { id: string; isLast: boolean };

/** App's side-panel order with fixed heights designTree=160, property=200, constraint=140. */
function fixtureEntries(mounts: Mounts, calls: TrackCall[] = []): SidePanelEntry[] {
  const entry = (
    id: string,
    mounted: () => boolean,
    track: (isLast: boolean) => string,
  ): SidePanelEntry => ({
    id,
    mounted,
    track: (isLast) => {
      calls.push({ id, isLast });
      return track(isLast);
    },
    render: () => null,
  });
  const when = (axis: OptionalAxis) => () => mounts[axis];
  return [
    entry('design-tree', alwaysMounted, () => '160px'),
    entry('splitter-design-tree', alwaysMounted, () => '4px'),
    entry('property-editor', alwaysMounted, () => '200px'),
    entry('splitter-side', alwaysMounted, () => '4px'),
    entry('constraint-panel', alwaysMounted, (isLast) => (isLast ? '1fr' : '140px')),
    entry('auto-resolve', when('autoResolve'), () => 'auto'),
    entry('buckling', when('buckling'), () => 'auto'),
    entry('fea-diagnostics', when('feaDiagnostics'), () => 'auto'),
    entry('mechanism', when('mechanism'), () => 'auto'),
    entry('splitter-constraint', when('chat'), () => '4px'),
    entry('chat', when('chat'), () => 'minmax(160px, 1fr)'),
  ];
}

describe('sidePanelTracks (B6: one track per mounted entry)', () => {
  it.each(ALL_COMBINATIONS.map((mounts) => [describeMounts(mounts), mounts] as const))(
    '%s: one track per mounted entry, at that entry\'s position',
    (_label, mounts) => {
      const mounted = mountedEntries(fixtureEntries(mounts));
      const tracks = splitGridTracks(sidePanelTracks(mounted));

      expect(tracks).toHaveLength(mounted.length);

      const trackOf = (id: string) => tracks[mounted.findIndex((e) => e.id === id)];
      const anyOptionalMounted = OPTIONAL_AXES.some((axis) => mounts[axis]);
      expect(trackOf('constraint-panel')).toBe(anyOptionalMounted ? '140px' : '1fr');
      if (mounts.chat) expect(trackOf('chat')).toBe('minmax(160px, 1fr)');
      if (mounts.buckling) expect(trackOf('buckling')).toBe('auto');
      if (mounts.feaDiagnostics) expect(trackOf('fea-diagnostics')).toBe('auto');
    },
  );

  it.each(ALL_COMBINATIONS.map((mounts) => [describeMounts(mounts), mounts] as const))(
    '%s: only the last mounted entry is told it is last',
    (_label, mounts) => {
      const calls: TrackCall[] = [];
      const mounted = mountedEntries(fixtureEntries(mounts, calls));
      const mountedIds = mounted.map((e) => e.id);

      sidePanelTracks(mounted);

      expect(calls.map((c) => c.id)).toEqual(mountedIds);
      const lastCalls = calls.filter((c) => c.isLast);
      expect(lastCalls).toEqual([{ id: mountedIds[mountedIds.length - 1], isLast: true }]);
    },
  );

  // The track strings the side panel produced before buckling and FEA
  // diagnostics owned tracks; every one of them must be unchanged.
  const PRESERVED: ReadonlyArray<readonly [Partial<Mounts>, string]> = [
    [{ chat: true }, '160px 4px 200px 4px 140px 4px minmax(160px, 1fr)'],
    [{ chat: true, autoResolve: true }, '160px 4px 200px 4px 140px auto 4px minmax(160px, 1fr)'],
    [{ chat: true, mechanism: true }, '160px 4px 200px 4px 140px auto 4px minmax(160px, 1fr)'],
    [{ chat: true, autoResolve: true, mechanism: true }, '160px 4px 200px 4px 140px auto auto 4px minmax(160px, 1fr)'],
    [{}, '160px 4px 200px 4px 1fr'],
    [{ autoResolve: true }, '160px 4px 200px 4px 140px auto'],
    [{ mechanism: true }, '160px 4px 200px 4px 140px auto'],
    [{ autoResolve: true, mechanism: true }, '160px 4px 200px 4px 140px auto auto'],
  ];

  it.each(PRESERVED.map(([partial, expected]) => {
    const mounts = { ...Object.fromEntries(OPTIONAL_AXES.map((a) => [a, false])), ...partial } as Mounts;
    return [describeMounts(mounts), mounts, expected] as const;
  }))('%s: reproduces the established track string', (_label, mounts, expected) => {
    expect(sidePanelTracks(mountedEntries(fixtureEntries(mounts)))).toBe(expected);
  });
});

describe('mountedEntries', () => {
  it('keeps the mounted entries, in input order, as the same objects', () => {
    const mounts: Mounts = { autoResolve: false, buckling: true, feaDiagnostics: false, mechanism: true, chat: true };
    const entries = fixtureEntries(mounts);

    const mounted = mountedEntries(entries);

    expect(mounted.map((e) => e.id)).toEqual([
      'design-tree', 'splitter-design-tree', 'property-editor', 'splitter-side', 'constraint-panel',
      'buckling', 'mechanism', 'splitter-constraint', 'chat',
    ]);
    mounted.forEach((e) => expect(entries).toContain(e));
  });

  it('an empty entry list yields no tracks', () => {
    expect(mountedEntries([])).toEqual([]);
    expect(sidePanelTracks([])).toBe('');
  });
});
