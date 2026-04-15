/**
 * MenuBar — top-level application menu bar with dropdown menus and
 * keyboard shortcut annotations.
 */
import { createSignal, onMount, onCleanup, Show } from 'solid-js';
import type { Component } from 'solid-js';
import { shortcutKey } from '../shortcuts';
import styles from './MenuBar.module.css';

export interface MenuBarProps {
  onOpen?: () => void;
  onSave?: () => void;
  onExport?: () => void;
  onReEvaluate?: () => void;
  onFitToView?: () => void;
  onToggleChat?: () => void;
  onHelp?: () => void;
}

type MenuId = 'file' | 'edit' | 'view' | 'help';

export const MenuBar: Component<MenuBarProps> = (props) => {
  const [openMenu, setOpenMenu] = createSignal<MenuId | null>(null);
  let containerRef: HTMLDivElement | undefined;

  function toggleMenu(id: MenuId) {
    setOpenMenu((prev) => (prev === id ? null : id));
  }

  function closeMenu() {
    setOpenMenu(null);
  }

  function switchMenu(id: MenuId) {
    // Only switch if a menu is already open (hover navigation)
    if (openMenu() !== null) {
      setOpenMenu(id);
    }
  }

  function handleItemClick(callback?: () => void) {
    callback?.();
    closeMenu();
  }

  onMount(() => {
    function handleMouseDown(e: MouseEvent) {
      if (containerRef && !containerRef.contains(e.target as Node)) {
        closeMenu();
      }
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        closeMenu();
      }
    }

    document.addEventListener('mousedown', handleMouseDown);
    document.addEventListener('keydown', handleKeyDown);

    onCleanup(() => {
      document.removeEventListener('mousedown', handleMouseDown);
      document.removeEventListener('keydown', handleKeyDown);
    });
  });

  return (
    <div
      ref={containerRef}
      class={styles.container}
      data-testid="menu-bar"
      role="menubar"
    >
      {/* File menu */}
      <div style={{ position: 'relative' }}>
        <button
          class={openMenu() === 'file' ? `${styles.trigger} ${styles.triggerOpen}` : styles.trigger}
          onClick={() => toggleMenu('file')}
          onMouseEnter={() => switchMenu('file')}
        >
          File
        </button>
        <Show when={openMenu() === 'file'}>
          <div class={styles.dropdown} role="menu">
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onOpen)}
            >
              <span>Open</span>
              <span class={styles.shortcut}>{shortcutKey('open')}</span>
            </button>
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onSave)}
            >
              <span>Save</span>
              <span class={styles.shortcut}>{shortcutKey('save')}</span>
            </button>
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onExport)}
            >
              <span>Export</span>
              <span class={styles.shortcut}>{shortcutKey('export')}</span>
            </button>
          </div>
        </Show>
      </div>

      {/* Edit menu */}
      <div style={{ position: 'relative' }}>
        <button
          class={openMenu() === 'edit' ? `${styles.trigger} ${styles.triggerOpen}` : styles.trigger}
          onClick={() => toggleMenu('edit')}
          onMouseEnter={() => switchMenu('edit')}
        >
          Edit
        </button>
        <Show when={openMenu() === 'edit'}>
          <div class={styles.dropdown} role="menu">
            <button class={`${styles.item} ${styles.itemDisabled}`} role="menuitem" disabled>
              <span>Undo</span>
              <span class={styles.shortcut}>{shortcutKey('undo')}</span>
            </button>
            <button class={`${styles.item} ${styles.itemDisabled}`} role="menuitem" disabled>
              <span>Redo</span>
              <span class={styles.shortcut}>{shortcutKey('redo')}</span>
            </button>
          </div>
        </Show>
      </div>

      {/* View menu */}
      <div style={{ position: 'relative' }}>
        <button
          class={openMenu() === 'view' ? `${styles.trigger} ${styles.triggerOpen}` : styles.trigger}
          onClick={() => toggleMenu('view')}
          onMouseEnter={() => switchMenu('view')}
        >
          View
        </button>
        <Show when={openMenu() === 'view'}>
          <div class={styles.dropdown} role="menu">
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onReEvaluate)}
            >
              <span>Re-evaluate</span>
              <span class={styles.shortcut}>{shortcutKey('reEvaluate')}</span>
            </button>
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onFitToView)}
            >
              <span>Fit to View</span>
              <span class={styles.shortcut}>{shortcutKey('fitToView')}</span>
            </button>
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onToggleChat)}
            >
              <span>Toggle Chat</span>
              <span class={styles.shortcut}>{shortcutKey('toggleChat')}</span>
            </button>
          </div>
        </Show>
      </div>

      {/* Help menu */}
      <div style={{ position: 'relative' }}>
        <button
          class={openMenu() === 'help' ? `${styles.trigger} ${styles.triggerOpen}` : styles.trigger}
          onClick={() => toggleMenu('help')}
          onMouseEnter={() => switchMenu('help')}
        >
          Help
        </button>
        <Show when={openMenu() === 'help'}>
          <div class={styles.dropdown} role="menu">
            <button
              class={styles.item}
              role="menuitem"
              onClick={() => handleItemClick(props.onHelp)}
            >
              <span>Keyboard Shortcuts</span>
              <span class={styles.shortcut}>{shortcutKey('help')}</span>
            </button>
          </div>
        </Show>
      </div>
    </div>
  );
};
