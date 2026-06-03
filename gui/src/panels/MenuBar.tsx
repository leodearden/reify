/**
 * MenuBar — top-level application menu bar with dropdown menus and
 * keyboard shortcut annotations.
 */
import { createSignal, onMount, onCleanup, Show, For } from 'solid-js';
import type { Component } from 'solid-js';
import { getShortcut, shortcutKey, type ShortcutId } from '../shortcuts';
import styles from './MenuBar.module.css';

export interface MenuBarProps {
  onNew?: () => void;
  onOpen?: () => void;
  onSave?: () => void;
  onExport?: () => void;
  onReEvaluate?: () => void;
  onFitToView?: () => void;
  onToggleChat?: () => void;
  onHelp?: () => void;
}

type MenuId = 'file' | 'edit' | 'view' | 'help';

type MenuItemDef = {
  label: string;
  shortcutId: ShortcutId;
  action?: keyof MenuBarProps;
};

type MenuDef = {
  id: MenuId;
  label: string;
  items: MenuItemDef[];
};

const MENU_DEFS: MenuDef[] = [
  {
    id: 'file',
    label: 'File',
    items: [
      { label: 'New', shortcutId: 'new', action: 'onNew' },
      { label: 'Open', shortcutId: 'open', action: 'onOpen' },
      { label: 'Save', shortcutId: 'save', action: 'onSave' },
      { label: 'Export', shortcutId: 'export', action: 'onExport' },
    ],
  },
  {
    id: 'edit',
    label: 'Edit',
    items: [
      { label: 'Undo', shortcutId: 'undo' },
      { label: 'Redo', shortcutId: 'redo' },
    ],
  },
  {
    id: 'view',
    label: 'View',
    items: [
      { label: 'Re-evaluate', shortcutId: 'reEvaluate', action: 'onReEvaluate' },
      { label: 'Fit to View', shortcutId: 'fitToView', action: 'onFitToView' },
      { label: 'Toggle Chat', shortcutId: 'toggleChat', action: 'onToggleChat' },
    ],
  },
  {
    id: 'help',
    label: 'Help',
    items: [
      { label: 'Keyboard Shortcuts', shortcutId: 'help', action: 'onHelp' },
    ],
  },
];

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
      if (containerRef && e.target instanceof Node && !containerRef.contains(e.target)) {
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
      <For each={MENU_DEFS}>
        {(menu) => (
          <div style={{ position: 'relative' }}>
            <button
              class={openMenu() === menu.id ? `${styles.trigger} ${styles.triggerOpen}` : styles.trigger}
              data-testid={`menu-trigger-${menu.id}`}
              onClick={() => toggleMenu(menu.id)}
              onMouseEnter={() => switchMenu(menu.id)}
            >
              {menu.label}
            </button>
            <Show when={openMenu() === menu.id}>
              <div class={styles.dropdown} role="menu">
                <For each={menu.items}>
                  {(item) => {
                    const isDisabled = getShortcut(item.shortcutId)?.disabled ?? false;
                    return (
                      <button
                        class={isDisabled ? `${styles.item} ${styles.itemDisabled}` : styles.item}
                        role="menuitem"
                        data-testid={`menu-item-${item.shortcutId}`}
                        disabled={isDisabled}
                        onClick={() => handleItemClick(item.action ? props[item.action] : undefined)}
                      >
                        <span>{item.label}</span>
                        <span class={styles.shortcut}>{shortcutKey(item.shortcutId)}</span>
                      </button>
                    );
                  }}
                </For>
              </div>
            </Show>
          </div>
        )}
      </For>
    </div>
  );
};
