
// Local clock stamps for Chat turns. Split from chat.js, which reads
// `window.__TAURI__` at import, so node can load this.
import { locale } from "./i18n.js";

function pad2(n) {
  return String(n).padStart(2, "0");
}

function localHm(at) {
  return `${pad2(at.getHours())}:${pad2(at.getMinutes())}`;
}

// The month's short name in the interface language, day first in every one
// this page speaks.
function dayMonth(at) {
  return `${at.getDate()} ${new Intl.DateTimeFormat(locale(), { month: "short" }).format(at)}`;
}

function sameLocalDay(a, b) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

// Five-character HH:mm on a 420-point surface until the local day changes,
// then the date once. Year and seconds stay on the title.
export function stampWhen(at, previousAt) {
  const time = localHm(at);
  const dayChanged = previousAt !== null && !sameLocalDay(previousAt, at);
  return {
    label: dayChanged ? `${dayMonth(at)} ${time}` : time,
    datetime: at.toISOString(),
    title: `${dayMonth(at)} ${at.getFullYear()}, ${time}:${pad2(at.getSeconds())}`,
  };
}
