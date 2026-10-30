import assert from "node:assert/strict";
import test from "node:test";

import { catalogKeys, readCatalog } from "./i18n-keys.js";

const LANGS = ["fr", "es"];
const placeholders = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

// A string the source translates and a catalog lacks falls back to English,
// which reads as a half-translated screen. Every one has to be in both.
test("every translated string is in every catalog", () => {
  const keys = catalogKeys();
  for (const lang of LANGS) {
    const catalog = readCatalog(lang);
    const missing = [...keys].filter((key) => !Object.hasOwn(catalog, key));
    assert.deepEqual(missing, [], `${lang}.json lacks these`);
  }
});

test("no catalog keeps a string the source no longer sends", () => {
  const keys = catalogKeys();
  for (const lang of LANGS) {
    const stale = Object.keys(readCatalog(lang)).filter((key) => !keys.has(key));
    assert.deepEqual(stale, [], `${lang}.json still carries these`);
  }
});

test("a translation keeps every placeholder and code span", () => {
  for (const lang of LANGS) {
    for (const [en, translated] of Object.entries(readCatalog(lang))) {
      assert.ok(translated.trim(), `${lang}: ${en} is blank`);
      assert.deepEqual(placeholders(translated), placeholders(en), `${lang}: ${en}`);
      assert.equal(translated.split("`").length, en.split("`").length, `${lang}: ${en}`);
    }
  }
});
