# greffier-cli

Greffier's command line for agents (ADR 0006). `greffier search`, `list`, `show`, `status` read the
meetings folder (and the app's search cache) through the core and never write. `import`, `process`, `redo`, `stop`, `rename`, `speaker`,
`correct`, `resolve`, `check-action`, `delete` are sent to the Greffier process over its socket,
which is started without a window if it does not run. `greffier help` gives the usage. A meeting
is named by its identifier (`2026-11-20-0939`), its folder name or path, a date or the start of
an identifier. Any first argument that does not start with `-` is a command (a mistyped one fails with
exit code 2); `greffier` alone, or with the window's options (`--snap`, `--scene`…), opens the
window.

## `search`

`greffier search recette vendredi` finds the lines that contain all the words, `greffier search
'"recette passe vendredi"'` the exact phrase (the arguments are joined; case, accents and the
typographic apostrophe are ignored). A word finds the words that contain it: `bookmark` finds
`bookmarks` but not the other way round, so an agent should type the stem. It searches the
titles, the summaries' lines and the utterances of the meetings started between `--since` and
`--until` (or on `--date`), most recent first; for each meeting, the title, then the summary's
lines, then the utterances. `--speaker` keeps the
utterances of the speakers whose displayed name contains it; `--in bilan,transcription,titre`
(or repeated) where to search; `--context N` adds the N utterances before and after each one.

Limits: 30 lines in all (`--limit`). Everything found is shown when it fits. Otherwise each
meeting shows as many summary lines and as many utterances as the limit allows for all the
meetings found, at least 5 of each, most recent meetings first: one talkative meeting does not
hide the others, and a search narrowed to one meeting (`--date`) shows up to `--limit` of its
lines. A long line is cut to 200 characters around the first word found. The last lines count
what the limits left out, by meeting (the 10 most recent named), and how to narrow down.

```
$ greffier search recette --in transcription --context 1
2026-11-20-1000 · Revue de release
    [0:00:00] Alexandre : Bonjour Khalid, on publie la release lundi ?
  [0:00:02] Khalid ? : Oui, si la recette passe vendredi.
    [0:00:05] Alexandre : Je préviens le support demain.
$ greffier search re --limit 4
2026-11-20-1000 · Revue de release
  titre
  bilan · On publie la release lundi si la recette passe vendredi.
  bilan · SPEAKER_00 — Alexandre
  [0:00:00] bilan · Date de la release
… 9 autres résultats dans 2 réunions : 2026-11-20-1000 (8), 2026-11-18-0915 (1)
Affinez (--date, --since, --until, --speaker, --in) ou augmentez --limit.
```

A summary line shows its moment (its own timestamp, or its topic's), without repeating it in the
text; a line of the brief or the actions has none. With `--context`, the utterances found and
those around them (indented, cut to 200 characters) follow each other in order, each once.
Nothing found: `Aucun résultat pour « … ».`, exit code 0. The command opens the core with adapters that refuse everything and reads the
search cache without writing it: entries missing or stale there are built in memory, and the
meetings folder and the app folder are left byte for byte as they were.

## Talking to the Greffier process

The socket is `<app folder>/greffier.sock` (0600), one JSON object per line, a response for each
request, several requests per connection. Requests: `"hello"`, `{"status":{"meeting":null}}`
(or a meeting, as typed), `{"execute":{<action>}}`, `"open_window"`. Actions, with meetings,
speakers, utterances, passages and actions as the user typed them (the process resolves them
against its live state, numbering as `show`): `process {meeting, speakers}`, `redo {meeting,
stage: transcription|relecture|bilan}`, `stop`, `rename {meeting, title}`, `name_speaker
{meeting, speaker, name}`, `merge_speaker {meeting, speaker, into}`, `split_speaker`, `correct
{meeting, utterance, text}`, `resolve {meeting, passage, resolution: "accept"|"keep"|"reopen"|
{"correct": text}}`, `check_action {meeting, action, done}`, `delete {meeting}`, `import {files,
title, speakers, process}`. Responses: `{"hello":{version, pid, meetings_folder, background}}`,
`{"status":{busy, meetings: [{id, folder_name, title, status, rank, time_remaining}]}}`,
`{"ok":{message, meeting, folder_name}}`, `{"error":{message}}`. Types in `src/protocol.rs`.

The command line refuses a process on another meetings folder. If nobody answers, it launches
`GREFFIER_SERVER_PROGRAM` (the tests' `greffier-test-server`), else the running binary when
`GREFFIER_FOLDER` or `GREFFIER_APP_FOLDER` is set or outside an app bundle, else the installed
app (`open -g -j -b io.github.fredealexandre.greffier --args --background`), and waits up to
15 s. `status` never launches it.

Writing prints one line: `2026-11-20-1000 · renommée : Revue` (`import`: the new identifier).
With `--json`: `{"id", "dossier", "message"}`. `--wait` follows the processing on stderr, one
line per stage, then prints `<id> · terminée · <titre>` (`{"id", "statut", "titre"}`).

## JSON (`--json`)

One line of JSON on stdout. Keys mirror the sheet's vocabulary (French, no accents). Every key
is always present; `null` when it does not apply. Times are in seconds, with an `h:mm:ss`
string next to them where it helps.

**Meeting** (`list` gives an array of them):

| key | |
|---|---|
| `id` | identifier, `2026-11-20-0939` |
| `dossier` | absolute path of the meeting's folder |
| `titre` | title |
| `date`, `debut` | `2026-11-20`, `09:39` |
| `duree`, `duree_texte` | seconds (integer), `2:34:22` |
| `statut` | `enregistrement_en_cours`, `enregistrement_interrompu`, `a_traiter`, `en_attente`, `en_cours`, `echec`, `terminee` |
| `etape` | stage pending, running or failed: `transcription`, `relecture`, `bilan` |
| `etape_faite` | last stage completed (same values) |
| `erreur` | failure message |
| `a_verifier` | number of passages to check still pending |

**`search`**: `resultats`, `total` (the lines found, without the limits) and `reunions` (the
meetings they come from). A result: `id`, `titre` (its meeting's), `emplacement` (`titre`,
`bilan`, `transcription`), `debut` (seconds) and `horodatage` (`null` for the title and a summary
line without a moment), `locuteur` (displayed name, utterances only), `replique` (the utterance's
number, from 1, as `show --transcription --numeros`), `extrait` (the line, cut to 200 characters
around the first word found, with `…`), `surlignes` (the words and phrases found in `extrait`,
`[start, end]` byte ranges), `avant` and `apres` (the utterances around, with `--context`:
`replique`, `debut`, `horodatage`, `locuteur`, `texte`, whole).

**`status`**: a meeting (or an array of the meetings not done) plus `rang` (place in the queue,
from 1), `progression` (0 to 1) and `temps_restant` (seconds), `null` while read from disk.

**`show <réunion>`**: a meeting plus `locuteurs` (see below) and the summary's outline, each
`null` until there is a summary: `en_bref` (the brief's first paragraph), `sujets` (`debut`,
`horodatage`, `titre`), `decisions` (`debut`, `horodatage`, `texte`) and `actions` (how many).
The text view gives the same: a few lines even for a long meeting (about 3.5 KB for 2.5 h,
against 20 KB for the whole summary), then where to read on. Views, each with `id`:

- `--bilan`: `bilan`; `--fiche`: `fiche` (the file as is).
- `--transcription`: `repliques`: `numero` (from 1), `debut`, `fin`, `horodatage`, `locuteur`
  (displayed name: confirmed, else proposed « Khalid ? », else label), `label` (merges
  followed), `texte`.
- `--locuteurs`: `locuteurs`: `index` (as in the label), `label`, `nom` (confirmed), `proposition`,
  `affiche` (displayed name), `fusionne_dans` (label), `temps_de_parole` (seconds; 0 if merged).
- `--actions`: `actions`: `numero` (from 1, all the summary's checkboxes), `fait`, `texte`.
- `--passages`: `passages`: `numero` (from 1), `replique` (its utterance's number), `debut`,
  `horodatage`, `extrait`, `hypothese`, `reecoute`, `statut` (`a_verifier`,
  `hypothese_acceptee`, `original_garde`, `corrige`), `correction`.
