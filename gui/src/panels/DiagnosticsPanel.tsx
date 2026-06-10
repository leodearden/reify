import { type Component, Show, For, createSignal, createMemo } from 'solid-js';
import type { DiagnosticInfo } from '../types';
import styles from './DiagnosticsPanel.module.css';
import {
  loadDiagnosticsLineWrap,
  saveDiagnosticsLineWrap,
} from '../hooks/diagnosticsPanelPersistence';
import { filterDiagnostics, groupDiagnostics } from './diagnosticsView';
import type { DiagnosticSource, GroupedDiagnostic } from './diagnosticsView';

/** Panel-facing wrapper that extends the wire-format DiagnosticInfo with a
 *  frontend-only source tag. The `source` field is never sent by the Rust
 *  backend; it is added at the App.tsx merge boundary so each row can
 *  display which pipeline produced the entry. */
export interface DiagnosticEntry extends DiagnosticInfo {
  source: 'compile' | 'tessellation';
}

export interface DiagnosticsPanelProps {
  /** Whether the panel body is collapsed (only header bar visible). */
  collapsed: boolean;
  /** Height in pixels of the panel when expanded. Applied as inline style. */
  height: number;
  diagnostics: DiagnosticEntry[];
  onToggleCollapsed: () => void;
  onNavigate: (d: DiagnosticEntry) => void;
}

const ALL_SOURCES: DiagnosticSource[] = ['compile', 'tessellation'];
const ALL_SEVERITIES = ['Error', 'Warning', 'Info'];

export const DiagnosticsPanel: Component<DiagnosticsPanelProps> = (props) => {
  const [lineWrap, setLineWrap] = createSignal(loadDiagnosticsLineWrap() ?? false);

  // Filter state — default all-selected so every diagnostic is visible on expand.
  const [selectedSources, setSelectedSources] = createSignal<Set<DiagnosticSource>>(
    new Set(ALL_SOURCES)
  );
  const [selectedSeverities, setSelectedSeverities] = createSignal<Set<string>>(
    // Union ALL_SEVERITIES with any severities actually present at mount time so that
    // unrecognized severity values (e.g. 'Hint', 'Note') are visible by default and
    // receive a toggle chip rather than being silently filtered out.
    new Set([...ALL_SEVERITIES, ...props.diagnostics.map(d => d.severity)])
  );

  // Available severity options derived from the full (unfiltered) props.diagnostics.
  // Renders known severities in priority order first, then any unrecognized ones, so
  // future backend severity values always get a chip.
  const availableSeverities = createMemo(() => {
    const seen = new Set<string>();
    for (const d of props.diagnostics) seen.add(d.severity);
    return [
      ...ALL_SEVERITIES.filter(s => seen.has(s)),
      ...[...seen].filter(s => !ALL_SEVERITIES.includes(s)),
    ];
  });

  // Available source options derived from the full (unfiltered) props.diagnostics.
  // Only sources actually present receive a chip, consistent with availableSeverities.
  const availableSources = createMemo(() => {
    const seen = new Set<DiagnosticSource>();
    for (const d of props.diagnostics) seen.add(d.source);
    return ALL_SOURCES.filter(s => seen.has(s));
  });

  // Filtered list used for rendering rows.
  const filteredDiagnostics = createMemo(() =>
    filterDiagnostics(props.diagnostics, {
      sources: selectedSources(),
      severities: selectedSeverities(),
    })
  );

  // Grouping toggle — ON by default to collapse repeated identical diagnostics.
  const [grouping, setGrouping] = createSignal(true);

  // Displayed groups: either deduped (grouping ON) or 1:1 with count=1 (grouping OFF).
  const displayedGroups = createMemo((): GroupedDiagnostic[] => {
    const filtered = filteredDiagnostics();
    if (grouping()) {
      return groupDiagnostics(filtered);
    }
    return filtered.map(d => ({ diagnostic: d, count: 1 }));
  });

  function toggleSource(source: DiagnosticSource) {
    setSelectedSources(prev => {
      const next = new Set(prev);
      if (next.has(source)) next.delete(source);
      else next.add(source);
      return next;
    });
  }

  function toggleSeverity(severity: string) {
    setSelectedSeverities(prev => {
      const next = new Set(prev);
      if (next.has(severity)) next.delete(severity);
      else next.add(severity);
      return next;
    });
  }

  function locationLabel(d: DiagnosticInfo): string {
    return `${d.file_path}:${d.line}:${d.column}`;
  }

  function severityClass(severity: string): string {
    switch (severity) {
      case 'Error': return styles.errorBadge;
      case 'Warning': return styles.warningBadge;
      default: return styles.infoBadge;
    }
  }

  function sourceChipClass(source: 'compile' | 'tessellation'): string {
    switch (source) {
      case 'compile': return styles.compileChip;
      case 'tessellation': return styles.tessellationChip;
    }
  }

  return (
    <div
      data-testid="diagnostics-panel"
      data-collapsed={props.collapsed ? 'true' : 'false'}
      class={`${styles.dockedRegion}${lineWrap() ? ` ${styles.lineWrapOn}` : ''}`}
      style={!props.collapsed ? { height: `${props.height}px` } : undefined}
    >
      {/* One-line header bar: always visible */}
      <div class={styles.dockedHeader}>
        <button
          type="button"
          data-testid="diagnostics-fold-toggle"
          class={styles.foldToggle}
          aria-expanded={!props.collapsed}
          aria-label="Toggle diagnostics panel"
          onClick={() => props.onToggleCollapsed()}
        >
          {props.collapsed ? '▶' : '▼'}
        </button>
        <span
          data-testid="panel-title-diagnostics"
          class={styles.dockedTitle}
        >
          Diagnostics ({props.diagnostics.length})
        </span>
      </div>

      {/* Body: only visible when expanded */}
      <Show when={!props.collapsed}>
        <Show when={props.diagnostics.length > 0}>
          <div class={styles.filterBar}>
            <For each={availableSources()}>
              {(source) => (
                <button
                  type="button"
                  class={`${styles.filterChip}${selectedSources().has(source) ? ` ${styles.filterChipActive}` : ''}`}
                  data-testid={`diagnostics-filter-source-${source}`}
                  aria-pressed={selectedSources().has(source)}
                  onClick={() => toggleSource(source)}
                >
                  {source}
                </button>
              )}
            </For>
            <For each={availableSeverities()}>
              {(severity) => (
                <button
                  type="button"
                  class={`${styles.filterChip}${selectedSeverities().has(severity) ? ` ${styles.filterChipActive}` : ''}`}
                  data-testid={`diagnostics-filter-severity-${severity}`}
                  aria-pressed={selectedSeverities().has(severity)}
                  onClick={() => toggleSeverity(severity)}
                >
                  {severity}
                </button>
              )}
            </For>
            <button
              type="button"
              class={`${styles.filterChip}${grouping() ? ` ${styles.filterChipActive}` : ''}`}
              data-testid="diagnostics-group-toggle"
              aria-pressed={grouping()}
              onClick={() => setGrouping(g => !g)}
            >
              Collapse repeated
            </button>
          </div>
        </Show>

        <Show
          when={displayedGroups().length > 0}
          fallback={
            <span class={styles.emptyState}>
              {props.diagnostics.length > 0
                ? 'No diagnostics match the current filters'
                : 'No diagnostics'}
            </span>
          }
        >
          <div class={styles.list}>
            <For each={displayedGroups()}>
              {(group) => (
                <div
                  class={styles.row}
                  data-testid="diagnostic-row"
                  onClick={() => props.onNavigate(group.diagnostic)}
                  role="button"
                  tabindex="0"
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      props.onNavigate(group.diagnostic);
                    }
                  }}
                >
                  <span class={severityClass(group.diagnostic.severity)}>
                    {group.diagnostic.severity}
                  </span>
                  <span
                    data-testid="diagnostic-source-chip"
                    class={sourceChipClass(group.diagnostic.source)}
                  >
                    {group.diagnostic.source}
                  </span>
                  <span class={styles.location}>{locationLabel(group.diagnostic)}</span>
                  <span class={styles.message}>{group.diagnostic.message}</span>
                  <Show when={group.count > 1}>
                    <span
                      class={styles.repeatCount}
                      data-testid="diagnostic-repeat-count"
                    >
                      x{group.count}
                    </span>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>

        <div class={styles.actions}>
          <label class={styles.wrapLabel}>
            <input
              type="checkbox"
              data-testid="diagnostics-line-wrap-toggle"
              checked={lineWrap()}
              onChange={(e) => {
                const checked = e.currentTarget.checked;
                setLineWrap(checked);
                saveDiagnosticsLineWrap(checked);
              }}
            />
            {' '}Wrap lines
          </label>
        </div>
      </Show>
    </div>
  );
};
