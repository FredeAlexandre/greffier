# Greffier

Greffier records work meetings on the Mac, transcribes them, reviews them and writes their summary, so that they can be revisited and what was said, decided and to be done can be found again.

The app's UI and the files it writes stay in French: each term gives its French UI label. The command line speaks English and uses the English terms below (options, messages, JSON keys).

## Meeting and files

**Meeting** (UI: *Réunion*):
A recorded conversation, with everything derived from it: recording, transcription, review, summary.
_Avoid_: session, call

**Meetings folder** (UI: *Dossier de réunions*):
The folder chosen by the user where Greffier keeps all the meetings, one subfolder per meeting, as files readable outside the app.
_Avoid_: library, vault, database

**Sheet** (UI: *Fiche*):
A meeting's entry file: its properties (date, duration, status…), its brief, its main actions and the links to the other files.
_Avoid_: meeting note, page

**Identifier** (command line: `identifier`):
A meeting's short, stable name for the command line and agents, taken from its folder's date and time (`2026-11-20-0939`), with `-2`, `-3`… when several meetings start in the same minute.
_Avoid_: id, slug, key

**Title** (UI: *Titre*):
A meeting's one-line summary ("Revue de release"), proposed by the summary and editable.
_Avoid_: subject, name

**Recording** (UI: *Enregistrement*):
The video and audio capture of a meeting (screen, Mac audio, microphone).
_Avoid_: video (when talking about the complete file)

**Marker** (UI: *Repère*):
An instant marked by the user during the recording, to find it again later on the timeline.
_Avoid_: bookmark

**Import** (UI: *Importer*):
Adding a meeting from existing files (video or audio only, with or without a transcription, with or without a summary); processing resumes at the first missing stage.
_Avoid_: migration

## Processing

**Processing** (UI: *Traitement*):
The chain of stages that turns a recording into a reviewed transcription and a summary.
_Avoid_: pipeline, job

**Stage** (UI: *Étape*):
A link in the processing: Transcription, Review or Summary.

**Transcription** (UI: *Transcription*):
The meeting's text, split into timestamped utterances attributed to speakers; also the stage that produces it.
_Avoid_: subtitles, verbatim

**Utterance** (UI: *Réplique*):
A continuous passage from a single speaker in the transcription, with its start and end timestamps.
_Avoid_: segment, line

**Speaker** (UI: *Locuteur*):
A distinct voice detected in a meeting (SPEAKER_00, SPEAKER_01…), which can be given a name. Two speakers can turn out to be the same voice and be merged.
_Avoid_: person, participant

**Speaker count** (UI: *Nombre de locuteurs*):
The number of expected voices, given before processing to diarize the speakers better; "détecté" (detected) if it is not given.
_Avoid_: people, participants

**Lexicon** (UI: *Lexique*):
The list of proper names, trade words and known transcription errors with their correct form, used by the transcription and the review and enriched at each meeting.
_Avoid_: dictionary, glossary

## Review and correction

**Review** (UI: *Relecture*, *Relecture IA*):
The automatic stage where an AI fixes the certain errors in the transcription, changes speakers when the conversation proves it and flags doubtful passages.
_Avoid_: AI review (except in the UI), revision, proofreading

**Relisten** (UI: *Réécoute*):
The re-transcription of a doubtful passage, isolated from the recording, so that the review can settle it.

**Passage to check** (UI: *À vérifier*):
A passage the review could not settle, even after relistening, and that the user must listen to.
_Avoid_: doubt, highlighted passage

**Correction** (UI: *Correction*):
A change made by hand by the user to the transcription or the speakers.
_Avoid_: review, edit

## Summary

**Summary** (UI: *Bilan*):
The written synthesis of a meeting: brief, named speakers, topics covered, decisions, actions and open questions, with timestamps.
_Avoid_: recap, minutes, notes

**Topic** (UI: *Sujet*):
A point covered during the meeting, timestamped at its start in the summary; the topics form the timeline's chapters.
_Avoid_: theme, chapter, title

**Decision** (UI: *Décision*):
A choice agreed on during the meeting, recorded and timestamped in the summary.

**Action** (UI: *Action*):
A thing to do coming out of the meeting, in the form "who — what — deadline", which can be checked off in the summary.
_Avoid_: task, todo
