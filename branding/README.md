# Fidget Branding Assets

Product logo and tray icon art package from Oded.

`app-icon-preview.png` shows what the app icon is supposed to look like — the
grid, the Dock slot it sits in against the square it replaced, and every size
the `.ico` carries. Regenerate it when the art changes, so a reviewer can see
the change rather than take the diff's word for it.

## Structure

### `banner.jpg`
README banner, 2000×667 JPEG, 87 KB. Carries the product name and the
"A Desktop Pet" tagline, so the README puts no heading above it. Checked in
as delivered; do not recompress or resize.

### Landing page hero banner (`website/`)
The banner atop the GitHub Pages landing page lives in `website/`, not
here, because `scripts/assemble-site.sh` copies it from there by name:
- `hero-banner-desktop.jpg` / `.webp` — 1600×533, 93 KB / 52 KB, the
  default sources.
- `hero-banner-mobile.jpg` / `.webp` — 960×533, 52 KB / 28 KB, picked at
  760px wide or less.

`website/landing.html` names all four; `tests/landing-page.test.js` fails
if one it names is missing from the assembled site.

### `logo-art/`
Reference PNG assets:
- `logo-512.png` — Product logo, 512×512 RGB, 153 KB. The source art
  `scripts/make-app-icon.py` cuts the app icon from; edit this, then rerun it.
- `app-icon-1024.png` — The app icon on Apple's macOS grid, 1024×1024 RGBA,
  318 KB. Generated from `logo-512.png`, not drawn.
- `tray-256.png` — Tray mark, 256×256 RGBA, 23 KB
- `tray-64.png` — Tray mark, 64×64 RGBA, 4.0 KB. `tray-256.png` downscaled once with Lanczos
- `tray-32.png` — Tray mark, 32×32 RGBA, 1.8 KB

## Usage

The app icon (`src-tauri/icons/icon.png`) is 512×512 RGBA, cut to Apple's macOS grid: a squircle body filling 824 of a 1024 canvas, scaled down to 512, with the margin around it transparent. Nothing downstream rounds a square icon — Tauri hands this file to `NSApp.setApplicationIconImage` on a dev run, and tauri-bundler builds the `.icns` from it for a packaged one — so the Dock draws whatever shape the file has.

512 is the largest size both packagers take. In the `.icns` format, 1024×1024 exists only as the 2x form of 512, which tauri-bundler asks for by filename, so a plain `icon.png` at 1024 matches no element type and the bundler errors out rather than skipping it. On Linux the icon is filed by its own dimensions, at `usr/share/icons/hicolor/<width>x<height>/apps/`, and the hicolor theme lists no directory above 512 — so an icon larger than that lands where nothing looks for it.

`python3 scripts/make-app-icon.py` regenerates this file, `logo-art/app-icon-1024.png` and `src-tauri/icons/icon.ico` from `logo-art/logo-512.png`.

The tray icon (`src-tauri/icons/tray.png`) is `logo-art/tray-64.png`, a 64×64 template icon for the system menu bar, designed to adapt to light/dark themes. `tauri::include_image!` embeds it as raw RGBA (16 KB at 64 px, 256 KB at 256), and 64 covers the largest tray any shell draws, 44 px on a Retina menu bar, so the OS only ever scales it down.
