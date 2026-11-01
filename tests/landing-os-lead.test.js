// The published landing page is loaded with an emulated OS. The browser
// decides which download leads, so only a real browser proves the lead, and
// the page under test is the published bytes with nothing injected.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { chromeSkip, chromePath, launchWithPipe, createPageTarget, cleanupChrome } from "./chrome-cdp.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

const WAIT_MS = 3000;

const CHROME_UA = "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.8059.85 Safari/537.36";
// The metadata carries the architecture, the high-entropy half of "does this
// machine have a build" that a plain UA string never states.
const cases = {
  "macOS on ARM": {
    ua: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ${CHROME_UA}`,
    platform: "macOS", platformVersion: "14.4.0", architecture: "arm",
    label: "Download for macOS", order: ["macos", "windows", "linux"],
    notes: [["macos", false], ["linux", true], ["unsupported", true]],
    brew: true,
  },
  "macOS on x86": {
    ua: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ${CHROME_UA}`,
    platform: "macOS", platformVersion: "10.15.7", architecture: "x86",
    label: "Download for macOS", order: ["macos", "windows", "linux"],
    notes: [["macos", true], ["linux", true], ["unsupported", false]],
  },
  "Windows on x86": {
    ua: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) ${CHROME_UA}`,
    platform: "Windows", platformVersion: "10.0.0", architecture: "x86",
    label: "Download for Windows", order: ["windows", "macos", "linux"],
    notes: [["macos", true], ["linux", true], ["unsupported", true]],
  },
  "Linux on x86": {
    ua: `Mozilla/5.0 (X11; Linux x86_64) ${CHROME_UA}`,
    platform: "Linux", platformVersion: "5.15.0", architecture: "x86",
    label: "Download for Linux", order: ["linux", "macos", "windows"],
    notes: [["macos", true], ["linux", false], ["unsupported", true]],
  },
  // Phones keep the generated state; the page's user-agent guard is what
  // keeps their desktop-matching signals off the rows.
  Android: {
    ua: `Mozilla/5.0 (Linux; Android 14; Pixel 8) ${CHROME_UA} Mobile`,
    platform: "Android", platformVersion: "14.0.0", architecture: "arm",
  },
  iPhone: {
    ua: `Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/537.36 ` +
      "(KHTML, like Gecko) Chrome/155.0.8059.85 Mobile Safari/537.36",
    platform: "iOS", platformVersion: "17.4.0", architecture: "arm",
  },
  // iPadOS asks for the desktop site, so an iPad sends a Mac user agent; only
  // its touch count keeps its desktop-matching rows off. Safari reaches the same
  // /mac/i check through navigator.platform, as this pose does through hints.
  "an iPad asking for the desktop site": {
    ua: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ${CHROME_UA}`,
    platform: "macOS", platformVersion: "10.15.7", architecture: "arm",
    touch: true,
  },
  // The metadata names Android but the user agent does not, so the page's
  // user-agent guard never fires and no row matches: the page keeps the
  // generated state.
  "an unknown platform": {
    ua: `Mozilla/5.0 (Unknown) ${CHROME_UA}`,
    platform: "Android", platformVersion: "0.0.0", architecture: "x86",
  },
};
const DEFAULT_STATE = {
  label: "Download for",
  first: "macos",
  order: ["macos", "windows", "linux"],
  classes: [["btn"], ["btn"], ["btn"]],
  notes: [
    ["macos", true, "Or install with Homebrew: brew install --cask omesser/fidget/fidget Copy"],
    ["linux", true, "Prefer a .deb? Get it from Releases."],
    ["unsupported", true, "There's no build for your machine yet. You can run it from source."],
  ],
};

// Everything the page's own script could have touched, read back from the DOM.
const STATE = `(() => {
  const buttons = [...document.querySelectorAll("#get .btn")];
  return {
    label: document.getElementById("download-label").textContent,
    first: document.getElementById("get").firstElementChild?.dataset.os ?? null,
    order: buttons.map((button) => button.dataset.os),
    classes: buttons.map((button) => [...button.classList]),
    notes: [...document.querySelectorAll("[data-os-note]")].map((note) =>
      [note.dataset.osNote, note.hidden, note.textContent.trim()]),
  };
})()`;

const metadata = (useCase) => ({
  brands: [
    { brand: "Not.A/Brand", version: "99" },
    { brand: "Chromium", version: "155" },
    { brand: "Google Chrome", version: "155" },
  ],
  fullVersion: "155.0.8059.85",
  platform: useCase.platform,
  platformVersion: useCase.platformVersion,
  architecture: useCase.architecture,
  model: "",
  mobile: false,
});

async function withSite(run) {
  const cdp = launchWithPipe(chromePath, [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--mute-audio",
    "--allow-file-access-from-files",
    "--window-size=1400,900",
  ]);
  try {
    const site = join(cdp.dir, "_site");
    const built = spawnSync("bash", ["scripts/assemble-site.sh", site, "/dev/null"], { cwd: ROOT, encoding: "utf8" });
    assert.equal(built.status, 0, `assemble-site.sh failed: ${built.stderr}`);
    return await run(site, cdp);
  } finally {
    await cleanupChrome(cdp);
  }
}

async function pose(site, cdp, useCase, beforeNavigate) {
  const { sessionId } = await createPageTarget(cdp);
  await cdp.send("Emulation.setUserAgentOverride", {
    userAgent: useCase.ua,
    userAgentMetadata: metadata(useCase),
  }, sessionId);
  // An iPad's touch count comes from an emulated touch point, not from its UA.
  if (useCase.touch) {
    await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 }, sessionId);
  }
  const errors = [];
  cdp.on(sessionId, "Runtime.exceptionThrown", (thrown) =>
    errors.push(thrown.exceptionDetails.text ?? "an exception was thrown"));
  if (beforeNavigate) await beforeNavigate(cdp);
  // Page.enable is on before these tests navigate, so the load event that
  // ends this promise is this navigate's own.
  const loaded = new Promise((resolve) => cdp.on(sessionId, "Page.loadEventFired", resolve));
  await cdp.send("Page.navigate", { url: pathToFileURL(join(site, "index.html")).href }, sessionId);
  await loaded;
  // The page registers its arch call first, so awaiting the same call here
  // settles after every change the arch answer can make.
  await cdp.send("Runtime.evaluate", {
    expression: "Promise.resolve(navigator.userAgentData?.getHighEntropyValues?.(['architecture']))",
    awaitPromise: true,
    returnByValue: true,
  }, sessionId);
  return {
    sessionId,
    state: (await cdp.send("Runtime.evaluate", { expression: STATE, returnByValue: true }, sessionId)).result.value,
    errors,
  };
}

test("the browser's platform leads the buttons and its install line", { skip: chromeSkip }, async () => {
  await withSite(async (site, cdp) => {
    for (const [name, useCase] of Object.entries(cases)) {
      const { state, errors } = await pose(site, cdp, useCase);
      assert.deepEqual(errors, [], `${name} threw nothing`);
      if (useCase.order) {
        assert.equal(state.label, useCase.label, `${name} named the wrong row`);
        assert.equal(state.first, useCase.order[0], `${name} did not lead with its row`);
        assert.deepEqual(state.order, useCase.order, `${name} reordered the wrong rows`);
        assert.deepEqual(state.classes, [["btn", "primary"], ["btn", "quiet"], ["btn", "quiet"]],
          `${name} did not keep one primary row: ${JSON.stringify(state.classes)}`);
        assert.deepEqual(state.notes.map(([os, hidden]) => [os, hidden]), useCase.notes,
          `${name} showed the wrong notes: ${JSON.stringify(state.notes)}`);
        if (useCase.brew) {
          assert.ok(state.notes[0][2].startsWith("Or install with Homebrew:"),
            `${name} lost the Homebrew line: ${state.notes[0][2]}`);
        }
      } else {
        assert.deepEqual(state, DEFAULT_STATE, `${name} left the generated state`);
      }
    }
  });
});

test("the Homebrew copy button copies the command and says Copied", { skip: chromeSkip }, async () => {
  await withSite(async (site, cdp) => {
    // The permission is browser-wide, so it is granted before any target opens;
    // reading the value back needs the read grant a write alone does not give.
    await cdp.send("Browser.grantPermissions", { permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"] });
    const { sessionId, state } = await pose(site, cdp, cases["macOS on ARM"]);
    assert.equal(state.first, "macos", "the Copy button has no macOS row to sit on");
    const seen = (await cdp.send("Runtime.evaluate", {
      expression: `(async () => {
        const button = document.querySelector(".copy[data-copy]");
        const said = new Promise((resolve) => new MutationObserver((_, observer) => {
          observer.disconnect();
          resolve(button.textContent);
        }).observe(button, { childList: true, subtree: true, characterData: true }));
        button.click();
        const settled = await Promise.race([said,
          new Promise((resolve) => setTimeout(() => resolve(null), ${WAIT_MS}))]);
        let clipboard = null;
        try { clipboard = await navigator.clipboard.readText(); } catch {}
        return { said: settled, clipboard };
      })()`,
      awaitPromise: true,
      returnByValue: true,
    }, sessionId)).result.value;
    assert.equal(seen.said, "Copied", "the button did not say Copied after its click");
    if (seen.clipboard !== null) {
      assert.equal(seen.clipboard, "brew install --cask omesser/fidget/fidget",
        "the clipboard held a different command");
    }
    // On a file:// page the clipboard read can stay forbidden even with the
    // permission granted; then only the label is affordable to assert.
  });
});

// The code is a flex item, so its own client rects are one box even when its
// text wraps; the rects of a Range over its text are the line boxes.
const BREW_LAYOUT = `(() => {
  const note = document.querySelector('[data-os-note="macos"]');
  const text = document.createRange();
  text.selectNodeContents(note.querySelector("code"));
  const code = text.getBoundingClientRect();
  const copy = note.querySelector(".copy").getBoundingClientRect();
  return {
    width: document.documentElement.clientWidth,
    lines: new Set([...text.getClientRects()].map((rect) => Math.round(rect.top))).size,
    besideCopy: code.top < copy.bottom && copy.top < code.bottom,
    copyRight: copy.right,
  };
})()`;

test("the Homebrew command stays whole, beside its Copy button, and on screen", { skip: chromeSkip }, async () => {
  await withSite(async (site, cdp) => {
    const { sessionId } = await pose(site, cdp, cases["macOS on ARM"]);
    for (const width of [1280, 390]) {
      await cdp.send("Emulation.setDeviceMetricsOverride",
        { width, height: 900, deviceScaleFactor: 1, mobile: false }, sessionId);
      const at = (await cdp.send("Runtime.evaluate", { expression: BREW_LAYOUT, returnByValue: true },
        sessionId)).result.value;
      assert.equal(at.lines, 1, `at ${width}px the command broke across ${at.lines} lines`);
      assert.ok(at.besideCopy, `at ${width}px Copy left the command's line`);
      assert.ok(at.copyRight <= at.width, `at ${width}px Copy ran off-screen`);
    }
  });
});

// Where the site header sits after a scroll, and whether the page runs wider
// than the viewport.
const HEADER_AT = `(() => {
  scrollTo(0, 1500);
  return {
    scrolled: scrollY > 0,
    top: document.querySelector(".site-nav")?.getBoundingClientRect().top ?? null,
    left: document.querySelector(".site-nav")?.getBoundingClientRect().left ?? null,
    overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
  };
})()`;

test("the site header stays at the top, flush left, on every page", { skip: chromeSkip }, async () => {
  await withSite(async (site, cdp) => {
    const { sessionId } = await pose(site, cdp, cases["macOS on ARM"]);
    for (const page of readdirSync(site).filter((name) => name.endsWith(".html"))) {
      const loaded = new Promise((resolve) => cdp.on(sessionId, "Page.loadEventFired", resolve));
      await cdp.send("Page.navigate", { url: pathToFileURL(join(site, page)).href }, sessionId);
      await loaded;
      for (const width of [1280, 390]) {
        await cdp.send("Emulation.setDeviceMetricsOverride",
          { width, height: 800, deviceScaleFactor: 1, mobile: false }, sessionId);
        const at = (await cdp.send("Runtime.evaluate", { expression: HEADER_AT, returnByValue: true },
          sessionId)).result.value;
        assert.ok(at.scrolled, `${page} at ${width}px did not scroll`);
        assert.equal(at.top, 0, `${page} at ${width}px left the header at ${at.top}`);
        assert.equal(at.left, 0, `${page} at ${width}px set the header ${at.left}px in`);
        assert.ok(at.overflow <= 0, `${page} at ${width}px runs ${at.overflow}px wider than the screen`);
      }
    }
  });
});
