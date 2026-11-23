# Greffier brand assets

Greffier's logo is a cat's head. In French slang *greffier* also means "cat", and in La Fontaine's
fable *Le Chat, la Belette et le Petit Lapin* the judge is a cat, Grippeminaud, who listens with his
eyes half closed. The head is shown as a marble sculpture rendered in round halftone dots (cream on
black) for the app icon and the logo, and as a flat mark for small sizes and the menu bar.

## Files

| Path | What | Use |
|---|---|---|
| `app-icon/Greffier.icns` | macOS app icon, every size | Copy to `Greffier.app/Contents/Resources/` and set `CFBundleIconFile` to `Greffier` in `Info.plist` |
| `app-icon/Greffier.iconset/` | The PNGs the `.icns` is built from (16 to 1024 px) | Asset catalogs, or rebuilding the `.icns` |
| `app-icon/greffier-app-icon-1024.png` | The icon at 1024 px, macOS grid (824 px body, transparent margin, shadow) | Website, README, release page |
| `logo/greffier-dots-on-black-1024.png` | The dotted head, full square on black | Social preview, website header |
| `logo/greffier-dots-transparent-1024.png` | The same dots on a transparent background | On dark backgrounds only (the dots are cream) |
| `logo/greffier-mark-{white,black,cream}-1024.png` | The flat mark (eyes closed) on transparent | Favicon, small sizes, documents |
| `menubar/MenubarIdleTemplate.png`, `@2x` | Menu bar icon, eyes closed (18 pt) | Menu bar, app ready |
| `menubar/MenubarRecordingTemplate.png`, `@2x` | Menu bar icon, eyes open (18 pt) | Menu bar, while recording |
| `sources/` | The three source images | Input of `tools/render.swift` |
| `tools/render.swift` | Renders everything above from `sources/` | `swift assets/brand/tools/render.swift` |

Sizes and rules applied by the renderer:

- **App icon**: each size of the iconset is rendered at its own size, never downscaled from 1024,
  so the dots stay sharp. At 64 px and below the dots no longer read, so these sizes use the flat
  mark in cream on the same black squircle.
- **Menu bar**: template images, black with alpha; macOS tints them for light and dark menu bars.
  The mark's lines are thickened before reducing so that the eyes still cover about a pixel at
  18 and 36 px. The two states differ by the eyes: closed when idle, open while recording, the cat
  is listening.

## Colors

| Name | Hex | Use |
|---|---|---|
| Ink | `#0A0A0A` | Background of the icon and of the dotted logo |
| Cream | `#EFE6D6` | The dots, the mark on dark backgrounds |

## Where the app uses them

- App icon: `xtask/src/install_app.rs` copies `Greffier.icns` into `Contents/Resources` and sets
  `CFBundleIconFile`. The Dock, the Finder and the notifications show it.
- Menu bar: `crates/menubar/src/mac.rs` builds the four template PNGs into the executable
  (`include_bytes!`). While recording, the open-eyes icon is tinted red.

After regenerating, run `cargo xtask install-app` again to see the new files.

## Sources and regeneration

The three source images were generated with Google Gemini (image model "Nano Banana"):

- `head-sculpture.jpg`: "A modernist sculpture of a cat's head only, front view, carved in smooth
  matte white marble, simplified geometric volumes in the spirit of François Pompon. Clearly feline
  anatomy, not human […] eyes peacefully closed […] Pure black background, single soft key light
  from the upper left […]"
- `head-mark-eyes-closed.jpg`: the sculpture given as reference, "turn this marble cat head
  sculpture into a minimalist flat logo mark […] pure white solid shape on a pure black background".
- `head-mark-eyes-open.jpg`: the closed-eyes mark given as reference, same shape with open almond
  eyes and slit pupils.

To regenerate every file after changing a source or a parameter:

```sh
swift assets/brand/tools/render.swift
```

It needs only macOS (Core Graphics, ImageIO and `iconutil`).
