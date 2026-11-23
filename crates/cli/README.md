# greffier-cli

Greffier's command line for agents (ADR 0006). `greffier search`, `list`, `show`, `status` read
the meetings folder (and the app's search cache) through the core and never write. `import`,
`process`, `redo`, `stop`, `rename`, `speaker`, `correct`, `resolve`, `check-action`, `delete`
are sent to the Greffier process over its socket, which is started without a window if it does
not run. `greffier help` gives the usage. A meeting is named by its identifier
(`2026-11-20-0939`), its folder name, a path to its folder (only inside the meetings folder:
the same name elsewhere is refused), a date or the start of an identifier. Any first
argument that does not start with `-` is a command (a mistyped one fails with exit code 2);
`greffier` alone, or with the window's options (`--snap`, `--scene`…), opens the window, and
`greffier --background` starts the app without a window, as the command line launches it (the
app's side: `crates/app/src/command_line.rs`). After the command, options and arguments come in
any order; after `--`, everything is an argument, for a text that starts with `-` (`greffier
correct 2026-11-20-0939 3 -- -5 degrees`).

The command line speaks English, with the glossary's English terms (meeting, summary, sheet,
utterance, speaker, passage to check…): commands, options, their values, help, messages, statuses,
JSON keys and values, and the socket protocol. Meeting content (titles, summaries, transcripts,
speaker names, a failed stage's message as the sheet keeps it) is printed as is. The app's UI and
the files Greffier writes stay French: the core renders each refusal both ways
(`Rejection::english`), and the Greffier process relays the English one. Errors go to stderr as
`greffier: <message>`, with user input in plain double quotes.

## `search`

`greffier search recette vendredi` finds the lines that contain all the words, `greffier search
'"recette passe vendredi"'` the exact phrase (the arguments are joined; case, accents and the
typographic apostrophe are ignored). A word finds the words that contain it: `bookmark` finds
`bookmarks` but not the other way round, so an agent should type the stem. It searches the
titles, the summaries' lines and the utterances of the meetings started between `--since` and
`--until` (or on `--date`), most recent first; for each meeting, the title, then the summary's
lines, then the utterances. `--speaker` keeps the utterances of the speakers whose displayed name
contains it; `--in summary,transcription,title` (or repeated) where to search; `--context N` adds
the N utterances before and after each one.

Limits: 30 lines in all (`--limit`). Everything found is shown when it fits. Otherwise each
meeting shows as many summary lines and as many utterances as the limit allows for all the
meetings found, at least 5 of each, most recent meetings first: one talkative meeting does not
hide the others, and a search narrowed to one meeting (`--date`) shows up to `--limit` of its
lines. A long line is cut to 200 characters around the first word found. The last lines count
what the limits left out, by meeting (the 10 most recent named), and how to narrow down.

```
$ greffier search recette --in transcription --context 1
2026-11-20-1000 · Revue de release
    [0:00:00] Alexandre: Bonjour Khalid, on publie la release lundi ?
  [0:00:02] Khalid ?: Oui, si la recette passe vendredi.
    [0:00:05] Alexandre: Je préviens le support demain.
$ greffier search re --limit 4
2026-11-20-1000 · Revue de release
  title
  summary · On publie la release lundi si la recette passe vendredi.
  summary · SPEAKER_00 — Alexandre
  [0:00:00] summary · Date de la release
… 9 more results in 2 meetings: 2026-11-20-1000 (8), 2026-11-18-0915 (1)
Narrow down (--date, --since, --until, --speaker, --in) or raise --limit.
```

A summary line shows its moment (its own timestamp, or its topic's), without repeating it in the
text; a line of the brief or the actions has none. With `--context`, the utterances found and
those around them (indented, cut to 200 characters) follow each other in order, each once.
Nothing found: `No results for "…".`, exit code 0; for several words, the line adds that they
must all be on the same line (a summary line or an utterance) and to search fewer words, or one.
The command opens the core with adapters that refuse everything and reads the search cache
without writing it: entries missing or stale there are built in memory, and the meetings folder
and the app folder are left byte for byte as they were.

## `show`

Without a view, a meeting's header and its summary's outline, a few lines even for a long
meeting (about 3.5 KB for 2.5 h, against 20 KB for the whole summary), then where to read on:

```
$ greffier show 2026-11-20-1000
2026-11-20-1000 · Revue de release
2026-11-20 10:00 · 0:00:12 · done, 1 to check
Speakers: Alexandre, Khalid ?
Folder: /…/2026-11-20 10h00 - Revue de release
Brief: On publie la release lundi si la recette passe vendredi.
Topics:
  [0:00:00] Date de la release
  [0:00:05] Support
Decisions:
  [0:00:02] Release lundi si la recette passe vendredi.
Actions: 2
Next: --summary for the full summary, --transcription --from h:mm:ss --to h:mm:ss for what was said.
```

Views: `--summary` (the file without its front matter), `--sheet` (the file as is),
`--transcription [--from T] [--to T] [--numbers]` (`[h:mm:ss] Name: text`), `--speakers`
(`index · label · name · speaking time · share`), `--actions` (the summary's checkboxes,
numbered as `check-action` takes them), `--passages [--from T] [--to T]` (the passages to check,
numbered as `resolve` takes them).

Statuses read `recording`, `recording interrupted`, `to process`, `queued`, `failed: Review`,
`done`, `done, 2 to check`; with the running process's data, `2nd in queue · 15 min` or
`Transcription 40 % · 7 min`. Stages: `Transcription`, `Review`, `Summary`.

## Talking to the Greffier process

The socket is `<app folder>/greffier.sock` (0600), one JSON object per line, a response for each
request, several requests per connection. Requests: `"hello"`, `{"status":{"meeting":null}}`
(or a meeting, as typed), `{"execute":{<action>}}`, `"open_window"`. Actions, with meetings,
speakers, utterances, passages and actions as the user typed them (the process resolves them
against its live state, numbering as `show`): `process {meeting, speakers}`, `redo {meeting,
stage: transcription|review|summary}`, `stop`, `rename {meeting, title}`, `name_speaker
{meeting, speaker, name}`, `merge_speaker {meeting, speaker, into}`, `split_speaker`, `correct
{meeting, utterance, text}`, `resolve {meeting, passage, resolution: "accept"|"keep"|"reopen"|
{"correct": text}}`, `check_action {meeting, action, done}`, `delete {meeting}`, `import {files,
title, speakers, process}`. The process reads the meetings folder again before each status and
action, so that a meeting is named against the folder as `list` reads it. Responses:
`{"hello":{version, pid, meetings_folder, background}}`,
`{"status":{busy, meetings: [{identifier, folder_name, title, status, rank, time_remaining}]}}`,
`{"ok":{message, meeting, folder_name}}`, `{"error":{message}}`; messages are in English. The
protocol is at version 3 (English since then): the command line refuses a process that speaks
another one. Types in `src/protocol.rs`.

The command line refuses a process on another meetings folder. If nobody answers, it launches
`GREFFIER_SERVER_PROGRAM` (the tests' `greffier-test-server`), else the running binary outside
an app bundle, else the installed app (`open -g -j -b io.github.fredealexandre.greffier --args
--background`; with `GREFFIER_FOLDER` or `GREFFIER_APP_FOLDER` set, `open -n -g -j -b … --env
GREFFIER_FOLDER=… --env GREFFIER_APP_FOLDER=… --args --background`, a separate instance that keeps
the bundle's permissions), and waits up to 15 s: a process launched that exits before answering
(it found the lock of one still quitting) is launched again, three launches at most. `status`
never launches it. An app folder whose socket path would pass 103 bytes fails at once (exit 3).
The process has 30 s to answer `hello` and `status`, 2 min an action, 10 min an import (the
media is copied and converted first); past that, the command line says the action may still
complete, to be checked with `greffier list` or `greffier status`.

Writing prints one line: `2026-11-20-1000 · renamed: Review` (`import`: the new identifier).
With `--json`: `{"identifier", "folder", "message"}`. `--wait` follows the processing on
stderr, one line per stage, then prints `<identifier> · done · <title>` (`{"identifier",
"status", "title"}`).

## JSON (`--json`)

One line of JSON on stdout. Keys are the glossary's English terms, in snake case; the identifier
is always `identifier`, never `id`. Every key is always present; `null` when it does not apply.
Times are in seconds, with an `h:mm:ss` string next to them where it helps.

**Meeting** (`list` gives an array of them):

| key | |
|---|---|
| `identifier` | `2026-11-20-0939` |
| `folder` | absolute path of the meeting's folder |
| `title` | title |
| `date`, `start` | `2026-11-20`, `09:39` |
| `duration`, `duration_text` | seconds (integer), `2:34:22` |
| `status` | `recording`, `recording_interrupted`, `to_process`, `queued`, `running`, `failed`, `done` |
| `stage` | stage queued, running or failed: `transcription`, `review`, `summary` |
| `completed_stage` | last stage completed (same values) |
| `error` | failure message, as the sheet keeps it |
| `to_check` | number of passages to check still pending |

**`search`**: `results`, `total` (the lines found, without the limits) and `meetings` (the
meetings they come from). A result: `identifier`, `title` (its meeting's), `location` (`title`,
`summary`, `transcription`), `instant` (seconds) and `timestamp` (`null` for the title and a
summary line without a moment), `speaker` (displayed name, utterances only), `utterance` (the
utterance's number, from 1, as `show --transcription --numbers`), `excerpt` (the line, cut to
200 characters around the first word found, with `…`), `highlights` (the words and phrases found
in `excerpt`, `[start, end]` byte ranges), `before` and `after` (the utterances around, with
`--context`: `utterance`, `start`, `timestamp`, `speaker`, `text`, whole).

**`status`**: a meeting (or an array of the meetings not done) plus `rank` (place in the queue,
from 1), `progress` (0 to 1) and `time_remaining` (seconds), `null` while read from disk.

**`show <meeting>`**: a meeting plus `speakers` (see below) and the summary's outline, each
`null` until there is a summary: `brief` (the brief's first paragraph), `topics` (`start`,
`timestamp`, `title`), `decisions` (`start`, `timestamp`, `text`) and `actions` (how many).
Views, each with `identifier`:

- `--summary`: `summary`; `--sheet`: `sheet` (the file as is).
- `--transcription`: `utterances`: `number` (from 1), `start`, `end`, `timestamp`, `speaker`
  (displayed name: confirmed, else proposed "Khalid ?", else label), `label` (merges
  followed), `text`.
- `--speakers`: `speakers`: `index` (as in the label), `label`, `name` (confirmed), `proposal`,
  `displayed` (displayed name), `merged_into` (label), `speaking_time` (seconds; 0 if merged).
- `--actions`: `actions`: `number` (from 1, all the summary's checkboxes), `done`, `text`.
- `--passages`: `passages`: `number` (from 1), `utterance` (its utterance's number), `start`,
  `timestamp`, `excerpt`, `hypothesis`, `relisten`, `status` (`to_check`,
  `hypothesis_accepted`, `original_kept`, `corrected`), `correction`.
