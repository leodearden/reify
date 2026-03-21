import { onCleanup } from 'solid-js';
import styles from './Splitter.module.css';

export interface SplitterProps {
  orientation: 'horizontal' | 'vertical';
  onResize: (delta: number) => void;
  'data-testid'?: string;
}

export function Splitter(props: SplitterProps) {
  let dragging = false;
  let startPos = 0;

  function onMouseDown(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    startPos = props.orientation === 'vertical' ? e.clientX : e.clientY;

    const cursor = props.orientation === 'vertical' ? 'col-resize' : 'row-resize';
    document.body.style.cursor = cursor;
    document.body.style.userSelect = 'none';

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging) return;
    const currentPos = props.orientation === 'vertical' ? e.clientX : e.clientY;
    const delta = currentPos - startPos;
    startPos = currentPos;
    props.onResize(delta);
  }

  function onMouseUp() {
    dragging = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    document.removeEventListener('mousemove', onMouseMove);
    document.removeEventListener('mouseup', onMouseUp);
  }

  onCleanup(() => {
    document.removeEventListener('mousemove', onMouseMove);
    document.removeEventListener('mouseup', onMouseUp);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  });

  return (
    <div
      class={styles.splitter}
      data-testid={props['data-testid']}
      data-orientation={props.orientation}
      onMouseDown={onMouseDown}
    />
  );
}
