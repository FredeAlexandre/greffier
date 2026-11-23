---
name: greffier
description: Find what was said in the user's recorded meetings (réunions) with the `greffier` command line, and act on them as the Greffier app does. Use when the user asks what was said, decided or to be done in a meeting, refers to a meeting by day or topic ("la réunion de mardi", "on a parlé de X"), or wants a recording imported, processed, renamed or its speakers, transcription or actions corrected.
---

# Greffier

Greffier records the user's meetings and keeps each one as open files in their meetings folder: a summary (*bilan*: brief, timestamped topics, decisions, actions), a transcription split into timestamped utterances (*répliques*) by speaker (*locuteur*), and a sheet (*fiche*). `greffier help` gives the exact syntax of every command; read it once per session before the first command.

Everything Greffier prints is French; quote it as is.

## Finding what was said

1. **Locate.**
   - A meeting named by its day: `greffier show 2026-11-20` gives its outline (brief, timestamped topics, decisions); `greffier list --date 2026-11-20` first if that day has several.
   - A topic: `greffier search <words>` returns, per meeting (most recent first), the summary lines, then the utterances, each with its timestamp. Search the word stem (`bookmark`, not `bookmarks`), two or three distinctive words rather than a sentence; quote an exact phrase (`'"fiscal year"'`). On a broad topic, start with `--in bilan`; narrow with `--since`/`--date`/`--speaker` when the last line says results were left out, and look for the meeting whose title carries the topic.
   - Done when you hold a meeting identifier (`2026-11-20-0939`) and a timestamp for each part of the question.
2. **Read a window**, when the outline does not answer already. `greffier show <id> --transcription --from <t-2min> --to <t+3min>` around each timestamp found. A whole transcription runs to tens of thousands of tokens: always give `--from`/`--to`, and widen the window rather than drop it. A decision can sit in the brief rather than in the decisions list; `--bilan` gives the full summary. Done when the passage answers the question in the speakers' own words.
3. **Answer with sources.** Cite each fact with its meeting and timestamp: « Revue de release (2026-11-20-0939), 1:25:40, Jérôme : … ». Say when a speaker's name ends with ` ?`: it is the summary's guess, not a confirmed name.

## Acting on meetings

Writing commands (`import`, `process`, `redo`, `rename`, `speaker`, `correct`, `resolve`, `check-action`, `stop`, `delete`) are carried out by the Greffier app, which the command line starts without a window if it is not running; reading commands never start it.

- **Processing spends the user's Claude usage** (the review and the summary run `claude`) and lasts minutes to an hour: run `import`, `process` or `redo` only when the user asked for it.
- **Follow processing** with `greffier status <id>`. `--wait` blocks until the end, so give it a `--timeout` shorter than your command time limit and poll `status` after exit code 4.
- **Name things as `show` numbers them**: speakers from `show <id> --locuteurs`, utterances from `show <id> --transcription --numeros`, actions from `--actions`, passages from `--passages`. Read the view right before changing it.
- **`delete`** moves the meeting to the Mac trash: run it only on the user's explicit request, then with `--yes`.

Exit codes: 0 done, 1 refused (the message says why), 2 wrong usage (check `greffier help`), 3 Greffier unreachable, 4 `--timeout` elapsed while processing goes on.
