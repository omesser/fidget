#!/usr/bin/env bash
# Assembles the Pages site. pages.yml and tests/landing-page.test.js both run it.
# RELEASE_JSON is `gh release view --json tagName,assets,body`; when it is empty,
# downloads link /releases/latest and the What's new strip is absent.
set -euo pipefail
cd "$(dirname "$0")/.."

site=${1:?usage: assemble-site.sh OUT_DIR RELEASE_JSON}
release=${2:?usage: assemble-site.sh OUT_DIR RELEASE_JSON}

mkdir -p "$site"
cp website/design.html "$site/design.html"
cp website/cues.html "$site/cues.html"
cp website/bubble.html "$site/bubble.html"
cp website/chat.html "$site/chat.html"
cp website/chat-mockups.html "$site/chat-mockups.html"
cp website/window-titles-hint.html "$site/window-titles-hint.html"
cp src/cue.js "$site/cue.js"
cp src/cue-possess.css "$site/cue-possess.css"
cp website/playground.js "$site/playground.js"
cp website/walker.js "$site/walker.js"
cp src/bubble.js "$site/bubble.js"
python3 scripts/make-character-gallery.py --self-check
python3 scripts/make-character-gallery.py --out "$site"
python3 scripts/make-expression-page.py --self-check
python3 scripts/make-expression-page.py --out "$site"
# Must run last: it gives each published page the site header.
python3 scripts/make-landing-page.py --self-check
python3 scripts/make-landing-page.py --out "$site" --release "$release"
cp src-tauri/icons/icon.png "$site/icon.png"
cp website/hero-banner-desktop.webp "$site/hero-banner-desktop.webp"
cp website/hero-banner-desktop.jpg "$site/hero-banner-desktop.jpg"
cp website/hero-banner-mobile.webp "$site/hero-banner-mobile.webp"
cp website/hero-banner-mobile.jpg "$site/hero-banner-mobile.jpg"
