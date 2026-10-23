// The caffeinate button in the Chat header. Only macOS sends it a view.

// `xmlns` is what puts the root in the SVG namespace. Without it,
// `DOMParser` returns an element no engine draws.
export const CUP_SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
  <path class="cup-body" d="M4.5 11h12c0 4-2.7 7-6 7s-6-3-6-7z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/>
  <path d="M16.3 12.3h1.2a2.1 2.1 0 0 1 0 4.2h-2.1" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
  <path d="M2.5 19.6c1.6 1 4.6 1.4 8 1.4s6.4-.4 8-1.4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
  <path d="M9 9c-1-1.2 1-2.2 0-3.6M12 9c-1.2-1.5 1.2-2.6 0-4.6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
</svg>`;

// `null` hides the button. A `reason` disables it, which happens when every
// flag is off, and becomes the tooltip. `label` shows only while on.
export function cupModel(field) {
  if (field == null) return null;
  const disabled = Boolean(field.reason);
  const on = Boolean(field.on) && !disabled;
  return {
    disabled,
    on,
    title: field.reason || "keep my computer awake",
    label: on ? "caffeinated" : null,
  };
}
