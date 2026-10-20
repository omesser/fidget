#!/usr/bin/env bash
# Assembles the Pages site. pages.yml and tests/landing-page.test.js both run it.
# RELEASE_JSON is `gh release view --json tagName,assets,body`; when it is empty,
# downloads link /releases/latest and the What's new strip is absent.
set -euo pipefail
cd "$(dirname "$0")/.."

site=${1:?usage: assemble-site.sh OUT_DIR RELEASE_JSON}
release=${2:?usage: assemble-site.sh OUT_DIR RELEASE_JSON}

mkdir -p "$site"
cp docs/design/design.html "$site/design.html"
cp docs/design/cues.html "$site/cues.html"
cp docs/design/bubble.html "$site/bubble.html"
cp docs/design/chat.html "$site/chat.html"
cp docs/design/chat-mockups.html "$site/chat-mockups.html"
cp docs/design/window-titles-hint.html "$site/window-titles-hint.html"
cp src/cue.js "$site/cue.js"
cp docs/design/playground.js "$site/playground.js"
python3 scripts/make-character-gallery.py --self-check
python3 scripts/make-character-gallery.py --out "$site"
python3 scripts/make-expression-page.py --self-check
python3 scripts/make-expression-page.py --out "$site"
# Must run last: it gives each published page the site header.
python3 scripts/make-landing-page.py --self-check
python3 scripts/make-landing-page.py --out "$site" --release "$release"
cp src-tauri/icons/icon.png "$site/icon.png"
cp docs/assets/hero-banner-desktop.webp "$site/hero-banner-desktop.webp"
cp docs/assets/hero-banner-desktop.jpg "$site/hero-banner-desktop.jpg"
cp docs/assets/hero-banner-mobile.webp "$site/hero-banner-mobile.webp"
cp docs/assets/hero-banner-mobile.jpg "$site/hero-banner-mobile.jpg"
