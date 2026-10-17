// The published landing page, assembled the way the Pages job assembles
// `_site/`, loaded in headless Chrome with an emulated OS. The browser
// decides which download leads, so only a real browser proves the lead right;
// the page under test is exactly the published bytes, nothing injected.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { findChrome, launchWithPipe, createPageTarget, cleanupChrome } from "./chrome-cdp.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

const SETTLE_MS = 300;
const CLICK_MS = 200;

const chrome = findChrome();
const skip = chrome ? false : "headless Chromium is not installed";

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
  // The page's Android guard must keep the generated state even though the
  // platform string itself names nothing the signposts match.
  Android: {
    ua: `Mozilla/5.0 (Linux; Android 14; Pixel 8) ${CHROME_UA} Mobile`,
    platform: "Android", platformVersion: "14.0.0", architecture: "arm",
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
  const dir = mkdtempSync(join(tmpdir(), "landing-os-"));
  let cdp;
  try {
    const site = join(dir, "_site");
    const built = spawnSync("bash", ["scripts/assemble-site.sh", site, "/dev/null"], { cwd: ROOT, encoding: "utf8" });
    assert.equal(built.status, 0, `assemble-site.sh failed: ${built.stderr}`);
    cdp = launchWithPipe(chrome, [
      "--headless=new",
      "--disable-gpu",
      "--no-sandbox",
      "--disable-dev-shm-usage",
      "--mute-audio",
      `--user-data-dir=${join(dir, "profile")}`,
      "--allow-file-access-from-files",
      "--window-size=1400,900",
    ]);
    return await run(site, cdp);
  } finally {
    if (cdp) await cleanupChrome(cdp);
    rmSync(dir, { recursive: true, force: true, maxRetries: 3 });
  }
}

async function pose(site, cdp, useCase, beforeNavigate) {
  const { sessionId } = await createPageTarget(cdp);
  await cdp.send("Emulation.setUserAgentOverride", {
    userAgent: useCase.ua,
    userAgentMetadata: metadata(useCase),
  }, sessionId);
  const errors = [];
  cdp.on(sessionId, "Runtime.exceptionThrown", (thrown) =>
    errors.push(thrown.exceptionDetails.text ?? "an exception was thrown"));
  if (beforeNavigate) await beforeNavigate(cdp);
  // A load event for the pre-navigation about:blank target is not ours, and
  // the readyState poll covers the event that never arrives.
  let navigated = false;
  const loaded = new Promise((resolve) => cdp.on(sessionId, "Page.loadEventFired", () => {
    if (navigated) resolve();
  }));
  const settled = (async () => {
    for (let waited = 0; waited < 5000; waited += 100) {
      const ready = (await cdp.send("Runtime.evaluate",
        { expression: "document.readyState", returnByValue: true }, sessionId)).result.value;
      if (ready === "complete") return;
    }
  })();
  await cdp.send("Page.navigate", { url: pathToFileURL(join(site, "index.html")).href }, sessionId);
  navigated = true;
  await Promise.race([loaded, settled]);
  // The arch promise lands after load, so the state is read late.
  await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
  return {
    sessionId,
    state: (await cdp.send("Runtime.evaluate", { expression: STATE, returnByValue: true }, sessionId)).result.value,
    errors,
  };
}

test("the browser's platform leads the buttons and its install line", { skip }, async () => {
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

test("the Homebrew copy button copies the command and says Copied", { skip }, async () => {
  await withSite(async (site, cdp) => {
    // The permission is browser-wide, so it is granted before any target opens;
    // reading the value back needs the read grant a write alone does not give.
    await cdp.send("Browser.grantPermissions", { permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"] });
    const { sessionId, state } = await pose(site, cdp, cases["macOS on ARM"]);
    assert.equal(state.first, "macos", "the Copy button has no macOS row to sit on");
    const seen = (await cdp.send("Runtime.evaluate", {
      expression: `(async () => {
        const button = document.querySelector(".copy[data-copy]");
        button.click();
        await new Promise((resolve) => setTimeout(resolve, ${CLICK_MS}));
        let clipboard = null;
        try { clipboard = await navigator.clipboard.readText(); } catch {}
        return { said: button.textContent, clipboard };
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
