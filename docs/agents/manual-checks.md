# Manual checks

What the automated tests cannot see. `cargo test` covers the core and the UI on fake adapters;
`cargo xtask smoke` covers the real adapters on this Mac (capture and MP4 format, keep-awake
assertion, whisper, pyannote, and with `--with-claude` the review and summary). What is left needs a
human in front of the installed app (`cargo xtask install-app`). Run a check when a change touches
what its "When" names, and all of them before closing a cycle or tagging a release.

1. **Permission prompts.** Reset them (`tccutil reset ScreenCapture io.github.fredealexandre.greffier`,
   same for `Microphone`), launch the app: the permissions card asks for the screen and system
   audio, the microphone and the notifications; it follows each answer as macOS gives it, and a
   refusal says where to go in System Settings.
   *When:* `xtask/src/install_app.rs` (identifier, signature, entitlements, usage descriptions),
   `request_permission` in `crates/capture/src/mac.rs`, the permissions card.

2. **Clicking « Bilan prêt ».** Let a meeting finish processing with the window behind other
   apps, then again with it minimized: the « Bilan prêt » notification appears, and clicking it
   brings the window forward on that meeting, without a crash (the crash fixed in 4e4d500 was an
   AppKit exception thrown by `deminiaturize:`, out of reach of Rust tests).
   *When:* the notification click handler in `crates/app/src/lib.rs`,
   `crates/capture/src/notifications.rs`, anything that shows or restores the window, a Rune bump.

3. **⇧⌘R from another app.** With another app in front (browser, Finder), ⇧⌘R starts a recording,
   ⇧⌘R again stops it; changing the shortcut in the settings takes effect at once.
   *When:* the global shortcut code (`crates/app/src/record_shortcut.rs`), the settings page, a
   Rune or winit bump.

4. **Menu bar icon.** The cat's head is in the menu bar, shows the recording state, its menu starts
   and stops a recording and opens the window; it stays right in light and dark menu bars.
   *When:* `crates/menubar/`, the icon assets, the app's startup and quit.

5. **A real call, end to end.** Record a short video call (someone else talking through the Mac,
   you on the microphone) from the installed app, stop: the meeting plays with sound and picture
   in sync, both voices are in the transcription, the summary arrives. The smoke tests record with
   the terminal's permissions; this one checks the signed app's.
   *When:* `crates/capture/`, `crates/app/src/settings.rs` (`adapters()`), the recording page.

6. **Keep awake for real.** Start a recording, leave the Mac untouched past its display-sleep
   delay: the display stays on and the recording keeps growing; after stopping, the display sleeps
   again. The smoke test only checks that the assertion is held.
   *When:* `crates/capture/src/sleep.rs`, `Greffier::keep_mac_awake`, the app's `keep_awake`.

7. **First launch on an empty app folder.** Move `~/Library/Application Support/Greffier/` aside,
   launch: every resource downloads (an interrupted download resumes), the private Python installs,
   the `claude` check is right; then put the folder back.
   *When:* the resource catalog (`crates/core/src/resources.rs`), `uv.lock`, the pinned uv, the
   first-launch page.

8. **Transcription quality.** Read the transcription of a real meeting of a few minutes: names and
   lexicon terms spelled right, no repeated phrase over a silence, nothing invented at the end.
   *When:* a whisper.cpp bump (`cargo xtask vendor-whisper`), `crates/engines/src/transcribe.cpp`
   parameters, the vocabulary or lexicon handling.

9. **Speakers.** On a real meeting with three people or more, with the speaker count left on
   automatic then set: the right number of speakers, utterances given to the right person,
   interruptions not split off.
   *When:* the pyannote model or lock bump, `crates/engines/src/diarize.py`,
   `crates/core/src/attribution.rs`.

10. **Review and summary quality.** On that real meeting: the review corrects what it should and
    flags real doubts only; the summary's topics, decisions and actions are right, with the right
    timestamps and owners; the proposed speaker names are justified.
    *When:* `crates/tools/resources/prompt-*.md`, the schemas, `claude::MODEL`, a Claude Code
    major update.
