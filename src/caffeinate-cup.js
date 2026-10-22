// The caffeinate button in the Chat header. Only macOS sends it a view. The
// SVG is drawn here because the Caffeinate project's icons carry no license
// we can vendor.

// `xmlns` is what puts the root in the SVG namespace. Without it,
// `DOMParser` returns an element no engine draws.
export const CUP_SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
  <path class="cup-body" d="M7 9h8v5a3 3 0 0 1-3 3h-2a3 3 0 0 1-3-3V9z" stroke="currentColor" stroke-width="1.4"/>
  <path d="M15 10h2.2a2.2 2.2 0 0 1 0 4.4H15" fill="none" stroke="currentColor" stroke-width="1.4"/>
  <path d="M9 5c.2 1.1.9 1.2 1 2.2M12 5c.2 1.1.9 1.2 1 2.2" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
</svg>`;

// `null` hides the button. A `reason` disables it, which happens when every
// flag is off.
export function cupModel(field) {
  if (field == null) return null;
  const disabled = Boolean(field.reason);
  const on = Boolean(field.on) && !disabled;
  return {
    disabled,
    on,
    title: field.reason || (on ? "Caffeinate on" : "Caffeinate off"),
    svg: CUP_SVG,
  };
}
