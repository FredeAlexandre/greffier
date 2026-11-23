# The UI is written with Rune, pinned to a `dev` revision

Greffier keeps Rune (Rust, Metal rendering), already used by the prototype, rather than a SwiftUI or web app: it natively covers the global shortcut, video and audio playback through AVFoundation, notifications, context menus, drag and drop and background work. Rune is pre-release, its APIs change a lot and it does not accept contributions: we pin it to a precise revision (with its `[patch.crates-io]` block) and only change it on purpose.

## Consequences

- The menu bar icon does not exist yet on macOS in Rune: we write it ourselves in `objc2` (NSStatusItem) from the main thread (`AppHandle::update`).
- Screen + Mac audio capture (ScreenCaptureKit) also has to be written ourselves; Rune only records the microphone.
- Rune does not build an app: we have our own bundle script (Info.plist, entitlements, signing).
