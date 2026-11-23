# Greffier brand assets

Greffier's logo is a cat peeking over the meeting table, eyes closed: it listens. In French slang
*greffier* also means "cat", and in La Fontaine's fable *Le Chat, la Belette et le Petit Lapin* the
judge is a cat, Grippeminaud, who listens with his eyes half closed. The cat is drawn flat and
round, from a few geometric primitives, so that it stays friendly and readable down to 16 px.

## Files

| Path | What | Use |
|---|---|---|
| `app-icon/Greffier.icns` | macOS app icon, every size | Copy to `Greffier.app/Contents/Resources/` and set `CFBundleIconFile` to `Greffier` in `Info.plist` |
| `app-icon/Greffier.iconset/` | The PNGs the `.icns` is built from (16 to 1024 px) | Asset catalogs, or rebuilding the `.icns` |
| `app-icon/greffier-app-icon-1024.png` | The icon at 1024 px, macOS grid (824 px body, transparent margin, shadow) | Website, README, release page |
| `logo/greffier-tile-1024.png` | The cream mark on the blue tile, full square | Social preview, website header |
| `logo/greffier-mark-{blue,ink,cream,white,black}.svg` | The mark alone, one colour, transparent | Documents, website, any size |
| `logo/greffier-mark-{blue,ink,cream,white,black}-1024.png` | The same at 1024 px | Where SVG is not accepted |
| `menubar/MenubarIdleTemplate.png`, `@2x` | Menu bar icon, eyes closed (18 pt) | Menu bar, app ready |
| `menubar/MenubarRecordingTemplate.png`, `@2x` | Menu bar icon, eyes open (18 pt) | Menu bar, while recording |
| `sources/greffier-mark*.svg` | The masters: the mark in black, eyes closed or open, regular or small-size cut | Input of `tools/render.swift` |
| `tools/marks.py` | Builds the masters from their geometry | `python3 assets/brand/tools/marks.py` |
| `tools/render.swift` | Renders everything else from the masters | `swift assets/brand/tools/render.swift` |

## The mark

- **Construction**: a 256 × 256 canvas. The head is one polygon with rounded corners (two ears, a
  dome cut flat at the bottom), the table a bar with round ends (radius 18), the eyes are holes.
  The whole mark sits a little above the geometric centre (optical centre).
- **Two states**: eyes closed (`greffier-mark.svg`), the cat listens; eyes open
  (`greffier-mark-recording.svg`), it is recording. The app icon and the logo use the closed eyes.
- **Small-size cut** (`greffier-mark-small*.svg`): at 32 px and below, the gap between the head and
  the table is wider (22 instead of 14) and the closed eyes are thicker (17 instead of 13), so that
  the table does not stick to the head and the eyes still show.

Sizes and rules applied by the renderer:

- **App icon**: the cream mark on a blue squircle with a light vertical gradient; the mark is about
  58 % of the tile's width (64 % with the small-size cut). Each size of the iconset is rendered at
  its own size, never downscaled from 1024; 32 px and below use the small-size cut.
- **Menu bar**: template images, black with alpha; macOS tints them for light and dark menu bars.
  The small-size cut fills the 18 pt square. Idle shows the closed eyes; while recording, the open
  eyes, tinted red by the app.

## Colours

| Name | Hex | Use |
|---|---|---|
| Blue | `#3D4FD8` | The tile (gradient `#4A5CE4` → `#3242C6`), the mark on light backgrounds |
| Cream | `#F4EFE6` | The mark on the tile and on dark backgrounds |
| Ink | `#14161F` | The mark in one dark colour, text next to it |

## Where the app uses them

- App icon: `xtask/src/install_app.rs` copies `Greffier.icns` into `Contents/Resources` and sets
  `CFBundleIconFile`. The Dock, the Finder and the notifications show it.
- Menu bar: `crates/menubar/src/mac.rs` builds the four template PNGs into the executable
  (`include_bytes!`). While recording, the open-eyes icon is tinted red.

After regenerating, run `cargo xtask install-app` again to see the new files.

## Regeneration

```sh
python3 assets/brand/tools/marks.py     # only after changing the geometry
swift assets/brand/tools/render.swift
```

`marks.py` needs only Python 3. `render.swift` needs macOS 14 or later (AppKit reads the SVGs) and
`iconutil`. The icon is a flat `.icns`: macOS Tahoe's Liquid Glass look (an Icon Composer `.icon`
file compiled by `actool`) would need a full Xcode install.
