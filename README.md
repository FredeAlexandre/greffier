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
trash, menu bar icon. Headless (scenes) and in its tests, the app stays on demo data and fake adapters.
During the cycles, the app only targets its author's Mac; 1.0.0 will come with an installation guide
and downloadable packages.

- User stories, cycles and 1.0.0 criteria: [docs/user-stories.md](docs/user-stories.md)
- Domain vocabulary: [GLOSSARY.md](GLOSSARY.md)
- Architecture decisions: [docs/adr/](docs/adr/)

## Run

```sh
cargo run            # the window (first build: ~2 min)
cargo run -- help    # the command line for agents (search, list, show, rename…), without the window
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
- `--binary PATH` packages an already built executable, `--no-launch` does not open the app,
  `--no-link` does not link the command line (below).
- The bundle embeds `uv` (version and sha256 pinned in `crates/tools/src/mac.rs`), downloaded once
  into `target/uv-<version>/`, where `cargo run` finds it too.

## The command line

`cargo xtask install-app` links `~/.local/bin/greffier` to the installed app's executable
(`/Applications/Greffier.app/Contents/MacOS/greffier`): a symbolic link there is replaced, a file
is left as it is with a warning, and it says so if `~/.local/bin` is not in the `PATH`. In
development, `cargo run -- <command>` or `target/debug/greffier <command>` does the same.

```sh
greffier help                         # every command and option
greffier search recette               # what was said, by meeting
greffier list                         # the meetings, most recent first
greffier show 2026-11-20-1000         # a meeting's outline; --bilan, --transcription, --json…
greffier rename 2026-11-20-1000 "Revue de release"
greffier delete 2026-11-20-1000 --yes # to the Mac's trash
```

Reading (`search`, `list`, `show`, `status`) goes straight to the meetings folder and never starts
the app. Writing (`import`, `process`, `redo`, `stop`, `rename`, `speaker`, `correct`, `resolve`,
`check-action`, `delete`) is sent to the running Greffier over `greffier.sock` in the app folder
([ADR 0006](docs/adr/0006-cli-talks-to-the-greffier-process.md)); if none runs, the command line
starts it without a window: `open -g -j -b io.github.fredealexandre.greffier --args --background`
for the installed app, the binary itself (`greffier --background`) in development or when
`GREFFIER_FOLDER` or `GREFFIER_APP_FOLDER` is set. Commands, output and JSON shapes:
[crates/cli/README.md](crates/cli/README.md).

Started with `--background`, Greffier has no window and no Dock icon; its menu bar icon shows the
progress and has « Ouvrir » and « Quitter ». Opening it (Finder, Dock, Spotlight, `open`, « Ouvrir »,
or launching `greffier` again) shows the window of the same process, with the processing under way,
and it then behaves as if opened normally. Until then it quits on its own after 5 minutes with
nothing to process and no request. One Greffier runs per app folder: a second launch asks the first
to show its window and exits.

| Variable | |
|---|---|
| `GREFFIER_FOLDER` | another meetings folder, for this run (the command line and the app) |
| `GREFFIER_APP_FOLDER` | another app folder (settings, lexicon, search cache, socket, lock) |
| `GREFFIER_IDLE_QUIT` | seconds a background Greffier waits idle before quitting (300); for trials |

### The Claude Code skill

[`skills/greffier/SKILL.md`](skills/greffier/SKILL.md) teaches an agent the way to use the command
line: `search`, then `show --transcription --from … --to …` on a window around what was found, then
an answer citing the meeting and the timestamp; for imports, `status` or `--wait` with a
`--timeout`. Make it available to Claude Code in every project with a link:

```sh
ln -s "$PWD/skills/greffier" ~/.claude/skills/greffier
```

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

Scenes and the app's tests keep fake resources.

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

Scenes render a screen of the app on demo data, to look at it:

```sh
target/debug/greffier --list-scenes
target/debug/greffier --snap out.png --scene meeting --theme dark
```

What the UI does is tested in `crates/app/tests/` (the app driven without a window, see
`crates/app/tests/support/mod.rs`).

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
    MP4 duration); `identifier.rs`: a meeting's identifier (`2026-11-20-0939`) and what may name
    it; `watch.rs`, `search.rs`, `deletion.rs`, `title.rs`: reread the folder, search,
    move to the trash, rename from the title.
  - `settings.rs`: where the app and meetings folders are (`GREFFIER_APP_FOLDER`,
    `GREFFIER_FOLDER`), the remembered settings (`settings.json` in the app folder); `resources.rs`: the
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
- `crates/cli/` (`greffier-cli`): the command line for agents (ADR 0006). Reading (`list`,
  `show`, `status`, `--json`, shapes in its `README.md`) goes to the meetings folder; writing
  (`import`, `process`, `rename`…) goes over a socket to the Greffier process, launched without a
  window if needed. `src/server/` is that process's side (protocol, actions on the core, socket,
  lock, idle quit), shared by the app and `greffier-test-server`, the tests' host. It depends on
  the core alone.
- `crates/app/` (`greffier`, the binary): the UI around the core.
  - `src/lib.rs`: playback, shortcuts, shell, scenes; `src/main.rs` hands a command
    (`greffier list`…) to `greffier-cli`, and otherwise runs the UI. `src/command_line.rs`: the
    app as the process the command line talks to: lock, socket, `--background`, idle quit.
  - `src/meeting.rs`: meeting page, video, tabs. `src/timeline.rs`: the timeline.
    `src/overlays.rs`: ⌘K search, deletion confirmation.
  - `src/pages.rs`: overview, recording, settings, first launch. `src/sidebar.rs`: sidebar and
    right-click menu. `src/ui.rs`: small shared UI pieces.
  - `src/check.rs`, `src/speakers.rs`, `src/redo.rs`, `src/title.rs`, `src/import.rs`: the À
    vérifier (passages to check) and Locuteurs (speakers) tabs, redoing a stage, changing the
    title, importing. `src/settings.rs`: the app's real adapters; `src/watch.rs`: FSEvents.
  - `tests/`: the UI driven headless, as the user does (clicks by key, keys, time), on fake adapters.
- `xtask/`: the development tasks, run with `cargo xtask <task>` (alias in `.cargo/config.toml`):
  `install-app` (packaging, signing and installing `Greffier.app`, linking `~/.local/bin/greffier`), `vendor-whisper` (copying the
  whisper.cpp sources pinned in `crates/engines/whisper-version.rs` into `crates/engines/whisper.cpp/`) and
  `publish-pyannote` (publishing the release of the pyannote files listed in the core's catalog,
  with `xtask/pyannote/`: license, attribution).

Rune is pinned to a `dev` revision (pre-release) in `crates/app/Cargo.toml` (and the root `Cargo.toml`
patches); it is only changed on purpose
([ADR 0004](docs/adr/0004-ui-in-rune.md)).
