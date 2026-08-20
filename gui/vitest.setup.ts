// Global vitest setup: jsdom Range rectangle-measurement polyfills (task #5361).
//
// jsdom's Range implementation does not provide getClientRects() or
// getBoundingClientRect() — accessing them throws "TypeError:
// range.getClientRects is not a function". CodeMirror's drawSelection() layer
// measures selection/cursor rectangles via a Range (RectangleMarker.forRange →
// rectanglesForRange at @codemirror/view), and CM routes any throw from that
// path through logException → console.error. task #5361 newly activates
// drawSelection() in the editor's PRIMARY-selection guard, so every real-CM
// mount under jsdom now emits those measure errors. Most tests ignore console
// noise, but ide-affordances.e2e.test.ts gates on a zero-console-error count
// and so trips (esc-5361-2).
//
// In real WebKitGTK both methods exist and drawSelection measures normally —
// this is purely a jsdom test-environment gap, not a production defect. We
// install minimal, standards-shaped stubs ONLY when absent (never clobbering a
// real implementation, e.g. if jsdom later grows one): getClientRects returns
// an empty DOMRectList-like, getBoundingClientRect returns a zero-sized DOMRect.
// This is the standard CodeMirror-in-jsdom workaround and benefits every future
// CM-measuring test, not just this task's.

function zeroRect(): DOMRect {
  const rect = {
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    width: 0,
    height: 0,
    toJSON() {
      return this;
    },
  };
  return rect as DOMRect;
}

function emptyRectList(): DOMRectList {
  const list = {
    length: 0,
    item(_index: number): DOMRect | null {
      return null;
    },
    [Symbol.iterator]: function* (): IterableIterator<DOMRect> {
      // no rectangles
    },
  };
  return list as unknown as DOMRectList;
}

const rangeProto = typeof Range !== 'undefined' ? Range.prototype : undefined;

if (rangeProto) {
  if (typeof rangeProto.getClientRects !== 'function') {
    rangeProto.getClientRects = function getClientRects(): DOMRectList {
      return emptyRectList();
    };
  }
  if (typeof rangeProto.getBoundingClientRect !== 'function') {
    rangeProto.getBoundingClientRect = function getBoundingClientRect(): DOMRect {
      return zeroRect();
    };
  }
}
