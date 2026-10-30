import { t } from "./i18n.js";

// The caffeinate button in the Chat header.

// `null` hides the button. A `reason` disables it, which is every flag off,
// and becomes the tooltip. `label` shows only while on.
export function cupModel(field) {
  if (field == null) return null;
  const disabled = Boolean(field.reason);
  const on = Boolean(field.on) && !disabled;
  return {
    disabled,
    on,
    title: field.reason ? t(field.reason) : t("keep my computer awake"),
    label: on ? t("caffeinated") : null,
  };
}
