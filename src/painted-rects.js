// Windows' SetWindowRgn clips drawing as well as clicks, so the bubble body and
// thinking dots are reported to the Shell, which unions them into the overlay's
// region. Rects are [x, y, width, height] in overlay coordinates.

// The CSS tail (::before, 10px) hangs below the body, or above it when inverted.
const TAIL_PX = 10;

// Only a bubble that can be read: hidden or mid-fade keeps no region.
export function computeBubblePaintedRect(bubble, pos) {
  if (!bubble?.classList.contains("visible")) return null;
  const opacity = parseFloat(bubble.style.opacity);
  if (opacity < 1) return null;
  const y = Math.round(pos.y) - (pos.inverted ? TAIL_PX : 0);
  return [Math.round(pos.x), y, bubble.offsetWidth, bubble.offsetHeight + TAIL_PX];
}

// One call per change with every view's rect, so the Shell never holds half a set.
export function createPaintedReporter(invoke) {
  let reported = null;
  return (views) => {
    const rects = [...views.values()].map((view) => view.paintedRect).filter(Boolean);
    const told = JSON.stringify(rects);
    if (told === reported) return;
    reported = told;
    invoke("overlay_painted_rects", { rects }).catch((err) => {
      console.error("overlay_painted_rects", err);
    });
  };
}
