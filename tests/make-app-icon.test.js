import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { tempDir } from "./temp-path.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(ROOT, "scripts", "make-app-icon.py");

// Outputs go to the temp dir too, so a miss cannot rewrite the icons in the tree.
const DRIVER = `
import importlib.util
import sys
from pathlib import Path
from PIL import Image

script, dest = sys.argv[1], Path(sys.argv[2])
art = dest / "logo-512.png"
Image.new("RGB", (32, 32), (18, 16, 22)).save(art)
icons = dest / "icons"
icons.mkdir()
spec = importlib.util.spec_from_file_location("make_app_icon", script)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
mod.SOURCE = art
mod.MASTER = dest / "app-icon-1024.png"
mod.PREVIEW = dest / "preview.png"
mod.ICONS = icons
mod.main()
`;

test("a flat logo exits saying the art has no head to find", (t) => {
  const dir = tempDir(t, "flat-logo-");
  assert.throws(
    () =>
      execFileSync("python3", ["-c", DRIVER, SCRIPT, dir], {
        cwd: ROOT,
        encoding: "utf8",
      }),
    (error) => {
      assert.equal(error.status, 1);
      assert.equal(error.stderr, "logo-512.png has no head to find\n");
      assert.equal(error.stdout, "");
      assert.deepEqual(readdirSync(dir).sort(), ["icons", "logo-512.png"]);
      assert.deepEqual(readdirSync(join(dir, "icons")), []);
      return true;
    },
  );
});
