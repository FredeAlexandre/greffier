# Meetings are open files in a meetings folder

Greffier has no database of its own: each meeting is a subfolder of readable files (Markdown, JSON, video) in a meetings folder chosen in the settings, so that the user and their AI agents can read and use them without going through the app. We rejected an opaque library with export, which would have cut the agents off from their source.

The format of these files is designed for Greffier, not inherited from the Obsidian vault that preceded it: no compatibility is owed to the old tool, which only served as an experiment. Old meetings come in through import, like any external recording.
