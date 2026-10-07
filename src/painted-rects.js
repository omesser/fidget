// Painted rect calculation for bubble and thinking indicator regions.
// Windows SetWindowRgn clips drawing as well as hit-testing, so the bubble
// body and thinking dots must be reported as painted rects to include them
// in the region. Format is [x, y, width, height] in overlay coordinates.

let reportedPainted = "";

/**
 * Compute painted rect for a bubble element when visible.
 * @param {HTMLElement} bubble - The bubble element
 * @param {{x: number, y: number}} pos - Position in overlay coords
 * @returns {[number, number, number, number] | null} [x, y, w, h] or null if hidden
 */
export function computeBubblePaintedRect(bubble, pos) {
  if (!bubble || !bubble.classList.contains("visible")) {
    return null;
  }

  const opacity = parseFloat(bubble.style.opacity);
  if (opacity < 1 && !isNaN(opacity)) {
    return null;
  }

  return [
    Math.round(pos.x),
    Math.round(pos.y),
    bubble.offsetWidth,
    bubble.offsetHeight,
  ];
}

/**
 * Batch painted rects from all views and invoke backend if changed.
 * @param {Map} views - Map of view objects with paintedRect property
 * @param {Function} invoke - Tauri invoke function
 */
export function reportPaintedRects(views, invoke) {
  const rects = [];
  for (const view of views.values()) {
    if (view.paintedRect) rects.push(view.paintedRect);
  }
  const serialized = JSON.stringify(rects);
  if (serialized === reportedPainted) return;
  reportedPainted = serialized;
  invoke("overlay_painted_rects", { rects }).catch((err) => {
    console.error("overlay_painted_rects", err);
  });
}

/**
 * Clear painted rects cache for testing.
 */
export function clearCache() {
  reportedPainted = "";
}
