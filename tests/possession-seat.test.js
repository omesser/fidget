// The bindings card is seated in the display's usable area, and it stays there
// while the sprite walks. Drives the real src/index.html and main.js.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

// A menu bar (or a top taskbar) occupies the top of the frame. The overlay
// covers that strip; the card has to start below it.
const MENU_BAR = 30;
const INSET = 16;

const workArea = { x: 0, y: MENU_BAR, width: 1400, height: 900 - MENU_BAR };

function possessed(x, extra) {
  return {
    id: "driver",
    character: "Buddy Bot",
    animation: "idle",
    frame_index: 0,
    x,
    y: 500,
    width: 64,
    height: 64,
    mirror: 1,
    bubble: true,
    possessed: true,
    paused: false,
    charge: 0.4,
    ...extra,
  };
}

const charging = {
  sprites: [possessed(120)],
  visible: true,
  fade_ms: 0,
  sound: false,
  hover_delay_ms: 1,
  work_area: workArea,
};

// The card is seated before Space goes down. A bar that only arrives with the
// first charging frame is what a Windows region misses.
const seated = {
  ...charging,
  sprites: [possessed(120, { charge: null })],
};

const released = seated;

const unpossessed = {
  ...charging,
  sprites: [possessed(640, { possessed: false, paused: false, charge: null })],
};

const walked = {
  ...charging,
  sprites: [possessed(640)],
};

const paused = {
  ...charging,
  sprites: [possessed(640, { paused: true, charge: null })],
};

let probe = null;

function drive() {
  probe ??= launch();
  return probe;
}

async function launch() {
  const stub = `
<script type="module">
  const handlers = {};
  const waiters = [];
  const until = (ready) =>
    new Promise((resolve) => {
      const check = () => (ready() ? (resolve(), true) : false);
      if (!check()) waiters.push(check);
    });
  const wake = () => {
    for (let i = waiters.length - 1; i >= 0; i -= 1) if (waiters[i]()) waiters.splice(i, 1);
  };
  // A walk that does not move the card sends no new rect report, so the
  // waiter has to notice the sprite's transform on a frame of its own.
  const pump = () => {
    wake();
    requestAnimationFrame(pump);
  };
  requestAnimationFrame(pump);
  window.__invoked = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        window.__invoked.push({ name, args: structuredClone(args) });
        queueMicrotask(wake);
        if (name === "character") return Promise.resolve({ characters: {} });
        if (name === "overlay_hit_tests_hotspots") return Promise.resolve(true);
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        queueMicrotask(wake);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
  const box = (el) => {
    const r = el.getBoundingClientRect();
    return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)];
  };
  const reports = () => window.__invoked.filter((c) => c.name === "overlay_rects").map((c) => c.args.rects);
  const last = () => reports().at(-1) ?? null;
  const cardEl = () => document.querySelector(".possession.visible");
  async function drive() {
    try {
      console.log("__PROBE_READY__");
      const barEl = () => document.querySelector(".possession-charge");
      const fillEl = () => barEl()?.querySelector(".possession-charge-fill");
      const fontOf = (selector) => {
        const el = document.querySelector(selector);
        return el ? getComputedStyle(el).fontFamily : null;
      };
      await until(() => handlers.frame);
      handlers.frame({ payload: ${JSON.stringify(seated)} });
      await until(() => cardEl() && last());
      const seatedReported = last();
      const releaseEl = cardEl().querySelector(".possession-release");
      const releaseBox = releaseEl && box(releaseEl);
      const releaseText = releaseEl?.textContent ?? null;
      const releaseHits = releaseEl
        ? document.elementFromPoint(releaseBox[0] + 4, releaseBox[1] + 4) === releaseEl
        : false;
      const before = window.__invoked.length;
      releaseEl?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      releaseEl?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
      releaseEl?.click();
      const releaseCalls = window.__invoked
        .slice(before)
        .filter((c) => c.name !== "overlay_rects");
      const fonts = {
        card: fontOf(".possession"),
        heading: fontOf(".possession-heading"),
        keys: fontOf(".possession-keys"),
        label: fontOf(".possession li span:last-child"),
      };
      handlers.frame({ payload: ${JSON.stringify(charging)} });
      await until(() => fillEl()?.style.width === "40%");
      const chargingCard = box(cardEl());
      const chargingBar = box(barEl());
      const chargingReported = last();
      handlers.frame({ payload: ${JSON.stringify(released)} });
      await until(() => fillEl() && !barEl().classList.contains("visible"));
      const releasedReported = last();
      handlers.frame({ payload: ${JSON.stringify(walked)} });
      await until(() => (document.querySelector(".sprite")?.style.transform || "").includes("640"));
      const walkedCard = box(cardEl());
      const walkedReported = last();
      handlers.frame({ payload: ${JSON.stringify(paused)} });
      await until(() => cardEl()?.classList.contains("paused") && last());
      const pausedCard = box(cardEl());
      const pausedReported = last();
      const pausedRelease = Boolean(cardEl().querySelector(".possession-release"));
      const { color, backgroundColor, opacity } = getComputedStyle(cardEl());
      const pausedStyle = { color, backgroundColor, opacity };

      const press = (type, x, y) =>
        (type === "pointerdown" ? document.elementFromPoint(x, y) : cardEl()).dispatchEvent(
          new PointerEvent(type, { bubbles: true, pointerId: 1, button: 0, buttons: type === "pointerup" ? 0 : 1, clientX: x, clientY: y }),
        );
      const focusAsks = (from) => window.__invoked.slice(from).filter((c) => c.name === "overlay_request_focus").length;
      const settle = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const dragBy = async (dx, dy) => {
        const [x, y] = box(cardEl());
        press("pointerdown", x + 8, y + 8);
        press("pointermove", x + 8 + dx / 2, y + 8 + dy / 2);
        press("pointermove", x + 8 + dx, y + 8 + dy);
        await settle();
        const during = last();
        press("pointerup", x + 8 + dx, y + 8 + dy);
        await settle();
        return during;
      };

      let asked = window.__invoked.length;
      const pausedDragReported = await dragBy(300, 200);
      const pausedDragFocus = focusAsks(asked);
      const pausedDropped = box(cardEl());
      const pausedDroppedReported = last();
      handlers.frame({ payload: ${JSON.stringify(walked)} });
      await until(() => !cardEl()?.classList.contains("paused"));
      await settle();
      const resumedCard = box(cardEl());
      const resumedReported = last();
      await dragBy(-100, 50);
      const liveDropped = box(cardEl());
      handlers.frame({ payload: ${JSON.stringify(paused)} });
      await until(() => cardEl()?.classList.contains("paused"));
      await settle();
      const repausedCard = box(cardEl());
      asked = window.__invoked.length;
      const [px, py] = box(cardEl());
      press("pointerdown", px + 8, py + 8);
      press("pointerup", px + 9, py + 8);
      const clickFocus = focusAsks(asked);
      await dragBy(5000, 5000);
      const clampedCard = box(cardEl());
      handlers.frame({ payload: ${JSON.stringify(unpossessed)} });
      await until(() => !cardEl());
      handlers.frame({ payload: ${JSON.stringify(seated)} });
      await until(() => cardEl());
      await settle();
      const nextSessionCard = box(cardEl());
      console.log("__PROBE_RESULT__" + JSON.stringify({
        pausedDragReported,
        pausedDragFocus,
        pausedDropped,
        pausedDroppedReported,
        resumedCard,
        resumedReported,
        liveDropped,
        repausedCard,
        clickFocus,
        clampedCard,
        nextSessionCard,
        seatedReported,
        releaseBox,
        releaseText,
        releaseHits,
        releaseCalls,
        pausedRelease,
        fonts,
        chargingCard,
        chargingBar,
        chargingReported,
        releasedReported,
        walkedCard,
        walkedReported,
        pausedCard,
        pausedReported,
        pausedStyle,
      }));
    } catch (err) {
      console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(err && err.stack || err) }));
    }
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace("<head>", `<head>${stub}`);
  return probePage(html, { args: ["--window-size=1400,900"], timeout: 8000 });
}

function channel(value) {
  const s = value / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function luminance(rgb) {
  return 0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b);
}

function contrast(a, b) {
  const left = luminance(a);
  const right = luminance(b);
  const [hi, lo] = left > right ? [left, right] : [right, left];
  return (hi + 0.05) / (lo + 0.05);
}

function parseCssColor(value) {
  const modern = String(value).match(
    /color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\)/,
  );
  if (modern) {
    return {
      r: Number(modern[1]) * 255,
      g: Number(modern[2]) * 255,
      b: Number(modern[3]) * 255,
      a: modern[4] === undefined ? 1 : Number(modern[4]),
    };
  }
  const match = String(value).match(
    /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:[,\s/]+([\d.]+))?\s*\)/,
  );
  assert.ok(match, `unparsed color ${value}`);
  return {
    r: Number(match[1]),
    g: Number(match[2]),
    b: Number(match[3]),
    a: match[4] === undefined ? 1 : Number(match[4]),
  };
}

function over(src, dst) {
  const a = src.a + dst.a * (1 - src.a);
  return {
    r: (src.r * src.a + dst.r * dst.a * (1 - src.a)) / a,
    g: (src.g * src.a + dst.g * dst.a * (1 - src.a)) / a,
    b: (src.b * src.a + dst.b * dst.a * (1 - src.a)) / a,
    a,
  };
}

function overBackdrop(layer, opacity, backdrop) {
  return over({ ...layer, a: layer.a * opacity }, { ...backdrop, a: 1 });
}

function pausedContrast(style, backdrop) {
  const text = parseCssColor(style.color);
  const panel = parseCssColor(style.backgroundColor);
  const opacity = Number(style.opacity);
  return contrast(
    overBackdrop(over(text, panel), opacity, backdrop),
    overBackdrop(panel, opacity, backdrop),
  );
}

function reportedFor(rects, measured) {
  return rects.find(
    (row) =>
      row.rect[0] === measured[0] &&
      row.rect[1] === measured[1] &&
      row.rect[2] === measured[2] &&
      row.rect[3] === measured[3],
  );
}

test(
  "the bindings card sits below a top menu bar and takes a click, live or paused",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    assert.equal(result.chargingCard[0], INSET, "left inset of the usable area");
    assert.equal(result.chargingCard[1], MENU_BAR + INSET, "below the menu bar");
    assert.deepEqual(result.walkedCard, result.chargingCard, "a walk does not move the card");

    const charging = reportedFor(result.chargingReported, result.chargingCard);
    assert.ok(charging, `charging card reported in ${JSON.stringify(result.chargingReported)}`);
    assert.equal(charging.clickable, true, "a live card takes the press that drags it");
    assert.equal(charging.follows, undefined, "the card does not ride the sprite");

    const walked = reportedFor(result.walkedReported, result.walkedCard);
    assert.ok(walked, "the card is still reported after the sprite moves");
    assert.equal(walked.follows, undefined);
    assert.deepEqual(walked.rect, charging.rect);

    assert.equal(result.pausedCard[1], MENU_BAR + INSET, "pause keeps the same seat");
    const pausedRow = reportedFor(result.pausedReported, result.pausedCard);
    assert.ok(pausedRow, `paused card reported in ${JSON.stringify(result.pausedReported)}`);
    assert.equal(pausedRow.clickable, true, "paused is the resume click");
    assert.equal(pausedRow.follows, undefined);
  },
);

test(
  "a charge that starts after the card is seated is drawn under it and then leaves",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    const bar = reportedFor(result.chargingReported, result.chargingBar);
    assert.ok(bar, `charge bar missing from ${JSON.stringify(result.chargingReported)}`);
    assert.equal(bar.clickable, false, "the bar is drawn and never takes a click");
    assert.equal(bar.follows, undefined, "the bar stays in the corner");
    assert.equal(reportedFor(result.seatedReported, result.chargingBar), undefined);
    assert.ok(result.chargingBar[1] >= result.chargingCard[1] + result.chargingCard[3]);
    assert.ok(result.chargingBar[1] + result.chargingBar[3] < 900, "the bar is inside the window");
    assert.equal(reportedFor(result.releasedReported, result.chargingBar), undefined);
  },
);

test(
  "the release button takes its own click and ends possession the way Esc does",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    assert.ok(result.releaseBox, "an active card has a release button");
    assert.equal(result.releaseText, "Release Possession (Esc)");
    assert.equal(result.releaseHits, true, "the button takes the pointer");
    const [bx, by, bw, bh] = result.releaseBox;
    const row = result.seatedReported.find(
      ({ rect: [x, y, w, h], clickable }) => clickable && x <= bx && y <= by && bx + bw <= x + w && by + bh <= y + h,
    );
    assert.ok(row, `no clickable rect holds the release button in ${JSON.stringify(result.seatedReported)}`);
    assert.equal(row.follows, undefined, "the button stays in the corner with the card");
    assert.deepEqual(result.releaseCalls, [
      { name: "overlay_possession_key", args: { key: "escape", down: true } },
      { name: "overlay_possession_key", args: { key: "escape", down: false } },
    ]);
    assert.equal(result.pausedRelease, false, "a paused card is the resume click, with no button");
  },
);

const SANS = "system-ui, -apple-system, sans-serif";

test(
  "the bindings card, its heading, its keys and its labels render in the shared sans",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    const flat = (family) => family.replace(/["']/g, "").replace(/\s+/g, " ").trim();
    assert.equal(flat(result.fonts.card), SANS);
    assert.equal(flat(result.fonts.heading), SANS);
    assert.equal(flat(result.fonts.keys), SANS);
    assert.equal(flat(result.fonts.label), SANS);
  },
);

test(
  "a paused card keeps the line readable on a light page and a dark one",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    const white = pausedContrast(result.pausedStyle, { r: 255, g: 255, b: 255 });
    const black = pausedContrast(result.pausedStyle, { r: 0, g: 0, b: 0 });
    assert.ok(white >= 4.5, `light ground contrast ${white.toFixed(2)} from ${JSON.stringify(result.pausedStyle)}`);
    assert.ok(black >= 4.5, `dark ground contrast ${black.toFixed(2)} from ${JSON.stringify(result.pausedStyle)}`);
    const panel = parseCssColor(result.pausedStyle.backgroundColor);
    const opacity = Number(result.pausedStyle.opacity);
    assert.ok(panel.a < 1 || opacity < 1, "a paused card stays dimmer than the live one");
  },
);

test(
  "a press on the card's background drags it, and it stays there until the next possession",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    const at = (rect) => rect.slice(0, 2);
    assert.equal(result.pausedDragFocus, 0, "dragging the paused card does not take the keyboard");
    assert.deepEqual(at(result.pausedDropped), [INSET + 300, MENU_BAR + INSET + 200], "the paused card follows the drag");
    assert.equal(reportedFor(result.pausedDroppedReported, result.pausedDropped)?.clickable, true);
    assert.ok(
      result.pausedDragReported.some(
        ({ rect, clickable }) => clickable && rect.join() === [0, MENU_BAR, 1400, 900 - MENU_BAR].join(),
      ),
      `mid-drag the overlay takes the pointer over its whole usable area: ${JSON.stringify(result.pausedDragReported)}`,
    );
    assert.deepEqual(at(result.resumedCard), at(result.pausedDropped), "focus coming back does not move it");
    assert.equal(reportedFor(result.resumedReported, result.resumedCard)?.clickable, true);
    assert.deepEqual(at(result.liveDropped), [INSET + 200, MENU_BAR + INSET + 250], "the live card follows the drag");
    assert.deepEqual(at(result.repausedCard), at(result.liveDropped), "focus leaving does not move it");
    assert.equal(result.clickFocus, 1, "a press that does not drag is still the resume click");
    const [x, y, w, h] = result.clampedCard;
    assert.deepEqual([x + w, y + h], [1400, 900], "a drag past the corner keeps the card on the display");
    assert.deepEqual(at(result.nextSessionCard), [INSET, MENU_BAR + INSET], "a new possession seats it again");
  },
);
