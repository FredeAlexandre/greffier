# Greffier

A Mac app that records work meetings, transcribes them, reviews them and writes their summary, to
quickly find what was said, decided and to be done. Everything runs on the Mac (capture,
transcription, diarization); only the review and the summary go through a Claude Code agent.
Meetings are stored as readable files (Markdown, JSON, video) in a meetings folder, for the user as
well as for their agents. The app's UI and the files it writes are in French.

## Status

**Cycle 1 built.** The UI, written with [Rune](https://github.com/anomalyco/rune), runs on top of a
headless core wired to the real adapters: native capture of the screen, the Mac audio and the
microphone (ScreenCaptureKit, 1080p MP4), import of a past meeting, integrated whisper.cpp
transcription, diarization by pyannote in a private Python, review and summary by the `claude` CLI,
writing the files into the meetings folder (watched for changes made outside the app), search,
trash, menu bar icon. Headless (scenes, scenarios), the app stays on demo data and fake adapters.
During the cycles, the app only targets its author's Mac; 1.0.0 will come with an installation guide
and downloadable packages.

- User stories, cycles and 1.0.0 criteria: [docs/user-stories.md](docs/user-stories.md)
- Domain vocabulary: [GLOSSARY.md](GLOSSARY.md)
- Architecture decisions: [docs/adr/](docs/adr/)

## Run

```sh
cargo run            # the window (first build: ~2 min)
cargo test           # the core on its fake adapters, the UI driven headless (crates/app/tests), the rest
cargo xtask smoke    # the real adapters on this Mac: capture (records 5 s, plays a tone), whisper, pyannote
cargo xtask smoke --with-claude   # and the review and summary by `claude` (costs Claude tokens)
```

The smoke tests are `#[ignore]`d by `cargo test` (and CI). They need the screen and microphone
permissions for the app that runs them (Terminal, the IDE), ffmpeg, the installed resources and a
meeting's audio (`GREFFIER_SMOKE_AUDIO`, default `~/Bench-Transcription/audio/A.wav`); a missing
prerequisite is skipped and listed in the summary.

## Install the app

```sh
cargo xtask install-app
```

Builds in release, assembles `Greffier.app` (identifier `io.github.fredealexandre.greffier`, version
read from `Cargo.toml`, usage descriptions for the screen, the microphone and the system audio,
hardened runtime with the microphone entitlement), signs it with the local identity "Greffier Dev",
replaces it in `/Applications` (quitting the app if it is running), then opens it and waits for its
window.

- The first time, the "Greffier Dev" identity (self-signed code signing certificate, 10 years) is
  created in the login keychain; after that it is reused. If the keychain is locked, the command
  stops and says to run `security unlock-keychain`, which asks for the login password.
- macOS ties permissions (screen, microphone, system audio, notifications) to the identifier and the
  certificate: they hold from one install to the next. Don't delete the identity and don't change
  the identifier, otherwise they have to be granted again.
- `--binary PATH` packages an already built executable, `--no-launch` does not open the app.
- The bundle embeds `uv` (version and sha256 pinned in `crates/tools/src/mac.rs`), downloaded once
  into `target/uv-<version>/`, where `cargo run` finds it too.

## First-launch resources

At first launch, the window shows the state of each resource and installs them all in
`~/Library/Application Support/Greffier/`, and nowhere else:

- whisper large-v3 and Silero VAD v5.1.2, from Hugging Face; pyannote `speaker-diarization-community-1`
  (CC-BY-4.0), from the repository's [`pyannote-community-1`](https://github.com/FredeAlexandre/greffier/releases/tag/pyannote-community-1)
  release, published with its license and attribution by `cargo xtask publish-pyannote`. Each file is
  checked against a pinned sha256; an interrupted download resumes where it stopped.
- pyannote's private Python, installed by the bundled uv from `crates/core/resources/python/uv.lock`.
- the `claude` CLI is only checked (installed and signed in), with what to do otherwise.
- the shipped lexicon (`crates/core/resources/lexicon.md`) is written there as `lexique.md` if it does not
  exist yet.

Headless: `cargo run -p greffier-tools --example install_resources` (`-- --check` to only run the
checks). Scenes and scenarios keep fake resources.

## Things to try

| Gesture | Where |
|---|---|
| ⇧⌘R or the red button | starts / stops a recording (time sped up ×20 in scenes) |
| ⌘M while recording | sets a marker, visible later on the timeline |
| Arrêter (stop) | the meeting arrives, goes through Transcription → Review → Summary, "Bilan prêt" banner |
| "Réunion du sam. 21/11" | meeting not processed yet: speaker count, "Transcrire et faire le bilan" |
| "Revue de release" | full-width video, timeline by topic and by speaker, decisions (◆), markers |
| Space, ← → | play / pause, ±10 s |
| Drag on the tracks | moves playback; click on a topic or a ◆: jumps there |
| ⌘1 … ⌘5 | Bilan / Transcription / À vérifier / Locuteurs / Infos (summary, transcription, passages to check, speakers, info) |
| Bilan (summary) | rendered Markdown; the ▶ timestamps start playback, the boxes can be checked, "Refaire le bilan" |
| À vérifier (passages to check) | ↑ ↓ from one passage to the next, E plays the clip (stops at its end), A accepts the hypothesis, G keeps the original, C corrects (↩ saves); "Annuler" reopens a passage |
| Transcription: right-click on an utterance, or C | corrects its text (↩ saves, esc cancels); `transcription.md` and `.json` are rewritten |
| ⌘K or "Rechercher" | searches titles, summaries and transcriptions; ↩ opens at the right moment |
| Transcription › "Refaire…" | redo the review (current lexicon) then the summary, or redo the transcription with another speaker count, then the review and the summary |
| Right-click on a meeting | menu: open, read the transcription, redo the summary / the review / the transcription, show in Finder, copy the summary, delete (Mac trash, after confirmation, or ⌘⌫ on the open meeting) |

## Headless

Scripted scenes and scenarios serve as visual tests:

```sh
target/debug/greffier --list-scenes
target/debug/greffier --snap out.png --scene meeting --theme dark
target/debug/greffier --script crates/app/scenarios/recording.json --snap end.png --at 20
# A scenario can start from a scene: the speaker's name field loses focus, f and space stay out of the page.
target/debug/greffier --scene people-naming --script crates/app/scenarios/naming-focus-lost.json --snap end.png --at mark:end
# ⌘2 from the name field, then space: the video plays, not full screen.
target/debug/greffier --scene people-naming --script crates/app/scenarios/tab-change-space.json --snap end.png --at mark:end
# A click on a meeting in the sidebar, then space: the video plays.
target/debug/greffier --script crates/app/scenarios/sidebar-space.json --snap end.png --at mark:end
# A click on a summary timestamp scrolls back up to the video, which plays.
target/debug/greffier --scene meeting --script crates/app/scenarios/summary-timestamp.json --snap end.png --at mark:end
```

## Code

The root `Cargo.toml` only declares the workspace (members, patches, profiles); every crate
lives in `crates/`, except `xtask/`.

- `crates/core/`: the headless core (crate `greffier-core`, no Rune and no `unsafe`). It owns the state of
  the meetings, the recording and the processing queue; it is driven by commands
  (`Greffier::execute`) and time is moved forward with `Greffier::advance`. It only knows the
  adapters' interfaces.
  - `adapters.rs`: the interfaces of everything that leaves the process (capture, transcriber,
    diarizer, agent, resources, trash, MP4 muxing, clock); `fake.rs`: their fakes.
  - `meeting.rs`: meeting, status, speakers, utterances, passages to check; `summary.rs`: what is
    read from the Markdown summary; `attribution.rs`: words attributed to speakers, and utterances
    (the vault's rules); `time.rs`: meeting dates and times.
  - `folder/`: the meetings folder (naming, sheet, transcriptions, import, renaming, fingerprints,
    MP4 duration); `watch.rs`, `search.rs`, `deletion.rs`, `title.rs`: reread the folder, search,
    move to the trash, rename from the title.
  - `settings.rs`: the remembered settings (`settings.json` in the app folder); `resources.rs`: the
    resource catalog (URLs, sizes, sha256) and its adapter; `lexicon.rs` and
    `resources/lexicon.md`: the shipped lexicon.
  - `demo.rs`: the prototype's demo data and fake adapters.
  - `tests/`: the test bench (temporary folder, fake adapters, commands, state and files).
- `crates/tools/` (`greffier-tools`): the real adapters that drive programs: `claude/` (the agent: review,
  summary, prompts in `resources/`), `mac.rs` (curl, sha256, bundled uv, private Python),
  `subprocess.rs` (work on a thread, waiting for a program that can be stopped).
- `crates/engines/` (`greffier-engines`): whisper.cpp built from its sources, audio decoded by AVFoundation,
  pyannote (`diarize.py`) and the relisten.
- `crates/capture/` (`greffier-capture`): ScreenCaptureKit and AVAssetWriter, the keep-awake assertion,
  the Mac trash, MP4 muxing of imported media. `crates/menubar/` (`greffier-menubar`): the menu bar icon.
- `crates/app/` (`greffier`, the binary): the UI around the core.
  - `src/main.rs`: playback, shortcuts, shell, scenes.
  - `src/meeting.rs`: meeting page, video, tabs. `src/timeline.rs`: the timeline.
    `src/overlays.rs`: ⌘K search, deletion confirmation.
  - `src/pages.rs`: overview, recording, settings, first launch. `src/sidebar.rs`: sidebar and
    right-click menu. `src/ui.rs`: small shared UI pieces.
  - `src/check.rs`, `src/speakers.rs`, `src/redo.rs`, `src/title.rs`, `src/import.rs`: the À
    vérifier (passages to check) and Locuteurs (speakers) tabs, redoing a stage, changing the
    title, importing. `src/settings.rs`: the app's real adapters; `src/watch.rs`: FSEvents.
  - `scenarios/`: the scripted UI scenarios (`--script`).
- `xtask/`: the development tasks, run with `cargo xtask <task>` (alias in `.cargo/config.toml`):
  `install-app` (packaging, signing and installing `Greffier.app`), `vendor-whisper` (copying the
  whisper.cpp sources pinned in `crates/engines/whisper-version.rs` into `crates/engines/whisper.cpp/`) and
  `publish-pyannote` (publishing the release of the pyannote files listed in the core's catalog,
  with `xtask/pyannote/`: license, attribution).

Rune is pinned to a `dev` revision (pre-release) in `crates/app/Cargo.toml` (and the root `Cargo.toml`
patches); it is only changed on purpose
([ADR 0004](docs/adr/0004-ui-in-rune.md)).
