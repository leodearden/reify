/**
 * Command palette component.
 *
 * Two modes:
 *  - command  — lists SHORTCUTS entries with callback wiring; Enter runs the command.
 *  - symbol   — lists flattened documentSymbol tree; Enter jumps the cursor.
 *
 * All business logic (fuzzy filtering, symbol flattening, coordinate conversion)
 * lives in the adjacent commandPaletteFilter.ts pure module.
 *
 * The component is entirely self-contained for keyboard events on its own input,
 * so it does not collide with the global useKeyboardShortcuts handler.
 */
import { createSignal, createMemo, createEffect, onMount, For } from 'solid-js';
import type { DocumentSymbol } from '../editor/lspClient';
import type { SourceLocation } from '../types';
import type { ShortcutId } from '../shortcuts';
import type { PaletteCommand } from '../hooks/useKeyboardShortcuts';
import {
  filterCommands,
  flattenSymbols,
  filterSymbols,
  symbolToLocation,
} from './commandPaletteFilter';
import styles from './CommandPalette.module.css';

// ── Props ─────────────────────────────────────────────────────────────────────

export interface CommandPaletteProps {
  /** Source of commands — called each time the filtered list is recomputed. */
  getCommands: () => PaletteCommand[];
  /** Execute a command by id (same callback map as useKeyboardShortcuts). */
  runCommand: (id: ShortcutId) => void;
  /** Fetch the current file's document symbols from the LSP. */
  fetchSymbols: () => Promise<DocumentSymbol[]>;
  /** Active file path (forwarded to symbolToLocation). */
  filePath: string;
  /** Called when the user selects a symbol. */
  onJumpToLocation: (loc: SourceLocation) => void;
  /** Called when the palette should be dismissed (Escape / action taken). */
  onClose: () => void;
  /**
   * Opening mode.
   *  - 'command' (default) — Ctrl+Shift+P path; user can type '@' to switch.
   *  - 'symbol' — Ctrl+Shift+O path; palette opens directly in symbol mode.
   */
  initialMode?: 'command' | 'symbol';
}

// ── Component ─────────────────────────────────────────────────────────────────

export function CommandPalette(props: CommandPaletteProps) {
  const [query, setQuery] = createSignal('');
  const [selectedIndex, setSelectedIndex] = createSignal(0);
  const [symbols, setSymbols] = createSignal<DocumentSymbol[]>([]);
  const [symbolsLoaded, setSymbolsLoaded] = createSignal(false);

  let inputRef: HTMLInputElement | undefined;

  // ── Derived mode ────────────────────────────────────────────────────────────

  /** True when showing the symbol list rather than the command list. */
  const isSymbolMode = createMemo(
    () => props.initialMode === 'symbol' || query().startsWith('@'),
  );

  /** Query text to pass to the fuzzy symbol filter (strips leading '@'). */
  const symbolQuery = createMemo(() => {
    const q = query();
    return q.startsWith('@') ? q.slice(1) : q;
  });

  // ── Symbol loading ──────────────────────────────────────────────────────────

  /** Call fetchSymbols once; subsequent transitions reuse the cached result. */
  function loadSymbols() {
    if (symbolsLoaded()) return;
    props.fetchSymbols().then((syms) => {
      setSymbols(syms);
      setSymbolsLoaded(true);
    });
  }

  // Load symbols as soon as symbol mode becomes active.
  createEffect(() => {
    if (isSymbolMode()) loadSymbols();
  });

  // ── Filtered list ───────────────────────────────────────────────────────────

  const currentList = createMemo<Array<PaletteCommand | { name: string; selectionRange: { start: { line: number; character: number }; end: { line: number; character: number } }; depth: number; containerName: string; kind: number }>>(() => {
    if (isSymbolMode()) {
      return filterSymbols(flattenSymbols(symbols()), symbolQuery());
    }
    return filterCommands(props.getCommands(), query());
  });

  // Reset selection whenever the list contents change (query changed).
  createEffect(() => {
    currentList(); // track
    setSelectedIndex(0);
  });

  // ── Keyboard handling ───────────────────────────────────────────────────────

  function handleKeyDown(e: KeyboardEvent) {
    const list = currentList();

    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        props.onClose();
        return;

      case 'ArrowDown':
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, list.length - 1));
        return;

      case 'ArrowUp':
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
        return;

      case 'Enter': {
        e.preventDefault();
        const item = list[selectedIndex()];
        if (!item) return;
        if (isSymbolMode()) {
          // item is a FlatSymbol
          const sym = item as ReturnType<typeof flattenSymbols>[number];
          props.onJumpToLocation(symbolToLocation(sym, props.filePath));
        } else {
          // item is a PaletteCommand
          const cmd = item as PaletteCommand;
          props.runCommand(cmd.id);
        }
        props.onClose();
        return;
      }
    }
  }

  // ── Mount ───────────────────────────────────────────────────────────────────

  onMount(() => {
    inputRef?.focus();
  });

  // ── Render ──────────────────────────────────────────────────────────────────

  return (
    <div class={styles.backdrop} onClick={() => props.onClose()}>
      <div class={styles.card} onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          type="text"
          class={styles.input}
          placeholder={
            isSymbolMode()
              ? 'Search symbols…'
              : 'Run a command… (type @ to search symbols)'
          }
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
        />
        <ul class={styles.list} role="listbox">
          <For each={currentList()}>
            {(item, index) => {
              if (isSymbolMode()) {
                const sym = item as ReturnType<typeof flattenSymbols>[number];
                return (
                  <li
                    class={`${styles.item} ${index() === selectedIndex() ? styles.selected : ''}`}
                    style={{ 'padding-left': `${(sym.depth + 1) * 16}px` }}
                    role="option"
                    aria-selected={index() === selectedIndex()}
                    title={sym.containerName || undefined}
                    onClick={() => {
                      props.onJumpToLocation(symbolToLocation(sym, props.filePath));
                      props.onClose();
                    }}
                  >
                    <span class={styles.symbolName}>{sym.name}</span>
                  </li>
                );
              } else {
                const cmd = item as PaletteCommand;
                return (
                  <li
                    class={`${styles.item} ${index() === selectedIndex() ? styles.selected : ''}`}
                    role="option"
                    aria-selected={index() === selectedIndex()}
                    onClick={() => {
                      props.runCommand(cmd.id);
                      props.onClose();
                    }}
                  >
                    <span class={styles.cmdTitle}>{cmd.title}</span>
                    <kbd class={styles.cmdKey}>{cmd.key}</kbd>
                  </li>
                );
              }
            }}
          </For>
        </ul>
      </div>
    </div>
  );
}
