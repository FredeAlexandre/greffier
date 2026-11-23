# greffier-cli

Greffier's command line for agents (ADR 0006), reading side: `greffier list`, `show`, `status`,
`help`. It reads the meetings folder through the core and never writes. `greffier help` gives
the usage. A meeting is named by its identifier (`2026-11-20-0939`), its folder name or path, a
date or the start of an identifier.

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
