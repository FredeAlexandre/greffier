# greffier-cli

Greffier's command line for agents (ADR 0006), reading side: `greffier search`, `list`, `show`,
`status`, `help`. It reads the meetings folder (and the app's search cache) through the core and
never writes. `greffier help` gives the usage. A meeting is named by its identifier
(`2026-11-20-0939`), its folder name or path, a date or the start of an identifier. Any first
argument that does not start with `-` is a command (a mistyped one fails with exit code 2);
`greffier` alone, or with the window's options (`--snap`, `--scene`…), opens the window.

## `search`

`greffier search fiscal year` finds the lines that contain all the words, `greffier search
'"fiscal year"'` the exact phrase (the arguments are joined; case, accents and the typographic
apostrophe are ignored). It searches the titles, the summaries' lines and the utterances of the
meetings started between `--since` and `--until` (or on `--date`), most recent first; for each
meeting, the title, then the summary's lines, then the utterances. `--speaker` keeps the
utterances of the speakers whose displayed name contains it; `--in bilan,transcription,titre`
(or repeated) where to search; `--context N` adds the N utterances before and after each one.

Limits: 30 lines in all (`--limit`). Everything found is shown when it fits. Otherwise each
meeting shows as many summary lines and as many utterances as the limit allows for all the
meetings found, at least 5 of each, most recent meetings first: one talkative meeting does not
hide the others, and a search narrowed to one meeting (`--date`) shows up to `--limit` of its
lines. A long line is cut to 200 characters around the first word found. The last lines count
what the limits left out, by meeting (the 10 most recent named), and how to narrow down.

```
2026-11-20-0939 · Revue de release avec Jérôme : permissions et dataset dynamique, fiscal year, bookmarks
  [1:31:33] bilan · Badge Alpha sur le filtre fiscal year. Objectif de 95 % de couverture…
  [1:25:32] Khalid ? : …on a ajouté le filtre fiscal year…
    [1:25:40] Jérôme ? : (context, with --context)
… 12 autres résultats dans 3 réunions : 2026-11-20-0939 (4), 2026-11-18-0934 (6), 2026-11-17-0932 (2)
Affinez (--date, --since, --until, --speaker, --in) ou augmentez --limit.
```

A summary line shows its moment (its own timestamp, or its topic's), without repeating it in the
text; a line of the brief or the actions has none. Nothing found: `Aucun résultat pour « … ».`,
exit code 0. The command opens the core with adapters that refuse everything and reads the
search cache without writing it: entries missing or stale there are built in memory, and the
meetings folder and the app folder are left byte for byte as they were.

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
