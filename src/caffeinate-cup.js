// The stay-awake cup. Drawn only when a frame carries the field, which macOS
// does and the other builds omit. The SVG is ours: the project this copies
// ships icons with no license we can vendor.

export const CUP_SVG = `<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
  <path d="M7 9h8v5a3 3 0 0 1-3 3h-2a3 3 0 0 1-3-3V9z" fill="currentColor" stroke="currentColor" stroke-width="1.4"/>
  <path d="M15 10h2.2a2.2 2.2 0 0 1 0 4.4H15" fill="none" stroke="currentColor" stroke-width="1.4"/>
  <path d="M9 5c.2 1.1.9 1.2 1 2.2M12 5c.2 1.1.9 1.2 1 2.2" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
</svg>`;

// `null` means draw nothing. A reason disables the button: an empty flag set
// has no argv to run.
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
